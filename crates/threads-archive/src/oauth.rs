//! Official Threads OAuth primitives.

use std::collections::BTreeMap;

use aes_gcm::{Aes256Gcm, KeyInit as _, Nonce, aead::Aead};
use sha2::{Digest as _, Sha256};

use crate::Database;
use crate::capability::{AcquisitionMode, NATIVE_SAVED_LIST_SYNC, SupportStatus};

const FORMAT_MARKER: [u8; 2] = [b't', 1];
const GENERATION_LEN: usize = 4;
const OWNER_BINDING_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const HEADER_LEN: usize = FORMAT_MARKER.len() + GENERATION_LEN + OWNER_BINDING_LEN + NONCE_LEN;
const CREDENTIAL_LABEL: &[u8] = b"ratatoskr/threads/official-credential/v1";
type ParsedEnvelope<'a> = (u32, [u8; OWNER_BINDING_LEN], [u8; NONCE_LEN], &'a [u8]);

/// The official account type the provider reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountType {
    /// A creator account.
    Creator,
}

/// A capability evaluated for one connected official account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OfficialCapability {
    /// The provider can identify the connected account.
    AccountIdentity,
    /// Own-account content synchronization.
    OwnAccountSync,
    /// Personal native Saved-list synchronization.
    NativeSavedList,
    /// Provider publishing.
    Publishing,
}

/// Whether an account capability is usable by this product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityAvailability {
    /// The capability is usable now.
    Available,
    /// The capability is unavailable with an explicit non-secret reason.
    Unavailable(String),
}

/// Stable account metadata returned by the official provider.
#[derive(Debug, Clone)]
pub struct OfficialAccount {
    account_id: uuid::Uuid,
    user_ref: uuid::Uuid,
    provider_account_id: String,
    username: String,
    account_type: AccountType,
}

/// A provider grant that has not yet crossed the encrypted storage boundary.
#[derive(Clone)]
pub struct OfficialGrant {
    access_token: String,
    refresh_token: String,
    scopes: Vec<String>,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl std::fmt::Debug for OfficialGrant {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialGrant")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("scopes", &self.scopes)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Whether the provider definitively acknowledged account revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevocationOutcome {
    /// The provider definitively acknowledged the revoke request.
    Confirmed,
    /// The provider request did not produce a definitive response.
    Uncertain,
}

/// A non-secret remaining-budget observation from an official provider response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetObservation {
    endpoint_class: String,
    remaining: u32,
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
    request_id: Option<String>,
}

/// A store for official credential lifecycle state.
#[derive(Debug, Clone)]
pub struct OfficialCredentialStore {
    database: Database,
    cipher: CredentialCipher,
}

/// A non-secret credential lifecycle failure.
#[derive(Debug, thiserror::Error)]
pub enum OfficialOAuthError {
    /// Credential encryption or decoding failed.
    #[error("official credential processing failed")]
    Credential(#[from] CredentialCipherError),
    /// The durable store rejected a lifecycle operation.
    #[error("official credential storage failed")]
    Storage(#[source] sqlx::Error),
    /// This placeholder path is not implemented.
    #[error("official credential lifecycle is not implemented")]
    Unavailable,
    /// The provider did not confirm revocation, so local material was retained.
    #[error("official credential revocation is uncertain")]
    RevocationUncertain,
}

impl OfficialAccount {
    /// Creates validated official account metadata.
    #[must_use]
    pub fn new(
        account_id: uuid::Uuid,
        user_ref: uuid::Uuid,
        provider_account_id: &str,
        username: &str,
        account_type: AccountType,
    ) -> Self {
        Self {
            account_id,
            user_ref,
            provider_account_id: provider_account_id.to_owned(),
            username: username.to_owned(),
            account_type,
        }
    }
}

impl OfficialGrant {
    /// Creates a grant that will be encrypted before durable storage.
    #[must_use]
    pub fn new(
        access_token: &str,
        refresh_token: &str,
        scopes: Vec<String>,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            access_token: access_token.to_owned(),
            refresh_token: refresh_token.to_owned(),
            scopes,
            expires_at,
        }
    }
}

impl OfficialCredentialStore {
    /// Creates an official credential store over the service-owned database.
    #[must_use]
    pub fn new(database: Database, cipher: CredentialCipher) -> Self {
        Self { database, cipher }
    }

