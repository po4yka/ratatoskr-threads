//! The state machine of an unacknowledged removal: armed at its first publication, published
//! again with the same bytes and id while Knowledge is silent, ended by an acknowledgement
//! (XR-021 CONTRACTS.md R5-11 rule 4). Real `PostgreSQL`, a hand-written transport with a fault
//! queue instead of a broker.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "assertions and fixture helpers in a test binary"
)]

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use ratatoskr_threads_archive::nats::NatsError;
use ratatoskr_threads_archive::outbox_relay::{EventTransport, PublishFailure, flush_outbox};
use ratatoskr_threads_archive::removal_acknowledgement::{
    AckOutcome, ResendPolicy, acknowledge, unacknowledged_removals,
};
use ratatoskr_threads_archive::test_support::TestDatabase;
use sqlx::PgPool;
use uuid::Uuid;

const REMOVED: &str = "social.source.removed.v1";
const CAPTURED: &str = "social.source.captured.v1";

/// One publication the pump asked the transport for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sent {
    subject: &'static str,
    event_id: Uuid,
    body: Vec<u8>,
}

/// Records every publication; `refuse_next` makes the next one fail like a refusing broker.
#[derive(Default)]
struct FakeTransport {
    sent: Mutex<Vec<Sent>>,
    refusals: Mutex<VecDeque<&'static str>>,
}

impl FakeTransport {
    fn sent(&self) -> Vec<Sent> {
        self.sent
            .lock()
            .expect("the record is not poisoned")
            .clone()
    }

    fn refuse_next(&self, class: &'static str) {
        self.refusals
            .lock()
            .expect("the queue is not poisoned")
            .push_back(class);
    }
}

impl EventTransport for FakeTransport {
    async fn publish(
        &self,
        subject: &'static str,
        event_id: Uuid,
        body: Vec<u8>,
    ) -> Result<(), PublishFailure> {
        if let Some(class) = self
            .refusals
            .lock()
            .expect("the queue is not poisoned")
            .pop_front()
        {
            return Err(PublishFailure {
                class,
                error: NatsError::Unacknowledged,
            });
        }
        self.sent
            .lock()
            .expect("the record is not poisoned")
            .push(Sent {
                subject,
                event_id,
                body,
            });
        Ok(())
    }
}

/// A policy long enough that nothing becomes due by itself during a test.
fn slow_policy() -> ResendPolicy {
    ResendPolicy::new(Duration::from_secs(100), Duration::from_secs(1_000))
}

fn tenant() -> String {
    format!("user:{}", Uuid::now_v7())
}

/// Inserts an unpublished row whose envelope carries only what the transitions read.
async fn insert_row(pool: &PgPool, event_type: &str, tenant: &str) -> Uuid {
    let event_id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at) \
         values ($1, $2, 'capture', $3, $4, now())",
    )
    .bind(event_id)
    .bind(event_type)
    .bind(Uuid::now_v7())
    .bind(serde_json::json!({"event_id": event_id.to_string(), "tenant_id": tenant}))
    .execute(pool)
    .await
    .expect("the outbox row inserts");
    event_id
}

/// Makes the resend of `event_id` due now without waiting for the clock.
async fn make_due(pool: &PgPool, event_id: Uuid) {
    sqlx::query(
        "update threads_archive.outbox_events \
         set next_resend_at = now() - interval '1 second' where event_id = $1",
    )
    .bind(event_id)
    .execute(pool)
    .await
    .expect("the row is made due");
}

/// `(resend_count, seconds until the next resend)` of a published removal.
async fn resend_state(pool: &PgPool, event_id: Uuid) -> (i32, f64) {
    sqlx::query_as(
        "select resend_count, extract(epoch from next_resend_at - now())::float8 \
         from threads_archive.outbox_events where event_id = $1",
    )
    .bind(event_id)
    .fetch_one(pool)
    .await
    .expect("the row is stored")
}

#[tokio::test]
async fn a_published_removal_is_armed_and_no_other_event_type_is() {
    let database = TestDatabase::create().await.expect("a test database");
    let pool = database.database.pool();
    let transport = FakeTransport::default();
    let removal = insert_row(pool, REMOVED, &tenant()).await;
    let captured = insert_row(pool, CAPTURED, &tenant()).await;

    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("both rows publish");

    let armed: f64 = sqlx::query_scalar(
        "select extract(epoch from next_resend_at - published_at)::float8 \
         from threads_archive.outbox_events where event_id = $1 and published_at is not null",
    )
    .bind(removal)
    .fetch_one(pool)
    .await
    .expect("the removal is published and armed");
    assert!(
        (armed - 100.0).abs() < 0.001,
        "armed {armed} s after publication"
    );
    let captured_armed: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "select next_resend_at from threads_archive.outbox_events where event_id = $1",
    )
    .bind(captured)
    .fetch_one(pool)
    .await
    .expect("the captured fact is stored");
    assert_eq!(captured_armed, None, "a captured fact is never resent");
    database.cleanup().await.expect("dropping the database");
}

