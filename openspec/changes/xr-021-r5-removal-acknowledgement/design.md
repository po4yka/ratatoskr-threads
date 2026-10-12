## Context

The authoritative cross-repository text is XR-021 CONTRACTS.md R5-11. This document records only decisions internal to `ratatoskr-threads`.

## Decisions

### State lives on the outbox row

A removal row moves through: unpublished, published and armed (`next_resend_at` set), then resent any number of times (`resend_count`, `next_resend_at` pushed out), then acknowledged (`acknowledged_at`, `ack_outcome`). No second table exists. The row's `event_id` is the envelope's `event_id` and the `Nats-Msg-Id`, so the acknowledgement's `acknowledged_event_id` finds the row by primary key, and the tenant check compares the acknowledgement's `owner` with the stored envelope's `tenant_id`.

### The transitions are one module, the pump is a transport-generic function

`removal_acknowledgement.rs` holds the SQL transitions (`mark_published`, `due_resends`, `record_resend`, `record_resend_failure`, `acknowledge`, `unacknowledged_removals`) so the pump and the consumer share one definition of each. `mark_published` arms `next_resend_at` only for `social.source.removed.v1`; `record_resend` computes `now() + least(resend_after * 2^resend_count, cap)` from the pre-update `resend_count` and clamps the exponent so the arithmetic cannot overflow. `outbox_relay.rs` takes the pump out of `nats.rs` (which reaches the 850-line ratchet otherwise) and puts it behind a small `EventTransport` trait implemented for the `JetStream` context, so the state machine is tested against PostgreSQL with a fake transport and the wire behaviour against a real broker.

### Resend runs after the fresh rows and stops at a refusing broker

A pass publishes due unpublished rows first. When none of those failed, it republishes due removals, oldest due first, and stops at the first failed republish, as the fresh loop does. A failed republish backs the row off on `next_attempt_at` and leaves `published_at`, `resend_count` and `next_resend_at` alone, then surfaces the failure to the worker like a failed fresh publish does (the process is restarted rather than staying ready over a broken relay). A broker that refuses one publish will refuse the next, so the pass does not hammer it.

### Configuration is a section of its own

`tombstone.resend_after_seconds` and `tombstone.resend_cap_seconds` follow the strict closed key set of `config.rs`. The minimum of 30 seconds is a configuration rule; tests construct `ResendPolicy` directly with millisecond delays. The 120 second duplicate window of the events stream makes a resend sooner than that a no-op at the broker, which is why the default is 600.

### The consumer mirrors the extractor-report consumer

`deletion_acks.rs` verifies the Edge-provisioned durable (name, filter `evt.knowledge.deletion.acknowledged.v1`, explicit acks, 30 s ack wait, not push based), never creates one, and loops under the same supervision as the other workers: it is its own `WorkerKind`, so a consumer that returns fails readiness and exits non-zero. An envelope that does not decode, has another event type, another producer or an invalid payload is Termed: it cannot be attributed and redelivery cannot fix it. A valid acknowledgement for another family, another tenant or an unknown id is acknowledged and ignored. A database failure is Nak'd with a delay. A duplicate acknowledgement changes nothing because the update requires `acknowledged_at is null`.

### Gauges are refreshed by the pump pass

The pump already runs every second against the same database, so it refreshes `threads_unacknowledged_tombstones` and `threads_oldest_unacknowledged_tombstone_age_seconds` at the end of each pass from one aggregate query over rows with `acknowledged_at is null`. They are exported through the existing Prometheus recorder on the operator listener. Readiness ignores them: a Knowledge outage must not make Threads unready.

### Deletion never removes an unacknowledged removal

The one outbox delete (`privacy_deletion.rs`, `remove_final_owner_source`) is restricted to `event_type <> 'social.source.removed.v1'`, which is stricter than the R5-11 predicate `not (event_type = 'social.source.removed.v1' and acknowledged_at is null)`: it keeps acknowledged removals too, and the inventory already classifies the outbox as retained audit. That predicate is kept and pinned by a test per deleting path (capture target and connection target). No retention job touches the outbox.

## Risks

A removal Knowledge never acknowledges is re-sent forever with the delay doubling to the cap (6 hours by default); that is the R5-11 rule. A service that cannot find its acknowledgement durable refuses to start, so Edge must be started before Threads (already the case for the command durable).
