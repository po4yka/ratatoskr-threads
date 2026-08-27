//! Real broker and database proof for the Threads browser-capture consumer.

use std::time::Duration;

use futures_util::StreamExt as _;
use ratatoskr_event_envelope::{
    CommandEnvelope, CommandPayload as _, EnvelopeSchemaVersion, ProducerName,
};
use ratatoskr_identifiers::{
    CommandId, ContentDigest, DigestAlgorithm, DigestHex, EntityRef, Extensions, OperationId,
    TenantRef, UserId, WireTimestamp,
};
use ratatoskr_social_contracts::{
    AcquisitionMethod, PostPermalink, SavedAuthority, SocialCaptureProvider, SocialCaptureRequested,
};
use ratatoskr_threads_archive::nats::{
    self, COMMAND_STREAM, COMMAND_SUBJECT, CONSUMER_NAME, NatsConnection,
};
use ratatoskr_threads_archive::test_support::TestDatabase;
use uuid::Uuid;

#[expect(
    clippy::disallowed_methods,
    reason = "the isolated test broker endpoint is selected by the test process"
)]
fn nats_url() -> String {
    std::env::var("THREADS_ARCHIVE_TEST_NATS_URL")
        .unwrap_or_else(|_| "nats://127.0.0.1:5422".to_owned())
}

#[expect(
    clippy::expect_used,
    reason = "fixture helper: every constructed fragment is static and contract-valid"
)]
fn command() -> CommandEnvelope {
    let payload = SocialCaptureRequested {
        operation_id: OperationId(Uuid::now_v7()),
        idempotency_key: ContentDigest {
            algorithm: DigestAlgorithm::Sha256,
            hex: DigestHex::parse(&"a".repeat(64)).expect("a SHA-256 digest"),
        },
        original_permalink: PostPermalink::parse("https://www.threads.net/@author/post/AbCd1")
            .expect("a permalink"),
        captured_at: WireTimestamp::now(),
        provider: SocialCaptureProvider::Threads,
        acquisition: AcquisitionMethod::BrowserExtension,
        saved_authority: SavedAuthority::ExplicitUserCapture,
        extensions: Extensions::new(),
    };
    let operation = EntityRef::parse(&format!("operation:{}", payload.operation_id.0))
        .expect("an operation reference");
    CommandEnvelope {
        command_id: CommandId(Uuid::now_v7()),
        command_type: SocialCaptureRequested::command_type(),
        issued_at: WireTimestamp::now(),
        producer: ProducerName::parse("ratatoskr-platform").expect("a producer"),
        aggregate_id: operation.clone(),
        correlation_id: operation,
        causation_id: None,
        tenant_id: Some(TenantRef::of_user(UserId(Uuid::now_v7()))),
        schema_version: EnvelopeSchemaVersion::CURRENT,
        payload: serde_json::to_value(payload)
            .expect("the command serializes")
            .as_object()
            .expect("the payload is an object")
            .clone(),
        extensions: Extensions::new(),
    }
}

#[tokio::test]
async fn durable_threads_subject_commits_then_reports_and_acknowledges() {
    let database = TestDatabase::create().await.expect("a test database");
    let client = async_nats::connect(nats_url())
        .await
        .expect("the isolated NATS broker is reachable");
    let context = async_nats::jetstream::new(client.clone());
    let commands = context
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: COMMAND_STREAM.to_owned(),
            subjects: vec!["cmd.>".to_owned()],
            ..async_nats::jetstream::stream::Config::default()
        })
        .await
        .expect("the command stream exists");
    let _events = context
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: "ratatoskr_events".to_owned(),
            subjects: vec!["evt.>".to_owned()],
            ..async_nats::jetstream::stream::Config::default()
        })
        .await
        .expect("the event stream exists");
    let _consumer = commands
        .get_or_create_consumer(
            CONSUMER_NAME,
            async_nats::jetstream::consumer::pull::Config {
                durable_name: Some(CONSUMER_NAME.to_owned()),
                filter_subject: COMMAND_SUBJECT.to_owned(),
                ..async_nats::jetstream::consumer::pull::Config::default()
            },
        )
        .await
        .expect("the preprovisioned Threads consumer exists");
    let mut reports = client
        .subscribe("evt.platform.operation.reported.v1")
        .await
        .expect("subscribing before command publication");
    let connection = NatsConnection::connect(&nats_url())
        .await
        .expect("the service role connects");
    nats::ensure_command_consumer(&connection)
        .await
        .expect("the service role opens only its preprovisioned consumer");
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let worker_database = database.database.clone();
    let worker_connection = connection.clone();
    let worker = tokio::spawn(async move {
        nats::run(&worker_connection, &worker_database, async move {
            let _ignored = stop_rx.await;
        })
        .await
    });

    let command = command();
    let published = context
        .publish(
            COMMAND_SUBJECT,
            command
                .to_canonical_json()
                .expect("the command serializes")
                .into(),
        )
        .await
        .expect("publishing a durable command");
    published.await.expect("the command is stored by JetStream");

    let report = tokio::time::timeout(Duration::from_secs(10), reports.next())
        .await
        .expect("the queued operation report arrives")
        .expect("the report subscription remains open");
    let report: serde_json::Value =
        serde_json::from_slice(&report.payload).expect("valid report JSON");
    assert_eq!(report["payload"]["status"], "queued");
    assert!(report["payload"].get("error").is_none());
    assert!(report["payload"].get("warnings").is_none());

    let captures: i64 = sqlx::query_scalar("select count(*) from threads_archive.captures")
        .fetch_one(database.database.pool())
        .await
        .expect("the capture transaction committed before the report");
    let inbox: i64 = sqlx::query_scalar("select count(*) from threads_archive.inbox_events")
        .fetch_one(database.database.pool())
        .await
        .expect("the inbox transaction committed before acknowledgement");
    assert_eq!(captures, 1);
    assert_eq!(inbox, 1);

    let mut consumer = commands
        .get_consumer::<async_nats::jetstream::consumer::pull::Config>(CONSUMER_NAME)
        .await
        .expect("reading the preprovisioned consumer");
    let info = consumer.info().await.expect("reading consumer state");
    assert_eq!(info.num_pending, 0, "the durable command was acknowledged");
    assert_eq!(info.num_ack_pending, 0, "no unacknowledged command remains");

    stop_tx.send(()).expect("stopping the worker");
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("the worker stops")
        .expect("the worker task joins")
        .expect("the worker exits cleanly");
    database
        .cleanup()
        .await
        .expect("dropping the test database");
}
