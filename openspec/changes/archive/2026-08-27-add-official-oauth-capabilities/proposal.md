## Why

Threads account linkage is currently only a documented intent. Ratatoskr needs a truthful official OAuth lane that keeps provider credentials service-local, discovers only currently usable capabilities, and preserves revocation evidence without retaining secrets.

## What Changes

- Add a service-local official Threads/Meta OAuth credential lifecycle: encrypted persistence, refresh replacement, and local revocation scrubbing.
- Record connected account identity, granted scopes, discovered capabilities, and rate-limit budget observations.
- Reconcile discovered capabilities against the repository capability matrix; unsupported native Saved synchronization and not-yet-implemented own-content synchronization remain unavailable.
- Add redacted synthetic provider fixtures and executable tests for encryption, reconciliation, and revoke completeness.

## Capabilities

### New Capabilities

- `official-oauth`: Connects a Threads account through the supported OAuth surface and manages its credentials, scope state, discovery evidence, and revocation safely.

### Modified Capabilities

- `capability-model`: Adds observable reconciliation of official account discovery against the fixed acquisition/authority matrix.

## Impact

- `crates/threads-archive`: OAuth domain service, encrypted credential envelope, SQLx persistence, and capability reconciliation.
- `schema.sql`: in-place first-version account, credential, capability, and budget fields only; no migration tooling.
- Workspace manifest and lockfile: pinned AES-GCM and random-source dependencies, after supply-chain review.
- `docs/CAPABILITY_MATRIX.md`, `README.md`, and integration fixtures/tests.
