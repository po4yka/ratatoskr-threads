## ADDED Requirements

### Requirement: An unacknowledged removal survives every later deletion

No deletion path SHALL remove a `social.source.removed.v1` outbox row whose `acknowledged_at` is null, whichever deletion target (capture or connection) removes later data of the same aggregate or post.

#### Scenario: a later capture deletion keeps the earlier unacknowledged removal

- **WHEN** a capture deletion removes the final source of a post and a removal row is queued, then a second capture deletion touches the same post
- **THEN** the unacknowledged removal row is still present with its stored bytes unchanged

#### Scenario: a later connection deletion keeps the earlier unacknowledged removal

- **WHEN** a connection deletion queues a removal and a later deletion of the same aggregate runs
- **THEN** the unacknowledged removal row is still present with its stored bytes unchanged
