//! Explicit capture intake: validated requests, stored capture records, and
//! truthful unavailability observations.
//!
//! A validated [`CaptureRequest`] carries everything one explicit capture
//! needs and nothing it may claim: the owner, an idempotency key, the raw URL
//! text byte-for-byte beside its canonical permalink, an optional note, and
//! the acquisition method paired with the client source under the documented
//! lane mapping. The request type deliberately carries no saved-authority and
//! no status field: what a capture proves is fixed by this lane, not chosen
//! by callers, so the misrepresentation of a local save as authoritative
//! platform state is physically unrepresentable (design decision D5).
//!
//! Hostile input is bounded by named rules before anything is stored
//! (design decision D10): the method/client pairing, the idempotency key
//! length, and the URL grammar are checked at construction, and nothing is
//! truncated or silently repaired.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::capability::SavedAuthority;
use crate::database::{Database, PersistenceError};
use crate::permalink::{CanonicalizedUrl, Permalink, PermalinkError};
use crate::publishing;

/// The longest idempotency key intake accepts, in bytes.
const MAX_IDEMPOTENCY_KEY_LEN: usize = 256;

/// The longest single unavailability-observation field intake accepts, in
/// bytes.
const MAX_OBSERVATION_FIELD_LEN: usize = 128;

/// Enforces the documented method/client pairing rule.
fn ensure_paired(
    acquisition_method: CaptureMethod,
    client_source: ClientSource,
) -> Result<(), CaptureError> {
    let paired = match acquisition_method {
        CaptureMethod::ShareExtension => matches!(
            client_source,
            ClientSource::IosShareExtension | ClientSource::AndroidShareTarget
        ),
        CaptureMethod::BrowserExtension => client_source == ClientSource::BrowserExtension,
        CaptureMethod::TelegramCapture => client_source == ClientSource::Telegram,
    };
    if paired {
        Ok(())
    } else {
        Err(CaptureError::PairingMismatch {
            acquisition_method,
            client_source,
        })
    }
}

/// Enforces the idempotency-key size rule.
fn ensure_key_bounded(idempotency_key: &str) -> Result<(), CaptureError> {
    let len = idempotency_key.len();
    if len == 0 {
        Err(CaptureError::EmptyIdempotencyKey)
    } else if len > MAX_IDEMPOTENCY_KEY_LEN {
        Err(CaptureError::IdempotencyKeyTooLong { len })
    } else {
        Ok(())
    }
}

/// Enforces one observation-field size rule; `field` names the field in the
/// refusal so callers can fix inputs one defect at a time.
fn ensure_observation_field_bounded(field: &'static str, value: &str) -> Result<(), CaptureError> {
    let len = value.len();
    if len == 0 || len > MAX_OBSERVATION_FIELD_LEN {
        Err(CaptureError::InvalidObservationField { field, len })
    } else {
        Ok(())
    }
}

fn parse_capture_method(value: &str) -> Option<CaptureMethod> {
    match value {
        "share_extension" => Some(CaptureMethod::ShareExtension),
        "browser_extension" => Some(CaptureMethod::BrowserExtension),
        "telegram_capture" => Some(CaptureMethod::TelegramCapture),
        _ => None,
    }
}

fn parse_client_source(value: &str) -> Option<ClientSource> {
    match value {
        "ios_share_extension" => Some(ClientSource::IosShareExtension),
        "android_share_target" => Some(ClientSource::AndroidShareTarget),
        "browser_extension" => Some(ClientSource::BrowserExtension),
        "telegram" => Some(ClientSource::Telegram),
        _ => None,
    }
}

fn parse_capture_status(value: &str) -> Option<CaptureStatus> {
    match value {
        "accepted" => Some(CaptureStatus::Accepted),
        "resolved" => Some(CaptureStatus::Resolved),
        "unavailable" => Some(CaptureStatus::Unavailable),
        "failed" => Some(CaptureStatus::Failed),
        _ => None,
    }
}

/// Builds a persistence failure naming the column whose stored value fell
/// outside its closed vocabulary; the CHECK constraints make this reachable
/// only through out-of-band writes.
fn vocabulary_error(column: &'static str, value: &str) -> CaptureError {
    CaptureError::Persistence(PersistenceError::Query(sqlx::Error::ColumnDecode {
        index: column.to_owned(),
        source: format!("stored value {value:?} violates the closed threads_archive vocabulary")
            .into(),
    }))
}

