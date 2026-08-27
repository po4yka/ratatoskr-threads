#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Domain library for the Ratatoskr Threads bounded context.
//!
//! The foundation owns process configuration, telemetry bootstrap, and
//! application of the first-version `threads_archive` schema. Account
//! connection, public resolution, and Data Export imports arrive with later
//! implementation plan items; explicit-capture intake lives in the `capture`
//! module.

/// Provenance semantics: the capability matrix, acquisition modes and their
/// authority ceilings, and the upstream-versus-preservation boundary.
pub mod capability;
/// Explicit capture intake: validated requests, stored capture records, and
/// truthful unavailability observations.
pub mod capture;
pub mod config;
/// The owned `PostgreSQL` pool and the embedded `threads_archive` schema.
pub mod database;
/// Privacy-safe observational linkage to completed Knowledge analyses.
pub mod knowledge;
/// Official Threads OAuth credentials and account capability discovery.
pub mod oauth;
pub mod permalink;
/// Supported public-resolution parsing and persistence.
pub mod public_resolution;
pub(crate) mod publishing;
/// The reply, quote, and repost edge contract with its open relation-kind
/// grammar aligned to the published social contracts.
pub mod relation;
/// Structured logs and the Prometheus registry.
pub mod telemetry;

pub use config::{AdminConfig, Config, ConfigError, Limits, StorageConfig, TelemetryConfig};
pub use database::{Database, PersistenceError};
pub use telemetry::{TelemetryError, TelemetryGuard, init_telemetry};

#[cfg(feature = "test-support")]
pub mod test_support;
