# Social-source publishing Specification

## Purpose

Publishes each preserved Threads source as an honest, contract-conformant fact that Knowledge can
index and analyse, while keeping provider availability and analysis-result ownership explicit.

## Requirements

### Requirement: A preserved Threads source publishes a conformant fact

When an explicit Threads capture or an official own-account observation has preserved a normalized
provider post, the service SHALL append exactly one state-carried `social.source.captured.v1` fact
for its owner and SHALL append a
`social.source.updated.v1` fact whenever the published normalized state changes. Each fact SHALL
carry the published social-contract snapshot with platform `threads`, stable source identity,
provider identity, canonical permalink, acquisition method, saved authority, capture and
publication timestamps where known, relations, raw-evidence reference where retained, content
digest, and upstream availability. A capture-backed source SHALL retain its explicit capture
provenance; an official-only source SHALL carry official provenance. The service SHALL not publish
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

#### Scenario: An unavailable-only capture stays local

- **WHEN** a Threads capture ends in a truthful unavailable fallback without preserved normalized
  content
- **THEN** no captured or updated social-source fact is appended for that capture

### Requirement: Knowledge completion links only to the analysed source revision

The service SHALL accept `knowledge.analysis.completed.v1` as a privacy-safe observational fact
only when its owner, `social_source_id`, and `content_digest` match a previously published Threads
source revision. It SHALL persist the completion linkage without storing a Knowledge run identifier
or result body, SHALL retain linkage for older revisions as superseded evidence, and SHALL make
redelivery idempotent.

#### Scenario: A matching completion round-trips through the source revision

- **WHEN** a completion fact names a published Threads source and its exact content digest
- **THEN** the persisted linkage is retrievable by that source identity and digest, and replaying
  the same completion produces no second linkage

#### Scenario: A completion cannot attach across revisions or owners

- **WHEN** a completion fact has a different digest or owner from the published Threads source
- **THEN** it creates no linkage to that source and does not change the source's published state

### Requirement: A provider tombstone republishes the retained source as unavailable

When a tombstone establishes that a previously published Threads post was deleted upstream, the
service SHALL append `social.source.updated.v1` whose snapshot sets
`upstream_availability = deleted_upstream` while retaining the previously preserved text, media
metadata, provenance, and raw-evidence reference. It SHALL NOT emit `social.source.removed.v1`,
because that event means a local-library removal rather than a provider observation.

#### Scenario: A post tombstone reaches Knowledge without erasing capture evidence

- **WHEN** a previously published Threads post receives a deletion tombstone
- **THEN** the next outbox fact is a contract-conformant updated snapshot with
  `deleted_upstream`, and the locally preserved post and capture evidence remain readable
