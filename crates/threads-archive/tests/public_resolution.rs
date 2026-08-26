//! Public-resolution behavior tests use synthetic, redacted provider fixtures.

use ratatoskr_threads_archive::capture::{
    CaptureMethod, CaptureRequest, CaptureStore, ClientSource, SubmitOutcome,
};
use ratatoskr_threads_archive::permalink::CanonicalizedUrl;
use ratatoskr_threads_archive::public_resolution::{
    ApprovedOembedClient, PARSER_VERSION, PublicPost, PublicResolutionError, PublicResolutionStore,
    RawObjectStore, parse_observation,
};
use ratatoskr_threads_archive::public_resolution::{
    GraphTarget, RelationGraphError, RelationInput, normalize_relations,
};
use ratatoskr_threads_archive::test_support::TestDatabase;
use std::collections::BTreeSet;
use std::path::PathBuf;
use uuid::Uuid;

const FIXTURE: &str = r#"{
  "provider_name": "Threads",
  "provider_url": "https://www.threads.net/",
  "url": "https://www.threads.net/@example/post/Dz9qL",
  "html": "<blockquote id=\"ig-tp-Dz9qL\">Public text</blockquote>",
  "type": "rich",
  "version": "1.0"
}"#;

#[test]
fn parses_supported_public_fixture_deterministically() {
    let requested = CanonicalizedUrl::try_from("https://threads.net/@Example/post/Dz9qL")
        .expect("fixture URL is a supported permalink");

    let post = parse_observation(requested.permalink(), FIXTURE)
        .expect("the approved public fixture must normalize");

    assert_eq!(post.provider_post_id, "Dz9qL");
    assert_eq!(
        post.permalink.as_str(),
        "https://www.threads.net/@example/post/Dz9qL"
    );
    assert_eq!(post.parser_version, PARSER_VERSION);
    assert_eq!(
        post.embed_html,
        "<blockquote id=\"ig-tp-Dz9qL\">Public text</blockquote>"
    );
}

#[test]
fn accepts_only_approved_threads_oembed_https_surfaces() {
    ApprovedOembedClient::new("https://graph.threads.com/oembed")
        .expect("official Threads oEmbed surface is accepted");
    for endpoint in [
        "http://graph.threads.com/oembed",
        "https://www.threads.net/@example/post/Dz9qL",
        "https://example.test/oembed",
        "https://graph.threads.com/oembed?access_token=forbidden",
    ] {
        assert!(
            ApprovedOembedClient::new(endpoint).is_err(),
            "only an approved public HTTPS oEmbed endpoint is usable"
        );
    }
}

#[tokio::test]
async fn re_resolution_appends_immutable_parser_versioned_revisions() {
    let database = TestDatabase::create().await.expect("a disposable database");
    let request = CaptureRequest::try_new(
        Uuid::now_v7(),
        "public-resolution".to_owned(),
        "https://www.threads.net/@example/post/Dz9qL",
        None,
        CaptureMethod::ShareExtension,
        ClientSource::IosShareExtension,
    )
    .expect("fixture capture is valid");
    let submitted = CaptureStore::new(&database.database)
        .submit(&request)
        .await
        .expect("capture inserts");
    assert!(
        matches!(submitted, SubmitOutcome::Created(_)),
        "first submission must create"
    );
    let SubmitOutcome::Created(capture) = submitted else {
        return;
    };
    let post = parse_observation(request.canonical_url(), FIXTURE).expect("fixture normalizes");
    let (raw_objects, raw_root) = raw_objects();
    let store = PublicResolutionStore::new(&database.database, raw_objects);

    let first = store
        .record(capture.capture_id, &post, FIXTURE.as_bytes())
        .await
        .expect("first resolution stores immutable evidence");
    let second = store
        .record(capture.capture_id, &post, b"{\"second\":true}")
        .await
        .expect("re-resolution appends immutable evidence");

    assert_ne!(
        first.raw_object_id, second.raw_object_id,
        "each observation has distinct raw evidence"
    );
    assert_ne!(
        first.revision_id, second.revision_id,
        "each observation appends a revision"
    );
    let (blob_ref, media_type): (String, String) = sqlx::query_as(
        "select blob_ref, media_type from threads_archive.raw_objects where raw_object_id = $1",
    )
    .bind(first.raw_object_id)
    .fetch_one(database.database.pool())
    .await
    .expect("first raw object has a BlobRef");
    assert_eq!(media_type, "application/json");
    let digest = blob_ref.rsplit('/').next().expect("BlobRef has a digest");
    assert_eq!(
        tokio::fs::read(raw_root.join("sha256").join(digest))
            .await
            .expect("raw bytes are stored before the revision is accepted"),
        FIXTURE.as_bytes()
    );
    database.cleanup().await.expect("cleanup succeeds");
    tokio::fs::remove_dir_all(raw_root)
        .await
        .expect("raw fixture directory cleans up");
}

