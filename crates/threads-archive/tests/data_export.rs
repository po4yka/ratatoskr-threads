//! Contract tests for safe Data Export receipt and parsing.

#![allow(
    clippy::cast_possible_wrap,
    clippy::expect_used,
    clippy::panic,
    reason = "assertions and synthetic fixture setup in an integration test"
)]

use std::collections::BTreeSet;
use std::io::{Cursor, Write as _};

use ratatoskr_threads_archive::data_export::{
    DataExportStore, ExportError, ExportLimits, ReceiptOutcome, completeness_report,
    extract_archive, inspect_archive, parse_export,
};
use ratatoskr_threads_archive::public_resolution::RawObjectStore;
use ratatoskr_threads_archive::test_support::TestDatabase;
use uuid::Uuid;
use zip::write::SimpleFileOptions;

#[test]
fn zip_slip_is_refused_before_any_projection() {
    let archive = zip_with_entries(&[("../outside.json", b"{}")]);

    let error = inspect_archive(&archive, ExportLimits::default())
        .expect_err("traversal entry must be refused before parsing");

    assert!(
        matches!(error, ExportError::Limit { limit: "path", .. }),
        "expected a path-safety refusal, got {error:?}"
    );
}

#[test]
fn declared_decompressed_byte_limit_is_refused_before_parser_projection() {
    let archive = zip_with_entries(&[("threads/posts.json", b"0123456789")]);
    let limits = ExportLimits {
        max_decompressed_bytes: 9,
        ..ExportLimits::default()
    };

    let error = inspect_archive(&archive, limits)
        .expect_err("an over-limit declared entry must be refused before parsing");

    assert!(
        matches!(
            error,
            ExportError::Limit {
                limit: "decompressed_bytes",
                ..
            }
        ),
        "expected a decompressed-byte refusal, got {error:?}"
    );
}

#[test]
fn entry_count_limit_is_refused_before_parser_projection() {
    let archive = zip_with_entries(&[("one.json", b"{}"), ("two.json", b"{}")]);
    let limits = ExportLimits {
        max_entries: 1,
        ..ExportLimits::default()
    };

    let error = inspect_archive(&archive, limits)
        .expect_err("an over-limit entry count must be refused before parsing");

    assert!(
        matches!(
            error,
            ExportError::Limit {
                limit: "entry_count",
                ..
            }
        ),
        "expected an entry-count refusal, got {error:?}"
    );
}

#[test]
fn nesting_limit_is_refused_deterministically() {
    let archive = zip_with_entries(&[("a/b/c/d/e.json", b"{}")]);
    let limits = ExportLimits {
        max_path_depth: 4,
        ..ExportLimits::default()
    };

    let error = inspect_archive(&archive, limits)
        .expect_err("an over-nested entry must be refused before parsing");

    assert!(
        matches!(
            error,
            ExportError::Limit {
                limit: "path_depth",
                ..
            }
        ),
        "expected a path-depth refusal, got {error:?}"
    );
}

#[test]
fn compression_ratio_limit_is_refused_deterministically() {
    let payload = vec![b'x'; 4_096];
    let archive = deflated_zip("threads/posts.json", &payload);
    let limits = ExportLimits {
        max_compression_ratio: 2,
        ..ExportLimits::default()
    };

    let error = inspect_archive(&archive, limits)
        .expect_err("a high-ratio entry must be refused before parsing");

    assert!(
        matches!(
            error,
            ExportError::Limit {
                limit: "compression_ratio",
                ..
            }
        ),
        "expected a compression-ratio refusal, got {error:?}"
    );
}

#[test]
fn completeness_report_counts_overlap_differences_and_non_comparable_captures() {
    let exports = BTreeSet::from(["a".to_owned(), "b".to_owned(), "c".to_owned()]);
    let report = completeness_report(
        &exports,
        vec![
            Some("a".to_owned()),
            Some("b".to_owned()),
            Some("d".to_owned()),
            None,
        ],
    );

    assert_eq!(report.export_identities, 3);
    assert_eq!(report.matched_captures, 2);
    assert_eq!(report.export_only, 1);
    assert_eq!(report.capture_only, 1);
    assert_eq!(report.non_comparable_captures, 1);
}

