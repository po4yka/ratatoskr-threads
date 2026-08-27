//! Platform-routed explicit browser-capture commands.

use chrono::{DateTime, Utc};
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
        let captured_at = DateTime::parse_from_rfc3339(&payload.captured_at.to_wire())
            .map_err(|_| BrowserCaptureCommandError::InvalidCapturedAt)?
            .with_timezone(&Utc);
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

    /// Deserializes and validates a broker command.
    ///
    /// # Errors
    ///
    /// Returns [`BrowserCaptureCommandError`] when the canonical envelope or
    /// its Threads payload cannot be accepted.
    pub fn from_json(bytes: &[u8]) -> Result<Self, BrowserCaptureCommandError> {
        let envelope =
            CommandEnvelope::from_json(bytes).map_err(BrowserCaptureCommandError::Malformed)?;
        Self::parse(&envelope)
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
