# data-export-reprocessing Specification

## Purpose
Defines deterministic dry-run and restartable application of an explicit parser version to an already retained immutable Threads Data Export archive.

## Requirements

### Requirement: Reprocessing uses verified retained evidence and an explicit supported parser

The service SHALL reprocess only an owner-authorized existing export receipt whose BlobStore bytes still match the retained digest and length. The caller SHALL name a parser version registered for the detected export version; unknown, incompatible, or ambiguous parser versions SHALL fail before projection work. Reprocessing SHALL never reacquire, replace, or silently reinterpret the original archive.

#### Scenario: tampered retained bytes stop before parsing
- **WHEN** the retained archive bytes no longer match the receipt digest or length
- **THEN** dry-run and apply both fail with a safe integrity result and create no projection or outbox mutation

#### Scenario: unsupported parser is refused explicitly
- **WHEN** a caller names a parser version not registered for the receipt's detected export version
- **THEN** the operation reports unsupported parser version rather than falling back to another parser

### Requirement: Dry-run has apply-fidelity without side effects

For the same receipt bytes, parser version, and current owner-scoped database snapshot, dry-run SHALL produce the same deterministic ordered classifications, record counts, warnings, conflicts, completeness report, and prospective source content digests that apply would produce. Dry-run SHALL perform no database write, BlobStore write/delete, outbox append, checkpoint update, or provider/network request. The report SHALL contain stable identifiers and bounded structural diagnostics but no post bodies, notes, credentials, raw archive bytes, or full private paths.

#### Scenario: dry-run and apply reports match
- **WHEN** dry-run is followed by apply against unchanged state for the same receipt and parser
- **THEN** their classifications, counts, warnings, conflicts, completeness evidence, and prospective-versus-applied digests are equal apart from run identity and timestamps

#### Scenario: dry-run leaves all durable state unchanged
- **WHEN** a dry-run completes for an archive containing normalized, unknown, and conflicting records
- **THEN** database row counts and values, BlobStore inventory, outbox contents, and restart checkpoints equal their pre-run state

### Requirement: Apply is checkpointed, restartable, and replay-safe

Apply SHALL persist a reprocessing identity bound to owner, receipt digest, detected export version, target parser version, and operation identity. It SHALL process records in a deterministic order, commit bounded checkpoints only with their projection/outbox effects, and resume after interruption from the last committed checkpoint. Replaying a completed operation SHALL return its retained report without duplicating revisions, conflicts, unknown-record evidence, or social-source facts.

#### Scenario: interruption resumes after the committed item
- **WHEN** apply stops after committing a checkpoint and is invoked again with the same operation identity
- **THEN** it resumes at the next deterministic item and the final result equals an uninterrupted run

#### Scenario: completed replay creates no duplicates
- **WHEN** a completed reprocessing operation is invoked again
- **THEN** the stored report is returned and all normalized revisions, evidence rows, and outbox event counts remain unchanged

### Requirement: Reprocessing never derives deletion from absence

Reprocessing SHALL preserve unknown sections and records under the existing import policy and SHALL treat absence from one archive or parser projection as no deletion evidence. A new parser MAY add or refine projections only from the retained archive bytes; ambiguous matches SHALL remain conflicts.

#### Scenario: a newer parser omits a previously projected category
- **WHEN** the target parser does not project a category present in an earlier run
- **THEN** no capture, source, post, or media record is deleted solely because of that omission and the report exposes the category difference
