## ADDED Requirements

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
