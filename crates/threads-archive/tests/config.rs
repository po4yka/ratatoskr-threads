//! Configuration strictness: the executable form of the service-runtime spec.

use secrecy::ExposeSecret as _;

use ratatoskr_threads_archive::{Config, StorageConfig};

#[test]
fn empty_environment_refuses_to_start_without_the_command_bus() {
    let error = Config::from_environment(Vec::<(String, String)>::new())
        .expect_err("Threads must not start without the command bus");

    assert!(error.to_string().contains("RATATOSKR__BUS__URL"));
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
        ("RATATOSKR__BUS__URL", "nats://127.0.0.1:4222"),
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
        ("RATATOSKR__BUS__URL", "nats://127.0.0.1:4222"),
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

#[test]
fn bus_configuration_requires_an_endpoint_and_accepts_an_optional_nkey_path() {
    let missing_url = Config::from_environment([(
        "RATATOSKR__BUS__NKEY_SEED_PATH",
        "/run/ratatoskr/threads.nkey",
    )])
    .expect_err("an nkey path without its NATS endpoint is refused");
    assert!(missing_url.to_string().contains("RATATOSKR__BUS__URL"));

    let config = Config::from_environment([
        ("RATATOSKR__BUS__URL", "tls://nats.internal:4222"),
        (
            "RATATOSKR__BUS__NKEY_SEED_PATH",
            "/run/ratatoskr/threads.nkey",
        ),
    ])
    .expect("the bounded NATS configuration is accepted");
    assert_eq!(config.bus.url, "tls://nats.internal:4222");
    assert_eq!(
        config
            .bus
            .nkey_seed_path
            .expect("the nkey path is retained")
            .to_string_lossy(),
        "/run/ratatoskr/threads.nkey"
    );
}
