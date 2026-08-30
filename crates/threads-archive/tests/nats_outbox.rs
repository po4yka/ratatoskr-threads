//! Transactional-outbox behavior through a controllable acknowledgement transport.

use std::sync::{Arc, Mutex};

use ratatoskr_threads_archive::nats::{
    NatsError, OutboxPublication, OutboxTransport, publish_outbox_pass,
};
use ratatoskr_threads_archive::test_support::TestDatabase;
use tokio::sync::oneshot;
use uuid::Uuid;

type Timestamp = chrono::DateTime<chrono::Utc>;
type PoisonState = (Option<Timestamp>, Option<Timestamp>, i32, Option<String>);
type RetryState = (i32, Option<Timestamp>, Option<Timestamp>, Option<String>);
type TerminalState = (
    i32,
    Option<Timestamp>,
    Option<Timestamp>,
    Option<String>,
    serde_json::Value,
);

fn record_publication(
    publications: &Arc<Mutex<Vec<OutboxPublication>>>,
    publication: OutboxPublication,
) {
    let mut guard = match publications.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.push(publication);
}

fn recorded_publications(
    publications: &Arc<Mutex<Vec<OutboxPublication>>>,
) -> Vec<OutboxPublication> {
    let guard = match publications.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clone()
}

struct ControlledTransport {
    acknowledgement: Option<oneshot::Receiver<()>>,
    publication: Option<oneshot::Sender<OutboxPublication>>,
}

struct SelectiveFailureTransport {
    failed_event_ids: Vec<Uuid>,
    publications: Arc<Mutex<Vec<OutboxPublication>>>,
}

