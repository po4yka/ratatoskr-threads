//! Schema contract: what exists after a fresh apply, that application is
//! idempotent, and that provenance vocabularies are enforced by the database.
//!
//! These tests talk to real `PostgreSQL`; `THREADS_ARCHIVE_TEST_DATABASE_URL`
//! selects the server and defaults to the `compose.yaml` endpoint. A missing
//! server is a failure, never a skip.

use uuid::Uuid;

use ratatoskr_threads_archive::Database;
use ratatoskr_threads_archive::test_support::{TestDatabase, admin_url};

/// The relations AGENTS.md's persistence vocabulary declares, no more, no fewer.
const DECLARED_TABLES: [&str; 13] = [
    "accounts",
    "captures",
    "capture_resolutions",
    "credentials",
    "export_records",
    "export_runs",
    "inbox_events",
    "media",
    "outbox_events",
    "post_relations",
    "posts",
    "raw_objects",
    "tombstones",
];

const INSERT_CAPTURE: &str = "insert into threads_archive.captures \
     (capture_id, user_ref, idempotency_key, canonical_url, original_url, acquisition_method, \
      saved_authority, client_source, status, captured_at) \
     values ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())";

const INSERT_POST: &str = "insert into threads_archive.posts \
     (post_id, permalink, post_kind, acquisition_method, saved_authority, upstream_status) \
     values ($1, $2, 'post', $3, $4, 'active')";

const INSERT_RELATION: &str = "insert into threads_archive.post_relations \
     (relation_id, parent_post_id, child_post_id, relation_kind) \
     values ($1, $2, $3, $4)";

const ACQUISITIONS: [&str; 7] = [
    "official_api",
    "share_extension",
    "browser_extension",
    "telegram_capture",
    "public_resolution",
    "data_export",
    "legacy_import",
];

const AUTHORITIES: [&str; 4] = [
    "explicit_user_capture",
    "export_observation",
    "authoritative_platform_state",
    "legacy_observation",
];

/// Insert one minimal post row and return its id.
#[expect(
    clippy::expect_used,
    reason = "integration-test helper outside any single test fn: an unanswered post insert is the failure"
)]
async fn insert_post(
    pool: &sqlx::PgPool,
    permalink: &str,
    acquisition_method: &str,
    saved_authority: &str,
) -> Uuid {
    let post_id = Uuid::now_v7();
    let inserted = sqlx::query(INSERT_POST)
        .bind(post_id)
        .bind(permalink)
        .bind(acquisition_method)
        .bind(saved_authority)
        .execute(pool)
        .await
        .expect("the marker post inserts");
    assert!(
        inserted.rows_affected() == 1,
        "the marker post must insert exactly one row"
    );
    post_id
}

#[expect(
    clippy::expect_used,
    reason = "integration-test helper: an unanswered catalog query is the failure"
)]
async fn archive_tables(pool: &sqlx::PgPool) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "select table_name from information_schema.tables \
         where table_schema = 'threads_archive' order by table_name",
    )
    .fetch_all(pool)
    .await
    .expect("the catalog query must answer");
    rows.into_iter().map(|(name,)| name).collect()
}

async fn connect_shared(
    name: &str,
) -> Result<Database, ratatoskr_threads_archive::PersistenceError> {
    let base = admin_url();
    let (prefix, _) = base.rsplit_once('/').unwrap_or(("", ""));
    let url = format!("{prefix}/{name}");
    Database::connect(&url, 2, std::time::Duration::from_secs(5)).await
}

