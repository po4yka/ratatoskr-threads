//! Completes accepted captures through the public oEmbed surface.
//!
//! A resolver pass claims the captures that are due, resolves each through the credential-free
//! oEmbed client, classifies every provider failure, retries transient ones on a bounded
//! schedule, and queues exactly one terminal operation report per Platform operation. The report
//! and the `operation_reported_at` guard that makes it exactly-once commit together, so a crash
//! between recording an observation and reporting it only reports on the next pass: a capture that
//! is already terminal is never fetched again.
//!
//! The cross-repository behaviour (report shape, retry schedule, exactly-once rule) is
//! XR-021 CONTRACTS.md section S10 (CD1, CD2, CD5, CD7).

use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, Utc};
use ratatoskr_identifiers::{OperationId, SocialSourceId};
use ratatoskr_social_contracts::{
    SocialContractError, SourceUnavailability, preserved_report, unavailable_report,
};
use reqwest::StatusCode;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::capture::{CaptureError, CaptureStore, UnavailabilityObservation};
use crate::capture_reporting::{ReportError, ReportTarget, append_report};
use crate::permalink::CanonicalizedUrl;
use crate::public_resolution::{
    ApprovedOembedClient, PARSER_VERSION, PublicResolutionError, PublicResolutionStore,
    RawObjectStore,
};
use crate::{Database, PersistenceError, publishing};

/// How long a claimed capture stays invisible to other passes before it is due again.
const CLAIM_LEASE_SECONDS: i64 = 120;
/// The wait after the first transient failure; each further failure waits four times as long.
const FIRST_RETRY_SECONDS: i64 = 30;
/// The longest wait between two attempts.
const LONGEST_RETRY_SECONDS: i64 = 30 * 60;

/// How the resolver treats one failed public resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// The provider says the post was deleted. Permanent.
    Deleted,
    /// The post is private, protected, unsupported or its response is unusable. Permanent.
    Inaccessible,
    /// The provider could not be reached or throttled the request. Retried on a schedule.
    Transient,
}

/// Classifies a public-resolution failure that came from the provider or the network.
///
/// A provider status of 404 or 410 is a deletion; 403 and every other client error is a permanent
/// refusal, except 408 and 429, which are retried like a server error. An unusable response (not
/// JSON, a missing field, another provider, another permalink) is permanent. Failures of this
/// service's own storage never reach the classifier: [`is_local_fault`] separates them first.
#[must_use]
pub fn classify(error: &PublicResolutionError) -> Resolution {
    match error {
        PublicResolutionError::ProviderStatus(status) => classify_status(*status),
        PublicResolutionError::Network | PublicResolutionError::ResponseTooLarge => {
            Resolution::Transient
        }
        PublicResolutionError::InvalidJson
        | PublicResolutionError::MissingField(_)
        | PublicResolutionError::WrongProvider
        | PublicResolutionError::PermalinkMismatch
        | PublicResolutionError::UnexpectedContentType
        | PublicResolutionError::RelationSourceMismatch
        | PublicResolutionError::TooManyRelations
        | PublicResolutionError::RelationGraph(_) => Resolution::Inaccessible,
        PublicResolutionError::UnsupportedEndpoint
        | PublicResolutionError::RawStorage(_)
        | PublicResolutionError::RawDigestMismatch
        | PublicResolutionError::Persistence(_) => Resolution::Transient,
    }
}

fn classify_status(status: StatusCode) -> Resolution {
    if status == StatusCode::NOT_FOUND || status == StatusCode::GONE {
        Resolution::Deleted
    } else if status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || !status.is_client_error()
    {
        Resolution::Transient
    } else {
        Resolution::Inaccessible
    }
}

/// Whether a failure came from this service's own storage or configuration rather than from the
/// provider. Such a failure is not evidence about the post, so it stops the resolver instead of
/// being counted against the capture.
#[must_use]
pub fn is_local_fault(error: &PublicResolutionError) -> bool {
    matches!(
        error,
        PublicResolutionError::UnsupportedEndpoint
            | PublicResolutionError::RawStorage(_)
            | PublicResolutionError::RawDigestMismatch
            | PublicResolutionError::Persistence(_)
    )
}

