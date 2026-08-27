# data-export-import Specification

## Purpose

Defines safe, immutable, provenance-honest ingestion of an owner-authorized Threads Data Export and the coverage evidence it can produce without mistaking an export for live provider authority.

## Requirements

### Requirement: An export receipt is immutable and idempotent
The service SHALL retain the exact received archive in immutable content-addressed storage before parsing and persist an owner-scoped import run with digest, length, and raw evidence. A repeat receipt for the same owner and digest SHALL converge on the existing run; the same bytes for another owner SHALL produce a distinct run.

#### Scenario: Receipt streams one immutable owner-scoped archive
- **WHEN** an authenticated owner submits a synthetic export as a stream
- **THEN** the returned run references immutable bytes whose SHA-256 and length equal the received stream, and a retry returns the same run

### Requirement: Archive inspection rejects hostile input before projection
The service SHALL reject traversal, absolute/backslash paths, excessive entry count, path depth, compressed/decompressed bytes, and compression ratio before normalizing any record. A refusal SHALL retain the receipt, mark its run failed, and create no projection.

#### Scenario: Traversal never reaches projections
- **WHEN** an owner imports an archive containing a traversal entry
- **THEN** the run is failed with a path-safety warning and no normalized post or relation is stored

### Requirement: A supported export uses one deterministic versioned parser
The service SHALL parse only the supported `threads-export-v1` layout, sort its posts and directed relations deterministically, and project each post with `data_export` acquisition and `export_observation` authority. Unknown ZIP sections SHALL remain linked to raw archive evidence and finish with a warning.

#### Scenario: Equivalent fixture order normalizes identically
- **WHEN** equivalent supported fixture archives have different safe entry ordering
- **THEN** their ordered post and relation projections and parser version are equal

### Requirement: Reconciliation and coverage are replay-safe and non-destructive
The service SHALL reconcile by stable provider identity, publish no duplicate fact for a completed replay, and persist owner-scoped overlap counts for export identities, matches, export-only, capture-only comparable identities, and non-comparable captures. Export absence SHALL not delete or alter a capture.

#### Scenario: Coverage separates overlap and unresolved captures
- **WHEN** an owner has matching, capture-only, and unresolved captures while an export has an extra post
- **THEN** the persisted report counts each category and no capture is tombstoned or changed

### Requirement: An immutable receipt supports explicit parser reprocessing

An owner-authorized retained export receipt SHALL remain addressable as the source for a `data-export-reprocessing` dry-run or apply operation. The original receipt digest, BlobStore reference, detected export version, and initial import evidence SHALL remain immutable while each reprocessing operation records its own target parser version, result, and completeness evidence.

#### Scenario: reprocessing preserves the original import receipt
- **WHEN** the same retained archive is applied under another registered parser version
- **THEN** its original receipt and initial import report remain byte-for-byte unchanged and the new parser evidence is separately addressable

### Requirement: Reprocessing preserves import safety and authority

Every reprocessing mode SHALL reuse the existing hostile-archive limits, path validation, unknown-section retention, owner scope, `export_observation` authority ceiling, and absence-without-deletion semantics. A parser version SHALL not weaken those controls or upgrade export evidence to native Saved-list authority.

#### Scenario: a new parser cannot reinterpret authority
- **WHEN** reprocessing derives additional normalized records from a retained export
- **THEN** every derived record remains an export observation and no result claims authoritative native Saved membership
