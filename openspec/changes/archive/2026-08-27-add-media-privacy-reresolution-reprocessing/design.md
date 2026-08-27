## Context

See `proposal.md` for motivation. Items 1-8 already share normalized posts across official-account, explicit-capture, public-resolution, and Data Export lanes; raw resolver/export bytes live in the service-owned content-addressed `RawObjectStore`; source facts and Knowledge completions are transactionally linked by source id plus digest. The current schema already separates media metadata from optional blob references and preserves append-only revisions, but it has no executable media policy, local-deletion model, re-resolution run state, or independent export reprocessing run.

The binding development rule permits only an in-place edit to the one `schema.sql`; it forbids database migration files/tooling and parallel versions. “Parser-version migration” in this change therefore means operational reprocessing of retained immutable export evidence, not schema/data migration. The existing pinned social contracts already provide `social.source.removed.v1`, and workspace `social-analysis-intake` defines Knowledge cleanup, so no cross-repository contract change is required.

## Goals / Non-Goals

**Goals:**

- Make all retained media bytes policy-admitted, bounded, reference-safe, and actually deletable.
- Make capture and connection deletion complete by construction against the owned schema inventory, while preserving independent lanes and other owners.
- Give public refresh a finite, observable run model whose guards execute before I/O.
- Produce one pure parser plan that powers both dry-run reports and restartable apply.
- Preserve the existing source identity, authority, raw-first evidence, transactional outbox, and no-private-session boundaries.

**Non-Goals:**

- Provider-side delete/unsave, publishing, account automation, login/cookie access, or automatic crawling.
- Automatic media download for every capture, user-upload/provider-media equivalence, or permanent retention promises.
- Database migrations, a schema ledger, a second API/database major, parser negotiation, or compatibility shims.
- Changing social/event contracts, writing Knowledge storage directly, or claiming Knowledge deletion before the removal fact is published and consumed.
- Proving compatibility with a real personal Threads export; the implementation remains validated by synthetic/redacted fixtures until an authorized real export is supplied separately.

## Decisions

### D1. One policy decision precedes every media-byte fetch

Add a pure `MediaRetentionPolicy::decide` boundary whose input contains acquisition/provenance, requested action, URL metadata, rights class, MIME/kind, declared size/lifetime, and current owner/object budgets. Its closed result is `MetadataOnly(reason)` or `Archive(lease)`, where the lease fixes the maximum bytes and retention deadline for the subsequent fetch. Unknown rights, size, MIME, expiry, or budget always yields metadata-only.

The fetch reuses the existing Reqwest/Rustls safety posture: HTTPS allowlist, no credential-bearing logs/cache keys, no unrestricted redirects, bounded deadline, bounded streaming, and verification of final URL/MIME/length/digest. Only after the content-addressed write and verification succeed does one transaction attach the blob evidence and mark `bytes_archived`. User uploads keep their existing raw-object kind and never pass through this provider-media transition.

Alternative considered: archive every resolver media URL and delete later. Rejected because it spends rights/storage budget before authorization and makes “metadata only” untruthful during failures.

### D2. Blob deletion is a durable post-commit workflow, not an external side effect inside SQL

Extend the current store with verified `delete_if_matches(blob_ref, digest)` and inventory helpers; do not add a new storage service or dependency. SQL changes first remove live references and insert a `blob_deletion_tasks` row keyed by digest/reference. A worker then checks the database-wide live-reference query, deletes only an unreferenced service-owned path, verifies absence, and marks the task complete. Failure leaves the task pending with bounded safe diagnostics. Content-addressed deduplication therefore cannot make one owner delete another live reference.

Alternative considered: delete the file inside the database transaction. Rejected because filesystem success cannot participate in PostgreSQL rollback and would create irrecoverable partial outcomes.

### D3. Deletion uses a closed schema inventory and a two-phase plan/apply engine

Define one compile-time `OWNED_DATA_CLASSES` inventory matching the exact current `schema.sql` table names plus BlobStore classes. Define both capture and connection classification maps over that enum. A test extracts the authoritative schema inventory and requires total one-to-one classification; adding a table without classifying it makes the gate fail. The pure planner reads owner-bound state and returns a deterministic `DeletionPlan` of row actions, source removals, retained shared references, blob tasks, and bounded counts. Preview renders that plan only. Apply locks the target, recomputes and validates the same plan, and performs all SQL mutations, outbox appends, audit insertion, and blob-task insertion in one transaction.

