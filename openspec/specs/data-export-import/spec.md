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
