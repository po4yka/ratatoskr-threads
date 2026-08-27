//! Closed inventory used to prove owner deletion remains complete as storage evolves.

mod inventory;

use crate::{Database, PersistenceError};
use uuid::Uuid;

pub use inventory::{
    CAPTURE_DELETION_CLASSIFICATIONS, CONNECTION_DELETION_CLASSIFICATIONS, OWNED_DATA_CLASSES,
};

/// One owner-scoped local deletion target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionTarget {
    /// One explicit capture and its capture-specific intent.
    Capture(Uuid),
    /// One official Threads account connection.
    Connection(Uuid),
}

/// Stable replay identity and owner authorization for a deletion request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeletionRequest {
    /// Caller-supplied stable idempotency identity.
    pub operation_id: Uuid,
    /// Internal owner identity authenticated by the caller.
    pub user_ref: Uuid,
    /// Capture or official connection to remove locally.
    pub target: DeletionTarget,
}

/// Terminal state of an owner deletion operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletionResult {
    /// Stable request identity.
    pub operation_id: Uuid,
    /// Deterministic content-free per-class effects applied by this operation.
    pub effects: Vec<DeletionEffectCount>,
}

/// Bounded count for one classified owned data class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeletionEffectCount {
    /// The classified storage class.
    pub class: OwnedDataClass,
    /// Planned target-specific action.
    pub action: DeletionAction,
    /// Rows or blob references affected, never content.
    pub affected_count: i64,
}

/// Deterministic, content-free deletion preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletionPlan {
    /// Stable operation identity used by apply.
    pub operation_id: Uuid,
    /// Classified per-class effects in inventory order.
    pub effects: Vec<DeletionEffectCount>,
}

/// Owner deletion refusal or persistence failure.
#[derive(Debug, thiserror::Error)]
pub enum PrivacyDeletionError {
    /// The target is absent or does not belong to the authenticated owner.
    #[error("the deletion target was not found for this owner")]
    TargetNotFound,
    /// Owned persistence failed without exposing content or credentials.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    /// A content-free removal fact could not be constructed or appended.
    #[error("the local source removal fact could not be published")]
    Publication,
    /// An operation id was already bound to a different owner or target.
    #[error("the deletion operation identity is already bound to another request")]
    OperationConflict,
}

/// Owner-bound deletion application service.
#[derive(Debug)]
pub struct DeletionStore<'a> {
    database: &'a Database,
}

impl<'a> DeletionStore<'a> {
    /// Creates a deletion store over the Threads-owned database.
    #[must_use]
    pub const fn new(database: &'a Database) -> Self {
        Self { database }
    }

    /// Computes a deterministic deletion plan without durable mutation.
    ///
    /// # Errors
    ///
    /// Returns a typed owner refusal or persistence failure.
    pub async fn preview(
        &self,
        request: DeletionRequest,
    ) -> Result<DeletionPlan, PrivacyDeletionError> {
        let mut transaction = self
            .database
            .pool()
            .begin()
            .await
            .map_err(PersistenceError::Query)?;
        require_owned_locked(&mut transaction, request).await?;
        let plan = build_plan(&mut transaction, request).await?;
        transaction
            .rollback()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(plan)
    }