New first-version tables are `deletion_operations`, `deletion_effects` (bounded class/count entries), `local_source_removals` (non-content resurrection guard), and `blob_deletion_tasks`. Target ids in retained audit rows have no foreign key so the target row can be physically deleted. Audit payloads are typed columns/closed enums, not arbitrary JSON that could accidentally retain bodies or notes.

Capture deletion removes capture-specific rows and only garbage-collects shared post/revision/relation/media/raw/source rows after reference queries prove no live capture, account, export projection, source, or other raw record needs them. Connection deletion first removes ciphertext credentials and account-local budgets/checkpoints; account posts are detached or removed by the same reference rules. A same-owner explicit capture or export observation keeps its lane, authority, and source live. Another owner is never in the deletion candidate set.

Alternative considered: rely on `ON DELETE CASCADE`. Rejected because cascade hides the completeness set, cannot express retain/detach decisions for shared posts/blobs, and cannot atomically append one removal fact per final library holding.

### D4. Local removal and upstream availability stay separate

Do not reuse the existing `tombstones` table, which records provider availability. `local_source_removals` records only owner/library removal identity, reason, operation, and time. For every source whose final owner holding disappears, apply calls a new publishing helper to append `SocialSourceRemoved` in the same transaction, then removes content-bearing source revisions and analysis-link rows. A remaining holding suppresses the removal event. Knowledge completion intake checks the local-removal guard and records a late completion as skipped, never linked.

This implements workspace `social-analysis-intake`: Threads produces a stop-analysing fact, Knowledge owns derived deletion, and neither side treats it as upstream provider deletion.

Alternative considered: call Knowledge synchronously. Rejected because it crosses ownership, couples availability, and loses the existing at-least-once outbox/inbox guarantees.

### D5. Re-resolution is a claim/check/reserve/execute/finalize state machine

Add `reresolution_runs` and `reresolution_items`. Selection orders due live captures by `(next_resolution_at, capture_id)` and persists candidate/skipped classification. Before each HTTP call the worker starts a short transaction that rechecks live ownership/status/policy, reserves one item and request from the run, and atomically consumes available endpoint budget. If any count, byte, deadline, concurrency, or provider guard fails, no call begins. Response streaming has its own maximum; accepted bytes are charged before evidence is committed, and no partial raw body survives refusal.

Only resolved, transiently unavailable, or resolver-failed captures become automatic candidates. Private/inaccessible, deleted-upstream, unsupported, and locally removed captures require a new explicit user acquisition action rather than automatic retry. Store a `next_resolution_at` policy deadline with jitter derived deterministically from capture id, avoiding synchronized refresh bursts.

The worker invokes the existing public resolver and publisher. Equal content adds observation evidence without another update event; changed content appends the current full snapshot. Any missing/error outcome keeps the prior projection.

Alternative considered: extend the existing account-sync scheduler with untracked calls. Rejected because public captures are not account authority and unpersisted scheduling cannot prove budgets, replay, or deletion races.

### D6. Dry-run and apply share a pure `ReprocessPlan`

Split Data Export handling into: verified receipt read/inspection; parser registry lookup by `(detected_export_version, parser_version)`; pure ordered parse/classification; owner-scoped current-state snapshot; pure reconciliation plan; and side-effecting apply. `ReprocessPlan` contains deterministic item keys, classifications, prospective post/relation/source digests, warnings/conflicts, and completeness counts. Its canonical plan fingerprint excludes operation ids and timestamps.

Dry-run executes through plan creation and serializes a `ReprocessReport`; it opens no write transaction and calls no store/network mutation. Apply creates `export_reprocessing_runs` plus checkpointed `export_reprocessing_items`, then applies deterministic bounded chunks. Each chunk commits item outcomes, projection effects, and outbox rows together. Resume verifies receipt digest, parser identity, and plan fingerprint/state preconditions before continuing. A completed replay returns its stored report.

The original `export_runs` remains the immutable receipt/initial-import identity. Reprocessing tables reference it rather than weakening its owner-plus-archive-digest uniqueness. Old parser revisions and reports remain addressable; absence never schedules deletion.

Alternative considered: mutate `export_runs.parser_version` and rerun the importer. Rejected because it silently rewrites evidence, destroys initial-import history, and cannot make dry-run/apply fidelity auditable.

### D7. The operator process exposes explicit reprocessing modes without a new parser dependency

Extend the existing small command grammar with:

```text
ratatoskr-threads-archive reprocess-export dry-run --owner <UUID> --run-id <UUID> --parser <TOKEN>
ratatoskr-threads-archive reprocess-export apply   --owner <UUID> --run-id <UUID> --parser <TOKEN> --operation-id <UUID>
```

