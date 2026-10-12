//! The state machine of a `social.source.removed.v1` outbox row after its first publication
//! (XR-021 CONTRACTS.md R5-11, rule 4).
//!
//! The events stream drops its oldest message after 7 days or at 1 GiB, so a removal that
//! Knowledge has not consumed in time would be lost once the pump marked it published. A removal
//! therefore stays on the outbox until Knowledge answers with `knowledge.deletion.acknowledged.v1`:
//! the pump arms [`mark_published`] with the next resend time, publishes the stored bytes again
//! when [`due_resends`] says so and records each copy with [`record_resend`], and the consumer of
//! the acknowledgement ends the cycle with [`acknowledge`]. Rows of every other event type never
//! use any of it.

use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;

use crate::PersistenceError;

/// The only event type that is kept and re-sent until it is acknowledged.
pub const REMOVAL_EVENT_TYPE: &str = "social.source.removed.v1";

/// How long a published removal waits for Knowledge before it is published again, and the longest
/// the wait between two copies grows to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResendPolicy {
    /// The delay before the first republish; each later delay doubles.
    pub after: Duration,
    /// The longest delay between two republishes.
    pub cap: Duration,
}

impl Default for ResendPolicy {
    /// The deployed defaults: a first copy after 600 s, the delay doubling up to 6 h.
    fn default() -> Self {
        Self::new(Duration::from_mins(10), Duration::from_hours(6))
    }
}

impl ResendPolicy {
    /// A policy from the resend delay and its cap.
    #[must_use]
    pub const fn new(after: Duration, cap: Duration) -> Self {
        Self { after, cap }
    }
}

/// What Knowledge did with a removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckOutcome {
    /// The removal was applied, or repeated one that had been.
    Applied,
    /// Knowledge refused it; the data stays there and an operator must look.
    Refused,
}

impl AckOutcome {
    /// The value stored in `ack_outcome`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Refused => "refused",
        }
    }
}

/// A published removal that is due to be published again.
#[derive(Debug, Clone, PartialEq)]
pub struct DueResend {
    /// The row's `event_id`, which is also the `Nats-Msg-Id` of every copy.
    pub event_id: Uuid,
    /// The stored envelope.
    pub payload: serde_json::Value,
}

/// Marks a row published after the broker acknowledged it. A removal is also armed for its first
/// republish: `next_resend_at` becomes the publish time plus `policy.after`.
///
/// # Errors
///
/// [`PersistenceError::Query`] when the update fails.
pub async fn mark_published(
    pool: &PgPool,
    event_id: Uuid,
    policy: ResendPolicy,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "update threads_archive.outbox_events \
            set published_at = now(), last_error = null, \
                next_resend_at = case when event_type = $2 \
                                      then now() + make_interval(secs => $3) end \
          where event_id = $1 and published_at is null",
    )
    .bind(event_id)
    .bind(REMOVAL_EVENT_TYPE)
    .bind(policy.after.as_secs_f64())
    .execute(pool)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

/// The published, unacknowledged removals whose resend and backoff times have passed, oldest
/// resend first.
///
/// # Errors
///
/// [`PersistenceError::Query`] when the read fails.
pub async fn due_resends(pool: &PgPool, limit: i64) -> Result<Vec<DueResend>, PersistenceError> {
    let rows: Vec<(Uuid, serde_json::Value)> = sqlx::query_as(
        "select event_id, payload from threads_archive.outbox_events \
          where event_type = $1 and published_at is not null and acknowledged_at is null \
            and next_resend_at <= now() and (next_attempt_at is null or next_attempt_at <= now()) \
          order by next_resend_at, event_id limit $2",
    )
    .bind(REMOVAL_EVENT_TYPE)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(rows
        .into_iter()
        .map(|(event_id, payload)| DueResend { event_id, payload })
        .collect())
}

/// Records one broker-acknowledged republish of an unacknowledged removal: `resend_count` grows by
/// one and `next_resend_at` becomes now plus the least of `policy.after * 2^resend_count` and
/// `policy.cap`.
///
/// # Errors
///
/// [`PersistenceError::Query`] when the update fails.
pub async fn record_resend(
    pool: &PgPool,
    event_id: Uuid,
    policy: ResendPolicy,
) -> Result<(), PersistenceError> {
    // The exponent is clamped at 30, far beyond the point where the cap wins, so an operator who
    // sets a tiny delay and lets a removal wait for years cannot overflow the arithmetic.
    sqlx::query(
        "update threads_archive.outbox_events \
            set resend_count = resend_count + 1, last_error = null, attempt_count = 0, \
                next_resend_at = now() + make_interval(secs => \
                    least($3::float8 * power(2::float8, least(resend_count, 30)::float8), \
                          $4::float8)) \
          where event_id = $1 and event_type = $2 and published_at is not null \
            and acknowledged_at is null",
    )
    .bind(event_id)
    .bind(REMOVAL_EVENT_TYPE)
    .bind(policy.after.as_secs_f64())
    .bind(policy.cap.as_secs_f64())
    .execute(pool)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

/// Records Knowledge's answer on the removal `event_id` of the tenant `tenant` (the wire form of
/// the acknowledgement's `owner`). Returns whether a row changed: an unknown id, another tenant's
/// row, a row that is not a removal and an already acknowledged row change nothing.
///
/// # Errors
///
/// [`PersistenceError::Query`] when the update fails.
pub async fn acknowledge(
    pool: &PgPool,
    event_id: Uuid,
    tenant: &str,
    outcome: AckOutcome,
) -> Result<bool, PersistenceError> {
    let changed = sqlx::query(
        "update threads_archive.outbox_events \
            set acknowledged_at = now(), ack_outcome = $4 \
          where event_id = $1 and event_type = $2 and payload ->> 'tenant_id' = $3 \
            and acknowledged_at is null",
    )
    .bind(event_id)
    .bind(REMOVAL_EVENT_TYPE)
    .bind(tenant)
    .bind(outcome.as_str())
    .execute(pool)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(changed.rows_affected() == 1)
}

/// The removals Knowledge has not acknowledged yet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unacknowledged {
    /// How many there are.
    pub count: i64,
    /// Seconds since the oldest was queued, or zero when there is none.
    pub oldest_age_seconds: f64,
}

/// Counts the unacknowledged removals and ages the oldest.
///
/// # Errors
///
/// [`PersistenceError::Query`] when the read fails.
pub async fn unacknowledged_removals(pool: &PgPool) -> Result<Unacknowledged, PersistenceError> {
    let (count, oldest_age_seconds): (i64, f64) = sqlx::query_as(
        "select count(*), coalesce(extract(epoch from now() - min(occurred_at)), 0)::float8 \
           from threads_archive.outbox_events \
          where event_type = $1 and acknowledged_at is null",
    )
    .bind(REMOVAL_EVENT_TYPE)
    .fetch_one(pool)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(Unacknowledged {
        count,
        oldest_age_seconds,
    })
}