    /// Records a connected account and its encrypted official grant.
    ///
    /// # Errors
    ///
    /// Returns an error when sealing or the atomic database write fails.
    pub async fn connect(
        &self,
        account: OfficialAccount,
        grant: OfficialGrant,
    ) -> Result<(), OfficialOAuthError> {
        let owner = account.account_id.to_string();
        let access = self.cipher.seal(&owner, &grant.access_token, "")?;
        let refresh = self.cipher.seal(&owner, &grant.refresh_token, "")?;
        let scopes = grant.scopes.join(" ");
        let mut transaction = self
            .database
            .pool()
            .begin()
            .await
            .map_err(OfficialOAuthError::Storage)?;
        sqlx::query(
            "insert into threads_archive.accounts \
             (account_id, user_ref, provider_account_id, username, account_type, connection_status, scopes, connected_at) \
             values ($1, $2, $3, $4, $5, 'connected', $6, now())",
        )
        .bind(account.account_id)
        .bind(account.user_ref)
        .bind(account.provider_account_id)
        .bind(account.username)
        .bind(account.account_type.as_str())
        .bind(&scopes)
        .execute(&mut *transaction)
        .await
        .map_err(OfficialOAuthError::Storage)?;
        sqlx::query("delete from threads_archive.credentials where account_id = $1")
            .bind(account.account_id)
            .execute(&mut *transaction)
            .await
            .map_err(OfficialOAuthError::Storage)?;
        sqlx::query(
            "insert into threads_archive.credentials \
             (credential_id, account_id, access_token_ciphertext, token_version, scopes, refresh_token_ciphertext, expires_at, rotated_at) \
             values ($1, $2, $3, $4, $5, $6, $7, now())",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(account.account_id)
        .bind(access.ciphertext())
        .bind(i32::try_from(self.cipher.generation).map_err(|_| OfficialOAuthError::Unavailable)?)
        .bind(&scopes)
        .bind(refresh.ciphertext())
        .bind(grant.expires_at)
        .execute(&mut *transaction)
        .await
        .map_err(OfficialOAuthError::Storage)?;
        append_audit(&mut transaction, account.account_id, "connected").await?;
        transaction
            .commit()
            .await
            .map_err(OfficialOAuthError::Storage)
    }

    /// Scrubs an officially revoked account's local credential material.
    ///
    /// # Errors
    ///
    /// Returns an error when the atomic scrub transaction fails.
    pub async fn revoke(
        &self,
        account_id: uuid::Uuid,
        outcome: RevocationOutcome,
    ) -> Result<(), OfficialOAuthError> {
        match outcome {
            RevocationOutcome::Confirmed => self.scrub_revoked(account_id).await,
            RevocationOutcome::Uncertain => Err(OfficialOAuthError::RevocationUncertain),
        }
    }

    /// Replaces an account's active grant after a successful official refresh.
    ///
    /// # Errors
    ///
    /// Returns an error when encryption or the atomic replacement fails.
    pub async fn refresh(
        &self,
        account_id: uuid::Uuid,
        grant: OfficialGrant,
    ) -> Result<(), OfficialOAuthError> {
        let owner = account_id.to_string();
        let access = self.cipher.seal(&owner, &grant.access_token, "")?;
        let refresh = self.cipher.seal(&owner, &grant.refresh_token, "")?;
        let scopes = grant.scopes.join(" ");
        let mut transaction = self
            .database
            .pool()
            .begin()
            .await
            .map_err(OfficialOAuthError::Storage)?;
        sqlx::query("delete from threads_archive.credentials where account_id = $1")
            .bind(account_id)
            .execute(&mut *transaction)
            .await
            .map_err(OfficialOAuthError::Storage)?;
        sqlx::query("update threads_archive.accounts set scopes = $2, connection_status = 'connected', updated_at = now() where account_id = $1")
            .bind(account_id).bind(&scopes).execute(&mut *transaction).await.map_err(OfficialOAuthError::Storage)?;
        sqlx::query("insert into threads_archive.credentials (credential_id, account_id, access_token_ciphertext, token_version, scopes, refresh_token_ciphertext, expires_at, rotated_at) values ($1, $2, $3, $4, $5, $6, $7, now())")
            .bind(uuid::Uuid::now_v7()).bind(account_id).bind(access.ciphertext())
            .bind(i32::try_from(self.cipher.generation).map_err(|_| OfficialOAuthError::Unavailable)?)
            .bind(&scopes).bind(refresh.ciphertext()).bind(grant.expires_at)
            .execute(&mut *transaction).await.map_err(OfficialOAuthError::Storage)?;
        append_audit(&mut transaction, account_id, "refreshed").await?;
        transaction
            .commit()
            .await
            .map_err(OfficialOAuthError::Storage)
    }

    /// Records one validated official API budget observation.
    ///
    /// # Errors
    ///
    /// Returns an error when the observation does not fit durable bounds or its write fails.
    pub async fn record_budget(
        &self,
        account_id: uuid::Uuid,
        observation: BudgetObservation,
    ) -> Result<(), OfficialOAuthError> {
        sqlx::query(
            "insert into threads_archive.account_budgets \
             (account_id, endpoint_class, remaining, resets_at, request_id) values ($1, $2, $3, $4, $5) \
             on conflict (account_id, endpoint_class) do update set \
             remaining = excluded.remaining, resets_at = excluded.resets_at, request_id = excluded.request_id, observed_at = now()",
        )
        .bind(account_id)
        .bind(observation.endpoint_class)
        .bind(i32::try_from(observation.remaining).map_err(|_| OfficialOAuthError::Unavailable)?)
        .bind(observation.resets_at)
        .bind(observation.request_id)
        .execute(self.database.pool())
        .await
        .map_err(OfficialOAuthError::Storage)?;
        Ok(())
    }

    async fn scrub_revoked(&self, account_id: uuid::Uuid) -> Result<(), OfficialOAuthError> {
        let mut transaction = self
            .database
            .pool()
            .begin()
            .await
            .map_err(OfficialOAuthError::Storage)?;
        sqlx::query("delete from threads_archive.credentials where account_id = $1")
            .bind(account_id)
            .execute(&mut *transaction)
            .await
            .map_err(OfficialOAuthError::Storage)?;
        sqlx::query(
            "update threads_archive.accounts \
             set connection_status = 'revoked', scopes = '', updated_at = now() where account_id = $1",
        )
        .bind(account_id)
        .execute(&mut *transaction)
        .await
        .map_err(OfficialOAuthError::Storage)?;
        append_audit(&mut transaction, account_id, "revoked").await?;
        transaction
            .commit()
            .await
            .map_err(OfficialOAuthError::Storage)
    }
}

impl BudgetObservation {
    /// Creates a bounded budget observation without provider response bodies or credentials.
    ///
    /// # Errors
    ///
    /// Returns an error when endpoint or request identifiers exceed their durable bounds.
    pub fn new(
        endpoint_class: &str,
        remaining: u32,
        resets_at: Option<chrono::DateTime<chrono::Utc>>,
        request_id: Option<&str>,
    ) -> Result<Self, OfficialOAuthError> {
        if endpoint_class.is_empty()
            || endpoint_class.len() > 64
            || request_id.is_some_and(|id| id.len() > 256)
        {
            return Err(OfficialOAuthError::Unavailable);
        }
        Ok(Self {
            endpoint_class: endpoint_class.to_owned(),
            remaining,
            resets_at,
            request_id: request_id.map(str::to_owned),
        })
    }
}

impl AccountType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Creator => "creator",
        }
    }
}

