//! Media byte archival and reference-safe retention tests.

use ratatoskr_threads_archive::media_retention::{
    AcquiredMedia, ApprovedMediaMime, BlobCleanupPlan, BlobDeletionBackend, BlobDeletionFailure,
    BlobDeletionTaskOutcome, MediaArchiveOutcome, MediaFetchLease, MediaPolicyInput,
    MediaRetentionDecision, MediaVerificationReason, MetadataOnlyReason, archive_acquired_media,
    observe_media, plan_media_reference_expiry, process_blob_deletion_task,
};
use ratatoskr_threads_archive::public_resolution::RawObjectStore;
use ratatoskr_threads_archive::test_support::TestDatabase;

#[test]
fn metadata_observation_never_downloads_without_authorized_policy() {
    let mut fetches = 0_u32;
    let decision = observe_media(
        MediaPolicyInput {
            archive_requested: false,
            acquisition_eligible: true,
            rights_confirmed: Some(true),
            kind_eligible: Some(true),
            mime_eligible: Some(true),
            url_lifetime_sufficient: Some(true),
            declared_bytes: Some(1024),
            max_object_bytes: 8 * 1024 * 1024,
            owner_remaining_bytes: Some(8 * 1024 * 1024),
            explicit_action: true,
        },
        |_| fetches += 1,
    );

    assert_eq!(
        decision,
        MediaRetentionDecision::MetadataOnly(MetadataOnlyReason::PolicyNotAuthorized)
    );
    assert_eq!(
        fetches, 0,
        "metadata-only observation must not start a fetch"
    );
}

fn eligible_policy() -> MediaPolicyInput {
    MediaPolicyInput {
        archive_requested: true,
        acquisition_eligible: true,
        rights_confirmed: Some(true),
        kind_eligible: Some(true),
        mime_eligible: Some(true),
        url_lifetime_sufficient: Some(true),
        declared_bytes: Some(1024),
        max_object_bytes: 4096,
        owner_remaining_bytes: Some(4096),
        explicit_action: true,
    }
}

#[test]
fn archival_refuses_before_fetch_when_any_eligibility_or_budget_guard_is_unknown_or_exhausted() {
    let cases = [
        (
            MetadataOnlyReason::AcquisitionNotEligible,
            MediaPolicyInput {
                acquisition_eligible: false,
                ..eligible_policy()
            },
        ),
        (
            MetadataOnlyReason::RightsUnknown,
            MediaPolicyInput {
                rights_confirmed: None,
                ..eligible_policy()
            },
        ),
        (
            MetadataOnlyReason::KindUnknown,
            MediaPolicyInput {
                kind_eligible: None,
                ..eligible_policy()
            },
        ),
        (
            MetadataOnlyReason::MimeUnknown,
            MediaPolicyInput {
                mime_eligible: None,
                ..eligible_policy()
            },
        ),
        (
            MetadataOnlyReason::UrlLifetimeUnknown,
            MediaPolicyInput {
                url_lifetime_sufficient: None,
                ..eligible_policy()
            },
        ),
        (
            MetadataOnlyReason::ObjectSizeUnknown,
            MediaPolicyInput {
                declared_bytes: None,
                ..eligible_policy()
            },
        ),
        (
            MetadataOnlyReason::ObjectBudgetExceeded,
            MediaPolicyInput {
                declared_bytes: Some(4097),
                ..eligible_policy()
            },
        ),
        (
            MetadataOnlyReason::OwnerBudgetUnknown,
            MediaPolicyInput {
                owner_remaining_bytes: None,
                ..eligible_policy()
            },
        ),
        (
            MetadataOnlyReason::OwnerBudgetExceeded,
            MediaPolicyInput {
                owner_remaining_bytes: Some(1023),
                ..eligible_policy()
            },
        ),
        (
            MetadataOnlyReason::ExplicitActionRequired,
            MediaPolicyInput {
                explicit_action: false,
                ..eligible_policy()
            },
        ),
    ];

    for (expected, input) in cases {
        let mut fetches = 0_u32;
        let decision = observe_media(input, |_| fetches += 1);
        assert_eq!(decision, MediaRetentionDecision::MetadataOnly(expected));
        assert_eq!(
            fetches, 0,
            "guard {expected:?} must refuse before network I/O"
        );
    }
}

#[tokio::test]
async fn verified_bytes_are_committed_only_after_https_mime_size_and_digest_checks() {
    let root = std::env::temp_dir().join(format!("threads-media-test-{}", uuid::Uuid::now_v7()));
    let store = RawObjectStore::new(&root);
    let body = b"synthetic provider media";
    let outcome = archive_acquired_media(
        &store,
        MediaFetchLease { max_bytes: 1024 },
        AcquiredMedia {
            final_url: "https://cdn.example.invalid/media.jpg",
            content_type: ApprovedMediaMime::ImageJpeg,
            declared_bytes: body.len() as u64,
            expected_digest: [0x5a; 32],
            body,
        },
    )
    .await
    .expect("verification refusal is a normal outcome");
    let object_count = std::fs::read_dir(root.join("sha256")).map_or(0, std::iter::Iterator::count);

    assert!(
        outcome
            == MediaArchiveOutcome::MetadataOnly(MediaVerificationReason::ContentDigestMismatch)
            && object_count == 0,
        "digest mismatch must remain metadata-only with no promoted partial object: outcome={outcome:?} object_count={object_count}"
    );

    if root.exists() {
        std::fs::remove_dir_all(root).expect("test storage cleanup");
    }
}

