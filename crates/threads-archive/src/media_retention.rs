//! Explicit, fail-closed policy boundary for provider-media byte archival.

use crate::public_resolution::{PublicResolutionError, RawObjectStore};
use crate::{Database, PersistenceError};
use sha2::{Digest as _, Sha256};
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

/// MIME values currently eligible for provider-media archival.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovedMediaMime {
    /// JPEG image bytes.
    ImageJpeg,
    /// PNG image bytes.
    ImagePng,
    /// MP4 video bytes.
    VideoMp4,
}

impl ApprovedMediaMime {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ImageJpeg => "image/jpeg",
            Self::ImagePng => "image/png",
            Self::VideoMp4 => "video/mp4",
        }
    }
}

/// One already-acquired response together with the immutable lease evidence.
#[derive(Debug)]
pub struct AcquiredMedia<'a> {
    /// Final URL after redirects.
    pub final_url: &'a str,
    /// Validated response content type.
    pub content_type: ApprovedMediaMime,
    /// Response byte length declared by the transport.
    pub declared_bytes: u64,
    /// Digest authorized by the caller's immutable observation.
    pub expected_digest: [u8; 32],
    /// Bounded synthetic or transport-acquired body.
    pub body: &'a [u8],
}

/// A post-fetch verification refusal that keeps the record metadata-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaVerificationReason {
    /// The final URL is not HTTPS.
    FinalUrlNotHttps,
    /// Actual bytes disagree with the response length.
    ContentLengthMismatch,
    /// Actual bytes disagree with the expected digest.
    ContentDigestMismatch,
    /// Actual bytes exceed the immutable fetch lease.
    ResponseBudgetExceeded,
}

/// Verified media storage evidence safe to attach to a media row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedMedia {
    /// Service-owned content-addressed reference.
    pub blob_ref: String,
    /// SHA-256 digest bytes.
    pub content_hash: Vec<u8>,
    /// Verified byte length.
    pub byte_size: i64,
    /// Approved MIME.
    pub media_type: &'static str,
}

/// Result of verifying and attempting to archive one acquired response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaArchiveOutcome {
    /// Verification refused the bytes without a durable partial object.
    MetadataOnly(MediaVerificationReason),
    /// Fully verified immutable bytes were stored.
    Archived(ArchivedMedia),
}

/// Reference-safe cleanup decision for one expiring media row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobCleanupPlan {
    /// Another database-wide live reference still requires the object.
    RetainShared {
        /// References excluding the expiring media row.
        live_references: i64,
    },
    /// No live reference remains, so durable deletion may be scheduled.
    ScheduleDelete {
        /// Digest-bound service-owned reference.
        blob_ref: String,
        /// Expected digest used by `delete_if_matches`.
        content_hash: Vec<u8>,
    },
}

/// Safe bounded failure vocabulary for post-commit `BlobStore` deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobDeletionFailure {
    /// The service-owned storage path could not be read or changed.
    StorageUnavailable,
    /// The object bytes did not match the task's expected digest.
    DigestMismatch,
    /// A database-wide live reference still requires the object.
    StillReferenced,
}

impl BlobDeletionFailure {
    const fn as_str(self) -> &'static str {
        match self {
            Self::StorageUnavailable => "storage_unavailable",
            Self::DigestMismatch => "digest_mismatch",
            Self::StillReferenced => "still_referenced",
        }
    }
}

impl BlobDeletionBackend for RawObjectStore {
    fn delete_if_matches<'a>(
        &'a self,
        blob_ref: &'a str,
        content_hash: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<(), BlobDeletionFailure>> + Send + 'a>> {
        Box::pin(async move {
            RawObjectStore::delete_if_matches(self, blob_ref, content_hash)
                .await
                .map_err(|error| match error {
                    PublicResolutionError::RawDigestMismatch => BlobDeletionFailure::DigestMismatch,
                    _ => BlobDeletionFailure::StorageUnavailable,
                })
        })
    }
}

/// Async digest-bound `BlobStore` deletion seam used by the durable worker.
pub trait BlobDeletionBackend {
    /// Deletes the named object only when its current bytes match `content_hash`.
    fn delete_if_matches<'a>(
        &'a self,
        blob_ref: &'a str,
        content_hash: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<(), BlobDeletionFailure>> + Send + 'a>>;
}

/// Durable state returned by one idempotent blob-task attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobDeletionTaskOutcome {
    /// The task remains durable for retry.
    Pending(BlobDeletionFailure),
    /// The object is verified absent and the task is terminal.
    Complete,
}

