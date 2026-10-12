//! The outbox pump: publishes unpublished rows, then publishes unacknowledged removals again.
//!
//! The pump is generic over [`EventTransport`] so the state machine is exercised against
//! `PostgreSQL` with a fake transport, while the production transport is the `JetStream` context.

use std::future::Future;

use async_nats::jetstream;
use sqlx::PgPool;
use uuid::Uuid;

use crate::PersistenceError;
use crate::nats::NatsError;
use crate::removal_acknowledgement::{self as removal, ResendPolicy};

const OUTBOX_BATCH_SIZE: i64 = 32;
const OPERATION_REPORT_SUBJECT: &str = "evt.platform.operation.reported.v1";
const SOCIAL_CAPTURED_SUBJECT: &str = "evt.social.source.captured.v1";
const SOCIAL_UPDATED_SUBJECT: &str = "evt.social.source.updated.v1";
const SOCIAL_REMOVED_SUBJECT: &str = "evt.social.source.removed.v1";

/// A publication the transport refused, with a content-free class for `last_error`.
#[derive(Debug)]
pub struct PublishFailure {
    /// The closed failure class stored in `outbox_events.last_error`.
    pub class: &'static str,
    /// The error surfaced to the worker.
    pub error: NatsError,
}

/// Publishes one outbox fact and waits until the broker has stored it.
pub trait EventTransport: Send + Sync {
    /// Publishes `body` to `subject` with `event_id` as the `Nats-Msg-Id`.
    fn publish(
        &self,
        subject: &'static str,
        event_id: Uuid,
        body: Vec<u8>,
    ) -> impl Future<Output = Result<(), PublishFailure>> + Send;
}

impl EventTransport for jetstream::Context {
    async fn publish(
        &self,
        subject: &'static str,
        event_id: Uuid,
        body: Vec<u8>,
    ) -> Result<(), PublishFailure> {
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("Nats-Msg-Id", event_id.to_string());
        let acknowledgement = self
            .publish_with_headers(subject, headers, body.into())
            .await
            .map_err(|error| PublishFailure {
                class: "publish_failed",
                error: NatsError::Bus(error.to_string()),
            })?;
        acknowledgement.await.map_err(|_| PublishFailure {
            class: "ack_failed",
            error: NatsError::Unacknowledged,
        })?;
        Ok(())
    }
}

/// Publishes the unpublished outbox rows that are due, oldest first, then the unacknowledged
/// removals that are due for another copy.
///
/// A row that fails to publish stays unpublished, counts the attempt, records a content-free
/// failure class and backs off exponentially (2 s, 4 s, ... capped at 5 min), so it cannot starve
/// the rows behind it. The pass still reaches those rows, then surfaces the first failure so the
/// worker stops and the process is restarted rather than staying ready over a broken relay. A row
/// whose event type has no subject is a programming error, which the table CHECK makes
/// unstorable: it stops the worker at once. A republish that fails is backed off the same way and
/// leaves `published_at` and the resend schedule alone; the resend never runs in a pass whose
/// fresh rows failed, because a broker that refused one publication will refuse the next.
///
/// # Errors
///
/// [`NatsError`] when a publication fails or the database cannot be read or written.
pub async fn flush_outbox<T: EventTransport>(
    transport: &T,
    pool: &PgPool,
    policy: ResendPolicy,
) -> Result<(), NatsError> {
    let rows: Vec<(Uuid, String, serde_json::Value)> = sqlx::query_as(
        "select event_id, event_type, payload from threads_archive.outbox_events \
         where published_at is null and (next_attempt_at is null or next_attempt_at <= now()) \
         order by occurred_at, event_id limit $1",
    )
    .bind(OUTBOX_BATCH_SIZE)
    .fetch_all(pool)
    .await
    .map_err(PersistenceError::Query)?;
    let mut first_failure = None;
    for (event_id, event_type, payload) in rows {
        let subject = outbox_subject(&event_type).ok_or(NatsError::UnexpectedOutboxEventType)?;
        match publish_row(transport, subject, event_id, &payload).await {
            Ok(()) => removal::mark_published(pool, event_id, policy).await?,
            Err(failure) => {
                record_failure(pool, event_id, failure.class).await?;
                first_failure.get_or_insert(failure.error);
            }
        }
    }
    if first_failure.is_none() {
        first_failure = resend_unacknowledged(transport, pool, policy).await?;
    }
    refresh_tombstone_gauges(pool).await?;
    first_failure.map_or(Ok(()), Err)
}