async fn append_audit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: uuid::Uuid,
    event_kind: &str,
) -> Result<(), OfficialOAuthError> {
    sqlx::query(
        "insert into threads_archive.credential_audit (audit_id, account_id, event_kind) values ($1, $2, $3)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(account_id)
    .bind(event_kind)
    .execute(&mut **transaction)
    .await
    .map_err(OfficialOAuthError::Storage)?;
    Ok(())
}

/// Reconciles official account discovery against the local capability matrix.
#[must_use]
pub fn reconcile_capabilities(
    _account_type: AccountType,
    scopes: &[String],
) -> BTreeMap<OfficialCapability, CapabilityAvailability> {
    let has_basic = scopes.iter().any(|scope| scope == "threads_basic");
    BTreeMap::from([
        (
            OfficialCapability::AccountIdentity,
            availability_for_scope(has_basic, "threads_basic"),
        ),
        (
            OfficialCapability::OwnAccountSync,
            availability_for_own_account_sync(has_basic),
        ),
        (
            OfficialCapability::NativeSavedList,
            CapabilityAvailability::Unavailable(NATIVE_SAVED_LIST_SYNC.reason.to_owned()),
        ),
        (
            OfficialCapability::Publishing,
            CapabilityAvailability::Unavailable("publishing requires separate consent".to_owned()),
        ),
    ])
}

fn availability_for_own_account_sync(has_basic: bool) -> CapabilityAvailability {
    match AcquisitionMode::OwnAccountSync.capability().status {
        SupportStatus::Supported => availability_for_scope(has_basic, "threads_basic"),
        status => availability_for_matrix(status),
    }
}

fn availability_for_scope(has_scope: bool, required_scope: &str) -> CapabilityAvailability {
    if has_scope {
        CapabilityAvailability::Available
    } else {
        CapabilityAvailability::Unavailable(format!("missing required scope: {required_scope}"))
    }
}

fn availability_for_matrix(status: SupportStatus) -> CapabilityAvailability {
    match status {
        SupportStatus::Supported => CapabilityAvailability::Available,
        SupportStatus::Planned => {
            CapabilityAvailability::Unavailable("own_account_sync is planned".to_owned())
        }
        SupportStatus::NotSupported => {
            CapabilityAvailability::Unavailable("own_account_sync is not supported".to_owned())
        }
    }
}

/// Errors from the credential envelope boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CredentialCipherError {
    /// Key generation zero is not a meaningful configured key generation.
    #[error("credential key generation is invalid")]
    InvalidKeyGeneration,
    /// The envelope does not contain one complete supported credential message.
    #[error("credential envelope is malformed")]
    MalformedEnvelope,
    /// The envelope was written under a different configured key generation.
    #[error("credential envelope key generation does not match")]
    KeyGeneration,
    /// The envelope belongs to another account.
    #[error("credential envelope owner binding does not match")]
    Binding,
    /// The authenticated encryption operation was refused.
    #[error("credential envelope authentication failed")]
    Authentication,
    /// The operating-system random source was unavailable.
    #[error("credential nonce could not be generated")]
    Randomness,
}

