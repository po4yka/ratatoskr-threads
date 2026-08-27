## Purpose

Defines safe, immutable, provenance-honest ingestion of a user-authorized Threads Data Export and
the completeness evidence it can produce without mistaking an export for live provider authority.

## ADDED Requirements

### Requirement: An authenticated export receipt is immutable and idempotent
The service SHALL accept an export only for its authenticated internal owner, calculate a SHA-256
digest while receiving the archive bytes, and retain the exact received bytes in service-owned
immutable storage before parsing. It SHALL create a durable import run referencing the archive's
digest, byte length, and raw-object evidence. A repeat receipt of the same owner and archive
digest SHALL converge on the existing run and SHALL not replace its bytes, evidence, or completed
outcome; the same bytes received for another owner SHALL remain a distinct owner-scoped run.

#### Scenario: Receipt streams one immutable owner-scoped archive
- **WHEN** an authenticated owner submits a valid synthetic export as a stream
- **THEN** the returned run references immutable bytes whose SHA-256 and length equal the received
  stream, and a retry by that owner returns the same run without a second archive record

#### Scenario: Receipt does not cross owner boundaries
- **WHEN** two authenticated owners submit byte-identical synthetic exports
- **THEN** each owner receives an independently owned import run and neither can retrieve the
  other's run through receipt or completeness operations

### Requirement: Archive inspection rejects hostile input before parser projection
Before recognizing records, the service SHALL inspect an export archive with declared limits for
entry count, path depth, compressed bytes, decompressed bytes, and compression ratio. It SHALL
refuse absolute paths, parent traversal, empty or duplicate normalized paths, unsupported entry
types, and any entry or archive that exceeds a declared limit. Refusal SHALL record a typed failed
run warning without normalizing a post, relation, capture, or completeness claim; the immutable
received archive remains retained as receipt evidence.

#### Scenario: Traversal never reaches extraction or projections
- **WHEN** an authenticated owner submits an archive containing a `../` or absolute-path entry
- **THEN** the run is failed with a path-safety warning, no file is created outside the isolated
  extraction root, and no normalized archive records are stored

#### Scenario: Resource-limit archives are refused deterministically
- **WHEN** an archive exceeds any configured entry-count, nesting, compressed-size,
  decompressed-size, or compression-ratio limit
- **THEN** inspection refuses the archive with the violated limit named and creates no normalized
  post or relation projection

### Requirement: A detected export uses one deterministic versioned parser
The service SHALL detect a supported export version from the safely inspected archive and select
one recorded parser version for that input. Parsing equivalent supported fixture archives SHALL
produce the same ordered normalized posts, directed relations, warnings, and raw-record references
regardless of entry ordering. Every normalized export observation SHALL carry `data_export`
acquisition and `export_observation` saved authority. Unknown sections, records, and fields that
cannot be normalized SHALL remain raw evidence and explicit warnings; an unsupported export
version SHALL finish without fabricated normalized content or native Saved-list authority.

#### Scenario: A supported fixture normalizes deterministic records and relations
- **WHEN** the same supported synthetic export is supplied with its safe entries in different ZIP
  orders
- **THEN** both runs expose equal ordered post identities and directed relation identities, each
  normalized post has `data_export` and `export_observation`, and their parser versions are equal

#### Scenario: Unknown export material is retained rather than discarded
- **WHEN** a supported export contains one recognized post section and one unknown section or
  record
- **THEN** the recognized content is normalized, the unknown material has a raw evidence record
  and warning, and no inferred native Saved-list state is reported

### Requirement: Import reconciliation is idempotent and absence is non-destructive
The service SHALL reconcile a normalized export observation by stable provider post identity and
preserve directed relations using the existing relation contract. Applying the same completed run
again SHALL not duplicate a post, relation, raw record, or source fact. A post, capture, relation,
or source absent from an export SHALL remain unchanged and SHALL not produce a deletion,
unavailable, unsave, or removal observation.

#### Scenario: Replaying an import preserves one projection
- **WHEN** a completed supported import is applied again for its owner
- **THEN** the normalized post, relation, and source-fact counts remain unchanged

#### Scenario: Export absence does not delete a capture
- **WHEN** an existing owner capture is not represented in a later supported export
- **THEN** the capture and any preserved source remain available without a tombstone or changed
  upstream availability

### Requirement: Completeness reports exact comparable coverage without deletion claims
For every completed or completed-with-warnings run, the service SHALL persist and return a report
that counts distinct export post identities, identities matching existing owner captures, export-only
identities, capture-only comparable identities, and captures that cannot be compared because they
lack stable provider identity. The report SHALL identify parsed categories, unsupported or unknown
categories, parser warnings, and whether an export category explicitly substantiates a claim. It
SHALL state that coverage is an observation of that archive, not proof of account-history
completeness, native Saved membership, or deletion.

#### Scenario: Completeness math separates overlap and unknown captures
- **WHEN** an owner has two comparable captures matching the fixture export, one comparable
  capture absent from it, and one unresolved capture while the export has one additional post
- **THEN** the report counts two matches, one export-only identity, one capture-only comparable
  identity, one non-comparable capture, and no deletion or native-Saved claim
