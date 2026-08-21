# Ratatoskr Threads Agent Instructions

## Scope

These instructions apply to the `ratatoskr-threads` repository.

This repository owns Threads-specific account integration, explicit user captures, public post resolution, and versioned Threads Data Export imports.

## Repository mission

The service has two independent ingestion lanes:

1. **Official account lane** for Threads identity, own posts/replies, and other capabilities exposed by the supported official API and granted scopes.
2. **Explicit capture lane** for public Threads posts a user deliberately sends to Ratatoskr through mobile share targets, the browser extension, Telegram, or another explicit action.

The service must preserve the difference between provider-authoritative account data and a user-created Ratatoskr archive entry.

## Current phase

The repository is in architecture bootstrap. Do not assume Rust crates, OAuth flows, API clients, oEmbed resolution, Data Export parsers, migrations, or CI commands exist unless they are present in the checkout.

When creating initial implementation:

- make acquisition and saved authority mandatory fields;
- separate official API, explicit capture, public resolution, and export import adapters;
- preserve raw evidence before normalization;
- do not use a logged-in browser session as the supported synchronization path.

### Development status

Ratatoskr is in development. No database holds data that has to survive a schema change. While this
status holds, these rules are binding, and they override anything else in this repository that
plans otherwise, including the rest of this file:

- **One version only.** The API, the database, and the contracts keep their first version. Do not
  add a `v2` or a later major version, and do not add version negotiation, deprecation windows, or
  parallel-major routing.
- **No database migrations.** Do not add a migration file, and do not add migration tooling. A
  schema change edits the current schema definition in place, and a test database is created from
  that definition.
- **The product is `Ratatoskr`.** It is not "Ratatoskr Next". Do not write that name in code,
  documentation, identifiers, comments, or commit messages.

Only the repository owner changes this status. Ask before you write anything these rules forbid.

## How a change starts

Every non-trivial change begins as an OpenSpec change rather than as an edit, and each assistant
starts one in its own syntax. Claude Code has the command: `/opsx:propose <what you want to build>`,
or `/opsx:explore` first when the shape is not clear yet. Codex has no project-level command and
triggers the same skill by name, `$openspec-propose`, or lets its description match it. OpenCode has
its own command, `/opsx-propose`. Whichever starts it, the result is `openspec/changes/<id>/` holding
a proposal, the spec deltas, a design and a task list, and you read that plan before any code is
written. `/opsx:apply`, `$openspec-apply-change` or `/opsx-apply` builds it, and `/opsx:archive`,
`$openspec-archive-change` or `/opsx-archive` folds the deltas into `openspec/specs/`.

`openspec/specs/` holds the behaviour that is true today, and it starts empty on purpose. A spec here
grows from a change that needed it. Do NOT convert `docs/REQUIREMENTS.md`, `docs/INTERFACES.md`,
`docs/DOMAIN.md` or `docs/DATA_MODEL.md` into specs in bulk. Those documents stay where they are, as
material an exploration reads. A spec set produced by bulk conversion is large, stale on the day it
lands, and trusted by nobody.

Behaviour that more than one repository can see — the shape of a contract, the meaning of a field, the
order in which repositories must receive a change — belongs in the `ratatoskr-workspace` store, not
here. `openspec/config.yaml` references it, so `openspec instructions` in this repository lists the
store's specs with the exact command that fetches one. Cite that spec from a local proposal instead
of restating it.

### Tests come first

The task list carries one pair per behaviour. The first task adds a test that fails. The second makes
it pass. Never one task that does both.

- Run the new test before you write the implementation, and confirm it fails for the reason the task
  states — not for a compile error or a typo.
- A refactor task comes after the tests are green. It adds no test and changes no behaviour.
- A task that cannot start from a failing test says why in one line. Configuration, documentation and
  generated files are the usual reasons.
- Do not tick a task whose test has not been run.

