//! Explicit-capture intake contract for change `add-explicit-capture`
//! (tasks 2.1/2.2 and 3.1/3.2): lane/client pairing is enforced by a named
//! rule before anything is stored, provenance is pinned to
//! `explicit_user_capture` regardless of lane, hostile input is bounded by
//! named rules, replay converges deterministically on one row per
//! `(user_ref, idempotency_key)`, and unavailable fallbacks record exactly
//! the evidence observed — never fabricated deletion state.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use ratatoskr_threads_archive::capability::SavedAuthority;
use ratatoskr_threads_archive::capture::{
    CaptureError, CaptureMethod, CaptureRecord, CaptureRequest, CaptureStatus, CaptureStore,
    ClientSource, SubmitOutcome, UnavailabilityObservation,
};
use ratatoskr_threads_archive::permalink::PermalinkError;
use ratatoskr_threads_archive::test_support::TestDatabase;

/// A canonical permalink every accepted sample in this file normalizes to.
const CANONICAL: &str = "https://www.threads.net/@user.name/post/Dz9qL";

/// The raw share-sheet text that canonicalizes to [`CANONICAL`].
const RAW: &str = "https://www.threads.net/@User.Name/post/Dz9qL?igsh=x#f";

/// Builds one validated request through the public constructor.
#[expect(
    clippy::expect_used,
    reason = "helper outside any single test fn: an unexpected intake refusal is the failure"
)]
fn request_for(
    method: CaptureMethod,
    client: ClientSource,
    key: &str,
    raw_url: &str,
    note: Option<&str>,
) -> CaptureRequest {
    CaptureRequest::try_new(
        Uuid::now_v7(),
        key.to_owned(),
        raw_url,
        note.map(str::to_owned),
        method,
        client,
    )
    .expect("this input satisfies every documented intake rule")
}

#[test]
fn every_documented_lane_pair_builds_a_validated_request() {
    let lanes = [
        (
            CaptureMethod::ShareExtension,
            ClientSource::IosShareExtension,
            RAW,
            CANONICAL,
        ),
        (
            CaptureMethod::ShareExtension,
            ClientSource::AndroidShareTarget,
            "http://threads.com/@USER/post/AbC_1/",
            "https://www.threads.net/@user/post/AbC_1",
        ),
        (
            CaptureMethod::BrowserExtension,
            ClientSource::BrowserExtension,
            "https://www.threads.net/@user/post/XyZw",
            "https://www.threads.net/@user/post/XyZw",
        ),
        (
            CaptureMethod::TelegramCapture,
            ClientSource::Telegram,
            "https://threads.net/@tg.user/post/t9Code",
            "https://www.threads.net/@tg.user/post/t9Code",
        ),
    ];
    for (method, client, raw, expected_canonical) in lanes {
        let request = request_for(method, client, "lane-pair-key", raw, None);
        assert_eq!(request.acquisition_method(), method);
        assert_eq!(request.client_source(), client);
        assert_eq!(
            request.canonical_url().as_str(),
            expected_canonical,
            "the validated request must retain the canonical permalink for {raw}"
        );
        assert_eq!(
            request.raw_url(),
            raw,
            "the validated request must retain the submitted text byte-for-byte"
        );
    }
}

#[test]
fn a_mismatched_pairing_is_refused_naming_the_pairing_rule() {
    let refusals = [
        (
            CaptureMethod::BrowserExtension,
            ClientSource::Telegram,
            "browser_extension must pair with browser_extension",
        ),
        (
            CaptureMethod::ShareExtension,
            ClientSource::Telegram,
            "share_extension must pair with a share-target client",
        ),
        (
            CaptureMethod::TelegramCapture,
            ClientSource::BrowserExtension,
            "telegram_capture must pair with telegram",
        ),
        (
            CaptureMethod::ShareExtension,
            ClientSource::BrowserExtension,
            "share_extension must pair with a share-target client",
        ),
    ];
    for (method, client, because) in refusals {
        let error = CaptureRequest::try_new(
            Uuid::now_v7(),
            "mismatched-lane".to_owned(),
            RAW,
            None,
            method,
            client,
        )
        .expect_err("a method/client combination outside the documented mapping must be refused");
        assert!(
            matches!(
                error,
                CaptureError::PairingMismatch {
                    acquisition_method,
                    client_source
                } if acquisition_method == method && client_source == client
            ),
            "the pairing refusal must name both offending values for {because}: {error:?}"
        );
        assert!(
            error.to_string().contains("pairing"),
            "the refusal message must name the pairing rule ({because}): {error}"
        );
    }
}

