//! Durable, at-least-once intake for Platform-routed Threads captures.

use ratatoskr_operation_contracts::OperationReported;
use ratatoskr_social_contracts::{SocialContractError, queued_report};
use sqlx::PgPool;
use uuid::Uuid;

use ratatoskr_social_contracts::{SourceUnavailability, unavailable_report};

use crate::browser_capture_command::{BrowserCaptureCommand, RejectedCommand};
use crate::capture_reporting::{ReportAggregate, ReportError, ReportTarget, append_report};
use crate::{Database, PersistenceError};

const CONSUMER_NAME: &str = "threads_browser_capture";

/// The observable outcome of delivering one broker command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCaptureDelivery {
    /// The command created or replayed one explicit capture and queued owner work.
    Accepted {
        /// The owned explicit-capture record.
        capture_id: Uuid,
    },
    /// The command was refused and its operation got a terminal failure report.
    Rejected,
    /// This command delivery was already recorded by the durable inbox.
    Duplicate,
}

impl BrowserCaptureDelivery {
    /// The persisted capture when this was the command's first delivery.
    #[must_use]
    pub const fn capture_id(self) -> Option<Uuid> {
        match self {
            Self::Accepted { capture_id } => Some(capture_id),
            Self::Rejected | Self::Duplicate => None,
        }
    }
}

/// A failure while atomically accepting a browser-capture command.
#[derive(Debug, thiserror::Error)]
pub enum BrowserCaptureInboxError {
    /// The archive database refused the inbox, capture, or outbox write.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    /// The contract-shaped operation report could not be serialized.
    #[error("the operation progress report could not be serialized")]
    Serialization(#[from] serde_json::Error),
    /// The queued report did not satisfy its contract.
    #[error("the operation progress report could not be constructed")]
    Contract(#[from] SocialContractError),
}

impl From<ReportError> for BrowserCaptureInboxError {
    fn from(error: ReportError) -> Self {
        match error {
            ReportError::Contract(error) => Self::Contract(error),
            ReportError::Serialization(error) => Self::Serialization(error),
            ReportError::Persistence(error) => Self::Persistence(PersistenceError::Query(error)),
        }
    }
}

/// Writes the inbox marker, explicit capture, and queued operation report in one transaction.
#[derive(Debug)]
pub struct BrowserCaptureInbox<'a> {
    pool: &'a PgPool,
}

impl<'a> BrowserCaptureInbox<'a> {
    /// Builds an inbox over the database owned by this bounded context.
    #[must_use]
    pub const fn new(database: &'a Database) -> Self {
        Self {
            pool: database.pool(),
        }
    }

    /// Delivers one already-validated Threads browser-capture command exactly once.
    ///
    /// The inbox row, the capture with its original URL/provenance/capture time,
    /// and a queued operation report commit atomically. A duplicate command ID
    /// performs no domain write, so broker redelivery cannot create another local
    /// capture or report.
    ///
    /// # Errors
    ///
    /// Returns [`BrowserCaptureInboxError`] without acknowledging the broker
    /// delivery when any owned write fails.
    pub async fn deliver(
        &self,
        command: &BrowserCaptureCommand,
    ) -> Result<BrowserCaptureDelivery, BrowserCaptureInboxError> {
        let mut transaction = self.pool.begin().await.map_err(PersistenceError::Query)?;
        if !claim_delivery(&mut transaction, command.command_id(), "processed").await? {
            transaction
                .commit()
                .await
                .map_err(PersistenceError::Query)?;
            return Ok(BrowserCaptureDelivery::Duplicate);
        }

        let capture_id = sqlx::query_scalar::<_, Uuid>(
            "insert into threads_archive.captures \
             (capture_id, user_ref, idempotency_key, canonical_url, original_url, acquisition_method, \
              saved_authority, client_source, status, note, captured_at, operation_id, command_id, \
              next_resolution_at) \
             values ($1, $2, $3, $4, $5, 'browser_extension', 'explicit_user_capture', \
                     'browser_extension', 'accepted', null, $6, $7, $8, now()) \
             on conflict (user_ref, idempotency_key) do update set capture_id = threads_archive.captures.capture_id \
             returning capture_id",
        )
        .bind(Uuid::now_v7())
        .bind(command.user_ref())
        .bind(command.idempotency_key())
        .bind(command.canonical_permalink())
        .bind(command.original_permalink())
        .bind(command.captured_at())
        .bind(command.operation_id().0)
        .bind(command.command_id())
        .fetch_one(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;

        let target = ReportTarget {
            owner: command.user_ref(),
            operation_id: command.operation_id(),
            command_id: command.command_id(),
            aggregate: ReportAggregate::Capture(capture_id),
        };
        let queued: OperationReported = queued_report(command.operation_id())?;
        append_report(&mut transaction, &target, &queued).await?;

        transaction
            .commit()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(BrowserCaptureDelivery::Accepted { capture_id })
    }

    /// Answers a refused command: the inbox marker and one terminal `Inaccessible` report commit
    /// together, so the operation is never left waiting and a redelivery reports nothing more.
    ///
    /// # Errors
    ///
    /// Returns [`BrowserCaptureInboxError`] without acknowledging the broker delivery when any
    /// owned write fails.
    pub async fn reject(
        &self,
        command: &RejectedCommand,
    ) -> Result<BrowserCaptureDelivery, BrowserCaptureInboxError> {
        let mut transaction = self.pool.begin().await.map_err(PersistenceError::Query)?;
        if !claim_delivery(&mut transaction, command.command_id(), "rejected").await? {
            transaction
                .commit()
                .await
                .map_err(PersistenceError::Query)?;
            return Ok(BrowserCaptureDelivery::Duplicate);
        }
        let target = ReportTarget {
            owner: command.user_ref(),
            operation_id: command.operation_id(),
            command_id: command.command_id(),
            aggregate: ReportAggregate::Operation,
        };
        let failed: OperationReported =
            unavailable_report(command.operation_id(), SourceUnavailability::Inaccessible)?;
        append_report(&mut transaction, &target, &failed).await?;
        transaction
            .commit()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(BrowserCaptureDelivery::Rejected)
    }
}

/// Records the delivery in the inbox, returning whether this is its first delivery.
async fn claim_delivery(
    connection: &mut sqlx::PgConnection,
    command_id: Uuid,
    outcome: &str,
) -> Result<bool, PersistenceError> {
    let claimed = sqlx::query_scalar::<_, Uuid>(
        "insert into threads_archive.inbox_events (consumer_name, event_id, consumed_at, handler_outcome) \
         values ($1, $2, now(), $3) on conflict do nothing returning event_id",
    )
    .bind(CONSUMER_NAME)
    .bind(command_id)
    .bind(outcome)
    .fetch_optional(connection)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(claimed.is_some())
}