Nothing can check the order in which the two were written. What CI does check is
`openspec validate --archived`, which fails when a change was archived with a task left unticked, and
the step in `fleet.yml` that fails when a repository holds a manifest and a `ci.yml` that never runs
a test. `ratatoskr-workspace/docs/QUALITY_GATES.md` states that limit rather than implying it is
covered.

## The Rust skill catalogue

`.agents/skills/` holds eighteen Rust skills, and `.claude/skills/` symlinks to them, so all three
assistants read one copy. Codex reads `.agents/skills/`, Claude Code reads `.claude/skills/`, and
OpenCode scans both, so the existing symlink already covers it and nothing belongs under
`.opencode/skills/`. Each is a reference sheet rather than a tutorial: the commands, flags,
thresholds and triage tables for one Rust concern. Your assistant reads the descriptions and opens a
skill only when the task matches one, so the set costs almost nothing until it is needed.

`rust-tdd` is the Rust form of the task pair above. `rust-lints` owns `clippy.toml`, which is where
this repository's size limits live. `rust-security` answers a `RUSTSEC` advisory.
`rust-async-internals` covers `tokio::select!` cancel safety and shutdown. `rust-database` covers
pool budgets and transaction ownership. `rust-compiler-errors` is the entry point when the build
fails and the cause is not obvious.

`rust-database` also carries a section on deploying migrations in compatible phases. The Development
status above overrides it: while that status holds, this product has no migrations at all. Read the
rest of that skill and skip that section.

The eighteen are identical in every Ratatoskr repository whose stack is Rust, and
`ratatoskr-workspace/.github/workflows/drift.yml` fails when one copy stops matching the others. Do
not edit a file under `.agents/skills/`. A correction belongs upstream in `po4yka/rust-skills` and
reaches this repository through `npx skills update`.

The catalogue holds forty-four skills and eighteen are vendored here.
`ratatoskr-workspace/docs/QUALITY_GATES.md` records which were left out and why. They are vendored
under BSD-3-Clause, (c) 2026 Nikita Pochaev, who also owns this repository; each `SKILL.md` keeps its
`license` field, and the full text is in that repository's `LICENSE`.

## Sources of truth

Use this order:

1. active task/changeset and accepted ADRs;
2. `README.md`;
3. social/event contracts from `ratatoskr-contracts`;
4. explicit capture records or complete import evidence;
5. official provider responses and safe redacted fixtures;
6. implementation details.

When the provider does not expose native Saved state, represent it as unknown. Do not infer authority from UI behavior or undocumented endpoints.

## Hard bounded-context rules

### Threads service owns

- Threads account linkage and encrypted credentials;
- granted scopes, token lifecycle, and account capability state;
- normalized own-account posts/replies and provider metadata;
- explicit Threads capture records and provider-specific resolution;
- acquisition method and saved-authority classification;
- public oEmbed/provider observations;
- Threads Data Export import runs, parser versions, raw archive references, and projections;
- upstream availability/tombstone state for Threads sources;
- Threads-specific outbox/inbox records;
- references to Knowledge analyses and client collection membership.

### Threads service does not own

- Platform sessions/devices;
- Ratatoskr collections/tags;
- generic article extraction;
- LLM summaries, embeddings, or search ranking;
- authoritative native Saved state not exposed by a supported source;
- user passwords, browser cookies, or hidden consumer tokens;
- Instagram/X state;
- Telegram/mobile/extension interaction state.

## Acquisition and saved authority

Every source records how it was acquired and what that observation proves.

Representative acquisition methods:

```text
OfficialApi
ShareExtension
BrowserExtension
TelegramCapture
DataExport
LegacyImport
```

Representative saved authority:

```text
ExplicitUserCapture
ExportObservation
Unknown
```

Rules:

