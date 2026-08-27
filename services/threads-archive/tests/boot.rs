//! Process boot contract: the real binary starts against a disposable
//! database, serves the operator plane, validates configuration, and stops
//! cleanly on SIGTERM.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ratatoskr_threads_archive::test_support::TestDatabase;

const BIN: &str = env!("CARGO_BIN_EXE_ratatoskr-threads-archive");
const READY_TIMEOUT: Duration = Duration::from_mins(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(20);
const NATS_URL: &str = "nats://127.0.0.1:5422";

/// Reserves a free loopback port for the operator listener.
#[expect(
    clippy::expect_used,
    reason = "boot-test helper: an unreservable port is the failure under test"
)]
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port exists");
    let port = listener.local_addr().expect("a bound address").port();
    drop(listener);
    port
}

/// One minimal HTTP/1.1 GET over raw TCP, closing after one response.
fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let status_line = response.lines().next()?;
    let status = status_line.split_whitespace().nth(1)?.parse::<u16>().ok()?;
    Some((status, response))
}

#[expect(clippy::expect_used, reason = "boot-test helper; see free_port")]
async fn spawn_service(database_url: &str, admin_port: u16) -> Child {
    ensure_command_stream().await;
    Command::new(BIN)
        .env("RATATOSKR__STORAGE__DATABASE_URL", database_url)
        .env(
            "RATATOSKR__ADMIN__LISTEN_ADDRESS",
            format!("127.0.0.1:{admin_port}"),
        )
        .env("RATATOSKR__BUS__URL", NATS_URL)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the service binary spawns")
}

#[expect(
    clippy::expect_used,
    reason = "test fixture: its broker/stream setup is required before service startup"
)]
async fn ensure_command_stream() {
    let client = async_nats::connect(NATS_URL)
        .await
        .expect("the local NATS test broker is reachable");
    let context = async_nats::jetstream::new(client);
    let stream = context
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: "ratatoskr_commands".to_owned(),
            subjects: vec!["cmd.>".to_owned()],
            ..async_nats::jetstream::stream::Config::default()
        })
        .await
        .expect("the command stream exists");
    let _consumer = stream
        .get_or_create_consumer(
            "threads_browser_capture",
            async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("threads_browser_capture".to_owned()),
                filter_subject: "cmd.threads.capture.requested.v1".to_owned(),
                ..async_nats::jetstream::consumer::pull::Config::default()
            },
        )
        .await
        .expect("the Threads command consumer exists");
}

#[cfg(unix)]
#[expect(clippy::expect_used, reason = "boot-test helper; see free_port")]
fn send_sigterm(child: &Child) {
    Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .output()
        .expect("SIGTERM is deliverable");
}

/// The connection URL of a disposable database by its generated name.
fn test_url(name: &str) -> String {
    let base = ratatoskr_threads_archive::test_support::admin_url();
    let (prefix, _) = base.rsplit_once('/').unwrap_or((base.as_str(), ""));
    format!("{prefix}/{name}")
}

fn wait_with_timeout(child: &mut Child, limit: Duration) -> std::io::Result<Option<i32>> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.code());
        }
        if Instant::now() > deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[tokio::test]
