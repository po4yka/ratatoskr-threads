//! Scheduled synchronization of official own-account content.

use crate::Database;
use crate::oauth::CapabilityAvailability;
use crate::permalink::Permalink;
use crate::public_resolution::{PublicResolutionError, RawObjectStore};
use crate::publishing;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// The observable result of one requested own-account synchronization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// The account cannot use own-content synchronization at this time.
    NoOp(String),
    /// A complete bounded scan was durably observed.
    Completed {
        /// The checkpoint that a later scan will use, when the provider supplied one.
        next_watermark: Option<String>,
    },
}

/// One post or reply observed through the official authenticated surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialOwnPost {
    /// Stable Threads provider identity.
    pub provider_post_id: String,
    /// Canonical permalink exposed by the provider.
    pub permalink: Permalink,
    /// Optional official text body.
    pub text_content: Option<String>,
    /// Provider publication timestamp.
    pub published_at: Option<DateTime<Utc>>,
    /// Stable parent identity when this item is a reply.
    pub reply_to_provider_post_id: Option<String>,
}

/// One completed bounded official listing page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialOwnContentPage {
    /// Immutable raw provider response bytes retained before normalization.
    pub raw_response: Vec<u8>,
    /// Normalized own posts and replies from this page.
    pub posts: Vec<OfficialOwnPost>,
    /// Opaque continuation checkpoint supplied after a completed page.
    pub next_watermark: Option<String>,
}

/// A bounded official own-content listing adapter.
pub trait OfficialOwnContentProvider: Send + Sync {
    /// Lists the next bounded page of the connected account's own content.
    fn list_own_content(
        &self,
        account_id: uuid::Uuid,
        watermark: Option<&str>,
    ) -> impl Future<Output = Result<OfficialOwnContentPage, OwnAccountSyncError>> + Send;
}

