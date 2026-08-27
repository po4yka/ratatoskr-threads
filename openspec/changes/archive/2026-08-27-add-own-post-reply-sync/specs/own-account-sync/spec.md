## Purpose

Defines truthful, incremental ingestion of the connected account's own Threads posts and replies through the supported official API.

## ADDED Requirements

### Requirement: Synchronization is capability-aware and bounded to official own content
The service SHALL run an own-account synchronization only when the connected account has the supported own-content capability. A requested run without that capability SHALL return an explicit no-op outcome, leave durable synchronization state unchanged, and make no official-provider content request. A completed run SHALL describe only the posts and replies returned by the supported official surface and SHALL not claim account-history completeness or native Saved-list state.

#### Scenario: Missing own-content capability is a truthful no-op
- **WHEN** a scheduled own-account synchronization is requested for a connected account whose capability is unavailable
- **THEN** the result is a no-op with the recorded unavailable reason, no provider listing request occurs, and the account's synchronization checkpoint is unchanged

#### Scenario: Supported content is observed without a completeness claim
- **WHEN** the official surface returns a page containing an own post and an own reply for a capable account
- **THEN** both observations are recorded as official own content and the outcome reports the observed page without asserting that the account's whole history or native Saved list was synchronized

### Requirement: Successful scans advance an account-bound checkpoint
The service SHALL persist an account-bound continuation checkpoint only after it has durably recorded every observation in the completed official scan. If a scan cannot complete, its prior checkpoint SHALL remain the next starting point and no partial checkpoint SHALL be exposed as current.

#### Scenario: Completed incremental scan advances its watermark
- **WHEN** a capable account completes an official incremental scan that returns a next watermark
- **THEN** a later scan for that account starts from that watermark and the stored checkpoint equals the completed scan's watermark

#### Scenario: Incomplete scan preserves the prior watermark
- **WHEN** an official incremental scan fails after returning some observations but before completion
- **THEN** the stored checkpoint remains the prior watermark and a later scan starts from that prior watermark

### Requirement: Official observations atomically take their permitted authority
For a provider post already represented from a lower-authority acquisition, the service SHALL atomically replace its normalized official-facing projection with the official observation, `official_api` acquisition, and `authoritative_platform_state` saved authority. It SHALL retain the stable provider identity, linked explicit captures, and any relation that the official observation supplies; it SHALL not use this authority to assert native Saved-list membership.

#### Scenario: Official observation swaps a lower-authority projection
- **WHEN** a capable account's official scan returns a provider post that is already stored from public resolution or explicit capture
- **THEN** one post with the same provider identity remains, its official projection and authoritative-platform-state authority are visible together, and the linked capture remains attached

#### Scenario: Official reply retains its parent relation
- **WHEN** an official scan returns an own reply carrying a stable parent provider identity
- **THEN** the reply and its parent relation are stored together as one committed observation
