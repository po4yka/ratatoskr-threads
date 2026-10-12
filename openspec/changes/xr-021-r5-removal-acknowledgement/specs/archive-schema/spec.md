## ADDED Requirements

### Requirement: Outbox rows carry a removal acknowledgement

`threads_archive.outbox_events` SHALL carry `acknowledged_at timestamptz`, `ack_outcome text` constrained to `applied` or `refused`, `resend_count integer not null default 0` and `next_resend_at timestamptz`.

#### Scenario: the outcome is closed

- **WHEN** a row is given an `ack_outcome` other than `applied` or `refused`
- **THEN** the database refuses it

#### Scenario: a new row starts unacknowledged

- **WHEN** an outbox row is inserted without the new columns
- **THEN** `acknowledged_at`, `ack_outcome` and `next_resend_at` are null and `resend_count` is 0
