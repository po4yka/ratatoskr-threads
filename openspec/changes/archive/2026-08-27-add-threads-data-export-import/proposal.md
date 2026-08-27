## Why

Ratatoskr can preserve intentional captures and official own-account observations, but it cannot
yet ingest a user-supplied Threads Data Export safely or state what that export covers relative to
the local capture archive. The import must retain immutable evidence and make absence explicitly
non-destructive before Data Export observations can be trusted.

## What Changes

- Add authenticated receipt of a supplied export stream that calculates SHA-256 while writing one
  immutable, content-addressed archive blob and creates a durable, idempotent import-run record.
- Add a bounded ZIP inspector and isolated extractor that rejects traversal, absolute paths,
  excessive entry counts, excessive nesting, oversized compressed or decompressed input, and
  suspicious compression ratios before parsing.
- Add a detected-export-version to parser-version mapping and a deterministic parser for the
  supported synthetic Threads export fixture; normalize posts and first-class relations with
  `data_export` acquisition and `export_observation` authority, while retaining unknown sections
  and records as raw evidence rather than dropping or guessing them.
- Reconcile accepted observations idempotently without treating export absence as deletion, and
  persist a completeness report that compares the export's identifiable posts with existing local
  captures as matched, export-only, and capture-only observations plus warnings and unsupported
  categories.
- Mark the Data Export capability supported and document its evidence boundary. Native Saved-list
  membership remains unknown unless the detected export explicitly contains and validates that
  category.

## Capabilities

### New Capabilities

- `data-export-import`: authenticated raw-first receipt, hostile-archive rejection, versioned
  deterministic export parsing, provenance-preserving reconciliation, and truthful completeness
  reporting.

### Modified Capabilities

- `archive-schema`: make raw archive receipt, durable import state, parser evidence, and
  completeness output enforceable in the current first-version schema.
- `capability-model`: report `DataExport` as supported once its receipt, parser, and durable
  reconciliation paths exist.
- `social-source-publishing`: publish a preserved normalized Data Export observation through the
  established source-fact contract without changing its wire grammar.

## Impact

- `crates/threads-archive`: export receipt/store, ZIP inspection/extraction, parser and
  reconciliation modules; synthetic hostile and fixture-driven integration tests; existing raw
  object, relation, capability, and publishing code.
- `schema.sql`: an in-place first-version schema update only; no migrations or migration tooling.
- `Cargo.toml`/`Cargo.lock`: a pinned maintained ZIP parser dependency if the existing dependency
  set has no safe archive reader.
- `README.md` and `DEVELOPMENT.md`: plan item 8 semantics, limits, authority, and explicit
  non-capabilities.
