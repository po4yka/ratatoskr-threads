//! Provenance semantics: acquisition modes, their authority ceilings, support
//! status, and the boundary between upstream availability and local preservation.
//!
//! Every wire value here equals, value for value, both a `threads_archive`
//! CHECK vocabulary and the serde representation of the published
//! `ratatoskr-social-contracts` crate at revision `fb88f94`; the tests in
//! `tests/capability.rs` pin all three together.

/// One way a source can enter this bounded context.
///
/// The inventory is closed: five modes covering every ingestion lane the
/// service may ever operate, with `LegacyImport` carrying monolith migration
/// and `ExplicitCapture` owning all three client lanes (share extensions,
/// browser extension, Telegram). Adding a mode means adding a lane and an
/// alignment entry, never silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AcquisitionMode {
    /// The user pushed a Threads URL into Ratatoskr through a share target,
    /// the browser extension, or Telegram.
    ExplicitCapture,
    /// Content was resolved through the supported public metadata surface.
    PublicResolution,
    /// The connected user's own posts and replies were read through the
    /// official authenticated API.
    OwnAccountSync,
    /// Records were parsed out of a Threads Data Export the user supplied.
    DataExport,
    /// Records were carried over from the retired monolith.
    LegacyImport,
}

impl AcquisitionMode {
    /// Every acquisition mode, exactly the declared inventory.
    pub const ALL: [AcquisitionMode; 5] = [
        AcquisitionMode::ExplicitCapture,
        AcquisitionMode::PublicResolution,
        AcquisitionMode::OwnAccountSync,
        AcquisitionMode::DataExport,
        AcquisitionMode::LegacyImport,
    ];

    /// The capability matrix answer for this mode: the mode's support status
    /// and its authority ceiling. A lane reports `Planned` until the plan item
    /// implementing it flips its status with a reviewed test change.
    #[must_use]
    pub fn capability(self) -> ModeCapability {
        let authority_ceiling = match self {
            Self::ExplicitCapture | Self::PublicResolution => SavedAuthority::ExplicitUserCapture,
            Self::OwnAccountSync => SavedAuthority::AuthoritativePlatformState,
            Self::DataExport => SavedAuthority::ExportObservation,
            Self::LegacyImport => SavedAuthority::LegacyObservation,
        };
        let status = match self {
            // Capture intake is implementation plan item 3; public resolution and relation
            // normalization is item 4, event publication item 5, OAuth discovery item 6,
            // own-post sync item 7, Data Export import item 8. Legacy migration has no
            // dedicated item yet and stays `Planned`.
            Self::ExplicitCapture
            | Self::PublicResolution
            | Self::OwnAccountSync
            | Self::DataExport
            | Self::LegacyImport => SupportStatus::Planned,
        };
        ModeCapability {
            mode: self,
            status,
            authority_ceiling,
        }
    }
}

/// Whether a capability can be exercised today.
///
/// `Planned` names a lane the repository intends to build; flipping a status to
/// `Supported` is a deliberate, tested change made by the plan item that lands
/// the implementation. `NotSupported` states a provider limitation honestly
/// instead of leaving it silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportStatus {
    /// Implemented and exercisable in this service today.
    Supported,
    /// Planned by an open implementation item; not exercisable yet.
    Planned,
    /// No supported provider surface exists; the service will not pretend.
    NotSupported,
}

/// What a saved-state claim is worth.
///
/// Mirrors the `SavedAuthority` vocabulary of `ratatoskr-social-contracts`
/// (`crates/social-contracts/src/vocabulary.rs`, revision `fb88f94`); the
/// alignment test pins the two sets together. An explicit capture proves the
/// user saved an item to Ratatoskr, never membership in a native Threads
/// Saved list, which no supported surface exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SavedAuthority {
    /// The platform itself exposes this state through a supported API surface.
    AuthoritativePlatformState,
    /// A user action inside Ratatoskr captured the source; provider state unknown.
    ExplicitUserCapture,
    /// A provider export shows the item was saved at some point, without live authority.
    ExportObservation,
    /// Migrated from the retired monolith; worth what that record was worth.
    LegacyObservation,
}

impl SavedAuthority {
    /// The `snake_case` wire value stored in provenance columns, equal to the
    /// schema CHECK vocabulary value for value.
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::AuthoritativePlatformState => "authoritative_platform_state",
            Self::ExplicitUserCapture => "explicit_user_capture",
            Self::ExportObservation => "export_observation",
            Self::LegacyObservation => "legacy_observation",
        }
    }
}

/// The capability matrix row for one acquisition mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeCapability {
    /// The mode this row describes.
    pub mode: AcquisitionMode,
    /// Whether the lane is exercisable today.
    pub status: SupportStatus,
    /// The strongest saved-authority claim this mode may ever make.
    pub authority_ceiling: SavedAuthority,
}

