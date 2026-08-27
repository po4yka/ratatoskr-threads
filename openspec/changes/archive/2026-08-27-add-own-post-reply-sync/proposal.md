## Why

An OAuth connection can truthfully expose the connected account's own posts and replies, but the service currently records that capability as planned and never consumes it. Item 7 makes the official account lane useful without claiming account-history completeness, native Saved membership, or any browser-derived state.

## What Changes

- Add capability-aware scheduled incremental synchronization of posts and replies exposed by the official authenticated Threads surface.
- Persist an account-bound scan watermark only after a complete successful scan, so retries resume truthfully and never advance past uncommitted observations.
- Store the raw official observation and atomically replace a lower-authority projection for the same provider post with the official-api, authoritative-platform-state projection; preserve stable provider identity and relations.
- Return an explicit no-op result without calling the provider or changing checkpoints when the connected account lacks own-content synchronization capability.
- Mark the implemented own-account synchronization acquisition mode as supported.

## Capabilities

### New Capabilities

- `own-account-sync`: Scheduled, checkpointed ingestion of a connected account's own official posts and replies.

### Modified Capabilities

- `capability-model`: Own-account synchronization changes from planned to supported once the lane is implemented.
- `social-source-publishing`: A preserved official own-account observation is published with its actual official provenance, while existing captured sources remain supported.

## Impact

- Affected code: the Threads archive Rust domain/persistence layer, the current in-place `threads_archive` schema, official-provider adapter seam, capability reconciliation, synthetic provider fixtures, and account-sync tests.
- APIs: no new cross-repository wire grammar or provider-write capability; the existing official account capability becomes executable locally.
- Dependencies: no new production dependencies.
