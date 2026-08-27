//! Safe, raw-first Threads Data Export handling.

use std::collections::BTreeSet;

use sqlx::PgPool;
use tokio::io::AsyncRead;
use uuid::Uuid;

use crate::database::{Database, PersistenceError};
use crate::public_resolution::{PublicResolutionError, RawObjectStore};
use crate::publishing;

const SUPPORTED_EXPORT_VERSION: &str = "threads-export-v1";
const PARSER_VERSION: &str = "threads-export-v1-parser-1";
const MAX_ARCHIVE_RECEIPT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_RECEIPT_LENGTH: usize = 64 * 1024 * 1024;

/// Bounded ZIP-inspection limits for one supplied export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportLimits {
    /// Largest number of archive entries that may be inspected.
    pub max_entries: usize,
    /// Largest normalized entry-path depth.
    pub max_path_depth: usize,
    /// Largest total compressed byte count.
    pub max_compressed_bytes: u64,
    /// Largest total declared decompressed byte count.
    pub max_decompressed_bytes: u64,
    /// Largest allowed declared decompressed-to-compressed ratio.
    pub max_compression_ratio: u64,
}

impl Default for ExportLimits {
    fn default() -> Self {
        Self {
            max_entries: 1_000,
            max_path_depth: 16,
            max_compressed_bytes: 64 * 1024 * 1024,
            max_decompressed_bytes: 256 * 1024 * 1024,
            max_compression_ratio: 100,
        }
    }
}

/// The safe archive names returned to a parser after inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedArchive {
    /// Stable archive-entry names in their original archive order.
    pub entry_names: Vec<String>,
}

/// Safe archive content extracted beneath a caller-owned private directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedArchive {
    /// Stable names extracted under the supplied root.
    pub entry_names: Vec<String>,
}

/// The durable outcome of receiving one owner-authorized export archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptOutcome {
    /// A new immutable archive receipt and running import record were stored.
    Created(ExportReceipt),
    /// The owner had already supplied these exact archive bytes.
    Replayed(ExportReceipt),
}

/// The durable evidence that identifies one received archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReceipt {
    /// Owner authenticated for this archive receipt.
    pub user_ref: Uuid,
    /// Stable import-run identity.
    pub run_id: Uuid,
    /// SHA-256 digest bytes of the received archive.
    pub archive_hash: Vec<u8>,
    /// Content-addressed reference to the immutable raw archive.
    pub archive_blob_ref: String,
    /// Exact received archive byte length.
    pub archive_byte_size: i64,
}

/// The terminal reconciliation evidence for one immutable export receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    /// The durable import-run identity.
    pub run_id: Uuid,
    /// Whether unknown sections required a warning outcome.
    pub completed_with_warnings: bool,
    /// Number of normalized records and preserved unknown sections.
    pub records_processed: i64,
    /// Owner-scoped coverage evidence; export absence never means deletion.
    pub completeness_report: CompletenessReport,
}

/// Persists raw-first owner-authorized Data Export receipts.
#[derive(Debug, Clone)]
pub struct DataExportStore {
    pool: PgPool,
    raw_objects: RawObjectStore,
}

impl DataExportStore {
    /// Creates a store using the Threads-owned database and raw evidence root.
    #[must_use]
    pub fn new(database: &Database, raw_objects: RawObjectStore) -> Self {
        Self {
            pool: database.pool().clone(),
            raw_objects,
        }
    }

    /// Stores immutable archive bytes and creates one owner-scoped running import.
    ///
    /// # Errors
    ///
    /// Returns [`ExportError`] when immutable raw storage or the durable
    /// owner/digest receipt transaction fails.
    pub async fn receive(
        &self,
        user_ref: Uuid,
        archive_bytes: &[u8],
    ) -> Result<ReceiptOutcome, ExportError> {
        if archive_bytes.len() > MAX_ARCHIVE_RECEIPT_LENGTH {
            return Err(ExportError::Limit {
                limit: "receipt_bytes",
                detail: archive_bytes.len().to_string(),
            });
        }
        let raw = self
            .raw_objects
            .store(archive_bytes)
            .await
            .map_err(ExportError::RawStorage)?;
        self.persist_receipt(user_ref, raw).await
    }

