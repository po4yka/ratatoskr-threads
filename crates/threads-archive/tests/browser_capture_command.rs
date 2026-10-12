//! Contract boundary for provider-specific browser capture commands.

use ratatoskr_event_envelope::{
    CommandEnvelope, CommandPayload as _, EnvelopeSchemaVersion, EventEnvelope, ProducerName,
};
use ratatoskr_identifiers::{
    CommandId, ContentDigest, DigestAlgorithm, DigestHex, EntityRef, EventId, Extensions,
    OperationId, TenantRef, UserId, WireTimestamp,
};
use ratatoskr_social_contracts::{
    AcquisitionMethod, PostPermalink, SavedAuthority, SocialCaptureProvider,
    SocialCaptureRequested, queued_report, report_envelope,
};
use uuid::Uuid;

use ratatoskr_threads_archive::browser_capture_command::BrowserCaptureCommand;
use ratatoskr_threads_archive::browser_capture_inbox::{
    BrowserCaptureDelivery, BrowserCaptureInbox,
};
use ratatoskr_threads_archive::test_support::TestDatabase;

fn browser_command(provider: SocialCaptureProvider) -> CommandEnvelope {
    browser_command_for(provider, "https://www.threads.net/@author/post/AbCd1")
}

#[expect(
    clippy::expect_used,
    reason = "fixture helper: every constructed fragment is static and contract-valid"
)]
fn browser_command_for(provider: SocialCaptureProvider, permalink: &str) -> CommandEnvelope {
    let idempotency_key = ContentDigest {
        algorithm: DigestAlgorithm::Sha256,
        hex: DigestHex::parse(&"a".repeat(64)).expect("a SHA-256 digest"),
    };
    let payload = SocialCaptureRequested {
        operation_id: OperationId(Uuid::now_v7()),
        idempotency_key,
        original_permalink: PostPermalink::parse(permalink).expect("a permalink"),
        captured_at: WireTimestamp::now(), // wall-clock: opaque fixture value, only round-tripped and never compared to a calendar value
        provider,
        acquisition: AcquisitionMethod::BrowserExtension,
        saved_authority: SavedAuthority::ExplicitUserCapture,
        extensions: Extensions::new(),
    };
    let aggregate_id = EntityRef::parse(&format!("operation:{}", payload.operation_id.0))
        .expect("an operation reference");
    CommandEnvelope {
        command_id: CommandId(Uuid::now_v7()),
        command_type: SocialCaptureRequested::command_type(),
        issued_at: WireTimestamp::now(), // wall-clock: opaque fixture value, unused by any assertion in this file
        producer: ProducerName::parse("ratatoskr-platform").expect("a producer"),
        aggregate_id: aggregate_id.clone(),
        correlation_id: aggregate_id,
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

#[test]
fn threads_consumer_accepts_only_threads_browser_capture_with_closed_provenance() {
    let command = BrowserCaptureCommand::parse(&browser_command(SocialCaptureProvider::Threads))
        .expect("the Threads command is accepted");

    assert_eq!(
        command.original_permalink(),
        "https://www.threads.net/@author/post/AbCd1"
    );
    assert_eq!(command.acquisition(), AcquisitionMethod::BrowserExtension);
    assert_eq!(
        command.saved_authority(),
        SavedAuthority::ExplicitUserCapture
    );

    let wrong_provider =
        BrowserCaptureCommand::parse(&browser_command(SocialCaptureProvider::Instagram));
    assert!(
        wrong_provider.is_err(),
        "an Instagram command must not reach Threads"
    );
}

#[tokio::test]
async fn threads_command_is_deduplicated_with_capture_provenance_and_queued_operation_report() {
    let database = TestDatabase::create().await.expect("a test database");
    let command = BrowserCaptureCommand::parse(&browser_command(SocialCaptureProvider::Threads))
        .expect("the Threads command is accepted");
    let inbox = BrowserCaptureInbox::new(&database.database);

    let first = inbox.deliver(&command).await.expect("the command persists");
    let capture_id = first
        .capture_id()
        .expect("the first command delivery must be accepted");
    let capture: (
        String,
        String,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
    ) = sqlx::query_as(
        "select original_url, acquisition_method, saved_authority, client_source, captured_at \
         from threads_archive.captures where capture_id = $1",
    )
    .bind(capture_id)
    .fetch_one(database.database.pool())
    .await
    .expect("the capture is stored");
    assert_eq!(capture.0, command.original_permalink());
    assert_eq!(capture.1, "browser_extension");
    assert_eq!(capture.2, "explicit_user_capture");
    assert_eq!(capture.3, "browser_extension");
    assert_eq!(capture.4, command.captured_at());

    let report: serde_json::Value = sqlx::query_scalar(
        "select payload from threads_archive.outbox_events \
         where event_type = 'platform.operation.reported.v1'",
    )
    .fetch_one(database.database.pool())
    .await
    .expect("a queued operation report is durable");
    assert_eq!(report["payload"]["status"], "queued");
    assert!(report["payload"].get("error").is_none());
    assert!(report["payload"].get("warnings").is_none());

    assert_eq!(
        inbox
            .deliver(&command)
            .await
            .expect("a duplicate is absorbed"),
        BrowserCaptureDelivery::Duplicate
    );
    let captures: i64 = sqlx::query_scalar("select count(*) from threads_archive.captures")
        .fetch_one(database.database.pool())
        .await
        .expect("counting captures");
    assert_eq!(captures, 1, "redelivery must not create another capture");

    database.cleanup().await.expect("cleanup");
}

/// `operation_id`, `command_id`, `resolution_attempts`, `operation_reported_at`,
/// `next_resolution_at` and whether that is already due.
type Bookkeeping = (
    Option<Uuid>,
    Option<Uuid>,
    i32,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<bool>,
);

#[tokio::test]
async fn intake_stores_operation_and_schedules_resolution() {
    let database = TestDatabase::create().await.expect("a test database");
    let command = BrowserCaptureCommand::parse(&browser_command(SocialCaptureProvider::Threads))
        .expect("the Threads command is accepted");

    let capture_id = BrowserCaptureInbox::new(&database.database)
        .deliver(&command)
        .await
        .expect("the command persists")
        .capture_id()
        .expect("the first delivery is accepted");

    let (operation_id, command_id, attempts, reported_at, due_at, due_now): Bookkeeping =
        sqlx::query_as(
            "select operation_id, command_id, resolution_attempts, operation_reported_at, \
                next_resolution_at, next_resolution_at <= now() \
         from threads_archive.captures where capture_id = $1",
        )
        .bind(capture_id)
        .fetch_one(database.database.pool())
        .await
        .expect("the capture is stored with its operation bookkeeping");
    assert_eq!(operation_id, Some(command.operation_id().0));
    assert_eq!(command_id, Some(command.command_id()));
    assert_eq!(attempts, 0);
    assert_eq!(reported_at, None, "nothing was reported terminally yet");
    assert!(due_at.is_some(), "resolution must be scheduled at intake");
    assert_eq!(due_now, Some(true), "resolution is due immediately");

    let (event_id, aggregate_id, correlation_id, causation_id, payload): (
        Uuid,
        Uuid,
        Option<Uuid>,
        Option<Uuid>,
        serde_json::Value,
    ) = sqlx::query_as(
        "select event_id, aggregate_id, correlation_id, causation_id, payload \
         from threads_archive.outbox_events \
         where aggregate_type = 'capture' and event_type = 'platform.operation.reported.v1'",
    )
    .fetch_one(database.database.pool())
    .await
    .expect("exactly one queued report is durable");
    assert_eq!(aggregate_id, capture_id);
    assert_eq!(correlation_id, Some(command.operation_id().0));
    assert_eq!(causation_id, Some(command.command_id()));

    let mut expected = report_envelope(
        "ratatoskr-threads",
        UserId(command.user_ref()),
        command.operation_id(),
        EntityRef::parse(&format!("capture:{capture_id}")).expect("a capture reference"),
        CommandId(command.command_id()),
        EventId(event_id),
        &queued_report(command.operation_id()).expect("the queued report"),
    )
    .expect("the contracts envelope builds");
    let stored = EventEnvelope::from_json(payload.to_string().as_bytes())
        .expect("the stored report is a canonical envelope");
    expected.occurred_at = stored.occurred_at;
    assert_eq!(
        stored, expected,
        "the queued report is exactly the contracts builder's"
    );

    database.cleanup().await.expect("cleanup");
}

#[tokio::test]
async fn a_command_whose_permalink_carries_a_percent_encoded_query_is_recorded_for_resolution() {
    let database = TestDatabase::create().await.expect("a test database");
    let command = BrowserCaptureCommand::parse(&browser_command_for(
        SocialCaptureProvider::Threads,
        "https://www.threads.net/@author/post/AbCd1?xmt=a%3Db",
    ))
    .expect("a tracking parameter with an escape does not refuse the command");

    let capture_id = BrowserCaptureInbox::new(&database.database)
        .deliver(&command)
        .await
        .expect("the command persists")
        .capture_id()
        .expect("the first delivery is accepted");

    let (canonical, status): (String, String) = sqlx::query_as(
        "select canonical_url, status from threads_archive.captures where capture_id = $1",
    )
    .bind(capture_id)
    .fetch_one(database.database.pool())
    .await
    .expect("the capture is stored");
    assert_eq!(canonical, "https://www.threads.net/@author/post/AbCd1");
    assert_eq!(
        status, "accepted",
        "the capture waits for the resolution lane"
    );

    let statuses: Vec<serde_json::Value> = sqlx::query_scalar(
        "select payload->'payload'->'status' from threads_archive.outbox_events \
         where event_type = 'platform.operation.reported.v1'",
    )
    .fetch_all(database.database.pool())
    .await
    .expect("the reports are readable");
    assert_eq!(
        statuses,
        vec![serde_json::json!("queued")],
        "only the queued report is emitted, no terminal failure"
    );

    database.cleanup().await.expect("cleanup");
}
