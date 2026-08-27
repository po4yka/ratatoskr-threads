//! Supported public-resolution contract.

use crate::permalink::{CanonicalizedUrl, Permalink};
use crate::publishing;
use crate::relation::RelationKind;
use crate::{Database, PersistenceError};
use chrono::{DateTime, Utc};
use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;
use reqwest::{StatusCode, Url};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt as _;
use uuid::Uuid;

/// The parser revision that interpreted one public observation.
pub const PARSER_VERSION: &str = "threads-oembed-v1";

/// A normalized public post observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicPost {
    /// Stable provider post identity.
    pub provider_post_id: String,
    /// The provider's canonical permalink.
    pub permalink: Permalink,
    /// Provider-visible embed HTML, retained as metadata but never executed.
    pub embed_html: String,
    /// Parser version responsible for this projection.
    pub parser_version: &'static str,
    /// Provider relations observed for this post. Each is normalized into a
    /// first-class graph row when the observation is stored.
    pub relations: Vec<RelationInput>,
}

/// Why an approved public observation was not accepted.
#[derive(Debug, thiserror::Error)]
pub enum PublicResolutionError {
    /// The response was not valid JSON.
    #[error("approved public response is not valid JSON")]
    InvalidJson,
    /// A required oEmbed field was absent or had the wrong type.
    #[error("approved public response is missing required field {0}")]
    MissingField(&'static str),
    /// The response did not identify Threads as its provider.
    #[error("approved public response did not identify Threads")]
    WrongProvider,
    /// The response URL did not identify the requested post.
    #[error("approved public response permalink does not match the requested post")]
    PermalinkMismatch,
    /// The approved endpoint was malformed or outside the allowlist.
    #[error("public resolver endpoint is not an approved Threads oEmbed HTTPS surface")]
    UnsupportedEndpoint,
    /// A public resolver request could not be built or completed.
    #[error("approved public resolver request failed")]
    Network,
    /// The resolver did not return a successful public-content observation.
    #[error("approved public resolver returned HTTP status {0}")]
    ProviderStatus(StatusCode),
    /// The response did not declare JSON content.
    #[error("approved public resolver did not return JSON content")]
    UnexpectedContentType,
    /// The resolver response exceeded the bounded raw-evidence limit.
    #[error("approved public resolver response exceeded the byte limit")]
    ResponseTooLarge,
    /// A relation belonged to a post other than the current observation.
    #[error("public observation relation does not originate from its post")]
    RelationSourceMismatch,
    /// The observation contained more relations than the bounded resolver contract permits.
    #[error("public observation exceeds the relation count limit")]
    TooManyRelations,
    /// The relation graph was invalid.
    #[error(transparent)]
    RelationGraph(#[from] RelationGraphError),
    /// Raw evidence could not be stored immutably by this service.
    #[error("raw public evidence storage failed")]
    RawStorage(#[source] std::io::Error),
    /// A content-addressed raw object disagreed with its digest path.
    #[error("stored raw evidence disagrees with its content digest")]
    RawDigestMismatch,
    /// An archive-owned query failed.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

/// One append-only resolution result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredResolution {
    /// Normalized post identity.
    pub post_id: Uuid,
    /// Immutable raw evidence identity.
    pub raw_object_id: Uuid,
    /// Parser-versioned revision identity.
    pub revision_id: Uuid,
    /// When the provider observation was recorded.
    pub observed_at: DateTime<Utc>,
}

/// A bounded, service-owned raw evidence directory.
///
/// Files are addressed by SHA-256 digest and created once. Database rows may
/// refer to the same immutable body from multiple observations without
/// replacing either observation's revision record.
#[derive(Debug, Clone)]
pub struct RawObjectStore {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredRaw {
    pub(crate) blob_ref: String,
    pub(crate) content_hash: Vec<u8>,
    pub(crate) byte_size: i64,
    pub(crate) media_type: &'static str,
}

impl RawObjectStore {
    /// Creates a service-owned raw evidence store rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Stores one raw response before it is normalized.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the immutable object cannot be
    /// created or an existing digest path has different bytes.
    pub(crate) async fn store(&self, bytes: &[u8]) -> Result<StoredRaw, PublicResolutionError> {
        let content_hash = Sha256::digest(bytes).to_vec();
        let digest = hex(&content_hash);
        let path = self.root.join("sha256").join(&digest);
        let byte_size = i64::try_from(bytes.len()).map_err(|_| {
            PublicResolutionError::Persistence(PersistenceError::Query(sqlx::Error::Configuration(
                "raw response size exceeds bigint".into(),
            )))
        })?;
        if fs::try_exists(&path)
            .await
            .map_err(PublicResolutionError::RawStorage)?
        {
            verify_raw_object(&path, &content_hash).await?;
        } else {
            self.create_once(&path, bytes, &content_hash).await?;
        }
        Ok(StoredRaw {
            blob_ref: format!("threads-archive/raw/sha256/{digest}"),
            content_hash,
            byte_size,
            media_type: "application/json",
        })
    }

    async fn create_once(
        &self,
        path: &Path,
        bytes: &[u8],
        content_hash: &[u8],
    ) -> Result<(), PublicResolutionError> {
        let parent = path
            .parent()
            .ok_or(PublicResolutionError::RawDigestMismatch)?;
        fs::create_dir_all(parent)
            .await
            .map_err(PublicResolutionError::RawStorage)?;
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .await
        {
            Ok(mut file) => {
                file.write_all(bytes)
                    .await
                    .map_err(PublicResolutionError::RawStorage)?;
                file.sync_all()
                    .await
                    .map_err(PublicResolutionError::RawStorage)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                verify_raw_object(path, content_hash).await
            }
            Err(error) => Err(PublicResolutionError::RawStorage(error)),
        }
    }
}

async fn verify_raw_object(path: &Path, expected_hash: &[u8]) -> Result<(), PublicResolutionError> {
    let existing = fs::read(path)
        .await
        .map_err(PublicResolutionError::RawStorage)?;
    if Sha256::digest(existing).as_slice() == expected_hash {
        Ok(())
    } else {
        Err(PublicResolutionError::RawDigestMismatch)
    }
}

/// One relation supplied by an approved public observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationInput {
    /// Referencing provider post identity.
    pub referencing_provider_post_id: String,
    /// Provider edge-kind token.
    pub relation_kind: String,
    /// Target provider post identity.
    pub target_provider_post_id: String,
    /// Optional target permalink evidence.
    pub target_permalink: Option<String>,
}

/// One normalized graph target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphTarget {
    /// The target exists in the normalized fixture set.
    Resolved,
    /// The target remains an explicit provider-identity reference.
    Unresolved {
        /// Observed canonical permalink evidence.
        permalink: Option<String>,
    },
}

/// One normalized directed graph edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRelation {
    /// Referencing provider post identity.
    pub referencing_provider_post_id: String,
    /// Validated provider edge kind.
    pub relation_kind: RelationKind,
    /// Target provider post identity.
    pub target_provider_post_id: String,
    /// Whether the target is available locally.
    pub target: GraphTarget,
}

/// Graph-normalization refusal.
#[derive(Debug, thiserror::Error)]
pub enum RelationGraphError {
    /// A provider edge kind was malformed.
    #[error(transparent)]
    InvalidKind(#[from] crate::relation::RelationKindError),
    /// A reply edge would close a directed cycle.
    #[error("reply relation would create a directed cycle")]
    ReplyCycle,
}

/// Normalizes fixture relation observations.
///
/// # Errors
///
/// Returns [`RelationGraphError::InvalidKind`] for malformed provider kinds or
/// [`RelationGraphError::ReplyCycle`] for a cyclic resolved reply hierarchy.
pub fn normalize_relations(
    known_provider_ids: &BTreeSet<String>,
    relations: Vec<RelationInput>,
) -> Result<Vec<GraphRelation>, RelationGraphError> {
    let mut normalized = Vec::with_capacity(relations.len());
    let mut replies: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for relation in relations {
        let kind = RelationKind::try_from(relation.relation_kind)?;
        let resolved = known_provider_ids.contains(&relation.target_provider_post_id);
        if kind.as_str() == "reply" && resolved {
            replies
                .entry(relation.referencing_provider_post_id.clone())
                .or_default()
                .push(relation.target_provider_post_id.clone());
        }
        normalized.push(GraphRelation {
            referencing_provider_post_id: relation.referencing_provider_post_id,
            relation_kind: kind,
            target_provider_post_id: relation.target_provider_post_id,
            target: if resolved {
                GraphTarget::Resolved
            } else {
                GraphTarget::Unresolved {
                    permalink: relation.target_permalink,
                }
            },
        });
    }
    if has_reply_cycle(&replies) {
        return Err(RelationGraphError::ReplyCycle);
    }
    normalized.sort_by(|left, right| {
        (
            &left.referencing_provider_post_id,
            left.relation_kind.as_str(),
            &left.target_provider_post_id,
        )
            .cmp(&(
                &right.referencing_provider_post_id,
                right.relation_kind.as_str(),
                &right.target_provider_post_id,
            ))
    });
    Ok(normalized)
}

fn has_reply_cycle(replies: &BTreeMap<String, Vec<String>>) -> bool {
    let mut complete = BTreeSet::new();
    for source in replies.keys() {
        if !complete.contains(source)
            && visits_cycle(source, replies, &mut BTreeSet::new(), &mut complete)
        {
            return true;
        }
    }
    false
}

fn visits_cycle(
    current: &str,
    replies: &BTreeMap<String, Vec<String>>,
    visiting: &mut BTreeSet<String>,
    complete: &mut BTreeSet<String>,
) -> bool {
    if !visiting.insert(current.to_owned()) {
        return true;
    }
    if let Some(targets) = replies.get(current) {
        for target in targets {
            if !complete.contains(target) && visits_cycle(target, replies, visiting, complete) {
                return true;
            }
        }
    }
    visiting.remove(current);
    complete.insert(current.to_owned());
    false
}

/// Persists public observations in the Threads-owned schema.
#[derive(Debug)]
pub struct PublicResolutionStore<'a> {
    database: &'a Database,
    raw_objects: RawObjectStore,
}

impl<'a> PublicResolutionStore<'a> {
    /// Builds a store over the archive database.
    #[must_use]
    pub fn new(database: &'a Database, raw_objects: RawObjectStore) -> Self {
        Self {
            database,
            raw_objects,
        }
    }

