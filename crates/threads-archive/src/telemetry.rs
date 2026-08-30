//! Structured telemetry: the JSON log pipeline and the Prometheus registry.
//!
//! Installed exactly once per process. A second installation attempt is a
//! refusal, not a reset: two subscribers or two recorders would split every
//! observation after startup.

use std::sync::{Arc, Mutex};

use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::util::SubscriberInitExt as _;

use crate::config::TelemetryConfig;

/// The one wire identity of this bounded context.
pub const SERVICE_NAME: &str = "ratatoskr-threads";

/// The deployable role this binary serves. One process, one role.
pub const ROLE: &str = "archive";

/// The crate version, compiled in.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The build's git commit, provided by the container build, or `unknown`
/// outside one — the first thing anyone checks when a deployment misbehaves.
pub const GIT_SHA: &str = match option_env!("RATATOSKR_GIT_SHA") {
    Some(sha) => sha,
    None => "unknown",
};

/// The declared toolchain.
pub const RUST_VERSION: &str = env!("CARGO_PKG_RUST_VERSION");

/// The build-identity gauge: one series, labelled with the compiled identity.
const BUILD_INFO_METRIC: &str = "threads_build_info";

/// One bounded lifecycle metric and its low-cardinality label keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleMetricDescriptor {
    /// Stable Prometheus metric name.
    pub name: &'static str,
    /// Closed label keys; values are closed operation/outcome vocabularies.
    pub labels: &'static [&'static str],
}

/// Returns the complete lifecycle metric surface.
#[must_use]
pub const fn lifecycle_metric_descriptors() -> &'static [LifecycleMetricDescriptor] {
    &[
        LifecycleMetricDescriptor {
            name: "threads_media_admission_total",
            labels: &["outcome", "reason"],
        },
        LifecycleMetricDescriptor {
            name: "threads_deletion_operations_total",
            labels: &["target", "outcome"],
        },
        LifecycleMetricDescriptor {
            name: "threads_blob_deletion_attempts_total",
            labels: &["outcome", "failure_class"],
        },
        LifecycleMetricDescriptor {
            name: "threads_reresolution_attempts_total",
            labels: &["outcome", "reason"],
        },
        LifecycleMetricDescriptor {
            name: "threads_reresolution_duration_seconds",
            labels: &["outcome"],
        },
        LifecycleMetricDescriptor {
            name: "threads_export_reprocessing_total",
            labels: &["mode", "outcome"],
        },
        LifecycleMetricDescriptor {
            name: "threads_export_reprocessing_duration_seconds",
            labels: &["mode", "outcome"],
        },
        LifecycleMetricDescriptor {
            name: "threads_outbox_pending",
            labels: &[],
        },
        LifecycleMetricDescriptor {
            name: "threads_outbox_failed_total",
            labels: &["failure_class", "terminal"],
        },
        LifecycleMetricDescriptor {
            name: "threads_outbox_redelivered_total",
            labels: &[],
        },
        LifecycleMetricDescriptor {
            name: "threads_outbox_dead_lettered",
            labels: &[],
        },
    ]
}

/// Closed, content-free outbox failure classes used in durable evidence and telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxFailureClass {
    /// The stored event type has no permitted Threads publication subject.
    UnsupportedEventType,
    /// The stored envelope cannot be encoded as wire JSON.
    PayloadEncodingFailed,
    /// The stored envelope identity or type disagrees with its row.
    InvalidOutboxEnvelope,
    /// The broker did not acknowledge the publication.
    BrokerUnacknowledged,
}

impl OutboxFailureClass {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedEventType => "unsupported_event_type",
            Self::PayloadEncodingFailed => "payload_encoding_failed",
            Self::InvalidOutboxEnvelope => "invalid_outbox_envelope",
            Self::BrokerUnacknowledged => "broker_unacknowledged",
        }
    }
}

/// Records one failed outbox outcome using only closed, content-free labels.
pub(crate) fn record_outbox_failure(failure_class: OutboxFailureClass, terminal: bool) {
    let failure_class = failure_class.as_str();
    counter!(
        "threads_outbox_failed_total",
        "failure_class" => failure_class,
        "terminal" => if terminal { "true" } else { "false" },
    )
    .increment(1);
    tracing::warn!(failure_class, terminal, "Threads outbox publication failed");
}