/// How an explicit capture reached this service.
///
/// The inventory is closed to the three wire methods the capability matrix
/// assigns to explicit capture; every value equals the `threads_archive`
/// CHECK vocabulary for the acquisition-method columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureMethod {
    /// The user pushed the post through a mobile share target.
    ShareExtension,
    /// The user pushed the post through the browser extension.
    BrowserExtension,
    /// The user pushed the post through Telegram.
    TelegramCapture,
}

impl CaptureMethod {
    /// The `snake_case` wire value stored in provenance columns, equal to the
    /// schema CHECK vocabulary value for value.
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::ShareExtension => "share_extension",
            Self::BrowserExtension => "browser_extension",
            Self::TelegramCapture => "telegram_capture",
        }
    }
}

/// The client surface a capture was submitted from.
///
/// Every value equals the `captures.client_source` CHECK vocabulary; each
/// value pairs with exactly one [`CaptureMethod`] under the documented lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientSource {
    /// The iOS share extension.
    IosShareExtension,
    /// The Android share target.
    AndroidShareTarget,
    /// The browser extension.
    BrowserExtension,
    /// Telegram.
    Telegram,
}

impl ClientSource {
    /// The `snake_case` wire value stored in the `client_source` column,
    /// equal to the schema CHECK vocabulary value for value.
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::IosShareExtension => "ios_share_extension",
            Self::AndroidShareTarget => "android_share_target",
            Self::BrowserExtension => "browser_extension",
            Self::Telegram => "telegram",
        }
    }
}

/// The lifecycle state of a stored capture.
///
/// Every value equals the `captures.status` CHECK vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureStatus {
    /// Stored and awaiting resolution outcome.
    Accepted,
    /// Resolved into a provider post representation.
    Resolved,
    /// Evidence-backed unavailability was recorded against the capture.
    Unavailable,
    /// Intake or resolution failed terminally.
    Failed,
}

impl CaptureStatus {
    /// The `snake_case` wire value stored in the `status` column, equal to
    /// the schema CHECK vocabulary value for value.
    #[must_use]
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Resolved => "resolved",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
        }
    }
}

/// Why intake refused one capture request.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CaptureError {
    /// The acquisition method and client source do not pair under the
    /// documented capture lanes.
    #[error(
        "the pairing rule requires share_extension with ios_share_extension or \
         android_share_target, browser_extension with browser_extension, and \
         telegram_capture with telegram; got {acquisition_method:?} with {client_source:?}"
    )]
    PairingMismatch {
        /// The requested acquisition method.
        acquisition_method: CaptureMethod,
        /// The client source that does not pair with it.
        client_source: ClientSource,
    },
    /// The idempotency key was empty.
    #[error("the idempotency key must be 1..=256 bytes")]
    EmptyIdempotencyKey,
    /// The idempotency key exceeded its length cap.
    #[error("the idempotency key must be 1..=256 bytes (got {len})")]
    IdempotencyKeyTooLong {
        /// The offending key length in bytes.
        len: usize,
    },
    /// An unavailability-observation field violated its size rule.
    #[error("observation fields must be 1..=128 bytes ({field} was {len})")]
    InvalidObservationField {
        /// Which field broke the rule (`reason_code` or `resolver_version`).
        field: &'static str,
        /// The offending field length in bytes.
        len: usize,
    },
    /// The submitted URL text failed permalink canonicalization; the wrapped
    /// error names the violated permalink rule.
    #[error("the submitted URL is not a Threads post permalink")]
    InvalidUrl(#[from] PermalinkError),
    /// An archive-owned query behind the capture failed.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    /// No stored capture exists under the referenced id.
    #[error("no capture exists for id {0}")]
    UnknownCapture(Uuid),
}

/// One validated explicit-capture submission.
///
/// Built only through [`CaptureRequest::try_new`], which enforces the lane
/// pairing, the idempotency-key rule, and the permalink grammar, and retains
/// the raw URL text byte-for-byte beside its canonical permalink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    user_ref: Uuid,
    idempotency_key: String,
    url: CanonicalizedUrl,
    note: Option<String>,
    acquisition_method: CaptureMethod,
    client_source: ClientSource,
}