    /// Records one already-fetched approved observation against a capture.
    ///
    /// # Errors
    ///
    /// Returns [`PublicResolutionError::Persistence`] when an archive query
    /// cannot complete.
    pub async fn record(
        &self,
        capture_id: Uuid,
        post: &PublicPost,
        raw_response: &[u8],
    ) -> Result<StoredResolution, PublicResolutionError> {
        validate_relation_sources(post)?;
        let raw = self.raw_objects.store(raw_response).await?;
        let mut transaction = self
            .database
            .pool()
            .begin()
            .await
            .map_err(PersistenceError::Query)?;
        let raw_object_id = Uuid::now_v7();
        sqlx::query(
            "insert into threads_archive.raw_objects (raw_object_id, object_kind, blob_ref, content_hash, byte_size, media_type, observed_at) values ($1, 'oembed_response', $2, $3, $4, $5, now())",
        )
        .bind(raw_object_id).bind(raw.blob_ref).bind(raw.content_hash).bind(raw.byte_size).bind(raw.media_type)
        .execute(&mut *transaction).await.map_err(PersistenceError::Query)?;
        let post_id: Uuid = sqlx::query_scalar(
            "insert into threads_archive.posts (post_id, provider_post_id, permalink, post_kind, text_content, acquisition_method, saved_authority, upstream_status) values ($1, $2, $3, 'post', $4, 'public_resolution', 'explicit_user_capture', 'active') on conflict (provider_post_id) do update set permalink = excluded.permalink, text_content = excluded.text_content, updated_at = now() returning post_id",
        )
        .bind(Uuid::now_v7()).bind(&post.provider_post_id).bind(post.permalink.as_str()).bind(&post.embed_html)
        .fetch_one(&mut *transaction).await.map_err(PersistenceError::Query)?;
        let revision_id = Uuid::now_v7();
        let observed_at: DateTime<Utc> = sqlx::query_scalar(
            "insert into threads_archive.post_revisions (revision_id, post_id, raw_object_id, parser_version, observed_at) values ($1, $2, $3, $4, now()) returning observed_at",
        )
        .bind(revision_id).bind(post_id).bind(raw_object_id).bind(post.parser_version)
        .fetch_one(&mut *transaction).await.map_err(PersistenceError::Query)?;
        record_relations(&mut transaction, post_id, post).await?;
        sqlx::query("update threads_archive.captures set post_id = $2, status = 'resolved' where capture_id = $1")
            .bind(capture_id).bind(post_id).execute(&mut *transaction).await.map_err(PersistenceError::Query)?;
        sqlx::query("insert into threads_archive.capture_resolutions (resolution_id, capture_id, outcome, resolver_version, raw_object_id, observed_at) values ($1, $2, 'resolved', $3, $4, $5)")
            .bind(Uuid::now_v7()).bind(capture_id).bind(post.parser_version).bind(raw_object_id).bind(observed_at)
            .execute(&mut *transaction).await.map_err(PersistenceError::Query)?;
        publishing::append_fact(&mut transaction, capture_id)
            .await
            .map_err(|error| PersistenceError::Query(sqlx::Error::Protocol(error.to_string())))?;
        transaction
            .commit()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(StoredResolution {
            post_id,
            raw_object_id,
            revision_id,
            observed_at,
        })
    }
}

const RELATION_LOCK: i64 = 0x7261_7461_736b_7204;
const MAX_RELATIONS_PER_OBSERVATION: usize = 128;

fn validate_relation_sources(post: &PublicPost) -> Result<(), PublicResolutionError> {
    if post.relations.len() > MAX_RELATIONS_PER_OBSERVATION {
        return Err(PublicResolutionError::TooManyRelations);
    }
    if post
        .relations
        .iter()
        .any(|relation| relation.referencing_provider_post_id != post.provider_post_id)
    {
        return Err(PublicResolutionError::RelationSourceMismatch);
    }
    Ok(())
}

async fn record_relations(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    post_id: Uuid,
    post: &PublicPost,
) -> Result<(), PublicResolutionError> {
    sqlx::query("select pg_advisory_xact_lock($1)")
        .bind(RELATION_LOCK)
        .execute(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
    let known: BTreeSet<String> = sqlx::query_scalar(
        "select provider_post_id from threads_archive.posts where provider_post_id is not null",
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?
    .into_iter()
    .collect();
    let existing: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "select source.provider_post_id, relation.relation_kind, relation.target_provider_post_id, relation.target_permalink from threads_archive.post_relations relation join threads_archive.posts source on source.post_id = relation.referencing_post_id where relation.relation_kind = 'reply'",
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    let mut all_replies: Vec<RelationInput> = existing
        .into_iter()
        .map(
            |(
                referencing_provider_post_id,
                relation_kind,
                target_provider_post_id,
                target_permalink,
            )| RelationInput {
                referencing_provider_post_id,
                relation_kind,
                target_provider_post_id,
                target_permalink,
            },
        )
        .collect();
    all_replies.extend(
        post.relations
            .iter()
            .filter(|relation| relation.relation_kind == "reply")
            .cloned(),
    );
    normalize_relations(&known, all_replies)?;

    for relation in normalize_relations(&known, post.relations.clone())? {
        let target_post_id: Option<Uuid> = sqlx::query_scalar(
            "select post_id from threads_archive.posts where provider_post_id = $1",
        )
        .bind(&relation.target_provider_post_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
        let target_permalink = match relation.target {
            GraphTarget::Resolved => None,
            GraphTarget::Unresolved { permalink } => permalink,
        };
        sqlx::query(
            "insert into threads_archive.post_relations (relation_id, referencing_post_id, target_post_id, target_provider_post_id, target_permalink, relation_kind) values ($1, $2, $3, $4, $5, $6) on conflict (referencing_post_id, target_provider_post_id, relation_kind) do update set target_post_id = excluded.target_post_id, target_permalink = coalesce(excluded.target_permalink, threads_archive.post_relations.target_permalink)",
        )
        .bind(Uuid::now_v7())
        .bind(post_id)
        .bind(target_post_id)
        .bind(&relation.target_provider_post_id)
        .bind(target_permalink)
        .bind(relation.relation_kind.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(result, "{byte:02x}");
    }
    result
}

/// Parses one approved public oEmbed response for a requested permalink.
///
/// # Errors
///
/// Returns a typed refusal when JSON, provider identity, required fields, or
/// canonical permalink evidence is invalid.
pub fn parse_observation(
    requested: &Permalink,
    payload: &str,
) -> Result<PublicPost, PublicResolutionError> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|_| PublicResolutionError::InvalidJson)?;
    let provider = string_field(&value, "provider_name")?;
    if provider != "Threads" {
        return Err(PublicResolutionError::WrongProvider);
    }
    let url = string_field(&value, "url")?;
    let canonical =
        CanonicalizedUrl::try_from(url).map_err(|_| PublicResolutionError::PermalinkMismatch)?;
    if canonical.permalink() != requested {
        return Err(PublicResolutionError::PermalinkMismatch);
    }
    let provider_post_id = requested
        .as_str()
        .rsplit('/')
        .next()
        .ok_or(PublicResolutionError::PermalinkMismatch)?
        .to_owned();
    Ok(PublicPost {
        provider_post_id,
        permalink: requested.clone(),
        embed_html: string_field(&value, "html")?.to_owned(),
        parser_version: PARSER_VERSION,
        relations: Vec::new(),
    })
}

fn string_field<'a>(
    value: &'a serde_json::Value,
    name: &'static str,
) -> Result<&'a str, PublicResolutionError> {
    value
        .get(name)
        .and_then(serde_json::Value::as_str)
        .ok_or(PublicResolutionError::MissingField(name))
}

const MAX_OEMBED_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_OEMBED_RESPONSE_BYTES_U64: u64 = 256 * 1024;
const OEMBED_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const OEMBED_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// A response from the only supported public resolution surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicObservation {
    /// Normalized public metadata and parser revision.
    pub post: PublicPost,
    /// Exact public bytes retained before normalization.
    pub raw_response: Vec<u8>,
}

