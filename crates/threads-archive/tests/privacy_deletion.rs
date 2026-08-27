//! Owner privacy-deletion completeness and behavior tests.

use std::collections::BTreeSet;

use ratatoskr_threads_archive::privacy_deletion::{
    CAPTURE_DELETION_CLASSIFICATIONS, CONNECTION_DELETION_CLASSIFICATIONS, DeletionRequest,
    DeletionStore, DeletionTarget, OWNED_DATA_CLASSES, PrivacyDeletionError,
};
use ratatoskr_threads_archive::test_support::TestDatabase;

#[tokio::test]
async fn deletion_classifies_every_owned_data_class() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let table_names: Vec<(String,)> = sqlx::query_as(
        "select table_name from information_schema.tables \
         where table_schema = 'threads_archive' and table_type = 'BASE TABLE' \
         order by table_name",
    )
    .fetch_all(test.database.pool())
    .await
    .expect("owned table inventory query must answer");

    let mut authoritative = table_names
        .into_iter()
        .map(|(name,)| format!("table:{name}"))
        .collect::<BTreeSet<_>>();
    authoritative.extend([
        "blob:raw_object".to_owned(),
        "blob:provider_media".to_owned(),
        "blob:export_archive".to_owned(),
    ]);

    let declared = OWNED_DATA_CLASSES
        .iter()
        .map(|class| class.key().to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        declared, authoritative,
        "closed inventory must equal schema plus BlobStore classes"
    );
    assert_eq!(
        OWNED_DATA_CLASSES.len(),
        declared.len(),
        "closed inventory must not contain duplicates"
    );

    for (target, classifications) in [
        ("capture", CAPTURE_DELETION_CLASSIFICATIONS),
        ("connection", CONNECTION_DELETION_CLASSIFICATIONS),
    ] {
        let classified = classifications
            .iter()
            .map(|entry| entry.class.key().to_owned())
            .collect::<BTreeSet<_>>();
        let missing = declared
            .difference(&classified)
            .cloned()
            .collect::<Vec<_>>();
        let unknown = classified
            .difference(&declared)
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty() && unknown.is_empty() && classifications.len() == classified.len(),
            "{target} deletion classification is not total: missing={missing:?} unknown={unknown:?} duplicates={}",
            classifications.len().saturating_sub(classified.len())
        );
    }

    test.cleanup().await.expect("cleanup must drop");
}

async fn lifecycle_counts(pool: &sqlx::PgPool) -> Result<(i64, i64, i64), sqlx::Error> {
    sqlx::query_as(
        "select \
           (select count(*) from threads_archive.captures), \
           (select count(*) from threads_archive.deletion_operations), \
           (select count(*) from threads_archive.outbox_events)",
    )
    .fetch_one(pool)
    .await
}

#[tokio::test]
async fn cross_owner_or_unknown_target_refuses_without_any_mutation() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let owner = uuid::Uuid::now_v7();
    let capture_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.captures \
         (capture_id, user_ref, idempotency_key, canonical_url, original_url, acquisition_method, \
          saved_authority, client_source, status, captured_at) \
         values ($1, $2, 'privacy-target', 'https://www.threads.net/@safe/post/one', \
          'https://threads.net/@safe/post/one', 'share_extension', 'explicit_user_capture', \
          'ios_share_extension', 'accepted', now())",
    )
    .bind(capture_id)
    .bind(owner)
    .execute(test.database.pool())
    .await
    .expect("owned capture stores");
    let before = lifecycle_counts(test.database.pool())
        .await
        .expect("lifecycle count snapshot reads");
    let store = DeletionStore::new(&test.database);

    for request in [
        DeletionRequest {
            operation_id: uuid::Uuid::now_v7(),
            user_ref: uuid::Uuid::now_v7(),
            target: DeletionTarget::Capture(capture_id),
        },
        DeletionRequest {
            operation_id: uuid::Uuid::now_v7(),
            user_ref: owner,
            target: DeletionTarget::Capture(uuid::Uuid::now_v7()),
        },
    ] {
        assert!(matches!(
            store.apply(request).await,
            Err(PrivacyDeletionError::TargetNotFound)
        ));
        assert_eq!(
            lifecycle_counts(test.database.pool())
                .await
                .expect("lifecycle count snapshot reads"),
            before
        );
    }

    test.cleanup().await.expect("cleanup must drop");
}

