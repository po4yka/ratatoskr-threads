## Context

See proposal.md. The schema already reserves `accounts`, `credentials`, and account capability storage, while the X OAuth sibling supplies the fleet's AES-256-GCM envelope pattern. This service is still in the single-schema development phase: its definition is edited in place, with no migration files or compatibility paths.

## Goals / Non-Goals

**Goals:**

- Store official Threads grants in account-bound, versioned authenticated envelopes.
- Persist the official account identity, scope/capability evidence, budget observations, and lifecycle audit without putting tokens in logs, events, fixtures, or public types.
- Make refresh and revoke truthful, atomic state transitions.
- Reconcile discovery against the fixed matrix without enabling own-post sync, native Saved-list sync, or provider writes.

**Non-Goals:**

- HTTP callback routes, browser-session automation, own post/reply synchronization, publishing, scope expansion, or a provider write flow.
- A schema migration, version-negotiation path, or externally visible contract change.

## Decisions

### Account-bound AES-256-GCM envelopes

Use the fleet envelope: format marker, fresh 96-bit nonce, AES-256-GCM ciphertext/tag, and authenticated associated data composed from a stable Threads credential label plus account UUID. A 32-byte configured master key and key generation are required at construction. The same grant cannot decrypt under another account. This is chosen over database-only encryption because the domain boundary controls owner binding and key version; it is chosen over copying X code verbatim because Threads has a narrower lifecycle surface and no OAuth callback implementation in this item.

### Adapter seam rather than a real provider client

Expose a small official-provider trait for exchange, refresh, revoke, and capability discovery. Production HTTP wiring is deferred until the callback facade and configured Meta application values are in scope; hand-written synthetic fixtures prove parsing and lifecycle semantics now. This keeps SDK types and provider tokens inside the adapter.

### Capability reconciliation is intersection, not scope projection

The reconciler evaluates account type and granted scopes against an explicit capability requirement table, then intersects the result with `AcquisitionMode::capability()`. Native Saved remains the matrix's `NotSupported`; own-account synchronization remains `Planned` until item 7. Write-capable scopes produce a recorded unavailable reason rather than publishing consent.

### Atomic durable lifecycle writes

One account transaction writes credential replacement, scope/capability state, account connection state, and non-secret audit evidence. Revoke attempts the provider call through the adapter; after a definitive provider reply it atomically scrubs all local token fields, expiry, and scopes and records the revoked state. A transport-uncertain revoke leaves material intact and returns an uncertainty error for later reconciliation.

### Budget state is observation, not authorization

Persist a bounded endpoint class, remaining count, optional reset timestamp, and optional validated request ID. It supports scheduling/telemetry but cannot enable any capability or cause data deletion.

## Risks / Trade-offs

- [Configured encryption key is malformed or rotated] → fail closed at configuration/construction; persisted key generation rejects a mismatch.
- [Provider response shape evolves] → strict synthetic fixtures and typed adapter result validation; malformed responses do not mutate a connected account.
- [Revocation response is lost] → retain encrypted material and mark the operation uncertain rather than falsely claiming it was scrubbed remotely.
- [Scopes are mistaken for product permission] → matrix reconciliation reports explicit unavailable reasons and keeps write/sync operations out of this item.

## Migration Plan

1. Apply the current in-place schema definition only to fresh development databases.
2. Deploy with the encryption master key and generation configured before enabling the official adapter.
3. On rollback, disable adapter invocation while retaining the same schema; no credential conversion or migration path exists in the development phase.
