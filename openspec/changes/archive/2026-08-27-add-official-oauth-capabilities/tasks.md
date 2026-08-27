## 1. Encrypted official credential boundary

- [x] 1.1 Add and run the owner-bound credential envelope RED test.
- [x] 1.2 Implement AES-256-GCM credential sealing and opening.
- [x] 1.3 Test malformed, tampered, and wrong-owner envelopes.
- [x] 1.4 Refuse invalid envelopes without secret rendering.

## 2. Official OAuth persistence and lifecycle

- [x] 2.1 Add and run storage and refresh integration tests.
- [x] 2.2 Persist encrypted credentials, identity, expiry, and audit state.
- [x] 2.3 Test definitive revoke scrubbing completeness.
- [x] 2.4 Preserve credentials when revoke acknowledgement is uncertain.

## 3. Discovery and budget reconciliation

- [x] 3.1 Test missing scope, planned sync, native Saved, and publishing consent results.
- [x] 3.2 Implement matrix-intersected capability reconciliation.
- [x] 3.3 Test budget persistence and bounded request identifiers.
- [x] 3.4 Persist bounded, non-secret budget observations.

## 4. Contract and quality completion

- [x] 4.1 Update documentation and schema contract tests.
- [x] 4.2 Run the documented product gate through `build-gate --`.
- [x] 4.3 Validate, inspect, and archive the completed OpenSpec change.
