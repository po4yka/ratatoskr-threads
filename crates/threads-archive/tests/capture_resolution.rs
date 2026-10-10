//! The capture resolver against a real database and a local HTTP fake of the oEmbed endpoint.

mod common;

use chrono::{DateTime, Duration, SubsecRound as _, Utc};
use ratatoskr_threads_archive::browser_capture_command::BrowserCaptureCommand;
use ratatoskr_threads_archive::browser_capture_inbox::BrowserCaptureInbox;
use ratatoskr_threads_archive::capture_resolution::{CaptureResolver, ResolverPolicy};
use ratatoskr_threads_archive::permalink::CanonicalizedUrl;
use ratatoskr_threads_archive::public_resolution::{
    ApprovedOembedClient, PublicResolutionStore, RawObjectStore,
};
use ratatoskr_threads_archive::test_support::TestDatabase;
use ratatoskr_threads_archive::test_support::fake_oembed::{FakeOembed, FakeReply};
use sqlx::PgPool;
use uuid::Uuid;

use common::{PERMALINK, threads_command};

const MAX_ATTEMPTS: u32 = 5;

struct Harness {
    database: TestDatabase,
    fake: FakeOembed,
    resolver: CaptureResolver,
    capture_id: Uuid,
    owner: Uuid,
}

impl Harness {
    fn pool(&self) -> &PgPool {
        self.database.database.pool()
    }
}

/// A delivered capture waiting to be resolved, with a fake endpoint answering `replies`.
#[expect(
    clippy::expect_used,
    reason = "test fixture: any failure to set the scenario up is the failure"
)]
async fn harness(replies: Vec<FakeReply>) -> Harness {
    let database = TestDatabase::create().await.expect("a test database");
    let fake = FakeOembed::start(PERMALINK, replies)
        .await
        .expect("the fake endpoint binds");
    let command = BrowserCaptureCommand::parse(&threads_command(Uuid::now_v7()))
        .expect("the Threads command is accepted");
    let capture_id = BrowserCaptureInbox::new(&database.database)
        .deliver(&command)
        .await
        .expect("the command persists")
        .capture_id()
        .expect("the first delivery is accepted");
    let raw_root = std::env::temp_dir().join(format!("ratatoskr-threads-raw-{}", Uuid::now_v7()));
    let resolver = CaptureResolver::new(
        database.database.clone(),
        ApprovedOembedClient::for_loopback_test(&fake.endpoint()).expect("a loopback endpoint"),
        RawObjectStore::new(raw_root),
        ResolverPolicy {
            max_attempts: MAX_ATTEMPTS,
            batch_size: 8,
            poll_interval: std::time::Duration::from_millis(10),
        },
    );
    Harness {
        database,
        fake,
        resolver,
        capture_id,
        owner: command.user_ref(),
    }
}

/// The payloads of every operation report in the outbox, oldest first.
#[expect(clippy::expect_used, reason = "test helper: an unanswered query fails")]
async fn reports(pool: &PgPool) -> Vec<serde_json::Value> {
    sqlx::query_scalar(
        "select payload from threads_archive.outbox_events \
         where event_type = 'platform.operation.reported.v1' order by occurred_at, event_id",
    )
    .fetch_all(pool)
    .await
    .expect("reading reports")
}

/// The terminal report payloads (everything after the queued one).
async fn terminal_reports(pool: &PgPool) -> Vec<serde_json::Value> {
    reports(pool)
        .await
        .into_iter()
        .filter(|envelope| {
            envelope.pointer("/payload/status") != Some(&serde_json::json!("queued"))
        })
        .collect()
}

#[expect(clippy::expect_used, reason = "test helper: an unanswered query fails")]
async fn scalar_count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("counting rows")
}

#[expect(clippy::expect_used, reason = "test helper: an unanswered query fails")]
async fn capture_state(pool: &PgPool, capture_id: Uuid) -> (String, i32, Option<DateTime<Utc>>) {
    sqlx::query_as(
        "select status, resolution_attempts, next_resolution_at \
         from threads_archive.captures where capture_id = $1",
    )
    .bind(capture_id)
    .fetch_one(pool)
    .await
    .expect("reading the capture")
}

#[expect(
    clippy::expect_used,
    reason = "test helper: a failed pass fails the test"
)]
async fn run_at(harness: &Harness, now: DateTime<Utc>) -> (usize, usize) {
    let summary = harness
        .resolver
        .run_due_once(now)
        .await
        .expect("the resolver pass completes");
    (summary.claimed, summary.reported)
}

