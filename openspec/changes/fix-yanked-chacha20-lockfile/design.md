## Context

See [proposal.md](proposal.md). `deny.toml`'s comment on `yanked = "deny"` states the rationale directly: "a yanked lockfile entry is a build that cannot be reproduced." That is exactly the failure here: `chacha20 0.10.1` was removed from the set of versions crates.io will resolve fresh installs against, so a clean `cargo fetch --locked` followed by `cargo deny --locked check` fails even though nothing in this repository changed.

## Goals / Non-Goals

**Goals:**

- Move the locked `chacha20` resolution to the current, unyanked `0.10.2`, restoring reproducibility.
- Touch nothing else in `Cargo.lock`: this is a single transitive patch bump, not a general dependency refresh.

**Non-Goals:**

- Pin `chacha20` as a direct dependency in any `Cargo.toml`. It is not used directly by this repository's code; it arrives solely through `async-nats`'s own dependency on `rand`, and pinning it directly would create a second place to maintain a version this repository does not otherwise care about.
- Loosen `deny.toml`'s `yanked = "deny"` policy or add an exception. The policy did its job correctly: it caught a genuinely unreproducible lockfile.
- Run a general `cargo update` across the whole lockfile. That would move dependencies unrelated to this failure and widen the diff and the risk surface for no benefit.

## Decisions

`cargo update -p chacha20` (confirmed via `--dry-run` first: `Updating chacha20 v0.10.1 -> v0.10.2`, `11 unchanged dependencies behind latest`). `0.10.2` is compatible with the same `^0.10` requirement `rand 0.10.2` already expressed, so no other crate's resolution needs to move.

## Risks / Trade-offs

- [A future crates.io yank of a different transitive dependency will reproduce the same failure shape] — expected and correct: `yanked = "deny"` is designed to catch exactly this, repeatedly, for as long as it stays configured that way. Nothing about this fix changes that behaviour.

## Migration Plan

No rollout coordination: a single-package lockfile update. Merge once the documented local gate and `openspec validate --all --strict` pass.
