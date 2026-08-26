## ADDED Requirements

### Requirement: Public resolution evidence and graph targets are durable schema records

The first-version `threads_archive` schema SHALL contain a post-revision relation that references one normalized post and one immutable raw object while recording the parser version and observation time.
The relation table SHALL represent a directed referencing post, an optional resolved target post,
and required target provider identity/permalink evidence so unresolved targets are storable without
placeholder posts.

#### Scenario: Fresh schema stores a revision and an unresolved edge

- **WHEN** the current schema is applied to a fresh database
- **THEN** a public-resolution revision and a relation with no local target post but target provider
  identity evidence can both be inserted under their declared constraints
