//! The process serves Knowledge's acknowledgements of a removal: the two tombstone gauges move
//! with the acknowledgement, and readiness never depends on Knowledge answering (XR-021
//! CONTRACTS.md R5-11, rule 4). The broker is a private anonymous `nats-server` so that the
//! durable `ratatoskr_threads_deletion_acks` is this test's alone.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "assertions in a test binary"
)]

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use async_nats::jetstream;
use ratatoskr_threads_archive::test_support::{TestDatabase, admin_url, raw_object_root};
use uuid::Uuid;

const BIN: &str = env!("CARGO_BIN_EXE_ratatoskr-threads-archive");
const HOST: &str = "127.0.0.1";
const OWNER: &str = "user:018f0000-0000-7000-8000-000000000005";
const READY_TIMEOUT: Duration = Duration::from_mins(1);
const EVENTS_STREAM: &str = "ratatoskr_events";
const DURABLE: &str = "ratatoskr_threads_deletion_acks";
const ACK_SUBJECT: &str = "evt.knowledge.deletion.acknowledged.v1";

/// A private anonymous `nats-server` with `JetStream`.
struct PrivateBroker {
    child: Child,
    port: u16,
    directory: PathBuf,
}

impl PrivateBroker {
    fn start() -> Self {
        let port = free_port();
        let directory = std::env::temp_dir().join(format!(
            "ratatoskr-threads-private-broker-{}",
            Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let child = Command::new("nats-server")
            .args(["-js", "-a", HOST, "-p", &port.to_string(), "-sd"])
            .arg(&directory)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("nats-server is on PATH (CI copies it out of the NATS image)");
        let broker = Self {
            child,
            port,
            directory,
        };
        let listening = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            TcpStream::connect((HOST, port)).is_ok()
        });
        assert!(listening, "the private broker starts listening");
        broker
    }

    fn url(&self) -> String {
        format!("nats://{HOST}:{}", self.port)
    }
}

impl Drop for PrivateBroker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind((HOST, 0)).expect("a free port exists");
    listener.local_addr().expect("a bound address").port()
}

