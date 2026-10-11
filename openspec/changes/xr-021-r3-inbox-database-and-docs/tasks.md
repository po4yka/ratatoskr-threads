## 1. Inbox prefix (R3-01)

- [x] 1.1 RED: `crates/threads-archive/tests/nats_inbox_prefix.rs::the_connection_completes_jetstream_requests_under_its_inbox_prefix` runs a private `nats-server` whose only users are an administrator and the Threads stanza from `deploy/nats/identity.conf`, verifies the durable, pulls from it and publishes `evt.platform.operation.reported.v1` through `NatsConnection::connect_with_nkey`; it fails today because the reply never arrives (the call times out).
- [x] 1.2 GREEN: add `INBOX_PREFIX` and apply `custom_inbox_prefix` in both constructors, add `deploy/nats/identity.conf`, the CI step that puts `nats-server` on `PATH`, and the `DEVELOPMENT.md` sentence.

## 2. Shared database and NVMe raw root (R3-11)

- [x] 2.1 RED: extend `services/threads-archive/tests/boot.rs::the_shipped_example_loads` to expect the database URL `postgres://ratatoskr_threads:CHANGE-ME@127.0.0.1:5432/ratatoskr`, the raw root `/mnt/nvme/ratatoskr/blobs/ratatoskr-threads` and `deploy/postgres/role.sql` creating `ratatoskr_threads` with `connect, create` on `ratatoskr`; it fails today on the database name.
- [x] 2.2 GREEN: edit the example, add `deploy/postgres/role.sql`, add the README deployment paragraph.

## 3. Grammar boundary (R3-15)

- [ ] 3.1 This pins behaviour that already holds, so it cannot start from a failing test: add `crates/threads-archive/tests/permalink.rs::the_boundary_lengths_of_the_shared_grammar_table`.

## 4. Documentation (R3-19)

- [ ] 4.1 Documentation, no failing test can precede it: rewrite the README "Workspace integration" paragraph from the workspace as it is.