async fn target_row_counts(pool: &sqlx::PgPool) -> Result<(i64, i64, i64), sqlx::Error> {
    sqlx::query_as(
        "select \
           (select count(*) from threads_archive.captures), \
           (select count(*) from threads_archive.accounts), \
           (select count(*) from threads_archive.deletion_operations)",
    )
    .fetch_one(pool)
    .await
}

#[tokio::test]
async fn preview_matches_apply_counts_and_leaves_durable_state_unchanged() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let owner = uuid::Uuid::now_v7();
    let capture_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.captures \
         (capture_id, user_ref, idempotency_key, canonical_url, original_url, acquisition_method, \
          saved_authority, client_source, status, captured_at) \
         values ($1, $2, 'preview-target', 'https://www.threads.net/@safe/post/preview', \
          'https://threads.net/@safe/post/preview', 'share_extension', 'explicit_user_capture', \
          'ios_share_extension', 'accepted', now())",
    )
    .bind(capture_id)
    .bind(owner)
    .execute(test.database.pool())
    .await
    .expect("capture target stores");
    let account_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.accounts \
         (account_id, user_ref, provider_account_id, username, account_type, connection_status, \
          scopes, connected_at) values ($1, $2, $3, 'redacted', 'personal', 'connected', \
          'threads_basic', now())",
    )
    .bind(account_id)
    .bind(owner)
    .bind(format!("provider-{account_id}"))
    .execute(test.database.pool())
    .await
    .expect("connection target stores");
    let store = DeletionStore::new(&test.database);

    for (target, expected_class) in [
        (DeletionTarget::Capture(capture_id), "table:captures"),
        (DeletionTarget::Connection(account_id), "table:accounts"),
    ] {
        let request = DeletionRequest {
            operation_id: uuid::Uuid::now_v7(),
            user_ref: owner,
            target,
        };
        let before = target_row_counts(test.database.pool())
            .await
            .expect("target count snapshot reads");
        let plan = store.preview(request).await.expect("preview answers");
        assert_eq!(
            target_row_counts(test.database.pool())
                .await
                .expect("target count snapshot reads"),
            before,
            "preview must be read-only"
        );
        assert_eq!(
            plan.effects
                .iter()
                .find(|effect| effect.class.key() == expected_class)
                .map(|effect| effect.affected_count),
            Some(1),
            "preview must enumerate the owned target row"
        );
        let applied = store.apply(request).await.expect("apply succeeds");
        assert_eq!(
            applied.effects, plan.effects,
            "apply must recompute the same counts under lock"
        );
    }

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn deleting_one_duplicate_capture_preserves_the_shared_source_and_emits_no_removal() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();
    let owner = uuid::Uuid::now_v7();
    let post_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.posts \
         (post_id, provider_post_id, permalink, post_kind, acquisition_method, saved_authority, upstream_status) \
         values ($1, 'shared-provider-post', 'https://www.threads.net/@safe/post/shared', 'post', \
          'public_resolution', 'explicit_user_capture', 'active')",
    )
    .bind(post_id)
    .execute(pool)
    .await
    .expect("shared post stores");
    let raw_id = uuid::Uuid::now_v7();
    let digest = vec![0x33_u8; 32];
    sqlx::query(
        "insert into threads_archive.raw_objects \
         (raw_object_id, object_kind, blob_ref, content_hash, byte_size, media_type, observed_at) \
         values ($1, 'oembed_response', 'threads-archive/raw/sha256/shared-raw', $2, 7, \
          'application/json', now())",
    )
    .bind(raw_id)
    .bind(&digest)
    .execute(pool)
    .await
    .expect("raw evidence stores");
    sqlx::query(
        "insert into threads_archive.post_revisions \
         (revision_id, post_id, raw_object_id, parser_version, observed_at) \
         values ($1, $2, $3, 'synthetic-v1', now())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(post_id)
    .bind(raw_id)
    .execute(pool)
    .await
    .expect("revision stores");
    sqlx::query(
        "insert into threads_archive.media \
         (media_id, post_id, media_kind, blob_ref, content_hash, byte_size, media_state, \
          retention_class, observed_at) values ($1, $2, 'image', \
          'threads-archive/raw/sha256/shared-media', $3, 7, 'bytes_archived', \
          'explicit_archive', now())",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(post_id)
    .bind(&digest)
    .execute(pool)
    .await
    .expect("media stores");
    let first_capture = uuid::Uuid::now_v7();
    let second_capture = uuid::Uuid::now_v7();
    for (capture_id, key) in [
        (first_capture, "duplicate-one"),
        (second_capture, "duplicate-two"),
    ] {
        sqlx::query(
            "insert into threads_archive.captures \
             (capture_id, user_ref, post_id, idempotency_key, canonical_url, original_url, \
              acquisition_method, saved_authority, client_source, status, captured_at) \
             values ($1, $2, $3, $4, 'https://www.threads.net/@safe/post/shared', \
              'https://threads.net/@safe/post/shared', 'share_extension', 'explicit_user_capture', \
              'ios_share_extension', 'resolved', now())",
        )
        .bind(capture_id)
        .bind(owner)
        .bind(post_id)
        .bind(key)
        .execute(pool)
        .await
        .expect("duplicate capture stores");
    }
    let social_source_id = uuid::Uuid::now_v7();
    insert_social_source(pool, social_source_id, owner, post_id, first_capture)
        .await
        .expect("source projection stores");
    let before: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.posts), \
           (select count(*) from threads_archive.post_revisions), \
           (select count(*) from threads_archive.raw_objects), \
           (select count(*) from threads_archive.media), \
           (select count(*) from threads_archive.social_sources)",
    )
    .fetch_one(pool)
    .await
    .expect("shared state snapshot reads");

    assert_duplicate_capture_retained(
        &test,
        owner,
        post_id,
        first_capture,
        second_capture,
        social_source_id,
        before,
    )
    .await
    .expect("one duplicate capture deletion succeeds");

    test.cleanup().await.expect("cleanup must drop");
}