    /// Streams one owner-authorized archive into immutable raw storage.
    ///
    /// # Errors
    ///
    /// Returns [`ExportError`] when the stream exceeds its bounded receipt
    /// budget, raw storage fails, or durable receipt persistence fails.
    pub async fn receive_stream<R>(
        &self,
        user_ref: Uuid,
        reader: &mut R,
    ) -> Result<ReceiptOutcome, ExportError>
    where
        R: AsyncRead + Unpin,
    {
        let raw = Box::pin(self.raw_objects.store_stream(
            reader,
            MAX_ARCHIVE_RECEIPT_BYTES,
            "application/zip",
        ))
        .await
        .map_err(ExportError::RawStorage)?;
        self.persist_receipt(user_ref, raw).await
    }

    /// Parses and reconciles the exact immutable archive identified by `receipt`.
    ///
    /// A terminal successful run is replay-safe: it returns its already-persisted
    /// report instead of duplicating projections or source facts.
    ///
    /// # Errors
    ///
    /// Returns an error after marking a safely received archive failed when its
    /// ZIP, layout, or supported version cannot be processed.
    pub async fn import(&self, receipt: &ExportReceipt) -> Result<ImportOutcome, ExportError> {
        let status = load_run_status(&self.pool, receipt).await?;
        if let Some(outcome) = status.terminal_outcome()? {
            return Ok(outcome);
        }
        let archive = self
            .raw_objects
            .read_verified(&receipt.archive_blob_ref, &receipt.archive_hash)
            .await
            .map_err(ExportError::RawStorage)?;
        let parsed = match parse_export(&archive, ExportLimits::default()) {
            Ok(parsed) => parsed,
            Err(error) => {
                mark_failed(&self.pool, receipt.run_id, safe_error_summary(&error)).await?;
                return Err(error);
            }
        };
        self.reconcile(receipt, parsed).await
    }

    async fn reconcile(
        &self,
        receipt: &ExportReceipt,
        parsed: ParsedExport,
    ) -> Result<ImportOutcome, ExportError> {
        let mut transaction = self.pool.begin().await.map_err(PersistenceError::Query)?;
        let archive_raw_object_id = load_archive_raw_object(&mut transaction, receipt).await?;
        let mut records_processed = 0_i64;
        for post in &parsed.posts {
            let post_id = record_export_post(&mut transaction, post, archive_raw_object_id).await?;
            record_normalized_post(
                &mut transaction,
                receipt.run_id,
                archive_raw_object_id,
                post,
            )
            .await?;
            publishing::append_export_fact(&mut transaction, receipt.user_ref, post_id)
                .await
                .map_err(|error| publish_error(&error))?;
            records_processed = records_processed
                .checked_add(1)
                .ok_or_else(record_count_error)?;
        }
        for relation in &parsed.relations {
            records_processed +=
                record_export_relation(&mut transaction, receipt.run_id, relation).await?;
        }
        for entry_name in &parsed.unknown_entries {
            record_unknown_section(
                &mut transaction,
                receipt.run_id,
                archive_raw_object_id,
                entry_name,
            )
            .await?;
            records_processed = records_processed
                .checked_add(1)
                .ok_or_else(record_count_error)?;
        }
        let captures = capture_identities(&mut transaction, receipt.user_ref).await?;
        let export_ids = parsed
            .posts
            .iter()
            .map(|post| post.provider_post_id.clone())
            .collect();
        let completeness_report = completeness_report(&export_ids, captures);
        let completed_with_warnings = !parsed.unknown_entries.is_empty();
        finish_run(
            &mut transaction,
            receipt.run_id,
            &parsed,
            records_processed,
            &completeness_report,
            completed_with_warnings,
        )
        .await?;
        transaction
            .commit()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(ImportOutcome {
            run_id: receipt.run_id,
            completed_with_warnings,
            records_processed,
            completeness_report,
        })
    }