#[test]
fn the_stored_record_pins_explicit_user_capture_regardless_of_lane() {
    let lanes = [
        (
            CaptureMethod::ShareExtension,
            ClientSource::IosShareExtension,
        ),
        (
            CaptureMethod::ShareExtension,
            ClientSource::AndroidShareTarget,
        ),
        (
            CaptureMethod::BrowserExtension,
            ClientSource::BrowserExtension,
        ),
        (CaptureMethod::TelegramCapture, ClientSource::Telegram),
    ];
    for (method, client) in lanes {
        let request = request_for(method, client, "authority-key", RAW, Some("keep"));
        let record = CaptureRecord::accepted(&request, Uuid::now_v7(), Utc::now());
        assert_eq!(
            record.saved_authority,
            SavedAuthority::ExplicitUserCapture,
            "no lane may widen the authority an explicit capture proves"
        );
        assert_eq!(record.acquisition_method, method);
        assert_eq!(record.client_source, client);
        assert_eq!(record.status, CaptureStatus::Accepted);
        assert_eq!(record.canonical_url.as_str(), CANONICAL);
        assert_eq!(record.original_url, RAW);
        assert_eq!(record.note.as_deref(), Some("keep"));
        assert_eq!(record.post_id, None);
        assert_eq!(record.user_ref, request.user_ref());
    }
}

#[test]
fn a_request_whose_url_cannot_canonicalize_is_refused_before_any_storage() {
    let error = CaptureRequest::try_new(
        Uuid::now_v7(),
        "bad-url-key".to_owned(),
        "https://example.com/@user/post/abc",
        None,
        CaptureMethod::ShareExtension,
        ClientSource::IosShareExtension,
    )
    .expect_err("a foreign-host URL must be refused at construction");
    assert!(
        matches!(error, CaptureError::InvalidUrl(PermalinkError::Host)),
        "the intake refusal must wrap the exact permalink rule that fired: {error:?}"
    );
}

#[test]
fn an_empty_idempotency_key_is_refused_naming_the_rule() {
    let error = CaptureRequest::try_new(
        Uuid::now_v7(),
        String::new(),
        RAW,
        None,
        CaptureMethod::BrowserExtension,
        ClientSource::BrowserExtension,
    )
    .expect_err("an empty idempotency key must be refused");
    assert!(
        matches!(error, CaptureError::EmptyIdempotencyKey),
        "the refusal must name the idempotency-key rule: {error:?}"
    );
    assert!(
        error.to_string().contains("1..=256 bytes"),
        "the refusal message must state the key bounds: {error}"
    );
}

#[test]
fn an_idempotency_key_over_256_bytes_is_refused_and_the_boundary_is_accepted() {
    let over = "k".repeat(257);
    let error = CaptureRequest::try_new(
        Uuid::now_v7(),
        over,
        RAW,
        None,
        CaptureMethod::BrowserExtension,
        ClientSource::BrowserExtension,
    )
    .expect_err("a 257-byte idempotency key must be refused");
    assert!(
        matches!(error, CaptureError::IdempotencyKeyTooLong { len: 257 }),
        "the refusal must carry the offending length: {error:?}"
    );

    let at_limit = "k".repeat(256);
    request_for(
        CaptureMethod::BrowserExtension,
        ClientSource::BrowserExtension,
        &at_limit,
        RAW,
        None,
    );
}