/// Publishes the stored bytes of every due, unacknowledged removal again and schedules the next
/// copy. Stops at the first refusal and returns it.
async fn resend_unacknowledged<T: EventTransport>(
    transport: &T,
    pool: &PgPool,
    policy: ResendPolicy,
) -> Result<Option<NatsError>, NatsError> {
    for due in removal::due_resends(pool, OUTBOX_BATCH_SIZE).await? {
        match publish_row(
            transport,
            SOCIAL_REMOVED_SUBJECT,
            due.event_id,
            &due.payload,
        )
        .await
        {
            Ok(()) => removal::record_resend(pool, due.event_id, policy).await?,
            Err(failure) => {
                tracing::warn!(
                    class = failure.class,
                    event_id = %due.event_id,
                    "the outbox pump could not publish a removal again; it is backed off"
                );
                record_failure(pool, due.event_id, failure.class).await?;
                return Ok(Some(failure.error));
            }
        }
    }
    Ok(None)
}

/// Publishes the two gauges that tell an operator Knowledge has not answered. They never feed
/// readiness: a Knowledge outage must not make Threads unready.
async fn refresh_tombstone_gauges(pool: &PgPool) -> Result<(), NatsError> {
    let waiting = removal::unacknowledged_removals(pool).await?;
    let count = u32::try_from(waiting.count).map_or(f64::from(u32::MAX), f64::from);
    metrics::gauge!("threads_unacknowledged_tombstones").set(count);
    metrics::gauge!("threads_oldest_unacknowledged_tombstone_age_seconds")
        .set(waiting.oldest_age_seconds);
    Ok(())
}

/// Counts the failed attempt, names its class and backs the row off on `next_attempt_at`.
async fn record_failure(
    pool: &PgPool,
    event_id: Uuid,
    failure_class: &'static str,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "update threads_archive.outbox_events \
         set attempt_count = attempt_count + 1, last_error = $2, \
             next_attempt_at = now() + make_interval(\
                 secs => least(power(2, attempt_count), 300)::float8) \
         where event_id = $1",
    )
    .bind(event_id)
    .bind(failure_class)
    .execute(pool)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

/// Publishes one row and waits for its `PubAck`, naming the failure class on error.
async fn publish_row<T: EventTransport>(
    transport: &T,
    subject: &'static str,
    event_id: Uuid,
    payload: &serde_json::Value,
) -> Result<(), PublishFailure> {
    let body = serde_json::to_vec(payload).map_err(|error| PublishFailure {
        class: "serialization_failed",
        error: NatsError::Bus(error.to_string()),
    })?;
    transport.publish(subject, event_id, body).await
}

pub(crate) fn outbox_subject(event_type: &str) -> Option<&'static str> {
    match event_type {
        "platform.operation.reported.v1" => Some(OPERATION_REPORT_SUBJECT),
        "social.source.captured.v1" => Some(SOCIAL_CAPTURED_SUBJECT),
        "social.source.updated.v1" => Some(SOCIAL_UPDATED_SUBJECT),
        removal::REMOVAL_EVENT_TYPE => Some(SOCIAL_REMOVED_SUBJECT),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::outbox_subject;

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
        assert_eq!(outbox_subject("social.source.deleted.v1"), None);
    }
}
