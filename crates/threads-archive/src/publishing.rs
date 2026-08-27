//! Transactional publication of Threads social-source facts.

use chrono::{DateTime, SecondsFormat, Utc};
use ratatoskr_event_envelope::{EventEnvelope, EventPayload};
use ratatoskr_identifiers::{
    BlobOwner, BlobRef, ContentDigest, DigestAlgorithm, DigestHex, EntityLocalId, Extensions,
    MediaType, SocialSourceId, TenantRef, WireTimestamp,
};
use ratatoskr_social_contracts::{
    AcquisitionMethod, CaptureCompleteness, Platform, PostPermalink, PostText, RemovalReason,
    SavedAuthority, SocialRelation, SocialRelationKind, SocialSourceCaptured, SocialSourceRemoved,
    SocialSourceSnapshot, SocialSourceUpdated, UpstreamAvailability,
};
use sha2::{Digest as _, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

const PLATFORM: &str = "threads";
const PRODUCER: &str = "ratatoskr-threads";
const IDENTITY_NAMESPACE: Uuid = Uuid::from_bytes([
    0x72, 0x61, 0x74, 0x61, 0x73, 0x6b, 0x72, 0x54, 0x68, 0x72, 0x65, 0x61, 0x64, 0x73, 0x53, 0x72,
]);

#[derive(Debug, Clone)]
struct SourceOrigin {
    owner: Uuid,
    post_id: Uuid,
    captured_at: DateTime<Utc>,
    capture_id: Option<Uuid>,
    capture_acquisition: Option<String>,
    operation_id: Uuid,
}

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
    let origin = load_capture_origin(connection, capture_id).await?;
    append_origin_fact(connection, origin).await
}

