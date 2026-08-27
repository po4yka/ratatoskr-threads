## Why

Plan item 9 is the remaining lifecycle gap between accepted Threads evidence and a privacy-safe, maintainable archive: media has no executable retention policy, owners cannot comprehensively erase a capture or connection, stale public observations have no bounded refresh job, and retained exports cannot be previewed and reprocessed under a newer parser. These guarantees are required now so already-implemented capture, official-account, Knowledge, and Data Export lanes do not accumulate data that cannot be governed or safely refreshed.

## What Changes

- Add an explicit media-retention policy that distinguishes metadata from archived bytes, admits byte archival only within type/size/rights/lifetime budgets, and makes expiry/deletion and BlobStore ownership observable without claiming provider completeness.
- Add owner-authorized deletion by capture and by official connection, with a closed enumeration of every affected owned table/blob class, transactional non-sensitive audit evidence, replay safety, and one `social.source.removed.v1` outbox fact per removed live source so Knowledge deletes derived analyses and embeddings under the workspace `social-analysis-intake` contract.
- Add a scheduled public re-resolution job that selects only eligible live captures and enforces per-run item, request, byte, deadline, and provider-budget guards before each supported public request; it appends evidence and never bypasses privacy or turns missing output into deletion.
- Add restartable parser-version reprocessing over retained immutable Data Export archives. Dry-run produces the same deterministic record classifications, counts, warnings, conflicts, and prospective digest changes as apply, while making no database, BlobStore, or outbox mutation; apply checkpoints progress and is replay-safe.
- Edit the first-version `schema.sql` definition in place for the new lifecycle state and audit records. This adds no database migration file, migration ledger, version negotiation, or parallel API/database major.
- Update runtime wiring, telemetry, documentation, and synthetic/redacted tests for the new media, privacy, re-resolution, and reprocessing guarantees.

## Capabilities

### New Capabilities

- `media-retention`: Media metadata/byte eligibility, bounded BlobStore retention, truthful completeness, expiry, and deletion semantics.
- `privacy-deletion`: Owner-scoped capture and connection deletion, complete owned-data/blob enumeration, audit evidence, idempotency, and downstream removal propagation.
- `re-resolution-jobs`: Eligible-capture selection and finite per-run/provider budgets for supported public re-resolution.
- `data-export-reprocessing`: Parser-version dry-run fidelity, checkpointed apply, immutable raw-archive reuse, and replay-safe reporting.

### Modified Capabilities

- `archive-schema`: The current first-version schema gains lifecycle policy state, deletion audit/checkpoint records, and constraints needed by the new capabilities, in place and without migrations.
- `public-resolution`: Re-resolution becomes an explicitly budgeted scheduled operation while preserving append-only evidence and truthful unavailable semantics.
- `data-export-import`: An existing immutable export receipt can be deterministically previewed and reprocessed by an explicit parser version without reacquisition or silent reinterpretation.
- `social-source-publishing`: Local privacy/retention deletion appends the canonical removal fact atomically so Knowledge stops analysis without interpreting it as upstream deletion.

## Impact

- Affected Rust areas: `crates/threads-archive` media, deletion, public-resolution, Data Export, publishing, BlobStore, database, and telemetry boundaries; `services/threads-archive` job/runtime wiring and an operator-facing reprocessing command or equivalent existing process boundary.
- Affected storage: the single in-place `schema.sql`, service-owned content-addressed blobs, transactional outbox, and non-sensitive deletion/reprocessing audit state. No new production dependency is planned.
- Cross-repository behavior uses existing pinned contracts: `social.source.removed.v1`, workspace `social-analysis-intake`, and workspace `blob-references`; no contract change or Knowledge-owned write is introduced here.
- Capture and official-account lanes remain distinct. Connection deletion removes only that owner's connection-derived holdings; shared provider identities and another owner's captures remain intact. No provider write, native unsave, browser session, cookie, or hidden API is involved.
