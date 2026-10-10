## Purpose

Defines how an accepted explicit Threads capture is resolved through the public oEmbed surface, how failures are classified and retried, and how exactly one terminal operation report is produced per Platform operation.

## ADDED Requirements

### Requirement: Intake schedules resolution and records the operation
Delivering a Platform capture command SHALL store the command's operation id on the capture, set its resolution attempts to zero, leave `operation_reported_at` null, and set `next_resolution_at` to the delivery time, in the same transaction that writes the inbox row and the queued report. The queued report SHALL equal the contracts `capture_report` queued envelope content for that operation.

#### Scenario: Intake stores the operation and schedules resolution
- **WHEN** the Threads command fixture is delivered through the inbox
- **THEN** the capture row carries the command's operation id, zero attempts, a null `operation_reported_at` and a `next_resolution_at` that is not null and not after now, and the outbox holds one queued report with stage `capture_queued`

### Requirement: Provider failures are classified
The resolver SHALL classify a public-resolution failure as Deleted for provider status 404 or 410, Inaccessible for status 403 and for invalid JSON, a missing field, a wrong provider or a permalink mismatch, and Transient for status 429, any 5xx status, network failure and an oversized response.

#### Scenario: The classification table is total over the provider failure set
- **WHEN** each public-resolution error is classified
- **THEN** it maps to exactly the class named above

### Requirement: A resolved capture is reported once as succeeded
When the oEmbed surface returns a valid observation the capture SHALL be recorded as resolved, one `social.source.captured.v1` fact owned by the capture's tenant SHALL be queued, and exactly one terminal `succeeded` report whose single result is `social.post` targeting `social_source:<social_sources id>` SHALL be queued; a later resolver run SHALL add nothing.

#### Scenario: A 200 response produces one fact and one succeeded report
- **WHEN** a due capture is resolved against a fake oEmbed endpoint answering 200 and the resolver runs twice
- **THEN** the capture is `resolved`, one captured fact and one succeeded report exist, `operation_reported_at` is set and the second run changes nothing

### Requirement: Permanent failures terminate immediately
A Deleted classification SHALL record a deleted tombstone, mark the capture `unavailable` and report `failed` with code `social.source.deleted` and retryable false; an Inaccessible classification SHALL report `failed` with code `social.source.unavailable` and retryable false.

#### Scenario: A 404 response is a terminal deleted report
- **WHEN** the oEmbed endpoint answers 404 for a due capture
- **THEN** the capture is `unavailable`, a deleted tombstone exists and one failed report with code `social.source.deleted` and retryable false exists

#### Scenario: A 403 response is a terminal inaccessible report
- **WHEN** the oEmbed endpoint answers 403 for a due capture
- **THEN** one failed report with code `social.source.unavailable` and retryable false exists

### Requirement: Transient failures retry on a bounded schedule
A transient failure at attempt n SHALL set the next resolution time to now plus the smaller of 30 seconds times 4 to the power n minus 1 and 30 minutes. The configured maximum number of transient failures SHALL end in one failed report with code `social.source.unavailable` and retryable true, leave the capture `accepted`, and store a `resolver_failed` resolution row without a tombstone.

#### Scenario: Four transient failures then success
- **WHEN** the endpoint answers 503 four times and then 200, and the resolver clock is advanced to each due time
- **THEN** the due times are 30 seconds, 2 minutes, 8 minutes and 30 minutes after each failure and the fifth run reports `succeeded`

#### Scenario: Five transient failures end in a retryable failure
- **WHEN** the endpoint answers 503 five times
- **THEN** one failed report with code `social.source.unavailable` and retryable true exists and the capture stays `accepted`

### Requirement: A crash between recording and reporting only reports
If a capture is already resolved but its operation is not yet reported, the next resolver run SHALL report it without fetching again.

#### Scenario: Resume after a crash
- **WHEN** a capture was recorded as resolved but `operation_reported_at` is null and the resolver runs against an endpoint that fails the test if called
- **THEN** exactly one succeeded report is queued and the endpoint is not called

### Requirement: The resolver surface is configured or the process refuses to start
With the bus configured, `RATATOSKR__PUBLIC_RESOLUTION__RAW_OBJECT_ROOT` SHALL be required and absolute, `RATATOSKR__PUBLIC_RESOLUTION__OEMBED_ENDPOINT` SHALL be an https URL defaulting to `https://graph.threads.com/oembed`, and the maximum attempts, batch size and poll interval SHALL default to 5, 8 and 2000 ms.

#### Scenario: A missing raw object root is a named violation
- **WHEN** the bus URL is set and the raw object root is not
- **THEN** configuration fails with a violation naming `RATATOSKR__PUBLIC_RESOLUTION__RAW_OBJECT_ROOT`

#### Scenario: Unsafe values are refused
- **WHEN** the raw object root is relative or the endpoint is not https
- **THEN** configuration fails naming the offending key
