//! Operator plane contract: liveness, readiness, metrics, version, caching.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt as _;
use metrics::{Key, Label, Metadata, Recorder as _};
use metrics_exporter_prometheus::PrometheusBuilder;
use tower::ServiceExt as _;

use ratatoskr_threads_archive_service::RuntimeState;

type RenderMetrics = Box<dyn Fn() -> String + Send + Sync>;

/// What one admin response looked like on the wire.
struct Observed {
    status: StatusCode,
    body: String,
    cache_control: Option<String>,
    content_type: Option<String>,
}

#[expect(
    clippy::expect_used,
    reason = "router-test helper: an unanswered request or unreadable body is the failure"
)]
async fn get_status(
    state: Arc<RuntimeState>,
    render_metrics: RenderMetrics,
    path: &str,
) -> Observed {
    let router = ratatoskr_threads_archive_service::admin_router(state, render_metrics);
    let response = router
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("a valid request"),
        )
        .await
        .expect("the router answers");
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .map(|value| value.to_str().expect("ASCII header").to_owned())
    };
    // The body is consumed last: reading it ends the response borrow.
    Observed {
        status: response.status(),
        cache_control: header("cache-control"),
        content_type: header("content-type"),
        body: String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .expect("a collectible body")
                .to_bytes()
                .to_vec(),
        )
        .expect("UTF-8 bodies"),
    }
}

fn noop_metrics() -> RenderMetrics {
    Box::new(String::new)
}

#[tokio::test]
async fn liveness_answers_200_in_starting_ready_and_draining_states() {
    let state = Arc::new(RuntimeState::new());

    let starting = get_status(Arc::clone(&state), noop_metrics(), "/health/live").await;
    assert_eq!(
        starting.status,
        StatusCode::OK,
        "live before startup completes"
    );
    assert!(
        starting.body.contains("live"),
        "the body must state liveness: {}",
        starting.body
    );

    state.mark_startup_complete();
    let ready = get_status(Arc::clone(&state), noop_metrics(), "/health/live").await;
    assert_eq!(ready.status, StatusCode::OK, "live when ready");

    state.begin_draining();
    let draining = get_status(state, noop_metrics(), "/health/live").await;
    assert_eq!(draining.status, StatusCode::OK, "live throughout drain");
}

#[tokio::test]
async fn readiness_is_503_before_startup_200_after_and_503_while_draining() {
    let state = Arc::new(RuntimeState::new());

    let before = get_status(Arc::clone(&state), noop_metrics(), "/health/ready").await;
    assert_eq!(
        before.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "not ready at start"
    );
    assert!(before.body.contains("not_ready"), "{}", before.body);
    assert!(
        before.body.contains("startup_incomplete"),
        "{}",
        before.body
    );

    state.mark_startup_complete();
    let up = get_status(Arc::clone(&state), noop_metrics(), "/health/ready").await;
    assert_eq!(up.status, StatusCode::OK, "ready after startup completes");
    assert!(up.body.contains("\"ready\""), "{}", up.body);

    // A down dependency is visible in the body without flipping readiness.
    state.set_database_reachable(false);
    let degraded = get_status(Arc::clone(&state), noop_metrics(), "/health/ready").await;
    assert_eq!(
        degraded.status,
        StatusCode::OK,
        "a down dependency must not flap readiness"
    );
    assert!(degraded.body.contains("\"database\""), "{}", degraded.body);
    assert!(degraded.body.contains("\"fail\""), "{}", degraded.body);
    assert!(
        degraded.body.contains("dependency_unavailable"),
        "{}",
        degraded.body
    );

    state.set_database_reachable(true);
    let recovered = get_status(Arc::clone(&state), noop_metrics(), "/health/ready").await;
    assert!(
        recovered.body.contains("\"pass\""),
        "recovered probe passes: {}",
        recovered.body
    );

    state.begin_draining();
    let draining = get_status(state, noop_metrics(), "/health/ready").await;
    assert_eq!(
        draining.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "draining is not ready"
    );
    assert!(
        draining.body.contains("shutdown_requested"),
        "{}",
        draining.body
    );
}

#[tokio::test]
async fn metrics_returns_prometheus_text_containing_threads_build_info() {
    // A private recorder, not a global install: several router tests run in
    // one process, and only one global recorder may exist.
    let recorder = PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    let metadata = Metadata::new(module_path!(), metrics::Level::INFO, None);
    recorder
        .register_gauge(
            &Key::from_parts(
                "threads_build_info",
                vec![
                    Label::new("service", "ratatoskr-threads"),
                    Label::new("version", env!("CARGO_PKG_VERSION")),
                    Label::new("git_sha", "unknown"),
                ],
            ),
            &metadata,
        )
        .set(1.0);

    let observed = get_status(
        Arc::new(RuntimeState::new()),
        Box::new(move || handle.render()),
        "/metrics",
    )
    .await;
    assert_eq!(observed.status, StatusCode::OK);
    assert!(
        observed.body.contains("threads_build_info"),
        "the exposition text must carry the build-info series: {}",
        observed.body
    );
    assert!(
        observed
            .content_type
            .as_deref()
            .is_some_and(|value| value.starts_with("text/plain")),
        "the Content-Type must be the Prometheus text exposition type: {:?}",
        observed.content_type
    );
}

#[tokio::test]
async fn version_returns_name_version_git_sha_and_rust_version() {
    let observed = get_status(Arc::new(RuntimeState::new()), noop_metrics(), "/version").await;
    assert_eq!(observed.status, StatusCode::OK);
    assert!(
        observed.body.contains("ratatoskr-threads"),
        "{}",
        observed.body
    );
    assert!(
        observed.body.contains(env!("CARGO_PKG_VERSION")),
        "{}",
        observed.body
    );
    assert!(observed.body.contains("git_sha"), "{}", observed.body);
    assert!(observed.body.contains("rust_version"), "{}", observed.body);
}

#[tokio::test]
async fn every_response_including_unknown_path_carries_no_store() {
    for path in [
        "/health/live",
        "/health/ready",
        "/metrics",
        "/version",
        "/nope",
    ] {
        let observed = get_status(Arc::new(RuntimeState::new()), noop_metrics(), path).await;
        if path == "/nope" {
            assert_eq!(observed.status, StatusCode::NOT_FOUND, "{path}");
            assert!(
                observed.cache_control.is_some(),
                "even the 404 forbids caching"
            );
        } else {
            assert_eq!(
                observed.cache_control.as_deref(),
                Some("no-store"),
                "{path} must forbid caching"
            );
        }
    }
}