impl ModeCapability {
    /// The closed wire vocabulary this mode produces, as stored provenance
    /// values shared with the schema CHECKs and the contract serde
    /// representations. Each contract acquisition method belongs to exactly
    /// one mode; `telegram_capture` is the documented Threads extension owned
    /// by explicit capture.
    #[must_use]
    pub fn wire_methods(&self) -> &'static [&'static str] {
        match self.mode {
            AcquisitionMode::ExplicitCapture => {
                &["share_extension", "browser_extension", "telegram_capture"]
            }
            AcquisitionMode::PublicResolution => &["public_resolution"],
            AcquisitionMode::OwnAccountSync => &["official_api"],
            AcquisitionMode::DataExport => &["data_export"],
            AcquisitionMode::LegacyImport => &["legacy_import"],
        }
    }
}

/// The matrix answer for native Saved-list synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeSavedSupport {
    /// Always [`SupportStatus::NotSupported`] while no supported surface exists.
    pub status: SupportStatus,
    /// Why the answer is what it is, written for operators and reviews.
    pub reason: &'static str,
}

/// Native Saved-list synchronization of a personal account.
///
/// Threads exposes no supported API surface that reads a personal account's
/// native Saved list, so this service states the limitation instead of
/// approximating it from captures or exports.
pub const NATIVE_SAVED_LIST_SYNC: NativeSavedSupport = NativeSavedSupport {
    status: SupportStatus::NotSupported,
    reason: "no supported provider surface exposes the personal Saved list",
};

/// What Threads last reported about a source's existence or accessibility.
///
/// Kept strictly apart from [`PreservationState`]: this vocabulary records the
/// provider's side only, mirrored value for value from the schema CHECK shared
/// by `posts.upstream_status` and `tombstones.availability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpstreamAvailability {
    /// The source resolved normally when last observed.
    Active,
    /// The provider stated or implied the source no longer exists.
    Deleted,
    /// The source exists but denies access to this observer.
    PrivateOrInaccessible,
    /// The author's account is gone, suspended, or unreachable.
    AuthorUnavailable,
    /// Failed transiently; retrying later may succeed.
    TemporarilyUnavailable,
    /// No usable observation exists yet.
    Unknown,
}

impl UpstreamAvailability {
    /// Every availability value, mirroring the schema CHECK exactly.
    pub const ALL: [UpstreamAvailability; 6] = [
        UpstreamAvailability::Active,
        UpstreamAvailability::Deleted,
        UpstreamAvailability::PrivateOrInaccessible,
        UpstreamAvailability::AuthorUnavailable,
        UpstreamAvailability::TemporarilyUnavailable,
        UpstreamAvailability::Unknown,
    ];

    /// The `snake_case` wire value stored in `posts.upstream_status` and
    /// `tombstones.availability`, equal to the schema CHECK value for value.
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Deleted => "deleted",
            Self::PrivateOrInaccessible => "private_or_inaccessible",
            Self::AuthorUnavailable => "author_unavailable",
            Self::TemporarilyUnavailable => "temporarily_unavailable",
            Self::Unknown => "unknown",
        }
    }
}

/// What Ratatoskr holds locally for a source.
///
/// Independent of [`UpstreamAvailability`] by construction: observing deletion
/// upstream never demotes content already preserved, and nothing here implies
/// anything about what the platform currently serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreservationState {
    /// Content bytes and metadata preserved locally.
    ContentPreserved,
    /// Metadata and raw evidence preserved; content bytes not archived.
    MetadataOnly,
    /// Only a user-uploaded artifact is held, with its own provenance.
    UserArtifactOnly,
    /// Nothing beyond the capture record itself.
    NothingPreserved,
}

impl PreservationState {
    /// Every preservation state.
    pub const ALL: [PreservationState; 4] = [
        PreservationState::ContentPreserved,
        PreservationState::MetadataOnly,
        PreservationState::UserArtifactOnly,
        PreservationState::NothingPreserved,
    ];
}

/// Apply an availability observation to a preservation state.
///
/// The rule is identity on purpose: observations describe what the provider
/// reported, and no observation — including a proven deletion — is evidence
/// about what Ratatoskr preserved. Demotion happens only through explicit user
/// action, so absence in a later export or a failed resolution can never delete
/// an archived capture (`AGENTS.md`, "absence of a category or object in one
/// export does not prove deletion").
#[must_use]
pub fn retention_after_observation(
    current: PreservationState,
    observed: UpstreamAvailability,
) -> PreservationState {
    let _ = observed;
    current
}