#[test]
fn an_empty_observation_field_is_refused_naming_the_rule() {
    for (observation, field) in [
        (
            UnavailabilityObservation::deleted(String::new()),
            "reason_code",
        ),
        (
            UnavailabilityObservation::private_or_inaccessible(String::new()),
            "reason_code",
        ),
        (
            UnavailabilityObservation::resolver_failed(String::new()),
            "resolver_version",
        ),
    ] {
        let error = observation.expect_err("an empty observation field must be refused");
        assert!(
            matches!(
                error,
                CaptureError::InvalidObservationField { field: named, len: 0 } if named == field
            ),
            "the refusal must name {field}: {error:?}"
        );
    }
}

#[test]
fn an_observation_field_over_128_bytes_is_refused_and_the_boundary_is_accepted() {
    let long = "r".repeat(129);
    let error = UnavailabilityObservation::deleted(long.clone())
        .expect_err("a 129-byte reason code must be refused");
    assert!(
        matches!(
            error,
            CaptureError::InvalidObservationField {
                field: "reason_code",
                len: 129
            }
        ),
        "the refusal must name the field and its length: {error:?}"
    );
    let error = UnavailabilityObservation::resolver_failed(long)
        .expect_err("a 129-byte resolver version must be refused");
    assert!(
        matches!(
            error,
            CaptureError::InvalidObservationField {
                field: "resolver_version",
                len: 129
            }
        ),
        "the refusal must name the field and its length: {error:?}"
    );

    UnavailabilityObservation::private_or_inaccessible("l".repeat(128))
        .expect("a 128-byte reason code sits at the documented limit");
    UnavailabilityObservation::resolver_failed("public-oembed-v0".to_owned())
        .expect("a normal resolver version is valid");
}

/// One stored capture row read back from the database, in schema column
/// order: capture id, open post reference, canonical URL, original URL,
/// acquisition method, saved authority, client source, status, note, and
/// captured time.
type StoredRow = (
    Uuid,
    Option<Uuid>,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    DateTime<Utc>,
);

#[expect(
    clippy::expect_used,
    reason = "database-test helper outside any single test fn: a missing stored row is the failure"
)]
async fn stored_row(pool: &sqlx::PgPool, user_ref: Uuid, key: &str) -> StoredRow {
    sqlx::query_as(
        "select capture_id, post_id, canonical_url, original_url, acquisition_method, \
         saved_authority, client_source, status, note, captured_at \
         from threads_archive.captures where user_ref = $1 and idempotency_key = $2",
    )
    .bind(user_ref)
    .bind(key)
    .fetch_one(pool)
    .await
    .expect("the stored capture row must exist")
}

#[expect(
    clippy::expect_used,
    reason = "database-test helper outside any single test fn: an unanswered count is the failure"
)]
async fn capture_count(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("select count(*) from threads_archive.captures")
        .fetch_one(pool)
        .await
        .expect("the capture count query must answer")
}

#[expect(
    clippy::expect_used,
    reason = "database-test helper outside any single test fn: an unanswered query is the failure"
)]
async fn tombstones_for(pool: &sqlx::PgPool, capture_id: Uuid) -> Vec<(String, Option<String>)> {
    sqlx::query_as(
        "select availability, reason_code from threads_archive.tombstones \
         where capture_id = $1 order by observed_at",
    )
    .bind(capture_id)
    .fetch_all(pool)
    .await
    .expect("the tombstone query must answer")
}

#[expect(
    clippy::expect_used,
    reason = "database-test helper outside any single test fn: an unanswered query is the failure"
)]
async fn resolutions_for(pool: &sqlx::PgPool, capture_id: Uuid) -> Vec<(String, Option<String>)> {
    sqlx::query_as(
        "select outcome, resolver_version from threads_archive.capture_resolutions \
         where capture_id = $1 order by observed_at",
    )
    .bind(capture_id)
    .fetch_all(pool)
    .await
    .expect("the resolution query must answer")
}

