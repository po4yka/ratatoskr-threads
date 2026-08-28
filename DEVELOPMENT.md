# Developing Ratatoskr Threads

> Status: Active development
> Last reviewed: 2026-08-25

Implementation plan items 1 through 9 are implemented. Item 8 provides owner-scoped immutable
archive receipts, safe bounded ZIP inspection/extraction, deterministic `threads-export-v1`
projection and relations, unknown-section retention, replay-safe reconciliation, and persisted
owner-scoped completeness evidence. An export's absence never changes a capture or becomes a
deletion/native-Saved assertion. Item 9 adds fail-closed media retention, owner deletion and
Knowledge removal propagation, finite public re-resolution, and restartable parser reprocessing.

## Intended toolchain

Rust/Tokio (pinned by `rust-toolchain.toml` at 1.97.0), SQLx/PostgreSQL, axum, Reqwest/Rustls,
`zip` with only its `deflate` feature, tracing, Prometheus, NATS, and media policy. Planned for
later items: provider fixtures/WireMock and testcontainers.

`RATATOSKR__BUS__URL` is required. Production supplies an nkey through
`RATATOSKR__BUS__NKEY_SEED_PATH`; the local unauthenticated NATS test broker may omit it.

## Code size limits

`clippy.toml` beside the root `Cargo.toml` carries the limits: functions at most 100 lines of code, signatures at most 7 arguments, block nesting at most 5 deep, plus `allow-unwrap-in-tests` and the disallowed direct environment reads outside the config module. The numbers are the fresh-tree baseline, not an ambition; an exception is a site-level `#[expect]` with a reason. The gate also enforces the one limit clippy cannot express: no tracked `.rs` file may exceed 850 lines.

## Current validation

The repository has two gates. The docs-only/OpenSpec gate stays unchanged:

```bash
git diff --check
openspec validate --all --strict
openspec validate --archived
```

`.github/workflows/openspec.yml` runs the two OpenSpec commands in CI; `.github/workflows/fleet.yml`
keeps checking its invariants now that a manifest exists.

### Rust — also the CI gate

```bash
cargo fetch --locked
cargo deny --locked check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
cargo test --workspace --locked
cargo test --workspace --locked --doc
cargo build --workspace --locked --release
```

`.github/workflows/ci.yml` runs this list against PostgreSQL 17 (service container in CI,
`compose.yaml` on a laptop: user/password/database `threads`, published on `127.0.0.1:5437`) and a
JetStream-enabled NATS broker (an explicit `docker run ... -js` step in CI, since a `services:`
container cannot be given a command; `compose.yaml` on a laptop, published on `127.0.0.1:5422`). The
suite creates disposable databases from the embedded schema per test; without the server the suite
fails rather than skips. CI additionally runs the 850-line file ratchet and a guard asserting this
command list is byte-identical to `.github/workflows/ci.yml`.

## Local run

```bash
docker compose up -d
cargo run -p ratatoskr-threads-archive-service
# operator plane on 127.0.0.1:9084: /health/live /health/ready /metrics /version
```

`RATATOSKR__STORAGE__DATABASE_URL=postgres://threads:threads@127.0.0.1:5437/threads` is
required to start; `<binary> check-config` validates configuration without binding (exit 78 when
invalid).

Parser reprocessing is an explicit operator command:

```bash
ratatoskr-threads-archive reprocess-export dry-run --owner UUID --run-id UUID --parser threads-export-v1-parser-1
ratatoskr-threads-archive reprocess-export apply --owner UUID --run-id UUID --parser threads-export-v1-parser-1 --operation-id UUID
```

The command writes one newline-terminated JSON document to stdout, diagnostics only to stderr, and
uses exits `0` success, `1` operational/output failure, `2` grammar failure, and `78` configuration
failure. Dry-run writes no run, item, checkpoint, outbox, or blob state. Apply validates plan/state
fingerprints and resumes committed chunks. It never changes `schema.sql` and is not database
migration tooling.

## Workflow

1. Verify the capability exists for the connected account type and current granted scopes.
2. Record acquisition method and saved authority explicitly.
3. Resolve only public content through supported official mechanisms; preserve unavailable/private state.
4. Store raw export/capture evidence before normalization and preserve unknown records.
5. Test privacy, expiry, replay, importer limits, media policy, and no-cookie/no-hidden-API invariants.
6. Treat `social.source.captured.v1` and `social.source.updated.v1` as facts that request downstream analysis; preserve only digest-scoped completion linkage locally.

Default tests use synthetic exports and no personal account credentials.

## What a clone needs before you plan a change

A change is planned with OpenSpec, which is a CLI a clone installs for itself. Use the version
`.github/workflows/openspec.yml` pins, so your terminal and the gate answer the same:

```bash
npm install --global @fission-ai/openspec@1.10.0
```

Cross-repository behaviour lives in a store, and registering one is per-machine state that no
repository can turn on for you — the same kind of step as `git config core.hooksPath .githooks`:

```bash
git clone git@github.com:po4yka/ratatoskr-workspace.git <path>
openspec store register <path> --id ratatoskr-workspace
```

`openspec doctor` reports whether both are in place.

## The Rust skills in this repository

`.agents/skills/` holds eighteen Rust skills vendored from `po4yka/rust-skills`, and
`.claude/skills/` symlinks to them. Unlike the steps above this needs nothing from your machine: the
files are in the tree, so a fresh clone already has them.

Update them with the catalogue and never by hand:

```bash
npx skills update
```

That rewrites `.agents/skills/` and `skills-lock.json` from the catalogue. Run it in one repository,
read the diff, then apply the same change to every Ratatoskr repository whose stack is Rust.
`ratatoskr-workspace/.github/workflows/drift.yml` fails when one copy differs from the others.
