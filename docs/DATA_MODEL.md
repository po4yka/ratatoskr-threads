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
