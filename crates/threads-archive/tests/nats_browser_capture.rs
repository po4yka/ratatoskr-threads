//! Real broker and database proof for the Threads browser-capture consumer.

use std::time::Duration;

use futures_util::StreamExt as _;
use ratatoskr_social_contracts::SocialCaptureProvider;
use ratatoskr_threads_archive::Database;
use ratatoskr_threads_archive::capture_resolution::{CaptureResolver, ResolverPolicy};
use ratatoskr_threads_archive::nats::{
    self, COMMAND_STREAM, COMMAND_SUBJECT, CONSUMER_NAME, NatsConnection,
};
use ratatoskr_threads_archive::public_resolution::{ApprovedOembedClient, RawObjectStore};
use ratatoskr_threads_archive::removal_acknowledgement::ResendPolicy;
use ratatoskr_threads_archive::test_support::fake_oembed::{FakeOembed, FakeReply};
use ratatoskr_threads_archive::test_support::{TestDatabase, nats_url};
use uuid::Uuid;

mod common;

use common::{PERMALINK, command_with, threads_command};

/// Every test here runs the real worker against the one durable consumer of the shared broker, so
/// two of them at once would steal each other's commands.
static BROKER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The worker as the service runs it, with the deployed resend defaults.
async fn run_bus(
    connection: NatsConnection,
    database: Database,
    stop: impl std::future::Future<Output = ()> + Send,
) -> Result<(), nats::NatsError> {
    nats::run(&connection, &database, ResendPolicy::default(), stop).await
}

#[expect(
    clippy::expect_used,
    reason = "fixture helper: the broker topology is a precondition of every test here"
)]
async fn provision(
    context: &async_nats::jetstream::Context,
) -> async_nats::jetstream::stream::Stream {
    let commands = context
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: COMMAND_STREAM.to_owned(),
            subjects: vec!["cmd.>".to_owned()],
            ..async_nats::jetstream::stream::Config::default()
        })
        .await
        .expect("the command stream exists");
    let _events = context
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: "ratatoskr_events".to_owned(),
            subjects: vec!["evt.>".to_owned()],
            ..async_nats::jetstream::stream::Config::default()
        })
        .await
        .expect("the event stream exists");
    let _consumer = commands
        .get_or_create_consumer(
            CONSUMER_NAME,
            async_nats::jetstream::consumer::pull::Config {
                durable_name: Some(CONSUMER_NAME.to_owned()),
                filter_subject: COMMAND_SUBJECT.to_owned(),
                ..async_nats::jetstream::consumer::pull::Config::default()
            },
        )
        .await
        .expect("the preprovisioned Threads consumer exists");
    commands
}

