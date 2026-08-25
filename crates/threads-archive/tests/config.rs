//! Configuration strictness: the executable form of the service-runtime spec.

use secrecy::ExposeSecret as _;

use ratatoskr_threads_archive::{Config, StorageConfig};

#[test]
fn empty_environment_yields_loopback_default_on_port_9084() {
    let config = Config::from_environment(Vec::<(String, String)>::new())
        .expect("an empty environment must be valid");

    assert_eq!(config.admin.listen_address.to_string(), "127.0.0.1:9084");
    assert!(config.storage.database_url.is_none());
    assert_eq!(config.limits.database_connections, 8);
    assert_eq!(config.limits.database_acquire_timeout_ms, 5_000);
    assert_eq!(config.limits.shutdown_timeout_ms, 10_000);
}

#[test]
fn unknown_prefixed_key_is_refused_without_echoing_value() {
    let error = Config::from_environment([("RATATOSKR__NOT_A_SECTION__VALUE", "1")])
        .expect_err("an unknown key must be refused");

    let rendered = error.to_string();
    assert!(
        rendered.contains("RATATOSKR__NOT_A_SECTION__VALUE"),
        "the report must name the offending key: {rendered}"
    );
    assert!(
        !rendered.contains('1'),
        "the report must not echo the supplied value: {rendered}"
    );
}

#[test]
fn multiple_violations_reported_together_value_free() {
    let error = Config::from_environment([
        ("RATATOSKR__ADMIN__LISTEN_ADDRESS", "10.0.0.1:9084"),
        ("RATATOSKR__LIMITS__DATABASE_CONNECTIONS", "0"),
    ])
    .expect_err("two independent violations must both be refused");

    assert_eq!(
        error.violations.len(),
        2,
        "both violations must be reported together: {error}"
    );
    let rendered = error.to_string();
    assert!(rendered.contains("RATATOSKR__ADMIN__LISTEN_ADDRESS"));
    assert!(rendered.contains("loopback"));
    assert!(rendered.contains("RATATOSKR__LIMITS__DATABASE_CONNECTIONS"));
    assert!(rendered.contains("must be a positive integer"));
    assert!(!rendered.contains("10.0.0.1"), "values must never render");
    assert!(
        !rendered.contains('0'),
        "the supplied value leaked into the report"
    );
}

#[test]
fn recognized_override_changes_exactly_its_own_field() {
    let config = Config::from_environment([
        ("RATATOSKR__LIMITS__DATABASE_CONNECTIONS", "3"),
        (
            "RATATOSKR__STORAGE__DATABASE_URL",
            "postgres://threads:threads@127.0.0.1:5437/threads",
        ),
    ])
    .expect("valid overrides must load");

    assert_eq!(config.limits.database_connections, 3);
    assert_eq!(config.limits.shutdown_timeout_ms, 10_000);
    let url = config
        .storage
        .database_url
        .as_ref()
        .expect("the override must set the database URL");
    assert_eq!(
        url.expose_secret(),
        "postgres://threads:threads@127.0.0.1:5437/threads"
    );
}

#[test]
fn valid_configuration_report_renders_no_secret_material() {
    // The report an operator sees is the Debug rendering (`check-config`
    // prints `{config:#?}`), so the redaction contract lives there.
    let config = Config {
        storage: StorageConfig {
            database_url: Some(secrecy::SecretString::from(
                "postgres://threads:hunter2@127.0.0.1:5437/threads",
            )),
        },
        ..Config::default()
    };

    let rendered = format!("{config:#?}");
    assert!(rendered.contains("[REDACTED]"), "{rendered}");
    assert!(
        !rendered.contains("hunter2"),
        "the secret leaked into Debug: {rendered}"
    );
}
