# archive-schema Specification

## Purpose

Defines the first-version PostgreSQL schema contract owned exclusively by `ratatoskr-threads` in the `threads_archive` schema: what exists after a fresh application, that application is idempotent and transactional, and that provenance vocabularies are enforced by the database rather than by caller discipline.

## Requirements

### Requirement: A fresh database receives the complete first-version schema
Applying the schema definition to a fresh database SHALL create the `threads_archive` schema containing, at minimum, the account, credential, post, post-relation, media, capture, capture-resolution, export-run, export-record, raw-object, tombstone, outbox-event, and inbox-event tables declared in `schema.sql`. The set of created relations SHALL exactly match the file — no relation outside it is created, none of them is missing.

#### Scenario: Fresh apply creates every declared table and nothing else
- **WHEN** the embedded schema definition is applied to a newly created empty database
- **THEN** querying the catalog lists exactly the tables declared in `schema.sql`, all within the `threads_archive` schema

### Requirement: Schema application is idempotent under a lock
Applying the schema definition to a database that already has it SHALL make no change, and two processes applying concurrently SHALL both succeed with the file applied exactly once. Application SHALL be transactional: a failure part-way leaves the database without any of the schema objects.

#### Scenario: Second apply is a no-op
- **WHEN** the schema definition is applied twice to the same database and a second fresh apply runs concurrently from another connection
- **THEN** both applications succeed, the table set is unchanged after the second apply, and no object was created twice

### Requirement: Provenance vocabularies are enforced closed
The capture and post records SHALL carry an acquisition-method column and a saved-authority column constrained by named CHECK constraints to their documented closed vocabularies (`official_api | share_extension | browser_extension | telegram_capture | public_resolution | data_export | legacy_import` for acquisition; `explicit_user_capture | export_observation | authoritative_platform_state | legacy_observation` for authority). Inserting any other value SHALL be refused by the database. No stored value SHALL assert membership in a native Threads Saved list: no supported provider surface exposes one, so no vocabulary value carries that meaning.

#### Scenario: Unknown acquisition method is refused
- **WHEN** a row is inserted into a provenance-bearing table with an acquisition method outside the closed vocabulary
- **THEN** the insert fails with the named CHECK constraint

#### Scenario: Documented authority values are accepted
- **WHEN** rows are inserted using each documented saved-authority value, including `explicit_user_capture`
- **THEN** all inserts succeed

#### Scenario: Public resolution is accepted on provenance-bearing tables
- **WHEN** rows are inserted into both provenance-bearing tables using acquisition method `public_resolution`
- **THEN** all inserts succeed

#### Scenario: The former unknown authority value is refused
- **WHEN** a row is inserted into a provenance-bearing table with saved authority `unknown`
- **THEN** the insert fails with the named CHECK constraint

### Requirement: No foreign key crosses the schema boundary
The schema SHALL NOT contain a foreign key referencing or referenced from a table outside `threads_archive`; references to identifiers owned elsewhere are plain columns. Provider identity columns that must be unique SHALL carry uniqueness enforced at the database.

#### Scenario: Catalog shows zero cross-schema foreign keys
- **WHEN** the applied schema is inspected in the catalog
- **THEN** no foreign key on any `threads_archive` table references a table in another schema

### Requirement: Tests get disposable databases built from the same schema
The test harness SHALL create uniquely named databases from the same embedded schema definition used at startup, against a configurable test database URL defaulting to the documented local compose endpoint, and SHALL drop them deterministically afterwards. A missing test database server is a test failure, not a skip.

#### Scenario: Two tests get isolated databases
- **WHEN** the harness creates databases for two tests in one run
- **THEN** each test connects to its own database with the full schema present, writes there do not collide across tests, and both databases are dropped after cleanup

### Requirement: Relation kinds follow the published open token grammar
The `post_relations.relation_kind` column SHALL accept exactly the tokens matching the published social-contract relation-kind grammar (lowercase letters, digits, and underscores, starting with a letter, at most 32 characters) and SHALL refuse anything else, so provider edge kinds beyond `reply`, `quote`, and `repost` are preserved losslessly instead of being refused or misfiled.