/// Records an acknowledged event that succeeded after at least one failed attempt.
pub(crate) fn record_outbox_redelivery() {
    counter!("threads_outbox_redelivered_total").increment(1);
}

/// Updates the process view of retained pending and terminal outbox rows.
pub(crate) fn record_outbox_depth(pending: i64, dead_lettered: i64) {
    gauge!("threads_outbox_pending").set(u32::try_from(pending).unwrap_or(u32::MAX));
    gauge!("threads_outbox_dead_lettered").set(u32::try_from(dead_lettered).unwrap_or(u32::MAX));
}

/// Closed lifecycle operation vocabulary used as metric label values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleOperation {
    /// Provider-media policy admission.
    MediaAdmission,
    /// Capture-target owner deletion.
    CaptureDeletion,
    /// Connection-target owner deletion.
    ConnectionDeletion,
    /// Digest-verified `BlobStore` cleanup.
    BlobDeletion,
    /// Public capture re-resolution.
    ReResolution,
    /// Read-only retained-export reprocessing.
    ExportDryRun,
    /// Mutating retained-export reprocessing.
    ExportApply,
}

/// Closed lifecycle outcome vocabulary used as metric label values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleOutcome {
    /// Work was admitted or started.
    Admitted,
    /// Policy kept only metadata.
    MetadataOnly,
    /// Work reached its terminal successful state.
    Complete,
    /// Durable external cleanup remains pending.
    Pending,
    /// A finite/privacy guard skipped work before I/O.
    Skipped,
    /// A safe operational failure occurred.
    Failed,
}

impl LifecycleOutcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::MetadataOnly => "metadata_only",
            Self::Complete => "complete",
            Self::Pending => "pending",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
}

/// Records one bounded lifecycle outcome without identifiers or content labels.
pub fn record_lifecycle_outcome(
    operation: LifecycleOperation,
    outcome: LifecycleOutcome,
    duration_seconds: Option<f64>,
) {
    let outcome = outcome.as_str();
    match operation {
        LifecycleOperation::MediaAdmission => {
            counter!("threads_media_admission_total", "outcome" => outcome, "reason" => "bounded")
                .increment(1);
        }
        LifecycleOperation::CaptureDeletion | LifecycleOperation::ConnectionDeletion => {
            let target = if operation == LifecycleOperation::CaptureDeletion {
                "capture"
            } else {
                "connection"
            };
            counter!("threads_deletion_operations_total", "target" => target, "outcome" => outcome)
                .increment(1);
        }
        LifecycleOperation::BlobDeletion => {
            counter!("threads_blob_deletion_attempts_total", "outcome" => outcome, "failure_class" => "bounded")
                .increment(1);
        }
        LifecycleOperation::ReResolution => {
            counter!("threads_reresolution_attempts_total", "outcome" => outcome, "reason" => "bounded")
                .increment(1);
            if let Some(duration) = duration_seconds {
                histogram!("threads_reresolution_duration_seconds", "outcome" => outcome)
                    .record(duration);
            }
        }
        LifecycleOperation::ExportDryRun | LifecycleOperation::ExportApply => {
            let mode = if operation == LifecycleOperation::ExportDryRun {
                "dry_run"
            } else {
                "apply"
            };
            counter!("threads_export_reprocessing_total", "mode" => mode, "outcome" => outcome)
                .increment(1);
            if let Some(duration) = duration_seconds {
                histogram!("threads_export_reprocessing_duration_seconds", "mode" => mode, "outcome" => outcome)
                    .record(duration);
            }
        }
    }
}

