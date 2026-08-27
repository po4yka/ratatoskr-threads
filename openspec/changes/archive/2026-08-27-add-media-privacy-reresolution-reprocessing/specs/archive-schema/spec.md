## ADDED Requirements

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
