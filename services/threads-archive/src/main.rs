#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Ratatoskr Threads Archive service process.
//!
//! Sequence, in this order and no other: load configuration, install
//! telemetry, refuse to start without a database, connect, apply the schema,
//! bind the operator listener, mark readiness — then serve until SIGTERM or
//! SIGINT and drain within the configured bound.
//!
//! Exit codes: `0` clean run; `1` runtime startup failure; `78`
//! (`EX_CONFIG`) configuration unreadable or invalid.

use std::future::{Future, IntoFuture as _};
use std::io::Write as _;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use secrecy::ExposeSecret as _;

use ratatoskr_threads_archive::capture_resolution::{CaptureResolver, ResolverPolicy};
use ratatoskr_threads_archive::data_export_reprocessing::{
    ReprocessClassification, ReprocessInput, ReprocessReport, ReprocessingStore,
    SUPPORTED_REPROCESSING_PARSER,
};
use ratatoskr_threads_archive::deletion_acks;
use ratatoskr_threads_archive::nats::{self, NatsConnection};
use ratatoskr_threads_archive::public_resolution::{ApprovedOembedClient, RawObjectStore};
use ratatoskr_threads_archive::removal_acknowledgement::ResendPolicy;
use ratatoskr_threads_archive::telemetry::SERVICE_NAME;
use ratatoskr_threads_archive::{Config, Database, PublicResolutionConfig};
use ratatoskr_threads_archive_service::RuntimeState;
use ratatoskr_threads_archive_service::workers::{self, Worker, WorkerKind};
use uuid::Uuid;

/// How often the prober copies the database answer into the readiness facts.
///
/// Long enough that the probe is not itself load; short enough that a
/// readiness state is never more than one scrape interval stale.
const DATABASE_PROBE_INTERVAL: Duration = Duration::from_secs(5);

/// How long a worker may take to stop on an orderly shutdown.
const WORKER_STOP_GRACE: Duration = Duration::from_secs(5);

/// How long the operator plane keeps answering after a worker returned early, so that a probe or a
/// scrape observes the failed readiness before the process exits non-zero.
const FAILURE_LINGER: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| argument == "reprocess-export")
    {
        return reprocess_export(&arguments);
    }
    if std::env::args().nth(1).as_deref() == Some("check-config") {
        return check_config();
    }
    match tokio_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(exit) => exit,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReprocessMode {
    DryRun,
    Apply,
}

#[derive(Debug, Clone, Copy)]
struct ReprocessCommand {
    mode: ReprocessMode,
    owner: Uuid,
    export_run_id: Uuid,
    operation_id: Option<Uuid>,
}

fn reprocess_export(arguments: &[String]) -> ExitCode {
    let command = match parse_reprocess_command(arguments) {
        Ok(command) => command,
        Err(message) => {
            eprintln!(
                "{SERVICE_NAME}: {message}\nusage: ratatoskr-threads-archive reprocess-export dry-run --owner UUID --run-id UUID --parser TOKEN\n       ratatoskr-threads-archive reprocess-export apply --owner UUID --run-id UUID --parser TOKEN --operation-id UUID"
            );
            return ExitCode::from(2);
        }
    };
    let config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{SERVICE_NAME}: {error}");
            return ExitCode::from(78);
        }
    };
    let Some(database_url) = config.storage.database_url.as_ref() else {
        eprintln!("{SERVICE_NAME}: reprocess-export requires RATATOSKR__STORAGE__DATABASE_URL");
        return ExitCode::from(78);
    };
    run_reprocess_export(command, database_url.expose_secret())
}