/// An own-content synchronization failure.
#[derive(Debug, thiserror::Error)]
pub enum OwnAccountSyncError {
    /// The synchronization behavior has not yet been implemented.
    #[error("own-account synchronization is unavailable")]
    Unavailable,
    /// Durable own-account synchronization failed.
    #[error("own-account synchronization persistence failed")]
    Persistence(#[source] sqlx::Error),
    /// Immutable raw observation storage failed.
    #[error(transparent)]
    Raw(#[from] PublicResolutionError),
}

/// Durable own-account synchronization over the service-owned archive database.
#[derive(Debug, Clone)]
pub struct OwnAccountSyncStore {
    database: Database,
    raw_objects: RawObjectStore,
}

impl OwnAccountSyncStore {
    /// Creates own-account synchronization storage.
    #[must_use]
    pub fn new(database: Database, raw_objects: RawObjectStore) -> Self {
        Self {
            database,
            raw_objects,
        }
    }

    /// Performs one requested bounded scan.
    ///
    /// # Errors
    ///
    /// Returns an error when the official adapter, raw evidence storage, or
    /// the single durable page transaction fails.
    pub async fn sync<P>(
        &self,
        provider: &P,
        account_id: Uuid,
        availability: &CapabilityAvailability,
    ) -> Result<SyncOutcome, OwnAccountSyncError>
    where
        P: OfficialOwnContentProvider,
    {
        if let CapabilityAvailability::Unavailable(reason) = availability {
            return Ok(SyncOutcome::NoOp(reason.clone()));
        }
        let watermark = checkpoint(self.database.pool(), account_id).await?;
        let page = provider
            .list_own_content(account_id, watermark.as_deref())
            .await?;
        let raw = self.raw_objects.store(&page.raw_response).await?;
        let mut transaction = self
            .database
            .pool()
            .begin()
            .await
            .map_err(OwnAccountSyncError::Persistence)?;
        let raw_object_id = record_raw(&mut transaction, raw).await?;
        for post in &page.posts {
            record_post(&mut transaction, account_id, post, raw_object_id).await?;
        }
        save_checkpoint(&mut transaction, account_id, page.next_watermark.as_ref()).await?;
        transaction
            .commit()
            .await
            .map_err(OwnAccountSyncError::Persistence)?;
        Ok(SyncOutcome::Completed {
            next_watermark: page.next_watermark,
        })
    }
}

async fn checkpoint(
    pool: &sqlx::PgPool,
    account_id: Uuid,
) -> Result<Option<String>, OwnAccountSyncError> {
    sqlx::query_scalar(
        "select watermark from threads_archive.account_sync_checkpoints where account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .map_err(OwnAccountSyncError::Persistence)
    .map(Option::flatten)
}

async fn record_raw(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    raw: crate::public_resolution::StoredRaw,
) -> Result<Uuid, OwnAccountSyncError> {
    let raw_object_id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.raw_objects \
         (raw_object_id, object_kind, blob_ref, content_hash, byte_size, media_type, observed_at) \
         values ($1, 'api_response', $2, $3, $4, $5, now())",
    )
    .bind(raw_object_id)
    .bind(raw.blob_ref)
    .bind(raw.content_hash)
    .bind(raw.byte_size)
    .bind(raw.media_type)
    .execute(&mut **transaction)
    .await
    .map_err(OwnAccountSyncError::Persistence)?;
    Ok(raw_object_id)
}

async fn record_post(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    post: &OfficialOwnPost,
    raw_object_id: Uuid,
) -> Result<(), OwnAccountSyncError> {
    let post_kind = if post.reply_to_provider_post_id.is_some() {
        "reply"
    } else {
        "post"
    };
    let post_id: Uuid = sqlx::query_scalar(
        "insert into threads_archive.posts \
         (post_id, account_id, provider_post_id, permalink, post_kind, text_content, published_at, acquisition_method, saved_authority, upstream_status) \
         values ($1, $2, $3, $4, $5, $6, $7, 'official_api', 'authoritative_platform_state', 'active') \
         on conflict (provider_post_id) do update set \
         account_id = excluded.account_id, permalink = excluded.permalink, post_kind = excluded.post_kind, \
         text_content = excluded.text_content, published_at = excluded.published_at, \
         acquisition_method = excluded.acquisition_method, saved_authority = excluded.saved_authority, \
         upstream_status = excluded.upstream_status, updated_at = now() returning post_id",
    )
    .bind(Uuid::now_v7())
    .bind(account_id)
    .bind(&post.provider_post_id)
    .bind(post.permalink.as_str())
    .bind(post_kind)
    .bind(&post.text_content)
    .bind(post.published_at)
    .fetch_one(&mut **transaction)
    .await
    .map_err(OwnAccountSyncError::Persistence)?;
    sqlx::query(
        "insert into threads_archive.post_revisions \
         (revision_id, post_id, raw_object_id, parser_version, observed_at) \
         values ($1, $2, $3, 'threads-official-own-content-v1', now())",
    )
    .bind(Uuid::now_v7())
    .bind(post_id)
    .bind(raw_object_id)
    .execute(&mut **transaction)
    .await
    .map_err(OwnAccountSyncError::Persistence)?;
    record_reply_relation(transaction, post_id, post).await?;
    append_source_facts(transaction, account_id, post_id).await
}

async fn record_reply_relation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    post_id: Uuid,
    post: &OfficialOwnPost,
) -> Result<(), OwnAccountSyncError> {
    let Some(parent) = &post.reply_to_provider_post_id else {
        return Ok(());
    };
    let parent_post_id: Option<Uuid> =
        sqlx::query_scalar("select post_id from threads_archive.posts where provider_post_id = $1")
            .bind(parent)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(OwnAccountSyncError::Persistence)?;
    sqlx::query(
        "insert into threads_archive.post_relations \
         (relation_id, referencing_post_id, target_post_id, target_provider_post_id, target_permalink, relation_kind) \
         values ($1, $2, $3, $4, null, 'reply') \
         on conflict (referencing_post_id, target_provider_post_id, relation_kind) do update set \
         target_post_id = excluded.target_post_id",
    )
    .bind(Uuid::now_v7())
    .bind(post_id)
    .bind(parent_post_id)
    .bind(parent)
    .execute(&mut **transaction)
    .await
    .map_err(OwnAccountSyncError::Persistence)?;
    Ok(())
}

async fn append_source_facts(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    post_id: Uuid,
) -> Result<(), OwnAccountSyncError> {
    let capture_ids: Vec<Uuid> =
        sqlx::query_scalar("select capture_id from threads_archive.captures where post_id = $1")
            .bind(post_id)
            .fetch_all(&mut **transaction)
            .await
            .map_err(OwnAccountSyncError::Persistence)?;
    if capture_ids.is_empty() {
        publishing::append_official_fact(transaction, account_id, post_id)
            .await
            .map_err(|error| publish_error(&error))?;
        return Ok(());
    }
    for capture_id in capture_ids {
        publishing::append_fact(transaction, capture_id)
            .await
            .map_err(|error| publish_error(&error))?;
    }
    Ok(())
}

async fn save_checkpoint(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    watermark: Option<&String>,
) -> Result<(), OwnAccountSyncError> {
    sqlx::query(
        "insert into threads_archive.account_sync_checkpoints (account_id, watermark, updated_at) \
         values ($1, $2, now()) on conflict (account_id) do update set \
         watermark = excluded.watermark, updated_at = now()",
    )
    .bind(account_id)
    .bind(watermark)
    .execute(&mut **transaction)
    .await
    .map_err(OwnAccountSyncError::Persistence)?;
    Ok(())
}

fn publish_error(error: &publishing::PublishError) -> OwnAccountSyncError {
    OwnAccountSyncError::Persistence(sqlx::Error::Protocol(error.to_string()))
}