#[test]
fn supported_fixture_normalizes_deterministic_export_posts_and_relations() {
    let manifest = br#"{
        "version":"threads-export-v1",
        "posts":[
            {"id":"post-b","permalink":"https://threads.net/@u/post/b","text":"b"},
            {"id":"post-a","permalink":"https://threads.net/@u/post/a","text":"a"}
        ],
        "relations":[
            {"from":"post-b","kind":"reply","to":"post-a"}
        ]
    }"#;
    let first = zip_with_entries(&[("ignored.json", b"{}"), ("threads_export.json", manifest)]);
    let second = zip_with_entries(&[("threads_export.json", manifest), ("ignored.json", b"{}")]);

    let first = parse_export(&first, ExportLimits::default()).expect("fixture one parses");
    let second = parse_export(&second, ExportLimits::default()).expect("fixture two parses");

    assert_eq!(
        first, second,
        "entry ordering must not change normalized output"
    );
    assert_eq!(first.parser_version, "threads-export-v1-parser-1");
    assert_eq!(
        first
            .posts
            .iter()
            .map(|post| post.provider_post_id.as_str())
            .collect::<Vec<_>>(),
        ["post-a", "post-b"]
    );
    assert_eq!(first.relations.len(), 1);
    assert_eq!(first.unknown_entries, ["ignored.json"]);
}

#[test]
fn safe_extraction_writes_only_beneath_the_owned_root() {
    let archive = zip_with_entries(&[("threads/posts.json", b"safe fixture")]);
    let root = std::env::temp_dir().join(format!("threads-export-extract-{}", Uuid::now_v7()));

    let extracted = extract_archive(&archive, ExportLimits::default(), &root)
        .expect("safe fixture extracts beneath its owned root");

    assert_eq!(extracted.entry_names, ["threads/posts.json"]);
    assert_eq!(
        std::fs::read(root.join("threads/posts.json")).expect("owned file reads"),
        b"safe fixture"
    );
    std::fs::remove_dir_all(root).expect("owned extraction root drops");
}

#[tokio::test]
async fn receipt_retains_immutable_bytes_and_replays_per_owner() {
    let database = TestDatabase::create()
        .await
        .expect("disposable archive database starts");
    let root = std::env::temp_dir().join(format!("threads-export-test-{}", Uuid::now_v7()));
    let store = DataExportStore::new(&database.database, RawObjectStore::new(root.clone()));
    let owner = Uuid::now_v7();
    let archive = b"received-before-inspection";

    let ReceiptOutcome::Created(first) = store
        .receive(owner, archive)
        .await
        .expect("first immutable receipt stores")
    else {
        panic!("first receipt must create an import run");
    };
    let ReceiptOutcome::Replayed(second) = store
        .receive(owner, archive)
        .await
        .expect("identical receipt replays")
    else {
        panic!("identical owner receipt must replay");
    };
    let ReceiptOutcome::Created(other_owner) = store
        .receive(Uuid::now_v7(), archive)
        .await
        .expect("another owner receives an independently scoped run")
    else {
        panic!("another owner must not replay this owner's run");
    };

    assert_eq!(first, second, "replay keeps immutable receipt evidence");
    assert_ne!(
        first.run_id, other_owner.run_id,
        "owner boundary scopes replay"
    );
    assert_eq!(first.archive_byte_size, archive.len() as i64);
    assert!(
        first
            .archive_blob_ref
            .starts_with("threads-archive/raw/sha256/"),
        "receipt exposes a content-addressed service-owned BlobRef"
    );
    database.cleanup().await.expect("disposable database drops");
    tokio::fs::remove_dir_all(root)
        .await
        .expect("raw fixture directory drops");
}

#[tokio::test]
async fn streamed_receipt_hashes_the_received_chunks() {
    let database = TestDatabase::create()
        .await
        .expect("disposable archive database starts");
    let root = std::env::temp_dir().join(format!("threads-export-stream-{}", Uuid::now_v7()));
    let store = DataExportStore::new(&database.database, RawObjectStore::new(root.clone()));
    let (mut writer, mut reader) = tokio::io::duplex(64);
    tokio::io::AsyncWriteExt::write_all(&mut writer, b"chunk-one")
        .await
        .expect("first synthetic chunk writes");
    tokio::io::AsyncWriteExt::write_all(&mut writer, b"-chunk-two")
        .await
        .expect("second synthetic chunk writes");
    drop(writer);

    let ReceiptOutcome::Created(receipt) = store
        .receive_stream(Uuid::now_v7(), &mut reader)
        .await
        .expect("streamed receipt stores")
    else {
        panic!("first streamed receipt must create an import run");
    };

    assert_eq!(receipt.archive_byte_size, 19);
    database.cleanup().await.expect("disposable database drops");
    tokio::fs::remove_dir_all(root)
        .await
        .expect("raw fixture directory drops");
}