async fn insert_social_source(
    pool: &sqlx::PgPool,
    social_source_id: uuid::Uuid,
    owner: uuid::Uuid,
    post_id: uuid::Uuid,
    first_capture: uuid::Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "insert into threads_archive.social_sources \
         (social_source_id, user_ref, post_id, first_capture_id) values ($1, $2, $3, $4)",
    )
    .bind(social_source_id)
    .bind(owner)
    .bind(post_id)
    .bind(first_capture)
    .execute(pool)
    .await?;
    Ok(())
}

async fn assert_duplicate_capture_retained(
    test: &TestDatabase,
    owner: uuid::Uuid,
    post_id: uuid::Uuid,
    first_capture: uuid::Uuid,
    second_capture: uuid::Uuid,
    social_source_id: uuid::Uuid,
    before: (i64, i64, i64, i64, i64),
) -> Result<(), Box<dyn std::error::Error>> {
    let applied = DeletionStore::new(&test.database)
        .apply(DeletionRequest {
            operation_id: uuid::Uuid::now_v7(),
            user_ref: owner,
            target: DeletionTarget::Capture(first_capture),
        })
        .await?;
    let after: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.posts), \
           (select count(*) from threads_archive.post_revisions), \
           (select count(*) from threads_archive.raw_objects), \
           (select count(*) from threads_archive.media), \
           (select count(*) from threads_archive.social_sources)",
    )
    .fetch_one(test.database.pool())
    .await?;
    let (captures, first_capture_ref, removals, removal_events): (i64, uuid::Uuid, i64, i64) =
        sqlx::query_as(
            "select \
               (select count(*) from threads_archive.captures where post_id = $1), \
               (select first_capture_id from threads_archive.social_sources where social_source_id = $2), \
               (select count(*) from threads_archive.local_source_removals), \
               (select count(*) from threads_archive.outbox_events where event_type like '%removed%')",
        )
        .bind(post_id)
        .bind(social_source_id)
        .fetch_one(test.database.pool())
        .await?;

    assert_eq!(
        after, before,
        "shared source graph and evidence must remain live"
    );
    assert_eq!(
        (captures, first_capture_ref, removals, removal_events),
        (1, second_capture, 0, 0)
    );
    assert_eq!(
        applied
            .effects
            .iter()
            .find(|effect| effect.class.key() == "table:posts")
            .map(|effect| (effect.action, effect.affected_count)),
        Some((
            ratatoskr_threads_archive::privacy_deletion::DeletionAction::RetainShared,
            1
        ))
    );

    Ok(())
}