impl OutboxTransport for SelectiveFailureTransport {
    async fn publish(&mut self, publication: OutboxPublication) -> Result<(), NatsError> {
        let should_fail = self.failed_event_ids.contains(&publication.event_id());
        let publications = Arc::clone(&self.publications);
        record_publication(&publications, publication);
        if should_fail {
            Err(NatsError::Bus(
                "raw broker detail must never reach durable evidence".to_owned(),
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Default)]
struct RecordingTransport {
    publications: Arc<Mutex<Vec<OutboxPublication>>>,
}

impl OutboxTransport for RecordingTransport {
    async fn publish(&mut self, publication: OutboxPublication) -> Result<(), NatsError> {
        let publications = Arc::clone(&self.publications);
        record_publication(&publications, publication);
        Ok(())
    }
}

impl OutboxTransport for ControlledTransport {
    async fn publish(&mut self, publication: OutboxPublication) -> Result<(), NatsError> {
        let acknowledgement = self.acknowledgement.take();
        let sent = self.publication.take();
        sent.ok_or(NatsError::UnexpectedOutboxEventType)?
            .send(publication)
            .map_err(|_| NatsError::UnexpectedOutboxEventType)?;
        acknowledgement
            .ok_or(NatsError::UnexpectedOutboxEventType)?
            .await
            .map_err(|_| NatsError::UnexpectedOutboxEventType)?;
        Ok(())
    }
}

fn removal_envelope(event_id: Uuid, aggregate_id: Uuid) -> serde_json::Value {
    envelope(event_id, aggregate_id, "social.source.removed.v1")
}

fn envelope(event_id: Uuid, aggregate_id: Uuid, event_type: &str) -> serde_json::Value {
    serde_json::json!({
        "event_id": event_id,
        "event_type": event_type,
        "occurred_at": "2026-08-30T00:00:00Z",
        "producer": "ratatoskr-threads",
        "aggregate_id": format!("social_source:{aggregate_id}"),
        "correlation_id": format!("deletion:{aggregate_id}"),
        "tenant_id": format!("user:{aggregate_id}"),
        "schema_version": 1,
        "payload": {}
    })
}

#[tokio::test]
async fn existing_removal_is_delivered_with_original_identity_only_after_ack() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool().clone();
    let event_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    let envelope = removal_envelope(event_id, aggregate_id);
    sqlx::query(
        "insert into threads_archive.outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at) \
         values ($1, 'social.source.removed.v1', 'capture', $2, $3, now())",
    )
    .bind(event_id)
    .bind(aggregate_id)
    .bind(&envelope)
    .execute(&pool)
    .await
    .expect("the existing removal row inserts");

    let (publication_sender, publication_receiver) = oneshot::channel();
    let (acknowledgement_sender, acknowledgement_receiver) = oneshot::channel();
    let mut transport = ControlledTransport {
        acknowledgement: Some(acknowledgement_receiver),
        publication: Some(publication_sender),
    };
    let publishing_pool = pool.clone();
    let publisher =
        tokio::spawn(async move { publish_outbox_pass(&mut transport, &publishing_pool).await });

    let publication = publication_receiver
        .await
        .expect("the stored removal reaches the transport");
    assert_eq!(publication.event_id(), event_id);
    assert_eq!(publication.subject(), "evt.social.source.removed.v1");
    assert_eq!(
        publication.payload(),
        serde_json::to_vec(&envelope)
            .expect("the expected stored envelope serializes")
            .as_slice(),
        "the publisher must not regenerate or replace the stored envelope"
    );
    let published_before_ack: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "select published_at from threads_archive.outbox_events where event_id = $1",
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .expect("the pre-ack state query answers");
    assert!(
        published_before_ack.is_none(),
        "publication cannot be marked before acknowledgement"
    );

    acknowledgement_sender
        .send(())
        .expect("the publisher still awaits acknowledgement");
    publisher
        .await
        .expect("the publisher task joins")
        .expect("the acknowledged pass succeeds");
    let published_after_ack: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "select published_at from threads_archive.outbox_events where event_id = $1",
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .expect("the post-ack state query answers");
    assert!(
        published_after_ack.is_some(),
        "the acknowledged stored event becomes published"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn unsupported_oldest_row_does_not_block_later_supported_row() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();
    let poison_event_id = Uuid::now_v7();
    let captured_event_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    for (event_id, event_type, payload, age) in [
        (
            poison_event_id,
            "threads.unsupported.fact.v1",
            envelope(poison_event_id, aggregate_id, "threads.unsupported.fact.v1"),
            "2 minutes",
        ),
        (
            captured_event_id,
            "social.source.captured.v1",
            envelope(captured_event_id, aggregate_id, "social.source.captured.v1"),
            "1 minute",
        ),
    ] {
        sqlx::query(
            "insert into threads_archive.outbox_events \
             (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at) \
             values ($1, $2, 'capture', $3, $4, now() - $5::interval)",
        )
        .bind(event_id)
        .bind(event_type)
        .bind(aggregate_id)
        .bind(payload)
        .bind(age)
        .execute(pool)
        .await
        .expect("the ordered outbox fixture row inserts");
    }

    let mut transport = RecordingTransport::default();
    let recorded = Arc::clone(&transport.publications);
    publish_outbox_pass(&mut transport, pool)
        .await
        .expect("one poison row cannot fail the bounded pass");

    let poison_state: PoisonState = sqlx::query_as(
        "select published_at, dead_lettered_at, attempt_count, last_error \
         from threads_archive.outbox_events where event_id = $1",
    )
    .bind(poison_event_id)
    .fetch_one(pool)
    .await
    .expect("the poison-row state query answers");
    assert_eq!(poison_state.0, None, "poison is never falsely published");
    assert!(poison_state.1.is_some(), "deterministic poison is terminal");
    assert_eq!(
        poison_state.2, 1,
        "poison records its first observed attempt"
    );
    assert_eq!(poison_state.3.as_deref(), Some("unsupported_event_type"));

    let captured_published: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "select published_at from threads_archive.outbox_events where event_id = $1",
    )
    .bind(captured_event_id)
    .fetch_one(pool)
    .await
    .expect("the later captured-row state query answers");
    assert!(
        captured_published.is_some(),
        "the later fact is acknowledged"
    );
    let publications = recorded_publications(&recorded);
    assert_eq!(publications.len(), 1);
    assert_eq!(publications[0].event_id(), captured_event_id);

    test.cleanup().await.expect("cleanup must drop");
}

#[expect(
    clippy::too_many_lines,
    reason = "one end-to-end retry policy scenario keeps its database timeline visible"
)]
#[tokio::test]
async fn transient_failure_honors_due_time_and_dead_letters_after_twelve_attempts() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();
    let aggregate_id = Uuid::now_v7();
    let not_due_event_id = Uuid::now_v7();
    let retry_event_id = Uuid::now_v7();
    let terminal_event_id = Uuid::now_v7();
    let captured_event_id = Uuid::now_v7();
    for (event_id, attempt_count, next_attempt) in [
        (not_due_event_id, 1, Some("5 minutes")),
        (retry_event_id, 10, None),
        (terminal_event_id, 11, None),
        (captured_event_id, 0, None),
    ] {
        sqlx::query(
            "insert into threads_archive.outbox_events \
             (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at, \
              attempt_count, next_attempt_at) \
             values ($1, 'social.source.captured.v1', 'capture', $2, $3, now() - interval '1 minute', \
                     $4, case when $5::text is null then null else now() + $5::interval end)",
        )
        .bind(event_id)
        .bind(aggregate_id)
        .bind(envelope(
            event_id,
            aggregate_id,
            "social.source.captured.v1",
        ))
        .bind(attempt_count)
        .bind(next_attempt)
        .execute(pool)
        .await
        .expect("the retry fixture row inserts");
    }

    let before_pass: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("select now()")
        .fetch_one(pool)
        .await
        .expect("the database clock answers");
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let mut transport = SelectiveFailureTransport {
        failed_event_ids: vec![retry_event_id, terminal_event_id],
        publications: Arc::clone(&recorded),
    };
    publish_outbox_pass(&mut transport, pool)
        .await
        .expect("transport failures are isolated to their rows");
    let after_pass: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("select now()")
        .fetch_one(pool)
        .await
        .expect("the database clock still answers");

    let not_due_state: (Option<chrono::DateTime<chrono::Utc>>, i32, Option<String>) =
        sqlx::query_as(
            "select published_at, attempt_count, last_error \
             from threads_archive.outbox_events where event_id = $1",
        )
        .bind(not_due_event_id)
        .fetch_one(pool)
        .await
        .expect("the not-due state query answers");
    assert_eq!(not_due_state, (None, 1, None), "not-due work is untouched");

    let retry_state: RetryState = sqlx::query_as(
        "select attempt_count, next_attempt_at, dead_lettered_at, last_error \
         from threads_archive.outbox_events where event_id = $1",
    )
    .bind(retry_event_id)
    .fetch_one(pool)
    .await
    .expect("the retry state query answers");
    assert_eq!(retry_state.0, 11);
    let retry_at = retry_state.1.expect("the retry remains scheduled");
    assert!(retry_at >= before_pass + chrono::Duration::seconds(299));
    assert!(retry_at <= after_pass + chrono::Duration::seconds(301));
    assert_eq!(retry_state.2, None);
    assert_eq!(retry_state.3.as_deref(), Some("broker_unacknowledged"));

    let terminal_state: TerminalState = sqlx::query_as(
        "select attempt_count, next_attempt_at, dead_lettered_at, last_error, payload \
         from threads_archive.outbox_events where event_id = $1",
    )
    .bind(terminal_event_id)
    .fetch_one(pool)
    .await
    .expect("the terminal state query answers");
    assert_eq!(terminal_state.0, 12);
    assert_eq!(terminal_state.1, None);
    assert!(terminal_state.2.is_some());
    assert_eq!(terminal_state.3.as_deref(), Some("broker_unacknowledged"));
    assert_eq!(
        terminal_state.4,
        envelope(terminal_event_id, aggregate_id, "social.source.captured.v1"),
        "dead-lettering retains the original event identity and envelope"
    );

    let captured_published: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "select published_at from threads_archive.outbox_events where event_id = $1",
    )
    .bind(captured_event_id)
    .fetch_one(pool)
    .await
    .expect("the later due row state query answers");
    assert!(captured_published.is_some(), "later due work still drains");
    let attempted_ids: Vec<Uuid> = recorded_publications(&recorded)
        .iter()
        .map(OutboxPublication::event_id)
        .collect();
    assert!(!attempted_ids.contains(&not_due_event_id));
    assert!(attempted_ids.contains(&captured_event_id));

    let mut later_transport = RecordingTransport::default();
    let later_recorded = Arc::clone(&later_transport.publications);
    publish_outbox_pass(&mut later_transport, pool)
        .await
        .expect("a later bounded pass succeeds");
    assert!(
        recorded_publications(&later_recorded).is_empty(),
        "not-due, backed-off, and dead-lettered rows are excluded"
    );

    test.cleanup().await.expect("cleanup must drop");
}
