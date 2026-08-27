## Why

Resolved Threads captures are preserved locally but do not yet become the normalized social
facts that downstream search and Knowledge analysis consume. This leaves the legacy searchable
entry behavior absent and gives no way to record that Knowledge finished analysing a particular
revision or to propagate a user removal.

## What Changes

- Publish contract-conformant `social.source.captured.v1` and `social.source.updated.v1` facts
  for preserved, normalized Threads sources with their provenance, relations, availability, raw
  evidence reference, and content digest.
- Treat those state-carried facts as the sole asynchronous analysis trigger, as defined by the
  accepted workspace `social-analysis-intake` interface; do not add a separate request command.
- Persist only the privacy-safe completion linkage `(social_source_id, content_digest)` from
  `knowledge.analysis.completed.v1`, keeping Knowledge run and result bodies out of the Threads
  schema.
- Propagate a tombstone for an already published provider post as `social.source.updated.v1` with
  `upstream_availability = deleted_upstream`, while retaining its previously preserved content.
  A provider tombstone is not a local-library removal and therefore never emits
  `social.source.removed.v1`.
- Do not publish unavailable-only captures, because the shared snapshot cannot represent them
  truthfully.

## Capabilities

### New Capabilities

- `social-source-publishing`: Contract-conformant Threads source facts, Knowledge completion
  linkage, and local removal propagation.

### Modified Capabilities

None.

## Impact

Affected areas are `threads_archive` persistence and event boundary code, the first-version
`schema.sql`, contract-pinned Rust dependencies, and integration tests. The producer consumes the
published `ratatoskr-social-contracts` package at commit
`9a9cdead0c689b946a52648eb76cc40158bd3c7b` and follows the accepted workspace change
`add-social-analysis-intake`; no Threads-to-Knowledge database access or provider credentials are
introduced.