#[tokio::test]
async fn durable_threads_subject_commits_then_reports_and_acknowledges() {
    let _broker = BROKER.lock().await;
    let database = TestDatabase::create().await.expect("a test database");
    let client = async_nats::connect(nats_url())
        .await
        .expect("the isolated NATS broker is reachable");
    let context = async_nats::jetstream::new(client.clone());
    let commands = provision(&context).await;
    let mut reports = client
        .subscribe("evt.platform.operation.reported.v1")
        .await
        .expect("subscribing before command publication");
    let connection = NatsConnection::connect(&nats_url())
        .await
        .expect("the service role connects");
    nats::ensure_command_consumer(&connection)
        .await
        .expect("the service role opens only its preprovisioned consumer");
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let worker_database = database.database.clone();
    let worker_connection = connection.clone();
    let worker = tokio::spawn(async move {
        run_bus(worker_connection, worker_database, async move {
            let _ignored = stop_rx.await;
        })
        .await
    });

    let command = threads_command(Uuid::now_v7());
    let published = context
        .publish(
            COMMAND_SUBJECT,
            command
                .to_canonical_json()
                .expect("the command serializes")
                .into(),
        )
        .await
        .expect("publishing a durable command");
    published.await.expect("the command is stored by JetStream");

    let report = tokio::time::timeout(Duration::from_secs(10), reports.next())
        .await
        .expect("the queued operation report arrives")
        .expect("the report subscription remains open");
    let report: serde_json::Value =
        serde_json::from_slice(&report.payload).expect("valid report JSON");
    assert_eq!(report["payload"]["status"], "queued");
    assert!(report["payload"].get("error").is_none());
    assert!(report["payload"].get("warnings").is_none());

    let captures: i64 = sqlx::query_scalar("select count(*) from threads_archive.captures")
        .fetch_one(database.database.pool())
        .await
        .expect("the capture transaction committed before the report");
    let inbox: i64 = sqlx::query_scalar("select count(*) from threads_archive.inbox_events")
        .fetch_one(database.database.pool())
        .await
        .expect("the inbox transaction committed before acknowledgement");
    assert_eq!(captures, 1);
    assert_eq!(inbox, 1);

    let mut consumer = commands
        .get_consumer::<async_nats::jetstream::consumer::pull::Config>(CONSUMER_NAME)
        .await
        .expect("reading the preprovisioned consumer");
    // The acknowledgement follows the report on the wire, so give the broker a moment to apply it.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let (pending, ack_pending) = loop {
        let info = consumer.info().await.expect("reading consumer state");
        let counts = (info.num_pending, info.num_ack_pending);
        if counts == (0, 0) || tokio::time::Instant::now() >= deadline {
            break counts;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(pending, 0, "the durable command was acknowledged");
    assert_eq!(ack_pending, 0, "no unacknowledged command remains");

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

/// Inserts one outbox row whose payload carries only a marker, so a test can recognise its own
/// messages among those of the other tests sharing the broker.
#[expect(
    clippy::expect_used,
    reason = "fixture helper: an unanswered insert is the failure"
)]
async fn insert_outbox_row(
    pool: &sqlx::PgPool,
    event_type: &str,
    payload: serde_json::Value,
    age_seconds: i32,
) -> Uuid {
    let event_id = Uuid::now_v7();
    sqlx::query(
        "insert into threads_archive.outbox_events \
         (event_id, event_type, aggregate_type, aggregate_id, payload, occurred_at) \
         values ($1, $2, 'capture', $3, $4, now() - ($5::int * interval '1 second'))",
    )
    .bind(event_id)
    .bind(event_type)
    .bind(Uuid::now_v7())
    .bind(payload)
    .bind(age_seconds)
    .execute(pool)
    .await
    .expect("the outbox row inserts");
    event_id
}

/// Collects the `Nats-Msg-Id` of every wanted message that reaches `subscription`, in arrival
/// order, until all of `wanted` were seen.
#[expect(
    clippy::expect_used,
    reason = "test helper: a missing message is the failure under test"
)]
async fn wait_for_messages(
    subscription: &mut async_nats::Subscriber,
    wanted: &[Uuid],
) -> Vec<Uuid> {
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while wanted.iter().any(|id| !seen.contains(id)) {
        let received = tokio::time::timeout_at(deadline, subscription.next()).await;
        assert!(
            received.is_ok(),
            "timed out waiting for {wanted:?}; saw {seen:?}"
        );
        let message = received
            .ok()
            .flatten()
            .expect("the subscription stays open");
        let id = message
            .headers
            .as_ref()
            .and_then(|headers| headers.get("Nats-Msg-Id"))
            .and_then(|value| value.as_str().parse::<Uuid>().ok())
            .expect("every relayed fact carries its event id");
        if wanted.contains(&id) {
            seen.push(id);
        }
    }
    seen
}

