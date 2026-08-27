# public-resolution Specification

## Purpose

Defines the supported public Threads resolver: approved oEmbed observations become immutable, parser-versioned evidence and one deterministic normalized post projection without any private session or hidden-provider access.

## Requirements

### Requirement: Resolution uses only the approved public representation
The service SHALL resolve an accepted canonical Threads permalink only through the configured approved Threads public oEmbed surface over verified HTTPS. It SHALL send no browser cookie, user password, MFA value, or hidden consumer API request, and it SHALL treat an unsupported URL, non-success provider result, oversized response, invalid JSON, or mismatched canonical permalink as an explicit resolver outcome rather than inventing post data.

#### Scenario: A supported permalink resolves from the approved public surface
- **WHEN** the resolver receives an accepted canonical permalink and the approved surface returns a schema-valid public observation for that permalink
- **THEN** the service records a resolved observation and returns the normalized public post

#### Scenario: An unsupported provider result remains explicit
- **WHEN** the approved surface returns an unavailable, malformed, oversized, or mismatched observation
- **THEN** the service records the matching non-resolved outcome without creating synthetic post content or claiming native Saved-list authority

### Requirement: Resolver evidence and revisions are append-only
Every accepted resolver response SHALL be preserved as immutable raw evidence with its byte hash, byte length, media type, observation time, and parser version. The service SHALL append one post revision for every successful resolution and SHALL never update or delete an earlier raw evidence record or post revision while applying a later result.

#### Scenario: Re-resolution retains earlier evidence
- **WHEN** the same provider post is resolved twice with different public observations
- **THEN** both raw evidence records and both parser-versioned revisions remain addressable while the current normalized post reflects only the later observation

### Requirement: Normalization is deterministic and provenance-preserving
Given equal approved evidence, parser version, and capture identity, normalization SHALL produce equal provider identity, canonical permalink, text/embed metadata, availability, acquisition method `public_resolution`, and saved authority `explicit_user_capture`. It SHALL use a stable provider post identity to converge repeated observations on one normalized post without merging posts solely by text or author similarity.

#### Scenario: Equal fixture input normalizes identically
- **WHEN** the same approved public fixture is normalized in two isolated databases
- **THEN** the normalized post fields and ordered relation graph are equal in both databases

### Requirement: Scheduled re-resolution uses the same supported resolver contract

Automatic or operator-triggered re-resolution SHALL pass through the same HTTPS-only, bounded, no-private-session public resolver validation and append-only evidence rules as first resolution. Scheduling SHALL not broaden supported permalink forms, redirects, media behavior, authority, or unavailable-state interpretation.

#### Scenario: scheduled and immediate resolution enforce identical input safety
- **WHEN** the same malformed, redirecting, oversized, or mismatched response is supplied to immediate resolution and scheduled re-resolution
- **THEN** both refuse it under the same outcome class and neither creates normalized content from it

### Requirement: Re-resolution admission is revalidated immediately before I/O

Immediately before a re-resolution request starts, the service SHALL confirm that the capture remains live, owner-held, policy-due, supported, and within the run and provider budgets defined by `re-resolution-jobs`. A stale queued candidate that no longer qualifies SHALL be skipped without network I/O or persisted resolver evidence.

#### Scenario: deletion wins over queued refresh
- **WHEN** a capture is selected for re-resolution and then deleted before request admission
- **THEN** no public request starts and no raw object, revision, tombstone, or source fact is appended for that candidate
