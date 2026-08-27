## ADDED Requirements

### Requirement: Export receipt and import state retain immutable evidence
The current first-version schema SHALL represent an owner-scoped Data Export run with an immutable
archive digest and BlobStore reference, receipt byte length, detected export version, parser
version, typed terminal or running outcome, processed-record count, warnings, and a completeness
report. It SHALL retain raw archive sections and unknown records as separately addressable
raw-object evidence, and SHALL prevent a second run for the same owner/archive digest while
allowing byte-identical archives for different owners. This schema change SHALL be made in the
current schema definition without a migration file or migration tooling.

#### Scenario: Fresh schema stores a complete export receipt
- **WHEN** the current schema is applied to a fresh database and a completed export run with an
  archive raw object, unknown record evidence, and completeness report is inserted
- **THEN** all receipt, parser, warning, and report fields persist and a duplicate
  `(user_ref, archive_hash)` insert is refused

#### Scenario: Archive evidence does not imply a normalized projection
- **WHEN** a failed hostile archive run is inserted with its immutable archive raw object
- **THEN** the run and evidence persist while no post, relation, or capture row is required or
  created by the schema
