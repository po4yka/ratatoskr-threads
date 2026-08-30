//! Threads' narrow `JetStream` boundary.
//!
//! This service pulls only its provider-specific command subject and emits
//! only event facts already durably written to its transactional outbox.

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use async_nats::jetstream;
use futures_util::StreamExt as _;
use ratatoskr_event_envelope::EventEnvelope;
use sqlx::PgPool;

use crate::browser_capture_command::BrowserCaptureCommand;
use crate::browser_capture_inbox::BrowserCaptureInbox;
use crate::telemetry::{
    OutboxFailureClass, record_outbox_depth, record_outbox_failure, record_outbox_redelivery,
};
use crate::{Database, PersistenceError};

/// The fleet-owned command stream declared by Platform.
pub const COMMAND_STREAM: &str = "ratatoskr_commands";
/// The sole command subject this service may consume.
pub const COMMAND_SUBJECT: &str = "cmd.threads.capture.requested.v1";
/// Durable cursor name for this provider command handler.
pub const CONSUMER_NAME: &str = "threads_browser_capture";
const OUTBOX_BATCH_SIZE: i64 = 32;
const OUTBOX_POLL_INTERVAL: Duration = Duration::from_secs(1);
const OPERATION_REPORT_SUBJECT: &str = "evt.platform.operation.reported.v1";
const SOCIAL_CAPTURED_SUBJECT: &str = "evt.social.source.captured.v1";
const SOCIAL_REMOVED_SUBJECT: &str = "evt.social.source.removed.v1";
const SOCIAL_UPDATED_SUBJECT: &str = "evt.social.source.updated.v1";
const MAX_OUTBOX_ATTEMPTS: i32 = 12;
const MAX_RETRY_DELAY_SECONDS: i32 = 300;

/// A connected NATS client retained beside its `JetStream` context.
#[derive(Debug, Clone)]
pub struct NatsConnection {
    client: async_nats::Client,
    context: jetstream::Context,
}

impl NatsConnection {
    /// Connects without a credential for a local development broker.
    ///
    /// Production deployments configure an nkey path and use
    /// [`Self::connect_with_nkey`] instead.
    ///
    /// # Errors
    ///
    /// Returns [`NatsError`] when NATS refuses the connection.
    pub async fn connect(url: &str) -> Result<Self, NatsError> {
        let client = async_nats::connect(url)
            .await
            .map_err(|error| NatsError::Bus(error.to_string()))?;
        Ok(Self {
            context: jetstream::new(client.clone()),
            client,
        })
    }

    /// Connects using the service nkey at `seed_path`.
    ///
    /// # Errors
    ///
    /// Returns [`NatsError`] when the credential cannot be read or NATS
    /// refuses the connection.
    pub async fn connect_with_nkey(url: &str, seed_path: &Path) -> Result<Self, NatsError> {
        let seed = std::fs::read_to_string(seed_path).map_err(NatsError::Credential)?;
        let client = async_nats::ConnectOptions::with_nkey(seed.trim().to_owned())
            .connect(url)
            .await
            .map_err(|error| NatsError::Bus(error.to_string()))?;
        Ok(Self {
            context: jetstream::new(client.clone()),
            client,
        })
    }

    /// Whether the connection is currently usable without a network probe.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        matches!(
            self.client.connection_state(),
            async_nats::connection::State::Connected
        )
    }
}

/// NATS or outbox processing failed.
#[derive(Debug, thiserror::Error)]
pub enum NatsError {
    /// The configured nkey seed could not be read.
    #[error("the NATS credential could not be read")]
    Credential(#[source] std::io::Error),
    /// NATS rejected or failed a protocol operation.
    #[error("the NATS operation failed")]
    Bus(String),
    /// An owned persistence operation failed.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    /// An outbox row requested a subject outside this service's NATS grant.
    #[error("the outbox event type is not permitted for Threads publication")]
    UnexpectedOutboxEventType,
    /// The stored envelope identity or type disagrees with its outbox row.
    #[error("the stored outbox envelope does not match its row identity")]
    InvalidOutboxEnvelope,
}

/// One stored outbox envelope ready for an acknowledged transport boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxPublication {
    event_id: uuid::Uuid,
    payload: Vec<u8>,
    subject: &'static str,
}

impl OutboxPublication {
    /// The stable at-least-once message identity used as `Nats-Msg-Id`.
    #[must_use]
    pub fn event_id(&self) -> uuid::Uuid {
        self.event_id
    }