impl CaptureRequest {
    /// Validates one submission against the intake rules and keeps both URL
    /// forms: the canonical permalink and the submitted text byte-for-byte.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::PairingMismatch`] when the method and client
    /// source are not a documented lane, the idempotency-key errors when the
    /// key is empty or longer than 256 bytes, and
    /// [`CaptureError::InvalidUrl`] when the URL text fails permalink
    /// canonicalization.
    pub fn try_new(
        user_ref: Uuid,
        idempotency_key: String,
        raw_url: &str,
        note: Option<String>,
        acquisition_method: CaptureMethod,
        client_source: ClientSource,
    ) -> Result<Self, CaptureError> {
        ensure_paired(acquisition_method, client_source)?;
        ensure_key_bounded(&idempotency_key)?;
        let url = CanonicalizedUrl::try_from(raw_url)?;
        Ok(Self {
            user_ref,
            idempotency_key,
            url,
            note,
            acquisition_method,
            client_source,
        })
    }

    /// The Ratatoskr owner of the capture.
    #[must_use]
    pub const fn user_ref(&self) -> Uuid {
        self.user_ref
    }

    /// The caller-supplied deduplication key.
    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    /// The submitted URL text, unchanged.
    #[must_use]
    pub fn raw_url(&self) -> &str {
        self.url.original()
    }

    /// The canonical permalink derived from the submitted text.
    #[must_use]
    pub fn canonical_url(&self) -> &Permalink {
        self.url.permalink()
    }

    /// The optional user note carried alongside the capture.
    #[must_use]
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The requested acquisition method.
    #[must_use]
    pub const fn acquisition_method(&self) -> CaptureMethod {
        self.acquisition_method
    }

    /// The client source the submission came from.
    #[must_use]
    pub const fn client_source(&self) -> ClientSource {
        self.client_source
    }
}

/// The stored view of one explicit capture.
///
/// Every field is the database row made typed. [`CaptureRecord::accepted`] is
/// the sanctioned way to produce one from a validated request, and it sets
/// [`CaptureRecord::saved_authority`] to
/// [`SavedAuthority::ExplicitUserCapture`] unconditionally: an explicit
/// capture proves the user saved the item to Ratatoskr, never membership in a
/// native platform list, and the store writes that same value so no code path
/// can widen it (design decision D5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRecord {
    /// The application-minted `UUIDv7` identity of the capture row.
    pub capture_id: Uuid,
    /// The Ratatoskr owner of the capture.
    pub user_ref: Uuid,
    /// The provider post the capture resolved to, while still open.
    pub post_id: Option<Uuid>,
    /// The caller-supplied deduplication key.
    pub idempotency_key: String,
    /// The canonical permalink of the captured post.
    pub canonical_url: Permalink,
    /// The submitted URL text, preserved byte-for-byte.
    pub original_url: String,
    /// How the capture reached this service.
    pub acquisition_method: CaptureMethod,
    /// Always [`SavedAuthority::ExplicitUserCapture`]: set by the store from
    /// the lane itself, never from request data, so a capture cannot claim
    /// more authority than its lane proves.
    pub saved_authority: SavedAuthority,
    /// The client surface the submission came from.
    pub client_source: ClientSource,
    /// The lifecycle state of the capture.
    pub status: CaptureStatus,
    /// The optional user note.
    pub note: Option<String>,
    /// When Ratatoskr accepted the capture; stamped once and never rewritten.
    pub captured_at: DateTime<Utc>,
}

impl CaptureRecord {
    /// Builds the accepted stored view of one validated request.
    ///
    /// The authority is pinned here rather than taken from the request: see
    /// the type documentation for why no request field may influence it.
    #[must_use]
    pub fn accepted(
        request: &CaptureRequest,
        capture_id: Uuid,
        captured_at: DateTime<Utc>,
    ) -> Self {
        Self {
            capture_id,
            user_ref: request.user_ref(),
            post_id: None,
            idempotency_key: request.idempotency_key().to_owned(),
            canonical_url: request.canonical_url().clone(),
            original_url: request.raw_url().to_owned(),
            acquisition_method: request.acquisition_method(),
            saved_authority: SavedAuthority::ExplicitUserCapture,
            client_source: request.client_source(),
            status: CaptureStatus::Accepted,
            note: request.note().map(str::to_owned),
            captured_at,
        }
    }
}