#[tokio::test]
async fn persists_first_class_relations_and_rejects_cycles_atomically() {
    let database = TestDatabase::create().await.expect("a disposable database");
    let (raw_objects, raw_root) = raw_objects();
    let store = PublicResolutionStore::new(&database.database, raw_objects);
    let parent_capture = capture(&database, "parent").await;
    let child_capture = capture(&database, "child").await;

    store
        .record(parent_capture, &post("parent", Vec::new()), b"parent")
        .await
        .expect("parent stores");
    store
        .record(
            child_capture,
            &post(
                "child",
                vec![
                    edge("child", "reply", "parent"),
                    RelationInput {
                        target_permalink: Some(
                            "https://www.threads.net/@example/post/missing".to_owned(),
                        ),
                        ..edge("child", "quote", "missing")
                    },
                ],
            ),
            b"child",
        )
        .await
        .expect("child and its graph store");

    let rows: Vec<(String, Option<String>, String)> = sqlx::query_as(
        "select relation_kind, target_post_id::text, target_provider_post_id from threads_archive.post_relations order by relation_kind, target_provider_post_id",
    )
    .fetch_all(database.database.pool())
    .await
    .expect("relation graph reads");
    assert_eq!(
        rows,
        vec![
            ("quote".to_owned(), None, "missing".to_owned()),
            (
                "reply".to_owned(),
                Some(rows[1].1.clone().expect("parent resolves")),
                "parent".to_owned()
            ),
        ],
        "quote and reply are rows; an orphan stays explicit"
    );

    let error = store
        .record(
            parent_capture,
            &post("parent", vec![edge("parent", "reply", "child")]),
            b"cycle",
        )
        .await
        .expect_err("a cycle must roll back the relation observation");
    assert!(matches!(
        error,
        PublicResolutionError::RelationGraph(RelationGraphError::ReplyCycle)
    ));
    let revision_count: i64 =
        sqlx::query_scalar("select count(*) from threads_archive.post_revisions")
            .fetch_one(database.database.pool())
            .await
            .expect("revision count reads");
    assert_eq!(revision_count, 2, "cycle appends no revision");
    database.cleanup().await.expect("cleanup succeeds");
    tokio::fs::remove_dir_all(raw_root)
        .await
        .expect("raw fixture directory cleans up");
}

fn raw_objects() -> (RawObjectStore, PathBuf) {
    let root = std::env::temp_dir().join(format!("ratatoskr-threads-raw-{}", Uuid::now_v7()));
    (RawObjectStore::new(root.clone()), root)
}

#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "synthetic fixture construction must fail loudly when its fixed inputs change"
)]
async fn capture(database: &TestDatabase, post_id: &str) -> Uuid {
    let permalink = format!("https://www.threads.net/@example/post/{post_id}");
    let request = CaptureRequest::try_new(
        Uuid::now_v7(),
        format!("capture-{post_id}"),
        &permalink,
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
        panic!("fixture capture is new");
    };
    capture.capture_id
}

#[expect(
    clippy::expect_used,
    reason = "synthetic fixture construction must fail loudly when its fixed inputs change"
)]
fn post(provider_post_id: &str, relations: Vec<RelationInput>) -> PublicPost {
    let raw_permalink = format!("https://www.threads.net/@example/post/{provider_post_id}");
    let permalink =
        CanonicalizedUrl::try_from(raw_permalink.as_str()).expect("fixture permalink normalizes");
    PublicPost {
        provider_post_id: provider_post_id.to_owned(),
        permalink: permalink.permalink().clone(),
        embed_html: format!("<blockquote>{provider_post_id}</blockquote>"),
        parser_version: PARSER_VERSION,
        relations,
    }
}

fn ids(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn edge(source: &str, kind: &str, target: &str) -> RelationInput {
    RelationInput {
        referencing_provider_post_id: source.to_owned(),
        relation_kind: kind.to_owned(),
        target_provider_post_id: target.to_owned(),
        target_permalink: None,
    }
}

#[test]
fn stores_directed_reply_and_quote_edges() {
    let graph = normalize_relations(
        &ids(&["child", "parent", "quoted"]),
        vec![
            edge("child", "quote", "quoted"),
            edge("child", "reply", "parent"),
        ],
    )
    .expect("fixture thread normalizes");
    assert_eq!(graph.len(), 2, "reply and quote must be first-class edges");
    assert_eq!(
        graph[0].relation_kind.as_str(),
        "quote",
        "stable order includes quote"
    );
    assert_eq!(
        graph[1].relation_kind.as_str(),
        "reply",
        "stable order includes reply"
    );
}

#[test]
fn keeps_orphan_relation_explicit() {
    let graph = normalize_relations(
        &ids(&["child"]),
        vec![edge("child", "reply", "missing-parent")],
    )
    .expect("orphan must not invalidate child");
    assert!(
        matches!(graph[0].target, GraphTarget::Unresolved { .. }),
        "missing parent remains explicit"
    );
}

#[test]
fn rejects_reply_cycles_atomically() {
    let error = normalize_relations(
        &ids(&["a", "b"]),
        vec![edge("a", "reply", "b"), edge("b", "reply", "a")],
    )
    .expect_err("a directed reply cycle must be refused");
    assert!(
        matches!(error, RelationGraphError::ReplyCycle),
        "cycle has its own refusal"
    );
}

#[test]
fn normalizes_permuted_relations_deterministically() {
    let known = ids(&["child", "parent", "quoted"]);
    let forward = normalize_relations(
        &known,
        vec![
            edge("child", "reply", "parent"),
            edge("child", "quote", "quoted"),
        ],
    )
    .expect("fixture normalizes");
    let reverse = normalize_relations(
        &known,
        vec![
            edge("child", "quote", "quoted"),
            edge("child", "reply", "parent"),
        ],
    )
    .expect("fixture normalizes");
    assert_eq!(
        forward, reverse,
        "input array order cannot change graph representation"
    );
}
