# Developing Ratatoskr Threads

> Status: Proposed  
> Last reviewed: 2026-08-17

Architecture bootstrap: OAuth, provider client, capture resolver, Data Export importer, schema, and publishing are not implemented.

## Intended toolchain

Rust/Tokio, Reqwest/Rustls, OAuth, SQLx/PostgreSQL, safe archive import, BlobStore, NATS, provider fixtures/WireMock, tracing, and testcontainers.

## Workflow

1. Confirm capability, account type, scopes, and whether the operation is read or separately consented write.
2. Preserve acquisition method, saved authority, canonical URL, and relation graph.
3. Use supported public resolution only; preserve unavailable/private state.
4. Store raw evidence/export before versioned normalization and preserve unknown records.
5. Test relation cycles, pagination, replay, privacy, archive limits, and no-cookie/no-hidden-API invariants.

The first scaffold PR must document exact commands. Default CI uses synthetic fixtures and no personal provider credentials.
