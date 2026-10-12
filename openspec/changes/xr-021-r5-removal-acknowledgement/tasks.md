## 0. Pin

- [x] 0.1 Move every `ratatoskr-*` git dependency to `d98967b7b7ee80442c93ef4657140068955c8f61` (`ratatoskr-operation-contracts` is already a direct dependency of the crate) and refresh `Cargo.lock`. A dependency pin cannot start from a failing test; the full gate is the check.

## 1. State machine and pump (R5-11 rule 4)

- [x] 1.1 RED: add `crates/threads-archive/tests/removal_acknowledgement.rs` (real PostgreSQL, fake transport) with the four columns in `schema.sql` and signature-only stubs of the transitions and the transport-generic pump; the cases arm `next_resend_at` for a removal only, select and republish a due unacknowledged removal with the same `event_id`, back off a failed republish without touching `published_at`, and end the resend on `applied` and `refused`; each fails on an assertion.
- [x] 1.2 GREEN: implement the transitions in `removal_acknowledgement.rs`, move the pump to `outbox_relay.rs` with the resend selection and bookkeeping, and add `RATATOSKR__TOMBSTONE__RESEND_AFTER_SECONDS` and `RATATOSKR__TOMBSTONE__RESEND_CAP_SECONDS` to the configuration and `deploy/systemd/threads.conf.example`.
- [x] 1.3 RED: add the `tombstone` cases to `crates/threads-archive/tests/config.rs` (defaults, range, cap below delay, both violations reported together).
- [x] 1.4 GREEN: implement the strict keys.

## 2. The consumer (R5-11 rules 3 and 4)

- [x] 2.1 RED: add `crates/threads-archive/tests/nats_deletion_acks.rs` (real broker under the deployed ACL fragment with the Edge-provisioned durable, PostgreSQL) and `services/threads-archive/tests/deletion_acks.rs` (the binary: gauges move, readiness stays green) against a signature-only `deletion_acks` module.
- [x] 2.2 GREEN: implement `deletion_acks.rs`, the supervised worker, the gauges and the counter, the wiring in `main.rs`, the stanza in `deploy/nats/identity.conf`, and the README, DEVELOPMENT and conf-example text.

## 3. A removal is never dropped while unacknowledged

- [x] 3.1 RED/characterization: add `crates/threads-archive/tests/privacy_deletion_removals.rs`, one test per deleting path. They pass at once because the existing delete already excludes every removal row, so they cannot be run RED; with that exclusion removed the connection-target test and the existing `connection_only_sources_each_emit_one_removal_and_replay_is_idempotent` fail (one removal survives instead of three and two), which shows they are not vacuous. The capture-target test passes even then, because a removal is filed under the deleted capture's own id and no later capture deletion filters on it; it pins the behaviour and cannot be made to fail by that mutation.
- [x] 3.2 GREEN: nothing beyond the existing predicate; document it in the privacy-deletion spec and the design.

## 4. Gate

- [x] 4.1 Run the full local gate (`cargo fmt`, `cargo clippy`, the 850-line check, `cargo test --workspace --locked`, `cargo test --workspace --locked --doc`, `cargo deny --locked check`, `openspec validate --all --strict`, `openspec validate --archived`) and record one real-broker transcript: publish, no acknowledgement, resend with the same `Nats-Msg-Id`, acknowledgement, stop.