#[tokio::test]
async fn deleting_the_final_capture_commits_content_free_audit_and_one_typed_removal() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();
    let owner = uuid::Uuid::now_v7();
    let post_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.posts \
         (post_id, provider_post_id, permalink, post_kind, text_content, acquisition_method, \
          saved_authority, upstream_status) values ($1, 'final-provider-post', \
          'https://www.threads.net/@private/post/final', 'post', 'PRIVATE BODY SENTINEL', \
          'public_resolution', 'explicit_user_capture', 'active')",
    )
    .bind(post_id)
    .execute(pool)
    .await
    .expect("final post stores");
    let capture_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.captures \
         (capture_id, user_ref, post_id, idempotency_key, canonical_url, original_url, \
          acquisition_method, saved_authority, client_source, status, note, captured_at) \
         values ($1, $2, $3, 'final-capture', 'https://www.threads.net/@private/post/final', \
          'https://threads.net/@private/post/final?PRIVATE_URL_SENTINEL', 'share_extension', \
          'explicit_user_capture', 'ios_share_extension', 'resolved', 'PRIVATE NOTE SENTINEL', now())",
    )
    .bind(capture_id)
    .bind(owner)
    .bind(post_id)
    .execute(pool)
    .await
    .expect("final capture stores");
    let social_source_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.social_sources \
         (social_source_id, user_ref, post_id, first_capture_id) values ($1, $2, $3, $4)",
    )
    .bind(social_source_id)
    .bind(owner)
    .bind(post_id)
    .bind(capture_id)
    .execute(pool)
    .await
    .expect("final source stores");
    let operation_id = uuid::Uuid::now_v7();

    let result = DeletionStore::new(&test.database)
        .apply(DeletionRequest {
            operation_id,
            user_ref: owner,
            target: DeletionTarget::Capture(capture_id),
        })
        .await
        .expect("final capture deletion succeeds");
    let counts: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.captures), \
           (select count(*) from threads_archive.posts), \
           (select count(*) from threads_archive.social_sources), \
           (select count(*) from threads_archive.deletion_operations), \
           (select count(*) from threads_archive.local_source_removals), \
           (select count(*) from threads_archive.outbox_events \
             where event_type = 'social.source.removed.v1')",
    )
    .fetch_one(pool)
    .await
    .expect("final deletion counts read");
    let (event_payload,): (serde_json::Value,) = sqlx::query_as(
        "select payload from threads_archive.outbox_events \
         where event_type = 'social.source.removed.v1'",
    )
    .fetch_one(pool)
    .await
    .expect("typed removal event reads");
    let audit = format!("{result:?}");
    let event = event_payload.to_string();

    assert_eq!(counts, (0, 0, 0, 1, 1, 1));
    assert!(event.contains("user_requested") && event.contains(&social_source_id.to_string()));
    for secret in [
        "PRIVATE BODY SENTINEL",
        "PRIVATE NOTE SENTINEL",
        "PRIVATE_URL_SENTINEL",
    ] {
        assert!(
            !audit.contains(secret) && !event.contains(secret),
            "deletion evidence leaked {secret}"
        );
    }

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn connection_deletion_erases_credentials_but_preserves_an_independent_explicit_capture() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();
    let owner = uuid::Uuid::now_v7();
    let account_id = uuid::Uuid::now_v7();
    insert_connection_with_credential(pool, owner, account_id)
        .await
        .expect("connection and encrypted credential store");
    let post_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.posts \
         (post_id, account_id, provider_post_id, permalink, post_kind, acquisition_method, \
          saved_authority, upstream_status) values ($1, $2, 'connection-shared-post', \
          'https://www.threads.net/@safe/post/account-shared', 'post', 'official_api', \
          'authoritative_platform_state', 'active')",
    )
    .bind(post_id)
    .bind(account_id)
    .execute(pool)
    .await
    .expect("official post stores");
    let capture_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.captures \
         (capture_id, user_ref, post_id, idempotency_key, canonical_url, original_url, \
          acquisition_method, saved_authority, client_source, status, captured_at) \
         values ($1, $2, $3, 'independent-capture', \
          'https://www.threads.net/@safe/post/account-shared', \
          'https://threads.net/@safe/post/account-shared', 'share_extension', \
          'explicit_user_capture', 'ios_share_extension', 'resolved', now())",
    )
    .bind(capture_id)
    .bind(owner)
    .bind(post_id)
    .execute(pool)
    .await
    .expect("independent capture stores");

    let result = DeletionStore::new(&test.database)
        .apply(DeletionRequest {
            operation_id: uuid::Uuid::now_v7(),
            user_ref: owner,
            target: DeletionTarget::Connection(account_id),
        })
        .await
        .expect("connection deletion succeeds");
    let stored: (i64, i64, i64, Option<uuid::Uuid>, String, String) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.accounts), \
           (select count(*) from threads_archive.credentials), \
           (select count(*) from threads_archive.captures where capture_id = $1), \
           (select account_id from threads_archive.posts where post_id = $2), \
           (select acquisition_method from threads_archive.captures where capture_id = $1), \
           (select saved_authority from threads_archive.captures where capture_id = $1)",
    )
    .bind(capture_id)
    .bind(post_id)
    .fetch_one(pool)
    .await
    .expect("connection deletion outcome reads");

    assert_eq!(
        stored,
        (
            0,
            0,
            1,
            None,
            "share_extension".to_owned(),
            "explicit_user_capture".to_owned()
        )
    );
    assert_eq!(
        result
            .effects
            .iter()
            .find(|effect| effect.class.key() == "table:posts")
            .map(|effect| (effect.action, effect.affected_count)),
        Some((
            ratatoskr_threads_archive::privacy_deletion::DeletionAction::RetainShared,
            1
        ))
    );

    test.cleanup().await.expect("cleanup must drop");
}

