//! A removal Threads published is kept and published again until Knowledge acknowledges it (XR-021
//! CONTRACTS.md R5-11 rule 4): the pump sends the same bytes again under the same `Nats-Msg-Id`,
//! the consumer of the Edge-provisioned durable `ratatoskr_threads_deletion_acks` ends the resend,
//! and a message that cannot be attributed to Knowledge is terminated. The broker is a private
//! `nats-server` enforcing the Threads stanza of `deploy/nats/identity.conf`.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "assertions in a test binary"
)]

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_nats::jetstream;
use ratatoskr_threads_archive::Database;
use ratatoskr_threads_archive::deletion_acks::{
    AcksError, AcksSummary, DURABLE, EVENTS_STREAM, FILTER_SUBJECT, consume_deletion_acks,
    ensure_acks_consumer,
};
use ratatoskr_threads_archive::nats::{
    self, COMMAND_STREAM, COMMAND_SUBJECT, CONSUMER_NAME, NatsConnection,
};
use ratatoskr_threads_archive::removal_acknowledgement::ResendPolicy;
use ratatoskr_threads_archive::test_support::TestDatabase;
use tokio::task::JoinHandle;
use uuid::Uuid;

mod enforcing;

use enforcing::Enforcing;

const OWNER: &str = "user:018f0000-0000-7000-8000-000000000005";
const OTHER_OWNER: &str = "user:018f0000-0000-7000-8000-000000000006";
const REMOVED_SUBJECT: &str = "evt.social.source.removed.v1";
const ACK_SUBJECT: &str = "evt.knowledge.deletion.acknowledged.v1";
/// A marker inside the stored envelope that no log line or metric may ever carry.
const CONTENT_MARKER: &str = "content-marker-never-logged";

/// What the test broker and the service identity look like once Edge has provisioned them.
struct Harness {
    _broker: Enforcing,
    edge: jetstream::Context,
    events: jetstream::stream::Stream,
    connection: NatsConnection,
    database: TestDatabase,
}

fn durable_config(
    durable: &str,
    filter: &str,
    ack_wait: Duration,
) -> jetstream::consumer::pull::Config {
    jetstream::consumer::pull::Config {
        durable_name: Some(durable.to_owned()),
        filter_subject: filter.to_owned(),
        ack_policy: jetstream::consumer::AckPolicy::Explicit,
        ack_wait,
        deliver_policy: jetstream::consumer::DeliverPolicy::All,
        ..jetstream::consumer::pull::Config::default()
    }
}

impl Harness {
    /// Starts the broker, plays Edge (both streams, the command durable, the acknowledgement
    /// durable) and connects the service identity through its own connection function. The events
    /// stream's duplicate window is 200 ms, so a test can see the second copy of a message id in
    /// the stream; the production window is 120 s, which is why the production resend delay
    /// defaults to 600 s.
    async fn start() -> Self {
        let threads_keys = nkeys::KeyPair::new_user();
        let admin_keys = nkeys::KeyPair::new_user();
        let broker = Enforcing::start(&threads_keys.public_key(), &admin_keys.public_key());
        let admin = async_nats::ConnectOptions::with_nkey(admin_keys.seed().expect("a seed"))
            .connect(broker.url())
            .await
            .expect("Edge connects");
        let edge = jetstream::new(admin);
        let commands = edge
            .get_or_create_stream(jetstream::stream::Config {
                name: COMMAND_STREAM.to_owned(),
                subjects: vec!["cmd.>".to_owned()],
                ..jetstream::stream::Config::default()
            })
            .await
            .expect("the command stream exists");
        commands
            .create_consumer(durable_config(
                CONSUMER_NAME,
                COMMAND_SUBJECT,
                Duration::from_secs(30),
            ))
            .await
            .expect("the command durable exists");
        let events = edge
            .get_or_create_stream(jetstream::stream::Config {
                name: EVENTS_STREAM.to_owned(),
                subjects: vec!["evt.>".to_owned()],
                duplicate_window: Duration::from_millis(200),
                ..jetstream::stream::Config::default()
            })
            .await
            .expect("the events stream exists");
        events
            .create_consumer(durable_config(
                DURABLE,
                FILTER_SUBJECT,
                Duration::from_secs(30),
            ))
            .await
            .expect("Edge provisions the acknowledgement durable");
        let seed_path = broker.directory.join("threads.nkey");
        std::fs::write(&seed_path, threads_keys.seed().expect("a seed")).expect("the seed");
        let connection = NatsConnection::connect_with_nkey(&broker.url(), &seed_path)
            .await
            .expect("the service connects under the Threads stanza");
        Self {
            _broker: broker,
            edge,
            events,
            connection,
            database: TestDatabase::create().await.expect("a test database"),
        }
    }