- A Ratatoskr capture proves explicit user intent to save the post locally.
- `captured_at` is the local capture time, not the provider's native Saved timestamp.
- Do not expose a capture as authoritative membership in the native Threads Saved list.
- Deleting a local archive record does not imply a native unsave.
- Preserve provider publication time separately.
- Preserve acquisition/authority through events, analysis, search, and UI contracts.

## Official account lane

Official API work may include only capabilities supported by the selected API/scopes, such as:

- connected Threads identity;
- own posts and replies;
- available interactions or insights;
- publishing only through separately authorized, explicitly scoped product behavior.

Rules:

- request minimum scopes;
- record granted scopes and capability state;
- separate read connection from publishing/write consent;
- normalize provider objects by stable external ID;
- treat usernames/display attributes/URLs as mutable;
- keep provider SDK types inside adapters;
- do not claim account-wide history completeness unless the API traversal is complete and verified;
- do not expand this service into a general social automation engine.

## OAuth and credentials

- Validate OAuth `state`, callback-user binding, redirect URI, nonce/PKCE requirements, and token response shape.
- Encrypt and version access/refresh tokens.
- Record provider account ID, granted scopes, expiry, refresh, revoke, and reauthorization state.
- Detect scope/capability downgrade explicitly.
- Never send Threads tokens to Platform, Knowledge, clients, events, Telegram, or logs.
- Audit permission changes and provider write operations.
- Do not store passwords or MFA secrets.

## Explicit capture intake

A capture request must contain or derive:

- authenticated internal user/device identity;
- original/canonical Threads permalink;
- capture timestamp;
- source client;
- idempotency key;
- optional note/local collection references handled through the appropriate Platform/client contract;
- operation and correlation IDs.

Rules:

- validate and normalize supported permalink forms;
- preserve the original URL;
- deduplicate repeated delivery idempotently;
- use only supported public/provider resolution methods;
- do not obtain user browser cookies or hidden API traffic;
- store a truthful partial/unavailable result when resolution fails;
- preserve the explicit capture record independently of provider availability, subject to retention policy.

## Public post resolution and oEmbed

For eligible public posts, a supported public resolver/oEmbed path may supply:

- canonical URL;
- external post/media identity when available;
- author/display metadata;
- post text exposed by the endpoint;
- embed representation/thumbnail/media metadata;
- observed availability.

Rules:

- treat the response as a public-content observation, not Saved-list evidence;
- validate response size, schema, MIME, redirects, and URLs;
- sanitize embed HTML before client rendering;
- never execute embed HTML on the server;
- cache/revalidate using an explicit policy;
- preserve raw response/blob reference when permitted;
- do not reverse engineer private page payloads when the public resolver is insufficient;
- do not leak provider tokens in request logs or cache keys.

## Replies, quotes, and conversation relations

Threads content may have reply/quote/conversation relationships.

- Model each provider post as its own source/object.
- Preserve parent/reply/quote relations by stable external ID.
- Do not duplicate parent content into every child as if it were authored by the child.
- Resolve referenced public posts only within explicit budgets and permissions.
- An unavailable parent does not invalidate the captured child; store the unresolved relation.
- Knowledge may analyze a thread/conversation, but this service owns provider structure and provenance.

## Private and unavailable content

- Never bypass privacy, login, age, region, block, or access controls.
- Never request cookie/session exfiltration from clients/extensions.
- Preserve the permalink, capture timestamp, optional user note, and explicit unavailable state.
- Distinguish private/inaccessible, deleted, temporarily unavailable, unsupported, malformed, and resolver-failed states when evidence supports it.
- A user-uploaded screenshot/file is a separate artifact with separate provenance and access policy.
- Do not represent user-uploaded evidence as provider-fetched canonical content.

## Media handling

Media metadata and complete media-byte archival are separate capabilities.

