## Context

See `proposal.md` for motivation and `specs/social-source-publishing/spec.md` for required
behaviour. The archive currently preserves captures, normalized posts, raw public-resolution
revisions, tombstones, and generic outbox/inbox rows, but has no tenant-scoped social source,
contract event builder, or Knowledge completion projection. The published contracts are pinned at
`ratatoskr-contracts` commit `9a9cdead0c689b946a52648eb76cc40158bd3c7b`; the workspace
`social-analysis-intake` agreement defines captured/updated facts as analysis requests.

## Goals / Non-Goals

**Goals:**

- Make one normalized, tenant-owned Threads source revision available to downstream consumers as
  a fully typed social contract event.
- Preserve the exact source revision that Knowledge completed without importing Knowledge-owned
  state.
- Make provider deletion observable downstream while retaining locally held evidence.

**Non-Goals:**

- NATS transport, account OAuth, provider re-resolution scheduling, media byte downloads, and
  Knowledge's social analysis worker or result bodies.
- A local user-deletion workflow. `social.source.removed.v1` remains reserved for that distinct
  future capability and is never inferred from a provider tombstone.

## Decisions

### D1: Snapshot facts are persisted transactionally in the existing outbox

The archive will build a contract snapshot from committed source, capture, post, relation, and raw
evidence rows, wrap it in the published event envelope, and insert it in `outbox_events` in the
same transaction that creates or changes the source revision. This makes database commit the
durable publication boundary; a later transport worker can deliver the exact stored bytes without
reconstructing a potentially changed snapshot.

Alternative: publish directly from the resolver. Rejected because a process failure between the
database write and publish loses a source fact, and rebuilding later can change timestamps or
relations.

### D2: Source identity is tenant-scoped and revision linkage is digest-scoped

A first-version source projection will map an owner plus normalized Threads post to one stable
`social_source_id`. A canonical digest of the complete emitted snapshot identifies a revision.
The projection permits several capture intents to point to one provider post without making two
owners share archive intent. Completion linkage stores only owner, source identity, digest, and
completion instant; it is accepted through the inbox's event-id deduplication.

Alternative: use `post_id` or `capture_id` as the shared source identity. Rejected because a post
is globally normalized while library ownership is tenant-specific, and a capture id cannot model
multiple idempotent capture intents for one source.

### D3: Provider deletion updates availability rather than removing the library source

When a deletion tombstone names a published post, the producer retains its capture and evidence
but produces an updated snapshot marked `deleted_upstream`. That state change produces a distinct
content digest and therefore a truthful new analysis input. The removal event is not reused:
the accepted workspace interface defines it as a local-library fact.

Alternative: emit `social.source.removed.v1` for every upstream deletion. Rejected because it
would falsely say the user removed the local archive entry and would make downstream privacy
deletion destroy valid preserved evidence.

### D4: Published contracts are an exact Git revision

The workspace will depend on `ratatoskr-social-contracts` (and its envelope types where needed)
at the exact accepted commit, with the resolved versions recorded in `Cargo.lock`. The contract
is the only source for wire names, provenance tokens, and snapshot validation.

Alternative: mirror the structs locally or depend on an unpinned default branch. Rejected because
both approaches permit producer/consumer wire drift.

## Risks / Trade-offs

- [A source update commits but its event is not transport-delivered] → the transactionally stored
  outbox record remains retryable and preserves the exact event body.
- [A stale Knowledge completion arrives after a newer source revision] → store it against its own
  digest as superseded evidence; do not alter the current revision.
- [An unavailable capture is mistaken for upstream deletion] → build source facts only from
  preserved normalized content and require a tombstone linked to an existing published post.
- [The schema changes during active development] → edit the current `schema.sql` and recreate
  disposable test databases; do not add migrations or migration tooling.

## Migration Plan

1. Deploy the first-version schema definition and service together so source projections, outbox,
   inbox, and linkage queries agree.
2. Start the future outbox transport only after it is separately implemented; the durable facts are
   safe to replay from the stored envelope bytes.
3. Roll back by reverting the producer change and recreating the development database from the
   prior schema definition. There is no production data migration in the current development
   status.
