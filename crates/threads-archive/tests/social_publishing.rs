//! Social-source publication integration tests use synthetic Threads evidence.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "assertions and synthetic fixture setup in an integration test"
)]

use ratatoskr_event_envelope::EventEnvelope;
use ratatoskr_identifiers::{Extensions, SocialSourceId, TenantRef, WireTimestamp};
use ratatoskr_social_contracts::{
    AcquisitionMethod, SavedAuthority, SocialSourceAnalysisCompleted, SocialSourceCaptured,
    SocialSourceUpdated, UpstreamAvailability,
};
use ratatoskr_threads_archive::capture::{
    CaptureMethod, CaptureRequest, CaptureStore, ClientSource, SubmitOutcome,
    UnavailabilityObservation,
};
use ratatoskr_threads_archive::knowledge::{CompletionLinkOutcome, KnowledgeCompletionStore};
use ratatoskr_threads_archive::permalink::CanonicalizedUrl;
use ratatoskr_threads_archive::public_resolution::{
    PARSER_VERSION, PublicPost, PublicResolutionStore, RawObjectStore,
};
use ratatoskr_threads_archive::test_support::TestDatabase;
use std::path::PathBuf;
use uuid::Uuid;

#[tokio::test]
async fn resolved_capture_appends_contract_conformant_captured_fact() {
    let database = TestDatabase::create()
        .await
        .expect("a disposable database is available");
    let capture_id = capture(&database).await;
    let raw_root = raw_root();
    let store = PublicResolutionStore::new(&database.database, RawObjectStore::new(&raw_root));
    store
        .record(capture_id, &post(), b"{\"html\":\"Public text\"}")
        .await
        .expect("the public observation is preserved");

    let event: Option<(String, serde_json::Value)> = sqlx::query_as(
        "select event_type, payload from threads_archive.outbox_events \
         where aggregate_type = 'capture' and aggregate_id = $1",
    )
    .bind(capture_id)
    .fetch_optional(database.database.pool())
    .await
    .expect("the outbox query answers");

    database.cleanup().await.expect("cleanup succeeds");
    tokio::fs::remove_dir_all(raw_root)
        .await
        .expect("raw fixture directory cleans up");

    assert_eq!(
        event.as_ref().map(|(event_type, _)| event_type.as_str()),
        Some("social.source.captured.v1"),
        "a preserved capture must append its state-carried social fact"
    );
    let (_, body) = event.expect("the captured fact is present");
    let envelope = EventEnvelope::from_json(
        serde_json::to_vec(&body)
            .expect("stored envelope re-serializes")
            .as_slice(),
    )
    .expect("stored event is a valid envelope");
    let fact = envelope
        .payload_as::<SocialSourceCaptured>()
        .expect("captured event has a typed social-source payload");
    assert_eq!(fact.source.platform.to_string(), "threads");
    assert_eq!(fact.source.acquisition, AcquisitionMethod::ShareExtension);
    assert_eq!(
        fact.source.saved_authority,
        SavedAuthority::ExplicitUserCapture
    );
    assert_eq!(
        fact.source.upstream_availability,
        UpstreamAvailability::Available
    );
    assert!(fact.source.permalink.is_some());
    assert!(fact.source.raw_blob.is_some());
}