- Do not automatically download all media for every capture.
- Define explicit policy for media eligibility, rights, URL expiry, size, storage budget, and completeness.
- Validate redirects, MIME, size, dimensions/duration, and hashes.
- Treat filenames and payloads as untrusted.
- Use approved BlobStore interfaces.
- Preserve provider-derived media separately from user-uploaded artifacts.
- Report metadata-only versus media-complete states honestly.

## Data Export imports

Threads Data Export archives are untrusted, versioned inputs.

Required import flow:

1. compute the archive hash;
2. store the original archive immutably;
3. enforce path traversal, absolute path, file count, nesting, decompressed size, and zip-bomb limits;
4. detect export/schema version and available categories;
5. parse into staging state;
6. preserve unknown sections as raw blobs/references when safe;
7. normalize known records;
8. emit counts, warnings, conflicts, and completeness report;
9. reconcile idempotently;
10. retain parser/import-run evidence.

Rules:

- absence of a category or object in one export does not prove deletion;
- do not promise the archive contains native Saved items until the actual export category establishes that;
- do not execute HTML, scripts, media, or archive contents;
- do not delete the original archive after import;
- parser upgrades create versioned reprocessing, not silent reinterpretation.

## Identity and deduplication

Prefer:

- stable provider post/media ID;
- canonical permalink;
- content hash as supporting evidence;
- acquisition/import provenance.

Rules:

- multiple local captures may reference one provider source while preserving capture-specific intent/timestamps;
- do not merge posts solely by author/text similarity;
- URL format changes do not create a new source when stable provider identity proves continuity;
- ambiguous export/capture matches remain conflicts instead of destructive merges;
- account-owned and captured external posts use compatible normalized source contracts without erasing their acquisition differences.

## Downstream integration

Publish normalized social-source events containing:

- platform/external identity;
- canonical URL;
- acquisition method;
- saved authority;
- author/publication/capture timestamps;
- text/media metadata;
- reply/quote relations;
- raw blob/reference and content hash where applicable;
- upstream availability;
- operation/correlation IDs.

`ratatoskr-knowledge` owns analysis/embeddings. Platform/clients own local organization/presentation. Generic linked articles are delegated to Extractor.

## Provider write operations

Publishing or other provider writes, if implemented, require:

- separately granted write scope;
- explicit user action and confirmation where material;
- idempotency key;
- persisted provider request/result evidence;
- audit record;
- truthful partial-success response;
- reconciliation after uncertain provider results.

Do not automatically publish, delete, or modify Threads content as a side effect of local archive operations.

## Upstream availability

Model availability explicitly, for example:

```text
active
deleted
private_or_inaccessible
author_unavailable
temporarily_unavailable
unknown
```

- Missing partial API/resolver output is not deletion evidence.
- Revalidation follows policy and provider terms.
- Publish tombstone/unavailable events for downstream index/projection updates.
- Preserve permitted audit/provenance even when the body can no longer be served.
- Distinguish lost scope/connection from provider content deletion.

## Persistence and migrations

Threads writes only its owned schema.

Conceptual data includes:

```text
threads_accounts
threads_credentials
threads_posts
threads_post_relations
threads_media
threads_captures
threads_capture_resolutions
threads_export_runs
threads_export_records
threads_raw_objects
threads_tombstones
threads_outbox
threads_inbox
```

Rules:

- no cross-schema writes or foreign keys;
- raw archives/responses remain separate from normalized projections;
- uniqueness/idempotency constraints reflect provider and capture identities;
- migrations preserve acquisition, authority, relation, and availability history;
- absence in partial API/export data never creates unproven deletion;
- secrets and large blobs use protected storage/reference mechanisms.

## Commands and events

Representative messages include:

```text
threads.capture.requested.v1
threads.capture.resolved.v1
threads.account.sync_requested.v1
threads.account.post_updated.v1
threads.export.ingested.v1
social.source.upserted.v1
social.source.unavailable.v1
social.connection.reauth_required.v1
```

Use canonical contracts, transactional outbox, inbox deduplication, correlation/causation IDs, and at-least-once-safe handlers.

