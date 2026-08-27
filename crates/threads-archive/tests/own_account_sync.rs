//! Official own-account synchronization behavior tests.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use ratatoskr_threads_archive::account_sync::{
    OfficialOwnContentPage, OfficialOwnContentProvider, OfficialOwnPost, OwnAccountSyncStore,
    SyncOutcome,
};
use ratatoskr_threads_archive::capture::{
    CaptureMethod, CaptureRequest, CaptureStore, ClientSource, SubmitOutcome,
};
use ratatoskr_threads_archive::oauth::CapabilityAvailability;
use ratatoskr_threads_archive::permalink::CanonicalizedUrl;
use ratatoskr_threads_archive::public_resolution::RawObjectStore;
use ratatoskr_threads_archive::public_resolution::{
    PARSER_VERSION, PublicPost, PublicResolutionStore,
};
use ratatoskr_threads_archive::test_support::TestDatabase;
use uuid::Uuid;

#[derive(Debug, Default)]
struct FakeOfficialOwnContentProvider {
    list_calls: AtomicUsize,
    watermarks: Mutex<Vec<Option<String>>>,
    pages: Mutex<VecDeque<OfficialOwnContentPage>>,
}

impl OfficialOwnContentProvider for FakeOfficialOwnContentProvider {
    #[expect(
        clippy::expect_used,
        reason = "a poisoned test fake mutex invalidates the test itself"
    )]
    async fn list_own_content(
        &self,
        _account_id: Uuid,
        watermark: Option<&str>,
    ) -> Result<OfficialOwnContentPage, ratatoskr_threads_archive::account_sync::OwnAccountSyncError>
    {
        self.list_calls.fetch_add(1, Ordering::Relaxed);
        self.watermarks
            .lock()
            .expect("test mutex")
            .push(watermark.map(str::to_owned));
        self.pages
            .lock()
            .expect("test mutex")
            .pop_front()
            .ok_or(ratatoskr_threads_archive::account_sync::OwnAccountSyncError::Unavailable)
    }
}

impl FakeOfficialOwnContentProvider {
    fn with_pages(pages: Vec<OfficialOwnContentPage>) -> Self {
        Self {
            list_calls: AtomicUsize::new(0),
            watermarks: Mutex::new(Vec::new()),
            pages: Mutex::new(pages.into()),
        }
    }
}

#[tokio::test]
async fn sync_without_own_content_capability_is_a_non_mutating_no_op() {
    let test = TestDatabase::create().await.expect("a disposable database");
    let account_id = insert_account(&test).await;
    sqlx::query(
        "insert into threads_archive.account_sync_checkpoints (account_id, watermark) \
         values ($1, 'checkpoint-before-no-op')",
    )
    .bind(account_id)
    .execute(test.database.pool())
    .await
    .expect("fixture checkpoint inserts");
    let provider = FakeOfficialOwnContentProvider::default();
    let raw_root = raw_root();
    let store = OwnAccountSyncStore::new(test.database.clone(), RawObjectStore::new(&raw_root));
    let outcome = store
        .sync(
            &provider,
            account_id,
            &CapabilityAvailability::Unavailable(
                "missing required scope: threads_basic".to_owned(),
            ),
        )
        .await
        .expect("missing capability must return a truthful no-op");
    let checkpoint: String = sqlx::query_scalar(
        "select watermark from threads_archive.account_sync_checkpoints where account_id = $1",
    )
    .bind(account_id)
    .fetch_one(test.database.pool())
    .await
    .expect("checkpoint reads");

    assert_eq!(
        outcome,
        SyncOutcome::NoOp("missing required scope: threads_basic".to_owned())
    );
    assert_eq!(provider.list_calls.load(Ordering::Relaxed), 0);
    assert_eq!(checkpoint, "checkpoint-before-no-op");

    test.cleanup().await.expect("cleanup");
    assert!(
        !tokio::fs::try_exists(raw_root)
            .await
            .expect("raw fixture path checks"),
        "no-op must not create a raw-response directory"
    );
}

