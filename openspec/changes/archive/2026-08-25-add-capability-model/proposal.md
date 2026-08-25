# Add the capability model, provenance semantics, and relation contracts

## Why

The Threads API surface is limited and evolving, so truthfulness about capabilities is a first-class requirement; the retired monolith captured Threads permalinks with no capability model, which left records whose provenance nobody could explain and made absence indistinguishable from deletion. Implementation plan item 2 must state what this bounded context can acquire, what each acquisition mode is allowed to prove about saved state, how upstream availability differs from local preservation, and which relation edges it models — before any lane exists (plan items 3-9 inherit these semantics).

## What Changes

- Add a `capability` module to `crates/threads-archive`: typed acquisition modes (`ExplicitCapture`, `PublicResolution`, `OwnAccountSync`, `DataExport`, `LegacyImport`), each answering an honest support status (`Supported`, `Planned`, or `NotSupported`), the closed set of wire acquisition-method values it produces, and the strongest saved-authority claim it may make. The explicit-capture mode owns the three client lanes of this context: `share_extension`, `browser_extension`, and `telegram_capture`.
- State the native Saved list as an explicit non-capability (`NotSupported` with the documented reason): no supported provider surface exposes a personal account's Saved list, so no mode may claim it.
- Keep upstream status and local preservation as distinct vocabularies by construction: the six-value availability vocabulary mirrored from the schema CHECKs describes the provider's side only, `PreservationState` describes what Ratatoskr holds, and applying any upstream observation to any preservation state leaves the preservation unchanged.
- Define the relation contract in a `relation` module: a validated open relation-kind token matching the published `SocialRelationKind` grammar value for value (so provider edge kinds this service does not model yet survive storage losslessly), and a post-relation type that names its target by stable provider external id with an explicitly representable unresolved target.
- Close the contract-alignment gaps found against the published `ratatoskr-social-contracts` crate, editing `schema.sql` in place per development status: add `public_resolution` to both provenance tables' acquisition-method CHECKs; align both saved-authority CHECKs with the contract grammar (`unknown` becomes `legacy_observation`; `authoritative_platform_state` joins as the ceiling only own-account sync may reach); open `post_relations.relation_kind` from three fixed values to the published token grammar.
- Document the matrix, authority rules per mode, the upstream-versus-preservation boundary, and a value-for-value alignment review against `ratatoskr-social-contracts@fb88f94` including the remaining gap list with dispositions in `docs/CAPABILITY_MATRIX.md`; align the README authority summary.

Out of scope: implementing any acquisition mode (capture intake, resolution adapters, export import, OAuth sync are plan items 3+); consuming the contracts crate as a build dependency; event publishing; redesigning `post_relations` storage for unresolved targets (plan item 4 owns that representation).

## Capabilities

### New Capabilities

- `capability-model`: The provenance-semantics layer — capability-matrix lookups per acquisition mode (support status, wire method vocabulary, authority ceiling), the native-Saved non-capability, exhaustive mapping between local constants and the published social-contract grammars, and the rule set keeping upstream availability separate from local preservation. Every requirement is executable against the library or the applied schema.
- `relation-contract`: The reply/quote/repost edge contract — the open validated relation-kind token aligned with the published grammar, direction preserved from child to target, targets named by stable provider external id, and unresolved targets represented explicitly instead of dropped.

### Modified Capabilities

- `archive-schema`: The provenance vocabularies enforced by named CHECK constraints change: acquisition methods gain `public_resolution`; saved authorities become exactly the four published contract values (`explicit_user_capture | export_observation | authoritative_platform_state | legacy_observation`, replacing `unknown`); `post_relations.relation_kind` accepts any token matching the published relation-kind grammar instead of only `reply`, `quote`, and `repost`.

## Impact

- Code: new `capability.rs` and `relation.rs` modules plus integration tests in `crates/threads-archive`; `schema.sql` vocabularies edited in place; no service binary behaviour changes.
- Dependencies: none added. `ratatoskr-social-contracts` is consumed as a reviewed reference at recorded revision `fb88f94` (repo `po4yka/ratatoskr-contracts`), not a build dependency; the gap list records when that decision must be revisited (event publication, plan item 5).
- Cross-repository contracts untouched: the social contracts are read-only here, and `telegram_capture` remains a documented local extension pending an upstream decision recorded in the gap list.
- Fleet gates: unchanged; the existing CI gate covers the new modules and schema text.
