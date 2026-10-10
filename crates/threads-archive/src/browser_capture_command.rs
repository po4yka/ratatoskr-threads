//! Platform-routed explicit browser-capture commands.

use chrono::{DateTime, SubsecRound as _, Utc};
use ratatoskr_event_envelope::{CommandEnvelope, CommandError};
use ratatoskr_identifiers::{DigestAlgorithm, OperationId};
use ratatoskr_social_contracts::{
    AcquisitionMethod, SavedAuthority, SocialCaptureProvider, SocialCaptureRequested,
};
use uuid::Uuid;

use crate::permalink::CanonicalizedUrl;

/// A validated command owned by this provider.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserCaptureCommand {
    command_id: Uuid,
    user_ref: Uuid,
    operation_id: OperationId,
    idempotency_key: String,
    original_permalink: String,
    canonical_permalink: String,
    captured_at: DateTime<Utc>,
    acquisition: AcquisitionMethod,
    saved_authority: SavedAuthority,
}

/// A Threads command that was refused, with what is still known about whom to answer.
///
/// The envelope and payload decoded, the provider is Threads and the owner is named, so Platform
/// is waiting on `operation_id` and must be told that the capture cannot happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RejectedCommand {
    command_id: Uuid,
    user_ref: Uuid,
    operation_id: OperationId,
}

impl RejectedCommand {
    /// The command delivery identity used by the durable inbox.
    #[must_use]
    pub const fn command_id(&self) -> Uuid {
        self.command_id
    }

    /// The Ratatoskr user who owns the refused capture.
    #[must_use]
    pub const fn user_ref(&self) -> Uuid {
        self.user_ref
    }

    /// The Platform operation that is waiting for an answer.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }
}

/// What the consumer does with one broker payload.
#[derive(Debug)]
pub enum Intake {
    /// A command this service can capture.
    Accepted(BrowserCaptureCommand),
    /// A Threads command with an operation and an owner that cannot be captured: the operation
    /// still gets a terminal failure.
    Rejected {
        /// Whom to answer.
        command: RejectedCommand,
        /// Why the command was refused.
        reason: BrowserCaptureCommandError,
    },
    /// Input nobody is waiting on (undecodable, another provider, no owner): dropped for good.
    Invalid(BrowserCaptureCommandError),
}