    fn database(&self) -> &Database {
        &self.database.database
    }

    /// Replaces the acknowledgement durable by one with another shape.
    async fn reprovision(&self, filter: &str, ack_wait: Duration) {
        self.events
            .delete_consumer(DURABLE)
            .await
            .expect("the durable is deleted");
        self.events
            .create_consumer(durable_config(DURABLE, filter, ack_wait))
            .await
            .expect("Edge provisions the durable again");
    }

    async fn publish(&self, subject: &str, body: String) -> u64 {
        self.edge
            .publish(subject.to_owned(), body.into())
            .await
            .expect("the message publishes")
            .await
            .expect("the broker stores the message")
            .sequence
    }

    /// The `Nats-Msg-Id` and bytes of every removal copy stored in the events stream.
    async fn removal_copies(&self) -> Vec<(String, Vec<u8>)> {
        let mut stream = self.events.clone();
        let last = stream
            .info()
            .await
            .expect("stream info")
            .state
            .last_sequence;
        let mut copies = Vec::new();
        for sequence in 1..=last {
            let raw = self
                .events
                .get_raw_message(sequence)
                .await
                .expect("the stored message is readable");
            if raw.subject.as_str() == REMOVED_SUBJECT {
                let id = raw
                    .headers
                    .get(async_nats::header::NATS_MESSAGE_ID)
                    .expect("every copy carries a Nats-Msg-Id")
                    .to_string();
                copies.push((id, raw.payload.to_vec()));
            }
        }
        copies
    }