#[tokio::test]
async fn received_export_reconciles_once_and_preserves_unknown_sections_with_a_report() {
    let database = TestDatabase::create()
        .await
        .expect("disposable archive database starts");
    let root = std::env::temp_dir().join(format!("threads-export-import-{}", Uuid::now_v7()));
    let store = DataExportStore::new(&database.database, RawObjectStore::new(root.clone()));
    let owner = Uuid::now_v7();
    let manifest = br#"{
        "version":"threads-export-v1",
        "posts":[
            {"id":"post_a","permalink":"https://threads.net/@u/post/a","text":"a"},
            {"id":"post_b","permalink":"https://threads.net/@u/post/b","text":"b"}
        ],
        "relations":[{"from":"post_b","kind":"reply","to":"post_a"}]
    }"#;
    let archive = zip_with_entries(&[
        ("threads_export.json", manifest),
        ("unrecognized.json", br#"{"future":true}"#),
    ]);
    let ReceiptOutcome::Created(receipt) = store
        .receive(owner, &archive)
        .await
        .expect("immutable receipt stores before import")
    else {
        panic!("first receipt must create an import run");
    };

    let first = store
        .import(&receipt)
        .await
        .expect("supported archive reconciles from its retained blob");
    let second = store
        .import(&receipt)
        .await
        .expect("terminal import replays without new projection rows");

    assert_eq!(first, second, "terminal report is replay-safe");
    assert!(first.completed_with_warnings);
    assert_eq!(first.records_processed, 4);
    assert_eq!(first.completeness_report.export_identities, 2);
    assert_eq!(first.completeness_report.export_only, 2);
    let post_count: i64 = sqlx::query_scalar("select count(*) from threads_archive.posts")
        .fetch_one(database.database.pool())
        .await
        .expect("normalized posts count");
    let relation_count: i64 =
        sqlx::query_scalar("select count(*) from threads_archive.post_relations")
            .fetch_one(database.database.pool())
            .await
            .expect("normalized relations count");
    let unknown_count: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.export_records where record_kind = 'unknown_section'",
    )
    .fetch_one(database.database.pool())
    .await
    .expect("unknown section record count");
    let unknown_raw_count: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.export_records \
         where record_kind = 'unknown_section' and raw_object_id is not null",
    )
    .fetch_one(database.database.pool())
    .await
    .expect("unknown section raw evidence count");
    let export_provenance_count: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.posts \
         where acquisition_method = 'data_export' and saved_authority = 'export_observation'",
    )
    .fetch_one(database.database.pool())
    .await
    .expect("export provenance count");
    let outbox_count: i64 =
        sqlx::query_scalar("select count(*) from threads_archive.outbox_events")
            .fetch_one(database.database.pool())
            .await
            .expect("Data Export source facts count");
    assert_eq!((post_count, relation_count, unknown_count), (2, 1, 1));
    assert_eq!(unknown_raw_count, 1);
    assert_eq!(export_provenance_count, 2);
    assert_eq!(outbox_count, 2, "replay adds no duplicate source fact");

    database.cleanup().await.expect("disposable database drops");
    tokio::fs::remove_dir_all(root)
        .await
        .expect("raw fixture directory drops");
}

#[tokio::test]
async fn hostile_receipt_is_retained_then_marked_failed_without_any_projection() {
    let database = TestDatabase::create()
        .await
        .expect("disposable archive database starts");
    let root = std::env::temp_dir().join(format!("threads-export-hostile-{}", Uuid::now_v7()));
    let store = DataExportStore::new(&database.database, RawObjectStore::new(root.clone()));
    let archive = zip_with_entries(&[("../outside.json", b"{}")]);
    let ReceiptOutcome::Created(receipt) = store
        .receive(Uuid::now_v7(), &archive)
        .await
        .expect("hostile archive is retained before inspection")
    else {
        panic!("first hostile receipt must create an import run");
    };

    let error = store
        .import(&receipt)
        .await
        .expect_err("hostile archive must fail after receipt but before projection");

    assert!(matches!(error, ExportError::Limit { limit: "path", .. }));
    let outcome: String =
        sqlx::query_scalar("select outcome from threads_archive.export_runs where run_id = $1")
            .bind(receipt.run_id)
            .fetch_one(database.database.pool())
            .await
            .expect("terminal hostile outcome reads");
    let post_count: i64 = sqlx::query_scalar("select count(*) from threads_archive.posts")
        .fetch_one(database.database.pool())
        .await
        .expect("no projection count reads");
    assert_eq!(outcome, "failed");
    assert_eq!(post_count, 0, "refusal creates no normalized post");

    database.cleanup().await.expect("disposable database drops");
    tokio::fs::remove_dir_all(root)
        .await
        .expect("raw fixture directory drops");
}

