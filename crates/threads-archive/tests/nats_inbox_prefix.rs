//! The service reaches its Edge-provisioned durable through its real connection function under the
//! Threads stanza of the deployed ACL (XR-021 CONTRACTS.md section R3-01). That stanza lets the
//! identity subscribe only to `_INBOX_threads.>`, so a connection without the inbox prefix never
//! receives a `JetStream` reply and its first request times out.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "assertions in a test binary"
)]

use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use async_nats::jetstream;
use futures_util::StreamExt as _;
use ratatoskr_threads_archive::nats::{
    self, COMMAND_STREAM, COMMAND_SUBJECT, CONSUMER_NAME, INBOX_PREFIX, NatsConnection,
};
use ratatoskr_threads_archive::test_support::TestDatabase;
use uuid::Uuid;

mod common;

use common::threads_command;

const HOST: &str = "127.0.0.1";
const EVENTS_STREAM: &str = "ratatoskr_events";
const REPORT_SUBJECT: &str = "evt.platform.operation.reported.v1";

/// A private `nats-server` with `JetStream` whose only users are an administrator standing in for
/// Edge and the Threads stanza shipped in `deploy/nats/identity.conf`.
struct Enforcing {
    child: Child,
    port: u16,
    directory: PathBuf,
}

impl Enforcing {
    fn start(threads_public: &str, admin_public: &str) -> Self {
        let stanza = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/nats/identity.conf"
        ))
        .expect("deploy/nats/identity.conf is shipped");
        let stanza: String = stanza
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("nkey:") {
                    format!("    nkey: {threads_public}")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let port = std::net::TcpListener::bind((HOST, 0))
            .expect("a probe listener")
            .local_addr()
            .expect("a local address")
            .port();
        let directory = std::env::temp_dir().join(format!(
            "ratatoskr-threads-inbox-prefix-{}",
            Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let config = format!(
            "listen: {HOST}:{port}\njetstream {{ store_dir: \"{store}\" }}\nauthorization {{\n  \
             users = [\n{stanza}\n    {{ nkey: {admin_public} }}\n  ]\n}}\n",
            store = directory.join("js").display(),
        );
        let config_path = directory.join("nats.conf");
        std::fs::write(&config_path, config).expect("the broker configuration is written");
        let child = Command::new("nats-server")
            .arg("-c")
            .arg(&config_path)
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
        assert!(listening, "the enforcing broker starts listening");
        broker
    }

    fn url(&self) -> String {
        format!("nats://{HOST}:{}", self.port)
    }
}

impl Drop for Enforcing {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Creates what Edge creates: both streams and the one durable the Threads identity may use.
async fn provision(context: &jetstream::Context) {
    let commands = context
        .get_or_create_stream(jetstream::stream::Config {
            name: COMMAND_STREAM.to_owned(),
            subjects: vec!["cmd.>".to_owned()],
            ..jetstream::stream::Config::default()
        })
        .await
        .expect("the command stream exists");
    commands
        .create_consumer(jetstream::consumer::pull::Config {
            durable_name: Some(CONSUMER_NAME.to_owned()),
            filter_subject: COMMAND_SUBJECT.to_owned(),
            ack_policy: jetstream::consumer::AckPolicy::Explicit,
            ..jetstream::consumer::pull::Config::default()
        })
        .await
        .expect("the command durable exists");
    context
        .get_or_create_stream(jetstream::stream::Config {
            name: EVENTS_STREAM.to_owned(),
            subjects: vec!["evt.>".to_owned()],
            ..jetstream::stream::Config::default()
        })
        .await
        .expect("the event stream exists");
}

#[tokio::test]
async fn the_connection_completes_jetstream_requests_under_its_inbox_prefix() {
    let threads_keys = nkeys::KeyPair::new_user();
    let admin_keys = nkeys::KeyPair::new_user();
    let broker = Enforcing::start(&threads_keys.public_key(), &admin_keys.public_key());
    let admin = async_nats::ConnectOptions::with_nkey(admin_keys.seed().expect("an admin seed"))
        .connect(broker.url())
        .await
        .expect("the provisioning identity connects");
    let admin_context = jetstream::new(admin.clone());
    provision(&admin_context).await;
    let mut reports = admin
        .subscribe(REPORT_SUBJECT)
        .await
        .expect("subscribing before the command is published");

    let seed_path = broker.directory.join("threads.nkey");
    std::fs::write(&seed_path, threads_keys.seed().expect("a Threads seed"))
        .expect("the seed is written");
    let connection = NatsConnection::connect_with_nkey(&broker.url(), &seed_path)
        .await
        .expect("the service connects through its own connection function");
    assert_eq!(INBOX_PREFIX, "_INBOX_threads");
    nats::ensure_command_consumer(&connection)
        .await
        .expect("the command durable verifies under the Threads stanza");

    let database = TestDatabase::create().await.expect("a test database");
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let worker_database = database.database.clone();
    let worker_connection = connection.clone();
    let worker = tokio::spawn(async move {
        nats::run(&worker_connection, &worker_database, async move {
            let _ignored = stop_rx.await;
        })
        .await
    });

    admin_context
        .publish(
            COMMAND_SUBJECT,
            threads_command(Uuid::now_v7())
                .to_canonical_json()
                .expect("the command serializes")
                .into(),
        )
        .await
        .expect("publishing a durable command")
        .await
        .expect("the command is stored by JetStream");

    let report = tokio::time::timeout(Duration::from_secs(10), reports.next())
        .await
        .expect("the service pulled the command and published its report")
        .expect("the report subscription remains open");
    let report: serde_json::Value =
        serde_json::from_slice(&report.payload).expect("valid report JSON");
    assert_eq!(report["payload"]["status"], "queued");

    // The relay marks a row published only after the broker acknowledged the publication, so a
    // published row is the proof that the PubAck reply reached the service under its prefix.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let published = loop {
        let published: i64 = sqlx::query_scalar(
            "select count(*) from threads_archive.outbox_events where published_at is not null",
        )
        .fetch_one(database.database.pool())
        .await
        .expect("counting published outbox rows");
        if published > 0 || tokio::time::Instant::now() >= deadline {
            break published;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(published, 1, "the report publication was acknowledged");

    stop_tx.send(()).expect("stopping the worker");
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("the worker stops")
        .expect("the worker task joins")
        .expect("the worker exits cleanly");
    database
        .cleanup()
        .await
        .expect("dropping the test database");
}
