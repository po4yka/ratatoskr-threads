# Threads connector data model

## Planned owned schema: `threads_archive.*`

- `accounts`, encrypted credentials, scopes, capabilities, expiry/status.
- `posts`, authors, text/media revisions, canonical permalinks, raw blob references.
- `post_relations` with type, direction, source/target provider ID, resolution status.
- `captures`, acquisition, saved authority, captured time, notes/collection references.
- resolution attempts and upstream states.
- `exports`, schema/parser version, archive hash/blob, import runs, warnings, unknown records.
- publishing/write audits, outbox/inbox.

## Constraints

Owner scope is mandatory. Provider IDs are stable identities. Relations are unique and cycle-safe without deleting graph evidence. Raw blobs are immutable. Authority cannot be silently upgraded. Missing data is not deletion. Credentials/private text are excluded from logs/events. Cross-schema writes/foreign keys are forbidden.

## Item 9 lifecycle state

- `media.retention_class`, deadline, and reason distinguish metadata from authorized complete bytes.
- `deletion_operations` and `deletion_effects` retain only identifiers, closed actions, and counts;
  `local_source_removals` prevents resurrection; `blob_deletion_tasks` makes external cleanup retryable.
- `reresolution_runs/items` persist finite ceilings, reservations, deterministic candidates, skips,
  and outcomes.
- `export_reprocessing_runs/items` bind one retained receipt and exact parser to plan/state
  fingerprints and committed checkpoints. They are operational history, not schema migrations.

The current schema definition is edited in place during development. Rollback creates a fresh test
database from the prior definition and disables new workers; no database migration file or version
negotiation exists.
