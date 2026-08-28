## Context

See [proposal.md](proposal.md). Two independent defects sit behind the one red `ci` / `gate` run, and both had been masked until now: a wildcard-import clippy error (commit 50a5d2c) and then the 850-line file-size gate (commit eb59627) each failed the job before `cargo test --workspace --locked` reached the test binary that carries the timestamp assertion. `cargo test --workspace --locked` has no `--no-fail-fast`, so once the assertion itself started failing (commit 2e5510c) it in turn hid the fact that `.github/workflows/ci.yml` never provisions a NATS broker for the three JetStream-dependent test binaries the same feature commit (50a5d2c) added.

## Goals / Non-Goals

**Goals:**

- Make `BrowserCaptureCommand::captured_at()` hold exactly the value that will exist after persistence, so no future comparison against a re-read row can fail on precision alone.
- Provision a real, healthy, JetStream-enabled NATS broker in CI at the address the three affected test binaries already hardcode, so `cargo test --workspace --locked` exercises `crates/threads-archive/tests/nats_browser_capture.rs`, `services/threads-archive/tests/boot.rs`, and `services/threads-archive/tests/reprocess_export.rs` for the first time since they were added.
- Leave `compose.yaml`'s NATS healthcheck able to actually pass, since it is dead code today (a service nothing depends on, but a false "unhealthy" is still a defect a developer would eventually trip over).

**Non-Goals:**

- Widen `threads_archive.captures.captured_at`, change its column type, or otherwise touch `schema.sql`. `timestamptz` losing sub-microsecond precision is normal PostgreSQL behaviour, not a defect.
- Patch only the test's assertion (e.g. truncating `command.captured_at()` at the comparison site instead of at `parse`). That would make this one test pass without fixing what the returned value actually represents — the next caller that persists a capture and compares its `captured_at()` against a re-read row hits the identical mismatch. Truncating at the domain boundary, where the value first becomes a `BrowserCaptureCommand`, removes the landmine at its source instead of at one call site.
- Add `--no-fail-fast` to `cargo test --workspace --locked`. It would have surfaced the NATS gap two commits earlier, and is a reasonable defense-in-depth for future masking chains, but is not required to close either defect here and is left as a suggestion rather than folded into this change.
- Enable JetStream on the NATS service via a GitHub Actions `services:` `image:`/`options:` entry. A service container's `options:` are `docker create` flags, not command arguments, and `nats:2-alpine`'s image needs `-js` passed as a command argument (it is not env-var-configurable, verified against `nats-server --help`); a plain `docker run` step is the only way to pass it without publishing a custom image, and every other Ratatoskr repository with a NATS-backed test suite already does exactly this.

## Decisions

**`browser_capture_command.rs`**: call `.trunc_subsecs(6)` (`chrono::SubsecRound`) on the parsed `DateTime<Utc>` immediately after `.with_timezone(&Utc)`, before it becomes part of `Self`. `trunc_subsecs` floors rather than rounds, matching how `sqlx`/PostgreSQL actually discard the sub-microsecond remainder on write (verified: PostgreSQL's microsecond-resolution internal representation truncates, it does not round, an over-precise input).

**`ci.yml`**: an explicit container step, matching the pattern already in `ratatoskr-platform`, `ratatoskr-extractor`, `ratatoskr-x`, `ratatoskr-instagram`, and `ratatoskr-telegram`'s own `ci.yml` files — `docker run -d ... nats:2-alpine@<digest> -js -m 8222`, polled via `curl -sf http://127.0.0.1:8222/healthz` before the build steps run. `-m 8222` is required for the same reason `compose.yaml` needed it: passing `-js` alone as the container's command replaces the image's default `--config /etc/nats/nats-server.conf`, which is what would otherwise have set `monitor_port: 8222`. The image is pinned by the same digest already used by `ratatoskr-instagram`'s and `ratatoskr-telegram`'s `ci.yml` for `nats:2-alpine`, verified resolvable against the Docker Hub registry API independently of this machine's local Docker credential state.

**`compose.yaml`**: the same `-m 8222` addition to the `nats` service's `command`, so its existing `healthcheck` (which already correctly targets `http://127.0.0.1:8222/healthz`) can pass.

## Risks / Trade-offs

- [Truncating `captured_at` at parse time could be read as silently discarding data the platform sent] — the platform's own contract commits to storing this value in a microsecond-resolution `timestamptz`; nothing downstream of this service ever observes or depends on the sub-microsecond remainder, and PostgreSQL was always going to discard it. Making the in-memory value match what will actually be durable is the more honest representation, not a new loss.
- [A `docker run` step outside `services:` is not automatically torn down the way a service container is if a later step in the same job fails] — the GitHub Actions runner VM is destroyed at the end of the job regardless, so this has no observable effect on CI; it would only matter for a self-hosted, long-lived runner, which this fleet does not use (`runs-on: ubuntu-latest`).
- [Pinning `nats:2-alpine` by a digest that is not the tag's current `HEAD`] — deliberate, same as `postgres:17@sha256:...` above it in the same file: a moving tag makes CI non-reproducible; the pinned digest remains pullable indefinitely once published, which was verified directly against the registry rather than assumed.

## Migration Plan

No rollout coordination: one production-code precision fix plus CI/local-dev provisioning. Merge once the documented local gate, the previously-failing test, the two newly-reachable NATS-dependent test binaries, and `openspec validate --all --strict` all pass.
