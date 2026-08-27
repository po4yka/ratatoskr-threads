# Ratatoskr Threads

`ratatoskr-threads` is the Threads account and capture bounded context for Ratatoskr. It combines official user-authorized account capabilities with explicit capture of public posts, official public representations, and versioned Data Export imports.

> **Status:** implementation plan items 1 through 8 are complete. Item 8 adds owner-scoped immutable Data Export receipts, bounded hostile-ZIP refusal, parser-versioned projection, raw retention for unknown sections, replay-safe source publication, and a completeness report that never turns export absence into deletion.

> [!IMPORTANT]
> **Ratatoskr is in development.** No database holds data that has to survive a schema change.
> While this status holds, these two rules replace what the documents below plan:
>
> - the API and the database keep their first version. There is no `v2` and no later major
>   version.
> - the database has no migrations. One schema definition exists, and a schema change edits it in
>   place.
>
> Only the repository owner changes this status.

## Role in Ratatoskr

Threads does not currently provide Ratatoskr with an authoritative API for enumerating a personal user's complete native Saved list. This repository therefore keeps official account data and explicit Ratatoskr captures as separate ingestion lanes.

### Official account lane

Where supported by the current Threads API and granted permissions, the connector may handle:

- Threads user identity;
- the user's own posts and replies;
- officially available interaction and insights data;
- publishing only through separately granted write authority;
- token expiry, refresh, reauthorization, and audit.

### Explicit capture lane

A public Threads post is archived through a deliberate action:

```text
Threads
  -> Share
  -> Ratatoskr mobile Share Extension
  -> canonical permalink
  -> ratatoskr-threads
  -> official public representation / oEmbed
  -> normalized SocialSource
```

Desktop capture follows the same semantics through `ratatoskr-browser-extension`.

## Core responsibilities

- official Threads account connection and capability detection;
- encrypted token ownership;
- synchronization of officially available own-account content;
- explicit public-post captures;
- canonical URL and short-link resolution;
- public oEmbed normalization;
- user notes and capture provenance;
- versioned Threads Data Export imports;
- media and attachment references;
- upstream availability observations;
- normalized social-source events;
- preservation of raw provider records.

The service does not use user passwords, server-side browser cookies, hidden bookmark APIs, or stealth automation.

## Authority and provenance

Every source records how it was acquired and what it proves:

```text
acquisition = OfficialApi | ShareExtension | BrowserExtension | TelegramCapture | PublicResolution | DataExport | LegacyImport
saved_authority = ExplicitUserCapture | AuthoritativePlatformState | ExportObservation | LegacyObservation
```

The saved-authority vocabulary equals the published `ratatoskr-social-contracts` grammar value for value; `TelegramCapture` is the documented Threads extension (see `docs/CAPABILITY_MATRIX.md`). What each acquisition mode may prove is fixed by its authority ceiling in the capability matrix.

A typical captured post is represented as:

```text
platform = Threads
acquisition = ShareExtension
saved_authority = ExplicitUserCapture
native_saved_state = Unknown
```

This means Ratatoskr can reliably state that the user captured the post locally at a known time. It cannot claim that the post remains in the native Threads Saved list unless an official authoritative surface later provides that evidence.

## Owned data model

The service owns a `threads_archive.*` PostgreSQL schema, created from `schema.sql` and applied at
startup. The first version declares:

```text
accounts
credentials
posts
post_revisions
post_relations
media
captures
capture_resolutions
export_runs
export_records
raw_objects
tombstones
outbox_events
inbox_events
social_sources
social_source_revisions
social_analysis_links
```

Rows are written by implemented and later capabilities; public-resolution rows retain immutable raw
oEmbed bytes before parser-versioned post revisions and first-class directed relation rows. The
schema vocabulary is already enforced: acquisition methods (`official_api | share_extension | browser_extension |
telegram_capture | public_resolution | data_export | legacy_import`) and saved authorities
(`explicit_user_capture | export_observation | authoritative_platform_state | legacy_observation`)
are closed CHECK constraints aligned with the published social-contract grammar, so a capture
cannot be stored as native Saved-list state. Large export archives, media, raw API/oEmbed
responses, and unknown provider records are stored in the content-addressed BlobStore.

## Capture flow

Clients submit a capture through Platform with an idempotency key. Threads then:

1. validates and canonicalizes the URL;
2. resolves supported short-link forms;
3. deduplicates the explicit capture;
4. retrieves the supported public representation;
5. stores raw evidence and normalized metadata;
6. records warnings or unavailable state without losing the user's note;
7. transactionally appends a contract-conformant `social.source.captured.v1` or `social.source.updated.v1` fact;
8. lets Knowledge perform optional analysis asynchronously.

A replay of the same capture converges on the same local source record while preserving new notes or collection links according to their owning context.

## Normalized post representation

The common social projection may include:

- local source ID and provider external ID;
- canonical URL;
- author identity and handle;
- post text;
- publication and edit timestamps;
- reply, quote, and repost relationships where exposed;
- media metadata and blob references where permitted;
- capture time and client source;
- user note and local linkage metadata;
- raw provider record reference;
- upstream availability state.

Unknown fields remain available in raw records so parser upgrades do not require reacquiring the post.

## Unavailable and private content

Ratatoskr does not bypass account privacy, login requirements, regional restrictions, removal, or other provider controls.

