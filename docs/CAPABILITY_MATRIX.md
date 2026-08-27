# Threads Capability Matrix

Status: authoritative for this repository. Records what `ratatoskr-threads` can acquire from Threads, what each acquisition mode is allowed to prove about saved state, and how the model aligns with the published social contracts.

Threads' API surface is limited and evolving, and the monolith captured Threads permalinks without a capability model, so gaps were silent: records existed whose provenance nobody could explain, and absence was indistinguishable from deletion. This matrix exists so that cannot recur. The executable form of everything below lives in `crates/threads-archive/src/capability.rs`, `crates/threads-archive/src/relation.rs`, and their tests; the behavior contracts live in `openspec/specs/capability-model/` and `openspec/specs/relation-contract/`.

## The matrix

Explicit capture is implemented (plan item 3): intake canonicalizes permalinks, stores captures idempotently with pinned `explicit_user_capture` provenance, and records evidence-class unavailable fallbacks, so its row reports `Supported`. Every other lane reports `Planned`, and the matrix stays a commitment device — an implementation item must flip its own row's status with a reviewed test change before any code path can claim it.

| Mode | Status | Wire acquisition methods | Authority ceiling |
|---|---|---|---|
| `ExplicitCapture` | Supported | `share_extension`, `browser_extension`, `telegram_capture` | `explicit_user_capture` |
| `PublicResolution` | Planned | `public_resolution` | `explicit_user_capture` |
| `OwnAccountSync` | Planned | `official_api` | `authoritative_platform_state` |
| `DataExport` | Planned | `data_export` | `export_observation` |
| `LegacyImport` | Planned | `legacy_import` | `legacy_observation` |

## Stated non-capabilities

| Capability | Status | Reason |
|---|---|---|
| Native Saved-list synchronization (personal account) | NotSupported | no supported provider surface exposes the personal Saved list |

Threads provides Ratatoskr with no API that reads a personal account's native Saved list. No mode may synthesize that claim: an explicit capture proves the user saved the item to Ratatoskr at `captured_at`, an export proves it was saved at some point in the past, and neither proves current native membership. Deleting a Ratatoskr capture likewise never implies a native unsave.

## Authority rules

The authority ceiling is data on the mode, not caller discipline:

- `ExplicitCapture` and `PublicResolution` may never exceed `explicit_user_capture`. Public resolution observes upstream content; it does not observe the user's saved state, so resolving a capture raises availability knowledge only, never authority.
- `OwnAccountSync` may reach `authoritative_platform_state`, but only for the connected user's own posts and replies that the official API actually exposes. A connected account never widens the authority of captures about other accounts' content.
- `DataExport` may never exceed `export_observation`: exports show past state without live authority.
- `LegacyImport` may never exceed `legacy_observation`: migrated records are worth exactly what the monolith proved.

Nothing in the module offers a conversion that raises authority above a mode's ceiling. Downstream events preserve these values unchanged (`ratatoskr-knowledge` and clients must be able to trust the label).

## Upstream status versus preservation

Two questions stay two vocabularies because they have different owners:

- **Upstream** — what Threads last reported, tracked over time in `tombstones.availability` and collapsed onto the post row as `posts.upstream_status`: `active`, `deleted`, `private_or_inaccessible`, `author_unavailable`, `temporarily_unavailable`, `unknown`.
- **Local** — what Ratatoskr holds (`PreservationState`: content preserved, metadata only, user artifact only, nothing beyond the capture record).

Unlike Instagram, this context needs no collapse function between observation and status vocabularies: both levels deliberately share one six-value CHECK vocabulary, so the mapping is identity by construction and nothing can be lost in translation between tombstone and post.

Preservation is independent of every observation. `retention_after_observation` is identity on purpose: observing deletion upstream keeps whatever was captured before, absence from a later export deletes nothing, and demotion happens only through explicit user action. A metadata-only capture is never reported as a complete backup.

