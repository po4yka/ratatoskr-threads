## Why

`Cargo.lock` locks `chacha20 0.10.1`, pulled in transitively through `rand 0.10.2` from `async-nats = "=0.50.0"`. crates.io yanked `chacha20 0.10.1` (and `0.10.0`) after this repository's last CI run that reached `cargo deny --locked check` (33092775164, 2026-08-27); only `0.10.2` remains unyanked in that line. `deny.toml` sets `[advisories] yanked = "deny"`, so `cargo deny --locked check` now fails with `error[yanked]: detected yanked crate (try 'cargo update -p chacha20')` at `Cargo.lock:24`, blocking `ci`'s `gate` job (which runs the check immediately after `cargo fetch --locked`, before any test step) and the scheduled `advisories` workflow's `cargo deny check advisories` step. This is unrelated to the timestamp-precision and NATS-provisioning defects fixed in `fix-threads-archive-ci-gate`; it surfaced independently while verifying that change's full documented gate, and would fail `ci` for any commit regardless of that change's content, since it depends only on the live crates.io registry state at check time, not on anything in this repository's tree.

## What Changes

- Run `cargo update -p chacha20` to move the locked resolution from the yanked `0.10.1` to the current `0.10.2`, and commit the resulting two-line `Cargo.lock` diff (version and checksum only).
- No `Cargo.toml` edit: `chacha20` is a transitive dependency, not a direct one, and `async-nats = "=0.50.0"` stays pinned exactly as before.

## Capabilities

No contract or externally-visible behaviour changes; this only moves a transitive lockfile entry to an unyanked patch version of the same crate. `skip_specs: true` is set in the change manifest.

## Impact

- `Cargo.lock` (`chacha20` entry only).
- `.github/workflows/ci.yml` `gate` job (`cargo deny --locked check`) and `.github/workflows/advisories.yml` (`cargo deny check advisories`) — no edits to either file, but both are what this unblocks.
- No other lockfile entry moves and no source code changes.