#[tokio::test]
async fn expiring_one_reference_preserves_a_blob_still_referenced_elsewhere() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let first_post = uuid::Uuid::now_v7();
    let second_post = uuid::Uuid::now_v7();
    for (post_id, suffix) in [(first_post, "one"), (second_post, "two")] {
        sqlx::query(
            "insert into threads_archive.posts \
             (post_id, permalink, post_kind, acquisition_method, saved_authority, upstream_status) \
             values ($1, $2, 'post', 'public_resolution', 'explicit_user_capture', 'active')",
        )
        .bind(post_id)
        .bind(format!("https://www.threads.net/@safe/post/{suffix}"))
        .execute(test.database.pool())
        .await
        .expect("synthetic post stores");
    }
    let first_media = uuid::Uuid::now_v7();
    let digest = vec![0x42_u8; 32];
    for (media_id, post_id) in [
        (first_media, first_post),
        (uuid::Uuid::now_v7(), second_post),
    ] {
        sqlx::query(
            "insert into threads_archive.media \
             (media_id, post_id, media_kind, blob_ref, content_hash, byte_size, media_state, \
              retention_class, observed_at) \
             values ($1, $2, 'image', 'threads-archive/raw/sha256/shared', $3, 7, \
              'bytes_archived', 'explicit_archive', now())",
        )
        .bind(media_id)
        .bind(post_id)
        .bind(&digest)
        .execute(test.database.pool())
        .await
        .expect("synthetic media reference stores");
    }

    let plan = plan_media_reference_expiry(&test.database, first_media)
        .await
        .expect("cleanup plan answers");
    assert_eq!(plan, BlobCleanupPlan::RetainShared { live_references: 1 });

    test.cleanup().await.expect("cleanup must drop");
}

struct FailOnceThenDelete {
    path: std::path::PathBuf,
    failed: std::sync::atomic::AtomicBool,
}

impl BlobDeletionBackend for FailOnceThenDelete {
    fn delete_if_matches<'a>(
        &'a self,
        _blob_ref: &'a str,
        _content_hash: &'a [u8],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), BlobDeletionFailure>> + Send + 'a>,
    > {
        Box::pin(async move {
            if !self.failed.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return Err(BlobDeletionFailure::StorageUnavailable);
            }
            tokio::fs::remove_file(&self.path)
                .await
                .map_err(|_| BlobDeletionFailure::StorageUnavailable)
        })
    }
}

#[tokio::test]
async fn failed_blob_delete_stays_pending_and_retries_to_verified_absence() {
    use sha2::{Digest as _, Sha256};

    let test = TestDatabase::create().await.expect("a fresh test database");
    let root = std::env::temp_dir().join(format!("threads-delete-test-{}", uuid::Uuid::now_v7()));
    let digest = Sha256::digest(b"unreferenced synthetic media").to_vec();
    let digest_hex = digest.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
        output
    });
    let path = root.join("sha256").join(&digest_hex);
    tokio::fs::create_dir_all(path.parent().expect("digest path has parent"))
        .await
        .expect("test store root");
    tokio::fs::write(&path, b"unreferenced synthetic media")
        .await
        .expect("test object stores");
    let task_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.blob_deletion_tasks \
         (task_id, blob_ref, content_hash, state) values ($1, $2, $3, 'pending')",
    )
    .bind(task_id)
    .bind(format!("threads-archive/raw/sha256/{digest_hex}"))
    .bind(&digest)
    .execute(test.database.pool())
    .await
    .expect("deletion task stores");
    let backend = FailOnceThenDelete {
        path: path.clone(),
        failed: std::sync::atomic::AtomicBool::new(false),
    };

    let first = process_blob_deletion_task(&test.database, &backend, task_id)
        .await
        .expect("first attempt records failure");
    let second = process_blob_deletion_task(&test.database, &backend, task_id)
        .await
        .expect("retry completes");
    let stored: (String, i32, Option<String>) = sqlx::query_as(
        "select state, attempt_count, last_failure_class \
         from threads_archive.blob_deletion_tasks where task_id = $1",
    )
    .bind(task_id)
    .fetch_one(test.database.pool())
    .await
    .expect("task state reads");

    assert_eq!(
        first,
        BlobDeletionTaskOutcome::Pending(BlobDeletionFailure::StorageUnavailable)
    );
    assert_eq!(second, BlobDeletionTaskOutcome::Complete);
    assert_eq!(stored, ("complete".to_owned(), 2, None));
    assert!(!path.exists(), "complete requires verified absence");

    if root.exists() {
        std::fs::remove_dir_all(root).expect("test storage cleanup");
    }
    test.cleanup().await.expect("cleanup must drop");
}
