//! Contract boundary for provider-specific browser capture commands.

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
use uuid::Uuid;

use ratatoskr_threads_archive::browser_capture_command::BrowserCaptureCommand;
use ratatoskr_threads_archive::browser_capture_inbox::{
    BrowserCaptureDelivery, BrowserCaptureInbox,
};
use ratatoskr_threads_archive::test_support::TestDatabase;

#[expect(
    clippy::expect_used,
    reason = "fixture helper: every constructed fragment is static and contract-valid"
)]
fn browser_command(provider: SocialCaptureProvider) -> CommandEnvelope {
    let idempotency_key = ContentDigest {
        algorithm: DigestAlgorithm::Sha256,
        hex: DigestHex::parse(&"a".repeat(64)).expect("a SHA-256 digest"),
    };
    let payload = SocialCaptureRequested {
        operation_id: OperationId(Uuid::now_v7()),
        idempotency_key,
        original_permalink: PostPermalink::parse("https://www.threads.net/@author/post/AbCd1")
            .expect("a permalink"),
        captured_at: WireTimestamp::now(),
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
        issued_at: WireTimestamp::now(),
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
