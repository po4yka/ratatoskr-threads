//! Budgeted and privacy-safe public re-resolution job tests.

use chrono::{Duration, Utc};
use ratatoskr_threads_archive::re_resolution::{
    PriorResolutionState, ReResolutionAttemptOutcome, ReResolutionBudget, ReResolutionCandidate,
    ReResolutionSelection, ReResolutionSkipReason, RefreshAccounting, RefreshClassification,
    attempt_with_budget, claim_capture_for_resolution, classify_refresh, select_candidates,
};
use ratatoskr_threads_archive::test_support::TestDatabase;
use uuid::Uuid;

#[test]
fn selection_admits_only_due_live_transient_or_resolved_captures() {
    let now = Utc::now();
    let resolved = Uuid::from_u128(1);
    let transient = Uuid::from_u128(2);
    let failed = Uuid::from_u128(3);
    let private = Uuid::from_u128(4);
    let deleted = Uuid::from_u128(5);
    let unsupported = Uuid::from_u128(6);
    let not_due = Uuid::from_u128(7);
    let removed = Uuid::from_u128(8);
    let candidate = |capture_id, due_offset, prior_state, locally_removed| ReResolutionCandidate {
        capture_id,
        next_resolution_at: now + Duration::seconds(due_offset),
        locally_removed,
        prior_state,
    };
    let selection = select_candidates(
        vec![
            candidate(failed, -10, PriorResolutionState::ResolverFailed, false),
            candidate(
                private,
                -9,
                PriorResolutionState::PrivateOrInaccessible,
                false,
            ),
            candidate(resolved, -30, PriorResolutionState::Resolved, false),
            candidate(deleted, -8, PriorResolutionState::Deleted, false),
            candidate(
                transient,
                -20,
                PriorResolutionState::TemporarilyUnavailable,
                false,
            ),
            candidate(unsupported, -7, PriorResolutionState::Unsupported, false),
            candidate(not_due, 60, PriorResolutionState::Resolved, false),
            candidate(removed, -6, PriorResolutionState::Resolved, true),
        ],
        now,
    );

    assert_eq!(
        selection,
        ReResolutionSelection {
            admitted: vec![resolved, transient, failed],
            skipped: vec![
                (private, ReResolutionSkipReason::PrivacyTerminal),
                (deleted, ReResolutionSkipReason::PrivacyTerminal),
                (unsupported, ReResolutionSkipReason::Unsupported),
                (removed, ReResolutionSkipReason::LocallyRemoved),
                (not_due, ReResolutionSkipReason::NotDue),
            ],
        }
    );
}

#[test]
fn unchanged_refresh_appends_evidence_without_duplicate_update() {
    let digest = [0x44_u8; 32];
    assert_eq!(
        classify_refresh(&digest, &digest),
        RefreshAccounting {
            classification: RefreshClassification::Unchanged,
            evidence_appended: true,
            update_emitted: false,
        }
    );
}

fn available_budget(now: chrono::DateTime<Utc>) -> ReResolutionBudget {
    ReResolutionBudget {
        max_items: 2,
        items_admitted: 0,
        max_requests: 2,
        requests_reserved: 0,
        max_response_bytes: 2048,
        response_bytes: 0,
        max_concurrency: 1,
        in_flight: 0,
        deadline_at: now + Duration::minutes(1),
        endpoint_remaining: Some(2),
    }
}

#[test]
fn request_never_starts_when_any_run_or_provider_budget_guard_is_exhausted() {
    let now = Utc::now();
    let cases = [
        (
            ReResolutionSkipReason::ItemBudget,
            ReResolutionBudget {
                max_items: 0,
                ..available_budget(now)
            },
        ),
        (
            ReResolutionSkipReason::ItemBudget,
            ReResolutionBudget {
                items_admitted: 2,
                ..available_budget(now)
            },
        ),
        (
            ReResolutionSkipReason::RequestBudget,
            ReResolutionBudget {
                requests_reserved: 2,
                ..available_budget(now)
            },
        ),
        (
            ReResolutionSkipReason::ByteBudget,
            ReResolutionBudget {
                response_bytes: 1537,
                ..available_budget(now)
            },
        ),
        (
            ReResolutionSkipReason::Deadline,
            ReResolutionBudget {
                deadline_at: now - Duration::seconds(1),
                ..available_budget(now)
            },
        ),
        (
            ReResolutionSkipReason::Concurrency,
            ReResolutionBudget {
                in_flight: 1,
                ..available_budget(now)
            },
        ),
        (
            ReResolutionSkipReason::ProviderBudget,
            ReResolutionBudget {
                endpoint_remaining: None,
                ..available_budget(now)
            },
        ),
        (
            ReResolutionSkipReason::ProviderBudget,
            ReResolutionBudget {
                endpoint_remaining: Some(0),
                ..available_budget(now)
            },
        ),
    ];

    for (expected, mut budget) in cases {
        let before = budget;
        let mut resolver_calls = 0_u32;
        let outcome = attempt_with_budget(&mut budget, 512, now, || resolver_calls += 1);
        assert_eq!(outcome, ReResolutionAttemptOutcome::Skipped(expected));
        assert_eq!(
            resolver_calls, 0,
            "guard {expected:?} must refuse before I/O"
        );
        assert_eq!(budget, before, "guard {expected:?} must reserve no counter");
    }
}

#[tokio::test]
async fn deletion_between_selection_and_claim_prevents_request_and_resurrection() {
    let test = TestDatabase::create().await.expect("a fresh test database");
    let capture_id = Uuid::now_v7();
    let owner = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.captures \
         (capture_id, user_ref, idempotency_key, canonical_url, original_url, acquisition_method, \
          saved_authority, client_source, status, captured_at, next_resolution_at) \
         values ($1, $2, 'race-candidate', 'https://www.threads.net/@safe/post/race', \
          'https://threads.net/@safe/post/race', 'share_extension', 'explicit_user_capture', \
          'ios_share_extension', 'failed', now(), now())",
    )
    .bind(capture_id)
    .bind(owner)
    .execute(test.database.pool())
    .await
    .expect("selected capture stores");
    sqlx::query("delete from threads_archive.captures where capture_id = $1")
        .bind(capture_id)
        .execute(test.database.pool())
        .await
        .expect("privacy deletion wins race");
    let now = Utc::now();
    let mut budget = available_budget(now);
    let before = budget;
    let mut resolver_calls = 0_u32;

    let outcome =
        claim_capture_for_resolution(&test.database, capture_id, &mut budget, 512, now, || {
            resolver_calls += 1;
        })
        .await
        .expect("claim recheck answers");
    let durable: (i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.post_revisions), \
           (select count(*) from threads_archive.outbox_events)",
    )
    .fetch_one(test.database.pool())
    .await
    .expect("durable race state reads");

    assert_eq!(
        outcome,
        ReResolutionAttemptOutcome::Skipped(ReResolutionSkipReason::LocallyRemoved)
    );
    assert_eq!(resolver_calls, 0);
    assert_eq!(budget, before);
    assert_eq!(durable, (0, 0));

    test.cleanup().await.expect("cleanup must drop");
}
