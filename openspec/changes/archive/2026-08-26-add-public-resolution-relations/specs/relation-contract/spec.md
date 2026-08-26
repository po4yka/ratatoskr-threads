## ADDED Requirements

### Requirement: Resolved reply edges preserve an acyclic hierarchy

The service SHALL reject a resolved `reply` edge that would make its directed provider-post graph cyclic, while preserving valid unresolved targets as specified by the existing relation-target requirement. This invariant SHALL hold for concurrent resolution transactions as well as one fixture normalization.

#### Scenario: Concurrently competing reply edges cannot commit a cycle

- **WHEN** resolution transactions attempt to add reply edges that together would form a directed
  cycle
- **THEN** at most the acyclic subset commits and the refused transaction reports a cycle-specific
  result without silently dropping a previously stored valid edge
