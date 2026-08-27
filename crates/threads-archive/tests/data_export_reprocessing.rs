//! Parser-version reprocessing and dry-run fidelity tests use synthetic exports only.

use ratatoskr_threads_archive::data_export_reprocessing::{
    ReprocessClassification, ReprocessInput, ReprocessingError, ReprocessingStore,
    RetainedExportReceipt, SUPPORTED_REPROCESSING_EXPORT, SUPPORTED_REPROCESSING_PARSER,
    begin_reprocessing, migration_apply, migration_dry_run,
};
use ratatoskr_threads_archive::test_support::TestDatabase;
use sha2::{Digest as _, Sha256};

#[test]
fn reprocessing_refuses_tampered_receipts_and_unsupported_parser_versions_before_projection() {
    let bytes = b"synthetic retained export";
    let correct_hash: [u8; 32] = Sha256::digest(bytes).into();
    let cases = [
        (
            RetainedExportReceipt {
                bytes: b"tampered synthetic retained export",
                expected_hash: correct_hash,
                expected_length: bytes.len() as u64,
                detected_version: SUPPORTED_REPROCESSING_EXPORT,
            },
            SUPPORTED_REPROCESSING_PARSER,
            ReprocessingError::ReceiptIntegrity,
        ),
        (
            RetainedExportReceipt {
                bytes,
                expected_hash: correct_hash,
                expected_length: bytes.len() as u64,
                detected_version: SUPPORTED_REPROCESSING_EXPORT,
            },
            "unregistered-parser",
            ReprocessingError::UnsupportedParser,
        ),
    ];

    for (receipt, parser, expected) in cases {
        let mut projection_calls = 0_u32;
        assert_eq!(
            begin_reprocessing(receipt, parser, || projection_calls += 1),
            Err(expected)
        );
        assert_eq!(
            projection_calls, 0,
            "refusal must precede projection planning"
        );
    }
}

async fn synthetic_export(
    test: &TestDatabase,
    owner: uuid::Uuid,
) -> Result<uuid::Uuid, sqlx::Error> {
    let export_run_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.export_runs \
         (run_id, user_ref, archive_hash, archive_blob_ref, archive_byte_size, detected_version, \
          parser_version, outcome, records_processed, finished_at) \
         values ($1, $2, $3, $4, 7, $5, $6, 'completed', 3, now())",
    )
    .bind(export_run_id)
    .bind(owner)
    .bind(vec![0x61_u8; 32])
    .bind(format!("threads-archive/raw/sha256/{export_run_id}"))
    .bind(SUPPORTED_REPROCESSING_EXPORT)
    .bind(SUPPORTED_REPROCESSING_PARSER)
    .execute(test.database.pool())
    .await?;
    Ok(export_run_id)
}

fn reprocessing_inputs() -> Vec<ReprocessInput> {
    vec![
        ReprocessInput {
            item_key: "a".to_owned(),
            classification: ReprocessClassification::Normalized,
            prospective_digest: Some("6".repeat(64)),
        },
        ReprocessInput {
            item_key: "b".to_owned(),
            classification: ReprocessClassification::UnknownRecord,
            prospective_digest: None,
        },
        ReprocessInput {
            item_key: "c".to_owned(),
            classification: ReprocessClassification::Warning,
            prospective_digest: None,
        },
    ]
}

