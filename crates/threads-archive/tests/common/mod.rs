//! A Platform-shaped Threads command shared by the capture-resolution integration tests.

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

/// The permalink every fixture command captures.
pub(crate) const PERMALINK: &str = "https://www.threads.net/@author/post/AbCd1";

/// A Platform `social.capture.requested.v1` command for Threads, owned by a fresh tenant.
pub(crate) fn threads_command(operation: Uuid) -> CommandEnvelope {
    command_with(operation, PERMALINK, SocialCaptureProvider::Threads)
}

/// A Platform `social.capture.requested.v1` command naming `permalink` and `provider`.
///
/// The contract only requires an `https` link, so `permalink` may be one this service cannot map.
#[expect(
    clippy::expect_used,
    reason = "fixture helper: every constructed fragment is static and contract-valid"
)]
pub(crate) fn command_with(
    operation: Uuid,
    permalink: &str,
    provider: SocialCaptureProvider,
) -> CommandEnvelope {
    let payload = SocialCaptureRequested {
        operation_id: OperationId(operation),
        idempotency_key: ContentDigest {
            algorithm: DigestAlgorithm::Sha256,
            hex: DigestHex::parse(&operation.simple().to_string().repeat(2))
                .expect("a SHA-256 digest"),
        },
        original_permalink: PostPermalink::parse(permalink).expect("a permalink"),
        captured_at: WireTimestamp::now(), // wall-clock: opaque fixture value, only round-tripped
        provider,
        acquisition: AcquisitionMethod::BrowserExtension,
        saved_authority: SavedAuthority::ExplicitUserCapture,
        extensions: Extensions::new(),
    };
    let aggregate =
        EntityRef::parse(&format!("operation:{operation}")).expect("an operation reference");
    CommandEnvelope {
        command_id: CommandId(Uuid::now_v7()),
        command_type: SocialCaptureRequested::command_type(),
        issued_at: WireTimestamp::now(), // wall-clock: opaque fixture value, unused by any assertion
        producer: ProducerName::parse("ratatoskr-platform").expect("a producer"),
        aggregate_id: aggregate.clone(),
        correlation_id: aggregate,
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
