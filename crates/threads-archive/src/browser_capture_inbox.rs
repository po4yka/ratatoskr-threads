//! Durable, at-least-once intake for Platform-routed Threads captures.

use ratatoskr_event_envelope::{
    EnvelopeSchemaVersion, EventEnvelope, EventPayload as _, ProducerName,
};
use ratatoskr_identifiers::{EntityRef, EventId, Extensions, TenantRef};
use ratatoskr_operation_contracts::{OperationReported, OperationStage, OperationStatus};
use sqlx::PgPool;
use uuid::Uuid;

use crate::browser_capture_command::BrowserCaptureCommand;
use crate::{Database, PersistenceError};

const CONSUMER_NAME: &str = "threads_browser_capture";
const OPERATION_REPORT_EVENT: &str = "platform.operation.reported.v1";
const PRODUCER: &str = "ratatoskr-threads";

/// The observable outcome of delivering one broker command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCaptureDelivery {
    /// The command created or replayed one explicit capture and queued owner work.
    Accepted {
        /// The owned explicit-capture record.
        capture_id: Uuid,
    },
    /// This command delivery was already recorded by the durable inbox.
    Duplicate,
}

impl BrowserCaptureDelivery {
    /// The persisted capture when this was the command's first delivery.
    #[must_use]
    pub const fn capture_id(self) -> Option<Uuid> {
        match self {
            Self::Accepted { capture_id } => Some(capture_id),
            Self::Duplicate => None,
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
    /// A static contract token did not validate.
    #[error("the operation progress report could not be constructed")]
    Contract,
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
        let received = sqlx::query_scalar::<_, Uuid>(
            "insert into threads_archive.inbox_events (consumer_name, event_id, consumed_at, handler_outcome) \
             values ($1, $2, now(), 'processed') on conflict do nothing returning event_id",
        )
        .bind(CONSUMER_NAME)
        .bind(command.command_id())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;
        if received.is_none() {
            transaction
                .commit()
                .await
                .map_err(PersistenceError::Query)?;
            return Ok(BrowserCaptureDelivery::Duplicate);
        }

        let capture_id = sqlx::query_scalar::<_, Uuid>(
            "insert into threads_archive.captures \
             (capture_id, user_ref, idempotency_key, canonical_url, original_url, acquisition_method, \
              saved_authority, client_source, status, note, captured_at) \
             values ($1, $2, $3, $4, $5, 'browser_extension', 'explicit_user_capture', \
                     'browser_extension', 'accepted', null, $6) \
             on conflict (user_ref, idempotency_key) do update set capture_id = threads_archive.captures.capture_id \
             returning capture_id",
        )
        .bind(Uuid::now_v7())
        .bind(command.user_ref())
        .bind(command.idempotency_key())
        .bind(command.canonical_permalink())
        .bind(command.original_permalink())
        .bind(command.captured_at())
        .fetch_one(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;

        let (event_id, payload) = queued_report(command, capture_id)?;
        sqlx::query(
            "insert into threads_archive.outbox_events \
             (event_id, event_type, aggregate_type, aggregate_id, payload, correlation_id, causation_id, occurred_at) \
             values ($1, $2, 'capture', $3, $4, $5, $6, now())",
        )
        .bind(event_id)
        .bind(OPERATION_REPORT_EVENT)
        .bind(capture_id)
        .bind(payload)
        .bind(command.operation_id().0)
        .bind(command.command_id())
        .execute(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;

        transaction
            .commit()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(BrowserCaptureDelivery::Accepted { capture_id })
    }
}

fn queued_report(
    command: &BrowserCaptureCommand,
    capture_id: Uuid,
) -> Result<(Uuid, serde_json::Value), BrowserCaptureInboxError> {
    let report = OperationReported {
        operation_id: command.operation_id(),
        status: OperationStatus::Queued,
        stage: Some(
            OperationStage::parse("capture_queued")
                .map_err(|_| BrowserCaptureInboxError::Contract)?,
        ),
        progress_percent: None,
        results: Vec::new(),
        error: None,
        warnings: Vec::new(),
        extensions: Extensions::new(),
    };
    let event_id = Uuid::now_v7();
    let operation = EntityRef::parse(&format!("operation:{}", command.operation_id().0))
        .map_err(|_| BrowserCaptureInboxError::Contract)?;
    let capture = EntityRef::parse(&format!("capture:{capture_id}"))
        .map_err(|_| BrowserCaptureInboxError::Contract)?;
    let cause = EntityRef::parse(&format!("command:{}", command.command_id()))
        .map_err(|_| BrowserCaptureInboxError::Contract)?;
    let mut envelope = EventEnvelope {
        event_id: EventId(event_id),
        event_type: OperationReported::event_type(),
        occurred_at: ratatoskr_identifiers::WireTimestamp::now(),
        producer: ProducerName::parse(PRODUCER).map_err(|_| BrowserCaptureInboxError::Contract)?,
        aggregate_id: capture,
        correlation_id: operation,
        causation_id: Some(cause),
        tenant_id: Some(TenantRef::of_user(ratatoskr_identifiers::UserId(
            command.user_ref(),
        ))),
        schema_version: EnvelopeSchemaVersion::CURRENT,
        payload: serde_json::Map::new(),
        extensions: Extensions::new(),
    };
    envelope
        .set_payload(&report)
        .map_err(|_| BrowserCaptureInboxError::Contract)?;
    let payload = serde_json::from_str(
        &envelope
            .to_canonical_json()
            .map_err(|_| BrowserCaptureInboxError::Contract)?,
    )?;
    Ok((event_id, payload))
}
