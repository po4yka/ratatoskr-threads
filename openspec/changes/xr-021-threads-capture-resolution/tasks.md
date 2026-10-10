## 1. Contracts pin

- [x] 1.1 Configuration task, no failing test applies: replace the four short-SHA `rev = "f0dc35f"` pins in `Cargo.toml` with `ad16855c4e7f3d52cd118274faa3b8f3ab4da576`, refresh `Cargo.lock`, fix additive-drift compile fallout, run the existing suite green and `cargo deny --locked check`.

## 2. Publish the removal fact and stop the poison-pill flush

- [x] 2.1 RED: in `crates/threads-archive/src/nats.rs` rewrite `outbox_publication_is_limited_to_the_threads_acl` to assert `outbox_subject("social.source.removed.v1") == Some("evt.social.source.removed.v1")` (it asserts `None` today); add `crates/threads-archive/tests/nats_browser_capture.rs::removal_row_is_published_and_later_rows_are_not_blocked` and `crates/threads-archive/tests/schema.rs::outbox_refuses_unlisted_event_types`. Fails today with `UnexpectedOutboxEventType` on the removed row and no CHECK.
- [x] 2.2 GREEN: add `SOCIAL_REMOVED_SUBJECT` and the mapping, the `event_type` CHECK, `last_error`, and `attempt_count`/`next_attempt_at` backoff in `flush_outbox`.

## 3. A stopped bus worker makes the service not ready and exits

- [x] 3.1 RED: `services/threads-archive/tests/boot.rs::bus_worker_exit_flips_readiness_and_exit_code` kills the broker after boot and asserts readiness 503 and a non-zero exit. Fails today (ready stays 200).
- [x] 3.2 GREEN: `RuntimeState::set_bus_ready`, the worker completion channel and the supervising select in `main.rs`.

## 4. Record the operation id and schedule resolution at intake

- [x] 4.1 RED: `crates/threads-archive/tests/browser_capture_command.rs::intake_stores_operation_and_schedules_resolution`. Fails today (`operation_id` column absent, `next_resolution_at` null).
- [x] 4.2 GREEN: `schema.sql` columns and index; `browser_capture_inbox.rs` inserts them and builds the queued report with the contracts builders; delete the local `queued_report`.

## 5. Configure the public resolver and classify provider failures

- [x] 5.1 RED: `crates/threads-archive/tests/config.rs` (new keys, require-when-bus-set, defaults, refusals) and `crates/threads-archive/tests/capture_resolution_classify.rs` (the classification table). Fails today (closed key set; `classify` absent behind a signature-only stub returning a neutral value).
- [x] 5.2 GREEN: `config.rs` keys and rule; `classify` in `capture_resolution.rs`.

## 6. Capture resolution worker

- [x] 6.1 RED: `crates/threads-archive/tests/capture_resolution.rs` with a local HTTP fake of the oEmbed endpoint: 200, 404, 403, 503 x4 then 200, 503 x5, crash between record and report. Fails today (nothing calls `resolve_and_record`, no terminal report producer).
- [x] 6.2 GREEN: `CaptureResolver::run_due_once`, `publishing::social_source_id_for`, the resolver task in `main.rs`, startup refusal.

## 7. Composed end-to-end test

- [x] 7.1 RED: `crates/threads-archive/tests/nats_browser_capture.rs::command_to_terminal_report_and_source_fact` is red until tasks 2 to 6 land together; it is written and run first against the pre-resolver tree.
- [x] 7.2 GREEN: passes with the resolver and relay running against the fake oEmbed server and a real broker.

## 8. Answer refused commands, settle messages by S02 item 7, and keep the resolver alive on a vanished source

- [x] 8.1 RED: `crates/threads-archive/tests/nats_browser_capture.rs::{rejected_but_attributable_command_gets_one_failed_report, unattributable_commands_are_terminated_without_a_report, a_failing_inbox_write_naks_and_the_command_completes_on_redelivery}` and `crates/threads-archive/tests/capture_resolution.rs::a_resolved_capture_whose_source_is_gone_is_reported_instead_of_stopping_the_resolver`. Fail today: the unsupported-permalink command is acknowledged silently (no report), garbage is acknowledged not terminated, a failed inbox write ends the worker, and the orphaned capture makes `run_due_once` return `MissingSource`.
- [x] 8.2 GREEN: `BrowserCaptureCommand::intake` and `RejectedCommand`, `BrowserCaptureInbox::reject` (inbox marker `rejected` plus the failed report in one transaction, filed under the operation), `outbox_events.aggregate_type` accepts `operation`, `nats::handle_delivery` settles Ack, Term or Nak(2 s), and the resolver reports a vanished source as `Inaccessible`.

## 9. Documentation and final gate

- [x] 9.1 Documentation and gate task, no failing test applies: `README.md`, `DEVELOPMENT.md` (name the breaks: the process refuses to start without `RATATOSKR__PUBLIC_RESOLUTION__RAW_OBJECT_ROOT`; `schema.sql` changed in place), `openspec validate --all --strict`, `openspec validate --archived`, and the full DEVELOPMENT.md gate.
