//! Official OAuth credential-envelope contract.

use chrono::{DateTime, Utc};
use ratatoskr_threads_archive::oauth::{
    AccountType, BudgetObservation, CapabilityAvailability, OfficialAccount, OfficialCapability,
    OfficialCredentialStore, OfficialGrant, RevocationOutcome, reconcile_capabilities,
};
use ratatoskr_threads_archive::oauth::{CredentialCipher, CredentialCipherError};
use ratatoskr_threads_archive::test_support::TestDatabase;
use uuid::Uuid;

#[test]
fn credential_envelope_round_trips_only_for_its_owner() {
    const OWNER: &str = "018f54e0-0000-7000-8000-000000000001";
    const OTHER_OWNER: &str = "018f54e0-0000-7000-8000-000000000002";
    const ACCESS_TOKEN: &str = "redacted-access-token";
    const REFRESH_TOKEN: &str = "redacted-refresh-token";

    let cipher = CredentialCipher::new([0x5A; 32], 7)
        .expect("a deterministic 32-byte test key and generation must be accepted");
    let envelope = cipher
        .seal(OWNER, ACCESS_TOKEN, REFRESH_TOKEN)
        .expect("a valid owner-bound credential must seal");

    for plaintext in [ACCESS_TOKEN, REFRESH_TOKEN] {
        assert!(
            !envelope
                .ciphertext()
                .windows(plaintext.len())
                .any(|window| window == plaintext.as_bytes()),
            "persisted ciphertext must not contain plaintext token material"
        );
    }

    let opened = cipher
        .open(OWNER, &envelope)
        .expect("the account that sealed the credential must open it");
    assert_eq!(opened.access_token(), ACCESS_TOKEN);
    assert_eq!(opened.refresh_token(), REFRESH_TOKEN);

    assert!(matches!(
        cipher.open(OTHER_OWNER, &envelope),
        Err(CredentialCipherError::Binding)
    ));
}

#[test]
fn malformed_and_tampered_envelopes_are_refused() {
    let cipher = CredentialCipher::new([0x22; 32], 1).expect("cipher");
    assert!(matches!(
        cipher.open(
            "owner",
            &ratatoskr_threads_archive::oauth::CredentialEnvelope::from_ciphertext(vec![0; 3])
        ),
        Err(CredentialCipherError::MalformedEnvelope)
    ));
    let envelope = cipher.seal("owner", "access", "refresh").expect("seal");
    let mut bytes = envelope.ciphertext().to_vec();
    *bytes
        .last_mut()
        .expect("authenticated ciphertext has a tag") ^= 1;
    assert!(matches!(
        cipher.open(
            "owner",
            &ratatoskr_threads_archive::oauth::CredentialEnvelope::from_ciphertext(bytes)
        ),
        Err(CredentialCipherError::Authentication)
    ));
}

#[test]
fn capability_discovery_reconciles_scopes_against_the_matrix() {
    let capabilities = reconcile_capabilities(
        AccountType::Creator,
        &[
            "threads_basic".to_owned(),
            "threads_content_publish".to_owned(),
        ],
    );

    assert_eq!(
        capabilities.get(&OfficialCapability::AccountIdentity),
        Some(&CapabilityAvailability::Available)
    );
    assert_eq!(
        capabilities.get(&OfficialCapability::OwnAccountSync),
        Some(&CapabilityAvailability::Available)
    );
    assert_eq!(
        capabilities.get(&OfficialCapability::NativeSavedList),
        Some(&CapabilityAvailability::Unavailable(
            "no supported provider surface exposes the personal Saved list".to_owned()
        ))
    );
    assert_eq!(
        capabilities.get(&OfficialCapability::Publishing),
        Some(&CapabilityAvailability::Unavailable(
            "publishing requires separate consent".to_owned()
        ))
    );
}

#[test]
fn capability_discovery_names_a_missing_required_scope() {
    let capabilities = reconcile_capabilities(AccountType::Creator, &[]);
    assert_eq!(
        capabilities.get(&OfficialCapability::AccountIdentity),
        Some(&CapabilityAvailability::Unavailable(
            "missing required scope: threads_basic".to_owned()
        ))
    );
}

#[tokio::test]
async fn revoke_scrubs_every_secret_and_keeps_non_secret_audit_evidence() {
    let test = TestDatabase::create().await.expect("a disposable database");
    let cipher = CredentialCipher::new([0x3C; 32], 1).expect("test cipher");
    let store = OfficialCredentialStore::new(test.database.clone(), cipher);
    let account_id = Uuid::now_v7();
    store
        .connect(
            OfficialAccount::new(
                account_id,
                Uuid::now_v7(),
                "meta-123",
                "threads-user",
                AccountType::Creator,
            ),
            OfficialGrant::new(
                "access-secret",
                "refresh-secret",
                vec!["threads_basic".to_owned()],
                at("2026-09-01T00:00:00Z"),
            ),
        )
        .await
        .expect("connection stores the encrypted grant");

    store
        .revoke(account_id, RevocationOutcome::Confirmed)
        .await
        .expect("a definitive revoke scrubs local material");

    let credential_rows: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.credentials where account_id = $1",
    )
    .bind(account_id)
    .fetch_one(test.database.pool())
    .await
    .expect("credential count");
    let account: (String, String) = sqlx::query_as(
        "select connection_status, scopes from threads_archive.accounts where account_id = $1",
    )
    .bind(account_id)
    .fetch_one(test.database.pool())
    .await
    .expect("revoked account");
    let audit_rows: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.credential_audit where account_id = $1 and event_kind = 'revoked'",
    )
    .bind(account_id)
    .fetch_one(test.database.pool())
    .await
    .expect("audit count");

    assert_eq!(
        credential_rows, 0,
        "all encrypted token fields must be removed"
    );
    assert_eq!(account, ("revoked".to_owned(), String::new()));
    assert_eq!(audit_rows, 1, "only non-secret lifecycle evidence remains");
    test.cleanup().await.expect("cleanup");
}