/// Processes one durable `BlobStore` deletion task.
///
/// # Errors
///
/// Returns a persistence failure when task state cannot be loaded or advanced.
pub async fn process_blob_deletion_task<B>(
    database: &Database,
    backend: &B,
    task_id: Uuid,
) -> Result<BlobDeletionTaskOutcome, PersistenceError>
where
    B: BlobDeletionBackend + Sync,
{
    let (blob_ref, content_hash, state): (String, Vec<u8>, String) = sqlx::query_as(
        "select blob_ref, content_hash, state from threads_archive.blob_deletion_tasks \
         where task_id = $1",
    )
    .bind(task_id)
    .fetch_one(database.pool())
    .await
    .map_err(PersistenceError::Query)?;
    if state == "complete" {
        return Ok(BlobDeletionTaskOutcome::Complete);
    }

    let (live_references,): (i64,) = sqlx::query_as(
        "select coalesce(sum(reference_count), 0)::bigint from (\
           select count(*)::bigint as reference_count from threads_archive.raw_objects \
            where blob_ref = $1 and content_hash = $2 \
           union all \
           select count(*)::bigint from threads_archive.media \
            where blob_ref = $1 and content_hash = $2 \
           union all \
           select count(*)::bigint from threads_archive.export_runs \
            where archive_blob_ref = $1 and archive_hash = $2\
         ) as live_references",
    )
    .bind(&blob_ref)
    .bind(&content_hash)
    .fetch_one(database.pool())
    .await
    .map_err(PersistenceError::Query)?;
    if live_references > 0 {
        record_blob_failure(database, task_id, BlobDeletionFailure::StillReferenced).await?;
        return Ok(BlobDeletionTaskOutcome::Pending(
            BlobDeletionFailure::StillReferenced,
        ));
    }

    if let Err(failure) = backend.delete_if_matches(&blob_ref, &content_hash).await {
        record_blob_failure(database, task_id, failure).await?;
        return Ok(BlobDeletionTaskOutcome::Pending(failure));
    }

    sqlx::query(
        "update threads_archive.blob_deletion_tasks \
         set state = 'complete', attempt_count = attempt_count + 1, last_failure_class = null, \
             completed_at = now(), updated_at = now() \
         where task_id = $1 and state = 'pending'",
    )
    .bind(task_id)
    .execute(database.pool())
    .await
    .map_err(PersistenceError::Query)?;
    Ok(BlobDeletionTaskOutcome::Complete)
}

async fn record_blob_failure(
    database: &Database,
    task_id: Uuid,
    failure: BlobDeletionFailure,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "update threads_archive.blob_deletion_tasks \
         set attempt_count = attempt_count + 1, last_failure_class = $2, updated_at = now() \
         where task_id = $1 and state = 'pending'",
    )
    .bind(task_id)
    .bind(failure.as_str())
    .execute(database.pool())
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

/// Plans expiry of one archived media reference without mutating SQL or `BlobStore` state.
///
/// # Errors
///
/// Returns a typed persistence failure when the row cannot be loaded.
pub async fn plan_media_reference_expiry(
    database: &Database,
    media_id: Uuid,
) -> Result<BlobCleanupPlan, PersistenceError> {
    let (blob_ref, content_hash): (String, Vec<u8>) = sqlx::query_as(
        "select blob_ref, content_hash from threads_archive.media where media_id = $1",
    )
    .bind(media_id)
    .fetch_one(database.pool())
    .await
    .map_err(PersistenceError::Query)?;
    let (live_references,): (i64,) = sqlx::query_as(
        "select coalesce(sum(reference_count), 0)::bigint from (\
           select count(*)::bigint as reference_count from threads_archive.raw_objects \
            where blob_ref = $1 and content_hash = $2 \
           union all \
           select count(*)::bigint from threads_archive.media \
            where blob_ref = $1 and content_hash = $2 and media_id <> $3 \
           union all \
           select count(*)::bigint from threads_archive.export_runs \
            where archive_blob_ref = $1 and archive_hash = $2\
         ) as live_references",
    )
    .bind(&blob_ref)
    .bind(&content_hash)
    .bind(media_id)
    .fetch_one(database.pool())
    .await
    .map_err(PersistenceError::Query)?;
    if live_references > 0 {
        return Ok(BlobCleanupPlan::RetainShared { live_references });
    }
    Ok(BlobCleanupPlan::ScheduleDelete {
        blob_ref,
        content_hash,
    })
}

/// Verifies and stores one response within its immutable fetch lease.
///
/// # Errors
///
/// Returns a typed storage failure when the service-owned object store fails.
pub async fn archive_acquired_media(
    store: &RawObjectStore,
    lease: MediaFetchLease,
    response: AcquiredMedia<'_>,
) -> Result<MediaArchiveOutcome, PublicResolutionError> {
    let final_url = reqwest::Url::parse(response.final_url)
        .map_err(|_| PublicResolutionError::UnsupportedEndpoint)?;
    if final_url.scheme() != "https" {
        return Ok(MediaArchiveOutcome::MetadataOnly(
            MediaVerificationReason::FinalUrlNotHttps,
        ));
    }
    let actual_bytes =
        u64::try_from(response.body.len()).map_err(|_| PublicResolutionError::ResponseTooLarge)?;
    if actual_bytes > lease.max_bytes {
        return Ok(MediaArchiveOutcome::MetadataOnly(
            MediaVerificationReason::ResponseBudgetExceeded,
        ));
    }
    if actual_bytes != response.declared_bytes {
        return Ok(MediaArchiveOutcome::MetadataOnly(
            MediaVerificationReason::ContentLengthMismatch,
        ));
    }
    if Sha256::digest(response.body).as_slice() != response.expected_digest {
        return Ok(MediaArchiveOutcome::MetadataOnly(
            MediaVerificationReason::ContentDigestMismatch,
        ));
    }
    let mut body = response.body;
    let stored = store
        .store_stream(&mut body, lease.max_bytes, response.content_type.as_str())
        .await?;
    Ok(MediaArchiveOutcome::Archived(ArchivedMedia {
        blob_ref: stored.blob_ref,
        content_hash: stored.content_hash,
        byte_size: stored.byte_size,
        media_type: stored.media_type,
    }))
}

