use super::{DataClassDisposition, DeletionAction, OwnedDataClass};

const fn disposition(class: OwnedDataClass, action: DeletionAction) -> DataClassDisposition {
    DataClassDisposition { class, action }
}

/// The exact first-version storage inventory, including `BlobStore` reference classes.
pub const OWNED_DATA_CLASSES: &[OwnedDataClass] = &[
    OwnedDataClass::AccountBudgets,
    OwnedDataClass::AccountSyncCheckpoints,
    OwnedDataClass::Accounts,
    OwnedDataClass::BlobDeletionTasks,
    OwnedDataClass::Captures,
    OwnedDataClass::CaptureResolutions,
    OwnedDataClass::Credentials,
    OwnedDataClass::CredentialAudit,
    OwnedDataClass::DeletionEffects,
    OwnedDataClass::DeletionOperations,
    OwnedDataClass::ExportRecords,
    OwnedDataClass::ExportReprocessingItems,
    OwnedDataClass::ExportReprocessingRuns,
    OwnedDataClass::ExportRuns,
    OwnedDataClass::InboxEvents,
    OwnedDataClass::LocalSourceRemovals,
    OwnedDataClass::Media,
    OwnedDataClass::OutboxEvents,
    OwnedDataClass::PostRelations,
    OwnedDataClass::PostRevisions,
    OwnedDataClass::Posts,
    OwnedDataClass::RawObjects,
    OwnedDataClass::ReresolutionItems,
    OwnedDataClass::ReresolutionRuns,
    OwnedDataClass::SocialAnalysisLinks,
    OwnedDataClass::SocialSourceRevisions,
    OwnedDataClass::SocialSources,
    OwnedDataClass::Tombstones,
    OwnedDataClass::RawObjectBlob,
    OwnedDataClass::MediaBlob,
    OwnedDataClass::ExportArchiveBlob,
];

/// Capture-target classification for every owned row and blob class.
pub const CAPTURE_DELETION_CLASSIFICATIONS: &[DataClassDisposition] = &[
    disposition(
        OwnedDataClass::AccountBudgets,
        DeletionAction::NotApplicable,
    ),
    disposition(
        OwnedDataClass::AccountSyncCheckpoints,
        DeletionAction::NotApplicable,
    ),
    disposition(OwnedDataClass::Accounts, DeletionAction::NotApplicable),
    disposition(
        OwnedDataClass::BlobDeletionTasks,
        DeletionAction::RetainAudit,
    ),
    disposition(OwnedDataClass::Captures, DeletionAction::Delete),
    disposition(OwnedDataClass::CaptureResolutions, DeletionAction::Delete),
    disposition(OwnedDataClass::Credentials, DeletionAction::NotApplicable),
    disposition(
        OwnedDataClass::CredentialAudit,
        DeletionAction::NotApplicable,
    ),
    disposition(OwnedDataClass::DeletionEffects, DeletionAction::RetainAudit),
    disposition(
        OwnedDataClass::DeletionOperations,
        DeletionAction::RetainAudit,
    ),
    disposition(OwnedDataClass::ExportRecords, DeletionAction::NotApplicable),
    disposition(
        OwnedDataClass::ExportReprocessingItems,
        DeletionAction::NotApplicable,
    ),
    disposition(
        OwnedDataClass::ExportReprocessingRuns,
        DeletionAction::NotApplicable,
    ),
    disposition(OwnedDataClass::ExportRuns, DeletionAction::NotApplicable),
    disposition(OwnedDataClass::InboxEvents, DeletionAction::RetainAudit),
    disposition(
        OwnedDataClass::LocalSourceRemovals,
        DeletionAction::RetainAudit,
    ),
    disposition(OwnedDataClass::Media, DeletionAction::Detach),
    disposition(OwnedDataClass::OutboxEvents, DeletionAction::RetainAudit),
    disposition(OwnedDataClass::PostRelations, DeletionAction::Detach),
    disposition(OwnedDataClass::PostRevisions, DeletionAction::Detach),
    disposition(OwnedDataClass::Posts, DeletionAction::Detach),
    disposition(OwnedDataClass::RawObjects, DeletionAction::Detach),
    disposition(OwnedDataClass::ReresolutionItems, DeletionAction::Delete),
    disposition(
        OwnedDataClass::ReresolutionRuns,
        DeletionAction::RetainAudit,
    ),
    disposition(OwnedDataClass::SocialAnalysisLinks, DeletionAction::Delete),
    disposition(
        OwnedDataClass::SocialSourceRevisions,
        DeletionAction::Detach,
    ),
    disposition(OwnedDataClass::SocialSources, DeletionAction::Detach),
    disposition(OwnedDataClass::Tombstones, DeletionAction::RetainShared),
    disposition(OwnedDataClass::RawObjectBlob, DeletionAction::Detach),
    disposition(OwnedDataClass::MediaBlob, DeletionAction::Detach),
    disposition(
        OwnedDataClass::ExportArchiveBlob,
        DeletionAction::NotApplicable,
    ),
];

