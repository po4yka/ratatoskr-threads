# Threads connector domain model

## Terms

- **Account connection:** identity, credentials, scopes, capabilities, status.
- **Post:** provider content node.
- **Relation:** reply-to, thread-root, quote, repost, or other documented edge.
- **Capture:** explicit local save request with canonical permalink and context; the submitted URL text is preserved beside the canonical form.
- **Acquisition method:** how a source arrived — official API, an explicit capture lane (mobile share target, browser extension, Telegram), public resolution, Data Export, or legacy import.
- **Saved authority:** explicit user capture, export observation, authoritative platform state, or legacy observation.
- **Unavailable state:** private, deleted, unsupported, blocked, expired, or unresolved.
- **Export snapshot:** immutable archive and parser/completeness report.

## Invariants

1. Local capture is not native Saved state.
2. Official account and third-party capture lanes are distinct.
3. Relation graph preserves provider IDs/direction and handles missing nodes/cycles.
4. Privacy is never bypassed.
5. Raw exports precede parsing; unknown variants survive.
6. Missing export records do not prove deletion.
7. Read connection does not imply publishing consent.

## Lifecycle terms

- **Media fetch lease:** immutable finite authorization created only after every rights/type/lifetime
  and storage guard is known and available.
- **Local source removal:** owner-library state, distinct from an upstream availability tombstone.
- **Deletion plan:** total content-free classification of every owned row/blob class for one target.
- **Re-resolution run:** due public refresh work admitted within item/request/byte/deadline,
  concurrency, and endpoint budgets.
- **Parser reprocessing:** deterministic reinterpretation of one verified retained export using an
  exact registered parser. It is not a database migration.

Producer/consumer meaning is unchanged for captured/updated facts. The new removal producer fact
means only that Ratatoskr no longer holds the source; Knowledge owns deletion of summaries,
embeddings, and search projections.
