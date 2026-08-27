## Context

See proposal.md. OAuth discovery already identifies the account and scopes, the archive schema already models official provenance and post relations, and public resolution has the raw-first storage pattern. Own-account synchronization is not yet executable; the capability matrix still reports it as planned and the social-source projection currently assumes every source began as a capture.

The repository remains in its first-version development phase: change the current schema definition in place, add no migration files or compatibility path, and retain no browser/session access.

## Goals / Non-Goals

**Goals:**

- Make a scheduler-invoked account-sync entry point safely fetch the connected account's own official posts and replies incrementally.
- Make a completed scan's observation, authoritative projection, social-source outbox fact, and watermark one durable transaction.
- Reuse existing raw-object and relation normalization rules while retaining capture rows during a provenance upgrade.

**Non-Goals:**

- A claim of exhaustive account history, native Saved synchronization, publishing, provider writes, browser automation, media-byte archival, new event grammars, or a schema migration.
- Retrying, advancing, or deleting content merely because a page is absent, partial, or fails.

## Decisions

### A narrow official-own-content adapter drives the scheduled worker

Add a small async adapter seam for a bounded page of authenticated own-content observations. It accepts the connected account identity and an optional opaque watermark, returns raw response bytes plus normalized post/reply candidates and a next watermark only on a completed page, and keeps access tokens and provider SDK/HTTP shapes inside the adapter. A Tokio interval invokes the account-sync entry point; the entry point remains callable directly by a job runner for deterministic tests.

This separates account lifecycle from listing semantics and supports hand-written synthetic provider fixtures. It is preferred to extending public-resolution because authenticated own content has different credentials, completeness, and authority. It is also preferred to a browser session because that violates the service boundary.

### Capability evaluation precedes provider I/O

The worker reads the connected account's current scopes and connection state, derives `OwnAccountSync` availability through the local matrix, and returns a typed no-op with the existing non-secret reason when unavailable. No adapter call, raw object, post mutation, outbox row, or checkpoint mutation is permitted on this path.

This makes scope downgrade and unimplemented/unsupported matrix state truthful. It deliberately does not let discovery itself launch a scan.

### One transaction commits projection swaps, relations, source facts, and watermark

Add an account-bound sync-checkpoint row to the current schema definition. After raw bytes are immutably stored, one database transaction upserts each provider post by stable provider ID, records the parser revision and reply relation, creates or updates the owner's social source and its outbox fact, then advances the checkpoint. The post upsert writes `official_api` and `authoritative_platform_state` together and preserves the existing post UUID, so captures and relations still point to the same record. The source record is generalized to allow an official-account origin instead of fabricating a capture.

This is preferred to separate commits because a visible watermark without every corresponding authoritative observation could lose data on retry. Raw blobs are content-addressed and may remain as harmless immutable orphans if the following transaction rolls back; they are never treated as a completed scan.

### Scan results are observations, not deletion proofs

The adapter's returned items are upserted; items absent from a page are untouched. A provider error, malformed fixture/response, relation validation failure, or persistence error rolls back the transaction and leaves the previous watermark current. The result reports observed count and next-checkpoint state, never an account-wide completeness flag.

This avoids turning pagination gaps or limited provider retention into tombstones.

### Existing published grammar is reused with actual post provenance

The existing social-source event types and vocabulary already carry `official_api` and `authoritative_platform_state`, so no workspace contract change is needed. Publication is refactored to load source provenance from the normalized post and owner from either capture or account origin; a source whose existing capture-backed post becomes official emits an update rather than losing the capture relationship.

This is preferred to a parallel own-content event, which would create unnecessary downstream routing and duplicate the canonical source identity.

## Risks / Trade-offs

- [Provider pagination or watermark semantics differ from the synthetic fixture] → keep the provider seam and opaque cursor contract narrow; production HTTP binding must validate provider documentation and response shape before enabling it.
- [A raw blob succeeds but the transaction fails] → retain only content-addressed immutable bytes; no checkpoint, projection, or outbox evidence claims the scan completed.
- [Authority upgrade changes an already published capture] → preserve the post UUID and capture linkage, publish one state update with actual official provenance, and test the atomic swap.
- [A long account scan holds a transaction too long] → fetch and validate the bounded page outside the transaction; transact only the durable page application and checkpoint advance.

## Migration Plan

1. Edit `schema.sql` in place to add the checkpoint and generalize source origin for new development databases; do not add a migration.
2. Deploy with the scheduler disabled until the official adapter and account scopes are configured; unavailable accounts continue returning no-op results.
3. On rollback, disable scheduler invocation. Existing official observations, source facts, and checkpoints remain valid first-version state and no conversion is required.