    /// Applies one stable owner deletion request.
    ///
    /// # Errors
    ///
    /// Returns a typed owner refusal or persistence failure.
    pub async fn apply(
        &self,
        request: DeletionRequest,
    ) -> Result<DeletionResult, PrivacyDeletionError> {
        if let Some(result) = load_completed(self.database, request).await? {
            return Ok(result);
        }
        let mut transaction = self
            .database
            .pool()
            .begin()
            .await
            .map_err(PersistenceError::Query)?;
        require_owned_locked(&mut transaction, request).await?;
        let plan = build_plan(&mut transaction, request).await?;
        let (target_kind, target_id) = target_parts(request.target);
        sqlx::query(
            "insert into threads_archive.deletion_operations \
             (operation_id, user_ref, target_kind, target_id, reason, state, requested_at, \
              finished_at) values ($1, $2, $3, $4, 'user_requested', 'complete', now(), now())",
        )
        .bind(request.operation_id)
        .bind(request.user_ref)
        .bind(target_kind)
        .bind(target_id)
        .execute(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;
        for effect in &plan.effects {
            sqlx::query(
                "insert into threads_archive.deletion_effects \
                 (operation_id, data_class, action, affected_count) values ($1, $2, $3, $4)",
            )
            .bind(request.operation_id)
            .bind(effect.class.audit_key())
            .bind(effect.action.as_str())
            .bind(effect.affected_count)
            .execute(&mut *transaction)
            .await
            .map_err(PersistenceError::Query)?;
        }
        apply_target_rows(&mut transaction, request).await?;
        transaction
            .commit()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(DeletionResult {
            operation_id: request.operation_id,
            effects: plan.effects,
        })
    }
}

async fn load_completed(
    database: &Database,
    request: DeletionRequest,
) -> Result<Option<DeletionResult>, PrivacyDeletionError> {
    let existing: Option<(Uuid, String, Uuid, String)> = sqlx::query_as(
        "select user_ref, target_kind, target_id, state \
         from threads_archive.deletion_operations where operation_id = $1",
    )
    .bind(request.operation_id)
    .fetch_optional(database.pool())
    .await
    .map_err(PersistenceError::Query)?;
    let Some((user_ref, target_kind, target_id, state)) = existing else {
        return Ok(None);
    };
    let (expected_kind, expected_id) = target_parts(request.target);
    if user_ref != request.user_ref || target_kind != expected_kind || target_id != expected_id {
        return Err(PrivacyDeletionError::OperationConflict);
    }
    if state != "complete" {
        return Ok(None);
    }
    let stored: Vec<(String, String, i64)> = sqlx::query_as(
        "select data_class, action, affected_count from threads_archive.deletion_effects \
         where operation_id = $1 order by data_class, action",
    )
    .bind(request.operation_id)
    .fetch_all(database.pool())
    .await
    .map_err(PersistenceError::Query)?;
    let mut effects = Vec::with_capacity(stored.len());
    for (class, action, affected_count) in stored {
        let class = OwnedDataClass::from_audit_key(&class)
            .ok_or(PrivacyDeletionError::OperationConflict)?;
        let action =
            DeletionAction::from_str(&action).ok_or(PrivacyDeletionError::OperationConflict)?;
        effects.push(DeletionEffectCount {
            class,
            action,
            affected_count,
        });
    }
    effects.sort_by_key(|effect| effect.class);
    Ok(Some(DeletionResult {
        operation_id: request.operation_id,
        effects,
    }))
}

fn target_parts(target: DeletionTarget) -> (&'static str, Uuid) {
    match target {
        DeletionTarget::Capture(id) => ("capture", id),
        DeletionTarget::Connection(id) => ("connection", id),
    }
}

async fn require_owned_locked(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: DeletionRequest,
) -> Result<(), PrivacyDeletionError> {
    let owner: Option<Uuid> = match request.target {
        DeletionTarget::Capture(capture_id) => sqlx::query_scalar(
            "select user_ref from threads_archive.captures where capture_id = $1 for update",
        )
        .bind(capture_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?,
        DeletionTarget::Connection(account_id) => sqlx::query_scalar(
            "select user_ref from threads_archive.accounts where account_id = $1 for update",
        )
        .bind(account_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?,
    };
    if owner != Some(request.user_ref) {
        return Err(PrivacyDeletionError::TargetNotFound);
    }
    Ok(())
}

async fn build_plan(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: DeletionRequest,
) -> Result<DeletionPlan, PrivacyDeletionError> {
    let classifications = match request.target {
        DeletionTarget::Capture(_) => CAPTURE_DELETION_CLASSIFICATIONS,
        DeletionTarget::Connection(_) => CONNECTION_DELETION_CLASSIFICATIONS,
    };
    let mut effects = classifications
        .iter()
        .map(|entry| DeletionEffectCount {
            class: entry.class,
            action: entry.action,
            affected_count: 0,
        })
        .collect::<Vec<_>>();
    match request.target {
        DeletionTarget::Capture(capture_id) => {
            set_count(&mut effects, OwnedDataClass::Captures, 1);
            let (post_id, resolutions, reresolution_items, tombstones): (
                Option<Uuid>,
                i64,
                i64,
                i64,
            ) = sqlx::query_as(
                "select \
                   (select post_id from threads_archive.captures where capture_id = $1), \
                   (select count(*) from threads_archive.capture_resolutions where capture_id = $1), \
                   (select count(*) from threads_archive.reresolution_items where capture_id = $1), \
                   (select count(*) from threads_archive.tombstones where capture_id = $1)",
            )
            .bind(capture_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(PersistenceError::Query)?;
            set_count(
                &mut effects,
                OwnedDataClass::CaptureResolutions,
                resolutions,
            );
            set_count(
                &mut effects,
                OwnedDataClass::ReresolutionItems,
                reresolution_items,
            );
            set_count(&mut effects, OwnedDataClass::Tombstones, tombstones);
            if let Some(post_id) = post_id {
                let (other_captures, posts, revisions, raw_objects, media, relations, sources, source_revisions, analysis_links): (i64, i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
                    "select \
                       (select count(*) from threads_archive.captures \
                        where user_ref = $2 and post_id = $3 and capture_id <> $1), \
                       (select count(*) from threads_archive.posts where post_id = $3), \
                       (select count(*) from threads_archive.post_revisions where post_id = $3), \
                       (select count(distinct raw.raw_object_id) from threads_archive.raw_objects raw \
                        join threads_archive.post_revisions rev on rev.raw_object_id = raw.raw_object_id \
                        where rev.post_id = $3), \
                       (select count(*) from threads_archive.media where post_id = $3), \
                       (select count(*) from threads_archive.post_relations \
                        where referencing_post_id = $3 or target_post_id = $3), \
                       (select count(*) from threads_archive.social_sources \
                        where user_ref = $2 and post_id = $3), \
                       (select count(*) from threads_archive.social_source_revisions revision \
                        join threads_archive.social_sources source \
                          on source.social_source_id = revision.social_source_id \
                        where source.user_ref = $2 and source.post_id = $3), \
                       (select count(*) from threads_archive.social_analysis_links link \
                        join threads_archive.social_sources source \
                          on source.social_source_id = link.social_source_id \
                        where source.user_ref = $2 and source.post_id = $3)",
                )
                .bind(capture_id)
                .bind(request.user_ref)
                .bind(post_id)
                .fetch_one(&mut **transaction)
                .await
                .map_err(PersistenceError::Query)?;
                if other_captures > 0 {
                    for (class, count) in [
                        (OwnedDataClass::Posts, posts),
                        (OwnedDataClass::PostRevisions, revisions),
                        (OwnedDataClass::RawObjects, raw_objects),
                        (OwnedDataClass::Media, media),
                        (OwnedDataClass::PostRelations, relations),
                        (OwnedDataClass::SocialSources, sources),
                        (OwnedDataClass::SocialSourceRevisions, source_revisions),
                        (OwnedDataClass::SocialAnalysisLinks, analysis_links),
                        (OwnedDataClass::RawObjectBlob, raw_objects),
                        (OwnedDataClass::MediaBlob, media),
                    ] {
                        set_effect(&mut effects, class, DeletionAction::RetainShared, count);
                    }
                }
            }
        }
        DeletionTarget::Connection(account_id) => {
            plan_connection(transaction, &mut effects, account_id).await?;
        }
    }
    Ok(DeletionPlan {
        operation_id: request.operation_id,
        effects,
    })
}

async fn plan_connection(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    effects: &mut [DeletionEffectCount],
    account_id: Uuid,
) -> Result<(), PrivacyDeletionError> {
    let (accounts, budgets, checkpoints, credentials, audit, posts, shared_posts): (
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
    ) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.accounts where account_id = $1), \
           (select count(*) from threads_archive.account_budgets where account_id = $1), \
           (select count(*) from threads_archive.account_sync_checkpoints where account_id = $1), \
           (select count(*) from threads_archive.credentials where account_id = $1), \
           (select count(*) from threads_archive.credential_audit where account_id = $1), \
           (select count(*) from threads_archive.posts where account_id = $1), \
           (select count(*) from threads_archive.posts post where post.account_id = $1 \
            and exists (select 1 from threads_archive.captures capture \
              where capture.post_id = post.post_id))",
    )
    .bind(account_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    for (class, count) in [
        (OwnedDataClass::Accounts, accounts),
        (OwnedDataClass::AccountBudgets, budgets),
        (OwnedDataClass::AccountSyncCheckpoints, checkpoints),
        (OwnedDataClass::Credentials, credentials),
        (OwnedDataClass::CredentialAudit, audit),
        (OwnedDataClass::Posts, posts),
    ] {
        set_count(effects, class, count);
    }
    if shared_posts > 0 {
        set_effect(
            effects,
            OwnedDataClass::Posts,
            DeletionAction::RetainShared,
            shared_posts,
        );
    }
    Ok(())
}

fn set_count(effects: &mut [DeletionEffectCount], class: OwnedDataClass, count: i64) {
    if let Some(effect) = effects.iter_mut().find(|effect| effect.class == class) {
        effect.affected_count = count;
    }
}

fn set_effect(
    effects: &mut [DeletionEffectCount],
    class: OwnedDataClass,
    action: DeletionAction,
    count: i64,
) {
    if let Some(effect) = effects.iter_mut().find(|effect| effect.class == class) {
        effect.action = action;
        effect.affected_count = count;
    }
}

async fn apply_target_rows(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: DeletionRequest,
) -> Result<(), PrivacyDeletionError> {
    match request.target {
        DeletionTarget::Capture(capture_id) => {
            let post_id: Option<Uuid> = sqlx::query_scalar(
                "select post_id from threads_archive.captures where capture_id = $1",
            )
            .bind(capture_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(PersistenceError::Query)?;
            if let Some(post_id) = post_id {
                remove_final_owner_source(transaction, request, post_id).await?;
            }
            sqlx::query(
                "update threads_archive.social_sources source set first_capture_id = (\
                   select capture_id from threads_archive.captures candidate \
                   where candidate.user_ref = source.user_ref and candidate.post_id = source.post_id \
                     and candidate.capture_id <> $1 order by candidate.capture_id limit 1\
                 ) where source.first_capture_id = $1",
            )
            .bind(capture_id)
            .execute(&mut **transaction)
            .await
            .map_err(PersistenceError::Query)?;
            sqlx::query(
                "delete from threads_archive.tombstones \
                 where capture_id = $1 and post_id is null",
            )
            .bind(capture_id)
            .execute(&mut **transaction)
            .await
            .map_err(PersistenceError::Query)?;
            sqlx::query(
                "update threads_archive.tombstones set capture_id = null \
                 where capture_id = $1 and post_id is not null",
            )
            .bind(capture_id)
            .execute(&mut **transaction)
            .await
            .map_err(PersistenceError::Query)?;
            sqlx::query("delete from threads_archive.reresolution_items where capture_id = $1")
                .bind(capture_id)
                .execute(&mut **transaction)
                .await
                .map_err(PersistenceError::Query)?;
            sqlx::query("delete from threads_archive.capture_resolutions where capture_id = $1")
                .bind(capture_id)
                .execute(&mut **transaction)
                .await
                .map_err(PersistenceError::Query)?;
            sqlx::query("delete from threads_archive.captures where capture_id = $1")
                .bind(capture_id)
                .execute(&mut **transaction)
                .await
                .map_err(PersistenceError::Query)?;
            if let Some(post_id) = post_id {
                remove_unreferenced_post(transaction, request.operation_id, post_id).await?;
            }
        }
        DeletionTarget::Connection(account_id) => {
            let post_ids: Vec<Uuid> = sqlx::query_scalar(
                "select post_id from threads_archive.posts where account_id = $1 order by post_id",
            )
            .bind(account_id)
            .fetch_all(&mut **transaction)
            .await
            .map_err(PersistenceError::Query)?;
            for post_id in &post_ids {
                remove_final_owner_source(transaction, request, *post_id).await?;
            }
            sqlx::query("update threads_archive.posts set account_id = null where account_id = $1")
                .bind(account_id)
                .execute(&mut **transaction)
                .await
                .map_err(PersistenceError::Query)?;
            for statement in [
                "delete from threads_archive.credentials where account_id = $1",
                "delete from threads_archive.credential_audit where account_id = $1",
                "delete from threads_archive.account_budgets where account_id = $1",
                "delete from threads_archive.account_sync_checkpoints where account_id = $1",
                "delete from threads_archive.accounts where account_id = $1",
            ] {
                sqlx::query(statement)
                    .bind(account_id)
                    .execute(&mut **transaction)
                    .await
                    .map_err(PersistenceError::Query)?;
            }
            for post_id in post_ids {
                remove_unreferenced_post(transaction, request.operation_id, post_id).await?;
            }
        }
    }
    Ok(())
}

async fn remove_final_owner_source(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request: DeletionRequest,
    post_id: Uuid,
) -> Result<(), PrivacyDeletionError> {
    let excluded_account = match request.target {
        DeletionTarget::Connection(id) => Some(id),
        DeletionTarget::Capture(_) => None,
    };
    let (other_captures, official_holding): (i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.captures \
            where user_ref = $1 and post_id = $2 and capture_id <> $3), \
           (select count(*) from threads_archive.posts post \
            join threads_archive.accounts account on account.account_id = post.account_id \
            where post.post_id = $2 and account.user_ref = $1 \
              and ($4::uuid is null or account.account_id <> $4))",
    )
    .bind(request.user_ref)
    .bind(post_id)
    .bind(match request.target {
        DeletionTarget::Capture(id) => id,
        DeletionTarget::Connection(_) => Uuid::nil(),
    })
    .bind(excluded_account)
    .fetch_one(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    if other_captures > 0 || official_holding > 0 {
        return Ok(());
    }
    let source_ids: Vec<Uuid> = sqlx::query_scalar(
        "select social_source_id from threads_archive.social_sources \
         where user_ref = $1 and post_id = $2 order by social_source_id",
    )
    .bind(request.user_ref)
    .bind(post_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    for source_id in source_ids {
        sqlx::query(
            "delete from threads_archive.outbox_events \
             where aggregate_id in ($1, $2) and event_type <> 'social.source.removed.v1'",
        )
        .bind(match request.target {
            DeletionTarget::Capture(id) | DeletionTarget::Connection(id) => id,
        })
        .bind(post_id)
        .execute(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
        crate::publishing::append_removal(
            transaction,
            request.user_ref,
            source_id,
            request.operation_id,
            match request.target {
                DeletionTarget::Capture(_) => "capture",
                DeletionTarget::Connection(_) => "account",
            },
            match request.target {
                DeletionTarget::Capture(id) | DeletionTarget::Connection(id) => id,
            },
        )
        .await
        .map_err(|_| PrivacyDeletionError::Publication)?;
        sqlx::query(
            "insert into threads_archive.local_source_removals \
             (user_ref, social_source_id, post_id, operation_id, reason, removed_at) \
             values ($1, $2, $3, $4, 'user_requested', now()) \
             on conflict (user_ref, social_source_id) do nothing",
        )
        .bind(request.user_ref)
        .bind(source_id)
        .bind(post_id)
        .bind(request.operation_id)
        .execute(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
        sqlx::query(
            "delete from threads_archive.social_analysis_links where social_source_id = $1",
        )
        .bind(source_id)
        .execute(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
        sqlx::query(
            "delete from threads_archive.social_source_revisions where social_source_id = $1",
        )
        .bind(source_id)
        .execute(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
        sqlx::query("delete from threads_archive.social_sources where social_source_id = $1")
            .bind(source_id)
            .execute(&mut **transaction)
            .await
            .map_err(PersistenceError::Query)?;
    }
    Ok(())
}

async fn remove_unreferenced_post(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: Uuid,
    post_id: Uuid,
) -> Result<(), PrivacyDeletionError> {
    let (captures, sources, official): (i64, i64, i64) = sqlx::query_as(
        "select \
           (select count(*) from threads_archive.captures where post_id = $1), \
           (select count(*) from threads_archive.social_sources where post_id = $1), \
           (select count(*) from threads_archive.posts where post_id = $1 and account_id is not null)",
    )
    .bind(post_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    if captures > 0 || sources > 0 || official > 0 {
        return Ok(());
    }
    let media_blobs: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "select blob_ref, content_hash from threads_archive.media \
         where post_id = $1 and blob_ref is not null and content_hash is not null",
    )
    .bind(post_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    for (blob_ref, content_hash) in media_blobs {
        schedule_blob_task(transaction, operation_id, blob_ref, content_hash).await?;
    }
    sqlx::query("delete from threads_archive.media where post_id = $1")
        .bind(post_id)
        .execute(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
    sqlx::query(
        "delete from threads_archive.post_relations \
         where referencing_post_id = $1 or target_post_id = $1",
    )
    .bind(post_id)
    .execute(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    let raw_ids: Vec<Uuid> = sqlx::query_scalar(
        "select raw_object_id from threads_archive.post_revisions where post_id = $1",
    )
    .bind(post_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    sqlx::query("delete from threads_archive.post_revisions where post_id = $1")
        .bind(post_id)
        .execute(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
    for raw_id in raw_ids {
        let raw: Option<(String, Vec<u8>)> = sqlx::query_as(
            "select blob_ref, content_hash from threads_archive.raw_objects raw \
             where raw_object_id = $1 and not exists (\
               select 1 from threads_archive.post_revisions revision \
               where revision.raw_object_id = raw.raw_object_id\
             ) and not exists (\
               select 1 from threads_archive.export_records record \
               where record.raw_object_id = raw.raw_object_id\
             )",
        )
        .bind(raw_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
        if let Some((blob_ref, content_hash)) = raw {
            schedule_blob_task(transaction, operation_id, blob_ref, content_hash).await?;
            sqlx::query("delete from threads_archive.raw_objects where raw_object_id = $1")
                .bind(raw_id)
                .execute(&mut **transaction)
                .await
                .map_err(PersistenceError::Query)?;
        }
    }
    sqlx::query("delete from threads_archive.tombstones where post_id = $1")
        .bind(post_id)
        .execute(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
    sqlx::query("delete from threads_archive.posts where post_id = $1")
        .bind(post_id)
        .execute(&mut **transaction)
        .await
        .map_err(PersistenceError::Query)?;
    Ok(())
}

async fn schedule_blob_task(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    operation_id: Uuid,
    blob_ref: String,
    content_hash: Vec<u8>,
) -> Result<(), PrivacyDeletionError> {
    sqlx::query(
        "insert into threads_archive.blob_deletion_tasks \
         (task_id, operation_id, blob_ref, content_hash, state) values ($1, $2, $3, $4, 'pending') \
         on conflict (blob_ref, content_hash) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(operation_id)
    .bind(blob_ref)
    .bind(content_hash)
    .execute(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

/// One Threads-owned database table or service-owned `BlobStore` reference class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnedDataClass {
    /// `threads_archive.account_budgets`.
    AccountBudgets,
    /// `threads_archive.account_sync_checkpoints`.
    AccountSyncCheckpoints,
    /// `threads_archive.accounts`.
    Accounts,
    /// `threads_archive.blob_deletion_tasks`.
    BlobDeletionTasks,
    /// `threads_archive.captures`.
    Captures,
    /// `threads_archive.capture_resolutions`.
    CaptureResolutions,
    /// `threads_archive.credentials`.
    Credentials,
    /// `threads_archive.credential_audit`.
    CredentialAudit,
    /// `threads_archive.deletion_effects`.
    DeletionEffects,
    /// `threads_archive.deletion_operations`.
    DeletionOperations,
    /// `threads_archive.export_records`.
    ExportRecords,
    /// `threads_archive.export_reprocessing_items`.
    ExportReprocessingItems,
    /// `threads_archive.export_reprocessing_runs`.
    ExportReprocessingRuns,
    /// `threads_archive.export_runs`.
    ExportRuns,
    /// `threads_archive.inbox_events`.
    InboxEvents,
    /// `threads_archive.local_source_removals`.
    LocalSourceRemovals,
    /// `threads_archive.media`.
    Media,
    /// `threads_archive.outbox_events`.
    OutboxEvents,
    /// `threads_archive.post_relations`.
    PostRelations,
    /// `threads_archive.post_revisions`.
    PostRevisions,
    /// `threads_archive.posts`.
    Posts,
    /// `threads_archive.raw_objects`.
    RawObjects,
    /// `threads_archive.reresolution_items`.
    ReresolutionItems,
    /// `threads_archive.reresolution_runs`.
    ReresolutionRuns,
    /// `threads_archive.social_analysis_links`.
    SocialAnalysisLinks,
    /// `threads_archive.social_source_revisions`.
    SocialSourceRevisions,
    /// `threads_archive.social_sources`.
    SocialSources,
    /// `threads_archive.tombstones`.
    Tombstones,
    /// A `raw_objects.blob_ref` service-owned object.
    RawObjectBlob,
    /// A `media.blob_ref` provider-media object.
    MediaBlob,
    /// An immutable `export_runs.archive_blob_ref` object.
    ExportArchiveBlob,
}

/// The effect one target-specific deletion plan can have on one owned class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionAction {
    /// Physically erase target-owned content or credentials.
    Delete,
    /// Remove only the target-specific reference.
    Detach,
    /// Keep bounded, content-free audit or delivery evidence.
    RetainAudit,
    /// Keep storage required by another authorized holding.
    RetainShared,
    /// The class cannot contain data for this target kind.
    NotApplicable,
}

/// One unambiguous target-specific classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataClassDisposition {
    /// The owned row/blob class.
    pub class: OwnedDataClass,
    /// Its target-specific deletion effect.
    pub action: DeletionAction,
}