#[tokio::test]
async fn completed_scan_advances_and_reuses_account_watermark() {
    let test = TestDatabase::create().await.expect("a disposable database");
    let provider = FakeOfficialOwnContentProvider::with_pages(vec![
        page("own-post-001", Some("watermark-001")),
        page("own-post-002", Some("watermark-002")),
    ]);
    let raw_root = raw_root();
    let store = OwnAccountSyncStore::new(test.database.clone(), RawObjectStore::new(&raw_root));
    let account_id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.accounts \
         (account_id, user_ref, provider_account_id, username, account_type, connection_status, scopes, connected_at) \
         values ($1, $2, $3, 'fixture', 'creator', 'connected', 'threads_basic', now())",
    )
    .bind(account_id)
    .bind(Uuid::now_v7())
    .bind(format!("provider-{account_id}"))
    .execute(test.database.pool())
    .await
    .expect("fixture account inserts");

    let first = store
        .sync(&provider, account_id, &CapabilityAvailability::Available)
        .await
        .expect("first completed page persists");
    let second = store
        .sync(&provider, account_id, &CapabilityAvailability::Available)
        .await
        .expect("second completed page persists");

    assert_eq!(
        first,
        SyncOutcome::Completed {
            next_watermark: Some("watermark-001".to_owned())
        }
    );
    assert_eq!(
        second,
        SyncOutcome::Completed {
            next_watermark: Some("watermark-002".to_owned())
        }
    );
    assert_eq!(
        *provider.watermarks.lock().expect("test mutex"),
        vec![None, Some("watermark-001".to_owned())]
    );
    let watermark: String = sqlx::query_scalar(
        "select watermark from threads_archive.account_sync_checkpoints where account_id = $1",
    )
    .bind(account_id)
    .fetch_one(test.database.pool())
    .await
    .expect("checkpoint persists");
    assert_eq!(watermark, "watermark-002");

    test.cleanup().await.expect("cleanup");
    tokio::fs::remove_dir_all(raw_root)
        .await
        .expect("raw fixture directory cleans up");
}

#[tokio::test]
async fn failed_scan_keeps_the_previous_account_watermark() {
    let test = TestDatabase::create().await.expect("a disposable database");
    let account_id = insert_account(&test).await;
    let provider = FakeOfficialOwnContentProvider::with_pages(vec![page(
        "own-post-failure",
        Some("watermark-before-failure"),
    )]);
    let raw_root = raw_root();
    let store = OwnAccountSyncStore::new(test.database.clone(), RawObjectStore::new(&raw_root));

    store
        .sync(&provider, account_id, &CapabilityAvailability::Available)
        .await
        .expect("first completed page persists");
    let error = store
        .sync(&provider, account_id, &CapabilityAvailability::Available)
        .await
        .expect_err("the fake has no second page");
    let watermark: String = sqlx::query_scalar(
        "select watermark from threads_archive.account_sync_checkpoints where account_id = $1",
    )
    .bind(account_id)
    .fetch_one(test.database.pool())
    .await
    .expect("checkpoint reads");

    assert!(matches!(
        error,
        ratatoskr_threads_archive::account_sync::OwnAccountSyncError::Unavailable
    ));
    assert_eq!(watermark, "watermark-before-failure");

    test.cleanup().await.expect("cleanup");
    tokio::fs::remove_dir_all(raw_root)
        .await
        .expect("raw fixture directory cleans up");
}

#[tokio::test]
async fn official_observation_atomically_swaps_a_captured_post_authority() {
    let test = TestDatabase::create().await.expect("a disposable database");
    let account_id = insert_account(&test).await;
    let capture_request = CaptureRequest::try_new(
        Uuid::now_v7(),
        "official-swap".to_owned(),
        "https://www.threads.net/@fixture/post/own-post-001",
        None,
        CaptureMethod::ShareExtension,
        ClientSource::IosShareExtension,
    )
    .expect("fixture capture is valid");
    let submitted = CaptureStore::new(&test.database)
        .submit(&capture_request)
        .await
        .expect("capture stores");
    assert!(matches!(submitted, SubmitOutcome::Created(_)));
    let SubmitOutcome::Created(capture) = submitted else {
        return;
    };
    let raw_root = raw_root();
    let resolver = PublicResolutionStore::new(&test.database, RawObjectStore::new(&raw_root));
    let permalink = capture_request.canonical_url().clone();
    resolver
        .record(
            capture.capture_id,
            &PublicPost {
                provider_post_id: "own-post-001".to_owned(),
                permalink: permalink.clone(),
                embed_html: "Synthetic public text".to_owned(),
                parser_version: PARSER_VERSION,
                relations: Vec::new(),
            },
            b"{\"fixture\":\"public\"}",
        )
        .await
        .expect("public observation stores");
    let provider = FakeOfficialOwnContentProvider::with_pages(vec![page(
        "own-post-001",
        Some("watermark-swap"),
    )]);
    let store = OwnAccountSyncStore::new(test.database.clone(), RawObjectStore::new(&raw_root));

    store
        .sync(&provider, account_id, &CapabilityAvailability::Available)
        .await
        .expect("official observation stores");

    let post: (String, String, i64) = sqlx::query_as(
        "select acquisition_method, saved_authority, count(*) over () \
         from threads_archive.posts where provider_post_id = 'own-post-001'",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("official post reads");
    let retained_capture: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.captures where capture_id = $1 and post_id is not null",
    )
    .bind(capture.capture_id)
    .fetch_one(test.database.pool())
    .await
    .expect("capture link reads");
    let event_count: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.outbox_events where aggregate_type = 'capture'",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("source facts read");

    assert_eq!(
        post,
        (
            "official_api".to_owned(),
            "authoritative_platform_state".to_owned(),
            1
        )
    );
    assert_eq!(retained_capture, 1);
    assert_eq!(
        event_count, 2,
        "authority swap must republish the source state"
    );

    test.cleanup().await.expect("cleanup");
    tokio::fs::remove_dir_all(raw_root)
        .await
        .expect("raw fixture directory cleans up");
}