/// A credential cipher configured with one master key generation.
#[derive(Clone)]
pub struct CredentialCipher {
    key: [u8; 32],
    generation: u32,
}

/// An encrypted credential envelope.
#[derive(Clone)]
pub struct CredentialEnvelope {
    ciphertext: Vec<u8>,
}

/// Decrypted credential material for the official adapter.
#[derive(Clone, PartialEq, Eq)]
pub struct OfficialCredential {
    access_token: String,
    refresh_token: String,
}

impl CredentialCipher {
    /// Constructs a cipher from the configured master key and its generation.
    ///
    /// # Errors
    ///
    /// Returns [`CredentialCipherError::InvalidKeyGeneration`] for generation zero.
    pub fn new(key: [u8; 32], generation: u32) -> Result<Self, CredentialCipherError> {
        if generation == 0 {
            return Err(CredentialCipherError::InvalidKeyGeneration);
        }
        Ok(Self { key, generation })
    }

    /// Seals one official access and refresh grant for an account.
    ///
    /// # Errors
    ///
    /// Returns an error when operating-system randomness or authenticated encryption fails.
    pub fn seal(
        &self,
        owner: &str,
        access_token: &str,
        refresh_token: &str,
    ) -> Result<CredentialEnvelope, CredentialCipherError> {
        let mut nonce = [0_u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|_| CredentialCipherError::Randomness)?;
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| CredentialCipherError::Authentication)?;
        let binding = owner_binding(owner);
        let plaintext = encode_credential(access_token, refresh_token)?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from(nonce).as_ref(),
                aes_gcm::aead::Payload {
                    msg: &plaintext,
                    aad: &associated_data(owner),
                },
            )
            .map_err(|_| CredentialCipherError::Authentication)?;
        let mut envelope = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        envelope.extend_from_slice(&FORMAT_MARKER);
        envelope.extend_from_slice(&self.generation.to_be_bytes());
        envelope.extend_from_slice(&binding);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        Ok(CredentialEnvelope {
            ciphertext: envelope,
        })
    }

    /// Opens a credential envelope for its account owner.
    ///
    /// # Errors
    ///
    /// Returns an error when the message is malformed, belongs to another owner,
    /// has another key generation, or fails authentication.
    pub fn open(
        &self,
        owner: &str,
        envelope: &CredentialEnvelope,
    ) -> Result<OfficialCredential, CredentialCipherError> {
        let (generation, binding, nonce, ciphertext) = parse_envelope(envelope.ciphertext())?;
        if generation != self.generation {
            return Err(CredentialCipherError::KeyGeneration);
        }
        if binding != owner_binding(owner) {
            return Err(CredentialCipherError::Binding);
        }
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| CredentialCipherError::Authentication)?;
        let plaintext = cipher
            .decrypt(
                Nonce::from(nonce).as_ref(),
                aes_gcm::aead::Payload {
                    msg: ciphertext,
                    aad: &associated_data(owner),
                },
            )
            .map_err(|_| CredentialCipherError::Authentication)?;
        decode_credential(&plaintext)
    }
}

impl std::fmt::Debug for CredentialCipher {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CredentialCipher")
            .field("key", &"[REDACTED]")
            .field("generation", &self.generation)
            .finish()
    }
}

