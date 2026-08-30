//! Telemetry bootstrap: the executable form of the service-runtime spec.

use ratatoskr_threads_archive::config::TelemetryConfig;
use ratatoskr_threads_archive::init_telemetry;
use ratatoskr_threads_archive::telemetry;

#[test]
fn initialization_succeeds_once_and_second_call_is_typed_error() {
    let config = TelemetryConfig {
        log_filter: "info".to_owned(),
    };

    init_telemetry(&config).expect("the first initialization must succeed");
    let error = init_telemetry(&config)
        .expect_err("a second initialization in one process must be refused");
    assert!(
        error.to_string().contains("telemetry"),
        "the failure must be the typed telemetry refusal: {error}"
    );
    assert!(
        matches!(
            error,
            ratatoskr_threads_archive::TelemetryError::AlreadyInstalled(_)
        ),
        "the second refusal must be the already-installed variant: {error:?}"
    );
}

#[test]
fn emitted_records_parse_as_json_with_identity_fields() {
    let record = telemetry::render_startup_record();

    let parsed: serde_json::Value =
        serde_json::from_str(&record).expect("the startup record must parse as JSON");

    assert_eq!(
        parsed["fields"]["service_name"],
        telemetry::SERVICE_NAME,
        "records carry the service identity"
    );
    assert_eq!(
        parsed["fields"]["version"],
        telemetry::VERSION,
        "records carry the crate version"
    );
    assert_eq!(
        parsed["fields"]["git_sha"],
        telemetry::GIT_SHA,
        "records carry the build's git SHA"
    );
}

#[test]
fn lifecycle_metrics_cover_bounded_outcomes_without_sensitive_labels() {
    let descriptors = telemetry::lifecycle_metric_descriptors();
    let names = descriptors
        .iter()
        .map(|descriptor| descriptor.name)
        .collect::<std::collections::BTreeSet<_>>();
    let required = [
        "threads_blob_deletion_attempts_total",
        "threads_deletion_operations_total",
        "threads_export_reprocessing_duration_seconds",
        "threads_export_reprocessing_total",
        "threads_media_admission_total",
        "threads_reresolution_attempts_total",
        "threads_reresolution_duration_seconds",
    ];
    let missing = required
        .into_iter()
        .filter(|name| !names.contains(name))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "lifecycle metrics are missing: {missing:?}"
    );
    let prohibited = [
        "username",
        "url",
        "post_text",
        "note",
        "credential",
        "raw_error",
        "capture_id",
        "source_id",
        "operation_id",
    ];
    for descriptor in descriptors {
        assert!(
            descriptor
                .labels
                .iter()
                .all(|label| !prohibited.contains(label)),
            "{} exposes a prohibited label in {:?}",
            descriptor.name,
            descriptor.labels
        );
    }
}

#[test]
fn outbox_metrics_cover_pending_failed_redelivered_and_dead_lettered_without_sensitive_labels() {
    let descriptors = telemetry::lifecycle_metric_descriptors();
    let names = descriptors
        .iter()
        .map(|descriptor| descriptor.name)
        .collect::<std::collections::BTreeSet<_>>();
    let required = [
        "threads_outbox_dead_lettered",
        "threads_outbox_failed_total",
        "threads_outbox_pending",
        "threads_outbox_redelivered_total",
    ];
    let missing = required
        .into_iter()
        .filter(|name| !names.contains(name))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "outbox metrics are missing: {missing:?}"
    );

    let prohibited = [
        "event_id",
        "payload",
        "url",
        "post_text",
        "credential",
        "raw_error",
    ];
    for descriptor in descriptors
        .iter()
        .filter(|descriptor| descriptor.name.starts_with("threads_outbox_"))
    {
        assert!(
            descriptor
                .labels
                .iter()
                .all(|label| !prohibited.contains(label)),
            "{} exposes a prohibited label in {:?}",
            descriptor.name,
            descriptor.labels
        );
    }

    let failure_record = telemetry::render_outbox_failure_record();
    let parsed: serde_json::Value = serde_json::from_str(&failure_record)
        .expect("the outbox failure record must parse as JSON");
    assert_eq!(parsed["fields"]["failure_class"], "broker_unacknowledged");
    assert_eq!(parsed["fields"]["terminal"], false);
    for sensitive_fragment in [
        "event_id",
        "payload",
        "http://",
        "post_text",
        "credential",
        "raw broker detail",
    ] {
        assert!(
            !failure_record.contains(sensitive_fragment),
            "failure log exposes prohibited content: {sensitive_fragment}"
        );
    }
}
