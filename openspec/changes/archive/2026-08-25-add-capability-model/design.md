# Design: capability model, provenance semantics, and relation contracts

## Context

Plan item 1 left a service whose `schema.sql` already enforces closed provenance vocabularies: six acquisition methods, three saved authorities (`explicit_user_capture | export_observation | unknown`), a six-value upstream-availability vocabulary shared by `posts.upstream_status` and `tombstones.availability`, and a closed `post_relations.relation_kind` of `reply | quote | repost`. The published `ratatoskr-social-contracts` crate (repo `po4yka/ratatoskr-contracts`, revision `fb88f94`, 2026-08-25) defines the wire grammars this context must eventually publish: `AcquisitionMethod` (six variants including `PublicResolution` but not `TelegramCapture`), `SavedAuthority` (four variants, no `Unknown`), and an open-by-design `SocialRelationKind` token. The schema CHECKs predate that review; four gaps follow, enumerated in the alignment table of `docs/CAPABILITY_MATRIX.md`.

## Goals / Non-Goals

**Goals:**

- One typed home per concept in `crates/threads-archive/src/capability.rs` and `src/relation.rs`, each carrying its snake_case wire value as data so code, schema CHECKs, and contract serde representations can be pinned together by tests.
- Lookups that make dishonesty unrepresentable: authority ceilings fixed per mode, support statuses explicit for all five modes, the native Saved list stated as `NotSupported` with its reason.
- Executable alignment: tests pin the local value sets against the recorded contract vocabularies and against the live database catalog, so drift in any of the three places fails CI.
- A written alignment review (`docs/CAPABILITY_MATRIX.md`) recording the mapping, the found-and-fixed gaps, and remaining gaps with dispositions.

**Non-Goals:**

- Implementing any acquisition mode (plan items 3+); flipping any mode's status to `Supported` is their job.
- Adding `ratatoskr-social-contracts` as a build dependency (see Decisions).
- Redesigning `post_relations` storage so unresolved targets become rows; plan item 4 owns that representation. This change delivers only the typed contract.
- Schema changes beyond the vocabulary alignments; preservation state gets no column until media handling defines storage policy.

## Decisions

### 1. Mirror the contract vocabularies as local constants; defer the dependency

Local enums carry wire-value accessors whose strings equal both the schema CHECK values and the contract serde representations. The alignment tests hardcode the contract sets copied from `crates/social-contracts/src/vocabulary.rs` at revision `fb88f94`, cited in the test doc comments.

Alternatives: consuming the crate via a git dependency would give compile-checked exhaustiveness, but couples this gate to another repository's HEAD, has no sibling precedent (`ratatoskr-instagram` made the same call at the same review), and buys nothing until event publication (plan item 5) constructs real payloads — at which point the dependency arrives deliberately. Copying without a recorded revision was rejected; the review records `fb88f94`.

### 2. Five modes; explicit capture owns the Telegram lane

The task names four forward-looking modes; `LegacyImport` is added because the contract grammar carries it and monolith migration must land somewhere honest. Every contract `AcquisitionMethod` variant belongs to exactly one mode: `ExplicitCapture` produces `share_extension`, `browser_extension`, and the Threads-specific `telegram_capture`; `PublicResolution` produces `public_resolution`; `OwnAccountSync` produces `official_api`; `DataExport` produces `data_export`; `LegacyImport` produces `legacy_import`.

Unlike Instagram, the local acquisition set is therefore a documented superset of the contract set: AGENTS.md names Telegram capture as a first-class client lane, and dropping it would misfile real captures. The gap list records the upstream decision (propose `TelegramCapture` in `ratatoskr-contracts`, or map at event publication).

### 3. The saved-authority vocabulary becomes exactly the contract grammar

The scaffold CHECK carried `unknown`, which lets any caller file any record under no-provenance — precisely the rot this model exists to prevent — while lacking `authoritative_platform_state` and `legacy_observation`. Both provenance CHECKs are edited in place to the four contract values: a legacy record whose original provenance cannot be explained is still honestly labelled `legacy_observation` ("worth what the monolith proved"), and `authoritative_platform_state` becomes storable ahead of own-post sync (plan item 7) because the official API authoritatively states own-account content. Native Saved membership stays unrepresentable: Threads exposes no Saved surface at all, so no value carries that meaning.

Alternative considered: keeping `unknown` and documenting a superset gap — rejected; it weakens the honesty invariant the capability matrix is built to enforce, and development status makes the in-place edit free.

### 4. Support status is a three-valued explicit answer, not absence of code

`SupportStatus::{Supported, Planned, NotSupported}`. All five modes report `Planned` today (capture intake is plan item 3 here, unlike the sibling where it had already landed); `NATIVE_SAVED_LIST_SYNC` reports `NotSupported` ("no supported provider surface exposes the personal Saved list"). Flipping a status is a deliberate test-plus-spec edit in the implementing change.

### 5. Two upstream-facing vocabularies stay two; threads needs no collapse function

Instagram needed a seven-to-five collapse because observations and media rows carry different vocabularies. Threads already tracks both levels in one closed six-value vocabulary (`active | deleted | private_or_inaccessible | author_unavailable | temporarily_unavailable | unknown`) on tombstones and posts alike, so the observation-to-status mapping is identity by construction and deliberately absent from the API. What remains is the separation rule: `UpstreamAvailability` (provider's side) and `PreservationState` (what Ratatoskr holds: content preserved, metadata only, user artifact only, nothing beyond the capture record) are distinct types, and `retention_after_observation(current, _observed)` returns `current` unchanged, making "an observation never demotes preservation" executable rather than conventional.

### 6. Relation kinds open to the published token grammar

`SocialRelationKind` is open on purpose in the contracts: an edge kind a consumer does not know must be kept, never discarded. The local `RelationKind` mirrors that grammar as a validated token (same pattern: lowercase start, `[a-z0-9_]`, at most 32 characters), and the `post_relations.relation_kind` CHECK widens from three fixed values to the same regex, so export parsing (plan item 8) can preserve provider edges losslessly. Direction follows the published `SocialRelation` shape: child → target, target named by stable provider external id; `PostRelation::target` is either resolved evidence or `UnresolvedTarget { provider_post_id }`, so an unavailable parent never invalidates a captured child and no content is synthesized.

Alternatives: keeping the closed three-value CHECK was rejected because refusing an unrecognized edge loses provider structure the raw evidence cannot restore; opening it without validation was rejected because garbage must fail loudly.

### 7. TDD pairs start from assertion-level failures

Task 1.x lands the test files together with skeleton modules whose lookups return placeholder values, so every failure is a failed assertion about behavior, not a compile error. The schema tests fail on the CHECK violations themselves, which need no skeleton.

## Risks / Trade-offs

- [Mirrored constants drift from contracts] → alignment tests pin both directions value-for-value and cite revision `fb88f94`; the dependency decision is revisited at event publication (plan item 5).
- [`retention_after_observation` looks like a no-op] → it encodes AGENTS.md's "absence never causes deletion" as a checked invariant; docs state why it exists.
- [`unknown` removal narrows what old tooling could store] → dev status means fresh databases only; no deployment holds rows across the change.
- [Open relation-kind CHECK admits nonsense that passes the regex] → the regex is the contract's own grammar; semantic modelling of new kinds remains a reviewed change, and malformed tokens still fail.
- [Status table rots when lanes ship] → spec scenario "No mode claims support while its lane is unimplemented" fails until the implementing change updates test and spec together.

## Migration Plan

None needed: development status means fresh databases only; `schema.sql` is edited in place and no deployment holds data across the change. Rollback is reverting one commit.

## Open Questions

None.