/// Appends the first fact or changed fact for one official account observation.
pub(crate) async fn append_official_fact(
    connection: &mut PgConnection,
    account_id: Uuid,
    post_id: Uuid,
) -> Result<(), PublishError> {
    let owner: Uuid =
        sqlx::query_scalar("select user_ref from threads_archive.accounts where account_id = $1")
            .bind(account_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(PublishError::NothingToPublish(post_id))?;
    let captured_at: DateTime<Utc> =
        sqlx::query_scalar("select updated_at from threads_archive.posts where post_id = $1")
            .bind(post_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(PublishError::NothingToPublish(post_id))?;
    append_origin_fact(
        connection,
        SourceOrigin {
            owner,
            post_id,
            captured_at,
            capture_id: None,
            capture_acquisition: None,
            operation_id: post_id,
        },
    )
    .await
}

/// Appends a source fact observed in an owner-authorized Data Export.
pub(crate) async fn append_export_fact(
    connection: &mut PgConnection,
    owner: Uuid,
    post_id: Uuid,
) -> Result<(), PublishError> {
    let captured_at: DateTime<Utc> =
        sqlx::query_scalar("select updated_at from threads_archive.posts where post_id = $1")
            .bind(post_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(PublishError::NothingToPublish(post_id))?;
    append_origin_fact(
        connection,
        SourceOrigin {
            owner,
            post_id,
            captured_at,
            capture_id: None,
            capture_acquisition: None,
            operation_id: post_id,
        },
    )
    .await
}

/// Appends one content-free local-library removal fact inside the caller's transaction.
pub(crate) async fn append_removal(
    connection: &mut PgConnection,
    owner: Uuid,
    social_source_id: Uuid,
    operation_id: Uuid,
    aggregate_type: &'static str,
    aggregate_id: Uuid,
) -> Result<(), PublishError> {
    let payload = SocialSourceRemoved {
        social_source_id: SocialSourceId::parse(&social_source_id.to_string())
            .map_err(|error| violation(operation_id, error))?,
        owner: TenantRef::parse(&format!("user:{owner}"))
            .map_err(|error| violation(operation_id, error))?,
        reason: RemovalReason::UserRequested,
        removed_at: WireTimestamp::now(),
        extensions: Extensions::default(),
    };
    let event_id = Uuid::now_v7();
    let template = serde_json::json!({
        "event_id": event_id.to_string(),
        "event_type": SocialSourceRemoved::EVENT_TYPE,
        "occurred_at": WireTimestamp::now().to_wire(),
        "producer": PRODUCER,
        "aggregate_id": format!("social_source:{social_source_id}"),
        "correlation_id": format!("deletion:{operation_id}"),
        "tenant_id": format!("user:{owner}"),
        "schema_version": 1,
        "payload": {}
    });
    let mut envelope = EventEnvelope::from_json(serde_json::to_vec(&template)?.as_slice())
        .map_err(|error| violation(operation_id, error))?;
    envelope
        .set_payload(&payload)
        .map_err(|error| violation(operation_id, error))?;
    let rendered = envelope
        .to_canonical_json()
        .map_err(|error| violation(operation_id, error))?;
    let envelope: serde_json::Value = serde_json::from_str(&rendered)?;
    sqlx::query(
        "insert into threads_archive.outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, correlation_id, \
          causation_id, occurred_at) values ($1, $2, $3, $4, $5, $6, null, now())",
    )
    .bind(event_id)
    .bind(SocialSourceRemoved::EVENT_TYPE)
    .bind(aggregate_type)
    .bind(aggregate_id)
    .bind(envelope)
    .bind(operation_id)
    .execute(connection)
    .await?;
    Ok(())
}

async fn append_origin_fact(
    connection: &mut PgConnection,
    origin: SourceOrigin,
) -> Result<(), PublishError> {
    let snapshot = build_snapshot(connection, origin.clone()).await?;
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
            .map_err(|error| violation(origin.operation_id, error))?,
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
                &origin,
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
                &origin,
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
            .map_err(|error| violation(origin.operation_id, error))?,
    )
    .bind(digest.hex.to_string())
    .bind(snapshot_value)
    .execute(&mut *connection)
    .await?;
    let aggregate_type = if origin.capture_id.is_some() {
        "capture"
    } else {
        "post"
    };
    sqlx::query(
        "insert into threads_archive.outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, correlation_id, causation_id, occurred_at) \
         values ($1, $2, $3, $4, $5, $6, null, now())",
    )
    .bind(event_id)
    .bind(event_type)
    .bind(aggregate_type)
    .bind(origin.operation_id)
    .bind(envelope)
    .bind(origin.operation_id)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

fn envelope_value<P>(
    event_id: Uuid,
    payload: &P,
    source_id: &str,
    owner: &str,
    origin: &SourceOrigin,
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
        "correlation_id": if origin.capture_id.is_some() {
            format!("capture:{}", origin.operation_id)
        } else {
            format!("post:{}", origin.operation_id)
        },
        "tenant_id": format!("user:{owner}"),
        "schema_version": 1,
        "payload": {}
    });
    let mut envelope = EventEnvelope::from_json(serde_json::to_vec(&template)?.as_slice())
        .map_err(|error| violation(origin.operation_id, error))?;
    envelope
        .set_payload(payload)
        .map_err(|error| violation(origin.operation_id, error))?;
    let rendered = envelope
        .to_canonical_json()
        .map_err(|error| violation(origin.operation_id, error))?;
    Ok(serde_json::from_str(&rendered)?)
}

async fn build_snapshot(
    connection: &mut PgConnection,
    origin: SourceOrigin,
) -> Result<SocialSourceSnapshot, PublishError> {
    let (
        acquisition,
        stored_authority,
        provider_post_id,
        permalink,
        text,
        published_at,
        upstream_status,
    ) = load_post(connection, origin.post_id, origin.operation_id).await?;
    let social_source_id = ensure_source(
        connection,
        origin.owner,
        origin.post_id,
        origin.capture_id,
        &permalink,
    )
    .await?;
    let (snapshot_acquisition, snapshot_authority) = match &origin.capture_acquisition {
        Some(capture_acquisition) => (capture_acquisition.as_str(), "explicit_user_capture"),
        None => (acquisition.as_str(), stored_authority.as_str()),
    };
    let (hash, length, media_type) =
        load_raw(connection, origin.post_id, origin.operation_id).await?;
    let relations = load_relations(connection, origin.post_id, origin.operation_id).await?;
    let raw_digest = digest_from_bytes(&hash, origin.operation_id)?;
    let content_digest = content_digest(
        text.as_ref(),
        &relations,
        &upstream_status,
        snapshot_acquisition,
        snapshot_authority,
        origin.operation_id,
    )?;
    Ok(SocialSourceSnapshot {
        social_source_id: SocialSourceId::parse(&social_source_id.to_string())
            .map_err(|error| violation(origin.operation_id, error))?,
        platform: Platform::parse(PLATFORM)
            .map_err(|error| violation(origin.operation_id, error))?,
        external_post_id: EntityLocalId::parse(&provider_post_id)
            .map_err(|error| violation(origin.operation_id, error))?,
        permalink: Some(
            PostPermalink::parse(&permalink)
                .map_err(|error| violation(origin.operation_id, error))?,
        ),
        owner: TenantRef::parse(&format!("user:{}", origin.owner))
            .map_err(|error| violation(origin.operation_id, error))?,
        author: None,
        published_at: published_at
            .map(|value| timestamp(value, origin.operation_id))
            .transpose()?,
        captured_at: timestamp(origin.captured_at, origin.operation_id)?,
        text: text
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|value| {
                PostText::parse(value).map_err(|error| violation(origin.operation_id, error))
            })
            .transpose()?,
        media: Vec::new(),
        relations,
        folders: Vec::new(),
        content_digest,
        raw_blob: Some(BlobRef {
            owner_service: BlobOwner::parse(PRODUCER)
                .map_err(|error| violation(origin.operation_id, error))?,
            digest: raw_digest,
            media_type: MediaType::parse(&media_type)
                .map_err(|error| violation(origin.operation_id, error))?,
            length_bytes: u64::try_from(length)
                .map_err(|error| violation(origin.operation_id, error))?,
        }),
        acquisition: acquisition_method(snapshot_acquisition, origin.operation_id)?,
        saved_authority: saved_authority(snapshot_authority, origin.operation_id)?,
        completeness: CaptureCompleteness::Complete,
        upstream_availability: availability(&upstream_status, origin.operation_id)?,
        checkpoint: None,
        warnings: Vec::new(),
        extensions: Extensions::default(),
    })
}

