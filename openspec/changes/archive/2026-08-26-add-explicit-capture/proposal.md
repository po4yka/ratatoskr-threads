## Why

Implementation plan item 3 is the first ingestion lane that stores user data: today the schema declares `captures`, `capture_resolutions`, and `tombstones` but no code writes them, so a client cannot archive a post. This change lands explicit capture intake end to end inside the library: canonical permalinks from messy share-sheet URLs, deterministic deduplication, capture records whose provenance is pinned to the explicit-capture lane, and truthful unavailable fallbacks.

## What Changes

- New permalink canonicalization: documented Threads post-URL forms normalize to one stable canonical permalink; everything else is refused with a typed error naming why.
- New validated capture intake: a request carries owner, idempotency key, raw URL, note, client source, and lane; the stored authority is always `explicit_user_capture`, the acquisition method is one of the three explicit-capture wire methods, and method/client pairing is enforced by named rules. Intake derives `captured_at` from the acceptance clock (AGENTS.md allows the request to contain or derive it); replay leaves the first value untouched.
- Deterministic idempotency: replaying a submission with the same `(user_ref, idempotency_key)` converges on the one stored record — same id, same captured time, same status — even when the raw URL text differs but canonicalizes to the same permalink; distinct keys create distinct captures over one source.
- Truthful unavailable fallback: evidence-backed unavailability (`deleted`, `private_or_inaccessible`) writes a tombstone plus an `unavailable` resolution row and marks the capture `unavailable`; a resolver failure writes only a `resolver_failed` resolution row — never deletion evidence — and the capture stays accepted. Note, captured time, and URLs survive in every fallback shape.
- Capability matrix: `ExplicitCapture` flips from `Planned` to `Supported`; the other four modes stay `Planned`.
- Out of scope (implementation plan item 4): any network fetch, oEmbed/public resolution internals, short-link resolution, `posts` row creation, event publication.

## Capabilities

### New Capabilities
- `permalink-canonicalization`: which textual Threads URL forms are accepted, the single stable canonical value they produce, and which forms are refused.
- `explicit-capture-intake`: capture record provenance, lane/client pairing, replay determinism, and unavailable fallback record shapes.

### Modified Capabilities
- `capability-model`: the `ExplicitCapture` mode reports `Supported` now that its implementing plan item lands; the scenario asserting no mode claims support is replaced.

## Impact

- `crates/threads-archive`: new modules `permalink.rs` and `capture.rs` (types, validation, sqlx-backed store) exported through `lib.rs`; new integration tests `tests/permalink.rs` and `tests/capture.rs`; `capability.rs` status flip reflected in `tests/capability.rs`.
- One in-place `schema.sql` edit: `captures` gains `original_url text not null` beside `canonical_url`, preserving the share-sheet input byte-for-byte (AGENTS.md, "preserve the original URL"); development status permits editing the definition in place. Every other written value already sits inside the first-version CHECK vocabularies.
- One new pinned dependency: `chrono` (with the sqlx `chrono` feature) so `timestamptz` columns map to `DateTime<Utc>`; the OAuth expiry work in a later plan item needs the same mapping.
- README/DEVELOPMENT status text moves item 3 from planned to existing.