/// A short, content-free reason stored with a permanent observation.
fn reason_code(error: &PublicResolutionError) -> String {
    match error {
        PublicResolutionError::ProviderStatus(status) => {
            format!("provider_status_{}", status.as_u16())
        }
        PublicResolutionError::InvalidJson => "invalid_json".to_owned(),
        PublicResolutionError::MissingField(_) => "missing_field".to_owned(),
        PublicResolutionError::WrongProvider => "wrong_provider".to_owned(),
        PublicResolutionError::PermalinkMismatch => "permalink_mismatch".to_owned(),
        PublicResolutionError::UnexpectedContentType => "unexpected_content_type".to_owned(),
        _ => "unusable_response".to_owned(),
    }
}

/// The wait before the attempt that follows the `attempt`-th transient failure.
#[must_use]
pub fn retry_delay(attempt: u32) -> chrono::Duration {
    let factor = 4_i64.saturating_pow(attempt.saturating_sub(1));
    let seconds = FIRST_RETRY_SECONDS
        .saturating_mul(factor)
        .min(LONGEST_RETRY_SECONDS);
    chrono::Duration::seconds(seconds)
}

/// The bounded retry policy of the resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolverPolicy {
    /// Transient failures tolerated before the capture is reported unavailable.
    pub max_attempts: u32,
    /// Captures claimed per pass.
    pub batch_size: u32,
    /// Pause between passes of [`CaptureResolver::run`].
    pub poll_interval: Duration,
}

/// What one resolver pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunSummary {
    /// Captures claimed by the pass.
    pub claimed: usize,
    /// Terminal operation reports queued by the pass.
    pub reported: usize,
}