#[tokio::test]
async fn matching_knowledge_completion_links_once_to_the_exact_source_revision() {
    let database = TestDatabase::create()
        .await
        .expect("a disposable database is available");
    let capture_id = capture(&database).await;
    let raw_root = raw_root();
    PublicResolutionStore::new(&database.database, RawObjectStore::new(&raw_root))
        .record(capture_id, &post(), b"{\"html\":\"Public text\"}")
        .await
        .expect("the public observation is preserved");
    let captured = captured_fact(&database, capture_id).await;
    let completion = SocialSourceAnalysisCompleted {
        owner: captured.source.owner,
        social_source_id: captured.source.social_source_id,
        content_digest: captured.source.content_digest.clone(),
        completed_at: WireTimestamp::now(),
        extensions: Extensions::default(),
    };
    let store = KnowledgeCompletionStore::new(&database.database);
    let event_id = Uuid::now_v7();
    let first = store
        .record(event_id, &completion)
        .await
        .expect("a matching completion is accepted");
    let replay = store
        .record(event_id, &completion)
        .await
        .expect("completion replay is accepted idempotently");
    let link: Option<(Uuid, String)> = sqlx::query_as(
        "select social_source_id, content_digest from threads_archive.social_analysis_links \
         where completion_event_id = $1",
    )
    .bind(event_id)
    .fetch_optional(database.database.pool())
    .await
    .expect("the persisted completion linkage is retrievable");

    database.cleanup().await.expect("cleanup succeeds");
    tokio::fs::remove_dir_all(raw_root)
        .await
        .expect("raw fixture directory cleans up");

    assert_eq!(first, CompletionLinkOutcome::Linked);
    assert_eq!(replay, CompletionLinkOutcome::Duplicate);
    assert_eq!(
        link,
        Some((
            completion
                .social_source_id
                .to_string()
                .parse()
                .expect("contract source id is a UUID"),
            completion.content_digest.hex.to_string(),
        ))
    );
}

#[tokio::test]
async fn post_tombstone_appends_deleted_upstream_update_without_erasing_evidence() {
    let database = TestDatabase::create()
        .await
        .expect("a disposable database is available");
    let capture_id = capture(&database).await;
    let raw_root = raw_root();
    PublicResolutionStore::new(&database.database, RawObjectStore::new(&raw_root))
        .record(capture_id, &post(), b"{\"html\":\"Public text\"}")
        .await
        .expect("the public observation is preserved");
    CaptureStore::new(&database.database)
        .record_observation(
            capture_id,
            &UnavailabilityObservation::deleted("provider-gone".to_owned())
                .expect("fixture deletion evidence is valid"),
        )
        .await
        .expect("the provider deletion is recorded");
    let events: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "select event_type, payload from threads_archive.outbox_events \
         where aggregate_type = 'capture' and aggregate_id = $1 order by occurred_at, event_id",
    )
    .bind(capture_id)
    .fetch_all(database.database.pool())
    .await
    .expect("the outbox query answers");
    let preserved_text: Option<String> = sqlx::query_scalar(
        "select text_content from threads_archive.posts where post_id = \
         (select post_id from threads_archive.captures where capture_id = $1)",
    )
    .bind(capture_id)
    .fetch_one(database.database.pool())
    .await
    .expect("the resolved post remains");

    database.cleanup().await.expect("cleanup succeeds");
    tokio::fs::remove_dir_all(raw_root)
        .await
        .expect("raw fixture directory cleans up");

    assert_eq!(
        events.len(),
        2,
        "a tombstone must append exactly one new fact"
    );
    assert_eq!(events[1].0, "social.source.updated.v1");
    let envelope = EventEnvelope::from_json(
        serde_json::to_vec(&events[1].1)
            .expect("stored envelope re-serializes")
            .as_slice(),
    )
    .expect("updated outbox row is a valid envelope");
    let update = envelope
        .payload_as::<SocialSourceUpdated>()
        .expect("updated event has a typed social-source payload");
    assert_eq!(
        update.source.upstream_availability,
        UpstreamAvailability::DeletedUpstream
    );
    assert_eq!(preserved_text.as_deref(), Some("Public text"));
    assert!(
        events
            .iter()
            .all(|(kind, _)| kind != "social.source.removed.v1")
    );
}

