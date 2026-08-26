## Purpose

Defines durable relation-graph normalization for public Threads observations so reply and quote
structure stays first-class, deterministic, and truthful when referenced posts are absent.

## ADDED Requirements

### Requirement: Reply and quote relations are directed first-class records

The service SHALL store each supplied reply or quote as a directed record from the referencing
post to a target named by stable provider post identity. It SHALL not encode provider relations in
post text, author fields, or a duplicated parent body, and it SHALL preserve a well-formed
provider relation kind without rewriting it.

#### Scenario: Fixture thread stores directed reply and quote edges

- **WHEN** a public fixture contains a reply-to-parent edge and a quote-to-target edge
- **THEN** the stored graph contains two directed first-class relations with the fixture's exact
  kinds and target provider identities

### Requirement: Missing targets remain explicit graph nodes by identity

When a supplied relation target has no local normalized post, the service SHALL retain an
explicit unresolved relation carrying the target provider identity and any observed canonical
permalink. It SHALL not create placeholder post text or discard the relation.

#### Scenario: An orphan relation stays explicit

- **WHEN** a fixture reply names a parent not included in the public observation
- **THEN** the child remains normalized and its stored relation is unresolved with the parent
  identity evidence intact

### Requirement: Reply hierarchy cannot contain cycles

The service SHALL reject a reply relation whose insertion would create a directed cycle in the
resolved reply hierarchy. A rejected relation SHALL not partially alter the post, revision, or
other relation records in its resolution transaction.

#### Scenario: A cyclic fixture is refused atomically

- **WHEN** a fixture would add a reply edge that closes a cycle in existing resolved reply edges
- **THEN** resolution fails with a cycle-specific error and no edge from that fixture is stored

### Requirement: Graph persistence has a deterministic ordering

The service SHALL expose normalized relations in a stable ordering by referencing provider identity,
relation kind, and target provider identity, independent of the observation's input order.

#### Scenario: Permuted relation input has one graph representation

- **WHEN** two equivalent fixtures differ only in relation-array order
- **THEN** their persisted and returned ordered relation graphs are equal