/// What intake learned when a captured post could not be resolved.
///
/// The variants carry exactly the evidence their class proves: deletion and
/// privacy observations name the provider state (and become tombstones), a
/// resolver failure names the resolver that produced nothing (missing output
/// is never deletion evidence, design decision D6).
///
/// Build instances through the validating associated functions
/// [`UnavailabilityObservation::deleted`],
/// [`UnavailabilityObservation::private_or_inaccessible`], and
/// [`UnavailabilityObservation::resolver_failed`]; direct variant
/// construction bypasses the field-size rules and is not sanctioned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnavailabilityObservation {
    /// The provider stated or implied the post no longer exists.
    Deleted {
        /// Why deletion is claimed, e.g. a provider error code.
        reason_code: String,
    },
    /// The post exists but denies access to this observer.
    PrivateOrInaccessible {
        /// Why access is denied, e.g. a provider response marker.
        reason_code: String,
    },
    /// The supported public resolver produced no output; no provider state
    /// was observed.
    ResolverFailed {
        /// The version of the resolver that failed.
        resolver_version: String,
    },
}

impl UnavailabilityObservation {
    /// Records an observed deletion.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::InvalidObservationField`] when `reason_code`
    /// is empty or longer than 128 bytes.
    pub fn deleted(reason_code: String) -> Result<Self, CaptureError> {
        ensure_observation_field_bounded("reason_code", &reason_code)?;
        Ok(Self::Deleted { reason_code })
    }

    /// Records an observed private-or-inaccessible state.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::InvalidObservationField`] when `reason_code`
    /// is empty or longer than 128 bytes.
    pub fn private_or_inaccessible(reason_code: String) -> Result<Self, CaptureError> {
        ensure_observation_field_bounded("reason_code", &reason_code)?;
        Ok(Self::PrivateOrInaccessible { reason_code })
    }

    /// Records a resolver failure.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::InvalidObservationField`] when
    /// `resolver_version` is empty or longer than 128 bytes.
    pub fn resolver_failed(resolver_version: String) -> Result<Self, CaptureError> {
        ensure_observation_field_bounded("resolver_version", &resolver_version)?;
        Ok(Self::ResolverFailed { resolver_version })
    }
}

/// The outcome of one idempotent submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitOutcome {
    /// The submission stored a new capture row.
    Created(CaptureRecord),
    /// The submission replayed an already-stored capture unchanged; the
    /// stored record wins over the replayed request in every field.
    Replayed(CaptureRecord),
}

/// Submits explicit captures and records unavailability observations against
/// the owned `threads_archive` schema.
#[derive(Debug)]
pub struct CaptureStore<'a> {
    pool: &'a PgPool,
}

impl<'a> CaptureStore<'a> {
    /// Builds a store over the pool owned by `database`.
    #[must_use]
    pub const fn new(database: &'a Database) -> Self {
        Self {
            pool: database.pool(),
        }
    }

