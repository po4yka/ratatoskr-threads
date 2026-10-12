//! A `social.source.removed.v1` fact that Knowledge has not acknowledged is the only copy of the
//! deletion this service can send again, so no deletion path may remove it (XR-021 CONTRACTS.md
//! R5-11 rule 4). One test per deleting path: the capture target and the connection target.

#![allow(
    clippy::expect_used,
    reason = "fixture helpers in a test binary: an unanswered insert is the failure"
)]

use ratatoskr_threads_archive::privacy_deletion::{DeletionRequest, DeletionStore, DeletionTarget};
use ratatoskr_threads_archive::test_support::TestDatabase;
use sqlx::PgPool;
use uuid::Uuid;

const REMOVED: &str = "social.source.removed.v1";
const CAPTURED: &str = "social.source.captured.v1";

/// A resolved capture of `owner` over a post of its own, with its social source.
async fn capture_with_source(pool: &PgPool, owner: Uuid, label: &str) -> (Uuid, Uuid, Uuid) {
    let (post_id, capture_id, source_id) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    sqlx::query(
        "insert into threads_archive.posts \
         (post_id, provider_post_id, permalink, post_kind, acquisition_method, saved_authority, \
          upstream_status) values ($1, $2, $3, 'post', 'public_resolution', \
          'explicit_user_capture', 'active')",
    )
    .bind(post_id)
    .bind(format!("provider-{label}"))
    .bind(format!("https://www.threads.net/@safe/post/{label}"))
    .execute(pool)
    .await
    .expect("the post stores");
    sqlx::query(
        "insert into threads_archive.captures \
         (capture_id, user_ref, post_id, idempotency_key, canonical_url, original_url, \
          acquisition_method, saved_authority, client_source, status, captured_at) \
         values ($1, $2, $3, $4, $5, $5, 'share_extension', 'explicit_user_capture', \
          'ios_share_extension', 'resolved', now())",
    )
    .bind(capture_id)
    .bind(owner)
    .bind(post_id)
    .bind(format!("key-{label}"))
    .bind(format!("https://www.threads.net/@safe/post/{label}"))
    .execute(pool)
    .await
    .expect("the capture stores");
    sqlx::query(
        "insert into threads_archive.social_sources \
         (social_source_id, user_ref, post_id, first_capture_id) values ($1, $2, $3, $4)",
    )
    .bind(source_id)
    .bind(owner)
    .bind(post_id)
    .bind(capture_id)
    .execute(pool)
    .await
    .expect("the source stores");
    (post_id, capture_id, source_id)
}

async fn delete(test: &TestDatabase, owner: Uuid, target: DeletionTarget) {
    DeletionStore::new(&test.database)
        .apply(DeletionRequest {
            operation_id: Uuid::now_v7(),
            user_ref: owner,
            target,
        })
        .await
        .expect("the deletion succeeds");
}

/// `(event_id, payload, published, acknowledged)` of every removal row, by event id.
async fn removals(pool: &PgPool) -> Vec<(Uuid, serde_json::Value, bool, bool)> {
    sqlx::query_as(
        "select event_id, payload, published_at is not null, acknowledged_at is not null \
           from threads_archive.outbox_events where event_type = $1 order by event_id",
    )
    .bind(REMOVED)
    .fetch_all(pool)
    .await
    .expect("the removal rows read")
}

#[tokio::test]
async fn a_later_capture_deletion_keeps_an_earlier_unacknowledged_removal() {
    let test = TestDatabase::create().await.expect("a test database");
    let pool = test.database.pool();
    let owner = Uuid::now_v7();
    let (_, first_capture, _) = capture_with_source(pool, owner, "first").await;
    let (_, second_capture, _) = capture_with_source(pool, owner, "second").await;
    // A fact filed under the capture the later deletion removes, so that deletion does run its
    // outbox cleanup over rows.
    let captured_fact = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at) \
         values ($1, $2, 'capture', $3, '{}'::jsonb, now())",
    )
    .bind(captured_fact)
    .bind(CAPTURED)
    .bind(second_capture)
    .execute(pool)
    .await
    .expect("the captured fact stores");

    delete(&test, owner, DeletionTarget::Capture(first_capture)).await;
    let earlier = removals(pool).await;
    assert_eq!(earlier.len(), 1, "the first deletion queued one removal");
    // The removal was published and armed for resending but Knowledge has not answered.
    sqlx::query(
        "update threads_archive.outbox_events set published_at = now(), \
         next_resend_at = now() + interval '10 minutes' where event_id = $1",
    )
    .bind(earlier[0].0)
    .execute(pool)
    .await
    .expect("the removal is published");
    let published = removals(pool).await;

    delete(&test, owner, DeletionTarget::Capture(second_capture)).await;

    let after = removals(pool).await;
    assert_eq!(after.len(), 2, "the later removal joined the earlier one");
    let kept = after
        .iter()
        .find(|row| row.0 == earlier[0].0)
        .expect("the earlier unacknowledged removal survives");
    assert_eq!(
        kept, &published[0],
        "its stored bytes and state are unchanged"
    );
    assert!(!kept.3, "it is still unacknowledged");
    let fact: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.outbox_events where event_id = $1",
    )
    .bind(captured_fact)
    .fetch_one(pool)
    .await
    .expect("counting the captured fact");
    assert_eq!(
        fact, 0,
        "the deletion did clean up the facts of the removed capture"
    );
    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn a_connection_deletion_keeps_every_removal_it_queued_unacknowledged() {
    let test = TestDatabase::create().await.expect("a test database");
    let pool = test.database.pool();
    let owner = Uuid::now_v7();
    let account_id = Uuid::now_v7();
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
    .expect("the connection stores");
    // Three connection-only sources: the removal queued for the first one sits under the same
    // aggregate (the account) that the cleanup of the second and third one filters on.
    for index in 0..3 {
        let post_id = Uuid::now_v7();
        sqlx::query(
            "insert into threads_archive.posts \
             (post_id, account_id, provider_post_id, permalink, post_kind, acquisition_method, \
              saved_authority, upstream_status) values ($1, $2, $3, $4, 'post', 'official_api', \
              'authoritative_platform_state', 'active')",
        )
        .bind(post_id)
        .bind(account_id)
        .bind(format!("connection-{index}"))
        .bind(format!(
            "https://www.threads.net/@safe/post/connection-{index}"
        ))
        .execute(pool)
        .await
        .expect("the post stores");
        sqlx::query(
            "insert into threads_archive.social_sources \
             (social_source_id, user_ref, post_id, first_capture_id) values ($1, $2, $3, null)",
        )
        .bind(Uuid::now_v7())
        .bind(owner)
        .bind(post_id)
        .execute(pool)
        .await
        .expect("the source stores");
    }

    delete(&test, owner, DeletionTarget::Connection(account_id)).await;

    let after = removals(pool).await;
    assert_eq!(
        after.len(),
        3,
        "one removal per source survives the whole deletion"
    );
    for (_, payload, published, acknowledged) in &after {
        assert_eq!(payload["event_type"], REMOVED);
        assert!(!published && !acknowledged, "queued and unacknowledged");
    }
    test.cleanup().await.expect("cleanup must drop");
}