/// Why a resolver pass could not complete. Every variant is a fault of this service, never of the
/// post being resolved.
#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    /// An archive-owned query failed.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    /// Recording an observation failed.
    #[error(transparent)]
    Capture(#[from] CaptureError),
    /// This service's own storage or configuration failed while resolving.
    #[error("the resolver's own storage failed")]
    Local(#[source] PublicResolutionError),
    /// A terminal report did not satisfy its contract.
    #[error("the terminal operation report is not contract-valid")]
    Report(#[from] SocialContractError),
    /// A terminal report could not be rendered.
    #[error("the terminal operation report could not be serialized")]
    Serialization(#[from] serde_json::Error),
    /// A resolved capture has no stored social source to point its report at.
    #[error("resolved capture {0} has no stored social source")]
    MissingSource(Uuid),
}

impl From<ReportError> for ResolverError {
    fn from(error: ReportError) -> Self {
        match error {
            ReportError::Contract(error) => Self::Report(error),
            ReportError::Serialization(error) => Self::Serialization(error),
            ReportError::Persistence(error) => Self::Persistence(PersistenceError::Query(error)),
        }
    }
}

impl From<sqlx::Error> for ResolverError {
    fn from(error: sqlx::Error) -> Self {
        Self::Persistence(PersistenceError::Query(error))
    }
}

/// One claimed capture, in the column order of the claim query.
type DueRow = (Uuid, Uuid, Uuid, Uuid, String, String, i32);

#[derive(Debug, Clone)]
struct Due {
    capture_id: Uuid,
    user_ref: Uuid,
    operation_id: Uuid,
    command_id: Uuid,
    canonical_url: String,
    status: String,
    resolution_attempts: i32,
}

#[derive(Debug, Clone, Copy)]
enum Terminal {
    Preserved,
    Unavailable(SourceUnavailability),
}

enum Attempt {
    Terminal(Terminal),
    Retry,
}

/// Claims due captures, resolves them, and reports each operation exactly once.
#[derive(Debug)]
pub struct CaptureResolver {
    database: Database,
    client: ApprovedOembedClient,
    raw_objects: RawObjectStore,
    policy: ResolverPolicy,
}

impl CaptureResolver {
    /// Builds a resolver over the archive database.
    #[must_use]
    pub const fn new(
        database: Database,
        client: ApprovedOembedClient,
        raw_objects: RawObjectStore,
        policy: ResolverPolicy,
    ) -> Self {
        Self {
            database,
            client,
            raw_objects,
            policy,
        }
    }

    /// Runs passes every poll interval until `stop` resolves.
    ///
    /// A pass that has started always finishes before `stop` is looked at again.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError`] as soon as a pass fails: the caller restarts the resolver, and the
    /// claim lease makes that safe.
    pub async fn run(&self, stop: impl Future<Output = ()> + Send) -> Result<(), ResolverError> {
        tokio::pin!(stop);
        let mut ticker = tokio::time::interval(self.policy.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                () = &mut stop => return Ok(()),
                _ = ticker.tick() => { self.run_due_once(Utc::now()).await?; }
            }
        }
    }

    /// Runs one pass over the captures that are due at `now`.
    ///
    /// `now` is the instant every schedule is computed from; production passes the wall clock and
    /// tests pass an advancing one.
    ///
    /// # Errors
    ///
    /// Returns [`ResolverError`] when this service's own storage fails. A provider failure is
    /// never an error: it is classified, retried or reported.
    pub async fn run_due_once(&self, now: DateTime<Utc>) -> Result<RunSummary, ResolverError> {
        let due = self.claim(now).await?;
        let mut summary = RunSummary {
            claimed: due.len(),
            reported: 0,
        };
        for row in due {
            if self.process(&row, now).await? {
                summary.reported += 1;
            }
        }
        Ok(summary)
    }

    /// Takes the due captures under a lease so that no other pass picks them up meanwhile.
    async fn claim(&self, now: DateTime<Utc>) -> Result<Vec<Due>, ResolverError> {
        let mut transaction = self
            .database
            .pool()
            .begin()
            .await
            .map_err(PersistenceError::Query)?;
        let rows: Vec<DueRow> = sqlx::query_as(
            "select capture_id, user_ref, operation_id, command_id, canonical_url, status, \
                    resolution_attempts \
             from threads_archive.captures \
             where operation_id is not null and command_id is not null \
               and operation_reported_at is null and next_resolution_at <= $1 \
             order by next_resolution_at, capture_id limit $2 \
             for update skip locked",
        )
        .bind(now)
        .bind(i64::from(self.policy.batch_size))
        .fetch_all(&mut *transaction)
        .await
        .map_err(PersistenceError::Query)?;
        let due: Vec<Due> = rows
            .into_iter()
            .map(
                |(
                    capture_id,
                    user_ref,
                    operation_id,
                    command_id,
                    canonical_url,
                    status,
                    resolution_attempts,
                )| Due {
                    capture_id,
                    user_ref,
                    operation_id,
                    command_id,
                    canonical_url,
                    status,
                    resolution_attempts,
                },
            )
            .collect();
        if !due.is_empty() {
            let ids: Vec<Uuid> = due.iter().map(|row| row.capture_id).collect();
            sqlx::query(
                "update threads_archive.captures set next_resolution_at = $2 \
                 where capture_id = any($1)",
            )
            .bind(ids)
            .bind(now + chrono::Duration::seconds(CLAIM_LEASE_SECONDS))
            .execute(&mut *transaction)
            .await
            .map_err(PersistenceError::Query)?;
        }
        transaction
            .commit()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(due)
    }

    /// Advances one claimed capture, returning whether its terminal report was queued.
    async fn process(&self, row: &Due, now: DateTime<Utc>) -> Result<bool, ResolverError> {
        let terminal = match self.recorded_terminal(row).await? {
            Some(terminal) => terminal,
            None => match self.attempt(row, now).await? {
                Attempt::Terminal(terminal) => terminal,
                Attempt::Retry => return Ok(false),
            },
        };
        self.report(row, terminal).await
    }

    /// The outcome an earlier pass already recorded for this capture, if any.
    async fn recorded_terminal(&self, row: &Due) -> Result<Option<Terminal>, ResolverError> {
        match row.status.as_str() {
            "resolved" => Ok(Some(Terminal::Preserved)),
            "unavailable" | "failed" => {
                let availability: Option<String> = sqlx::query_scalar(
                    "select availability from threads_archive.tombstones \
                     where capture_id = $1 order by observed_at desc, tombstone_id desc limit 1",
                )
                .bind(row.capture_id)
                .fetch_optional(self.database.pool())
                .await?;
                let reason = if availability.as_deref() == Some("deleted") {
                    SourceUnavailability::Deleted
                } else {
                    SourceUnavailability::Inaccessible
                };
                Ok(Some(Terminal::Unavailable(reason)))
            }
            _ if u32::try_from(row.resolution_attempts)
                .is_ok_and(|attempts| attempts >= self.policy.max_attempts) =>
            {
                Ok(Some(Terminal::Unavailable(SourceUnavailability::Transient)))
            }
            _ => Ok(None),
        }
    }

    /// Fetches and records one capture, or schedules its next attempt.
    async fn attempt(&self, row: &Due, now: DateTime<Utc>) -> Result<Attempt, ResolverError> {
        let captures = CaptureStore::new(&self.database);
        let Ok(canonical) = CanonicalizedUrl::try_from(row.canonical_url.as_str()) else {
            captures
                .record_observation(
                    row.capture_id,
                    &UnavailabilityObservation::private_or_inaccessible(
                        "invalid_permalink".to_owned(),
                    )?,
                )
                .await?;
            return Ok(Attempt::Terminal(Terminal::Unavailable(
                SourceUnavailability::Inaccessible,
            )));
        };
        let store = PublicResolutionStore::new(&self.database, self.raw_objects.clone());
        let error = match self
            .client
            .resolve_and_record(&store, row.capture_id, canonical.permalink())
            .await
        {
            Ok(_) => return Ok(Attempt::Terminal(Terminal::Preserved)),
            Err(error) if is_local_fault(&error) => return Err(ResolverError::Local(error)),
            Err(error) => error,
        };
        match classify(&error) {
            Resolution::Deleted => {
                captures
                    .record_observation(
                        row.capture_id,
                        &UnavailabilityObservation::deleted(reason_code(&error))?,
                    )
                    .await?;
                Ok(Attempt::Terminal(Terminal::Unavailable(
                    SourceUnavailability::Deleted,
                )))
            }
            Resolution::Inaccessible => {
                captures
                    .record_observation(
                        row.capture_id,
                        &UnavailabilityObservation::private_or_inaccessible(reason_code(&error))?,
                    )
                    .await?;
                Ok(Attempt::Terminal(Terminal::Unavailable(
                    SourceUnavailability::Inaccessible,
                )))
            }
            Resolution::Transient => self.record_transient_failure(row, now).await,
        }
    }

    /// Stores a failed attempt as evidence and either schedules the next one or gives up.
    ///
    /// A resolver failure is not deletion evidence: the capture stays `accepted` and no tombstone
    /// is written, whichever attempt this was.
    async fn record_transient_failure(
        &self,
        row: &Due,
        now: DateTime<Utc>,
    ) -> Result<Attempt, ResolverError> {
        CaptureStore::new(&self.database)
            .record_observation(
                row.capture_id,
                &UnavailabilityObservation::resolver_failed(PARSER_VERSION.to_owned())?,
            )
            .await?;
        let attempts = row.resolution_attempts.saturating_add(1);
        let attempt = u32::try_from(attempts).unwrap_or(u32::MAX);
        sqlx::query(
            "update threads_archive.captures \
             set resolution_attempts = $2, next_resolution_at = $3 where capture_id = $1",
        )
        .bind(row.capture_id)
        .bind(attempts)
        .bind(now + retry_delay(attempt))
        .execute(self.database.pool())
        .await?;
        if attempt >= self.policy.max_attempts {
            Ok(Attempt::Terminal(Terminal::Unavailable(
                SourceUnavailability::Transient,
            )))
        } else {
            Ok(Attempt::Retry)
        }
    }

    /// Queues the terminal report and sets the exactly-once guard in one transaction.
    ///
    /// Returns `false` when another pass already reported this operation.
    async fn report(&self, row: &Due, terminal: Terminal) -> Result<bool, ResolverError> {
        let mut transaction = self
            .database
            .pool()
            .begin()
            .await
            .map_err(PersistenceError::Query)?;
        let guarded: Option<Option<Uuid>> = sqlx::query_scalar(
            "update threads_archive.captures \
             set operation_reported_at = now(), next_resolution_at = null \
             where capture_id = $1 and operation_reported_at is null returning post_id",
        )
        .bind(row.capture_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(post_id) = guarded else {
            return Ok(false);
        };
        let operation = OperationId(row.operation_id);
        let report = match terminal {
            Terminal::Preserved => {
                let source = self.stored_source(&mut transaction, row, post_id).await?;
                preserved_report(operation, source)
            }
            Terminal::Unavailable(reason) => unavailable_report(operation, reason),
        }?;
        let target = ReportTarget {
            owner: row.user_ref,
            operation_id: operation,
            command_id: row.command_id,
            capture_id: row.capture_id,
        };
        append_report(&mut transaction, &target, &report).await?;
        transaction
            .commit()
            .await
            .map_err(PersistenceError::Query)?;
        Ok(true)
    }

    async fn stored_source(
        &self,
        connection: &mut PgConnection,
        row: &Due,
        post_id: Option<Uuid>,
    ) -> Result<SocialSourceId, ResolverError> {
        let missing = || ResolverError::MissingSource(row.capture_id);
        let post_id = post_id.ok_or_else(missing)?;
        let source = publishing::social_source_id_for(connection, row.user_ref, post_id)
            .await?
            .ok_or_else(missing)?;
        SocialSourceId::parse(&source.to_string()).map_err(|_| missing())
    }
}