#[tokio::test]
async fn apply_resumes_after_committed_checkpoint_and_completed_replay_adds_nothing() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let owner = uuid::Uuid::now_v7();
    let export_run_id = synthetic_export(&test, owner)
        .await
        .expect("synthetic export receipt stores");
    let inputs = reprocessing_inputs();
    let state = "7".repeat(64);
    let operation_id = uuid::Uuid::now_v7();
    let store = ReprocessingStore::new(&test.database);

    let interrupted = store
        .apply_chunk(owner, export_run_id, operation_id, &inputs, &state, 1)
        .await
        .expect("first chunk commits");
    assert!(!interrupted.completed);
    let resumed = store
        .apply_chunk(
            owner,
            export_run_id,
            operation_id,
            &inputs,
            &state,
            usize::MAX,
        )
        .await
        .expect("same operation resumes");
    let fresh = store
        .apply_chunk(
            owner,
            export_run_id,
            uuid::Uuid::now_v7(),
            &inputs,
            &state,
            usize::MAX,
        )
        .await
        .expect("fresh uninterrupted run completes");
    assert_eq!(resumed.report, fresh.report);
    let before_replay: (i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.export_reprocessing_runs), \
           (select count(*) from threads_archive.export_reprocessing_items)",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("completed counts read");
    let replay = store
        .apply_chunk(
            owner,
            export_run_id,
            operation_id,
            &inputs,
            &state,
            usize::MAX,
        )
        .await
        .expect("completed operation replays");
    let after_replay: (i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.export_reprocessing_runs), \
           (select count(*) from threads_archive.export_reprocessing_items)",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("replay counts read");

    assert_eq!(replay, resumed);
    assert_eq!(after_replay, before_replay);

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn parser_omission_never_deletes_existing_capture_source_or_media() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let owner = uuid::Uuid::now_v7();
    let export_run_id = synthetic_export(&test, owner)
        .await
        .expect("synthetic export receipt stores");
    let post_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.posts \
         (post_id, provider_post_id, permalink, post_kind, acquisition_method, saved_authority, upstream_status) \
         values ($1, 'omission-post', 'https://www.threads.net/@safe/post/omission', 'post', \
          'data_export', 'export_observation', 'active')",
    )
    .bind(post_id)
    .execute(test.database.pool())
    .await
    .expect("existing export projection stores");
    sqlx::query(
        "insert into threads_archive.captures \
         (capture_id, user_ref, post_id, idempotency_key, canonical_url, original_url, \
          acquisition_method, saved_authority, client_source, status, captured_at) \
         values ($1, $2, $3, 'omission-capture', 'https://www.threads.net/@safe/post/omission', \
          'https://threads.net/@safe/post/omission', 'data_export', 'export_observation', \
          'browser_extension', 'resolved', now())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(owner)
    .bind(post_id)
    .execute(test.database.pool())
    .await
    .expect("existing capture projection stores");
    sqlx::query(
        "insert into threads_archive.media \
         (media_id, post_id, media_kind, media_state, retention_class, observed_at) \
         values ($1, $2, 'image', 'metadata_only', 'metadata_only', now())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(post_id)
    .execute(test.database.pool())
    .await
    .expect("existing media projection stores");
    let before: (i64, i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.posts), \
           (select count(*) from threads_archive.captures), \
           (select count(*) from threads_archive.media)",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("projection snapshot reads");
    let inputs = vec![ReprocessInput {
        item_key: post_id.to_string(),
        classification: ReprocessClassification::Omitted,
        prospective_digest: None,
    }];

    let outcome = ReprocessingStore::new(&test.database)
        .apply_chunk(
            owner,
            export_run_id,
            uuid::Uuid::now_v7(),
            &inputs,
            &"8".repeat(64),
            usize::MAX,
        )
        .await
        .expect("omission is a reportable completed outcome");
    let after: (i64, i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.posts), \
           (select count(*) from threads_archive.captures), \
           (select count(*) from threads_archive.media)",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("projection snapshot reads");

    assert_eq!(
        outcome.report.items[0].classification,
        ReprocessClassification::Omitted
    );
    assert_eq!(
        after, before,
        "parser omission is absence-without-deletion evidence"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[test]
fn migration_dry_run_matches_apply_report_for_unchanged_state() {
    let inputs = vec![
        ReprocessInput {
            item_key: "z-warning".to_owned(),
            classification: ReprocessClassification::Warning,
            prospective_digest: None,
        },
        ReprocessInput {
            item_key: "a-normalized".to_owned(),
            classification: ReprocessClassification::Normalized,
            prospective_digest: Some("1".repeat(64)),
        },
        ReprocessInput {
            item_key: "m-conflict".to_owned(),
            classification: ReprocessClassification::Conflict,
            prospective_digest: Some("2".repeat(64)),
        },
        ReprocessInput {
            item_key: "u-unknown".to_owned(),
            classification: ReprocessClassification::UnknownSection,
            prospective_digest: None,
        },
    ];
    let state_fingerprint = "3".repeat(64);

    let dry_run = migration_dry_run(&inputs, &state_fingerprint);
    let applied = migration_apply(&inputs, &state_fingerprint);

    assert_eq!(
        dry_run, applied,
        "dry-run and apply must render one shared deterministic plan"
    );
}

#[tokio::test]
async fn dry_run_does_not_change_database_blob_outbox_or_checkpoint_state() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let owner = uuid::Uuid::now_v7();
    let export_run_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.export_runs \
         (run_id, user_ref, archive_hash, archive_blob_ref, archive_byte_size, detected_version, \
          parser_version, outcome, records_processed, completeness_report, finished_at) \
         values ($1, $2, $3, 'threads-archive/raw/sha256/synthetic-export', 7, $4, $5, \
          'completed', 1, '{\"synthetic\":true}', now())",
    )
    .bind(export_run_id)
    .bind(owner)
    .bind(vec![0x22_u8; 32])
    .bind(SUPPORTED_REPROCESSING_EXPORT)
    .bind(SUPPORTED_REPROCESSING_PARSER)
    .execute(test.database.pool())
    .await
    .expect("synthetic export receipt stores");
    let before: (i64, i64, i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.export_runs), \
           (select count(*) from threads_archive.export_reprocessing_runs), \
           (select count(*) from threads_archive.export_reprocessing_items), \
           (select count(*) from threads_archive.outbox_events)",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("dry-run snapshot reads");
    let inputs = vec![
        ReprocessInput {
            item_key: "known".to_owned(),
            classification: ReprocessClassification::Normalized,
            prospective_digest: Some("4".repeat(64)),
        },
        ReprocessInput {
            item_key: "unknown".to_owned(),
            classification: ReprocessClassification::UnknownRecord,
            prospective_digest: None,
        },
        ReprocessInput {
            item_key: "conflict".to_owned(),
            classification: ReprocessClassification::Conflict,
            prospective_digest: None,
        },
    ];

    ReprocessingStore::new(&test.database)
        .dry_run(owner, export_run_id, &inputs, &"5".repeat(64))
        .await
        .expect("dry-run report answers");
    let after: (i64, i64, i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.export_runs), \
           (select count(*) from threads_archive.export_reprocessing_runs), \
           (select count(*) from threads_archive.export_reprocessing_items), \
           (select count(*) from threads_archive.outbox_events)",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("dry-run snapshot reads");

    assert_eq!(
        after, before,
        "dry-run must not create run, item, outbox, or checkpoint state"
    );

    test.cleanup().await.expect("cleanup must drop");
}