async fn insert_connection_with_credential(
    pool: &sqlx::PgPool,
    owner: uuid::Uuid,
    account_id: uuid::Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "insert into threads_archive.accounts \
         (account_id, user_ref, provider_account_id, username, account_type, connection_status, \
          scopes, connected_at) values ($1, $2, $3, 'redacted', 'personal', 'connected', \
          'threads_basic', now())",
    )
    .bind(account_id)
    .bind(owner)
    .bind(format!("provider-{account_id}"))
    .execute(pool)
    .await?;
    sqlx::query(
        "insert into threads_archive.credentials \
         (credential_id, account_id, access_token_ciphertext, token_version, scopes) \
         values ($1, $2, $3, 1, 'threads_basic')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(account_id)
    .bind(vec![0x77_u8; 32])
    .execute(pool)
    .await?;
    Ok(())
}

#[tokio::test]
async fn connection_only_sources_each_emit_one_removal_and_replay_is_idempotent() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let pool = test.database.pool();
    let owner = uuid::Uuid::now_v7();
    let account_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.accounts \
         (account_id, user_ref, provider_account_id, username, account_type, connection_status, \
          scopes, connected_at) values ($1, $2, $3, 'redacted', 'personal', 'connected', \
          'threads_basic', now())",
    )
    .bind(account_id)
    .bind(owner)
    .bind(format!("provider-{account_id}"))
    .execute(pool)
    .await
    .expect("connection stores");
    let mut source_ids = Vec::new();
    for index in 0..2 {
        let post_id = uuid::Uuid::now_v7();
        sqlx::query(
            "insert into threads_archive.posts \
             (post_id, account_id, provider_post_id, permalink, post_kind, acquisition_method, \
              saved_authority, upstream_status) values ($1, $2, $3, $4, 'post', 'official_api', \
              'authoritative_platform_state', 'active')",
        )
        .bind(post_id)
        .bind(account_id)
        .bind(format!("connection-only-{index}"))
        .bind(format!(
            "https://www.threads.net/@safe/post/connection-only-{index}"
        ))
        .execute(pool)
        .await
        .expect("connection-only post stores");
        let source_id = uuid::Uuid::now_v7();
        sqlx::query(
            "insert into threads_archive.social_sources \
             (social_source_id, user_ref, post_id, first_capture_id) values ($1, $2, $3, null)",
        )
        .bind(source_id)
        .bind(owner)
        .bind(post_id)
        .execute(pool)
        .await
        .expect("connection-only source stores");
        source_ids.push(source_id);
    }
    source_ids.sort();
    let request = DeletionRequest {
        operation_id: uuid::Uuid::now_v7(),
        user_ref: owner,
        target: DeletionTarget::Connection(account_id),
    };
    let store = DeletionStore::new(&test.database);

    let first = store
        .apply(request)
        .await
        .expect("first delivery completes");
    let replay = store
        .apply(request)
        .await
        .expect("completed delivery replays");
    let removed_sources: Vec<(uuid::Uuid,)> = sqlx::query_as(
        "select (payload->'payload'->>'social_source_id')::uuid \
         from threads_archive.outbox_events where event_type = 'social.source.removed.v1' \
         order by (payload->'payload'->>'social_source_id')::uuid",
    )
    .fetch_all(pool)
    .await
    .expect("removal facts read");
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.accounts), \
           (select count(*) from threads_archive.posts), \
           (select count(*) from threads_archive.deletion_operations), \
           (select count(*) from threads_archive.local_source_removals)",
    )
    .fetch_one(pool)
    .await
    .expect("connection replay counts read");

    assert_eq!(first, replay);
    assert_eq!(
        removed_sources,
        source_ids.into_iter().map(|id| (id,)).collect::<Vec<_>>()
    );
    assert_eq!(counts, (0, 0, 1, 2));

    test.cleanup().await.expect("cleanup must drop");
}
