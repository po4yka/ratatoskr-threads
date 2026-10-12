//! Configuration strictness: the executable form of the service-runtime spec.

use secrecy::ExposeSecret as _;

use ratatoskr_threads_archive::{Config, StorageConfig};

const REQUIRED_RESOLVER_ROOT: &str = "RATATOSKR__PUBLIC_RESOLUTION__RAW_OBJECT_ROOT";
const RESOLVER_ROOT: &str = "/var/lib/ratatoskr/threads/raw";

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
        (REQUIRED_RESOLVER_ROOT, RESOLVER_ROOT),
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
        (REQUIRED_RESOLVER_ROOT, RESOLVER_ROOT),
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
        (REQUIRED_RESOLVER_ROOT, RESOLVER_ROOT),
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

#[test]
fn bus_configuration_requires_the_public_resolution_surface() {
    let error = Config::from_environment([("RATATOSKR__BUS__URL", "nats://127.0.0.1:4222")])
        .expect_err("a bus without the surface that completes its work is refused");

    assert_eq!(error.violations.len(), 1, "{error}");
    assert!(
        error.to_string().contains(REQUIRED_RESOLVER_ROOT),
        "{error}"
    );
}

#[test]
fn public_resolution_defaults_to_the_official_endpoint_and_a_bounded_retry_budget() {
    let config = Config::from_environment([
        ("RATATOSKR__BUS__URL", "nats://127.0.0.1:4222"),
        (REQUIRED_RESOLVER_ROOT, RESOLVER_ROOT),
    ])
    .expect("the minimal resolver configuration loads");

    let resolution = &config.public_resolution;
    assert_eq!(
        resolution.oembed_endpoint,
        "https://graph.threads.com/oembed"
    );
    assert_eq!(
        resolution
            .raw_object_root
            .as_deref()
            .map(std::path::Path::new),
        Some(std::path::Path::new("/var/lib/ratatoskr/threads/raw"))
    );
    assert_eq!(resolution.max_attempts, 5);
    assert_eq!(resolution.batch_size, 8);
    assert_eq!(resolution.poll_interval_ms, 2_000);
}

#[test]
fn public_resolution_overrides_apply_and_unsafe_values_are_refused_by_key() {
    let config = Config::from_environment([
        ("RATATOSKR__BUS__URL", "nats://127.0.0.1:4222"),
        (REQUIRED_RESOLVER_ROOT, RESOLVER_ROOT),
        ("RATATOSKR__PUBLIC_RESOLUTION__MAX_ATTEMPTS", "3"),
        ("RATATOSKR__PUBLIC_RESOLUTION__BATCH_SIZE", "2"),
        ("RATATOSKR__PUBLIC_RESOLUTION__POLL_INTERVAL_MS", "250"),
    ])
    .expect("valid overrides load");
    assert_eq!(config.public_resolution.max_attempts, 3);
    assert_eq!(config.public_resolution.batch_size, 2);
    assert_eq!(config.public_resolution.poll_interval_ms, 250);

    let error = Config::from_environment([
        ("RATATOSKR__BUS__URL", "nats://127.0.0.1:4222"),
        (REQUIRED_RESOLVER_ROOT, "relative/raw"),
        (
            "RATATOSKR__PUBLIC_RESOLUTION__OEMBED_ENDPOINT",
            "http://graph.threads.com/oembed",
        ),
        ("RATATOSKR__PUBLIC_RESOLUTION__MAX_ATTEMPTS", "0"),
    ])
    .expect_err("a relative root, a plain-HTTP endpoint and a zero budget are refused");
    let rendered = error.to_string();
    assert_eq!(error.violations.len(), 3, "{rendered}");
    assert!(rendered.contains(REQUIRED_RESOLVER_ROOT), "{rendered}");
    assert!(
        rendered.contains("RATATOSKR__PUBLIC_RESOLUTION__OEMBED_ENDPOINT"),
        "{rendered}"
    );
    assert!(
        rendered.contains("RATATOSKR__PUBLIC_RESOLUTION__MAX_ATTEMPTS"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("relative/raw"),
        "values must never render: {rendered}"
    );
}

const RESEND_AFTER: &str = "RATATOSKR__TOMBSTONE__RESEND_AFTER_SECONDS";
const RESEND_CAP: &str = "RATATOSKR__TOMBSTONE__RESEND_CAP_SECONDS";

fn with_tombstone<'a>(entries: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut all = vec![
        ("RATATOSKR__BUS__URL", "nats://127.0.0.1:4222"),
        (REQUIRED_RESOLVER_ROOT, RESOLVER_ROOT),
    ];
    all.extend_from_slice(entries);
    all
}