#[tokio::test]
async fn foreign_or_stale_knowledge_completion_does_not_link() {
    let database = TestDatabase::create()
        .await
        .expect("a disposable database is available");
    let capture_id = capture(&database).await;
    let raw_root = raw_root();
    let resolution = PublicResolutionStore::new(&database.database, RawObjectStore::new(&raw_root));
    resolution
        .record(capture_id, &post(), b"{\"html\":\"Public text\"}")
        .await
        .expect("the first public observation is preserved");
    let original = captured_fact(&database, capture_id).await;
    resolution
        .record(
            capture_id,
            &post_with_text("Changed public text"),
            b"{\"html\":\"Changed public text\"}",
        )
        .await
        .expect("the changed public observation is preserved");
    let store = KnowledgeCompletionStore::new(&database.database);
    let older_exact = SocialSourceAnalysisCompleted {
        owner: original.source.owner,
        social_source_id: original.source.social_source_id,
        content_digest: original.source.content_digest.clone(),
        completed_at: WireTimestamp::now(),
        extensions: Extensions::default(),
    };
    let foreign_owner = SocialSourceAnalysisCompleted {
        owner: TenantRef::parse(&format!("user:{}", Uuid::now_v7())).expect("fixture owner parses"),
        ..older_exact.clone()
    };
    let wrong_source = SocialSourceAnalysisCompleted {
        social_source_id: SocialSourceId::parse(&Uuid::now_v7().to_string())
            .expect("fixture source id parses"),
        ..older_exact.clone()
    };
    let exact = store
        .record(Uuid::now_v7(), &older_exact)
        .await
        .expect("an older exact completion is accepted");
    let foreign = store
        .record(Uuid::now_v7(), &foreign_owner)
        .await
        .expect("a foreign completion is recorded as rejected");
    let stale = store
        .record(Uuid::now_v7(), &wrong_source)
        .await
        .expect("a stale completion is recorded as rejected");
    let links: i64 =
        sqlx::query_scalar("select count(*) from threads_archive.social_analysis_links")
            .fetch_one(database.database.pool())
            .await
            .expect("the linkage query answers");

    database.cleanup().await.expect("cleanup succeeds");
    tokio::fs::remove_dir_all(raw_root)
        .await
        .expect("raw fixture directory cleans up");

    assert_eq!(exact, CompletionLinkOutcome::Linked);
    assert_eq!(foreign, CompletionLinkOutcome::Rejected);
    assert_eq!(stale, CompletionLinkOutcome::Rejected);
    assert_eq!(links, 1, "only the exact historical revision links");
}

#[tokio::test]
async fn late_knowledge_completion_cannot_resurrect_a_locally_removed_source() {
    let database = TestDatabase::create()
        .await
        .expect("a disposable database is available");
    let owner = Uuid::now_v7();
    let post_id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.posts \
         (post_id, provider_post_id, permalink, post_kind, acquisition_method, saved_authority, upstream_status) \
         values ($1, 'late-completion-post', 'https://www.threads.net/@safe/post/late', 'post', \
          'public_resolution', 'explicit_user_capture', 'active')",
    )
    .bind(post_id)
    .execute(database.database.pool())
    .await
    .expect("post stores");
    let source_id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.social_sources \
         (social_source_id, user_ref, post_id) values ($1, $2, $3)",
    )
    .bind(source_id)
    .bind(owner)
    .bind(post_id)
    .execute(database.database.pool())
    .await
    .expect("source stores");
    let digest = "a".repeat(64);
    sqlx::query(
        "insert into threads_archive.social_source_revisions \
         (source_revision_id, social_source_id, content_digest, snapshot, observed_at) \
         values ($1, $2, $3, '{}', now())",
    )
    .bind(Uuid::now_v7())
    .bind(source_id)
    .bind(&digest)
    .execute(database.database.pool())
    .await
    .expect("source revision stores");
    let operation_id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.deletion_operations \
         (operation_id, user_ref, target_kind, target_id, reason, state, requested_at, finished_at) \
         values ($1, $2, 'capture', $3, 'user_requested', 'complete', now(), now())",
    )
    .bind(operation_id)
    .bind(owner)
    .bind(Uuid::now_v7())
    .execute(database.database.pool())
    .await
    .expect("deletion audit stores");
    sqlx::query(
        "insert into threads_archive.local_source_removals \
         (user_ref, social_source_id, post_id, operation_id, reason, removed_at) \
         values ($1, $2, $3, $4, 'user_requested', now())",
    )
    .bind(owner)
    .bind(source_id)
    .bind(post_id)
    .bind(operation_id)
    .execute(database.database.pool())
    .await
    .expect("local removal guard stores");
    let completion = SocialSourceAnalysisCompleted {
        owner: TenantRef::parse(&format!("user:{owner}")).expect("fixture owner parses"),
        social_source_id: SocialSourceId::parse(&source_id.to_string())
            .expect("fixture source id parses"),
        content_digest: ratatoskr_identifiers::ContentDigest {
            algorithm: ratatoskr_identifiers::DigestAlgorithm::Sha256,
            hex: ratatoskr_identifiers::DigestHex::parse(&digest).expect("fixture digest parses"),
        },
        completed_at: WireTimestamp::now(),
        extensions: Extensions::default(),
    };
    let event_id = Uuid::now_v7();

    let first = KnowledgeCompletionStore::new(&database.database)
        .record(event_id, &completion)
        .await
        .expect("late completion is safely consumed");
    let replay = KnowledgeCompletionStore::new(&database.database)
        .record(event_id, &completion)
        .await
        .expect("late completion replay is safe");
    let (links, outcome): (i64, String) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.social_analysis_links), \
           (select handler_outcome from threads_archive.inbox_events \
            where consumer_name = 'threads-social-source-knowledge' and event_id = $1)",
    )
    .bind(event_id)
    .fetch_one(database.database.pool())
    .await
    .expect("late completion outcome reads");

    assert_eq!(first, CompletionLinkOutcome::LocallyRemoved);
    assert_eq!(replay, CompletionLinkOutcome::Duplicate);
    assert_eq!((links, outcome), (0, "locally_removed".to_owned()));

    database.cleanup().await.expect("cleanup succeeds");
}