Never publish native-Saved authority from an explicit capture.

## Prohibited implementation approaches

Do not add:

- server-side logged-in browser automation for Threads;
- password/MFA storage or replay;
- cookie/session exfiltration;
- hidden/private consumer API reverse engineering as the supported path;
- stealth or anti-bot bypass;
- uncontrolled background crawling;
- fields or UI semantics that misrepresent a local capture as authoritative native Saved state.

Any future experimental local connector requires a separate ADR, security model, and explicit scope; it cannot weaken the default service rules.

## Security and privacy

- Encrypt provider credentials inside this service.
- Enforce internal-user ownership on accounts, captures, exports, and source access.
- Treat posts, URLs, embed HTML, archives, media, and filenames as hostile input.
- Sanitize rendered content and prohibit script execution.
- Do not log private post bodies, user notes, raw archives, or tokens by default.
- Restrict raw blob/export access and retention.
- Redact provider errors before user display.
- Audit external writes and credential changes.
- Use least-privilege database, network, and storage roles.

## Observability

Required telemetry should cover:

- connection/capability/reauth state without secrets;
- captures accepted, deduplicated, resolved, unavailable, and failed;
- account sync pages/items/completeness;
- public resolver latency/failure class;
- relation resolution counts;
- Data Export counts, warnings, unknown categories, and completeness;
- media metadata/archive state;
- provider writes and uncertain-result reconciliation;
- outbox/inbox lag and duplicates;
- correlation, account, source, capture, and import-run IDs in non-sensitive form.

Avoid usernames, post text, and full URLs as ordinary metric labels.

## Testing expectations

When implementation exists, include applicable tests for:

- OAuth state/scope/refresh/revoke and token redaction;
- provider ID/permalink normalization;
- explicit capture idempotency/provenance;
- saved-authority semantics;
- public resolver/oEmbed schema, sanitization, cache, and unavailable responses;
- reply/quote relation handling;
- private-content refusal and user-upload separation;
- hostile/versioned/restartable Data Export import;
- unknown export categories and absence-without-deletion;
- provider write idempotency and uncertain results;
- availability/tombstone states;
- outbox/inbox replay and migrations.

Use synthetic/redacted fixtures. Do not depend on a live personal Threads account in normal tests.

## Cross-repository change rules

Use a workspace changeset when changing:

- social/event contracts;
- capture APIs used by Platform, mobile, browser extension, web, or Telegram;
- Knowledge analysis inputs;
- linked-article extraction requests;
- OAuth/callback/scopes;
- media/BlobStore contracts;
- Data Export completeness semantics;
- deployment secrets or migration/cutover behavior.

List producer/consumer compatibility, rollout, rollback, privacy, reprocessing/reindexing, and user-visible authority impact.

## Git and PR workflow

- State the affected lane: official account, capture, public resolution, relations, media, Data Export, or provider writes.
- Keep authority/provenance changes separate from unrelated refactors.
- Include safe provider/import fixtures and tests.
- Document scopes, external writes, raw data, storage, and retention impact.
- Do not add login/cookie scraping.
- Do not commit credentials, personal exports, private media, or real user notes.
- Do not claim native Saved synchronization without authoritative supported evidence.
- Update README/ADRs when provider capability or product semantics change.

## Completion criteria

A task is complete only when:

- responsibility belongs to the Threads bounded context;
- official account and explicit capture lanes remain separate;
- acquisition method and saved authority are explicit and honest;
- no browser-session/password/cookie automation is introduced;
- relations and provider identity remain normalized;
- private/unavailable content is handled without bypass;
- Data Export import is raw-first, safe, versioned, idempotent, and completeness-aware;
- provider writes require separate scope, intent, idempotency, and audit;
- normalized events preserve provenance;
- relevant security/import/resolution tests pass;
- contracts, migrations, telemetry, and cross-repository rollout are documented.