fn parse_reprocess_command(arguments: &[String]) -> Result<ReprocessCommand, &'static str> {
    if arguments.first().map(String::as_str) != Some("reprocess-export") {
        return Err("invalid reprocess-export command");
    }
    let mode = match arguments.get(1).map(String::as_str) {
        Some("dry-run") => ReprocessMode::DryRun,
        Some("apply") => ReprocessMode::Apply,
        _ => return Err("mode must be dry-run or apply"),
    };
    let mut owner = None;
    let mut export_run_id = None;
    let mut parser = None;
    let mut operation_id = None;
    let mut index = 2;
    while index < arguments.len() {
        let flag = arguments
            .get(index)
            .map(String::as_str)
            .ok_or("invalid reprocess-export flag")?;
        let value = arguments
            .get(index + 1)
            .ok_or("every flag requires one value")?;
        match flag {
            "--owner" if owner.is_none() => {
                owner = Some(value.parse().map_err(|_| "--owner must be a UUID")?);
            }
            "--run-id" if export_run_id.is_none() => {
                export_run_id = Some(value.parse().map_err(|_| "--run-id must be a UUID")?);
            }
            "--parser" if parser.is_none() => parser = Some(value.as_str()),
            "--operation-id" if operation_id.is_none() => {
                operation_id = Some(value.parse().map_err(|_| "--operation-id must be a UUID")?);
            }
            _ => return Err("unknown or duplicate reprocess-export flag"),
        }
        index += 2;
    }
    if parser != Some(SUPPORTED_REPROCESSING_PARSER) {
        return Err("--parser is not registered");
    }
    if mode == ReprocessMode::DryRun && operation_id.is_some() {
        return Err("dry-run does not accept --operation-id");
    }
    if mode == ReprocessMode::Apply && operation_id.is_none() {
        return Err("apply requires --operation-id");
    }
    Ok(ReprocessCommand {
        mode,
        owner: owner.ok_or("--owner is required")?,
        export_run_id: export_run_id.ok_or("--run-id is required")?,
        operation_id,
    })
}

