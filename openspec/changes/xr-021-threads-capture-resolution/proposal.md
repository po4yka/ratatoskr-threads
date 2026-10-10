## Why

Three defects in `ratatoskr-threads` stop an explicit browser capture from ever completing, found by changeset XR-021 (cross-repository integration defects):

1. One `social.source.removed.v1` outbox row aborts the whole outbox pump with `NatsError::UnexpectedOutboxEventType`, so every later row (including operation reports and captured facts) never publishes. The Threads NATS identity does not yet list `evt.social.source.removed.v1`, and the relay mapped only two of the four event types the service writes.
2. The NATS worker's failure is only logged. `/health/ready` stays 200 and the process stays up while nothing is consumed or published.
3. Intake leaves `next_resolution_at` null and nothing calls `ApprovedOembedClient::resolve_and_record`, so every capture stops at `Queued`: Platform never sees a terminal `succeeded` or `failed` report and Knowledge never receives `social.source.captured.v1` for an explicit capture.

Cross-repository behaviour (report shape, retry policy, exactly-once terminal report, bus lifecycle rules, the Threads publish ACL) is defined in XR-021 CONTRACTS.md sections S02, S03, S10 (CD1, CD2, CD5, CD7); this change cites it and does not restate it.

## What Changes

- Pin every `ratatoskr-*` git dependency to the one wave-1 contracts commit `ad16855c4e7f3d52cd118274faa3b8f3ab4da576` (a full SHA; the old pin was the short SHA `f0dc35f`).
- The outbox relay maps `social.source.removed.v1` to `evt.social.source.removed.v1`. An unmapped event type remains a hard error that stops the worker, but `schema.sql` now carries an `event_type` CHECK on `outbox_events` so such a row cannot be inserted. A failing publish increments `attempt_count` and sets `next_attempt_at` with backoff so a failing head row never starves later rows.
- The bus worker reports its own completion; a return before the stop signal marks readiness false and exits the process non-zero. `RuntimeState` gains a bus readiness bit.
- Intake stores the Platform operation id, schedules resolution (`next_resolution_at = now()`), and builds the queued report with the contracts builders. `schema.sql` gains `captures.operation_id`, `captures.resolution_attempts`, `captures.operation_reported_at` and the partial index `captures_resolution_due_idx`.
- `config.rs` gains `RATATOSKR__PUBLIC_RESOLUTION__{OEMBED_ENDPOINT,RAW_OBJECT_ROOT,MAX_ATTEMPTS,BATCH_SIZE,POLL_INTERVAL_MS}`.
- A new `CaptureResolver` claims due captures, resolves them through the credential-free oEmbed client, classifies provider failures, retries transient ones on the 30 s / 2 min / 8 min / 30 min schedule, and sends exactly one terminal operation report per operation inside the transaction that sets `operation_reported_at`. It runs as its own task next to the NATS worker.
- BREAKING: the process refuses to start when the bus is configured (always, today) and `RATATOSKR__PUBLIC_RESOLUTION__RAW_OBJECT_ROOT` is unset. `schema.sql` changed in place (no migration; recreate development databases).

## Capabilities

### New Capabilities

- `capture-resolution`: the resolver worker, its failure classification, bounded retry, exactly-once terminal reporting, and its configuration.

### Modified Capabilities

- `service-runtime`: a stopped bus worker makes the process not ready and exit non-zero.
- `social-source-publishing`: the removal fact is published and a poison row cannot block later rows.

## Impact

- `Cargo.toml`, `Cargo.lock`, `schema.sql`, `crates/threads-archive/src/{nats,browser_capture_inbox,config,publishing,capture_resolution}.rs`, `services/threads-archive/src/{main,lib}.rs`, tests, `README.md`, `DEVELOPMENT.md`.
- Out of scope: Knowledge ingestion of social facts, Threads OAuth lanes, erasure for the new columns, any ACL edit (Platform owns `ratatoskr.conf`).