    /// The serialized envelope loaded from the outbox row.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// The closed NATS subject selected from the stored event type.
    #[must_use]
    pub fn subject(&self) -> &'static str {
        self.subject
    }
}

/// A transport that completes only after the broker acknowledges a publication.
pub trait OutboxTransport {
    /// Publish one stored envelope and await its acknowledgement.
    fn publish(
        &mut self,
        publication: OutboxPublication,
    ) -> impl Future<Output = Result<(), NatsError>> + Send;
}

/// Processes one bounded outbox pass through a controllable acknowledged transport.
///
/// # Errors
///
/// Returns [`NatsError`] when outbox persistence cannot record a truthful result.
pub async fn publish_outbox_pass<T>(transport: &mut T, pool: &PgPool) -> Result<(), NatsError>
where
    T: OutboxTransport,
{
    let rows: Vec<(uuid::Uuid, String, serde_json::Value, i32)> = sqlx::query_as(
        "select event_id, event_type, payload, attempt_count \
         from threads_archive.outbox_events \
         where published_at is null and dead_lettered_at is null \
           and coalesce(next_attempt_at, occurred_at) <= now() \
         order by coalesce(next_attempt_at, occurred_at), occurred_at, event_id limit $1",
    )
    .bind(OUTBOX_BATCH_SIZE)
    .fetch_all(pool)
    .await
    .map_err(PersistenceError::Query)?;
    for (event_id, event_type, payload, attempt_count) in rows {
        let Some(subject) = outbox_subject(&event_type) else {
            mark_terminal_failure(pool, event_id, OutboxFailureClass::UnsupportedEventType).await?;
            record_outbox_failure(OutboxFailureClass::UnsupportedEventType, true);
            continue;
        };
        let Ok(payload) = serde_json::to_vec(&payload) else {
            mark_terminal_failure(pool, event_id, OutboxFailureClass::PayloadEncodingFailed)
                .await?;
            record_outbox_failure(OutboxFailureClass::PayloadEncodingFailed, true);
            continue;
        };
        let Ok(envelope) = EventEnvelope::from_json(&payload) else {
            mark_terminal_failure(pool, event_id, OutboxFailureClass::InvalidOutboxEnvelope)
                .await?;
            record_outbox_failure(OutboxFailureClass::InvalidOutboxEnvelope, true);
            continue;
        };
        if envelope.event_id.0 != event_id || envelope.event_type.to_wire() != event_type {
            mark_terminal_failure(pool, event_id, OutboxFailureClass::InvalidOutboxEnvelope)
                .await?;
            record_outbox_failure(OutboxFailureClass::InvalidOutboxEnvelope, true);
            continue;
        }
        if transport
            .publish(OutboxPublication {
                event_id,
                payload,
                subject,
            })
            .await
            .is_err()
        {
            let terminal = mark_transport_failure(pool, event_id, attempt_count).await?;
            record_outbox_failure(OutboxFailureClass::BrokerUnacknowledged, terminal);
            continue;
        }
        sqlx::query(
            "update threads_archive.outbox_events \
             set published_at = now(), next_attempt_at = null, last_error = null \
             where event_id = $1 and published_at is null and dead_lettered_at is null",
        )
        .bind(event_id)
        .execute(pool)
        .await
        .map_err(PersistenceError::Query)?;
        if attempt_count > 0 {
            record_outbox_redelivery();
        }
    }
    observe_outbox_depth(pool).await?;
    Ok(())
}

async fn mark_transport_failure(
    pool: &PgPool,
    event_id: uuid::Uuid,
    previous_attempt_count: i32,
) -> Result<bool, NatsError> {
    let attempt_count = previous_attempt_count + 1;
    let terminal = attempt_count >= MAX_OUTBOX_ATTEMPTS;
    let retry_delay_seconds = retry_delay_seconds(attempt_count);
    sqlx::query(
        "update threads_archive.outbox_events \
         set attempt_count = $2, \
             next_attempt_at = case when $3 then null \
                                    else now() + ($4::integer * interval '1 second') end, \
             dead_lettered_at = case when $3 then now() else null end, \
             last_error = $5 \
         where event_id = $1 and published_at is null and dead_lettered_at is null",
    )
    .bind(event_id)
    .bind(attempt_count)
    .bind(terminal)
    .bind(retry_delay_seconds)
    .bind(OutboxFailureClass::BrokerUnacknowledged.as_str())
    .execute(pool)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(terminal)
}