#[tokio::main]
async fn run_reprocess_export(command: ReprocessCommand, database_url: &str) -> ExitCode {
    let database = match Database::connect(database_url, 2, Duration::from_secs(5)).await {
        Ok(database) => database,
        Err(error) => {
            eprintln!("{SERVICE_NAME}: reprocess-export database connection failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = database.apply_schema().await {
        eprintln!("{SERVICE_NAME}: reprocess-export schema check failed: {error}");
        return ExitCode::FAILURE;
    }
    let (inputs, state_fingerprint) = match load_reprocessing_inputs(&database, command).await {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{SERVICE_NAME}: reprocess-export receipt load failed: {error}");
            database.close().await;
            return ExitCode::FAILURE;
        }
    };
    let store = ReprocessingStore::new(&database);
    let rendered = match command.mode {
        ReprocessMode::DryRun => match store
            .dry_run(
                command.owner,
                command.export_run_id,
                &inputs,
                &state_fingerprint,
            )
            .await
        {
            Ok(report) => render_cli_report("dry-run", command, &report, None),
            Err(error) => {
                eprintln!("{SERVICE_NAME}: reprocess-export dry-run failed: {error}");
                database.close().await;
                return ExitCode::FAILURE;
            }
        },
        ReprocessMode::Apply => match store
            .apply_chunk(
                command.owner,
                command.export_run_id,
                command.operation_id.unwrap_or(Uuid::nil()),
                &inputs,
                &state_fingerprint,
                usize::MAX,
            )
            .await
        {
            Ok(outcome) => render_cli_report(
                "apply",
                command,
                &outcome.report,
                Some((outcome.reprocessing_run_id, outcome.completed)),
            ),
            Err(error) => {
                eprintln!("{SERVICE_NAME}: reprocess-export apply failed: {error}");
                database.close().await;
                return ExitCode::FAILURE;
            }
        },
    };
    database.close().await;
    match write_json_stdout(&rendered) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{SERVICE_NAME}: reprocess-export stdout failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn load_reprocessing_inputs(
    database: &Database,
    command: ReprocessCommand,
) -> Result<(Vec<ReprocessInput>, String), sqlx::Error> {
    let archive_hash: Vec<u8> = sqlx::query_scalar(
        "select archive_hash from threads_archive.export_runs \
         where run_id = $1 and user_ref = $2 and detected_version is not null",
    )
    .bind(command.export_run_id)
    .bind(command.owner)
    .fetch_one(database.pool())
    .await?;
    let records: Vec<(Uuid, String)> = sqlx::query_as(
        "select record_id, record_kind from threads_archive.export_records \
         where run_id = $1 order by record_id",
    )
    .bind(command.export_run_id)
    .fetch_all(database.pool())
    .await?;
    let inputs = records
        .into_iter()
        .map(|(record_id, record_kind)| {
            Ok(ReprocessInput {
                item_key: record_id.to_string(),
                classification: classification(&record_kind)?,
                prospective_digest: None,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    let state_fingerprint = archive_hash.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
        output
    });
    if state_fingerprint.len() != 64 {
        return Err(sqlx::Error::Protocol(
            "archive hash is not a SHA-256 digest".to_owned(),
        ));
    }
    Ok((inputs, state_fingerprint))
}

fn classification(value: &str) -> Result<ReprocessClassification, sqlx::Error> {
    let classification = match value {
        "normalized" => ReprocessClassification::Normalized,
        "unknown_record" => ReprocessClassification::UnknownRecord,
        "unknown_section" => ReprocessClassification::UnknownSection,
        "conflict" => ReprocessClassification::Conflict,
        "warning" => ReprocessClassification::Warning,
        _ => {
            return Err(sqlx::Error::Protocol(
                "unregistered retained export record kind".to_owned(),
            ));
        }
    };
    Ok(classification)
}

fn render_cli_report(
    mode: &str,
    command: ReprocessCommand,
    report: &ReprocessReport,
    applied: Option<(Uuid, bool)>,
) -> serde_json::Value {
    let items = report
        .items
        .iter()
        .map(|item| {
            serde_json::json!({
                "item_key": item.item_key,
                "classification": item.classification.wire_name(),
                "digest": item.digest,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "mode": mode,
        "owner": command.owner,
        "export_run_id": command.export_run_id,
        "parser_version": SUPPORTED_REPROCESSING_PARSER,
        "operation_id": command.operation_id,
        "reprocessing_run_id": applied.map(|value| value.0),
        "completed": applied.map(|value| value.1),
        "report": {
            "items": items,
            "counts": report.counts,
            "warnings": report.warnings,
            "conflicts": report.conflicts,
            "plan_fingerprint": report.plan_fingerprint,
            "state_fingerprint": report.state_fingerprint,
        }
    })
}

fn write_json_stdout(value: &serde_json::Value) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value).map_err(std::io::Error::other)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

/// `<binary> check-config`: load and validate without binding anything.
///
/// Both outputs go to stderr: no subscriber exists yet, and a stray line on
/// stdout could be mistaken for a log record. The effective configuration is
/// safe to render because every secret member is redacted by type.
fn check_config() -> ExitCode {
    match Config::load() {
        Ok(config) => {
            eprintln!("{SERVICE_NAME}: configuration is valid.\n{config:#?}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{SERVICE_NAME}: {error}");
            ExitCode::from(78)
        }
    }
}

#[tokio::main]
async fn tokio_main() -> Result<(), ExitCode> {
    let config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{SERVICE_NAME}: {error}");
            return Err(ExitCode::from(78));
        }
    };

    let guard = match ratatoskr_threads_archive::init_telemetry(&config.telemetry) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("{SERVICE_NAME}: refusing to start; telemetry failed: {error}");
            return Err(ExitCode::FAILURE);
        }
    };
    tracing::info!(
        service_name = SERVICE_NAME,
        version = ratatoskr_threads_archive::telemetry::VERSION,
        git_sha = ratatoskr_threads_archive::telemetry::GIT_SHA,
        config = ?config,
        "startup"
    );

    // Refusing to start without a database is deliberate: every capability
    // this binary will ever offer reads the archive database, and a process
    // that started anyway would report itself ready and fail everything.
    let Some(database_url) = config.storage.database_url.as_ref() else {
        eprintln!("{SERVICE_NAME}: refusing to start without RATATOSKR__STORAGE__DATABASE_URL");
        return Err(ExitCode::FAILURE);
    };

    let database = Database::connect(
        database_url.expose_secret(),
        config.limits.database_connections,
        Duration::from_millis(config.limits.database_acquire_timeout_ms),
    )
    .await
    .map_err(|error| {
        tracing::error!(%error, "the database could not be reached");
        ExitCode::FAILURE
    })?;
    database.apply_schema().await.map_err(|error| {
        tracing::error!(%error, "the schema could not be applied");
        ExitCode::FAILURE
    })?;

    let connection = connect_bus(&config.bus).await?;
    let mut bus = spawn_nats_consumer(
        connection.clone(),
        database.clone(),
        config.tombstone.resend_policy(),
    )
    .await?;
    let mut acks = spawn_deletion_acks(connection, database.clone()).await?;
    let mut resolver = spawn_capture_resolver(&config.public_resolution, database.clone()).await?;

    let runtime = Arc::new(RuntimeState::new());
    let listener = tokio::net::TcpListener::bind(config.admin.listen_address)
        .await
        .map_err(|error| {
            tracing::error!(
                bind = %config.admin.listen_address,
                %error,
                "the operator listener could not bind"
            );
            ExitCode::FAILURE
        })?;

    // The first probe happens before readiness flips, so the process never
    // reports itself ready over an unverified dependency.
    let prober = spawn_database_prober(database.clone(), Arc::clone(&runtime));
    runtime.set_bus_ready(true);
    runtime.set_deletion_acks_ready(true);
    runtime.set_resolver_ready(true);
    runtime.mark_startup_complete();
    tracing::info!(admin = %config.admin.listen_address, "startup complete");

    let metrics_handle = guard.metrics_handle();
    let serve_result = serve_admin(
        listener,
        Arc::clone(&runtime),
        database,
        move || metrics_handle.render(),
        Duration::from_millis(config.limits.shutdown_timeout_ms),
        async {
            tokio::select! {
                kind = bus.exited() => kind,
                kind = acks.exited() => kind,
                kind = resolver.exited() => kind,
            }
        },
    )
    .await;

    prober.abort();
    resolver.stop(WORKER_STOP_GRACE).await;
    acks.stop(WORKER_STOP_GRACE).await;
    bus.stop(WORKER_STOP_GRACE).await;

    match serve_result {
        Ok(()) => {
            guard.shutdown();
            Ok(())
        }
        Err(error) => {
            tracing::error!(%error, "the service stopped abnormally");
            Err(ExitCode::FAILURE)
        }
    }
}

/// Connects the one broker identity that both bus workers share.
async fn connect_bus(
    bus: &ratatoskr_threads_archive::BusConfig,
) -> Result<NatsConnection, ExitCode> {
    match bus.nkey_seed_path.as_deref() {
        Some(seed_path) => NatsConnection::connect_with_nkey(&bus.url, seed_path).await,
        None => NatsConnection::connect(&bus.url).await,
    }
    .map_err(|error| {
        tracing::error!(%error, "the NATS connection could not be established");
        ExitCode::FAILURE
    })
}

async fn spawn_nats_consumer(
    connection: NatsConnection,
    database: Database,
    resend: ResendPolicy,
) -> Result<Worker, ExitCode> {
    nats::ensure_command_consumer(&connection)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the NATS command consumer is not deployable");
            ExitCode::FAILURE
        })?;
    Ok(workers::spawn(WorkerKind::Bus, move |stop| async move {
        nats::run(&connection, &database, resend, stop).await
    }))
}