    /// Waits until the acknowledgement durable has settled every message published so far.
    async fn wait_until_consumed(&self, sequence: u64) {
        let consumer: jetstream::consumer::Consumer<jetstream::consumer::pull::Config> = self
            .events
            .get_consumer(DURABLE)
            .await
            .expect("the durable exists");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let mut handle = consumer.clone();
            let info = handle.info().await.expect("the durable has info").clone();
            if info.ack_floor.stream_sequence >= sequence && info.num_ack_pending == 0 {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the consumer never settled stream sequence {sequence}: {info:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn finish(self) {
        self.database
            .cleanup()
            .await
            .expect("dropping the database");
    }
}

/// A removal envelope of `OWNER`; the row's `event_id` is the envelope's.
fn removal_envelope(id: Uuid, owner: &str) -> serde_json::Value {
    serde_json::json!({
        "event_id": id,
        "event_type": "social.source.removed.v1",
        "occurred_at": "2026-10-12T08:00:00Z",
        "producer": "ratatoskr-threads",
        "aggregate_id": format!("social_source:{id}"),
        "correlation_id": format!("deletion:{id}"),
        "tenant_id": owner,
        "schema_version": 1,
        "payload": {
            "social_source_id": id,
            "owner": owner,
            "reason": "user_requested",
            "removed_at": "2026-10-12T08:00:00Z",
            "note": CONTENT_MARKER,
        },
    })
}

async fn insert_removal(database: &Database, owner: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at) \
         values ($1, 'social.source.removed.v1', 'capture', $2, $3, now())",
    )
    .bind(id)
    .bind(Uuid::now_v7())
    .bind(removal_envelope(id, owner))
    .execute(database.pool())
    .await
    .expect("the removal row seeds");
    id
}

/// `(acknowledged, ack_outcome, resend_count, published)` of a row.
async fn row_state(database: &Database, id: Uuid) -> (bool, Option<String>, i32, bool) {
    sqlx::query_as(
        "select acknowledged_at is not null, ack_outcome, resend_count, published_at is not null \
           from threads_archive.outbox_events where event_id = $1",
    )
    .bind(id)
    .fetch_one(database.pool())
    .await
    .expect("the row is readable")
}

async fn wait_for(what: &str, mut condition: impl AsyncFnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition().await {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// What Knowledge publishes: the envelope of CONTRACTS.md R5-11 rule 1.
struct Answer {
    acknowledged: Uuid,
    owner: &'static str,
    family: &'static str,
    outcome: &'static str,
    refusal: Option<&'static str>,
    producer: &'static str,
    event_type: &'static str,
}

impl Answer {
    fn applied(acknowledged: Uuid) -> Self {
        Self {
            acknowledged,
            owner: OWNER,
            family: "social",
            outcome: "applied",
            refusal: None,
            producer: "ratatoskr-knowledge",
            event_type: "knowledge.deletion.acknowledged.v1",
        }
    }

    fn refused(acknowledged: Uuid) -> Self {
        Self {
            outcome: "refused",
            refusal: Some("foreign_subject"),
            ..Self::applied(acknowledged)
        }
    }

    fn document(&self) -> String {
        let mut payload = serde_json::json!({
            "acknowledged_event_id": self.acknowledged,
            "family": self.family,
            "owner": self.owner,
            "subject": format!("social_source:{}", self.acknowledged),
            "outcome": self.outcome,
            "acknowledged_at": "2026-10-12T08:00:02Z",
        });
        if let (Some(refusal), Some(object)) = (self.refusal, payload.as_object_mut()) {
            object.insert("refusal".to_owned(), serde_json::Value::from(refusal));
        }
        serde_json::json!({
            "event_id": Uuid::now_v7(),
            "event_type": self.event_type,
            "occurred_at": "2026-10-12T08:00:02Z",
            "producer": self.producer,
            "aggregate_id": format!("social_source:{}", self.acknowledged),
            "correlation_id": format!("deletion:{}", self.acknowledged),
            "causation_id": format!("event:{}", self.acknowledged),
            "tenant_id": self.owner,
            "schema_version": 1,
            "payload": payload,
        })
        .to_string()
    }
}

/// A running acknowledgement consumer whose summary is read when it stops.
struct Running {
    stop: tokio::sync::watch::Sender<bool>,
    handle: JoinHandle<Result<AcksSummary, AcksError>>,
}

impl Running {
    fn start(harness: &Harness) -> Self {
        let (stop, mut stopped) = tokio::sync::watch::channel(false);
        let connection = harness.connection.clone();
        let database = harness.database().clone();
        let handle = tokio::spawn(async move {
            consume_deletion_acks(&connection, &database, async move {
                let _ = stopped.wait_for(|requested| *requested).await;
            })
            .await
        });
        Self { stop, handle }
    }

    async fn finish(self) -> AcksSummary {
        let _ = self.stop.send(true);
        self.handle
            .await
            .expect("the consumer task does not panic")
            .expect("the consumer stops cleanly")
    }
}

/// Captures the log lines of the process-global subscriber the refusal test depends on.
#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("not poisoned")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl LogBuffer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("not poisoned")).into_owned()
    }
}

/// The log buffer and the Prometheus handle of the one subscriber and recorder this binary has.
static TELEMETRY: OnceLock<(LogBuffer, metrics_exporter_prometheus::PrometheusHandle)> =
    OnceLock::new();

fn telemetry() -> &'static (LogBuffer, metrics_exporter_prometheus::PrometheusHandle) {
    TELEMETRY.get_or_init(|| {
        let buffer = LogBuffer::default();
        let writer = buffer.clone();
        tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::INFO)
            .with_writer(move || writer.clone())
            .try_init()
            .expect("this test binary installs one subscriber");
        let handle = metrics_exporter_prometheus::PrometheusBuilder::new()
            .install_recorder()
            .expect("this test binary installs one recorder");
        (buffer, handle)
    })
}