#[tokio::test]
async fn persisted_completeness_compares_export_with_owner_captures_without_deletion_claims() {
    let database = TestDatabase::create()
        .await
        .expect("disposable archive database starts");
    let root = std::env::temp_dir().join(format!("threads-export-report-{}", Uuid::now_v7()));
    let store = DataExportStore::new(&database.database, RawObjectStore::new(root.clone()));
    let owner = Uuid::now_v7();
    let matched_post = insert_capture_post(&database, "post_a").await;
    let capture_only_post = insert_capture_post(&database, "captured_only").await;
    insert_capture(&database, owner, Some(matched_post), "matched").await;
    insert_capture(&database, owner, Some(capture_only_post), "capture-only").await;
    insert_capture(&database, owner, None, "unresolved").await;
    let archive = zip_with_entries(&[(
        "threads_export.json",
        br#"{
            "version":"threads-export-v1",
            "posts":[
                {"id":"post_a","permalink":"https://threads.net/@u/post/a"},
                {"id":"post_b","permalink":"https://threads.net/@u/post/b"}
            ]
        }"#,
    )]);
    let ReceiptOutcome::Created(receipt) = store
        .receive(owner, &archive)
        .await
        .expect("report fixture receipt stores")
    else {
        panic!("first report fixture receipt must create an import run");
    };

    let outcome = store
        .import(&receipt)
        .await
        .expect("report fixture import completes");

    assert_eq!(outcome.completeness_report.export_identities, 2);
    assert_eq!(outcome.completeness_report.matched_captures, 1);
    assert_eq!(outcome.completeness_report.export_only, 1);
    assert_eq!(outcome.completeness_report.capture_only, 1);
    assert_eq!(outcome.completeness_report.non_comparable_captures, 1);
    let capture_statuses: Vec<String> = sqlx::query_scalar(
        "select status from threads_archive.captures where user_ref = $1 order by idempotency_key",
    )
    .bind(owner)
    .fetch_all(database.database.pool())
    .await
    .expect("captures remain readable after partial export");
    assert_eq!(capture_statuses, ["accepted", "accepted", "accepted"]);

    database.cleanup().await.expect("disposable database drops");
    tokio::fs::remove_dir_all(root)
        .await
        .expect("raw fixture directory drops");
}

async fn insert_capture_post(database: &TestDatabase, provider_post_id: &str) -> Uuid {
    let post_id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.posts \
         (post_id, provider_post_id, permalink, post_kind, acquisition_method, saved_authority, upstream_status) \
         values ($1, $2, $3, 'post', 'public_resolution', 'explicit_user_capture', 'active')",
    )
    .bind(post_id)
    .bind(provider_post_id)
    .bind(format!("https://threads.net/@u/post/{provider_post_id}"))
    .execute(database.database.pool())
    .await
    .expect("synthetic capture post stores");
    post_id
}

async fn insert_capture(
    database: &TestDatabase,
    owner: Uuid,
    post_id: Option<Uuid>,
    idempotency_key: &str,
) {
    sqlx::query(
        "insert into threads_archive.captures \
         (capture_id, user_ref, post_id, idempotency_key, canonical_url, original_url, acquisition_method, \
          saved_authority, client_source, status, captured_at) \
         values ($1, $2, $3, $4, $5, $5, 'share_extension', 'explicit_user_capture', \
          'ios_share_extension', 'accepted', now())",
    )
    .bind(Uuid::now_v7())
    .bind(owner)
    .bind(post_id)
    .bind(idempotency_key)
    .bind(format!("https://threads.net/@u/post/{idempotency_key}"))
    .execute(database.database.pool())
    .await
    .expect("synthetic capture stores");
}

#[expect(
    clippy::expect_used,
    reason = "synthetic in-memory fixture construction cannot receive untrusted I/O"
)]
fn zip_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    for (name, bytes) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .expect("synthetic ZIP fixture entry starts");
        writer
            .write_all(bytes)
            .expect("synthetic ZIP fixture bytes write");
    }
    writer
        .finish()
        .expect("synthetic ZIP fixture finalizes")
        .into_inner()
}

#[expect(
    clippy::expect_used,
    reason = "synthetic in-memory fixture construction cannot receive untrusted I/O"
)]
fn deflated_zip(name: &str, bytes: &[u8]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    writer
        .start_file(
            name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .expect("compressed synthetic ZIP fixture entry starts");
    writer
        .write_all(bytes)
        .expect("compressed synthetic ZIP fixture bytes write");
    writer
        .finish()
        .expect("compressed synthetic ZIP fixture finalizes")
        .into_inner()
}