#[tokio::test]
async fn a_public_post_is_preserved_and_reported_exactly_once() {
    let harness = harness(vec![FakeReply::Oembed]).await;
    let pool = harness.pool();

    assert_eq!(run_at(&harness, Utc::now()).await, (1, 1));

    let (status, attempts, _due) = capture_state(pool, harness.capture_id).await;
    assert_eq!((status.as_str(), attempts), ("resolved", 0));
    let reported: Option<DateTime<Utc>> = sqlx::query_scalar(
        "select operation_reported_at from threads_archive.captures where capture_id = $1",
    )
    .bind(harness.capture_id)
    .fetch_one(pool)
    .await
    .expect("reading the idempotency guard");
    assert!(reported.is_some(), "the terminal report sets the guard");

    let facts: Vec<serde_json::Value> = sqlx::query_scalar(
        "select payload from threads_archive.outbox_events \
         where event_type = 'social.source.captured.v1'",
    )
    .fetch_all(pool)
    .await
    .expect("reading facts");
    assert_eq!(facts.len(), 1, "one captured fact");
    assert_eq!(
        facts[0]["tenant_id"],
        format!("user:{}", harness.owner),
        "the fact is owned by the command's tenant"
    );

    let source: Uuid = sqlx::query_scalar(
        "select social_source_id from threads_archive.social_sources where user_ref = $1",
    )
    .bind(harness.owner)
    .fetch_one(pool)
    .await
    .expect("the stored source identity");
    let terminal = terminal_reports(pool).await;
    assert_eq!(terminal.len(), 1, "exactly one terminal report");
    let payload = &terminal[0]["payload"];
    assert_eq!(payload["status"], "succeeded");
    assert_eq!(payload["stage"], "capture_preserved");
    assert_eq!(payload["results"][0]["result_kind"], "social.post");
    assert_eq!(
        payload["results"][0]["target"],
        format!("social_source:{source}"),
        "the report names the actual stored source"
    );
    assert_eq!(
        terminal[0]["tenant_id"],
        format!("user:{}", harness.owner),
        "the report is owned by the command's tenant"
    );

    assert_eq!(
        run_at(&harness, Utc::now() + Duration::hours(2)).await,
        (0, 0)
    );
    assert_eq!(terminal_reports(pool).await.len(), 1);
    assert_eq!(
        harness.fake.requests(),
        1,
        "a reported capture is never refetched"
    );
    harness.database.cleanup().await.expect("cleanup");
}

#[tokio::test]
async fn a_deleted_post_is_tombstoned_and_reported_as_deleted() {
    let harness = harness(vec![FakeReply::Status(404)]).await;
    let pool = harness.pool();

    assert_eq!(run_at(&harness, Utc::now()).await, (1, 1));

    let (status, _attempts, _due) = capture_state(pool, harness.capture_id).await;
    assert_eq!(status, "unavailable");
    assert_eq!(
        scalar_count(
            pool,
            "select count(*) from threads_archive.tombstones where availability = 'deleted'"
        )
        .await,
        1
    );
    let terminal = terminal_reports(pool).await;
    assert_eq!(terminal.len(), 1);
    let payload = &terminal[0]["payload"];
    assert_eq!(payload["status"], "failed");
    assert_eq!(payload["error"]["code"], "social.source.deleted");
    assert_eq!(payload["error"]["retryable"], false);
    assert_eq!(
        scalar_count(
            pool,
            "select count(*) from threads_archive.outbox_events \
             where event_type = 'social.source.captured.v1'"
        )
        .await,
        0,
        "nothing was preserved, so no fact"
    );
    assert_eq!(
        run_at(&harness, Utc::now() + Duration::hours(2)).await,
        (0, 0)
    );
    harness.database.cleanup().await.expect("cleanup");
}

#[tokio::test]
async fn a_forbidden_post_is_reported_as_permanently_unavailable() {
    let harness = harness(vec![FakeReply::Status(403)]).await;
    let pool = harness.pool();

    assert_eq!(run_at(&harness, Utc::now()).await, (1, 1));

    let (status, _attempts, _due) = capture_state(pool, harness.capture_id).await;
    assert_eq!(status, "unavailable");
    let terminal = terminal_reports(pool).await;
    assert_eq!(terminal.len(), 1);
    let payload = &terminal[0]["payload"];
    assert_eq!(payload["status"], "failed");
    assert_eq!(payload["stage"], "capture_unavailable");
    assert_eq!(payload["error"]["code"], "social.source.unavailable");
    assert_eq!(payload["error"]["retryable"], false);
    harness.database.cleanup().await.expect("cleanup");
}