#[tokio::test]
async fn an_unacknowledged_removal_is_published_again_until_knowledge_acknowledges_it() {
    let harness = Harness::start().await;
    let id = insert_removal(harness.database(), OWNER).await;
    let policy = ResendPolicy::new(Duration::from_millis(400), Duration::from_secs(2));
    let (stop_pump, pump_stopped) = tokio::sync::oneshot::channel::<()>();
    let pump = {
        let connection = harness.connection.clone();
        let database = harness.database().clone();
        tokio::spawn(async move {
            nats::run(&connection, &database, policy, async move {
                let _ = pump_stopped.await;
            })
            .await
        })
    };
    let consumer = Running::start(&harness);

    eprintln!("transcript: the pump publishes the removal {id}");
    wait_for("the first copy in the stream", async || {
        harness.removal_copies().await.len() == 1
    })
    .await;
    assert!(
        !row_state(harness.database(), id).await.0,
        "the first publish is not an acknowledgement"
    );

    eprintln!("transcript: no acknowledgement arrives; the pump publishes the same bytes again");
    wait_for("the second copy in the stream", async || {
        harness.removal_copies().await.len() >= 2
    })
    .await;
    let copies = harness.removal_copies().await;
    eprintln!(
        "transcript: {} copies in the stream, Nats-Msg-Id {} on each, {} bytes each",
        copies.len(),
        copies[0].0,
        copies[0].1.len()
    );
    assert_eq!(
        copies[0].0,
        id.to_string(),
        "the Nats-Msg-Id is the event id"
    );
    assert!(
        copies.iter().all(|copy| copy == &copies[0]),
        "every copy has the same Nats-Msg-Id and the same bytes"
    );
    let (acknowledged, _, resend_count, published) = row_state(harness.database(), id).await;
    assert!(!acknowledged && published && resend_count >= 1);

    eprintln!("transcript: Knowledge acknowledges the removal as applied");
    let sequence = harness
        .publish(ACK_SUBJECT, Answer::applied(id).document())
        .await;
    harness.wait_until_consumed(sequence).await;
    wait_for("the row to be acknowledged", async || {
        row_state(harness.database(), id).await.0
    })
    .await;
    assert_eq!(
        row_state(harness.database(), id).await.1.as_deref(),
        Some("applied")
    );

    eprintln!("transcript: the resend stops");
    tokio::time::sleep(Duration::from_millis(1_800)).await;
    let settled = harness.removal_copies().await.len();
    tokio::time::sleep(Duration::from_millis(3_500)).await;
    assert_eq!(
        harness.removal_copies().await.len(),
        settled,
        "no copy is published after the acknowledgement"
    );

    eprintln!(
        "transcript: {settled} copies in the stream after the acknowledgement, unchanged 3.5 s later"
    );
    let summary = consumer.finish().await;
    assert_eq!(summary.applied, 1, "{summary:?}");
    let _ = stop_pump.send(());
    pump.await
        .expect("the pump task joins")
        .expect("the pump stops cleanly");
    harness.finish().await;
}

#[tokio::test]
async fn a_refused_acknowledgement_stops_the_resend_and_is_surfaced_without_content() {
    let (logs, metrics) = telemetry();
    let harness = Harness::start().await;
    let id = insert_removal(harness.database(), OWNER).await;
    sqlx::query(
        "update threads_archive.outbox_events set published_at = now(), \
         next_resend_at = now() where event_id = $1",
    )
    .bind(id)
    .execute(harness.database().pool())
    .await
    .expect("the removal is published and due");
    let consumer = Running::start(&harness);

    let sequence = harness
        .publish(ACK_SUBJECT, Answer::refused(id).document())
        .await;
    harness.wait_until_consumed(sequence).await;
    wait_for("the row to be acknowledged", async || {
        row_state(harness.database(), id).await.0
    })
    .await;

    let state = row_state(harness.database(), id).await;
    assert_eq!(state.1.as_deref(), Some("refused"), "{state:?}");
    let summary = consumer.finish().await;
    assert_eq!((summary.refused, summary.applied), (1, 0), "{summary:?}");
    let rendered = metrics.render();
    assert!(
        rendered.contains("threads_tombstone_refused_total 1"),
        "the counter moved by one:\n{rendered}"
    );
    let logged = logs.text();
    let line = logged
        .lines()
        .find(|line| line.contains(&id.to_string()) && line.contains("\"level\":\"ERROR\""))
        .unwrap_or_else(|| panic!("an ERROR line names the refused removal:\n{logged}"));
    assert!(line.contains("foreign_subject"), "{line}");
    assert!(
        !logged.contains(CONTENT_MARKER) && !rendered.contains(CONTENT_MARKER),
        "neither the log nor the metrics carry stored content"
    );
    harness.finish().await;
}

