//! Privacy-safe storage for Knowledge completion linkage facts.

use crate::{Database, PersistenceError};
use chrono::{DateTime, Utc};
use ratatoskr_social_contracts::SocialSourceAnalysisCompleted;
use uuid::Uuid;

/// The observable outcome of attempting to link a Knowledge completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionLinkOutcome {
    /// The completion was linked to an exact Threads source revision.
    Linked,
    /// The same event was already processed.
    Duplicate,
    /// The completion did not name a Threads revision owned by this tenant.
    Rejected,
}

/// Accepts privacy-safe completion facts from Knowledge.
#[derive(Debug, Clone)]
pub struct KnowledgeCompletionStore<'a> {
    database: &'a Database,
}

impl<'a> KnowledgeCompletionStore<'a> {
    /// Creates a completion-link store over the owned archive database.
    #[must_use]
    pub const fn new(database: &'a Database) -> Self {
        Self { database }
    }

    /// Records one typed completion fact exactly once.
    ///
    /// A completion can link only to an existing source revision with the
    /// same tenant and digest. Other inputs are retained as rejected inbox
    /// observations, which makes redelivery deterministic without admitting
    /// a cross-tenant or stale link.
    ///
    /// # Errors
    ///
    /// Returns [`PersistenceError`] when the archive cannot transactionally
    /// record the inbox observation or its linkage.
    pub async fn record(
        &self,
        event_id: Uuid,
        completion: &SocialSourceAnalysisCompleted,
    ) -> Result<CompletionLinkOutcome, PersistenceError> {
        let mut transaction = self
            .database
            .pool()
            .begin()
            .await
            .map_err(PersistenceError::Query)?;
        let accepted: Option<Uuid> = sqlx::query_scalar(
            "insert into threads_archive.inbox_events \
             (consumer_name, event_id, consumed_at, handler_outcome) \
             values ('threads-social-source-knowledge', $1, now(), 'processed') \
             on conflict (consumer_name, event_id) do nothing returning event_id",
        )
        .bind(event_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;
        if accepted.is_none() {
            transaction
                .commit()
                .await
                .map_err(PersistenceError::Query)?;
            return Ok(CompletionLinkOutcome::Duplicate);
        }

        let owner = completion.owner.user_id().0;
        let source_id = completion.social_source_id.to_string();
        let digest = completion.content_digest.hex.to_string();
        let matches: Option<Uuid> =
            sqlx::query_scalar(
                "select revision.source_revision_id \
             from threads_archive.social_source_revisions revision \
             join threads_archive.social_sources source \
               on source.social_source_id = revision.social_source_id \
             where revision.social_source_id = $1 and revision.content_digest = $2 \
               and source.user_ref = $3",
            )
            .bind(source_id.parse::<Uuid>().map_err(|error| {
                PersistenceError::Query(sqlx::Error::Protocol(error.to_string()))
            })?)
            .bind(&digest)
            .bind(owner)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(PersistenceError::Query)?;
        if matches.is_none() {
            sqlx::query(
                "update threads_archive.inbox_events set handler_outcome = 'rejected' \
                 where consumer_name = 'threads-social-source-knowledge' and event_id = $1",
            )
            .bind(event_id)
            .execute(&mut *transaction)
            .await
            .map_err(PersistenceError::Query)?;
            transaction
                .commit()
                .await
                .map_err(PersistenceError::Query)?;
            return Ok(CompletionLinkOutcome::Rejected);
        }
        let completed_at = DateTime::parse_from_rfc3339(&completion.completed_at.to_wire())
            .map_err(|error| PersistenceError::Query(sqlx::Error::Protocol(error.to_string())))?
            .with_timezone(&Utc);
        sqlx::query(
            "insert into threads_archive.social_analysis_links \
             (completion_event_id, user_ref, social_source_id, content_digest, completed_at) \
             values ($1, $2, $3, $4, $5)",
        )
        .bind(event_id)
        .bind(owner)
        .bind(
            source_id.parse::<Uuid>().map_err(|error| {
                PersistenceError::Query(sqlx::Error::Protocol(error.to_string()))
            })?,
        )
        .bind(digest)
        .bind(completed_at)
        .execute(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;
        transaction
            .commit()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(CompletionLinkOutcome::Linked)
    }
}
