# relation-contract Specification

## Purpose
Defines the reply, quote, and repost edge contract for Threads posts: which relation kinds exist on the wire, how direction and targets are named, and why an edge whose target is unavailable is stored as unresolved instead of dropped.

## Requirements

### Requirement: Relation kinds follow the published open token grammar
A relation kind SHALL be a validated token matching the published `SocialRelationKind` grammar (lowercase letters, digits, and underscores, starting with a letter, at most 32 characters), equal value for value to the serde representation of that grammar at the revision recorded in the alignment review. The documented kinds `reply`, `quote`, and `repost` SHALL be accepted; an unrecognized but well-formed provider edge kind SHALL be preserved as itself rather than refused or rewritten.

#### Scenario: Documented relation kinds are accepted
- **WHEN** a relation kind is parsed from each of `reply`, `quote`, and `repost`
- **THEN** every parse succeeds and round-trips to the same wire value

#### Scenario: An unknown well-formed kind is preserved
- **WHEN** a relation kind is parsed from a token the service does not model yet, such as `mention`
- **THEN** the parse succeeds, the token round-trips unchanged, and it is distinguishable from every documented kind

#### Scenario: A malformed kind is refused
- **WHEN** a relation kind is parsed from a token violating the published grammar, such as an uppercase letter, an empty string, a leading digit or underscore, or a 33-character token
- **THEN** the parse fails with an error naming the violated rule

### Requirement: Relations name their target by stable provider identity and keep direction explicit
A post relation SHALL be directed from the referencing post to its target, and the target SHALL be named by the target post's stable provider external id, mirroring the published `SocialRelation` shape. When the target has not been resolved into a local source record, the relation SHALL carry an explicitly unresolved target that preserves the provider external id or permalink evidence held for it; absence of a resolvable parent SHALL NOT invalidate the captured child and SHALL NOT invent target content.

#### Scenario: A reply names its parent by provider id
- **WHEN** a reply relation is constructed from the child's perspective with the parent's stable provider external id
- **THEN** the relation reports the documented kind, keeps the child-to-parent direction, and carries exactly that provider id

#### Scenario: An unavailable parent stays an unresolved relation
- **WHEN** a relation's target cannot be resolved to a local source record
- **THEN** the relation is representable with an explicitly unresolved target carrying the provider identity evidence held for it, and no target content is synthesized

### Requirement: Resolved reply edges preserve an acyclic hierarchy
The service SHALL reject a resolved `reply` edge that would make its directed provider-post graph cyclic, while preserving valid unresolved targets as specified by the existing relation-target requirement. This invariant SHALL hold for concurrent resolution transactions as well as one fixture normalization.

#### Scenario: Concurrently competing reply edges cannot commit a cycle
- **WHEN** resolution transactions attempt to add reply edges that together would form a directed cycle
- **THEN** at most the acyclic subset commits and the refused transaction reports a cycle-specific result without silently dropping a previously stored valid edge