fn retry_delay_seconds(attempt_count: i32) -> i32 {
    let exponent = u32::try_from(attempt_count.saturating_sub(1)).unwrap_or(u32::MAX);
    2_i32
        .checked_pow(exponent)
        .unwrap_or(MAX_RETRY_DELAY_SECONDS)
        .min(MAX_RETRY_DELAY_SECONDS)
}

async fn mark_terminal_failure(
    pool: &PgPool,
    event_id: uuid::Uuid,
    failure_class: OutboxFailureClass,
) -> Result<(), NatsError> {
    sqlx::query(
        "update threads_archive.outbox_events \
         set attempt_count = attempt_count + 1, dead_lettered_at = now(), \
             next_attempt_at = null, last_error = $2 \
         where event_id = $1 and published_at is null and dead_lettered_at is null",
    )
    .bind(event_id)
    .bind(failure_class.as_str())
    .execute(pool)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

async fn observe_outbox_depth(pool: &PgPool) -> Result<(), NatsError> {
    let (pending, dead_lettered): (i64, i64) = sqlx::query_as(
        "select count(*) filter (where published_at is null and dead_lettered_at is null), \
                count(*) filter (where dead_lettered_at is not null) \
         from threads_archive.outbox_events",
    )
    .fetch_one(pool)
    .await
    .map_err(PersistenceError::Query)?;
    record_outbox_depth(pending, dead_lettered);
    Ok(())
}

struct JetStreamOutboxTransport<'a> {
    context: &'a jetstream::Context,
}

impl OutboxTransport for JetStreamOutboxTransport<'_> {
    async fn publish(&mut self, publication: OutboxPublication) -> Result<(), NatsError> {
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("Nats-Msg-Id", publication.event_id.to_string());
        let acknowledgement = self
            .context
            .publish_with_headers(publication.subject, headers, publication.payload.into())
            .await
            .map_err(|error| NatsError::Bus(error.to_string()))?;
        acknowledgement
            .await
            .map_err(|error| NatsError::Bus(error.to_string()))?;
        Ok(())
    }
}

/// Runs the durable Threads command consumer until `stop` resolves.
///
/// A capture is acknowledged only after its inbox/capture/outbox transaction
/// committed and every pending outbox fact was acknowledged by `JetStream`.
/// Malformed or wrong-provider commands are acknowledged after a warning: a
/// broker redelivery cannot repair a bytestring this build refuses.
///
/// # Errors
///
/// Returns [`NatsError`] when the broker, owned persistence, or event
/// publication cannot progress. The caller should restart the worker; the
/// durable consumer and inbox make that safe.
pub async fn run(
    connection: &NatsConnection,
    database: &Database,
    stop: impl Future<Output = ()> + Send,
) -> Result<(), NatsError> {
    let consumer = open_consumer(&connection.context).await?;
    let mut messages = consumer
        .messages()
        .await
        .map_err(|error| NatsError::Bus(error.to_string()))?;
    let inbox = BrowserCaptureInbox::new(database);
    let mut outbox_ticker = tokio::time::interval(OUTBOX_POLL_INTERVAL);
    outbox_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tokio::pin!(stop);

    flush_outbox(&connection.context, database.pool()).await?;
    loop {
        tokio::select! {
            biased;
            () = &mut stop => return Ok(()),
            _ = outbox_ticker.tick() => flush_outbox(&connection.context, database.pool()).await?,
            message = messages.next() => {
                let Some(message) = message else {
                    return Err(NatsError::Bus("the durable command stream ended".to_owned()));
                };
                let message = message.map_err(|error| NatsError::Bus(error.to_string()))?;
                let command = match BrowserCaptureCommand::from_json(&message.payload) {
                    Ok(command) => command,
                    Err(error) => {
                        tracing::warn!(%error, "rejecting malformed or non-Threads browser capture command");
                        message.ack().await.map_err(|ack| NatsError::Bus(ack.to_string()))?;
                        continue;
                    }
                };
                inbox.deliver(&command).await.map_err(|error| match error {
                    crate::browser_capture_inbox::BrowserCaptureInboxError::Persistence(error) => NatsError::Persistence(error),
                    other => NatsError::Bus(other.to_string()),
                })?;
                flush_outbox(&connection.context, database.pool()).await?;
                message.ack().await.map_err(|ack| NatsError::Bus(ack.to_string()))?;
            }
        }
    }
}

/// Verifies the command stream and durable consumer before readiness flips.
///
/// # Errors
///
/// Returns [`NatsError`] when the deployed service identity cannot reach the
/// fleet command stream, its preprovisioned durable cursor, or its exact
/// provider-specific filter.
pub async fn ensure_command_consumer(connection: &NatsConnection) -> Result<(), NatsError> {
    let _consumer = open_consumer(&connection.context).await?;
    Ok(())
}

