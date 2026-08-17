# Threads connector testing strategy

Required tests:

- OAuth binding, credentials, refresh/revoke, scopes, capability drift, and write consent.
- Permalink classification/canonicalization and malicious URLs.
- Post/reply/quote/repost/thread-root normalization, missing nodes, duplicate edges, and cycles.
- Explicit capture idempotency/provenance and public/private/deleted/unsupported resolution.
- Safe Data Export import: schema versions, zip/path/decompression limits, unknown records, duplicates, partial assets.
- Optional publishing/reply idempotency and error/audit matrix.
- Missing-data versus deletion semantics, privacy deletion, migrations, outbox/inbox replay, no-secret/content logging.
- Workspace capture -> Threads -> Knowledge flow.

Fixtures are synthetic or authorized; no personal account is required in CI.