    async fn persist_receipt(
        &self,
        user_ref: Uuid,
        raw: crate::public_resolution::StoredRaw,
    ) -> Result<ReceiptOutcome, ExportError> {
        let mut transaction = self.pool.begin().await.map_err(PersistenceError::Query)?;
        let run_id = Uuid::now_v7();
        let created: Option<Uuid> = sqlx::query_scalar(
            "insert into threads_archive.export_runs \
             (run_id, user_ref, archive_hash, archive_blob_ref, archive_byte_size, parser_version, outcome) \
             values ($1, $2, $3, $4, $5, 'pending', 'running') \
             on conflict (user_ref, archive_hash) do nothing returning run_id",
        )
        .bind(run_id)
        .bind(user_ref)
        .bind(&raw.content_hash)
        .bind(&raw.blob_ref)
        .bind(raw.byte_size)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;
        let outcome = if let Some(run_id) = created {
            insert_archive_raw(&mut transaction, &raw).await?;
            ReceiptOutcome::Created(ExportReceipt {
                user_ref,
                run_id,
                archive_hash: raw.content_hash,
                archive_blob_ref: raw.blob_ref,
                archive_byte_size: raw.byte_size,
            })
        } else {
            ReceiptOutcome::Replayed(
                load_receipt(&mut transaction, user_ref, &raw.content_hash).await?,
            )
        };
        transaction
            .commit()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(outcome)
    }
}

