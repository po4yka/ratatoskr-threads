//! Threads' narrow `JetStream` boundary.
//!
//! This service pulls only its provider-specific command subject and emits
//! only event facts already durably written to its transactional outbox.

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use async_nats::jetstream;
use futures_util::StreamExt as _;
use sqlx::PgPool;

use crate::browser_capture_command::BrowserCaptureCommand;
use crate::browser_capture_inbox::BrowserCaptureInbox;
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
const SOCIAL_UPDATED_SUBJECT: &str = "evt.social.source.updated.v1";

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
    let rows: Vec<(uuid::Uuid, String, serde_json::Value)> = sqlx::query_as(
        "select event_id, event_type, payload from threads_archive.outbox_events \
         where published_at is null order by occurred_at, event_id limit $1",
    )
    .bind(OUTBOX_BATCH_SIZE)
    .fetch_all(pool)
    .await
    .map_err(PersistenceError::Query)?;
    for (event_id, event_type, payload) in rows {
        let subject = outbox_subject(&event_type).ok_or(NatsError::UnexpectedOutboxEventType)?;
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("Nats-Msg-Id", event_id.to_string());
        let acknowledgement = context
            .publish_with_headers(
                subject,
                headers,
                serde_json::to_vec(&payload)
                    .map_err(|error| NatsError::Bus(error.to_string()))?
                    .into(),
            )
            .await
            .map_err(|error| NatsError::Bus(error.to_string()))?;
        acknowledgement
            .await
            .map_err(|error| NatsError::Bus(error.to_string()))?;
        sqlx::query(
            "update threads_archive.outbox_events set published_at = now() where event_id = $1",
        )
        .bind(event_id)
        .execute(pool)
        .await
        .map_err(PersistenceError::Query)?;
    }
    Ok(())
}

fn outbox_subject(event_type: &str) -> Option<&'static str> {
    match event_type {
        "platform.operation.reported.v1" => Some(OPERATION_REPORT_SUBJECT),
        "social.source.captured.v1" => Some(SOCIAL_CAPTURED_SUBJECT),
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
        assert_eq!(outbox_subject("social.source.removed.v1"), None);
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