#[tokio::test]
async fn unavailable_only_capture_does_not_append_social_source_fact() {
    let database = TestDatabase::create()
        .await
        .expect("a disposable database is available");
    let capture_id = capture(&database).await;
    CaptureStore::new(&database.database)
        .record_observation(
            capture_id,
            &UnavailabilityObservation::deleted("provider-gone".to_owned())
                .expect("fixture deletion evidence is valid"),
        )
        .await
        .expect("the unavailable fallback is preserved");
    let facts: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.outbox_events \
         where aggregate_type = 'capture' and aggregate_id = $1",
    )
    .bind(capture_id)
    .fetch_one(database.database.pool())
    .await
    .expect("the outbox query answers");
    database.cleanup().await.expect("cleanup succeeds");

    assert_eq!(
        facts, 0,
        "unavailable-only captures have no normalized source fact"
    );
}

async fn captured_fact(database: &TestDatabase, capture_id: Uuid) -> SocialSourceCaptured {
    let body: serde_json::Value = sqlx::query_scalar(
        "select payload from threads_archive.outbox_events \
         where aggregate_type = 'capture' and aggregate_id = $1",
    )
    .bind(capture_id)
    .fetch_one(database.database.pool())
    .await
    .expect("the captured outbox fact is present");
    let envelope = EventEnvelope::from_json(
        serde_json::to_vec(&body)
            .expect("stored envelope re-serializes")
            .as_slice(),
    )
    .expect("stored event is a valid envelope");
    envelope
        .payload_as::<SocialSourceCaptured>()
        .expect("stored event has a typed captured payload")
}

async fn capture(database: &TestDatabase) -> Uuid {
    let request = CaptureRequest::try_new(
        Uuid::now_v7(),
        "social-publishing".to_owned(),
        "https://www.threads.net/@example/post/Dz9qL",
        None,
        CaptureMethod::ShareExtension,
        ClientSource::IosShareExtension,
    )
    .expect("fixture capture is valid");
    let outcome = CaptureStore::new(&database.database)
        .submit(&request)
        .await
        .expect("fixture capture inserts");
    let SubmitOutcome::Created(capture) = outcome else {
        panic!("fixture capture must be new");
    };
    capture.capture_id
}

fn post() -> PublicPost {
    post_with_text("Public text")
}

fn post_with_text(embed_html: &str) -> PublicPost {
    let permalink = CanonicalizedUrl::try_from("https://www.threads.net/@example/post/Dz9qL")
        .expect("fixture permalink normalizes");
    PublicPost {
        provider_post_id: "Dz9qL".to_owned(),
        permalink: permalink.permalink().clone(),
        embed_html: embed_html.to_owned(),
        parser_version: PARSER_VERSION,
        relations: Vec::new(),
    }
}

fn raw_root() -> PathBuf {
    std::env::temp_dir().join(format!("ratatoskr-threads-social-{}", Uuid::now_v7()))
}