Missing partial API/resolver output is not deletion evidence: only an observed tombstone records deleted or unavailable state, and `unknown` exists precisely so "we looked at nothing yet" is never written as a stronger claim than that.

## Relation contract

Relations are directed edges from the referencing post (reply, quote, or repost) to its target, with targets named by stable provider external id — mirroring the published `SocialRelation` shape. Three decisions define it:

1. **Open kind grammar.** The published `SocialRelationKind` token is open on purpose, and the local contract matches it value for value: lowercase letters, digits, and underscores, starting with a letter, at most 32 characters. `reply`, `quote`, and `repost` are modelled today; a well-formed provider edge kind beyond them is preserved as itself, and the `post_relations.relation_kind` CHECK accepts exactly the same grammar so storage cannot force a choice between data loss and misfiling.
2. **Provider-id targeting.** Targets are always named by the target's stable provider external id, whether or not the target has been resolved into a local source record.
3. **Unresolved targets survive.** An unavailable parent does not invalidate the captured child: `RelationTarget::Unresolved` carries whatever evidence the referencing post exposed (provider id, permalink) and synthesizes nothing.

Gap: `post_relations` rows reference local `posts` rows through foreign keys, so storing an unresolved edge needs a representation chosen by plan item 4 (relation graph normalization). This change delivers the typed contract, not the storage.

## Alignment review: `ratatoskr-social-contracts`

Published social contracts are consumed directly from exact revision `9a9cdead0c689b946a52648eb76cc40158bd3c7b`, including the envelope and identifier types. Publication never mirrors wire payload structs locally.

| Contract concept | Local counterpart | Verdict |
|---|---|---|
| `AcquisitionMethod` (6 closed variants) | one wire method per variant, owned by exactly one `AcquisitionMode`; plus `telegram_capture` | aligned + documented extension |
| `SavedAuthority` (4 closed variants) | `SavedAuthority` mirror; reachable set equals the vocabulary via mode ceilings; schema CHECKs accept exactly these four | aligned, exhaustive |
| `SocialRelationKind` (open token grammar) | `RelationKind` validated token; `post_relations.relation_kind` CHECK widened to the same regex | aligned by widening the schema in place |
| `SocialRelation` (kind + target external id) | `PostRelation` (explicit referencing side + kind + target) | aligned; direction made explicit data |
| `UpstreamAvailability` (`available`, `unavailable`, `deleted_upstream`) | `active -> available`; `deleted -> deleted_upstream`; observed inaccessible/unavailable states -> `unavailable` | aligned for published resolved sources |
| `CaptureCompleteness` (`complete`, `partial`) | public-resolution source facts are `complete` for the bounded oEmbed representation; later media/export lanes own any partial warnings | aligned for item 5 |
| `SocialFolderMembership` | not applicable | Threads exposes no provider-native saved-folder membership through a supported surface |

Gaps found and their disposition:

1. The local acquisition grammar extends the contract grammar with `telegram_capture` — Telegram is a first-class explicit-capture client lane named in AGENTS.md, so dropping or misfiling it would be dishonest. Disposition: propose `TelegramCapture` upstream to `ratatoskr-contracts`, or map it explicitly at event publication (plan item 5); until then the extension stays recorded here and every contract variant remains produced locally by exactly one mode.
2. The contract's three-value `UpstreamAvailability` cannot express a never-observed local `unknown`; unavailable-only captures therefore do not publish a source fact. Once a preserved post has an observed tombstone, the mapping above is emitted in `social.source.updated.v1`.
3. `telegram_capture` remains a local explicit-capture lane absent from the current closed published `AcquisitionMethod` vocabulary. It is not silently relabeled in a social fact; publication for that lane requires an additive contract decision.
4. Preservation state has no column yet — intentional until the media-handling plan item defines storage policy and budget; the type exists so the distinction precedes storage.
5. Unresolved relation targets are stored explicitly by provider id and optional permalink; item 4 owns their normalization.