/// Starts the consumer of Knowledge's deletion acknowledgements after verifying that Edge
/// provisioned its durable as specified.
async fn spawn_deletion_acks(
    connection: NatsConnection,
    database: Database,
) -> Result<Worker, ExitCode> {
    deletion_acks::ensure_acks_consumer(&connection)
        .await
        .map_err(|error| {
            tracing::error!(%error, "the deletion-acknowledgement consumer is not deployable");
            ExitCode::FAILURE
        })?;
    Ok(workers::spawn(
        WorkerKind::DeletionAcks,
        move |stop| async move {
            deletion_acks::consume_deletion_acks(&connection, &database, stop)
                .await
                .map(|_summary| ())
        },
    ))
}

/// Starts the capture resolver that completes accepted captures.
///
/// The configuration guarantees the raw evidence root whenever the bus is configured, so a missing
/// one here is a refusal to start rather than a silent no-op.
async fn spawn_capture_resolver(
    config: &PublicResolutionConfig,
    database: Database,
) -> Result<Worker, ExitCode> {
    let Some(root) = config.raw_object_root.clone() else {
        eprintln!(
            "{SERVICE_NAME}: refusing to start without RATATOSKR__PUBLIC_RESOLUTION__RAW_OBJECT_ROOT"
        );
        return Err(ExitCode::FAILURE);
    };
    tokio::fs::create_dir_all(&root).await.map_err(|error| {
        tracing::error!(%error, "the raw evidence directory could not be created");
        ExitCode::FAILURE
    })?;
    let client = ApprovedOembedClient::new(&config.oembed_endpoint).map_err(|error| {
        tracing::error!(%error, "the public oEmbed client could not be built");
        ExitCode::FAILURE
    })?;
    let resolver = CaptureResolver::new(
        database,
        client,
        RawObjectStore::new(root),
        ResolverPolicy {
            max_attempts: config.max_attempts,
            batch_size: config.batch_size,
            poll_interval: Duration::from_millis(config.poll_interval_ms),
        },
    );
    Ok(workers::spawn(
        WorkerKind::Resolver,
        move |stop| async move { resolver.run(stop).await },
    ))
}