#[tokio::test]
async fn removal_row_is_published_and_later_rows_are_not_blocked() {
    let _broker = BROKER.lock().await;
    let database = TestDatabase::create().await.expect("a test database");
    let client = async_nats::connect(nats_url())
        .await
        .expect("the isolated NATS broker is reachable");
    let context = async_nats::jetstream::new(client.clone());
    let _commands = provision(&context).await;
    let mut facts = client
        .subscribe("evt.social.source.>")
        .await
        .expect("subscribing before the relay starts");

    let pool = database.database.pool();
    let removed = insert_outbox_row(
        pool,
        "social.source.removed.v1",
        serde_json::json!({"marker": "removed"}),
        2,
    )
    .await;
    let captured = insert_outbox_row(
        pool,
        "social.source.captured.v1",
        serde_json::json!({"marker": "captured"}),
        1,
    )
    .await;

    let connection = NatsConnection::connect(&nats_url())
        .await
        .expect("the service role connects");
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let worker_database = database.database.clone();
    let worker = tokio::spawn(async move {
        run_bus(connection, worker_database, async move {
            let _ignored = stop_rx.await;
        })
        .await
    });

    let seen = wait_for_messages(&mut facts, &[removed, captured]).await;
    assert_eq!(
        seen,
        vec![removed, captured],
        "the removal fact publishes first and does not stop the later fact"
    );
    // The relay marks a row published after its PubAck, which can land just after the subscriber
    // sees the message, so wait for the effect under a deadline rather than reading it once.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let unpublished = loop {
        let count: i64 = sqlx::query_scalar(
            "select count(*) from threads_archive.outbox_events where published_at is null",
        )
        .fetch_one(pool)
        .await
        .expect("counting unpublished rows");
        if count == 0 || tokio::time::Instant::now() >= deadline {
            break count;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(
        unpublished, 0,
        "both rows are marked published after PubAck"
    );

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

#[tokio::test]
async fn failing_head_row_backs_off_and_later_rows_still_publish() {
    let _broker = BROKER.lock().await;
    let database = TestDatabase::create().await.expect("a test database");
    let client = async_nats::connect(nats_url())
        .await
        .expect("the isolated NATS broker is reachable");
    let context = async_nats::jetstream::new(client.clone());
    let _commands = provision(&context).await;
    let mut facts = client
        .subscribe("evt.social.source.>")
        .await
        .expect("subscribing before the relay starts");

    // A payload beyond the broker's maximum is refused before it leaves the client: the cheapest
    // deterministic publish failure that does not depend on a broker permission.
    let pool = database.database.pool();
    let poison = insert_outbox_row(
        pool,
        "social.source.captured.v1",
        serde_json::json!({"marker": "x".repeat(2 * 1024 * 1024)}),
        2,
    )
    .await;
    let healthy = insert_outbox_row(
        pool,
        "social.source.updated.v1",
        serde_json::json!({"marker": "healthy"}),
        1,
    )
    .await;

    let connection = NatsConnection::connect(&nats_url())
        .await
        .expect("the service role connects");
    let worker_database = database.database.clone();
    let worker =
        tokio::spawn(
            async move { run_bus(connection, worker_database, std::future::pending()).await },
        );

    let seen = wait_for_messages(&mut facts, &[healthy]).await;
    assert_eq!(
        seen,
        vec![healthy],
        "the row behind the failing one publishes"
    );
    let outcome = tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("a failed publication stops the worker rather than being swallowed")
        .expect("the worker task joins");
    assert!(
        outcome.is_err(),
        "the worker surfaces the failed publication after the pass"
    );

    let (attempts, last_error, delayed, published): (i32, Option<String>, bool, bool) =
        sqlx::query_as(
            "select attempt_count, last_error, next_attempt_at > now(), published_at is not null \
             from threads_archive.outbox_events where event_id = $1",
        )
        .bind(poison)
        .fetch_one(pool)
        .await
        .expect("the failing row is still stored");
    assert_eq!(attempts, 1, "the failure is counted");
    assert_eq!(last_error.as_deref(), Some("publish_failed"));
    assert!(
        delayed,
        "the failing row backs off instead of retrying immediately"
    );
    assert!(!published, "a failed publication is never marked published");

    database
        .cleanup()
        .await
        .expect("dropping the test database");
}

/// Waits for the first message on `subscription` whose JSON body satisfies `wanted`.
#[expect(
    clippy::expect_used,
    reason = "test helper: a missing message is the failure under test"
)]
async fn next_matching(
    subscription: &mut async_nats::Subscriber,
    what: &str,
    wanted: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let received = tokio::time::timeout_at(deadline, subscription.next()).await;
        assert!(received.is_ok(), "timed out waiting for {what}");
        let message = received
            .ok()
            .flatten()
            .expect("the subscription stays open");
        let body: serde_json::Value =
            serde_json::from_slice(&message.payload).expect("every relayed fact is JSON");
        if wanted(&body) {
            return body;
        }
    }
}

/// The relay and the capture resolver running side by side, as the service runs them.
struct Pipeline {
    stop_bus: tokio::sync::oneshot::Sender<()>,
    stop_resolver: tokio::sync::oneshot::Sender<()>,
    bus: tokio::task::JoinHandle<Result<(), nats::NatsError>>,
    resolution: tokio::task::JoinHandle<
        Result<(), ratatoskr_threads_archive::capture_resolution::ResolverError>,
    >,
}

