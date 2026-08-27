## ADDED Requirements

### Requirement: Final local deletion propagates one canonical removal fact

When an owner-authorized capture or connection deletion removes the owner's final live holding of a social source, the service SHALL commit exactly one typed `social.source.removed.v1` outbox fact in the same database transaction as the live-source removal and deletion audit. The fact SHALL carry the stable source identity, owner, closed reason, and removal instant only; it SHALL not assert provider deletion, native unsave, or include removed content. Under the workspace `social-analysis-intake` contract this fact instructs Knowledge to delete or tombstone derived analyses, embeddings, and index entries.

#### Scenario: final capture deletion propagates Knowledge cleanup
- **WHEN** an owner deletes the last capture holding a published source
- **THEN** one `social.source.removed.v1` fact with `user_requested` is committed and no removed snapshot or content is present in its payload

#### Scenario: retention expiry uses the retention reason
- **WHEN** policy expiry removes the final live holding of a published source
- **THEN** one removal fact is committed with `retention_policy` and without an upstream-deletion assertion

### Requirement: Remaining holdings suppress removal and resurrection

The service SHALL not emit a removal fact while the owner still holds the source through another capture, official connection, or export-derived live holding. Once a removal fact has been committed, a late Knowledge completion or stale queued re-resolution SHALL not recreate a completion link or source membership; only a new explicit authorized acquisition operation may create a new live source fact.

#### Scenario: deleting one of several holdings emits nothing
- **WHEN** a source remains held by another live acquisition after one capture or connection is deleted
- **THEN** no removal fact is emitted and the current live source remains analysable

#### Scenario: late completion cannot recreate deleted linkage
- **WHEN** Knowledge delivers a completion for a digest after the owner's final holding was deleted
- **THEN** the inbox handles it replay-safely without recreating a live source or analysis link