async fn load_capture_origin(
    connection: &mut PgConnection,
    capture_id: Uuid,
) -> Result<SourceOrigin, PublishError> {
    sqlx::query_as(
        "select c.user_ref, c.post_id, c.captured_at, c.acquisition_method \
         from threads_archive.captures c join threads_archive.posts p on p.post_id = c.post_id \
         where c.capture_id = $1",
    )
    .bind(capture_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PublishError::NothingToPublish(capture_id))
    .map(
        |(owner, post_id, captured_at, capture_acquisition)| SourceOrigin {
            owner,
            post_id,
            captured_at,
            capture_id: Some(capture_id),
            capture_acquisition: Some(capture_acquisition),
            operation_id: capture_id,
        },
    )
}

type StoredPost = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<DateTime<Utc>>,
    String,
);

async fn load_post(
    connection: &mut PgConnection,
    post_id: Uuid,
    operation_id: Uuid,
) -> Result<StoredPost, PublishError> {
    sqlx::query_as(
        "select acquisition_method, saved_authority, provider_post_id, permalink, text_content, published_at, upstream_status \
         from threads_archive.posts where post_id = $1",
    )
    .bind(post_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(PublishError::NothingToPublish(operation_id))
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
    capture_id: Option<Uuid>,
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
        "official_api" => Ok(AcquisitionMethod::OfficialApi),
        "data_export" => Ok(AcquisitionMethod::DataExport),
        "share_extension" => Ok(AcquisitionMethod::ShareExtension),
        "browser_extension" => Ok(AcquisitionMethod::BrowserExtension),
        "public_resolution" => Ok(AcquisitionMethod::PublicResolution),
        other => Err(PublishError::ContractViolation {
            capture_id,
            reason: format!("unsupported explicit-capture acquisition {other}"),
        }),
    }
}

fn saved_authority(value: &str, operation_id: Uuid) -> Result<SavedAuthority, PublishError> {
    match value {
        "authoritative_platform_state" => Ok(SavedAuthority::AuthoritativePlatformState),
        "explicit_user_capture" => Ok(SavedAuthority::ExplicitUserCapture),
        "export_observation" => Ok(SavedAuthority::ExportObservation),
        "legacy_observation" => Ok(SavedAuthority::LegacyObservation),
        other => Err(PublishError::ContractViolation {
            capture_id: operation_id,
            reason: format!("unsupported saved authority {other}"),
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
    acquisition: &str,
    saved_authority: &str,
    operation_id: Uuid,
) -> Result<ContentDigest, PublishError> {
    let material = serde_json::to_vec(&serde_json::json!({
        "text": text,
        "relations": relations,
        "availability": availability,
        "acquisition": acquisition,
        "saved_authority": saved_authority,
    }))?;
    digest_from_bytes(&Sha256::digest(material), operation_id)
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