/// A command that this consumer cannot accept.
#[derive(Debug, thiserror::Error)]
pub enum BrowserCaptureCommandError {
    /// The common envelope or its typed payload is malformed.
    #[error("the browser capture command is malformed")]
    Malformed(#[source] CommandError),
    /// The command names a different provider owner.
    #[error("the browser capture command is owned by a different provider")]
    WrongProvider,
    /// The social command widened its closed browser acquisition semantics.
    #[error("the browser capture command has invalid acquisition provenance")]
    InvalidAcquisition,
    /// The social command widened its closed explicit-capture authority.
    #[error("the browser capture command has invalid saved authority")]
    InvalidSavedAuthority,
    /// Platform omitted the tenant that identifies the capture owner.
    #[error("the browser capture command has no owner")]
    MissingTenant,
    /// The command permalink is not valid under Threads' local permalink policy.
    #[error("the browser capture command has an invalid Threads permalink")]
    InvalidPermalink,
    /// The wire capture timestamp cannot be represented by this service's database clock type.
    #[error("the browser capture command has an invalid capture timestamp")]
    InvalidCapturedAt,
    /// The contract carried a digest algorithm this first-version handler cannot use as an idempotency key.
    #[error("the browser capture command has an unsupported idempotency digest")]
    UnsupportedIdempotencyDigest,
}

impl BrowserCaptureCommandError {
    /// Whether a command refused with this error still deserves an answer.
    ///
    /// True for the refusals that come after the provider and owner checks passed: the command
    /// was meant for Threads, its operation is waiting, and nothing a redelivery does can change
    /// the answer.
    const fn is_attributable(&self) -> bool {
        matches!(
            self,
            Self::InvalidAcquisition
                | Self::InvalidSavedAuthority
                | Self::InvalidPermalink
                | Self::InvalidCapturedAt
                | Self::UnsupportedIdempotencyDigest
        )
    }
}

/// The operation and owner a refused Threads command names, when it names both.
fn attribute(envelope: &CommandEnvelope) -> Option<RejectedCommand> {
    let payload = envelope.payload_as::<SocialCaptureRequested>().ok()?;
    let user_ref = envelope.tenant_id?.user_id().0;
    Some(RejectedCommand {
        command_id: envelope.command_id.0,
        user_ref,
        operation_id: payload.operation_id,
    })
}

impl BrowserCaptureCommand {
    /// Parses a Platform command intended for this service.
    ///
    /// # Errors
    ///
    /// Returns [`BrowserCaptureCommandError`] when the command cannot be accepted.
    pub fn parse(envelope: &CommandEnvelope) -> Result<Self, BrowserCaptureCommandError> {
        let payload = envelope
            .payload_as::<SocialCaptureRequested>()
            .map_err(BrowserCaptureCommandError::Malformed)?;
        if payload.provider != SocialCaptureProvider::Threads {
            return Err(BrowserCaptureCommandError::WrongProvider);
        }
        if payload.acquisition != AcquisitionMethod::BrowserExtension {
            return Err(BrowserCaptureCommandError::InvalidAcquisition);
        }
        if payload.saved_authority != SavedAuthority::ExplicitUserCapture {
            return Err(BrowserCaptureCommandError::InvalidSavedAuthority);
        }
        let user_ref = envelope
            .tenant_id
            .ok_or(BrowserCaptureCommandError::MissingTenant)?
            .user_id()
            .0;
        let original_permalink = payload.original_permalink.to_string();
        let canonicalized = CanonicalizedUrl::try_from(original_permalink.as_str())
            .map_err(|_| BrowserCaptureCommandError::InvalidPermalink)?;
        // `captures.captured_at` is `timestamptz`, a microsecond-resolution wire type; truncate
        // here so the in-memory instant already matches what a later read of the persisted row
        // will return, rather than letting Postgres silently drop the sub-microsecond remainder
        // of `WireTimestamp`'s nanosecond precision on write.
        let captured_at = DateTime::parse_from_rfc3339(&payload.captured_at.to_wire())
            .map_err(|_| BrowserCaptureCommandError::InvalidCapturedAt)?
            .with_timezone(&Utc)
            .trunc_subsecs(6);
        let idempotency_key = match payload.idempotency_key.algorithm {
            DigestAlgorithm::Sha256 => format!("sha256:{}", payload.idempotency_key.hex),
            _ => return Err(BrowserCaptureCommandError::UnsupportedIdempotencyDigest),
        };
        Ok(Self {
            command_id: envelope.command_id.0,
            user_ref,
            operation_id: payload.operation_id,
            idempotency_key,
            original_permalink,
            canonical_permalink: canonicalized.permalink().as_str().to_owned(),
            captured_at,
            acquisition: payload.acquisition,
            saved_authority: payload.saved_authority,
        })
    }

    /// Decides what to do with a broker payload.
    ///
    /// A payload this service refuses for a reason of its own (a permalink it cannot map, widened
    /// provenance, an unusable digest) is [`Intake::Rejected`] when the command names its
    /// operation and owner; anything else that fails is [`Intake::Invalid`].
    #[must_use]
    pub fn intake(bytes: &[u8]) -> Intake {
        let envelope = match CommandEnvelope::from_json(bytes) {
            Ok(envelope) => envelope,
            Err(error) => return Intake::Invalid(BrowserCaptureCommandError::Malformed(error)),
        };
        match Self::parse(&envelope) {
            Ok(command) => Intake::Accepted(command),
            Err(reason) if reason.is_attributable() => match attribute(&envelope) {
                Some(command) => Intake::Rejected { command, reason },
                None => Intake::Invalid(reason),
            },
            Err(reason) => Intake::Invalid(reason),
        }
    }

    /// The command delivery identity used by the durable inbox.
    #[must_use]
    pub const fn command_id(&self) -> Uuid {
        self.command_id
    }

    /// The Ratatoskr user who owns the explicit capture.
    #[must_use]
    pub const fn user_ref(&self) -> Uuid {
        self.user_ref
    }

    /// The Platform operation this command advances.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// The stable domain idempotency key supplied by Platform.
    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    /// The captured original permalink.
    #[must_use]
    pub fn original_permalink(&self) -> &str {
        &self.original_permalink
    }

    /// The provider-local canonical Threads permalink.
    #[must_use]
    pub fn canonical_permalink(&self) -> &str {
        &self.canonical_permalink
    }

    /// The user-action instant received from Platform, not the consumer clock.
    #[must_use]
    pub const fn captured_at(&self) -> DateTime<Utc> {
        self.captured_at
    }

    /// The closed acquisition value.
    #[must_use]
    pub const fn acquisition(&self) -> AcquisitionMethod {
        self.acquisition
    }

    /// The closed saved-authority value.
    #[must_use]
    pub const fn saved_authority(&self) -> SavedAuthority {
        self.saved_authority
    }
}
