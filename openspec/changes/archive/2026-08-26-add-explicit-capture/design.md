## Context

The library already owns the schema (`Database`, `apply_schema`), the provenance vocabulary (`capability.rs`), and disposable-database test support (`test_support.rs`, feature `test-support`). The `threads_archive` first version declares every table this change writes; `(user_ref, idempotency_key)` uniqueness on `captures` exists. Plan item 4 owns public resolution: nothing here fetches from the network or creates `posts` rows, so a stored capture's `post_id` stays open exactly as the schema comment intends.

## Goals

- One stable canonical permalink per post, produced textually, with typed refusals for everything else.
- Deterministic replay: same owner + key → one row, identical record returned.
- Provenance that cannot lie: authority is not request-shaped at all, and lane/client pairing is validated.
- Fallback records that never invent provider evidence.

## Decisions

- **D1. Canonical host is `www.threads.net`.** Every existing fixture, doc example, and test in this repository uses it; one constant defines it, so a later reviewed change can move it together with its table. Both provider domains (`threads.net`, `threads.com`) and their www forms are accepted inputs.
- **D2. Handles fold to lowercase; codes stay verbatim.** Identity must not depend on how a share sheet spelled the handle, so canonicalization lowercases ASCII handles. Post codes are case-sensitive provider tokens: folding them could merge two distinct posts, so they are preserved byte-for-byte and case differences keep permalinks distinct.
- **D3. `/t/<code>` short form is refused at intake.** Textual canonicalization cannot know what a short code points at without a network fetch, which belongs to the public-resolution lane (plan item 4). Refusing names the rule instead of storing a guess.
- **D4. Idempotency rides the database unique constraint.** `submit` inserts and maps a `(user_ref, idempotency_key)` conflict to "select the stored row and return it unchanged". Capture ids are generated UUIDv7 at first insert, not derived hashes: the observable determinism contract (one row, identical returned record) holds under at-least-once replays without coupling the id scheme to the key format. A different key over the same permalink intentionally creates a second capture — distinct local saves are distinct intent (AGENTS.md identity rules).
- **D5. Authority is pinned, not requested.** The request type has no saved-authority field; the store writes `explicit_user_capture` unconditionally, which makes the misrepresentation physically unrepresentable rather than merely validated away. Acquisition method and client source are requested but must pair per the capability matrix's wire-method ownership (`share_extension` ↔ ios/android share clients, `browser_extension` ↔ browser extension, `telegram_capture` ↔ telegram).
- **D6. Evidence classes map to honest fallback shapes.** `deleted` and `private_or_inaccessible` are provider observations: tombstone (subject = capture) + resolution outcome `unavailable` + capture status `unavailable`. A resolver failure carries no provider evidence: only resolution outcome `resolver_failed`, no tombstone, capture stays `accepted` — missing output is never deletion evidence (AGENTS.md upstream-availability rules).
- **D7. Module layout follows the existing seam style.** `permalink.rs`: pure textual types (`Permalink`, `CanonicalizedUrl` carrying original + canonical) and a `TryFrom<&str>` entry with `thiserror` errors naming the violated rule. `capture.rs`: validated request/record types plus an async `CaptureStore` over the pool handed to it (`&Database`, like the rest of the crate), reusing `PersistenceError` for query failures and a dedicated `CaptureError` for intake-rule refusals. Exports go through `lib.rs`.
- **D8. One in-place schema edit, nothing else.** `captures` gains `original_url text not null`: AGENTS.md requires preserving the original URL and today only the canonical form has a column. Everything this change writes otherwise already fits the CHECK vocabularies and FK shapes; development status forbids migrations anyway, and none is needed.
- **D9. `captured_at` is derived at intake; time maps through pinned `chrono`.** AGENTS.md lets a capture request contain or derive its capture timestamp; item 3 derives it from the acceptance clock (`now()` at insert), and replay keeps the first value because conflict-nothing never touches the stored row. Client-supplied capture times arrive with the client-facing contract of a later plan item. Reading/writing `timestamptz` needs sqlx's `chrono` feature, so workspace pins `chrono` (default features off) — the established mapping later OAuth expiry work reuses.
- **D10. Hostile input is bounded by named rules.** Idempotency keys are non-empty and length-capped; raw URL text is length-capped before parsing. Refusals name the rule; nothing is truncated or silently repaired.

## Risks / Trade-offs

- Provider URL grammar may drift beyond the four accepted hosts or grow new path shapes. Accepted-input risk is bounded by refusing unknown forms (typed error surfaces immediately); widening the grammar is a reviewed table addition, never silent acceptance.
- Lowercased handles could theoretically conflate two handles differing only by case if the provider treats them as distinct; provider evidence from resolution (item 4), not the handle, will own identity there, and captures preserve the raw input either way.
- `[should_panic]`-style refusal tests assert error variants/messages, keeping refusal reasons part of the contract.

## Migration Plan

Not applicable: development status, no data exists behind this behavior. Tests create disposable databases from the current schema definition.

## Open Questions

None blocking. Canonical-host choice (D1) is recorded as a deliberate convention flip-point if the provider's primary domain changes.