/// Serves the operator plane until a shutdown signal, a server failure, or a background worker
/// returning early; the last of these is an error.
async fn serve_admin(
    listener: tokio::net::TcpListener,
    runtime: Arc<RuntimeState>,
    database: Database,
    render_metrics: impl Fn() -> String + Send + Sync + 'static,
    shutdown_timeout: Duration,
    worker_exit: impl Future<Output = WorkerKind>,
) -> Result<(), String> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(
        listener,
        ratatoskr_threads_archive_service::admin_router(runtime.clone(), render_metrics),
    )
    .with_graceful_shutdown(async move {
        let _ignored = shutdown_rx.await;
    })
    .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => {
            database.close().await;
            result.map_err(|error| error.to_string())
        }
        result = shutdown_signal() => {
            result.map_err(|error| error.to_string())?;
            // Readiness fails immediately; the listener stays open through
            // the drain window so in-flight requests finish.
            runtime.begin_draining();
            let _ignored = shutdown_tx.send(());
            if tokio::time::timeout(shutdown_timeout, &mut server).await.is_err() {
                database.close().await;
                return Err("the operator server did not stop within the shutdown bound".to_owned());
            }
            database.close().await;
            Ok(())
        }
        kind = worker_exit => {
            tracing::error!(worker = %kind, "a background worker returned before shutdown; reporting not ready and exiting");
            runtime.worker_stopped(kind);
            // The failed readiness is the signal an operator or load balancer acts on, so keep
            // answering it for a moment before the process leaves.
            let still_serving = tokio::time::timeout(FAILURE_LINGER, &mut server).await.is_err();
            if still_serving {
                runtime.begin_draining();
                let _ignored = shutdown_tx.send(());
                let _ignored = tokio::time::timeout(shutdown_timeout, &mut server).await;
            }
            database.close().await;
            Err(format!("the {kind} worker returned before shutdown"))
        }
    }
}

/// Copies the database answer into readiness forever.
///
/// A separate loop because it answers a different question at a different
/// cadence than any request: this keeps `/health/ready` honest while adding
/// almost no load — one `select 1` per interval.
fn spawn_database_prober(
    database: Database,
    runtime: Arc<RuntimeState>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(DATABASE_PROBE_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            runtime.set_database_reachable(database.ping().await.is_ok());
        }
    })
}

#[cfg(unix)]
async fn shutdown_signal() -> std::io::Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = terminate.recv() => Ok(()),
        result = tokio::signal::ctrl_c() => result,
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}