#[tokio::test]
async fn a_due_unacknowledged_removal_is_published_again_with_the_same_id_and_bytes() {
    let database = TestDatabase::create().await.expect("a test database");
    let pool = database.database.pool();
    let transport = FakeTransport::default();
    let removal = insert_row(pool, REMOVED, &tenant()).await;
    let captured = insert_row(pool, CAPTURED, &tenant()).await;
    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("the first pass publishes both");
    assert_eq!(transport.sent().len(), 2);

    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("a pass before the resend is due publishes nothing");
    assert_eq!(transport.sent().len(), 2, "nothing is due yet");

    make_due(pool, removal).await;
    make_due(pool, captured).await;
    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("the due removal is published again");

    let sent = transport.sent();
    assert_eq!(sent.len(), 3, "exactly the removal is sent again: {sent:?}");
    let first = sent
        .iter()
        .find(|copy| copy.event_id == removal)
        .expect("the first copy");
    let again = sent.last().expect("the second copy");
    assert_eq!(again.event_id, removal, "same Nats-Msg-Id");
    assert_eq!(again.subject, "evt.social.source.removed.v1");
    assert_eq!(again.body, first.body, "the stored bytes, unchanged");
    let (count, until_next) = resend_state(pool, removal).await;
    assert_eq!(count, 1);
    assert!(
        (99.0..=100.0).contains(&until_next),
        "the next copy is a resend delay away: {until_next}"
    );
    database.cleanup().await.expect("dropping the database");
}

#[tokio::test]
async fn the_resend_delay_doubles_up_to_the_cap_and_never_stops_by_count() {
    let database = TestDatabase::create().await.expect("a test database");
    let pool = database.database.pool();
    let transport = FakeTransport::default();
    let removal = insert_row(pool, REMOVED, &tenant()).await;
    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("the first publication");

    // The delay after copy n is `after * 2^(n - 1)`, from the count before the update.
    for (copy, expected) in [
        (1, 100.0),
        (2, 200.0),
        (3, 400.0),
        (4, 800.0),
        (5, 1_000.0),
        (6, 1_000.0),
    ] {
        make_due(pool, removal).await;
        flush_outbox(&transport, pool, slow_policy())
            .await
            .expect("the resend publishes");
        let (count, until_next) = resend_state(pool, removal).await;
        assert_eq!(count, copy);
        assert!(
            (expected - 1.0..=expected).contains(&until_next),
            "after copy {copy} the next one is {expected} s away, saw {until_next}"
        );
    }
    assert_eq!(
        transport.sent().len(),
        7,
        "the first publication and six copies"
    );
    database.cleanup().await.expect("dropping the database");
}

#[tokio::test]
async fn an_acknowledgement_ends_the_resend_and_a_second_one_changes_nothing() {
    let database = TestDatabase::create().await.expect("a test database");
    let pool = database.database.pool();
    let transport = FakeTransport::default();
    let owner = tenant();
    let applied = insert_row(pool, REMOVED, &owner).await;
    let refused = insert_row(pool, REMOVED, &owner).await;
    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("the first publication");

    assert!(
        acknowledge(pool, applied, &owner, AckOutcome::Applied)
            .await
            .expect("recording the acknowledgement"),
        "the first acknowledgement changes the row"
    );
    assert!(
        acknowledge(pool, refused, &owner, AckOutcome::Refused)
            .await
            .expect("recording the refusal"),
    );
    assert!(
        !acknowledge(pool, applied, &owner, AckOutcome::Refused)
            .await
            .expect("a duplicate is not an error"),
        "a duplicate acknowledgement changes nothing"
    );

    let outcomes: Vec<(Uuid, Option<String>, bool)> = sqlx::query_as(
        "select event_id, ack_outcome, acknowledged_at is not null \
         from threads_archive.outbox_events order by event_id",
    )
    .fetch_all(pool)
    .await
    .expect("reading the rows");
    for (event_id, outcome, acknowledged) in outcomes {
        assert!(acknowledged);
        let expected = if event_id == applied {
            "applied"
        } else {
            "refused"
        };
        assert_eq!(outcome.as_deref(), Some(expected), "the first answer stays");
    }

    make_due(pool, applied).await;
    make_due(pool, refused).await;
    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("a pass with nothing to resend");
    assert_eq!(
        transport.sent().len(),
        2,
        "neither removal is published again"
    );
    database.cleanup().await.expect("dropping the database");
}