#[test]
fn tombstone_resend_defaults_to_ten_minutes_and_a_six_hour_cap() {
    let config = Config::from_environment(with_tombstone(&[])).expect("the defaults load");

    assert_eq!(config.tombstone.resend_after_seconds, 600);
    assert_eq!(config.tombstone.resend_cap_seconds, 21_600);
    let policy = config.tombstone.resend_policy();
    assert_eq!(policy.after, std::time::Duration::from_mins(10));
    assert_eq!(policy.cap, std::time::Duration::from_hours(6));
}

#[test]
fn tombstone_resend_overrides_apply_inside_the_documented_range() {
    let config = Config::from_environment(with_tombstone(&[
        (RESEND_AFTER, "30"),
        (RESEND_CAP, "604800"),
    ]))
    .expect("the range limits load");
    assert_eq!(config.tombstone.resend_after_seconds, 30);
    assert_eq!(config.tombstone.resend_cap_seconds, 604_800);

    let config = Config::from_environment(with_tombstone(&[
        (RESEND_AFTER, "86400"),
        (RESEND_CAP, "86400"),
    ]))
    .expect("a cap equal to the delay loads");
    assert_eq!(config.tombstone.resend_after_seconds, 86_400);
}

#[test]
fn tombstone_resend_values_outside_the_range_are_refused_together_and_by_key() {
    let error = Config::from_environment(with_tombstone(&[
        (RESEND_AFTER, "29"),
        (RESEND_CAP, "604801"),
    ]))
    .expect_err("a delay under 30 s and a cap over 7 days are refused");
    let rendered = error.to_string();
    assert_eq!(error.violations.len(), 2, "{rendered}");
    assert!(rendered.contains(RESEND_AFTER), "{rendered}");
    assert!(rendered.contains(RESEND_CAP), "{rendered}");
    assert!(!rendered.contains("604801"), "values must never render");
    assert!(
        rendered
            .contains("RATATOSKR__TOMBSTONE__RESEND_AFTER_SECONDS must be between 30 and 86400"),
        "the rule names the range: {rendered}"
    );
    assert!(
        rendered.contains("RATATOSKR__TOMBSTONE__RESEND_CAP_SECONDS must be at most 604800"),
        "the rule names the limit: {rendered}"
    );

    let error = Config::from_environment(with_tombstone(&[(RESEND_AFTER, "86401")]))
        .expect_err("a delay over one day is refused");
    assert!(error.to_string().contains(RESEND_AFTER), "{error}");
}

#[test]
fn tombstone_cap_below_the_delay_is_refused() {
    let error = Config::from_environment(with_tombstone(&[
        (RESEND_AFTER, "7200"),
        (RESEND_CAP, "3600"),
    ]))
    .expect_err("a cap below the delay is refused");
    assert_eq!(error.violations.len(), 1, "{error}");
    assert!(error.to_string().contains(RESEND_CAP), "{error}");

    let error = Config::from_environment(with_tombstone(&[(RESEND_AFTER, "30000")]))
        .expect_err("a delay above the default cap needs a cap of its own");
    assert!(error.to_string().contains(RESEND_CAP), "{error}");
}