#[tokio::test]
async fn official_reply_publishes_its_parent_relation_with_official_provenance() {
    let test = TestDatabase::create().await.expect("a disposable database");
    let account_id = insert_account(&test).await;
    let provider = FakeOfficialOwnContentProvider::with_pages(vec![reply_page()]);
    let raw_root = raw_root();
    let store = OwnAccountSyncStore::new(test.database.clone(), RawObjectStore::new(&raw_root));

    store
        .sync(&provider, account_id, &CapabilityAvailability::Available)
        .await
        .expect("official reply stores");

    let relation: (String, String) = sqlx::query_as(
        "select relation_kind, target_provider_post_id from threads_archive.post_relations",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("reply relation stores");
    let fact: serde_json::Value = sqlx::query_scalar(
        "select payload from threads_archive.outbox_events where aggregate_type = 'post'",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("official source fact stores");
    let source = &fact["payload"]["source"];

    assert_eq!(relation, ("reply".to_owned(), "parent-post-001".to_owned()));
    assert_eq!(source["acquisition"], "official_api");
    assert_eq!(source["saved_authority"], "authoritative_platform_state");

    test.cleanup().await.expect("cleanup");
    tokio::fs::remove_dir_all(raw_root)
        .await
        .expect("raw fixture directory cleans up");
}

#[expect(
    clippy::expect_used,
    reason = "fixed synthetic fixture inputs must parse"
)]
fn page(provider_post_id: &str, watermark: Option<&str>) -> OfficialOwnContentPage {
    let url = format!("https://www.threads.net/@fixture/post/{provider_post_id}");
    OfficialOwnContentPage {
        raw_response: include_bytes!("fixtures/official-own-content-page.json").to_vec(),
        posts: vec![OfficialOwnPost {
            provider_post_id: provider_post_id.to_owned(),
            permalink: CanonicalizedUrl::try_from(url.as_str())
                .expect("synthetic permalink normalizes")
                .permalink()
                .clone(),
            text_content: Some("Synthetic own post".to_owned()),
            published_at: None,
            reply_to_provider_post_id: None,
        }],
        next_watermark: watermark.map(str::to_owned),
    }
}

#[expect(
    clippy::expect_used,
    reason = "fixed synthetic reply fixture input must parse"
)]
fn reply_page() -> OfficialOwnContentPage {
    let permalink =
        CanonicalizedUrl::try_from("https://www.threads.net/@fixture/post/own-reply-001")
            .expect("synthetic reply permalink normalizes")
            .permalink()
            .clone();
    OfficialOwnContentPage {
        raw_response: include_bytes!("fixtures/official-own-content-reply-page.json").to_vec(),
        posts: vec![OfficialOwnPost {
            provider_post_id: "own-reply-001".to_owned(),
            permalink,
            text_content: Some("Synthetic own reply".to_owned()),
            published_at: None,
            reply_to_provider_post_id: Some("parent-post-001".to_owned()),
        }],
        next_watermark: Some("watermark-reply".to_owned()),
    }
}

fn raw_root() -> PathBuf {
    std::env::temp_dir().join(format!("ratatoskr-threads-own-sync-{}", Uuid::now_v7()))
}

#[expect(clippy::expect_used, reason = "synthetic database fixture setup")]
async fn insert_account(test: &TestDatabase) -> Uuid {
    let account_id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.accounts \
         (account_id, user_ref, provider_account_id, username, account_type, connection_status, scopes, connected_at) \
         values ($1, $2, $3, 'fixture', 'creator', 'connected', 'threads_basic', now())",
    )
    .bind(account_id)
    .bind(Uuid::now_v7())
    .bind(format!("provider-{account_id}"))
    .execute(test.database.pool())
    .await
    .expect("fixture account inserts");
    account_id
}