#[tokio::test]
async fn uncertain_revoke_retains_the_active_credential() {
    let test = TestDatabase::create().await.expect("database");
    let store = OfficialCredentialStore::new(
        test.database.clone(),
        CredentialCipher::new([0x43; 32], 1).expect("cipher"),
    );
    let account_id = Uuid::now_v7();
    store
        .connect(
            OfficialAccount::new(
                account_id,
                Uuid::now_v7(),
                "meta-uncertain",
                "threads-user",
                AccountType::Creator,
            ),
            OfficialGrant::new(
                "access",
                "refresh",
                vec!["threads_basic".to_owned()],
                at("2026-09-01T00:00:00Z"),
            ),
        )
        .await
        .expect("connect");
    assert!(matches!(
        store.revoke(account_id, RevocationOutcome::Uncertain).await,
        Err(ratatoskr_threads_archive::oauth::OfficialOAuthError::RevocationUncertain)
    ));
    let rows: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.credentials where account_id = $1",
    )
    .bind(account_id)
    .fetch_one(test.database.pool())
    .await
    .expect("count");
    assert_eq!(rows, 1);
    test.cleanup().await.expect("cleanup");
}

#[tokio::test]
async fn refresh_replaces_the_active_encrypted_grant() {
    let test = TestDatabase::create().await.expect("a disposable database");
    let store = OfficialCredentialStore::new(
        test.database.clone(),
        CredentialCipher::new([0x6D; 32], 1).expect("test cipher"),
    );
    let account_id = Uuid::now_v7();
    store
        .connect(
            OfficialAccount::new(
                account_id,
                Uuid::now_v7(),
                "meta-456",
                "threads-user",
                AccountType::Creator,
            ),
            OfficialGrant::new(
                "old-access",
                "old-refresh",
                vec!["threads_basic".to_owned()],
                at("2026-09-01T00:00:00Z"),
            ),
        )
        .await
        .expect("connected grant");
    let before: Vec<u8> = sqlx::query_scalar(
        "select access_token_ciphertext from threads_archive.credentials where account_id = $1",
    )
    .bind(account_id)
    .fetch_one(test.database.pool())
    .await
    .expect("old ciphertext");

    store
        .refresh(
            account_id,
            OfficialGrant::new(
                "new-access",
                "new-refresh",
                vec!["threads_basic".to_owned()],
                at("2026-10-01T00:00:00Z"),
            ),
        )
        .await
        .expect("refresh replaces grant");
    let after: (Vec<u8>, String) = sqlx::query_as("select access_token_ciphertext, scopes from threads_archive.credentials where account_id = $1")
        .bind(account_id).fetch_one(test.database.pool()).await.expect("refreshed credential");
    assert_ne!(
        before, after.0,
        "refresh must replace encrypted access material"
    );
    assert_eq!(after.1, "threads_basic");
    test.cleanup().await.expect("cleanup");
}

#[test]
fn budget_observation_refuses_unbounded_request_identifiers() {
    assert!(BudgetObservation::new("identity", 3, None, Some(&"x".repeat(257))).is_err());
}

#[tokio::test]
async fn valid_budget_observation_is_persisted_without_secret_material() {
    let test = TestDatabase::create().await.expect("database");
    let store = OfficialCredentialStore::new(
        test.database.clone(),
        CredentialCipher::new([0x44; 32], 1).expect("cipher"),
    );
    let account_id = Uuid::now_v7();
    store
        .connect(
            OfficialAccount::new(
                account_id,
                Uuid::now_v7(),
                "meta-budget",
                "threads-user",
                AccountType::Creator,
            ),
            OfficialGrant::new(
                "access",
                "refresh",
                vec!["threads_basic".to_owned()],
                at("2026-09-01T00:00:00Z"),
            ),
        )
        .await
        .expect("connect");
    store
        .record_budget(
            account_id,
            BudgetObservation::new(
                "identity",
                3,
                Some(at("2026-09-02T00:00:00Z")),
                Some("request-1"),
            )
            .expect("valid budget"),
        )
        .await
        .expect("record budget");
    let row: (i32, String) = sqlx::query_as("select remaining, request_id from threads_archive.account_budgets where account_id = $1 and endpoint_class = 'identity'").bind(account_id).fetch_one(test.database.pool()).await.expect("budget");
    assert_eq!(row, (3, "request-1".to_owned()));
    test.cleanup().await.expect("cleanup");
}

#[expect(
    clippy::expect_used,
    reason = "a fixed literal invalidates the test source rather than exercising production behavior"
)]
fn at(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("fixed RFC3339 instant")
        .with_timezone(&Utc)
}