async fn open_consumer(
    context: &jetstream::Context,
) -> Result<jetstream::consumer::Consumer<jetstream::consumer::pull::Config>, NatsError> {
    let consumer = context
        .get_consumer_from_stream::<jetstream::consumer::pull::Config, _, _>(
            CONSUMER_NAME,
            COMMAND_STREAM,
        )
        .await
        .map_err(|error| NatsError::Bus(error.to_string()))?;
    let info = consumer.cached_info();
    validate_preprovisioned_consumer(&info.stream_name, &info.name, &info.config)?;
    Ok(consumer)
}

fn validate_preprovisioned_consumer(
    stream_name: &str,
    consumer_name: &str,
    config: &jetstream::consumer::Config,
) -> Result<(), NatsError> {
    if stream_name != COMMAND_STREAM
        || consumer_name != CONSUMER_NAME
        || config.durable_name.as_deref() != Some(CONSUMER_NAME)
    {
        return Err(NatsError::Bus(
            "the preprovisioned consumer has the wrong durable identity".to_owned(),
        ));
    }
    if config.filter_subject != COMMAND_SUBJECT {
        return Err(NatsError::Bus(
            "the preprovisioned consumer has the wrong subject filter".to_owned(),
        ));
    }
    if config.deliver_subject.is_some()
        || config.ack_policy != async_nats::jetstream::consumer::AckPolicy::Explicit
    {
        return Err(NatsError::Bus(
            "the preprovisioned consumer is not an explicit-ack pull consumer".to_owned(),
        ));
    }
    Ok(())
}

async fn flush_outbox(context: &jetstream::Context, pool: &PgPool) -> Result<(), NatsError> {
    let mut transport = JetStreamOutboxTransport { context };
    publish_outbox_pass(&mut transport, pool).await
}

fn outbox_subject(event_type: &str) -> Option<&'static str> {
    match event_type {
        "platform.operation.reported.v1" => Some(OPERATION_REPORT_SUBJECT),
        "social.source.captured.v1" => Some(SOCIAL_CAPTURED_SUBJECT),
        "social.source.removed.v1" => Some(SOCIAL_REMOVED_SUBJECT),
        "social.source.updated.v1" => Some(SOCIAL_UPDATED_SUBJECT),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COMMAND_STREAM, COMMAND_SUBJECT, CONSUMER_NAME, outbox_subject,
        validate_preprovisioned_consumer,
    };
    use async_nats::jetstream::consumer::{AckPolicy, Config};

    fn pull_config() -> Config {
        Config {
            durable_name: Some(CONSUMER_NAME.to_owned()),
            filter_subject: COMMAND_SUBJECT.to_owned(),
            ack_policy: AckPolicy::Explicit,
            ..Config::default()
        }
    }

    #[test]
    fn outbox_publication_is_limited_to_the_threads_acl() {
        assert_eq!(
            outbox_subject("platform.operation.reported.v1"),
            Some("evt.platform.operation.reported.v1")
        );
        assert_eq!(
            outbox_subject("social.source.captured.v1"),
            Some("evt.social.source.captured.v1")
        );
        assert_eq!(
            outbox_subject("social.source.updated.v1"),
            Some("evt.social.source.updated.v1")
        );
        assert_eq!(
            outbox_subject("social.source.removed.v1"),
            Some("evt.social.source.removed.v1")
        );
    }

    #[test]
    fn preprovisioned_consumer_must_be_the_exact_explicit_ack_pull_durable() {
        assert!(
            validate_preprovisioned_consumer(COMMAND_STREAM, CONSUMER_NAME, &pull_config()).is_ok()
        );

        let mut push = pull_config();
        push.deliver_subject = Some("deliver.somewhere.else".to_owned());
        assert!(validate_preprovisioned_consumer(COMMAND_STREAM, CONSUMER_NAME, &push).is_err());

        let mut wrong_ack = pull_config();
        wrong_ack.ack_policy = AckPolicy::All;
        assert!(
            validate_preprovisioned_consumer(COMMAND_STREAM, CONSUMER_NAME, &wrong_ack).is_err()
        );

        let mut wrong_filter = pull_config();
        wrong_filter.filter_subject = "cmd.x.capture.requested.v1".to_owned();
        assert!(
            validate_preprovisioned_consumer(COMMAND_STREAM, CONSUMER_NAME, &wrong_filter).is_err()
        );
    }
}
