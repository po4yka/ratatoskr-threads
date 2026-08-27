//! Transactional publication of Threads social-source facts.

use chrono::{DateTime, SecondsFormat, Utc};
use ratatoskr_event_envelope::{EventEnvelope, EventPayload};
use ratatoskr_identifiers::{
    BlobOwner, BlobRef, ContentDigest, DigestAlgorithm, DigestHex, EntityLocalId, Extensions,
    MediaType, SocialSourceId, TenantRef, WireTimestamp,
};
use ratatoskr_social_contracts::{
    AcquisitionMethod, CaptureCompleteness, Platform, PostPermalink, PostText, SavedAuthority,
    SocialRelation, SocialRelationKind, SocialSourceCaptured, SocialSourceSnapshot,
    SocialSourceUpdated, UpstreamAvailability,
};
use sha2::{Digest as _, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

const PLATFORM: &str = "threads";
const PRODUCER: &str = "ratatoskr-threads";
const IDENTITY_NAMESPACE: Uuid = Uuid::from_bytes([
    0x72, 0x61, 0x74, 0x61, 0x73, 0x6b, 0x72, 0x54, 0x68, 0x72, 0x65, 0x61, 0x64, 0x73, 0x53, 0x72,
]);

type StoredCapture = (
    Uuid,
    Uuid,
    String,
    String,
    DateTime<Utc>,
    String,
    String,
    Option<String>,
    String,
);

/// A fact could not be constructed from the persisted Threads evidence.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PublishError {
    /// The capture is not yet resolved to a normalized post and raw evidence.
    #[error("capture {0} has no publishable normalized record")]
    NothingToPublish(Uuid),
    /// A stored field did not meet its published contract grammar.
    #[error("contract validation failed for capture {capture_id}: {reason}")]
    ContractViolation {
        /// The capture whose persisted data is invalid.
        capture_id: Uuid,
        /// Safe structural reason; it never includes post bodies.
        reason: String,
    },
    /// An archive query or transactional outbox write failed.
    #[error("social-source publication persistence failed")]
    Persistence(#[from] sqlx::Error),
    /// Canonical event serialization failed.
    #[error("social-source publication serialization failed")]
    Serialization(#[from] serde_json::Error),
}

/// Appends the first fact for a source, or an update only when its persisted
/// normalized revision changed. The caller owns the surrounding state transaction.
pub(crate) async fn append_fact(
    connection: &mut PgConnection,
    capture_id: Uuid,
) -> Result<(), PublishError> {
    let snapshot = build_snapshot(connection, capture_id).await?;
    let source_id = snapshot.social_source_id.to_string();
    let digest = snapshot.content_digest.clone();
    let snapshot_value = serde_json::to_value(&snapshot)?;
    let previous_digest: Option<String> = sqlx::query_scalar(
        "select content_digest from threads_archive.social_source_revisions \
         where social_source_id = $1 order by observed_at desc limit 1",
    )
    .bind(
        source_id
            .parse::<Uuid>()
            .map_err(|error| violation(capture_id, error))?,
    )
    .fetch_optional(&mut *connection)
    .await?;
    if previous_digest.as_deref() == Some(&digest.hex.to_string()) {
        return Ok(());
    }

    let event_id = Uuid::now_v7();
    let owner = snapshot.owner.user_id().0.to_string();
    let (event_type, envelope) = if previous_digest.is_some() {
        (
            SocialSourceUpdated::EVENT_TYPE,
            envelope_value(
                event_id,
                &SocialSourceUpdated {
                    source: snapshot,
                    extensions: Extensions::default(),
                },
                &source_id,
                &owner,
                capture_id,
            )?,
        )
    } else {
        (
            SocialSourceCaptured::EVENT_TYPE,
            envelope_value(
                event_id,
                &SocialSourceCaptured {
                    source: snapshot,
                    extensions: Extensions::default(),
                },
                &source_id,
                &owner,
                capture_id,
            )?,
        )
    };
    sqlx::query(
        "insert into threads_archive.social_source_revisions \
         (source_revision_id, social_source_id, content_digest, snapshot, observed_at) \
         values ($1, $2, $3, $4, now())",
    )
    .bind(Uuid::now_v7())
    .bind(
        source_id
            .parse::<Uuid>()
            .map_err(|error| violation(capture_id, error))?,
    )
    .bind(digest.hex.to_string())
    .bind(snapshot_value)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "insert into threads_archive.outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, correlation_id, causation_id, occurred_at) \
         values ($1, $2, 'capture', $3, $4, $5, null, now())",
    )
    .bind(event_id)
    .bind(event_type)
    .bind(capture_id)
    .bind(envelope)
    .bind(capture_id)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

fn envelope_value<P>(
    event_id: Uuid,
    payload: &P,
    source_id: &str,
    owner: &str,
    capture_id: Uuid,
) -> Result<serde_json::Value, PublishError>
where
    P: EventPayload + serde::Serialize + Sync,
{
    let template = serde_json::json!({
        "event_id": event_id.to_string(),
        "event_type": P::EVENT_TYPE,
        "occurred_at": WireTimestamp::now().to_wire(),
        "producer": PRODUCER,
        "aggregate_id": format!("social_source:{source_id}"),
        "correlation_id": format!("capture:{capture_id}"),
        "tenant_id": format!("user:{owner}"),
        "schema_version": 1,
        "payload": {}
    });
    let mut envelope = EventEnvelope::from_json(serde_json::to_vec(&template)?.as_slice())
        .map_err(|error| violation(capture_id, error))?;
    envelope
        .set_payload(payload)
        .map_err(|error| violation(capture_id, error))?;
    let rendered = envelope
        .to_canonical_json()
        .map_err(|error| violation(capture_id, error))?;
    Ok(serde_json::from_str(&rendered)?)
}

async fn build_snapshot(
    connection: &mut PgConnection,
    capture_id: Uuid,
) -> Result<SocialSourceSnapshot, PublishError> {
    let (
        owner,
        post_id,
        canonical_url,
        acquisition,
        captured_at,
        provider_post_id,
        permalink,
        text,
        upstream_status,
    ) = load_capture(connection, capture_id).await?;
    let social_source_id =
        ensure_source(connection, owner, post_id, capture_id, &canonical_url).await?;
    let (hash, length, media_type) = load_raw(connection, post_id, capture_id).await?;
    let relations = load_relations(connection, post_id, capture_id).await?;
    let raw_digest = digest_from_bytes(&hash, capture_id)?;
    let content_digest = content_digest(text.as_ref(), &relations, &upstream_status, capture_id)?;
    Ok(SocialSourceSnapshot {
        social_source_id: SocialSourceId::parse(&social_source_id.to_string())
            .map_err(|error| violation(capture_id, error))?,
        platform: Platform::parse(PLATFORM).map_err(|error| violation(capture_id, error))?,
        external_post_id: EntityLocalId::parse(&provider_post_id)
            .map_err(|error| violation(capture_id, error))?,
        permalink: Some(
            PostPermalink::parse(&permalink).map_err(|error| violation(capture_id, error))?,
        ),
        owner: TenantRef::parse(&format!("user:{owner}"))
            .map_err(|error| violation(capture_id, error))?,
        author: None,
        published_at: None,
        captured_at: timestamp(captured_at, capture_id)?,
        text: text
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|value| PostText::parse(value).map_err(|error| violation(capture_id, error)))
            .transpose()?,
        media: Vec::new(),
        relations,
        folders: Vec::new(),
        content_digest,
        raw_blob: Some(BlobRef {
            owner_service: BlobOwner::parse(PRODUCER)
                .map_err(|error| violation(capture_id, error))?,
            digest: raw_digest,
            media_type: MediaType::parse(&media_type)
                .map_err(|error| violation(capture_id, error))?,
            length_bytes: u64::try_from(length).map_err(|error| violation(capture_id, error))?,
        }),
        acquisition: acquisition_method(&acquisition, capture_id)?,
        saved_authority: SavedAuthority::ExplicitUserCapture,
        completeness: CaptureCompleteness::Complete,
        upstream_availability: availability(&upstream_status, capture_id)?,
        checkpoint: None,
        warnings: Vec::new(),
        extensions: Extensions::default(),
    })
}

async fn load_capture(
    connection: &mut PgConnection,
    capture_id: Uuid,
) -> Result<StoredCapture, PublishError> {
    sqlx::query_as(
        "select c.user_ref, c.post_id, c.canonical_url, c.acquisition_method, c.captured_at, \
                p.provider_post_id, p.permalink, p.text_content, p.upstream_status \
         from threads_archive.captures c join threads_archive.posts p on p.post_id = c.post_id \
         where c.capture_id = $1",
    )
    .bind(capture_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PublishError::NothingToPublish(capture_id))
}

async fn load_raw(
    connection: &mut PgConnection,
    post_id: Uuid,
    capture_id: Uuid,
) -> Result<(Vec<u8>, i64, String), PublishError> {
    sqlx::query_as(
        "select raw.content_hash, raw.byte_size, raw.media_type \
         from threads_archive.post_revisions revision \
         join threads_archive.raw_objects raw on raw.raw_object_id = revision.raw_object_id \
         where revision.post_id = $1 order by revision.observed_at desc, revision.revision_id desc limit 1",
    )
    .bind(post_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PublishError::NothingToPublish(capture_id))
}

async fn load_relations(
    connection: &mut PgConnection,
    post_id: Uuid,
    capture_id: Uuid,
) -> Result<Vec<SocialRelation>, PublishError> {
    let relation_rows: Vec<(String, String)> = sqlx::query_as(
        "select relation_kind, target_provider_post_id from threads_archive.post_relations \
         where referencing_post_id = $1 order by relation_kind, target_provider_post_id",
    )
    .bind(post_id)
    .fetch_all(&mut *connection)
    .await?;
    relation_rows
        .into_iter()
        .map(|(kind, target)| {
            Ok(SocialRelation {
                relation_kind: SocialRelationKind::parse(&kind)
                    .map_err(|error| violation(capture_id, error))?,
                target_post_id: EntityLocalId::parse(&target)
                    .map_err(|error| violation(capture_id, error))?,
            })
        })
        .collect()
}

async fn ensure_source(
    connection: &mut PgConnection,
    owner: Uuid,
    post_id: Uuid,
    capture_id: Uuid,
    canonical_url: &str,
) -> Result<Uuid, PublishError> {
    let derived = Uuid::new_v5(
        &IDENTITY_NAMESPACE,
        format!("{owner}\0{canonical_url}").as_bytes(),
    );
    sqlx::query_scalar(
        "insert into threads_archive.social_sources (social_source_id, user_ref, post_id, first_capture_id) \
         values ($1, $2, $3, $4) on conflict (user_ref, post_id) do update \
         set post_id = excluded.post_id returning social_source_id",
    )
    .bind(derived)
    .bind(owner)
    .bind(post_id)
    .bind(capture_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(PublishError::Persistence)
}

fn acquisition_method(value: &str, capture_id: Uuid) -> Result<AcquisitionMethod, PublishError> {
    match value {
        "share_extension" => Ok(AcquisitionMethod::ShareExtension),
        "browser_extension" => Ok(AcquisitionMethod::BrowserExtension),
        other => Err(PublishError::ContractViolation {
            capture_id,
            reason: format!("unsupported explicit-capture acquisition {other}"),
        }),
    }
}

fn availability(value: &str, capture_id: Uuid) -> Result<UpstreamAvailability, PublishError> {
    match value {
        "active" => Ok(UpstreamAvailability::Available),
        "deleted" => Ok(UpstreamAvailability::DeletedUpstream),
        "private_or_inaccessible"
        | "author_unavailable"
        | "temporarily_unavailable"
        | "unknown" => Ok(UpstreamAvailability::Unavailable),
        other => Err(PublishError::ContractViolation {
            capture_id,
            reason: format!("unsupported upstream availability {other}"),
        }),
    }
}

fn digest_from_bytes(bytes: &[u8], capture_id: Uuid) -> Result<ContentDigest, PublishError> {
    let rendered = hex(bytes);
    Ok(ContentDigest {
        algorithm: DigestAlgorithm::Sha256,
        hex: DigestHex::parse(&rendered).map_err(|error| violation(capture_id, error))?,
    })
}

fn content_digest(
    text: Option<&String>,
    relations: &[SocialRelation],
    availability: &str,
    capture_id: Uuid,
) -> Result<ContentDigest, PublishError> {
    let material = serde_json::to_vec(&serde_json::json!({
        "text": text,
        "relations": relations,
        "availability": availability,
    }))?;
    digest_from_bytes(&Sha256::digest(material), capture_id)
}

fn timestamp(value: DateTime<Utc>, capture_id: Uuid) -> Result<WireTimestamp, PublishError> {
    WireTimestamp::parse(&value.to_rfc3339_opts(SecondsFormat::Secs, true))
        .map_err(|error| violation(capture_id, error))
}

fn violation(capture_id: Uuid, error: impl std::fmt::Display) -> PublishError {
    PublishError::ContractViolation {
        capture_id,
        reason: error.to_string(),
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(result, "{byte:02x}");
    }
    result
}