#[expect(
    clippy::expect_used,
    reason = "database-test helper outside any single test fn: a missing status is the failure"
)]
async fn status_of(pool: &sqlx::PgPool, capture_id: Uuid) -> String {
    sqlx::query_scalar("select status from threads_archive.captures where capture_id = $1")
        .bind(capture_id)
        .fetch_one(pool)
        .await
        .expect("the status query must answer")
}

#[expect(
    clippy::expect_used,
    reason = "database-test helper outside any single test fn: an unexpected refusal is the failure"
)]
fn request_for_owner(
    user_ref: Uuid,
    key: &str,
    raw_url: &str,
    note: Option<&str>,
) -> CaptureRequest {
    CaptureRequest::try_new(
        user_ref,
        key.to_owned(),
        raw_url,
        note.map(str::to_owned),
        CaptureMethod::ShareExtension,
        ClientSource::IosShareExtension,
    )
    .expect("this input satisfies every documented intake rule")
}

/// The stored record when the outcome reports creation.
fn created_record(outcome: SubmitOutcome) -> Option<CaptureRecord> {
    match outcome {
        SubmitOutcome::Created(record) => Some(record),
        SubmitOutcome::Replayed(_) => None,
    }
}

/// The stored record when the outcome reports a replay.
fn replayed_record(outcome: SubmitOutcome) -> Option<CaptureRecord> {
    match outcome {
        SubmitOutcome::Replayed(record) => Some(record),
        SubmitOutcome::Created(_) => None,
    }
}