impl Pipeline {
    #[expect(
        clippy::expect_used,
        reason = "test helper: a pipeline that cannot start is the failure"
    )]
    async fn start(database: &TestDatabase, fake: &FakeOembed) -> Self {
        let connection = NatsConnection::connect(&nats_url())
            .await
            .expect("the service role connects");
        let raw_root =
            std::env::temp_dir().join(format!("ratatoskr-threads-raw-{}", Uuid::now_v7()));
        let resolver = CaptureResolver::new(
            database.database.clone(),
            ApprovedOembedClient::for_loopback_test(&fake.endpoint()).expect("a loopback endpoint"),
            RawObjectStore::new(raw_root),
            ResolverPolicy {
                max_attempts: 5,
                batch_size: 8,
                poll_interval: Duration::from_millis(20),
            },
        );
        let (stop_bus, bus_stopped) = tokio::sync::oneshot::channel::<()>();
        let (stop_resolver, resolver_stopped) = tokio::sync::oneshot::channel::<()>();
        let bus_database = database.database.clone();
        let bus = tokio::spawn(async move {
            run_bus(connection, bus_database, async move {
                let _ignored = bus_stopped.await;
            })
            .await
        });
        let resolution = tokio::spawn(async move {
            resolver
                .run(async move {
                    let _ignored = resolver_stopped.await;
                })
                .await
        });
        Self {
            stop_bus,
            stop_resolver,
            bus,
            resolution,
        }
    }

    #[expect(
        clippy::expect_used,
        reason = "test helper: a worker that fails to stop cleanly is the failure"
    )]
    async fn stop(self) {
        self.stop_bus.send(()).expect("stopping the relay");
        self.stop_resolver.send(()).expect("stopping the resolver");
        tokio::time::timeout(Duration::from_secs(5), self.bus)
            .await
            .expect("the relay stops")
            .expect("the relay task joins")
            .expect("the relay exits cleanly");
        tokio::time::timeout(Duration::from_secs(5), self.resolution)
            .await
            .expect("the resolver stops")
            .expect("the resolver task joins")
            .expect("the resolver exits cleanly");
    }
}

#[tokio::test]
async fn command_to_terminal_report_and_source_fact() {
    let _broker = BROKER.lock().await;
    let database = TestDatabase::create().await.expect("a test database");
    let client = async_nats::connect(nats_url())
        .await
        .expect("the isolated NATS broker is reachable");
    let context = async_nats::jetstream::new(client.clone());
    let _commands = provision(&context).await;
    let mut reports = client
        .subscribe("evt.platform.operation.reported.v1")
        .await
        .expect("subscribing to operation reports");
    let mut facts = client
        .subscribe("evt.social.source.captured.v1")
        .await
        .expect("subscribing to captured facts");

    let fake = FakeOembed::start(PERMALINK, vec![FakeReply::Oembed])
        .await
        .expect("the fake oEmbed endpoint binds");
    let pipeline = Pipeline::start(&database, &fake).await;

    let command = threads_command(Uuid::now_v7());
    let operation = command
        .correlation_id
        .to_string()
        .trim_start_matches("operation:")
        .to_owned();
    let tenant = command
        .tenant_id
        .expect("the command names its tenant")
        .to_string();
    let published = context
        .publish(
            COMMAND_SUBJECT,
            command
                .to_canonical_json()
                .expect("the command serializes")
                .into(),
        )
        .await
        .expect("publishing the command");
    published.await.expect("the command is stored by JetStream");

    let ours = |body: &serde_json::Value, status: &str| {
        body["correlation_id"] == format!("operation:{operation}")
            && body["payload"]["status"] == status
    };
    let queued = next_matching(&mut reports, "the queued report", |body| {
        ours(body, "queued")
    })
    .await;
    assert_eq!(queued["tenant_id"], tenant.as_str());
    let captured = next_matching(&mut facts, "the captured source fact", |body| {
        body["tenant_id"] == tenant.as_str()
    })
    .await;
    assert_eq!(captured["event_type"], "social.source.captured.v1");
    let terminal = next_matching(&mut reports, "the terminal report", |body| {
        ours(body, "succeeded")
    })
    .await;
    assert_eq!(terminal["tenant_id"], tenant.as_str());
    assert_eq!(terminal["payload"]["stage"], "capture_preserved");
    assert_eq!(
        terminal["payload"]["results"][0]["target"],
        format!(
            "social_source:{}",
            captured["payload"]["source"]["social_source_id"]
                .as_str()
                .expect("the fact names its source")
        ),
        "the report points at the source the fact announced"
    );
    assert_eq!(fake.requests(), 1);

    pipeline.stop().await;
    database
        .cleanup()
        .await
        .expect("dropping the test database");
}