async fn ready_reaches_200_after_startup() {
    let test = TestDatabase::create().await.expect("a prepared database");
    let url = test_url(test.name());
    let port = free_port();

    let mut child = spawn_service(&url, port).await;

    // Readiness arrives only after connect + schema apply + bind.
    let deadline = Instant::now() + READY_TIMEOUT;
    let mut ready_status = None;
    while Instant::now() < deadline {
        if let Some((status, _)) = http_get(port, "/health/ready") {
            ready_status = Some(status);
            if status == 200 {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(
        ready_status,
        Some(200),
        "readiness did not arrive within {READY_TIMEOUT:?}"
    );

    let _ = child.kill();
    let _ = child.wait();
    test.cleanup().await.expect("cleanup drops");
}

#[cfg(unix)]
#[tokio::test]
async fn live_metrics_version_answer_200_and_unknown_path_404_while_serving() {
    let test = TestDatabase::create().await.expect("a prepared database");
    let url = test_url(test.name());
    let port = free_port();

    let mut child = spawn_service(&url, port).await;

    // Wait for readiness before probing the rest of the plane.
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if matches!(http_get(port, "/health/ready"), Some((200, _))) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let (live_status, live_body) =
        http_get(port, "/health/live").expect("live answers while serving");
    assert_eq!(live_status, 200);
    assert!(live_body.contains("live"), "{live_body}");

    let (_, metrics_body) = http_get(port, "/metrics").expect("metrics answers");
    assert!(
        metrics_body.contains("threads_build_info"),
        "build info must be exported: {metrics_body}"
    );

    let (_, version_body) = http_get(port, "/version").expect("version answers");
    assert!(version_body.contains("ratatoskr-threads"), "{version_body}");

    let (unknown_status, _) = http_get(port, "/definitely/not/here").expect("the 404 answers");
    assert_eq!(unknown_status, 404);

    let _ = child.kill();
    let _ = child.wait();
    test.cleanup().await.expect("cleanup drops");
}

#[cfg(unix)]
#[tokio::test]
async fn sigterm_exits_0_within_shutdown_bound() {
    let test = TestDatabase::create().await.expect("a prepared database");
    let url = test_url(test.name());
    let port = free_port();

    let mut child = spawn_service(&url, port).await;

    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if matches!(http_get(port, "/health/ready"), Some((200, _))) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    send_sigterm(&child);
    let exited = wait_with_timeout(&mut child, SHUTDOWN_TIMEOUT).expect("no spawn error");
    assert_eq!(
        exited,
        Some(0),
        "SIGTERM must produce a clean exit within the shutdown bound"
    );

    test.cleanup().await.expect("cleanup drops");
}

#[tokio::test]
async fn check_config_exits_0_binding_no_port() {
    let test = TestDatabase::create().await.expect("a prepared database");
    let output = Command::new(BIN)
        .arg("check-config")
        .env("RATATOSKR__STORAGE__DATABASE_URL", test_url(test.name()))
        .env("RATATOSKR__BUS__URL", NATS_URL)
        .output()
        .expect("check-config runs");

    assert_eq!(output.status.code(), Some(0), "valid configuration passes");
    let rendered = String::from_utf8_lossy(&output.stderr);
    assert!(rendered.contains("configuration is valid"), "{rendered}");

    // No listener may have been opened by validation.
    let probe = std::net::TcpStream::connect(("127.0.0.1", 9084));
    assert!(probe.is_err(), "check-config must not bind the admin port");

    test.cleanup().await.expect("cleanup drops");
}

#[tokio::test]
async fn invalid_configuration_exits_78_with_value_free_report() {
    let output = Command::new(BIN)
        .arg("check-config")
        .env("RATATOSKR__ADMIN__LISTEN_ADDRESS", "10.9.8.7:9084")
        .env("RATATOSKR__LIMITS__SHUTDOWN_TIMEOUT_MS", "0")
        .output()
        .expect("check-config runs");

    assert_eq!(
        output.status.code(),
        Some(78),
        "invalid configuration is EX_CONFIG"
    );
    let rendered = String::from_utf8_lossy(&output.stderr);
    assert!(rendered.contains("RATATOSKR__ADMIN__LISTEN_ADDRESS"));
    assert!(rendered.contains("RATATOSKR__LIMITS__SHUTDOWN_TIMEOUT_MS"));
    assert!(!rendered.contains("10.9.8.7"), "values never render");
}

#[tokio::test]
async fn missing_database_url_refuses_startup() {
    let port = free_port();
    let mut child = Command::new(BIN)
        .env(
            "RATATOSKR__ADMIN__LISTEN_ADDRESS",
            format!("127.0.0.1:{port}"),
        )
        .env_remove("RATATOSKR__STORAGE__DATABASE_URL")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the service binary spawns");

    let exited = wait_with_timeout(&mut child, READY_TIMEOUT).expect("no spawn error");
    assert_ne!(
        exited,
        Some(0),
        "a process without its database must refuse to start"
    );
    let _ = child.kill();
    let _ = child.wait();
}