Parsing remains in project code; no new `clap` dependency is added. Arguments are closed, duplicates/unknowns are usage errors, and the database/blob locations continue to come only from validated `RATATOSKR__` configuration. Each successful invocation writes exactly one newline-terminated canonical JSON report to stdout and diagnostics only to stderr. Exit `0` means report produced/completed or resumably already complete, `2` means invalid invocation, `78` remains invalid configuration, and `1` means an operational/integrity failure. Dry-run never prompts; apply requires the explicit subcommand and operation id. Broken stdout pipe after producing no other destination is a clean exit.

The command is an operator tool over an already authorized owner/run identity; it does not create a new public endpoint or alter runtime admin routes.

Alternative considered: an unauthenticated HTTP admin mutation. Rejected because the current admin plane is read-only/loopback and adding mutation authority there expands the security surface unnecessarily.

### D8. Schema edits are first-version replacement only

Edit `schema.sql` in place to add the lifecycle tables/columns and constraints named above, including media policy/deadline state and re-resolution due state. `Database::apply_schema` continues to initialize fresh disposable databases from the embedded file. Do not add `migrations/`, SQLx `migrate`, a schema ledger, old/new dual reads, version negotiation, or backfill code.

The `rust-database` migration-deployment section is explicitly inapplicable under the repository's development status. Transaction ownership, finite pool acquisition, stable lock order, outbox atomicity, and real PostgreSQL tests remain applicable.

## Risks / Trade-offs

- [Deletion classification drifts when a table is added] → make the exact schema inventory versus both target maps a mandatory test and refuse unknown runtime classes before mutation.
- [Shared post/raw/blob cleanup removes another live holding] → owner-scoped planning plus explicit cross-lane/reference queries; physical deletion only after a second global live-reference check.
- [Database deletion commits but BlobStore deletion fails] → durable pending tasks, idempotent digest-verified delete, completion state distinct from database commit, and retry telemetry.
- [Removal outbox is delayed] → retain the outbox fact and local source-removal guard; report local deletion separately from downstream consumption, never claim Knowledge deletion from producer-side evidence alone.
- [Provider budget changes between selection and request] → reserve persisted endpoint capacity in the immediate pre-I/O transaction; selection alone grants no authority.
- [Large re-resolution/import runs monopolize connections] → short claim/finalize transactions, bounded chunks, no database connection held during HTTP, file I/O, parsing, or retry delay.
- [Dry-run differs because state changes before apply] → report the state/plan fingerprint; fidelity is promised for unchanged state, and apply refuses a stale expected fingerprint when one is supplied.
- [Parser bug creates deterministic but wrong output] → retain immutable archive and all prior parser reports/revisions, require explicit parser selection, and permit rollback by disabling that parser and reapplying the prior registered parser.
- [CLI JSON becomes an accidental unstable script surface] → define a single first-version report schema, deterministic ordering, stdout/stderr separation, and process-boundary golden tests.
- [Synthetic exports hide real provider variation] → state this verification gap in delivery; never claim real-export compatibility without authorized evidence.

## Migration Plan

1. Add tests and in-place first-version schema definitions for lifecycle state; fresh test databases are recreated from that definition. There is no migration of an existing database.
2. Land pure media/deletion/re-resolution/reprocessing planners and their RED/GREEN tests before wiring side effects.
3. Wire transactional stores, outbox removal facts, BlobStore deletion worker, bounded re-resolution worker, and explicit CLI modes.
4. Enable item-9 scheduling only after configuration carries finite non-zero budgets; keep it disabled when absent. Existing capture, account-sync, and import paths remain available.
5. Validate producer compatibility against the pinned removal contract and workspace specs. Rollout does not require a coordinated Knowledge change because the accepted consumer contract already defines removal handling; nevertheless, producer completion proves only outbox creation, not consumer deletion.
6. Rollback by disabling item-9 workers/CLI apply, reverting the code/schema definition for newly created development databases, and retaining immutable raw evidence/outbox/audit artifacts permitted by policy. No down migration or compatibility path is created.
7. Parser rollback selects the prior registered parser for a new explicit reprocessing operation; it never rewrites or deletes the failed parser's evidence. Re-indexing happens only through emitted updated/removed facts, under Knowledge ownership.

Privacy impact: capture/connection content, credentials, notes, media, raw evidence, and completion links become explicitly enumerable and deletable; retained audit/outbox records are content-free. User-visible authority is unchanged: explicit capture remains local intent, official own-content remains supported API evidence, Data Export remains `export_observation`, and no native Saved state is invented.