When a post cannot be resolved, the service may retain:

- canonical URL;
- capture timestamp and source client;
- user note and local collection references;
- selected text explicitly supplied by the user;
- an optional user-uploaded screenshot or file with separate provenance;
- the observed unavailability reason and time.

A user upload is never relabeled as an official provider response.

## Data Export imports

Threads exports are immutable, versioned observations rather than one permanent schema. An
authenticated caller supplies its `user_ref`; receipt streaming hashes and stores the exact bytes
before any inspection, bounded to 64 MiB. The run is unique per owner and archive digest, so a
retry returns the existing receipt rather than replacing raw bytes or creating a second import.

The current parser accepts the synthetic/redacted `threads-export-v1` layout: a safe ZIP with
`threads_export.json`, sorted `posts` and directed `relations`. Inspection rejects path traversal,
absolute/backslash paths, more than 1,000 entries, path depth over 16, cumulative compressed bytes
over 64 MiB, declared decompressed bytes over 256 MiB, and a ratio above 100:1. Optional extraction
uses a caller-owned isolated directory and never writes an archive-supplied path outside it.

Each import reads the retained digest-verified blob, persists parser/version/run evidence, creates
normalized posts and relations transactionally, and publishes Data Export observations with
`export_observation` authority. Unknown ZIP sections remain linked to the immutable archive in
`export_records` and make the run `completed_with_warnings`; they are never guessed as a newer
schema or native Saved state.

The report counts distinct export identities, captured identities that match, export-only
identities, capture-only comparable identities, and captures lacking a stable identity. It is
owner-scoped evidence of overlap, not a deletion signal or a claim that the archive is a complete
native Saved list. A refused archive remains retained and its run is marked `failed` without a
normalized projection.

## Official account connection

Credential requirements:

- OAuth state and callback-user binding;
- minimum required scopes;
- encrypted token storage and rotation;
- expiry and refresh tracking;
- explicit capability detection;
- separate consent for publishing or other provider mutations;
- no tokens in events, logs, traces, or public client responses.

Account synchronization and explicit captures are related by provider identity but remain distinct sources of authority.

## Linked content

External URLs from a post are delegated to `ratatoskr-extractor`. `ratatoskr-knowledge` may then create a composite analysis that separates:

- what the Threads post states;
- what the linked document contains;
- how the sources relate;
- which claims map to which provenance.

The Threads service itself does not run LLM analysis or own the search index.

## Commands and events

Expected contracts include:

```text
threads.account.connected.v1
threads.account.reauth_required.v1
threads.account.sync_requested.v1
threads.post.upserted.v1
threads.capture.requested.v1
threads.capture.resolved.v1
threads.capture.unavailable.v1
threads.export.ingest_requested.v1
threads.export.ingested.v1
social.source.captured.v1
social.source.updated.v1
social.source.removed.v1 # reserved for a future local-library deletion, never a provider tombstone
knowledge.analysis.completed.v1
```

Handlers are idempotent under at-least-once delivery. Capture, import, and account-sync results retain separate provenance.

## Security invariants

1. No Threads password or user browser cookie is collected.
2. Official OAuth and supported public representations are the primary provider surfaces.
3. A capture requires an explicit user action or user-provided export.
4. Private-content controls are never bypassed.
5. Write scopes require a distinct consent flow.
6. Unknown export records are retained losslessly.
7. Absence from an export does not delete a local capture.
8. User-uploaded artifacts have separate provenance.
9. Other services never receive Threads credentials.

## Observability

Core metrics include:

```text
threads_capture_duration
threads_capture_resolved
threads_capture_unavailable
threads_oembed_failures
threads_account_sync_duration
threads_rate_limit_waits
threads_export_import_duration
threads_export_unknown_records
threads_export_completeness
threads_reauth_required
```

Every operation records acquisition method, authority, resolver/parser version, warnings, and correlation identifiers.

## Non-goals

- Automatic authoritative mirroring of native Saved items without an official API.
- Server-side login or stealth browser scraping.
- Bypassing private or unavailable content restrictions.
- LLM analysis or search ownership.
- Treating local captures as provider-native saved state.
- Writing Ratatoskr tags and collections back to Threads.
- Claiming export completeness without validated evidence.

## Initial milestones

1. Define account, post, capture, export, and provenance schemas.
2. Implement URL recognition and canonicalization.
3. Add explicit capture and public oEmbed resolution.
4. Publish normalized social-source events.
5. Integrate mobile Share Extensions and browser extension.
6. Add encrypted official OAuth and capability discovery.
7. Add official OAuth and own-account synchronization.
8. Add safe versioned Data Export imports. ✓
9. Integrate linked documents with Extractor and analysis with Knowledge.

## Workspace integration

Planned: `ratatoskr-workspace` will pin Threads with compatible social contracts, Platform, Mobile, Browser Extension, Extractor, Knowledge, and clients. No workspace pin or integration profile exists for this service today. The connector will remain independently testable using mock OAuth/API servers, public-resolution fixtures, and synthetic export archives.

## Project status

Implementation plan items 1 through 8 exist: the service owns schema/provenance, explicit capture
and public resolution, social-source publication, encrypted official OAuth capability discovery,
checkpointed official own-post/reply observations, and safe raw-first Data Export reconciliation.
Media policy and provider writes remain planned.
