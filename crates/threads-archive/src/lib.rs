#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Domain library for the Ratatoskr Threads bounded context.
//!
//! The foundation owns process configuration, telemetry bootstrap, and
//! application of the first-version `threads_archive` schema. Account
//! connection, explicit captures, public resolution, and Data Export imports
//! arrive with later implementation plan items.

/// Provenance semantics: the capability matrix, acquisition modes and their
/// authority ceilings, and the upstream-versus-preservation boundary.
pub mod capability;
pub mod config;
/// The owned `PostgreSQL` pool and the embedded `threads_archive` schema.
pub mod database;
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