/// A permalink the contract accepts (any `https` link) that Threads cannot map to a post.
const UNSUPPORTED_PERMALINK: &str = "https://example.com/@author/post/AbCd1";

/// Starts only the bus worker, the way the service runs it beside the resolver.
struct BusWorker {
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), nats::NatsError>>,
}

impl BusWorker {
    #[expect(
        clippy::expect_used,
        reason = "test helper: a worker that cannot start is the failure"
    )]
    async fn start(database: &TestDatabase) -> Self {
        let connection = NatsConnection::connect(&nats_url())
            .await
            .expect("the service role connects");
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let worker_database = database.database.clone();
        let task = tokio::spawn(async move {
            run_bus(connection, worker_database, async move {
                let _ignored = stopped.await;
            })
            .await
        });
        Self { stop, task }
    }

    #[expect(
        clippy::expect_used,
        reason = "test helper: a worker that fails to stop cleanly is the failure"
    )]
    async fn stop(self) {
        self.stop.send(()).expect("stopping the worker");
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .expect("the worker stops")
            .expect("the worker task joins")
            .expect("the worker exits cleanly");
    }
}

#[expect(
    clippy::expect_used,
    reason = "test helper: an unstored command is the failure"
)]
async fn publish_command(
    context: &async_nats::jetstream::Context,
    command: &ratatoskr_event_envelope::CommandEnvelope,
) {
    context
        .publish(
            COMMAND_SUBJECT,
            command
                .to_canonical_json()
                .expect("the command serializes")
                .into(),
        )
        .await
        .expect("publishing the command")
        .await
        .expect("the command is stored by JetStream");
}