#[tokio::test]
async fn fresh_apply_creates_every_declared_table_and_nothing_else() {
    let test = TestDatabase::create().await.expect("a fresh test database");

    let tables = archive_tables(test.database.pool()).await;
    let mut declared = DECLARED_TABLES.map(str::to_owned);
    declared.sort_unstable();
    assert_eq!(
        tables, declared,
        "the applied schema must match the declared inventory exactly"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn second_apply_is_a_no_op_and_concurrent_applies_both_succeed() {
    let test = TestDatabase::create_raw()
        .await
        .expect("a raw database for two racers");

    test.database.apply_schema().await.expect("first apply");
    let before = archive_tables(test.database.pool()).await;
    test.database.apply_schema().await.expect("second apply");
    let after = archive_tables(test.database.pool()).await;

    assert_eq!(before.len(), DECLARED_TABLES.len());
    assert_eq!(before, after, "a second apply must change nothing");

    let one = connect_shared(test.name())
        .await
        .expect("racer one connects");
    let two = connect_shared(test.name())
        .await
        .expect("racer two connects");

    let (first, second) = tokio::join!(one.apply_schema(), two.apply_schema());
    first.expect("the first concurrent application succeeds");
    second.expect("the second concurrent application succeeds");

    let tables = archive_tables(one.pool()).await;
    assert_eq!(tables.len(), DECLARED_TABLES.len(), "applied exactly once");

    one.close().await;
    two.close().await;
    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn unknown_acquisition_method_is_refused_by_named_check() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();

    let refused = sqlx::query(INSERT_CAPTURE)
        .bind(Uuid::now_v7())
        .bind(Uuid::now_v7())
        .bind(Uuid::now_v7().to_string())
        .bind("https://www.threads.net/@example/post/example")
        .bind("https://www.threads.net/@example/post/example")
        .bind("carrier_pigeon")
        .bind("explicit_user_capture")
        .bind("ios_share_extension")
        .bind("accepted")
        .execute(pool)
        .await;
    let error = refused.expect_err("an unknown acquisition method must be refused");
    assert!(
        error
            .to_string()
            .contains("captures_acquisition_method_check"),
        "the named CHECK constraint must reject it: {error}"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn every_documented_authority_value_inserts_including_explicit_user_capture() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();

    for acquisition in ACQUISITIONS {
        let inserted = sqlx::query(INSERT_CAPTURE)
            .bind(Uuid::now_v7())
            .bind(Uuid::now_v7())
            .bind(Uuid::now_v7().to_string())
            .bind("https://www.threads.net/@example/post/example")
            .bind("https://www.threads.net/@example/post/example")
            .bind(acquisition)
            .bind("explicit_user_capture")
            .bind("ios_share_extension")
            .bind("accepted")
            .execute(pool)
            .await;
        assert!(
            inserted.is_ok(),
            "documented acquisition {acquisition} must be accepted: {:?}",
            inserted.err().map(|e| e.to_string())
        );
    }
    for authority in AUTHORITIES {
        let inserted = sqlx::query(INSERT_CAPTURE)
            .bind(Uuid::now_v7())
            .bind(Uuid::now_v7())
            .bind(Uuid::now_v7().to_string())
            .bind("https://www.threads.net/@example/post/example")
            .bind("https://www.threads.net/@example/post/example")
            .bind("share_extension")
            .bind(authority)
            .bind("browser_extension")
            .bind("accepted")
            .execute(pool)
            .await;
        assert!(
            inserted.is_ok(),
            "documented authority {authority} must be accepted: {:?}",
            inserted.err().map(|e| e.to_string())
        );
    }

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn catalog_shows_zero_cross_schema_foreign_keys() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();

    let crossing: i64 = sqlx::query_scalar(
        "select count(*) from pg_constraint con \
         join pg_class rel on rel.oid = con.conrelid \
         join pg_namespace nsp on nsp.oid = rel.relnamespace \
         where nsp.nspname = 'threads_archive' \
           and con.contype = 'f' \
           and exists ( \
               select 1 from pg_class other \
               join pg_namespace onsp on onsp.oid = other.relnamespace \
               where other.oid = con.confrelid and onsp.nspname <> 'threads_archive')",
    )
    .fetch_one(pool)
    .await
    .expect("the catalog query must answer");

    assert_eq!(crossing, 0, "no foreign key may leave threads_archive");

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn public_resolution_is_accepted_on_provenance_tables() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();

    let capture = sqlx::query(INSERT_CAPTURE)
        .bind(Uuid::now_v7())
        .bind(Uuid::now_v7())
        .bind(Uuid::now_v7().to_string())
        .bind("https://www.threads.net/@example/post/example")
        .bind("https://www.threads.net/@example/post/example")
        .bind("public_resolution")
        .bind("explicit_user_capture")
        .bind("telegram")
        .bind("accepted")
        .execute(pool)
        .await;
    assert!(
        capture.is_ok(),
        "public_resolution must be accepted on captures: {:?}",
        capture.err().map(|e| e.to_string())
    );

    let post = sqlx::query(INSERT_POST)
        .bind(Uuid::now_v7())
        .bind("https://www.threads.net/@example/post/resolved")
        .bind("public_resolution")
        .bind("explicit_user_capture")
        .execute(pool)
        .await;
    assert!(
        post.is_ok(),
        "public_resolution must be accepted on posts: {:?}",
        post.err().map(|e| e.to_string())
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn the_former_unknown_authority_value_is_refused() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();

    let refused = sqlx::query(INSERT_CAPTURE)
        .bind(Uuid::now_v7())
        .bind(Uuid::now_v7())
        .bind(Uuid::now_v7().to_string())
        .bind("https://www.threads.net/@example/post/example")
        .bind("https://www.threads.net/@example/post/example")
        .bind("share_extension")
        .bind("unknown")
        .bind("browser_extension")
        .bind("accepted")
        .execute(pool)
        .await;
    let error = refused.expect_err("the former unknown authority must be refused");
    assert!(
        error.to_string().contains("captures_saved_authority_check"),
        "the named CHECK constraint must reject it: {error}"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn a_well_formed_relation_kind_beyond_the_documented_three_is_accepted() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();

    let parent = insert_post(
        pool,
        "https://www.threads.net/@example/post/parent",
        "public_resolution",
        "explicit_user_capture",
    )
    .await;
    let child = insert_post(
        pool,
        "https://www.threads.net/@example/post/child",
        "share_extension",
        "explicit_user_capture",
    )
    .await;

    let inserted = sqlx::query(INSERT_RELATION)
        .bind(Uuid::now_v7())
        .bind(parent)
        .bind(child)
        .bind("mention")
        .execute(pool)
        .await;
    assert!(
        inserted.is_ok(),
        "a well-formed kind beyond the documented three must be accepted: {:?}",
        inserted.err().map(|e| e.to_string())
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn a_malformed_relation_kind_is_refused_by_named_check() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();

    let parent = insert_post(
        pool,
        "https://www.threads.net/@example/post/parent",
        "share_extension",
        "explicit_user_capture",
    )
    .await;
    let child = insert_post(
        pool,
        "https://www.threads.net/@example/post/child",
        "share_extension",
        "explicit_user_capture",
    )
    .await;

    let too_long = format!("{}{}", "a", "b".repeat(32));
    for malformed in ["Mention", "", "1mention", "_mention", too_long.as_str()] {
        let refused = sqlx::query(INSERT_RELATION)
            .bind(Uuid::now_v7())
            .bind(parent)
            .bind(child)
            .bind(malformed)
            .execute(pool)
            .await;
        let error = refused.expect_err("a malformed relation kind must be refused");
        assert!(
            error
                .as_database_error()
                .map(|database| database.constraint())
                .is_some_and(|constraint| constraint == Some("post_relations_relation_kind_check")),
            "the named CHECK constraint must reject {malformed:?}: {error}"
        );
    }

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn two_harness_databases_are_isolated_and_drop_on_cleanup() {
    const INSERT_ACCOUNT: &str = "insert into threads_archive.accounts \
         (account_id, user_ref, provider_account_id, username, account_type, \
          connection_status, scopes, connected_at) \
         values ($1, $1, $2, 'who', 'business', 'connected', '', now())";

    let one = TestDatabase::create().await.expect("database one");
    let two = TestDatabase::create().await.expect("database two");
    assert_ne!(one.name(), two.name());

    for db in [&one, &two] {
        sqlx::query(INSERT_ACCOUNT)
            .bind(Uuid::now_v7())
            .bind(format!("p-{}", Uuid::now_v7()))
            .execute(db.database.pool())
            .await
            .expect("the marker row inserts");
    }

    let one_rows: i64 = sqlx::query_scalar("select count(*) from threads_archive.accounts")
        .fetch_one(one.database.pool())
        .await
        .expect("count in database one");
    assert_eq!(one_rows, 1, "isolation: each database sees only its row");

    let name_one = one.name().to_owned();
    let name_two = two.name().to_owned();
    one.cleanup().await.expect("drop one");
    two.cleanup().await.expect("drop two");

    let admin = Database::connect(&admin_url(), 1, std::time::Duration::from_secs(5))
        .await
        .expect("admin pool connects");
    let remaining: Vec<(String,)> =
        sqlx::query_as("select datname from pg_database where datname = any($1)")
            .bind(vec![name_one.clone(), name_two.clone()])
            .fetch_all(admin.pool())
            .await
            .expect("existence check answers");
    assert!(
        remaining.is_empty(),
        "both databases must be gone: {remaining:?}"
    );
    admin.close().await;
}