#[tokio::test]
async fn transient_failures_back_off_on_the_bounded_schedule_then_succeed() {
    let mut replies = vec![FakeReply::Status(503); 4];
    replies.push(FakeReply::Oembed);
    let harness = harness(replies).await;
    let pool = harness.pool();

    let mut now = Utc::now().trunc_subsecs(6);
    for (attempt, wait_seconds) in [(1, 30), (2, 120), (3, 480), (4, 1_800)] {
        assert_eq!(
            run_at(&harness, now).await,
            (1, 0),
            "attempt {attempt} is retried"
        );
        let (status, attempts, due) = capture_state(pool, harness.capture_id).await;
        assert_eq!((status.as_str(), attempts), ("accepted", attempt));
        let due = due.expect("a retry is scheduled");
        assert_eq!(
            due,
            now + Duration::seconds(wait_seconds),
            "attempt {attempt}"
        );
        assert_eq!(
            run_at(&harness, due - Duration::seconds(1)).await,
            (0, 0),
            "nothing runs before the retry is due"
        );
        now = due;
    }
    assert!(
        terminal_reports(pool).await.is_empty(),
        "no report while retrying"
    );

    assert_eq!(run_at(&harness, now).await, (1, 1));
    let terminal = terminal_reports(pool).await;
    assert_eq!(terminal.len(), 1);
    assert_eq!(terminal[0]["payload"]["status"], "succeeded");
    assert_eq!(harness.fake.requests(), 5);
    harness.database.cleanup().await.expect("cleanup");
}

#[tokio::test]
async fn exhausted_transient_failures_end_in_one_retryable_failure() {
    let harness = harness(vec![FakeReply::Status(503)]).await;
    let pool = harness.pool();

    let mut now = Utc::now();
    for attempt in 1..MAX_ATTEMPTS {
        assert_eq!(run_at(&harness, now).await, (1, 0), "attempt {attempt}");
        let (_status, _attempts, due) = capture_state(pool, harness.capture_id).await;
        now = due.expect("a retry is scheduled");
    }
    assert_eq!(
        run_at(&harness, now).await,
        (1, 1),
        "the last attempt reports"
    );

    let (status, attempts, _due) = capture_state(pool, harness.capture_id).await;
    assert_eq!(
        status, "accepted",
        "an exhausted retry is not deletion evidence"
    );
    assert_eq!(attempts, 5);
    let terminal = terminal_reports(pool).await;
    assert_eq!(terminal.len(), 1);
    let payload = &terminal[0]["payload"];
    assert_eq!(payload["status"], "failed");
    assert_eq!(payload["error"]["code"], "social.source.unavailable");
    assert_eq!(payload["error"]["retryable"], true);
    assert_eq!(
        scalar_count(
            pool,
            "select count(*) from threads_archive.capture_resolutions \
             where outcome = 'resolver_failed'"
        )
        .await,
        5,
        "every failed attempt is stored as evidence"
    );
    assert_eq!(
        scalar_count(pool, "select count(*) from threads_archive.tombstones").await,
        0
    );
    assert_eq!(run_at(&harness, now + Duration::hours(2)).await, (0, 0));
    assert_eq!(harness.fake.requests(), 5);
    harness.database.cleanup().await.expect("cleanup");
}

#[tokio::test]
async fn a_crash_between_recording_and_reporting_only_reports() {
    let harness = harness(vec![FakeReply::Oembed]).await;
    let pool = harness.pool();

    // The first run got as far as recording the observation, then died before the report.
    let client = ApprovedOembedClient::for_loopback_test(&harness.fake.endpoint())
        .expect("a loopback endpoint");
    let raw_root = std::env::temp_dir().join(format!("ratatoskr-threads-raw-{}", Uuid::now_v7()));
    let store =
        PublicResolutionStore::new(&harness.database.database, RawObjectStore::new(raw_root));
    let canonical = CanonicalizedUrl::try_from(PERMALINK).expect("a supported permalink");
    client
        .resolve_and_record(&store, harness.capture_id, canonical.permalink())
        .await
        .expect("the observation is recorded");
    let (status, _attempts, _due) = capture_state(pool, harness.capture_id).await;
    assert_eq!(status, "resolved");
    assert!(
        terminal_reports(pool).await.is_empty(),
        "the crash left no report"
    );

    assert_eq!(run_at(&harness, Utc::now()).await, (1, 1));

    let terminal = terminal_reports(pool).await;
    assert_eq!(terminal.len(), 1);
    assert_eq!(terminal[0]["payload"]["status"], "succeeded");
    assert_eq!(
        harness.fake.requests(),
        1,
        "the recorded capture is not fetched again"
    );
    harness.database.cleanup().await.expect("cleanup");
}
