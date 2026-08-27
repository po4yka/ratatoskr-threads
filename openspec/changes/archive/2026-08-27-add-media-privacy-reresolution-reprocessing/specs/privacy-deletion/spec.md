## Purpose

Defines owner-authorized, replay-safe deletion of one capture or one official connection, including complete owned-data enumeration, BlobStore cleanup, audit, and downstream privacy propagation.

## ADDED Requirements

### Requirement: Deletion is owner-bound and idempotent

A deletion request SHALL carry an authenticated owner, stable operation identity, and exactly one target: a capture or official connection. The service SHALL reject an unknown or differently owned target without mutation. Replaying the same owner, operation, and target SHALL converge on the original result and SHALL not duplicate removal facts, audit rows, or blob deletion work.

#### Scenario: cross-owner capture deletion fails closed
- **WHEN** an authenticated owner requests deletion of a capture owned by another user
- **THEN** the service performs no row, blob, or outbox mutation and returns a safe ownership refusal

#### Scenario: deletion replay returns one result
- **WHEN** the same authorized deletion operation is delivered more than once
- **THEN** every delivery reports the same terminal or pending result and only one audit record exists

### Requirement: Every owned data class is classified before deletion

The service SHALL derive a deletion plan that enumerates every table and BlobStore reference class owned by the current Threads schema and classifies it as delete, detach, retain as non-sensitive audit, retain because another live owner or lane references it, or not applicable to the target. Applying deletion SHALL fail closed before mutation if any owned class is missing, ambiguously classified, or would leave a foreign-key or live-blob reference dangling. The completion report SHALL enumerate affected row and blob counts by bounded class without content, URLs, notes, usernames, or token material.

#### Scenario: schema inventory and deletion inventory stay complete
- **WHEN** the current owned schema table inventory is compared with capture and connection deletion classifications
- **THEN** every table is classified for both target kinds and neither plan contains an unknown class

#### Scenario: dry enumeration exposes all effects without mutation
- **WHEN** an authorized caller previews deletion for a populated capture or connection
- **THEN** the report lists the same row/blob classes and counts that apply would process while all database, BlobStore, and outbox state remains byte-for-byte unchanged

### Requirement: Capture deletion preserves independent holdings

Deleting one capture SHALL remove its note, intake record, resolution attempts, capture-specific user artifacts, and live library membership only when no other capture or official-account holding for that owner keeps the same source. It SHALL not assert a native unsave or upstream deletion, and SHALL not remove a provider post, raw evidence, media blob, relation, or another owner's library source while any live authorized reference still requires it.

#### Scenario: duplicate intent keeps the shared source live
- **WHEN** an owner deletes one of two captures that reference the same normalized post
- **THEN** only the selected capture-specific data is deleted and no removal fact is emitted for the still-held library source

#### Scenario: final capture removes the live library source
- **WHEN** an owner deletes the last local holding of a captured source
- **THEN** the source becomes unavailable to that owner's library, derived completion links are removed locally, and exactly one downstream removal fact is committed

### Requirement: Connection deletion removes only connection-owned authority

Deleting an official connection SHALL revoke and delete its credential material, budgets, checkpoints, account-derived revisions and live source holdings, then remove or detach normalized provider records only where no explicit capture, export observation, other connection, or other owner still requires them. Explicit captures SHALL survive connection deletion with their original acquisition and saved authority. The operation SHALL perform no provider post deletion, native unsave, or other provider write.

#### Scenario: an explicitly captured own post survives disconnect deletion
- **WHEN** an owner deletes a connection whose observed post is also held by an explicit capture
- **THEN** credential and connection-only state are deleted while the capture and its explicit-user-capture authority remain live and no removal fact is emitted for that retained library source

#### Scenario: a connection-only source is removed
- **WHEN** an owner deletes a connection and one normalized source has no independent holding
- **THEN** that source is removed from the owner's library and exactly one downstream removal fact is committed without claiming upstream deletion

### Requirement: Deletion completion is durable, audited, and privacy-safe

The database mutation, non-sensitive audit record, downstream removal outbox rows, and durable unreferenced-blob deletion work SHALL be committed atomically. Physical blob deletion MAY finish asynchronously, but the operation SHALL remain pending until every scheduled blob is verified deleted or already absent. Audit evidence SHALL retain operation/target identifiers, reason, bounded counts, timestamps, and completion state only; it SHALL contain no credentials, post bodies, notes, full URLs, raw archives, or media bytes.

#### Scenario: a database failure publishes no partial deletion
- **WHEN** deletion fails before its database transaction commits
- **THEN** the target remains live and no audit, removal event, or blob-deletion task from that attempt is visible

#### Scenario: audit survives content erasure without retaining content
- **WHEN** a deletion reaches complete state
- **THEN** operators can prove its scope and counts while none of the deleted private content or credential material remains in the audit