#[tokio::test]
async fn an_acknowledgement_for_another_tenant_an_unknown_id_or_another_event_changes_nothing() {
    let database = TestDatabase::create().await.expect("a test database");
    let pool = database.database.pool();
    let transport = FakeTransport::default();
    let owner = tenant();
    let removal = insert_row(pool, REMOVED, &owner).await;
    let captured = insert_row(pool, CAPTURED, &owner).await;
    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("the first publication");

    for (id, tenant) in [
        (removal, tenant()),
        (Uuid::now_v7(), owner.clone()),
        (captured, owner.clone()),
    ] {
        assert!(
            !acknowledge(pool, id, &tenant, AckOutcome::Applied)
                .await
                .expect("a miss is not an error"),
            "{id} must not match"
        );
    }
    let acknowledged: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.outbox_events where acknowledged_at is not null",
    )
    .fetch_one(pool)
    .await
    .expect("counting acknowledged rows");
    assert_eq!(acknowledged, 0);
    assert!(
        acknowledge(pool, removal, &owner, AckOutcome::Applied)
            .await
            .expect("recording the acknowledgement"),
        "the same removal does match for its own tenant, so the misses above were misses"
    );
    database.cleanup().await.expect("dropping the database");
}

#[tokio::test]
async fn a_failed_republish_backs_off_without_touching_the_publication_or_the_schedule() {
    let database = TestDatabase::create().await.expect("a test database");
    let pool = database.database.pool();
    let transport = FakeTransport::default();
    let removal = insert_row(pool, REMOVED, &tenant()).await;
    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("the first publication");
    make_due(pool, removal).await;
    let before: (
        chrono::DateTime<chrono::Utc>,
        i32,
        chrono::DateTime<chrono::Utc>,
    ) = sqlx::query_as(
        "select published_at, resend_count, next_resend_at \
             from threads_archive.outbox_events where event_id = $1",
    )
    .bind(removal)
    .fetch_one(pool)
    .await
    .expect("the row before the refusal");

    transport.refuse_next("publish_failed");
    let outcome = flush_outbox(&transport, pool, slow_policy()).await;
    assert!(outcome.is_err(), "the refusal is surfaced to the worker");

    let after: (
        chrono::DateTime<chrono::Utc>,
        i32,
        chrono::DateTime<chrono::Utc>,
        i32,
        Option<String>,
        bool,
    ) = sqlx::query_as(
        "select published_at, resend_count, next_resend_at, attempt_count, last_error, \
                    next_attempt_at > now() \
             from threads_archive.outbox_events where event_id = $1",
    )
    .bind(removal)
    .fetch_one(pool)
    .await
    .expect("the row after the refusal");
    assert_eq!(
        (after.0, after.1, after.2),
        before,
        "the schedule is untouched"
    );
    assert_eq!(after.3, 1, "the failed attempt is counted");
    assert_eq!(after.4.as_deref(), Some("publish_failed"));
    assert!(after.5, "the row is backed off");

    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("the backed-off row is not attempted again at once");
    assert_eq!(
        transport.sent().len(),
        1,
        "only the first publication reached the broker"
    );
    database.cleanup().await.expect("dropping the database");
}

#[tokio::test]
async fn the_unacknowledged_count_and_age_follow_the_acknowledgements() {
    let database = TestDatabase::create().await.expect("a test database");
    let pool = database.database.pool();
    let transport = FakeTransport::default();
    let owner = tenant();
    let none = unacknowledged_removals(pool).await.expect("an empty read");
    assert_eq!(none.count, 0);
    assert!(none.oldest_age_seconds.abs() < f64::EPSILON);

    let oldest = insert_row(pool, REMOVED, &owner).await;
    let newer = insert_row(pool, REMOVED, &owner).await;
    insert_row(pool, CAPTURED, &owner).await;
    sqlx::query(
        "update threads_archive.outbox_events set occurred_at = now() - interval '120 seconds' \
         where event_id = $1",
    )
    .bind(oldest)
    .execute(pool)
    .await
    .expect("aging the oldest removal");
    flush_outbox(&transport, pool, slow_policy())
        .await
        .expect("publishing");

    let waiting = unacknowledged_removals(pool)
        .await
        .expect("two removals wait");
    assert_eq!(waiting.count, 2, "only removals are counted");
    assert!(waiting.oldest_age_seconds >= 120.0, "{waiting:?}");
    acknowledge(pool, oldest, &owner, AckOutcome::Applied)
        .await
        .expect("acknowledging the oldest");
    let waiting = unacknowledged_removals(pool)
        .await
        .expect("one removal waits");
    assert_eq!(waiting.count, 1);
    assert!(
        waiting.oldest_age_seconds < 120.0,
        "the age is of the one still waiting"
    );
    acknowledge(pool, newer, &owner, AckOutcome::Refused)
        .await
        .expect("acknowledging the other");
    assert_eq!(unacknowledged_removals(pool).await.expect("none").count, 0);
    database.cleanup().await.expect("dropping the database");
}