/// Rustls-only client for Meta's public Threads oEmbed surface.
#[derive(Debug, Clone)]
pub struct ApprovedOembedClient {
    client: reqwest::Client,
    endpoint: Url,
}

impl ApprovedOembedClient {
    /// Builds a resolver for a configured approved Threads oEmbed endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`PublicResolutionError::UnsupportedEndpoint`] if the endpoint
    /// is not HTTPS, not a Meta Threads public host, or not an oEmbed route.
    pub fn new(endpoint: &str) -> Result<Self, PublicResolutionError> {
        let endpoint =
            Url::parse(endpoint).map_err(|_| PublicResolutionError::UnsupportedEndpoint)?;
        if !approved_endpoint(&endpoint) {
            return Err(PublicResolutionError::UnsupportedEndpoint);
        }
        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .connect_timeout(OEMBED_CONNECT_TIMEOUT)
            .timeout(OEMBED_REQUEST_TIMEOUT)
            .build()
            .map_err(|_| PublicResolutionError::Network)?;
        Ok(Self { client, endpoint })
    }

    /// Fetches and parses one public oEmbed observation.
    ///
    /// No account credentials, browser cookies, private endpoints, redirects,
    /// or executable embed rendering participate in this request.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal for endpoint, transport, content-type, size,
    /// provider, or parser validation failures.
    pub async fn resolve(
        &self,
        permalink: &Permalink,
    ) -> Result<PublicObservation, PublicResolutionError> {
        let response = self
            .client
            .get(self.endpoint.clone())
            .query(&[("url", permalink.as_str())])
            .send()
            .await
            .map_err(|_| PublicResolutionError::Network)?;
        let status = response.status();
        if !status.is_success() {
            return Err(PublicResolutionError::ProviderStatus(status));
        }
        if !is_json(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
        ) {
            return Err(PublicResolutionError::UnexpectedContentType);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_OEMBED_RESPONSE_BYTES_U64)
        {
            return Err(PublicResolutionError::ResponseTooLarge);
        }
        let raw_response = bounded_body(response).await?;
        let payload =
            std::str::from_utf8(&raw_response).map_err(|_| PublicResolutionError::InvalidJson)?;
        let post = parse_observation(permalink, payload)?;
        Ok(PublicObservation { post, raw_response })
    }