/// Official-connection classification for every owned row and blob class.
pub const CONNECTION_DELETION_CLASSIFICATIONS: &[DataClassDisposition] = &[
    disposition(OwnedDataClass::AccountBudgets, DeletionAction::Delete),
    disposition(
        OwnedDataClass::AccountSyncCheckpoints,
        DeletionAction::Delete,
    ),
    disposition(OwnedDataClass::Accounts, DeletionAction::Delete),
    disposition(
        OwnedDataClass::BlobDeletionTasks,
        DeletionAction::RetainAudit,
    ),
    disposition(OwnedDataClass::Captures, DeletionAction::NotApplicable),
    disposition(
        OwnedDataClass::CaptureResolutions,
        DeletionAction::NotApplicable,
    ),
    disposition(OwnedDataClass::Credentials, DeletionAction::Delete),
    disposition(OwnedDataClass::CredentialAudit, DeletionAction::RetainAudit),
    disposition(OwnedDataClass::DeletionEffects, DeletionAction::RetainAudit),
    disposition(
        OwnedDataClass::DeletionOperations,
        DeletionAction::RetainAudit,
    ),
    disposition(OwnedDataClass::ExportRecords, DeletionAction::NotApplicable),
    disposition(
        OwnedDataClass::ExportReprocessingItems,
        DeletionAction::NotApplicable,
    ),
    disposition(
        OwnedDataClass::ExportReprocessingRuns,
        DeletionAction::NotApplicable,
    ),
    disposition(OwnedDataClass::ExportRuns, DeletionAction::NotApplicable),
    disposition(OwnedDataClass::InboxEvents, DeletionAction::RetainAudit),
    disposition(
        OwnedDataClass::LocalSourceRemovals,
        DeletionAction::RetainAudit,
    ),
    disposition(OwnedDataClass::Media, DeletionAction::Detach),
    disposition(OwnedDataClass::OutboxEvents, DeletionAction::RetainAudit),
    disposition(OwnedDataClass::PostRelations, DeletionAction::Detach),
    disposition(OwnedDataClass::PostRevisions, DeletionAction::Detach),
    disposition(OwnedDataClass::Posts, DeletionAction::Detach),
    disposition(OwnedDataClass::RawObjects, DeletionAction::Detach),
    disposition(
        OwnedDataClass::ReresolutionItems,
        DeletionAction::NotApplicable,
    ),
    disposition(
        OwnedDataClass::ReresolutionRuns,
        DeletionAction::NotApplicable,
    ),
    disposition(OwnedDataClass::SocialAnalysisLinks, DeletionAction::Detach),
    disposition(
        OwnedDataClass::SocialSourceRevisions,
        DeletionAction::Detach,
    ),
    disposition(OwnedDataClass::SocialSources, DeletionAction::Detach),
    disposition(OwnedDataClass::Tombstones, DeletionAction::RetainShared),
    disposition(OwnedDataClass::RawObjectBlob, DeletionAction::Detach),
    disposition(OwnedDataClass::MediaBlob, DeletionAction::Detach),
    disposition(
        OwnedDataClass::ExportArchiveBlob,
        DeletionAction::NotApplicable,
    ),
];

impl OwnedDataClass {
    /// Returns the stable inventory key used in content-free deletion reports.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::AccountBudgets => "table:account_budgets",
            Self::AccountSyncCheckpoints => "table:account_sync_checkpoints",
            Self::Accounts => "table:accounts",
            Self::BlobDeletionTasks => "table:blob_deletion_tasks",
            Self::Captures => "table:captures",
            Self::CaptureResolutions => "table:capture_resolutions",
            Self::Credentials => "table:credentials",
            Self::CredentialAudit => "table:credential_audit",
            Self::DeletionEffects => "table:deletion_effects",
            Self::DeletionOperations => "table:deletion_operations",
            Self::ExportRecords => "table:export_records",
            Self::ExportReprocessingItems => "table:export_reprocessing_items",
            Self::ExportReprocessingRuns => "table:export_reprocessing_runs",
            Self::ExportRuns => "table:export_runs",
            Self::InboxEvents => "table:inbox_events",
            Self::LocalSourceRemovals => "table:local_source_removals",
            Self::Media => "table:media",
            Self::OutboxEvents => "table:outbox_events",
            Self::PostRelations => "table:post_relations",
            Self::PostRevisions => "table:post_revisions",
            Self::Posts => "table:posts",
            Self::RawObjects => "table:raw_objects",
            Self::ReresolutionItems => "table:reresolution_items",
            Self::ReresolutionRuns => "table:reresolution_runs",
            Self::SocialAnalysisLinks => "table:social_analysis_links",
            Self::SocialSourceRevisions => "table:social_source_revisions",
            Self::SocialSources => "table:social_sources",
            Self::Tombstones => "table:tombstones",
            Self::RawObjectBlob => "blob:raw_object",
            Self::MediaBlob => "blob:provider_media",
            Self::ExportArchiveBlob => "blob:export_archive",
        }
    }

    pub(crate) fn audit_key(self) -> &'static str {
        match self {
            Self::RawObjectBlob => "raw_object_blob",
            Self::MediaBlob => "provider_media_blob",
            Self::ExportArchiveBlob => "export_archive_blob",
            _ => self.key().strip_prefix("table:").unwrap_or("invalid_class"),
        }
    }

    pub(crate) fn from_audit_key(value: &str) -> Option<Self> {
        OWNED_DATA_CLASSES
            .iter()
            .copied()
            .find(|class| class.audit_key() == value)
    }
}

impl DeletionAction {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Detach => "detach",
            Self::RetainAudit => "retain_audit",
            Self::RetainShared => "retain_shared",
            Self::NotApplicable => "not_applicable",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "delete" => Some(Self::Delete),
            "detach" => Some(Self::Detach),
            "retain_audit" => Some(Self::RetainAudit),
            "retain_shared" => Some(Self::RetainShared),
            "not_applicable" => Some(Self::NotApplicable),
            _ => None,
        }
    }
}