/// Telemetry bootstrap failure.
#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    /// The configured filter expression did not parse.
    #[error("telemetry could not be initialized: the log filter is invalid")]
    LogFilter(#[source] tracing_subscriber::filter::ParseError),
    /// The Prometheus recorder could not be installed.
    #[error("telemetry could not be initialized: the metrics recorder refused installation")]
    MetricsRecorder(#[source] metrics_exporter_prometheus::BuildError),
    /// A global subscriber is already installed; two subscribers would split
    /// every observation after startup.
    #[error("telemetry is already initialized")]
    AlreadyInstalled(#[source] tracing_subscriber::util::TryInitError),
}

/// Owns the telemetry runtime for the life of the process.
#[derive(Debug)]
pub struct TelemetryGuard {
    /// The text exposition renderer of the installed recorder.
    pub(crate) metrics_handle: PrometheusHandle,
}

impl TelemetryGuard {
    /// A cloneable renderer of the installed recorder, handed to whatever
    /// surface serves the exposition text.
    #[must_use]
    pub fn metrics_handle(&self) -> PrometheusHandle {
        self.metrics_handle.clone()
    }

    /// Releases telemetry resources before exit.
    pub fn shutdown(self) {}
}

/// Installs the process-wide structured telemetry once.
///
/// # Errors
///
/// Returns [`TelemetryError`] when the filter expression is invalid, a global
/// subscriber is already installed, or the Prometheus recorder cannot be
/// installed.
pub fn init_telemetry(config: &TelemetryConfig) -> Result<TelemetryGuard, TelemetryError> {
    let filter = EnvFilter::try_new(&config.log_filter).map_err(TelemetryError::LogFilter)?;

    // The subscriber goes first, so a second initialization attempt is
    // refused as [`TelemetryError::AlreadyInstalled`] before anything else
    // becomes global. A failure after this point still aborts startup — none
    // of these installations is recoverable and no listener has bound yet.
    json_subscriber(filter, std::io::stderr)
        .try_init()
        .map_err(TelemetryError::AlreadyInstalled)?;

    let metrics_handle = PrometheusBuilder::new()
        .install_recorder()
        .map_err(TelemetryError::MetricsRecorder)?;
    gauge!(BUILD_INFO_METRIC,
        "service" => SERVICE_NAME,
        "role" => ROLE,
        "version" => VERSION,
        "git_sha" => GIT_SHA,
        "rust_version" => RUST_VERSION,
    )
    .set(1.0);

    Ok(TelemetryGuard { metrics_handle })
}

/// Renders the startup identity record through the production JSON formatter
/// into a string.
///
/// Exists so contract tests can parse exactly what an operator's first log
/// line looks like without scraping the process's stderr. It touches no
/// global state: the subscriber is thread-local for the duration of one emit.
#[must_use]
pub fn render_startup_record() -> String {
    let buffer = RecordBuffer(Arc::new(Mutex::new(Vec::new())));
    emit_startup_record(json_subscriber(EnvFilter::new("info"), buffer.clone()));
    buffer.snapshot()
}

/// Renders the production outbox failure record for privacy contract tests.
#[must_use]
pub fn render_outbox_failure_record() -> String {
    let buffer = RecordBuffer(Arc::new(Mutex::new(Vec::new())));
    let subscriber = json_subscriber(EnvFilter::new("warn"), buffer.clone());
    tracing::subscriber::with_default(subscriber, || {
        record_outbox_failure(OutboxFailureClass::BrokerUnacknowledged, false);
    });
    buffer.snapshot()
}

/// One JSON formatter configuration, shared by the global install and the
/// contract-test renderer so both produce byte-identical record shapes.
fn json_subscriber<W>(filter: EnvFilter, writer: W) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_writer(writer)
        .finish()
}

/// Emits the startup identity record through `subscriber`.
fn emit_startup_record<S>(subscriber: S)
where
    S: tracing::Subscriber + Send + Sync + 'static,
{
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            service_name = SERVICE_NAME,
            version = VERSION,
            git_sha = GIT_SHA,
            "startup"
        );
    });
}

/// A shared in-memory writer capturing what the formatter produces.
#[derive(Clone)]
struct RecordBuffer(Arc<Mutex<Vec<u8>>>);

impl RecordBuffer {
    fn snapshot(&self) -> String {
        // A poisoned mutex means a writer panicked mid-record; the bytes
        // written so far are still the honest answer to render.
        let guard = match self.0.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        String::from_utf8_lossy(&guard).into_owned()
    }
}

struct RecordBufferWriter<'a>(&'a RecordBuffer);

impl std::io::Write for RecordBufferWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = match self.0.0.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for RecordBuffer {
    type Writer = RecordBufferWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        RecordBufferWriter(self)
    }
}
