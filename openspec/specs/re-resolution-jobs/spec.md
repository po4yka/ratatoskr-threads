# re-resolution-jobs Specification

## Purpose
Defines controlled refresh of eligible public Threads captures so stale observations can improve without uncontrolled crawling, privacy bypass, or provider-budget overrun.

## Requirements

### Requirement: Automatic re-resolution selects only eligible live captures

A re-resolution run SHALL consider only non-deleted owner-held captures whose policy deadline is due and whose last supported observation is resolved, temporarily unavailable, or resolver-failed. It SHALL exclude locally deleted captures, proven private/inaccessible or deleted upstream subjects, unsupported URLs, user-upload-only artifacts, and any target lacking current supported public-resolution eligibility. Re-resolution SHALL use only the approved public resolver and SHALL never use login sessions, cookies, hidden APIs, or anti-bot bypass.

#### Scenario: permanent privacy observations are not retried automatically
- **WHEN** a due selection contains a capture last proven private/inaccessible and one capture with a transient resolver failure
- **THEN** only the transiently failed capture is eligible for automatic public re-resolution

#### Scenario: deleted local data cannot be resurrected
- **WHEN** a stale queued candidate is deleted before its request begins
- **THEN** the worker rechecks eligibility, performs no request, and creates no new source, revision, or analysis fact

### Requirement: Every run has finite admission and provider budgets

A re-resolution run SHALL require non-zero finite limits for admitted captures, provider requests, accepted response bytes, operation duration, and concurrency. Before each request it SHALL atomically reserve both local run capacity and the latest persisted provider endpoint budget; when any guard cannot reserve capacity the request SHALL not start. Retries SHALL not exceed the same request and deadline budgets. Run results SHALL report attempted, updated, unchanged, skipped-by-reason, failed, request, and byte counts without URLs or post bodies.

#### Scenario: item limit stops admission before the next request
- **WHEN** a run reaches its maximum admitted capture count with more eligible candidates available
- **THEN** no resolver call begins for the next candidate and the report records it as budget-skipped

#### Scenario: response-byte budget cannot be overspent
- **WHEN** accepting another bounded resolver response would exceed the run byte budget
- **THEN** the response is refused, no raw or normalized partial body is committed, and total accepted bytes remain within the configured limit

#### Scenario: exhausted provider allowance starts no request
- **WHEN** the persisted endpoint budget has no remaining request allowance
- **THEN** the run starts no provider request and records provider-budget exhaustion separately from resolver failure

### Requirement: Re-resolution appends evidence and reports truthful outcomes

An accepted re-resolution SHALL append raw evidence and a parser-versioned revision under the existing public-resolution contract. Equal normalized content SHALL not emit a duplicate update fact; changed normalized content SHALL publish one full updated source fact. A timeout, malformed response, unavailable response, or exhausted budget SHALL preserve the prior live projection and SHALL not become deletion evidence.

#### Scenario: unchanged evidence produces no downstream churn
- **WHEN** a due capture resolves to the same normalized content digest
- **THEN** immutable observation evidence is appended while no duplicate social-source update fact is emitted

#### Scenario: missing output preserves prior content
- **WHEN** a previously resolved capture receives no accepted response within its budget
- **THEN** its prior normalized source remains current and the run reports the bounded failure without a tombstone or removal fact
