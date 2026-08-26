## Context

The capture lane stores canonical URLs and truthful unavailable outcomes but deliberately leaves `captures.post_id` open. `raw_objects` and `capture_resolutions` already reserve the evidence shape; `posts` and `post_relations` cannot yet retain parser-versioned post observations or a relation target that has no local post. See proposal.md and the workspace `blob-references` spec.

## Goals / Non-Goals

**Goals:**

- Fetch one canonical permalink through the approved public oEmbed HTTPS surface with finite timeout, response-size, and redirect policy.
- Preserve raw response bytes in a service-owned content-addressed location before deriving a parser-versioned post revision and current projection.
- Atomically converge posts by provider identity and persist reply/quote structure, including explicit unresolved targets, without permitting reply cycles.

**Non-Goals:**

- Own-account sync, Data Export ingestion, event publication, media-byte archival, write actions, browser-session access, or generic web crawling.
- Rendering or executing provider embed HTML. The response remains raw evidence; no embed HTML is exposed as rendered client content in this change.

## Decisions

- **D1: One approved oEmbed adapter with an injectable transport seam.** The production adapter uses the configured official Threads oEmbed endpoint with verified TLS and a single GET request; a trait permits synthetic fixtures and local HTTP tests. The input permalink is the sole query value. The adapter carries no cookies or user credentials. This implements the supported surface rather than scraping page HTML or private payloads.
- **D2: Bounded, no-retry GET contract.** Resolution has one finite operation deadline, bounded connect and response sizes, accepts HTTPS only, and follows no redirects. A GET is idempotent, but this initial provider adapter makes one attempt so it never amplifies provider failures; re-resolution is an explicit later caller action.
- **D3: Raw-first, local content-addressed evidence.** The resolver writes response bytes once under a service-owned SHA-256-addressed path and records the workspace `BlobRef` fields in `raw_objects` before parsing. Existing content is verified by digest and never overwritten. This fulfils the workspace blob-reference ownership contract without a shared blob service.
- **D4: Strict versioned parser.** A parser version constant and a closed oEmbed response schema accept only fields needed for this capability; required provider identity and canonical permalink are validated before normalization. Extra provider fields stay only in raw bytes. A malformed payload has an explicit resolver result and never creates a partial post.
- **D5: Current projection plus append-only revisions.** A successful resolution upserts `posts` by provider id, inserts one `post_revisions` row linked to its raw object, writes a resolved `capture_resolutions` row, then associates the capture. Earlier revisions are never updated. Re-resolution therefore appends evidence even when the projection values are unchanged.
- **D6: Explicit target identity replaces parent/child column convention.** `post_relations` uses `referencing_post_id`, optional `target_post_id`, mandatory `target_provider_post_id`, optional `target_permalink`, and `relation_kind`. This preserves the published child-to-target direction and stores orphans without synthetic `posts` rows.
- **D7: One transaction and graph advisory lock for reply insertion.** Resolution owns one SQLx transaction from post/revision through relation writes. Reply edges acquire the graph advisory lock, then a recursive query rejects any edge that reaches its referencing post from its target; this serializes the invariant across concurrent fixtures. Quotes remain directed graph edges but are not constrained as a parent hierarchy.
- **D8: Stable order at every boundary.** Parser relation candidates are sorted by referencing provider id, kind, and target provider id before persistence and reads use the same ordering. This makes fixture and replay output independent of JSON array order.

## Risks / Trade-offs

- [Provider oEmbed response lacks relation fields] → Preserve only relations explicitly supplied by the approved response fixture or adapter contract; never infer relations from embed HTML or page scraping. A future approved source requires a new reviewed adapter change.
- [Global reply-graph lock serializes writes] → Correctness is prioritized for this narrow public lane; measure contention before replacing it with a more granular proven-safe protocol.
- [Public endpoint schema drifts] → Strict parser produces a truthful resolver failure while raw evidence remains available for a parser-versioned follow-up.
- [Raw response includes hostile HTML] → Store bytes without execution and do not render them in this scope.

## Migration Plan

No database migration is created. Development status permits only an in-place edit to `schema.sql`; tests create fresh disposable PostgreSQL databases from that exact definition. Rollback is a code revert before release because no existing deployment data is preserved in this phase.
