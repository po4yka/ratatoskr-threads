## Why

The `ratatoskr_events` stream drops its oldest message after 7 days or at 1 GiB. Today the Threads outbox pump marks a `social.source.removed.v1` row published at the broker acknowledgement and never looks at it again, so a removal that Knowledge does not consume in time is lost and the deleted post stays in Knowledge. Changeset XR-021 round 5 (CONTRACTS.md R5-11, rule 4) makes Knowledge acknowledge every applied or refused removal with `knowledge.deletion.acknowledged.v1` and makes the producer re-send the fact until that acknowledgement arrives.

## What Changes

- Every `ratatoskr-*` git dependency moves to the round 5 contracts revision `d98967b7b7ee80442c93ef4657140068955c8f61`, which registers `knowledge.deletion.acknowledged.v1`.
- `threads_archive.outbox_events` gains `acknowledged_at`, `ack_outcome`, `resend_count` and `next_resend_at` (`schema.sql` in place). Only `social.source.removed.v1` rows use them.
- The pump arms `next_resend_at = published_at + resend_after` when it marks a removal published, republishes the stored bytes of a due unacknowledged removal with the same `event_id` as `Nats-Msg-Id`, and doubles the resend delay up to a cap. The resend never stops by count and never touches `published_at`.
- A new supervised consumer of the Edge-provisioned durable `ratatoskr_threads_deletion_acks` sets `acknowledged_at` and `ack_outcome` on the matching row. A `refused` outcome stops the resend, logs at ERROR without content and increments `threads_tombstone_refused_total`. Two gauges, `threads_unacknowledged_tombstones` and `threads_oldest_unacknowledged_tombstone_age_seconds`, are served on the operator listener. Readiness is not tied to them.
- New keys `RATATOSKR__TOMBSTONE__RESEND_AFTER_SECONDS` (default 600, 30..=86400) and `RATATOSKR__TOMBSTONE__RESEND_CAP_SECONDS` (default 21600, at least `RESEND_AFTER`, at most 604800).
- `deploy/nats/identity.conf` gains the three permissions for the new durable, byte-equal to Platform's stanza after the usual normalization.
- BREAKING: `nats::run` takes a `ResendPolicy`, and the outbox pump moves from `nats.rs` to `outbox_relay.rs` behind a transport trait; the service now refuses to start when the Edge-provisioned acknowledgement durable is missing or differs from its spec. `schema.sql` changes in place (no migration; recreate development databases).
- A removal that Knowledge has not acknowledged is never deleted. The only outbox delete (`privacy_deletion.rs`) already excludes every removal row; a test per deleting path pins that.

## Capabilities

### New Capabilities

- `removal-acknowledgement`: a removal is kept and re-sent until Knowledge acknowledges it.

### Modified Capabilities

- `archive-schema`: the four acknowledgement columns.
- `privacy-deletion`: an unacknowledged removal survives every later deletion.

## Impact

- Affected code: `schema.sql`, `crates/threads-archive/src/{nats,outbox_relay,removal_acknowledgement,deletion_acks,config,lib}.rs`, `services/threads-archive/src/{main,lib,workers}.rs`, the NATS stanza, the operator example, README and DEVELOPMENT.
- Out of scope: the oEmbed-only post text limit, live Meta resolution, erasing social facts from the events stream (CONTRACTS.md R5-15), a permanently invalid command leaving its operation unreported, and every other repository.

This change cites XR-021 CONTRACTS.md R5-11.