#[tokio::test]
async fn submitting_stores_a_row_with_pinned_explicit_provenance() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let store = CaptureStore::new(&test.database);
    let user = Uuid::now_v7();
    let request = request_for_owner(user, "provenance-key", RAW, Some("keep for later"));

    let outcome = store.submit(&request).await.expect("submit must store");
    let record =
        created_record(outcome).expect("a first submission under a fresh key must be Created");

    assert_eq!(
        record.saved_authority,
        SavedAuthority::ExplicitUserCapture,
        "the store pins explicit_user_capture regardless of what was submitted"
    );
    assert_eq!(record.acquisition_method, CaptureMethod::ShareExtension);
    assert_eq!(record.client_source, ClientSource::IosShareExtension);
    assert_eq!(record.status, CaptureStatus::Accepted);
    assert_eq!(record.canonical_url.as_str(), CANONICAL);
    assert_eq!(record.original_url, RAW);
    assert_eq!(record.note.as_deref(), Some("keep for later"));

    let pool = test.database.pool();
    let (
        capture_id,
        post_id,
        canonical_url,
        original_url,
        acquisition_method,
        saved_authority,
        client_source,
        status,
        note,
        captured_at,
    ) = stored_row(pool, user, "provenance-key").await;

    assert_eq!(
        capture_id, record.capture_id,
        "the row carries the minted id"
    );
    assert_eq!(post_id, None, "an unresolved capture keeps post_id open");
    assert_eq!(saved_authority, "explicit_user_capture");
    assert_eq!(acquisition_method, "share_extension");
    assert_eq!(client_source, "ios_share_extension");
    assert_eq!(canonical_url, CANONICAL);
    assert_eq!(
        original_url, RAW,
        "the original submitted text must be stored byte-for-byte"
    );
    assert_eq!(note.as_deref(), Some("keep for later"));
    assert_eq!(status, "accepted");
    assert_eq!(
        captured_at.date_naive(),
        Utc::now().date_naive(),
        "captured_at must be stamped by the acceptance clock"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn an_identical_replay_returns_the_stored_record_and_creates_no_second_row() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let store = CaptureStore::new(&test.database);
    let user = Uuid::now_v7();
    let request = request_for_owner(user, "replay-key", RAW, Some("note"));

    let first = store.submit(&request).await.expect("first submit stores");
    let second = store.submit(&request).await.expect("replay answers");

    let first_record =
        created_record(first).expect("the first submission under a fresh key must be Created");
    let second_record = replayed_record(second).expect("the identical replay must be Replayed");

    assert_eq!(
        first_record.capture_id, second_record.capture_id,
        "a replay converges on the same capture id"
    );
    assert_eq!(
        first_record.captured_at, second_record.captured_at,
        "a replay keeps the first acceptance stamp"
    );
    assert_eq!(second_record.status, first_record.status);
    assert_eq!(second_record.canonical_url, first_record.canonical_url);
    assert_eq!(second_record.original_url, first_record.original_url);
    assert_eq!(
        capture_count(test.database.pool()).await,
        1,
        "exactly one capture row may exist per (user_ref, idempotency_key)"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn a_replay_through_different_raw_text_converges_on_the_stored_record() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let store = CaptureStore::new(&test.database);
    let user = Uuid::now_v7();
    let first_raw = "https://www.threads.net/@user/post/CodE1";
    let replayed_raw = "https://threads.com/@USER/post/CodE1?igsh=x#f";
    let first = request_for_owner(user, "converge-key", first_raw, None);
    let replay = request_for_owner(user, "converge-key", replayed_raw, None);

    store.submit(&first).await.expect("first submit stores");
    let second = store.submit(&replay).await.expect("replay answers");

    let record =
        replayed_record(second).expect("a replay that canonicalizes equal must be Replayed");

    let (_, _, canonical_url, original_url, _, _, _, _, _, _) =
        stored_row(test.database.pool(), user, "converge-key").await;
    assert_eq!(
        record.canonical_url.as_str(),
        canonical_url,
        "both spellings describe one permalink"
    );
    assert_eq!(
        original_url, first_raw,
        "the stored original text must stay the FIRST submitted text"
    );
    assert_eq!(canonical_url, "https://www.threads.net/@user/post/CodE1");
    assert_eq!(
        capture_count(test.database.pool()).await,
        1,
        "no second row may appear for the same key"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn distinct_keys_over_one_permalink_create_independent_captures() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let store = CaptureStore::new(&test.database);
    let user = Uuid::now_v7();

    let first = store
        .submit(&request_for_owner(user, "key-one", RAW, None))
        .await
        .expect("first key stores");
    let second = store
        .submit(&request_for_owner(user, "key-two", RAW, None))
        .await
        .expect("second key stores");

    for (outcome, name) in [(&first, "first"), (&second, "second")] {
        assert!(
            matches!(outcome, SubmitOutcome::Created(_)),
            "{name} submission under its own key must be Created: {outcome:?}"
        );
    }
    let first_id = created_record(first)
        .expect("asserted Created above")
        .capture_id;
    let second_id = created_record(second)
        .expect("asserted Created above")
        .capture_id;
    assert_ne!(
        first_id, second_id,
        "distinct keys are distinct intent and get distinct captures"
    );
    assert_eq!(
        capture_count(test.database.pool()).await,
        2,
        "one permalink under two keys creates two rows"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn an_observed_deletion_writes_tombstone_backed_unavailable_state() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let store = CaptureStore::new(&test.database);
    let user = Uuid::now_v7();

    let outcome = store
        .submit(&request_for_owner(user, "deletion-key", RAW, Some("note")))
        .await
        .expect("submit must store");
    let record = created_record(outcome).expect("a first submission must be Created");
    let observation =
        UnavailabilityObservation::deleted("gone".to_owned()).expect("a valid reason code");

    store
        .record_observation(record.capture_id, &observation)
        .await
        .expect("the deletion observation must record");

    let pool = test.database.pool();
    assert_eq!(
        tombstones_for(pool, record.capture_id).await,
        vec![("deleted".to_owned(), Some("gone".to_owned()))],
        "the tombstone names availability deleted with the observed reason"
    );
    assert_eq!(
        resolutions_for(pool, record.capture_id).await,
        vec![("unavailable".to_owned(), None)],
        "the resolution records the unavailable outcome"
    );
    assert_eq!(status_of(pool, record.capture_id).await, "unavailable");

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn an_observed_privacy_writes_the_same_truthful_fallback_shape() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let store = CaptureStore::new(&test.database);
    let user = Uuid::now_v7();

    let outcome = store
        .submit(&request_for_owner(user, "privacy-key", RAW, Some("note")))
        .await
        .expect("submit must store");
    let record = created_record(outcome).expect("a first submission must be Created");
    let observation = UnavailabilityObservation::private_or_inaccessible("locked".to_owned())
        .expect("a valid reason code");

    store
        .record_observation(record.capture_id, &observation)
        .await
        .expect("the privacy observation must record");

    let pool = test.database.pool();
    assert_eq!(
        tombstones_for(pool, record.capture_id).await,
        vec![(
            "private_or_inaccessible".to_owned(),
            Some("locked".to_owned())
        )],
        "the fallback shape equals the deletion case except the availability"
    );
    assert_eq!(
        resolutions_for(pool, record.capture_id).await,
        vec![("unavailable".to_owned(), None)]
    );
    assert_eq!(status_of(pool, record.capture_id).await, "unavailable");

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn a_resolver_failure_never_fabricates_deletion_evidence() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let store = CaptureStore::new(&test.database);
    let user = Uuid::now_v7();

    let outcome = store
        .submit(&request_for_owner(user, "resolver-key", RAW, None))
        .await
        .expect("submit must store");
    let record = created_record(outcome).expect("a first submission must be Created");
    let observation = UnavailabilityObservation::resolver_failed("public-oembed-v0".to_owned())
        .expect("a valid resolver version");

    store
        .record_observation(record.capture_id, &observation)
        .await
        .expect("the resolver-failure observation must record");

    let pool = test.database.pool();
    assert_eq!(
        resolutions_for(pool, record.capture_id).await,
        vec![(
            "resolver_failed".to_owned(),
            Some("public-oembed-v0".to_owned())
        )],
        "the resolution names the failed resolver"
    );
    assert!(
        tombstones_for(pool, record.capture_id).await.is_empty(),
        "missing resolver output is never deletion evidence"
    );
    assert_eq!(
        status_of(pool, record.capture_id).await,
        "accepted",
        "a resolver failure leaves the capture accepted"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn the_users_context_survives_every_fallback() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let store = CaptureStore::new(&test.database);
    let user = Uuid::now_v7();

    let outcome = store
        .submit(&request_for_owner(
            user,
            "context-key",
            RAW,
            Some("my note"),
        ))
        .await
        .expect("submit must store");
    let record = created_record(outcome).expect("a first submission must be Created");

    let (_, _, before_canonical, before_original, _, _, _, _, before_note, before_at) =
        stored_row(test.database.pool(), user, "context-key").await;
    assert_eq!(before_note.as_deref(), Some("my note"));

    let observation =
        UnavailabilityObservation::deleted("gone".to_owned()).expect("a valid reason code");
    store
        .record_observation(record.capture_id, &observation)
        .await
        .expect("the deletion observation must record");

    let after = stored_row(test.database.pool(), user, "context-key").await;
    assert_eq!(
        status_of(test.database.pool(), record.capture_id).await,
        "unavailable",
        "the fallback must actually have run"
    );

    let (_, _, after_canonical, after_original, _, _, _, _, after_note, after_at) = after;
    assert_eq!(before_canonical, after_canonical, "the permalink survives");
    assert_eq!(before_original, after_original, "the raw text survives");
    assert_eq!(before_note, after_note, "the note survives");
    assert_eq!(
        before_at, after_at,
        "the acceptance stamp is never rewritten"
    );

    test.cleanup().await.expect("cleanup must drop");
}

#[tokio::test]
async fn an_observation_against_an_unknown_capture_is_refused() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let store = CaptureStore::new(&test.database);
    let unknown = Uuid::now_v7();
    let observation =
        UnavailabilityObservation::deleted("gone".to_owned()).expect("a valid reason code");

    let error = store
        .record_observation(unknown, &observation)
        .await
        .expect_err("an observation against an unknown capture must be refused");
    assert!(
        matches!(error, CaptureError::UnknownCapture(id) if id == unknown),
        "the refusal must name the unknown capture id: {error:?}"
    );

    test.cleanup().await.expect("cleanup must drop");
}
