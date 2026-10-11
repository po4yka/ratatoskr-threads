## Why

XR-021 CONTRACTS.md section R3-01: every non-Edge identity could subscribe to the shared `_INBOX.>` namespace, so a compromised service could read the JetStream deliveries of every other service. Each identity now subscribes only to its own inbox prefix, and Threads must open its broker connection under `_INBOX_threads` and prove it against a broker that enforces its own stanza. Section R3-11 puts every service in the one database `ratatoskr` under its own role, with a checked-in provisioning file, and moves Threads' raw object root off the boot device. Section R3-15 rule 3 pins the permalink grammar bounds, and R3-19 removes a stale README statement.

## What Changes

- `NatsConnection::connect` and `connect_with_nkey` set `custom_inbox_prefix(INBOX_PREFIX)` with `INBOX_PREFIX = "_INBOX_threads"`. **BREAKING**: a build without the prefix cannot complete JetStream requests under the new ACL. Nothing is deployed; reload NATS and deploy the services of one snapshot together.
- Add `deploy/nats/identity.conf`, a byte-equal copy (comments, whitespace and the nkey token ignored) of the `THREADS` stanza of Platform's `deploy/nats/ratatoskr.conf`.
- Add a test that runs a private `nats-server` whose only users are an administrator and that stanza, and completes a verify, a pull and a publish through `connect_with_nkey`.
- CI puts `nats-server` on `PATH` by copying it out of the pinned NATS container; `DEVELOPMENT.md` says so.
- **BREAKING**: the operator example's database URL names the shared database `ratatoskr` instead of `ratatoskr_threads`, and its raw object root moves from `/var/lib/ratatoskr-threads/raw` to `/mnt/nvme/ratatoskr/blobs/ratatoskr-threads`.
- Add `deploy/postgres/role.sql` and a README deployment paragraph.
- Pin the permalink grammar bounds (30-character handle, 128-character code) with a boundary test; behaviour already holds.
- Rewrite the README "Workspace integration" paragraph from the workspace as it is.

## Capabilities

### New Capabilities

- `nats-inbox-prefix`: the service's broker connection uses its own inbox prefix and works under its own stanza.

### Modified Capabilities

- `operator-example`: states the shared database and the NVMe raw root. The capability's spec is not yet in `openspec/specs/` (the earlier change is not archived), so this change carries the delta as an addition.

## Impact

- `crates/threads-archive/src/nats.rs`, `deploy/nats/identity.conf`, `deploy/postgres/role.sql`, `deploy/systemd/threads.conf.example`, `README.md`, `DEVELOPMENT.md`, `.github/workflows/ci.yml`, new tests, `nkeys` as a dev-dependency.
- No schema change and no wire contract change.
