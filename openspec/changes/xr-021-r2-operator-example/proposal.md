## Why

Threads ships no operator example, so a deployment has no statement of the bus and public-resolution keys the explicit-capture flow needs (XR-021 CONTRACTS.md section R2-10). Since the first round the process refuses to start with a bus configured and no `RATATOSKR__PUBLIC_RESOLUTION__RAW_OBJECT_ROOT`, so an operator who guesses the key set gets exit 78 with no document to consult.

Round 1 already made the outbox pump and the readiness surface report a failing publisher (R2-03 needs nothing here); the existing `bus_worker_exit_flips_readiness_and_exit_code` and `removal_row_is_published_and_later_rows_are_not_blocked` tests are the evidence.

## What Changes

- Add `deploy/systemd/threads.conf.example`: the operator listener `127.0.0.1:9084`, the database URL with a `CHANGE-ME` password, the bus URL and the absolute seed path `/etc/ratatoskr/threads.nkey`, and the public-resolution keys with the approved oEmbed endpoint.
- Add a boot test that loads the shipped example through the service's own configuration loader, so the example cannot rot.
- Point to the example from `DEVELOPMENT.md`.

## Capabilities

### New Capabilities

- `operator-example`: the shipped, loadable operator configuration example.

### Modified Capabilities

None.

## Impact

- `deploy/systemd/threads.conf.example`, `services/threads-archive/tests/boot.rs`, `DEVELOPMENT.md`.
- No schema change, no contract change, no production code change.
