## 1. Reproduce the failing gate

- [x] 1.1 This is CI configuration and a dependency-registry fact, not a new unit test, so the failing gate command is the failing test: `build-gate cargo deny --locked check` on the pre-fix `Cargo.lock` exits nonzero with `error[yanked]: detected yanked crate (try 'cargo update -p chacha20')` at `Cargo.lock:24`, resolving `chacha20 v0.10.1 <- rand v0.10.2 <- async-nats v0.50.0 <- ratatoskr-threads-archive v0.1.0` (and `-service`), and the run's own summary line `advisories FAILED, bans ok, licenses ok, sources ok`. Confirmed this blocks the documented gate chain: the full `DEVELOPMENT.md` command sequence run as one `&&` chain stopped at this exact step and never reached `cargo fmt`, `clippy`, or any test.

## 2. Move the lockfile off the yanked version

- [x] 2.1 `cargo update -p chacha20 --dry-run` confirmed the only move is `chacha20 v0.10.1 -> v0.10.2`, `11 unchanged dependencies behind latest`; ran `cargo update -p chacha20` for real. `Cargo.lock` diff is two lines (`version` and `checksum` on the `chacha20` entry only). No `Cargo.toml` edit.

## 3. Verify the repair

- [x] 3.1 `build-gate cargo deny --locked check` — `advisories ok, bans ok, licenses ok, sources ok`.
- [x] 3.2 Full documented gate from `DEVELOPMENT.md`, run together with the `fix-threads-archive-ci-gate` change (same tree, same commit): `cargo fetch --locked`, `cargo deny --locked check`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, the 850-line file-size ratchet, `cargo build --workspace --locked`, `cargo test --workspace --locked` (150 tests, all `ok`), `cargo test --workspace --locked --doc`, `cargo build --workspace --locked --release` — all passed; log ends `FULL_GATE_ALL_PASSED`. See `fix-threads-archive-ci-gate/tasks.md` task 4.4 for the full step-by-step record.
- [x] 3.3 `openspec validate --all --strict` — `Totals: 18 passed, 0 failed` (includes both `change/fix-threads-archive-ci-gate` and `change/fix-yanked-chacha20-lockfile`). `openspec validate --archived` — `Totals: 9 passed, 0 failed`.