    /// Resolves an approved public observation then appends it to the archive.
    ///
    /// # Errors
    ///
    /// Propagates the typed acquisition, parsing, raw-storage, graph, and
    /// persistence failures from [`Self::resolve`] and [`PublicResolutionStore::record`].
    pub async fn resolve_and_record(
        &self,
        store: &PublicResolutionStore<'_>,
        capture_id: Uuid,
        permalink: &Permalink,
    ) -> Result<StoredResolution, PublicResolutionError> {
        let observation = self.resolve(permalink).await?;
        store
            .record(capture_id, &observation.post, &observation.raw_response)
            .await
    }
}

fn approved_endpoint(endpoint: &Url) -> bool {
    let approved_host = matches!(
        endpoint.host_str(),
        Some("graph.threads.com" | "graph.threads.net")
    );
    endpoint.scheme() == "https"
        && approved_host
        && endpoint.path().ends_with("/oembed")
        && endpoint.query().is_none()
}

fn is_json(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|value| {
        value.split(';').next().is_some_and(|mime| {
            mime.trim().eq_ignore_ascii_case("application/json") || mime.trim().ends_with("+json")
        })
    })
}

async fn bounded_body(response: reqwest::Response) -> Result<Vec<u8>, PublicResolutionError> {
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| PublicResolutionError::Network)?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_OEMBED_RESPONSE_BYTES {
            return Err(PublicResolutionError::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
