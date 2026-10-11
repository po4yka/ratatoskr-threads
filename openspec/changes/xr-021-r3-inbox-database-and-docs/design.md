## Context

See [proposal.md](proposal.md) and XR-021 CONTRACTS.md sections R3-01, R3-11, R3-15 and R3-19. Platform's `deploy/nats/ratatoskr.conf` is the only deployed ACL; this repository carries a copy of its own stanza so its broker test has the file to run.

## Decisions

**One constant, applied in both constructors.** `INBOX_PREFIX` sits beside the durable names in `nats.rs`, so the workspace text scan finds the literal inside a `custom_inbox_prefix(` call. `connect` (the development path, no credential) uses the same prefix as `connect_with_nkey` so a development broker exercises the same reply subjects as production.

**The test starts a real enforcing broker.** `tests/nats_inbox_prefix.rs` spawns `nats-server` with a configuration whose only users are the test administrator (standing in for Edge, who provisions the streams and the durable) and the shipped `deploy/nats/identity.conf` stanza with its placeholder nkey replaced by a generated public key. It then connects through `NatsConnection::connect_with_nkey`, verifies the durable, pulls from it and publishes an operation report with a PubAck. Under the stanza `subscribe.allow = ["_INBOX_threads.>"]` a connection that uses the default `_INBOX.` prefix never receives a reply and the call times out, which is the RED state.

**Binary on PATH.** `nats-server` is copied out of the pinned NATS container in CI, the model being the X repository's workflow step. A missing binary fails the test.

**Shared database.** The example connects as `ratatoskr_threads` to `ratatoskr`. `role.sql` creates the role without a password, with no elevated attributes, grants `connect, create` on `ratatoskr` (the service creates its own `threads_archive` schema from `schema.sql`) and revokes the `public` schema.

## Risks / Trade-offs

- A service built before this change cannot complete JetStream requests under the new ACL. Nothing is deployed, so the break is named, not staged.
- The stanza copy can drift from Platform's; the workspace `bus_acl.rs` check is the guard.