/// One minimal HTTP/1.1 GET over raw TCP, closing after one response.
fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect((HOST, port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {HOST}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let status = response
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some((status, response))
}

/// The value of a Prometheus series without labels in a scrape body.
fn series(body: &str, name: &str) -> Option<f64> {
    body.lines().find_map(|line| {
        line.strip_prefix(name)?
            .strip_prefix(' ')?
            .trim()
            .parse()
            .ok()
    })
}

/// Edge's side: both streams, the command durable and the acknowledgement durable.
async fn provision(url: &str, acknowledgement_filter: &str) -> jetstream::Context {
    let context = jetstream::new(async_nats::connect(url).await.expect("the broker connects"));
    let commands = context
        .get_or_create_stream(jetstream::stream::Config {
            name: "ratatoskr_commands".to_owned(),
            subjects: vec!["cmd.>".to_owned()],
            ..jetstream::stream::Config::default()
        })
        .await
        .expect("the command stream exists");
    commands
        .create_consumer(jetstream::consumer::pull::Config {
            durable_name: Some("threads_browser_capture".to_owned()),
            filter_subject: "cmd.threads.capture.requested.v1".to_owned(),
            ack_policy: jetstream::consumer::AckPolicy::Explicit,
            ..jetstream::consumer::pull::Config::default()
        })
        .await
        .expect("the command durable exists");
    let events = context
        .get_or_create_stream(jetstream::stream::Config {
            name: EVENTS_STREAM.to_owned(),
            subjects: vec!["evt.>".to_owned()],
            ..jetstream::stream::Config::default()
        })
        .await
        .expect("the events stream exists");
    events
        .create_consumer(jetstream::consumer::pull::Config {
            durable_name: Some(DURABLE.to_owned()),
            filter_subject: acknowledgement_filter.to_owned(),
            ack_policy: jetstream::consumer::AckPolicy::Explicit,
            ack_wait: Duration::from_secs(30),
            ..jetstream::consumer::pull::Config::default()
        })
        .await
        .expect("the acknowledgement durable exists");
    context
}

/// Starts the service binary against `test`'s database and `broker`.
fn spawn_service(broker: &PrivateBroker, test: &TestDatabase, port: u16) -> Child {
    let (base, _) = admin_url()
        .rsplit_once('/')
        .map(|(prefix, name)| (prefix.to_owned(), name.to_owned()))
        .expect("the admin URL names a database");
    Command::new(BIN)
        .env(
            "RATATOSKR__STORAGE__DATABASE_URL",
            format!("{base}/{}", test.name()),
        )
        .env("RATATOSKR__ADMIN__LISTEN_ADDRESS", format!("{HOST}:{port}"))
        .env("RATATOSKR__BUS__URL", broker.url())
        .env(
            "RATATOSKR__PUBLIC_RESOLUTION__RAW_OBJECT_ROOT",
            raw_object_root(),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the service binary spawns")
}

fn removal_envelope(id: Uuid) -> serde_json::Value {
    serde_json::json!({
        "event_id": id,
        "event_type": "social.source.removed.v1",
        "occurred_at": "2026-10-12T08:00:00Z",
        "producer": "ratatoskr-threads",
        "aggregate_id": format!("social_source:{id}"),
        "correlation_id": format!("deletion:{id}"),
        "tenant_id": OWNER,
        "schema_version": 1,
        "payload": {},
    })
}

fn applied_acknowledgement(acknowledged: Uuid) -> String {
    serde_json::json!({
        "event_id": Uuid::now_v7(),
        "event_type": "knowledge.deletion.acknowledged.v1",
        "occurred_at": "2026-10-12T08:00:02Z",
        "producer": "ratatoskr-knowledge",
        "aggregate_id": format!("social_source:{acknowledged}"),
        "correlation_id": format!("deletion:{acknowledged}"),
        "causation_id": format!("event:{acknowledged}"),
        "tenant_id": OWNER,
        "schema_version": 1,
        "payload": {
            "acknowledged_event_id": acknowledged,
            "family": "social",
            "owner": OWNER,
            "subject": format!("social_source:{acknowledged}"),
            "outcome": "applied",
            "acknowledged_at": "2026-10-12T08:00:02Z",
        },
    })
    .to_string()
}

async fn wait_for(what: &str, mut condition: impl AsyncFnMut() -> bool) {
    let deadline = Instant::now() + READY_TIMEOUT;
    while !condition().await {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn an_unacknowledged_removal_moves_the_gauges_and_never_the_readiness() {
    let broker = PrivateBroker::start();
    let context = provision(&broker.url(), ACK_SUBJECT).await;
    let test = TestDatabase::create().await.expect("a prepared database");
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at) \
         values ($1, 'social.source.removed.v1', 'capture', $2, $3, \
                 now() - interval '90 seconds')",
    )
    .bind(id)
    .bind(Uuid::now_v7())
    .bind(removal_envelope(id))
    .execute(test.database.pool())
    .await
    .expect("the removal seeds");
    let port = free_port();
    let mut child = spawn_service(&broker, &test, port);

    wait_for("readiness", async || {
        matches!(http_get(port, "/health/ready"), Some((200, _)))
    })
    .await;
    wait_for(
        "the gauges to show the unacknowledged removal",
        async || {
            http_get(port, "/metrics").is_some_and(|(_, body)| {
                series(&body, "threads_unacknowledged_tombstones") == Some(1.0)
                    && series(&body, "threads_oldest_unacknowledged_tombstone_age_seconds")
                        .is_some_and(|age| age >= 90.0)
            })
        },
    )
    .await;
    let (status, ready) = http_get(port, "/health/ready").expect("readiness answers");
    assert_eq!(
        status, 200,
        "Knowledge never answered and the service is ready: {ready}"
    );
    assert!(
        ready.contains("\"name\":\"deletion_acks\",\"state\":\"pass\""),
        "the acknowledgement consumer is a readiness check: {ready}"
    );

    context
        .publish(ACK_SUBJECT, applied_acknowledgement(id).into())
        .await
        .expect("the acknowledgement publishes")
        .await
        .expect("the broker stores it");
    wait_for("the gauges to fall to zero", async || {
        http_get(port, "/metrics").is_some_and(|(_, body)| {
            series(&body, "threads_unacknowledged_tombstones") == Some(0.0)
                && series(&body, "threads_oldest_unacknowledged_tombstone_age_seconds") == Some(0.0)
        })
    })
    .await;
    assert!(
        matches!(http_get(port, "/health/ready"), Some((200, _))),
        "readiness is unchanged by the acknowledgement"
    );

    let _ = child.kill();
    let _ = child.wait();
    test.cleanup().await.expect("cleanup drops");
}

#[tokio::test]
async fn a_durable_that_differs_from_its_spec_keeps_the_service_from_starting() {
    let broker = PrivateBroker::start();
    let _context = provision(&broker.url(), "evt.knowledge.deletion.>").await;
    let test = TestDatabase::create().await.expect("a prepared database");
    let port = free_port();
    let mut child = spawn_service(&broker, &test, port);

    let deadline = Instant::now() + READY_TIMEOUT;
    let exited = loop {
        if let Some(status) = child.try_wait().expect("the child can be polled") {
            break status.code();
        }
        assert!(
            Instant::now() < deadline,
            "the service kept running over a mismatched durable"
        );
        assert!(
            !matches!(http_get(port, "/health/ready"), Some((200, _))),
            "the service reported ready over a mismatched durable"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(exited, Some(1), "a deployment fault exits non-zero");

    test.cleanup().await.expect("cleanup drops");
}
