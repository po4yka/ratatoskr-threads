## MODIFIED Requirements

### Requirement: A preserved Threads source publishes a conformant fact

When an explicit Threads capture, an official own-account observation, or a completed Data Export
observation has preserved a normalized provider post, the service SHALL append exactly one
state-carried `social.source.captured.v1` fact for its owner and SHALL append a
`social.source.updated.v1` fact whenever the published normalized state changes. Each fact SHALL
carry the published social-contract snapshot with platform `threads`, stable source identity,
provider identity, canonical permalink, acquisition method, saved authority, capture and
publication timestamps where known, relations, raw-evidence reference where retained, content
digest, and upstream availability. A capture-backed source SHALL retain its explicit capture
provenance; an official-only source SHALL carry official provenance; an export-only source SHALL
carry `data_export` acquisition and `export_observation` authority. The service SHALL not publish
a fact for an unavailable-only capture.

#### Scenario: A resolved capture creates a self-contained analysis fact

- **WHEN** an explicit Threads capture is resolved into preserved normalized content
- **THEN** its outbox contains one `social.source.captured.v1` envelope whose typed snapshot
  round-trips through the published contract and states the capture's provenance without claiming
  native Saved-list authority

#### Scenario: An official own-account observation creates a self-contained analysis fact

- **WHEN** a capable account synchronization preserves an official own post or reply
- **THEN** its outbox contains one `social.source.captured.v1` envelope whose typed snapshot
  carries `official_api` acquisition and `authoritative_platform_state` authority with its local
  observation timestamp and without a native Saved-list claim

#### Scenario: A completed export observation creates a self-contained analysis fact

- **WHEN** a supported Data Export preserves a normalized provider post that has no existing
  published source for its owner
- **THEN** its outbox contains one `social.source.captured.v1` envelope whose snapshot carries
  `data_export` acquisition and `export_observation` authority without asserting live provider or
  native Saved-list membership

#### Scenario: An unavailable-only capture stays local

- **WHEN** a Threads capture ends in a truthful unavailable fallback without preserved normalized
  content
- **THEN** no captured or updated social-source fact is appended for that capture