/// Why an observation remains metadata-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataOnlyReason {
    /// No authorized policy requested byte archival.
    PolicyNotAuthorized,
    /// The acquisition lane cannot authorize provider-media archival.
    AcquisitionNotEligible,
    /// Rights or permission to retain bytes was not established.
    RightsUnknown,
    /// Media kind eligibility was not established.
    KindUnknown,
    /// Response MIME eligibility was not established.
    MimeUnknown,
    /// The provider URL lifetime cannot cover the fetch lease.
    UrlLifetimeUnknown,
    /// The provider object size is unknown.
    ObjectSizeUnknown,
    /// The object exceeds its per-object byte ceiling.
    ObjectBudgetExceeded,
    /// Remaining owner storage is unknown.
    OwnerBudgetUnknown,
    /// Remaining owner storage cannot fit the object.
    OwnerBudgetExceeded,
    /// This policy class requires a separate explicit user action.
    ExplicitActionRequired,
}

/// Immutable finite permission to begin one media fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaFetchLease {
    /// Maximum response bytes accepted for this object.
    pub max_bytes: u64,
}

/// Persisted policy result for one provider-media observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaRetentionDecision {
    /// Persist metadata only and do not start network I/O.
    MetadataOnly(MetadataOnlyReason),
    /// Byte archival is authorized within this immutable lease.
    Archive(MediaFetchLease),
}

/// Inputs required before provider-media bytes may be requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaPolicyInput {
    /// Whether an authorized retention policy explicitly requests bytes.
    pub archive_requested: bool,
    /// Whether the acquisition lane permits provider-media byte archival.
    pub acquisition_eligible: bool,
    /// Whether rights/permission are affirmatively established.
    pub rights_confirmed: Option<bool>,
    /// Whether the provider media kind is eligible.
    pub kind_eligible: Option<bool>,
    /// Whether the expected MIME is eligible.
    pub mime_eligible: Option<bool>,
    /// Whether the URL remains valid for the whole fetch lease.
    pub url_lifetime_sufficient: Option<bool>,
    /// Provider-declared byte length, when known.
    pub declared_bytes: Option<u64>,
    /// Per-object response ceiling.
    pub max_object_bytes: u64,
    /// Owner storage remaining before this object is admitted.
    pub owner_remaining_bytes: Option<u64>,
    /// Whether a separately recorded explicit user action exists.
    pub explicit_action: bool,
}

/// Evaluates policy and invokes `fetch` only when byte archival was authorized.
pub fn observe_media<F>(input: MediaPolicyInput, mut fetch: F) -> MediaRetentionDecision
where
    F: FnMut(MediaFetchLease),
{
    if !input.archive_requested {
        return MediaRetentionDecision::MetadataOnly(MetadataOnlyReason::PolicyNotAuthorized);
    }
    let refused = if !input.acquisition_eligible {
        Some(MetadataOnlyReason::AcquisitionNotEligible)
    } else if input.rights_confirmed != Some(true) {
        Some(MetadataOnlyReason::RightsUnknown)
    } else if input.kind_eligible != Some(true) {
        Some(MetadataOnlyReason::KindUnknown)
    } else if input.mime_eligible != Some(true) {
        Some(MetadataOnlyReason::MimeUnknown)
    } else if input.url_lifetime_sufficient != Some(true) {
        Some(MetadataOnlyReason::UrlLifetimeUnknown)
    } else if input.declared_bytes.is_none() {
        Some(MetadataOnlyReason::ObjectSizeUnknown)
    } else if input.declared_bytes == Some(0)
        || input
            .declared_bytes
            .is_some_and(|bytes| bytes > input.max_object_bytes)
    {
        Some(MetadataOnlyReason::ObjectBudgetExceeded)
    } else if input.owner_remaining_bytes.is_none() {
        Some(MetadataOnlyReason::OwnerBudgetUnknown)
    } else if input
        .owner_remaining_bytes
        .zip(input.declared_bytes)
        .is_some_and(|(remaining, bytes)| remaining < bytes)
    {
        Some(MetadataOnlyReason::OwnerBudgetExceeded)
    } else if !input.explicit_action {
        Some(MetadataOnlyReason::ExplicitActionRequired)
    } else {
        None
    };
    if let Some(reason) = refused {
        return MediaRetentionDecision::MetadataOnly(reason);
    }
    let Some(max_bytes) = input.declared_bytes else {
        return MediaRetentionDecision::MetadataOnly(MetadataOnlyReason::ObjectSizeUnknown);
    };
    let lease = MediaFetchLease { max_bytes };
    fetch(lease);
    MediaRetentionDecision::Archive(lease)
}