#[tokio::test]
async fn rejected_but_attributable_command_gets_one_failed_report() {
    let _broker = BROKER.lock().await;
    let database = TestDatabase::create().await.expect("a test database");
    let client = async_nats::connect(nats_url())
        .await
        .expect("the isolated NATS broker is reachable");
    let context = async_nats::jetstream::new(client.clone());
    let commands = provision(&context).await;
    let mut reports = client
        .subscribe("evt.platform.operation.reported.v1")
        .await
        .expect("subscribing to operation reports");
    let worker = BusWorker::start(&database).await;

    let command = command_with(
        Uuid::now_v7(),
        UNSUPPORTED_PERMALINK,
        SocialCaptureProvider::Threads,
    );
    let operation = command
        .correlation_id
        .to_string()
        .trim_start_matches("operation:")
        .to_owned();
    let tenant = command
        .tenant_id
        .expect("the command names its tenant")
        .to_string();
    publish_command(&context, &command).await;

    let report = next_matching(&mut reports, "the failed report", |body| {
        body["correlation_id"] == format!("operation:{operation}")
    })
    .await;
    assert_eq!(report["tenant_id"], tenant.as_str());
    assert_eq!(
        report["causation_id"],
        format!("command:{}", command.command_id.0)
    );
    assert_eq!(report["payload"]["status"], "failed");
    assert_eq!(report["payload"]["stage"], "capture_unavailable");
    assert_eq!(
        report["payload"]["error"]["code"],
        "social.source.unavailable"
    );
    assert_eq!(report["payload"]["error"]["retryable"], false);

    // The same command arriving again (a redelivery after a lost ack) adds nothing.
    publish_command(&context, &command).await;
    let again = tokio::time::timeout(Duration::from_millis(1_500), async {
        loop {
            let message = reports.next().await?;
            let body: serde_json::Value = serde_json::from_slice(&message.payload).ok()?;
            if body["correlation_id"] == format!("operation:{operation}") {
                return Some(body);
            }
        }
    })
    .await;
    assert!(
        again.is_err(),
        "a redelivered rejection reports nothing more"
    );
    let stored: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.outbox_events \
         where event_type = 'platform.operation.reported.v1'",
    )
    .fetch_one(database.database.pool())
    .await
    .expect("counting reports");
    let rejected: i64 = sqlx::query_scalar(
        "select count(*) from threads_archive.inbox_events where handler_outcome = 'rejected'",
    )
    .fetch_one(database.database.pool())
    .await
    .expect("counting rejections");
    let captures: i64 = sqlx::query_scalar("select count(*) from threads_archive.captures")
        .fetch_one(database.database.pool())
        .await
        .expect("counting captures");
    assert_eq!((stored, rejected, captures), (1, 1, 0));

    let mut consumer = commands
        .get_consumer::<async_nats::jetstream::consumer::pull::Config>(CONSUMER_NAME)
        .await
        .expect("reading the preprovisioned consumer");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let counts = loop {
        let info = consumer.info().await.expect("reading consumer state");
        let counts = (info.num_pending, info.num_ack_pending);
        if counts == (0, 0) || tokio::time::Instant::now() >= deadline {
            break counts;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(counts, (0, 0), "both deliveries were settled");

    worker.stop().await;
    database
        .cleanup()
        .await
        .expect("dropping the test database");
}

#[tokio::test]
async fn unattributable_commands_are_terminated_without_a_report() {
    let _broker = BROKER.lock().await;
    let database = TestDatabase::create().await.expect("a test database");
    let client = async_nats::connect(nats_url())
        .await
        .expect("the isolated NATS broker is reachable");
    let context = async_nats::jetstream::new(client.clone());
    let _commands = provision(&context).await;
    let mut terminated = client
        .subscribe(format!(
            "$JS.EVENT.ADVISORY.CONSUMER.MSG_TERMINATED.{COMMAND_STREAM}.{CONSUMER_NAME}"
        ))
        .await
        .expect("subscribing to termination advisories");
    let worker = BusWorker::start(&database).await;

    context
        .publish(COMMAND_SUBJECT, "{\"not\":\"an envelope\"}".into())
        .await
        .expect("publishing garbage")
        .await
        .expect("the garbage is stored by JetStream");
    publish_command(
        &context,
        &command_with(Uuid::now_v7(), PERMALINK, SocialCaptureProvider::Instagram),
    )
    .await;

    for what in ["the undecodable command", "the wrong-provider command"] {
        let advisory = tokio::time::timeout(Duration::from_secs(10), terminated.next()).await;
        assert!(
            advisory.is_ok_and(|message| message.is_some()),
            "{what} is terminated, not acknowledged"
        );
    }
    let reports: i64 = sqlx::query_scalar("select count(*) from threads_archive.outbox_events")
        .fetch_one(database.database.pool())
        .await
        .expect("counting outbox rows");
    assert_eq!(
        reports, 0,
        "nothing is attributable, so nothing is reported"
    );

    worker.stop().await;
    database
        .cleanup()
        .await
        .expect("dropping the test database");
}

#[tokio::test]
async fn a_failing_inbox_write_naks_and_the_command_completes_on_redelivery() {
    let _broker = BROKER.lock().await;
    let database = TestDatabase::create().await.expect("a test database");
    let client = async_nats::connect(nats_url())
        .await
        .expect("the isolated NATS broker is reachable");
    let context = async_nats::jetstream::new(client.clone());
    let _commands = provision(&context).await;
    let mut reports = client
        .subscribe("evt.platform.operation.reported.v1")
        .await
        .expect("subscribing to operation reports");
    let worker = BusWorker::start(&database).await;
    let pool = database.database.pool();

    sqlx::query("alter table threads_archive.inbox_events rename to inbox_events_offline")
        .execute(pool)
        .await
        .expect("taking the inbox offline");
    let command = threads_command(Uuid::now_v7());
    let operation = command
        .correlation_id
        .to_string()
        .trim_start_matches("operation:")
        .to_owned();
    publish_command(&context, &command).await;
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert!(
        !worker.task.is_finished(),
        "a transient inbox failure must not end the worker"
    );

    sqlx::query("alter table threads_archive.inbox_events_offline rename to inbox_events")
        .execute(pool)
        .await
        .expect("bringing the inbox back");
    let queued = next_matching(&mut reports, "the queued report after redelivery", |body| {
        body["correlation_id"] == format!("operation:{operation}")
    })
    .await;
    assert_eq!(queued["payload"]["status"], "queued");

    worker.stop().await;
    database
        .cleanup()
        .await
        .expect("dropping the test database");
}