#[tokio::test]
async fn messages_that_name_no_row_are_ignored_and_unattributable_ones_are_terminated() {
    let harness = Harness::start().await;
    let removal = insert_removal(harness.database(), OWNER).await;
    let other = insert_removal(harness.database(), OTHER_OWNER).await;
    let consumer = Running::start(&harness);

    // Valid acknowledgements that match nothing: an unknown id, the other tenant's row named
    // with the wrong owner, and a row of another family.
    let unknown = Answer::applied(Uuid::now_v7());
    let wrong_owner = Answer::applied(other);
    let wrong_family = Answer {
        family: "ai_archive",
        ..Answer::applied(removal)
    };
    // Messages that cannot be attributed to Knowledge, so redelivery cannot repair them.
    let wrong_producer = Answer {
        producer: "ratatoskr-x",
        ..Answer::applied(removal)
    };
    let wrong_type = Answer {
        event_type: "social.source.removed.v1",
        ..Answer::applied(removal)
    };
    let invalid_payload = Answer {
        outcome: "refused",
        refusal: None,
        ..Answer::applied(removal)
    };
    let mut last = 0;
    for body in [
        unknown.document(),
        wrong_owner.document(),
        wrong_family.document(),
        "this is not an envelope".to_owned(),
        wrong_producer.document(),
        wrong_type.document(),
        invalid_payload.document(),
    ] {
        last = harness.publish(ACK_SUBJECT, body).await;
    }
    harness.wait_until_consumed(last).await;

    let summary = consumer.finish().await;
    assert_eq!(
        summary,
        AcksSummary {
            applied: 0,
            refused: 0,
            ignored: 3,
            terminated: 4,
            retried: 0,
        }
    );
    for id in [removal, other] {
        let state = row_state(harness.database(), id).await;
        assert!(!state.0, "no row was acknowledged: {state:?}");
    }

    // A valid acknowledgement after the noise still works: the consumer was not poisoned.
    let consumer = Running::start(&harness);
    let sequence = harness
        .publish(ACK_SUBJECT, Answer::applied(removal).document())
        .await;
    harness.wait_until_consumed(sequence).await;
    assert_eq!(consumer.finish().await.applied, 1);
    assert!(row_state(harness.database(), removal).await.0);
    harness.finish().await;
}

#[tokio::test]
async fn a_durable_that_differs_from_its_spec_keeps_the_consumer_from_starting() {
    let harness = Harness::start().await;
    ensure_acks_consumer(&harness.connection)
        .await
        .expect("the Edge-provisioned durable verifies");

    harness
        .reprovision("evt.knowledge.deletion.>", Duration::from_secs(30))
        .await;
    assert!(
        matches!(
            ensure_acks_consumer(&harness.connection).await,
            Err(AcksError::Bus(_))
        ),
        "another filter is refused"
    );
    let stop = std::future::pending::<()>();
    assert!(
        consume_deletion_acks(&harness.connection, harness.database(), stop)
            .await
            .is_err(),
        "the consumer does not run over the wrong filter"
    );

    harness
        .reprovision(FILTER_SUBJECT, Duration::from_mins(1))
        .await;
    assert!(
        matches!(
            ensure_acks_consumer(&harness.connection).await,
            Err(AcksError::Bus(_))
        ),
        "another ack wait is refused"
    );

    harness
        .reprovision(FILTER_SUBJECT, Duration::from_secs(30))
        .await;
    ensure_acks_consumer(&harness.connection)
        .await
        .expect("the right shape verifies again");
    harness.finish().await;
}

#[tokio::test]
async fn the_identity_reaches_its_own_durable_and_no_sibling_s() {
    let harness = Harness::start().await;
    harness
        .events
        .create_consumer(durable_config(
            "ratatoskr_x_deletion_acks",
            FILTER_SUBJECT,
            Duration::from_secs(30),
        ))
        .await
        .expect("Edge provisions a sibling's durable");

    ensure_acks_consumer(&harness.connection)
        .await
        .expect("its own durable is reachable under the stanza");
    let sibling = tokio::time::timeout(
        Duration::from_secs(10),
        harness
            .connection
            .jetstream()
            .get_consumer_from_stream::<jetstream::consumer::pull::Config, _, _>(
                "ratatoskr_x_deletion_acks",
                EVENTS_STREAM,
            ),
    )
    .await;
    assert!(
        !matches!(sibling, Ok(Ok(_))),
        "a sibling's durable is refused by the broker"
    );
    harness.finish().await;
}