    /// Stores one validated capture idempotently.
    ///
    /// The first submission under a `(user_ref, idempotency_key)` pair
    /// inserts a new capture stamped by the acceptance clock and returns it
    /// as [`SubmitOutcome::Created`]. Any later submission under the same
    /// pair returns the stored record unchanged as
    /// [`SubmitOutcome::Replayed`] — the stored row wins in every field,
    /// including when the replayed raw URL text differs but canonicalizes to
    /// the same permalink. A different key over one permalink is a distinct
    /// intent and creates its own capture (design decision D4).
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Persistence`] when an archive-owned query
    /// fails.
    pub async fn submit(&self, request: &CaptureRequest) -> Result<SubmitOutcome, CaptureError> {
        let inserted: Option<(Uuid, DateTime<Utc>)> = sqlx::query_as(
            "insert into threads_archive.captures \
             (capture_id, user_ref, idempotency_key, canonical_url, original_url, \
              acquisition_method, saved_authority, client_source, status, note, captured_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now()) \
             on conflict (user_ref, idempotency_key) do nothing \
             returning capture_id, captured_at",
        )
        .bind(Uuid::now_v7())
        .bind(request.user_ref())
        .bind(request.idempotency_key())
        .bind(request.canonical_url().as_str())
        .bind(request.raw_url())
        .bind(request.acquisition_method().wire_value())
        .bind(SavedAuthority::ExplicitUserCapture.wire_value())
        .bind(request.client_source().wire_value())
        .bind(CaptureStatus::Accepted.wire_value())
        .bind(request.note())
        .fetch_optional(self.pool)
        .await
        .map_err(PersistenceError::Query)?;

        if let Some((capture_id, captured_at)) = inserted {
            return Ok(SubmitOutcome::Created(CaptureRecord::accepted(
                request,
                capture_id,
                captured_at,
            )));
        }

        let row: Option<CaptureRow> = sqlx::query_as(REPLAY_QUERY)
            .bind(request.user_ref())
            .bind(request.idempotency_key())
            .fetch_optional(self.pool)
            .await
            .map_err(PersistenceError::Query)?;
        let record = record_from_row(row.ok_or_else(replayed_row_vanished)?)?;
        Ok(SubmitOutcome::Replayed(record))
    }

    /// Records what intake learned when a captured post could not be
    /// resolved, mapping evidence classes to honest fallback shapes (design
    /// decision D6).
    ///
    /// An observed deletion or private-or-inaccessible state writes a
    /// tombstone naming the capture as subject plus a resolution row with
    /// outcome `unavailable`, and marks the capture `unavailable`. A resolver
    /// failure writes only a resolution row with outcome `resolver_failed` —
    /// never a tombstone — because missing output is not deletion evidence;
    /// the capture stays `accepted`. In every shape the capture's note,
    /// captured time, original URL text, and canonical permalink survive
    /// untouched.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::UnknownCapture`] when no stored capture
    /// exists under `capture_id`, and [`CaptureError::Persistence`] when an
    /// archive-owned query fails.
    pub async fn record_observation(
        &self,
        capture_id: Uuid,
        observation: &UnavailabilityObservation,
    ) -> Result<(), CaptureError> {
        let known: Option<i32> =
            sqlx::query_scalar("select 1 from threads_archive.captures where capture_id = $1")
                .bind(capture_id)
                .fetch_optional(self.pool)
                .await
                .map_err(PersistenceError::Query)?;
        if known.is_none() {
            return Err(CaptureError::UnknownCapture(capture_id));
        }

        match observation {
            UnavailabilityObservation::Deleted { reason_code } => {
                self.mark_unavailable(capture_id, "deleted", reason_code)
                    .await
            }
            UnavailabilityObservation::PrivateOrInaccessible { reason_code } => {
                self.mark_unavailable(capture_id, "private_or_inaccessible", reason_code)
                    .await
            }
            UnavailabilityObservation::ResolverFailed { resolver_version } => {
                sqlx::query(
                    "insert into threads_archive.capture_resolutions \
                     (resolution_id, capture_id, outcome, resolver_version, raw_object_id, \
                      observed_at) \
                     values ($1, $2, 'resolver_failed', $3, null, now())",
                )
                .bind(Uuid::now_v7())
                .bind(capture_id)
                .bind(resolver_version)
                .execute(self.pool)
                .await
                .map_err(PersistenceError::Query)?;
                Ok(())
            }
        }
    }

    /// Writes the tombstone-backed fallback for an evidence-backed provider
    /// observation: status flip, tombstone naming the capture as subject, and
    /// the `unavailable` resolution row — in one transaction.
    async fn mark_unavailable(
        &self,
        capture_id: Uuid,
        availability: &'static str,
        reason_code: &str,
    ) -> Result<(), CaptureError> {
        let mut transaction = self.pool.begin().await.map_err(PersistenceError::Query)?;
        let post_id: Option<Uuid> = sqlx::query_scalar(
            "select post_id from threads_archive.captures where capture_id = $1 for update",
        )
        .bind(capture_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;

        sqlx::query(
            "update threads_archive.captures set status = 'unavailable' where capture_id = $1",
        )
        .bind(capture_id)
        .execute(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;

        if let Some(post_id) = post_id {
            sqlx::query(
                "update threads_archive.posts set upstream_status = $2, updated_at = now() \
                 where post_id = $1",
            )
            .bind(post_id)
            .bind(availability)
            .execute(&mut *transaction)
            .await
            .map_err(PersistenceError::Query)?;
        }

        sqlx::query(
            "insert into threads_archive.tombstones \
             (tombstone_id, post_id, capture_id, availability, reason_code, resolver_version, \
              observed_at) \
             values ($1, $2, $3, $4, $5, null, now())",
        )
        .bind(Uuid::now_v7())
        .bind(post_id)
        .bind(capture_id)
        .bind(availability)
        .bind(reason_code)
        .execute(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;

        if post_id.is_some() {
            publishing::append_fact(&mut transaction, capture_id)
                .await
                .map_err(|error| {
                    CaptureError::Persistence(PersistenceError::Query(sqlx::Error::Protocol(
                        error.to_string(),
                    )))
                })?;
        }

        sqlx::query(
            "insert into threads_archive.capture_resolutions \
             (resolution_id, capture_id, outcome, resolver_version, raw_object_id, observed_at) \
             values ($1, $2, 'unavailable', null, null, now())",
        )
        .bind(Uuid::now_v7())
        .bind(capture_id)
        .execute(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;

        transaction
            .commit()
            .await
            .map_err(|error| CaptureError::Persistence(PersistenceError::Query(error)))
    }
}

/// One stored capture row read back for a replay, in schema column order.
type CaptureRow = (
    Uuid,
    Uuid,
    Option<Uuid>,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    DateTime<Utc>,
);

/// The columns of one stored capture, keyed by owner and idempotency key.
const REPLAY_QUERY: &str = "select capture_id, user_ref, post_id, idempotency_key, \
     canonical_url, original_url, acquisition_method, saved_authority, client_source, \
     status, note, captured_at \
     from threads_archive.captures where user_ref = $1 and idempotency_key = $2";

/// A conflict was recorded but the stored row disappeared before the replay
/// read; nothing in this bounded context deletes captures, so this names an
/// out-of-band write rather than being retried blindly.
fn replayed_row_vanished() -> CaptureError {
    CaptureError::Persistence(PersistenceError::Query(sqlx::Error::Configuration(
        "a replayed capture vanished between the idempotency conflict and its read".into(),
    )))
}

/// Builds the typed record from one stored row. Every wire value must sit in
/// its closed vocabulary and the canonical URL must itself satisfy the
/// permalink grammar; anything else is storage corruption, not caller input.
fn record_from_row(row: CaptureRow) -> Result<CaptureRecord, CaptureError> {
    let (
        capture_id,
        user_ref,
        post_id,
        idempotency_key,
        canonical_url,
        original_url,
        acquisition_method,
        saved_authority,
        client_source,
        status,
        note,
        captured_at,
    ) = row;

    if saved_authority != SavedAuthority::ExplicitUserCapture.wire_value() {
        return Err(vocabulary_error(
            "captures.saved_authority",
            &saved_authority,
        ));
    }
    let Some(acquisition_method) = parse_capture_method(&acquisition_method) else {
        return Err(vocabulary_error(
            "captures.acquisition_method",
            &acquisition_method,
        ));
    };
    let Some(client_source) = parse_client_source(&client_source) else {
        return Err(vocabulary_error("captures.client_source", &client_source));
    };
    let Some(status) = parse_capture_status(&status) else {
        return Err(vocabulary_error("captures.status", &status));
    };
    let canonicalized = CanonicalizedUrl::try_from(canonical_url.as_str())
        .map_err(|_| vocabulary_error("captures.canonical_url", &canonical_url))?;

    Ok(CaptureRecord {
        capture_id,
        user_ref,
        post_id,
        idempotency_key,
        canonical_url: canonicalized.permalink().clone(),
        original_url,
        acquisition_method,
        saved_authority: SavedAuthority::ExplicitUserCapture,
        client_source,
        status,
        note,
        captured_at,
    })
}
