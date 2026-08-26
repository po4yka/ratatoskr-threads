## Why

Explicit capture now preserves a user's intent but leaves its `post_id` open. The service needs a
supported public resolution lane that turns approved provider observations into durable, honest
post and relation records without scraping private surfaces or rewriting evidence when a post is
re-resolved.

## What Changes

- Add a public-resolution adapter limited to the documented Threads public oEmbed representation;
  it accepts only normalized supported permalinks, uses no user session, cookie, or hidden API,
  and turns unavailable or malformed observations into explicit outcomes.
- Preserve each accepted resolver payload as immutable raw evidence with a parser version and
  append a normalized post revision on every resolution rather than overwriting prior evidence.
- Normalize the supplied post identity, permalink, text/embed metadata, availability, and capture
  linkage into the existing first-version schema.
- Store reply and quote edges as directed, first-class relations keyed by stable provider identity;
  retain unresolved targets explicitly and refuse a relation that would form a cycle in the reply
  hierarchy.
- Mark `PublicResolution` as supported in the capability matrix. Own-post synchronization,
  Data Export import, event publication, media-byte archival, and provider write operations remain
  out of scope.

## Capabilities

### New Capabilities

- `public-resolution`: approved public oEmbed acquisition, raw-first parser-versioned evidence,
  immutable re-resolution history, and deterministic normalized post projection.
- `relation-graph-normalization`: directed reply and quote graph persistence, explicit unresolved
  targets, cycle prevention, and deterministic fixture normalization.

### Modified Capabilities

- `archive-schema`: persist immutable post revisions and unresolved relation targets in the
  first-version schema.
- `capability-model`: `PublicResolution` reports `Supported` when this lane is implemented.
- `relation-contract`: make reply-cycle prevention and stable deterministic persistence part of the
  relation contract.

## Impact

- `crates/threads-archive`: new resolver/parser and SQLx store modules, existing capability and
  relation types, deterministic synthetic fixture tests, and a pinned Reqwest/Rustls dependency.
- `schema.sql`: one in-place first-version definition update for post revisions and relation target
  identity; no migration files or migration tooling.
- `README.md` and `DEVELOPMENT.md`: item 4 becomes implemented while later lanes remain planned.