impl std::fmt::Debug for CredentialEnvelope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CredentialEnvelope")
            .field("ciphertext", &"[REDACTED]")
            .finish()
    }
}

impl std::fmt::Debug for OfficialCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OfficialCredential([REDACTED])")
    }
}

fn owner_binding(owner: &str) -> [u8; OWNER_BINDING_LEN] {
    Sha256::digest([CREDENTIAL_LABEL, owner.as_bytes()].concat()).into()
}

fn associated_data(owner: &str) -> Vec<u8> {
    [CREDENTIAL_LABEL, owner.as_bytes()].concat()
}

fn encode_credential(
    access_token: &str,
    refresh_token: &str,
) -> Result<Vec<u8>, CredentialCipherError> {
    let access =
        u32::try_from(access_token.len()).map_err(|_| CredentialCipherError::MalformedEnvelope)?;
    let refresh =
        u32::try_from(refresh_token.len()).map_err(|_| CredentialCipherError::MalformedEnvelope)?;
    let mut plaintext = Vec::with_capacity(8 + access_token.len() + refresh_token.len());
    plaintext.extend_from_slice(&access.to_be_bytes());
    plaintext.extend_from_slice(access_token.as_bytes());
    plaintext.extend_from_slice(&refresh.to_be_bytes());
    plaintext.extend_from_slice(refresh_token.as_bytes());
    Ok(plaintext)
}

fn parse_envelope(envelope: &[u8]) -> Result<ParsedEnvelope<'_>, CredentialCipherError> {
    if envelope.len() < HEADER_LEN + TAG_LEN
        || envelope.get(..FORMAT_MARKER.len()) != Some(&FORMAT_MARKER)
    {
        return Err(CredentialCipherError::MalformedEnvelope);
    }
    let generation_start = FORMAT_MARKER.len();
    let binding_start = generation_start + GENERATION_LEN;
    let nonce_start = binding_start + OWNER_BINDING_LEN;
    let generation = parse_array::<GENERATION_LEN>(envelope.get(generation_start..binding_start))
        .map(u32::from_be_bytes)?;
    let binding = parse_array::<OWNER_BINDING_LEN>(envelope.get(binding_start..nonce_start))?;
    let nonce_end = nonce_start + NONCE_LEN;
    let nonce = parse_array::<NONCE_LEN>(envelope.get(nonce_start..nonce_end))?;
    let ciphertext = envelope
        .get(nonce_end..)
        .ok_or(CredentialCipherError::MalformedEnvelope)?;
    Ok((generation, binding, nonce, ciphertext))
}

fn parse_array<const SIZE: usize>(
    input: Option<&[u8]>,
) -> Result<[u8; SIZE], CredentialCipherError> {
    input
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(CredentialCipherError::MalformedEnvelope)
}

fn decode_credential(plaintext: &[u8]) -> Result<OfficialCredential, CredentialCipherError> {
    let (access, remainder) = decode_part(plaintext)?;
    let (refresh, trailing) = decode_part(remainder)?;
    if !trailing.is_empty() {
        return Err(CredentialCipherError::MalformedEnvelope);
    }
    let access_token =
        String::from_utf8(access.to_vec()).map_err(|_| CredentialCipherError::MalformedEnvelope)?;
    let refresh_token = String::from_utf8(refresh.to_vec())
        .map_err(|_| CredentialCipherError::MalformedEnvelope)?;
    Ok(OfficialCredential {
        access_token,
        refresh_token,
    })
}

fn decode_part(input: &[u8]) -> Result<(&[u8], &[u8]), CredentialCipherError> {
    let Some((length, remainder)) = input.split_first_chunk::<4>() else {
        return Err(CredentialCipherError::MalformedEnvelope);
    };
    let length = usize::try_from(u32::from_be_bytes(*length))
        .map_err(|_| CredentialCipherError::MalformedEnvelope)?;
    if remainder.len() < length {
        return Err(CredentialCipherError::MalformedEnvelope);
    }
    Ok(remainder.split_at(length))
}

impl CredentialEnvelope {
    /// Reconstructs an encrypted envelope loaded from service-owned durable storage.
    #[must_use]
    pub fn from_ciphertext(ciphertext: Vec<u8>) -> Self {
        Self { ciphertext }
    }
    /// Returns the persisted encrypted representation.
    #[must_use]
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }
}

impl OfficialCredential {
    /// Returns the access token only for the official adapter boundary.
    #[must_use]
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// Returns the refresh token only for the official adapter boundary.
    #[must_use]
    pub fn refresh_token(&self) -> &str {
        &self.refresh_token
    }
}