#### Scenario: A well-formed relation kind beyond the documented three is accepted
- **WHEN** a post-relation edge is inserted with the well-formed relation kind `mention`
- **THEN** the insert succeeds

#### Scenario: A malformed relation kind is refused
- **WHEN** a post-relation edge is inserted with a relation kind violating the grammar, such as an uppercase letter, an empty string, or a 33-character token
- **THEN** the insert fails with the named CHECK constraint

### Requirement: Public resolution evidence and graph targets are durable schema records
The first-version `threads_archive` schema SHALL contain a post-revision relation that references one normalized post and one immutable raw object while recording the parser version and observation time. The relation table SHALL represent a directed referencing post, an optional resolved target post, and required target provider identity/permalink evidence so unresolved targets are storable without placeholder posts.

#### Scenario: Fresh schema stores a revision and an unresolved edge
- **WHEN** the current schema is applied to a fresh database
- **THEN** a public-resolution revision and a relation with no local target post but target provider identity evidence can both be inserted under their declared constraints

### Requirement: Export receipt and import state retain immutable evidence
The current first-version schema SHALL represent an owner-scoped Data Export run with an immutable archive digest and BlobStore reference, receipt byte length, detected export version, parser version, typed terminal or running outcome, processed-record count, warnings, and a completeness report. It SHALL retain the raw archive as addressable evidence and link unknown export sections to that evidence, while preventing a second run for the same owner/archive digest and allowing byte-identical archives for different owners. This schema change SHALL be made in the current schema definition without a migration file or migration tooling.

#### Scenario: Fresh schema stores a complete export receipt
- **WHEN** the current schema is applied to a fresh database and a completed export run with an archive raw object, unknown-section evidence, and completeness report is inserted
- **THEN** all receipt, parser, warning, and report fields persist and a duplicate `(user_ref, archive_hash)` insert is refused

#### Scenario: Archive evidence does not imply a normalized projection
- **WHEN** a failed hostile archive run is inserted with its immutable archive raw object
- **THEN** the run and evidence persist while no post, relation, or capture row is required or created by the schema

### Requirement: The first-version schema represents lifecycle work without migrations

Applying the current schema definition to a fresh database SHALL create the constrained state needed for media retention decisions, owner deletion operations and audits, durable blob-deletion work, re-resolution runs, and parser-version reprocessing checkpoints/reports. All new references SHALL remain inside `threads_archive`; secrets and large bytes SHALL remain outside audit/report rows. The repository SHALL contain no migration file, migration ledger, schema-version negotiation, or later database major.

#### Scenario: a fresh database exposes every lifecycle data class
- **WHEN** the embedded current schema is applied to an empty PostgreSQL database
- **THEN** the schema inventory and constraints expose each item-9 lifecycle class and the deletion classifier accounts for every owned table

#### Scenario: no migration mechanism appears
- **WHEN** repository schema and manifest files are inspected after this change
- **THEN** exactly the current schema definition remains the initialization source and no migration directory, runner, ledger, or SQLx migrate feature exists

### Requirement: Lifecycle state enforces safe identities and closed outcomes

Deletion/reprocessing operations SHALL be unique by owner and stable operation identity, re-resolution checkpoints SHALL prevent duplicate claim of one candidate, and media/blob work SHALL distinguish metadata-only, archived, pending deletion, and deleted-or-absent outcomes without storing an invalid mixed state. Constraints SHALL reject a deletion audit that embeds content-bearing fields or a reprocessing record not bound to one retained export receipt and parser version.

#### Scenario: duplicate operation identity is constrained
- **WHEN** two transactions attempt to create the same owner-scoped deletion or reprocessing operation
- **THEN** one durable operation identity wins and the other observes the same stored operation rather than creating duplicate work

#### Scenario: impossible media state is rejected
- **WHEN** a row claims archived bytes without the required digest, length, and BlobStore reference
- **THEN** the database rejects the write