async fn insert_archive_raw(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    raw: &crate::public_resolution::StoredRaw,
) -> Result<(), ExportError> {
    sqlx::query(
        "insert into threads_archive.raw_objects \
         (raw_object_id, object_kind, blob_ref, content_hash, byte_size, media_type, observed_at) \
         values ($1, 'export_archive', $2, $3, $4, 'application/zip', now())",
    )
    .bind(Uuid::now_v7())
    .bind(&raw.blob_ref)
    .bind(&raw.content_hash)
    .bind(raw.byte_size)
    .execute(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

async fn load_receipt(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_ref: Uuid,
    archive_hash: &[u8],
) -> Result<ExportReceipt, ExportError> {
    let receipt: Option<(Uuid, Vec<u8>, String, i64)> = sqlx::query_as(
        "select run_id, archive_hash, archive_blob_ref, archive_byte_size \
         from threads_archive.export_runs where user_ref = $1 and archive_hash = $2",
    )
    .bind(user_ref)
    .bind(archive_hash)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    let (run_id, archive_hash, archive_blob_ref, archive_byte_size) = receipt.ok_or_else(|| {
        ExportError::Persistence(PersistenceError::Query(sqlx::Error::RowNotFound))
    })?;
    Ok(ExportReceipt {
        user_ref,
        run_id,
        archive_hash,
        archive_blob_ref,
        archive_byte_size,
    })
}

enum RunStatus {
    Running,
    Completed(ImportOutcome),
    Failed,
}

impl RunStatus {
    fn terminal_outcome(self) -> Result<Option<ImportOutcome>, ExportError> {
        match self {
            Self::Running => Ok(None),
            Self::Completed(outcome) => Ok(Some(outcome)),
            Self::Failed => Err(ExportError::Persistence(PersistenceError::Query(
                sqlx::Error::Protocol("Data Export run is already failed".to_owned()),
            ))),
        }
    }
}

async fn load_run_status(pool: &PgPool, receipt: &ExportReceipt) -> Result<RunStatus, ExportError> {
    let row: Option<(String, i64, Option<serde_json::Value>)> = sqlx::query_as(
        "select outcome, records_processed, completeness_report from threads_archive.export_runs \
         where run_id = $1 and user_ref = $2 and archive_hash = $3 and archive_blob_ref = $4",
    )
    .bind(receipt.run_id)
    .bind(receipt.user_ref)
    .bind(&receipt.archive_hash)
    .bind(&receipt.archive_blob_ref)
    .fetch_optional(pool)
    .await
    .map_err(PersistenceError::Query)?;
    let (outcome, records_processed, report) = row.ok_or_else(|| {
        ExportError::Persistence(PersistenceError::Query(sqlx::Error::RowNotFound))
    })?;
    match outcome.as_str() {
        "running" => Ok(RunStatus::Running),
        "completed" | "completed_with_warnings" => {
            let report = report.ok_or_else(|| {
                ExportError::Persistence(PersistenceError::Query(sqlx::Error::ColumnNotFound(
                    "completeness_report".to_owned(),
                )))
            })?;
            let completeness_report = serde_json::from_value(report).map_err(|error| {
                ExportError::Persistence(PersistenceError::Query(sqlx::Error::Decode(Box::new(
                    error,
                ))))
            })?;
            Ok(RunStatus::Completed(ImportOutcome {
                run_id: receipt.run_id,
                completed_with_warnings: outcome == "completed_with_warnings",
                records_processed,
                completeness_report,
            }))
        }
        "failed" => Ok(RunStatus::Failed),
        _ => Err(ExportError::Persistence(PersistenceError::Query(
            sqlx::Error::Protocol("unknown Data Export run outcome".to_owned()),
        ))),
    }
}

async fn mark_failed(pool: &PgPool, run_id: Uuid, warning: String) -> Result<(), ExportError> {
    sqlx::query(
        "update threads_archive.export_runs set outcome = 'failed', warnings_summary = $2, \
         finished_at = now() where run_id = $1 and outcome = 'running'",
    )
    .bind(run_id)
    .bind(warning)
    .execute(pool)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

fn safe_error_summary(error: &ExportError) -> String {
    match error {
        ExportError::InvalidZip(_) => "invalid_zip".to_owned(),
        ExportError::Limit { limit, .. } => format!("archive_limit:{limit}"),
        ExportError::UnsupportedLayout => "unsupported_layout".to_owned(),
        ExportError::InvalidManifest(_) => "invalid_manifest".to_owned(),
        ExportError::UnsupportedVersion(_) => "unsupported_version".to_owned(),
        ExportError::Extraction { .. } => "extraction_failed".to_owned(),
        ExportError::RawStorage(_) | ExportError::Persistence(_) => "import_failed".to_owned(),
    }
}

async fn load_archive_raw_object(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    receipt: &ExportReceipt,
) -> Result<Uuid, ExportError> {
    sqlx::query_scalar(
        "select raw_object_id from threads_archive.raw_objects \
         where object_kind = 'export_archive' and blob_ref = $1 and content_hash = $2 \
         order by observed_at, raw_object_id limit 1",
    )
    .bind(&receipt.archive_blob_ref)
    .bind(&receipt.archive_hash)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?
    .ok_or_else(|| ExportError::Persistence(PersistenceError::Query(sqlx::Error::RowNotFound)))
}

async fn record_export_post(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    post: &ExportPost,
    raw_object_id: Uuid,
) -> Result<Uuid, ExportError> {
    let post_id: Uuid = sqlx::query_scalar(
        "insert into threads_archive.posts \
         (post_id, provider_post_id, permalink, post_kind, text_content, acquisition_method, saved_authority, upstream_status) \
         values ($1, $2, $3, 'post', $4, 'data_export', 'export_observation', 'active') \
         on conflict (provider_post_id) do update set permalink = excluded.permalink, \
         text_content = excluded.text_content, acquisition_method = excluded.acquisition_method, \
         saved_authority = excluded.saved_authority, upstream_status = excluded.upstream_status, \
         updated_at = now() returning post_id",
    )
    .bind(Uuid::now_v7())
    .bind(&post.provider_post_id)
    .bind(&post.permalink)
    .bind(&post.text)
    .fetch_one(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    sqlx::query(
        "insert into threads_archive.post_revisions \
         (revision_id, post_id, raw_object_id, parser_version, observed_at) \
         values ($1, $2, $3, $4, now())",
    )
    .bind(Uuid::now_v7())
    .bind(post_id)
    .bind(raw_object_id)
    .bind(PARSER_VERSION)
    .execute(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(post_id)
}

async fn record_export_relation(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
    relation: &ExportRelation,
) -> Result<i64, ExportError> {
    let referencing_post_id: Option<Uuid> =
        sqlx::query_scalar("select post_id from threads_archive.posts where provider_post_id = $1")
            .bind(&relation.referencing_provider_post_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(PersistenceError::Query)?;
    let Some(referencing_post_id) = referencing_post_id else {
        record_warning(transaction, run_id, "unresolved_relation_source", relation).await?;
        return Ok(1);
    };
    let target_post_id: Option<Uuid> =
        sqlx::query_scalar("select post_id from threads_archive.posts where provider_post_id = $1")
            .bind(&relation.target_provider_post_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(PersistenceError::Query)?;
    sqlx::query(
        "insert into threads_archive.post_relations \
         (relation_id, referencing_post_id, target_post_id, target_provider_post_id, target_permalink, relation_kind) \
         values ($1, $2, $3, $4, null, $5) \
         on conflict (referencing_post_id, target_provider_post_id, relation_kind) do update set \
         target_post_id = excluded.target_post_id",
    )
    .bind(Uuid::now_v7())
    .bind(referencing_post_id)
    .bind(target_post_id)
    .bind(&relation.target_provider_post_id)
    .bind(&relation.relation_kind)
    .execute(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(1)
}

async fn record_normalized_post(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
    raw_object_id: Uuid,
    post: &ExportPost,
) -> Result<(), ExportError> {
    sqlx::query(
        "insert into threads_archive.export_records \
         (record_id, run_id, record_kind, category, provider_record_id, raw_object_id, payload) \
         values ($1, $2, 'normalized', 'posts', $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(run_id)
    .bind(&post.provider_post_id)
    .bind(raw_object_id)
    .bind(serde_json::to_value(post).map_err(ExportError::InvalidManifest)?)
    .execute(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

async fn record_unknown_section(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
    raw_object_id: Uuid,
    entry_name: &str,
) -> Result<(), ExportError> {
    sqlx::query(
        "insert into threads_archive.export_records \
         (record_id, run_id, record_kind, category, raw_object_id, payload) \
         values ($1, $2, 'unknown_section', $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(run_id)
    .bind(entry_name)
    .bind(raw_object_id)
    .bind(serde_json::json!({ "warning": "unknown_section_preserved" }))
    .execute(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

async fn record_warning(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
    warning: &str,
    relation: &ExportRelation,
) -> Result<(), ExportError> {
    sqlx::query(
        "insert into threads_archive.export_records \
         (record_id, run_id, record_kind, category, payload) \
         values ($1, $2, 'warning', 'relations', $3)",
    )
    .bind(Uuid::now_v7())
    .bind(run_id)
    .bind(serde_json::json!({
        "warning": warning,
        "referencing_provider_post_id": relation.referencing_provider_post_id,
        "target_provider_post_id": relation.target_provider_post_id,
    }))
    .execute(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

async fn capture_identities(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_ref: Uuid,
) -> Result<Vec<Option<String>>, ExportError> {
    sqlx::query_scalar(
        "select post.provider_post_id from threads_archive.captures capture \
         left join threads_archive.posts post on post.post_id = capture.post_id \
         where capture.user_ref = $1",
    )
    .bind(user_ref)
    .fetch_all(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)
    .map_err(ExportError::from)
}

async fn finish_run(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_id: Uuid,
    parsed: &ParsedExport,
    records_processed: i64,
    report: &CompletenessReport,
    completed_with_warnings: bool,
) -> Result<(), ExportError> {
    let outcome = if completed_with_warnings {
        "completed_with_warnings"
    } else {
        "completed"
    };
    let warning = completed_with_warnings.then_some("unknown_export_sections_preserved");
    sqlx::query(
        "update threads_archive.export_runs set detected_version = $2, parser_version = $3, \
         outcome = $4, records_processed = $5, warnings_summary = $6, completeness_report = $7, \
         finished_at = now() where run_id = $1 and outcome = 'running'",
    )
    .bind(run_id)
    .bind(&parsed.detected_version)
    .bind(parsed.parser_version)
    .bind(outcome)
    .bind(records_processed)
    .bind(warning)
    .bind(serde_json::to_value(report).map_err(ExportError::InvalidManifest)?)
    .execute(&mut **transaction)
    .await
    .map_err(PersistenceError::Query)?;
    Ok(())
}

fn record_count_error() -> ExportError {
    ExportError::Persistence(PersistenceError::Query(sqlx::Error::Configuration(
        "Data Export record count exceeds bigint".to_owned().into(),
    )))
}

fn publish_error(error: &publishing::PublishError) -> ExportError {
    ExportError::Persistence(PersistenceError::Query(sqlx::Error::Protocol(
        error.to_string(),
    )))
}

/// Coverage evidence from one export compared with locally captured identities.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct CompletenessReport {
    /// Distinct provider identities recognized in the export.
    pub export_identities: usize,
    /// Comparable captures also present in the export.
    pub matched_captures: usize,
    /// Export identities not represented by a comparable local capture.
    pub export_only: usize,
    /// Comparable local captures absent from the export.
    pub capture_only: usize,
    /// Local captures without a stable identity to compare.
    pub non_comparable_captures: usize,
}

/// One parsed Threads export post with export-observation provenance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExportPost {
    /// Stable provider identity from the supported export layout.
    pub provider_post_id: String,
    /// Canonical permalink observed in the export.
    pub permalink: String,
    /// Provider text exposed by the export.
    pub text: Option<String>,
}

/// One directed relation supplied by the export.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct ExportRelation {
    /// Referencing stable provider identity.
    pub referencing_provider_post_id: String,
    /// Provider relation token.
    pub relation_kind: String,
    /// Target stable provider identity.
    pub target_provider_post_id: String,
}

/// Deterministic output from the one supported export parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedExport {
    /// Detected provider export version.
    pub detected_version: String,
    /// Stable implementation parser version.
    pub parser_version: &'static str,
    /// Ordered recognized posts.
    pub posts: Vec<ExportPost>,
    /// Ordered directed relations.
    pub relations: Vec<ExportRelation>,
    /// Unrecognized archive sections retained for raw-first persistence.
    pub unknown_entries: Vec<String>,
}

/// Builds initial completeness evidence from export and capture identities.
#[must_use]
pub fn completeness_report(
    export_identities: &BTreeSet<String>,
    capture_identities: Vec<Option<String>>,
) -> CompletenessReport {
    let (comparable, non_comparable_captures) = split_capture_identities(capture_identities);
    let matched_captures = export_identities.intersection(&comparable).count();
    let export_only = export_identities.difference(&comparable).count();
    let capture_only = comparable.difference(export_identities).count();
    CompletenessReport {
        export_identities: export_identities.len(),
        matched_captures,
        export_only,
        capture_only,
        non_comparable_captures,
    }
}

fn split_capture_identities(capture_identities: Vec<Option<String>>) -> (BTreeSet<String>, usize) {
    capture_identities.into_iter().fold(
        (BTreeSet::new(), 0),
        |(mut comparable, non_comparable), identity| match identity {
            Some(identity) => {
                comparable.insert(identity);
                (comparable, non_comparable)
            }
            None => (comparable, non_comparable + 1),
        },
    )
}

/// Why an export archive was refused before parsing.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// Immutable service-owned raw evidence storage failed.
    #[error("raw Data Export evidence storage failed")]
    RawStorage(#[source] PublicResolutionError),
    /// A Data Export persistence query failed.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    /// The bytes are not a readable ZIP archive.
    #[error("export archive is not a readable ZIP")]
    InvalidZip(#[source] zip::result::ZipError),
    /// Archive content broke a named safety constraint.
    #[error("export archive violates {limit}: {detail}")]
    Limit {
        /// The violated named constraint.
        limit: &'static str,
        /// Safe structural detail.
        detail: String,
    },
    /// The safely inspected archive does not carry the supported export layout.
    #[error("export archive does not contain the supported Threads export layout")]
    UnsupportedLayout,
    /// A supported-layout manifest is not valid JSON.
    #[error("supported export manifest is invalid JSON")]
    InvalidManifest(#[source] serde_json::Error),
    /// The manifest declared a provider version this parser does not support.
    #[error("export version {0:?} is not supported")]
    UnsupportedVersion(String),
    /// A private extraction operation failed without exposing archive content.
    #[error("safe export extraction failed while {operation}")]
    Extraction {
        /// Safe operation class.
        operation: &'static str,
        /// The underlying local storage error.
        #[source]
        source: std::io::Error,
    },
}

/// Inspects ZIP metadata before any parser reads archive content.
///
/// # Errors
///
/// Returns [`ExportError::InvalidZip`] when the bytes do not contain a ZIP,
/// and later versions return a named [`ExportError::Limit`] for unsafe input.
#[path = "data_export_archive.rs"]
mod data_export_archive;
pub use data_export_archive::{extract_archive, inspect_archive, parse_export};
