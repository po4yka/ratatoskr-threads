//! Queues Platform operation reports for explicit captures inside the caller's transaction.

use ratatoskr_identifiers::{CommandId, EntityRef, EventId, OperationId, UserId};
use ratatoskr_operation_contracts::OperationReported;
use ratatoskr_social_contracts::{SocialContractError, report_envelope};
use sqlx::PgConnection;
use uuid::Uuid;

const OPERATION_REPORT_EVENT: &str = "platform.operation.reported.v1";
const PRODUCER: &str = "ratatoskr-threads";

/// The capture and the Platform command that one report belongs to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReportTarget {
    /// The Ratatoskr user who owns the capture.
    pub(crate) owner: Uuid,
    /// The Platform operation being reported.
    pub(crate) operation_id: OperationId,
    /// The Platform command that started the operation.
    pub(crate) command_id: Uuid,
    /// The capture the operation advances.
    pub(crate) capture_id: Uuid,
}

/// A report could not be queued.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ReportError {
    /// A member of the report or its envelope failed its contract grammar.
    #[error("the operation report could not be constructed")]
    Contract(#[from] SocialContractError),
    /// The envelope could not be rendered as canonical JSON.
    #[error("the operation report could not be serialized")]
    Serialization(#[from] serde_json::Error),
    /// The outbox write failed.
    #[error("the operation report could not be stored")]
    Persistence(#[from] sqlx::Error),
}

fn construction(error: impl std::fmt::Display) -> ReportError {
    ReportError::Contract(SocialContractError::ReportConstruction(error.to_string()))
}

/// Writes `report` to the outbox as a complete `platform.operation.reported.v1` envelope.
pub(crate) async fn append_report(
    connection: &mut PgConnection,
    target: &ReportTarget,
    report: &OperationReported,
) -> Result<(), ReportError> {
    let event_id = Uuid::now_v7();
    let aggregate =
        EntityRef::parse(&format!("capture:{}", target.capture_id)).map_err(construction)?;
    let envelope = report_envelope(
        PRODUCER,
        UserId(target.owner),
        target.operation_id,
        aggregate,
        CommandId(target.command_id),
        EventId(event_id),
        report,
    )?;
    let payload: serde_json::Value =
        serde_json::from_str(&envelope.to_canonical_json().map_err(construction)?)?;
    sqlx::query(
        "insert into threads_archive.outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, correlation_id, causation_id, occurred_at) \
         values ($1, $2, 'capture', $3, $4, $5, $6, now())",
    )
    .bind(event_id)
    .bind(OPERATION_REPORT_EVENT)
    .bind(target.capture_id)
    .bind(payload)
    .bind(target.operation_id.0)
    .bind(target.command_id)
    .execute(connection)
    .await?;
    Ok(())
}
