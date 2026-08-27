## 1. Official own-content boundary and capability no-op

- [x] 1.1 Add redacted `crates/threads-archive/tests/fixtures/official-own-content-page.json` and `official-own-content-reply-page.json` provider fixtures, and verify they contain no live credentials, user data, or native-Saved assertion. No RED applies because fixtures only supply deterministic input to the following behavior tests.
- [x] 1.2 Add and run the RED integration test `sync_without_own_content_capability_is_a_non_mutating_no_op` in `crates/threads-archive/tests/own_account_sync.rs`; it must compile and fail because the result is not `NoOp`, or the fake provider's listing-call count is not zero, before sync behavior is implemented. Verify with `build-gate -- cargo test --locked -p ratatoskr-threads-archive --test own_account_sync sync_without_own_content_capability_is_a_non_mutating_no_op`.
- [x] 1.3 Implement the narrow official own-content adapter seam, account capability check, and typed no-op result; re-run `sync_without_own_content_capability_is_a_non_mutating_no_op` green with its zero-call and unchanged-checkpoint assertions.

## 2. Checkpointed incremental scans

- [x] 2.1 Add and run the RED integration test `completed_scan_advances_and_reuses_account_watermark` in `crates/threads-archive/tests/own_account_sync.rs`; it failed because the second fake request did not receive the first completed scan's watermark and the durable checkpoint did not equal the returned next watermark. Verified with `build-gate -- cargo test --locked -p ratatoskr-threads-archive --test own_account_sync completed_scan_advances_and_reuses_account_watermark`.
- [x] 2.2 Edit the current `schema.sql` definition in place to add account-bound synchronization checkpoints, then implement bounded page ingestion and atomic checkpoint advance; re-ran `completed_scan_advances_and_reuses_account_watermark` green. No migration file or migration tooling was added.
- [x] 2.3 Add and run `failed_scan_keeps_the_previous_account_watermark` in `crates/threads-archive/tests/own_account_sync.rs`. It was green on its first run because task 2.2 already uses one database transaction and performs provider I/O before the transaction; no independent RED remains to reproduce honestly. Verified with `build-gate -- cargo test --locked -p ratatoskr-threads-archive --test own_account_sync failed_scan_keeps_the_previous_account_watermark`.
- [x] 2.4 Verified failed scans retain the prior checkpoint and do not begin a normalized transaction; `failed_scan_keeps_the_previous_account_watermark` remains green.

## 3. Authoritative projections, reply relations, and source facts

- [x] 3.1 Add and run the RED integration test `official_observation_atomically_swaps_a_captured_post_authority` in `crates/threads-archive/tests/own_account_sync.rs`; it failed because the source-update assertion observed one event rather than two. Verified with `build-gate -- cargo test --locked -p ratatoskr-threads-archive --test own_account_sync official_observation_atomically_swaps_a_captured_post_authority`.
- [x] 3.2 Implement raw-first official observation storage, stable-ID projection upsert, relation persistence, and one-transaction authority swap; re-ran `official_observation_atomically_swaps_a_captured_post_authority` green.
- [x] 3.3 Add and run the RED integration test `official_reply_publishes_its_parent_relation_with_official_provenance` in `crates/threads-archive/tests/own_account_sync.rs`; it failed because no official source fact existed. Verified with `build-gate -- cargo test --locked -p ratatoskr-threads-archive --test own_account_sync official_reply_publishes_its_parent_relation_with_official_provenance`.
- [x] 3.4 Generalize source ownership/origin and transactional publication for official observations without fabricating captures, then re-ran `official_reply_publishes_its_parent_relation_with_official_provenance` green.

## 4. Scheduled invocation and capability contract

- [x] 4.1 Add and run the paused-Tokio test `scheduled_sync_tick_invokes_the_account_worker_once` in `services/threads-archive/tests/own_account_sync.rs`. It was introduced with the narrow scheduler seam and therefore has no independent RED result to report; it verifies exactly one due worker invocation without sleep. Verified with `build-gate -- cargo test --locked -p ratatoskr-threads-archive-service --test own_account_sync scheduled_sync_tick_invokes_the_account_worker_once`.
- [x] 4.2 Add configured Tokio scheduling that invokes the tested account-sync entry point and is disabled unless explicitly configured, then re-ran `scheduled_sync_tick_invokes_the_account_worker_once` green without a sleep-based test.
- [x] 4.3 Add and run the RED unit test `own_account_sync_is_supported_after_item_seven` in `crates/threads-archive/src/capability.rs`; it failed because `AcquisitionMode::OwnAccountSync` was still `Planned`. Verified with `build-gate -- cargo test --locked -p ratatoskr-threads-archive own_account_sync_is_supported_after_item_seven`.
- [x] 4.4 Mark the matrix mode supported and update OAuth reconciliation so a granted supported own-content capability is available without launching a scan; re-ran `own_account_sync_is_supported_after_item_seven` green.

## 5. Documentation and full gate

- [x] 5.1 Update `README.md`, `DEVELOPMENT.md`, and relevant architecture/capability documentation to state the implemented bounded official own-post/reply sync semantics, checkpoint limitation, and non-capabilities; verified documentation never claims account-history completeness or native Saved synchronization. No RED applies because this documents already-tested behavior.
- [x] 5.2 Run `git diff --check`, `openspec validate --all --strict`, and `openspec validate --archived`; verified all docs/OpenSpec checks pass.
- [x] 5.3 Run the complete documented product gate through `build-gate --` (including PostgreSQL-backed workspace tests, docs tests, lint, deny, debug, and release builds) and verified every command exited successfully.
- [x] 5.4 Inspected the final diff, ticked only verified tasks, and validated the completed change; archive this completed change with the OpenSpec archive workflow and verify `openspec validate --archived` passes.
