# Design: Scaffold the ratatoskr-threads service

## Context

The repository holds documents only. The closest sibling, `ratatoskr-instagram`, archived the identical scaffold change (`2026-08-25-scaffold-instagram-service`) against the same toolchain pin and fleet gates one day earlier, so a proven reference tree exists at `../instagram` for every artifact this change creates. See proposal.md for motivation; the two delta specs define the behaviour contracts.

## Goals / Non-Goals

**Goals:**

- A workspace whose layout, manifests, lint configuration and process contract are structurally identical to the Instagram sibling, differing only where the Threads bounded context differs (names, ports, vocabularies, table set).
- First-version `threads_archive` schema that encodes the AGENTS.md authority rules in CHECK constraints, so an explicit capture can never be represented as authoritative native platform state.
- Every spec scenario mapped to a named failing test before its implementation task.

**Non-Goals:**

- No NATS bus, BlobStore client, provider HTTP clients, OAuth, capture intake, resolution, or export import — those are plan items 2-9 and their arrival must not require changing the foundation's public surface.
- No database migrations, no version negotiation, no v2 anything (binding development status).

## Decisions

### D1: Mirror the Instagram sibling file-for-file

Every new file is adapted from `repos/social/instagram` (workspace manifest, `crates/instagram-archive/*`, `services/instagram-archive/*`, tests, `ci.yml`, `compose.yaml`, `deny.toml`). Rationale: the fleet keeps shared files byte-identical by drift check and reviews one shape; a second foundation shape would be a divergence to defend forever. Alternative considered: designing from scratch — rejected because it produces gratuitous differences from seven existing Rust repositories.

### D2: Names and identifiers

- Library crate `crates/threads-archive` (`ratatoskr-threads-archive`), binary crate `services/threads-archive` (`ratatoskr-threads-archive-service`), mirroring the sibling split of domain library vs deployable process.
- Service name constant `ratatoskr-threads`; repository URL `https://github.com/po4yka/ratatoskr-threads`.
- Default admin listener `127.0.0.1:9084` (Instagram uses 9082 for its operator plane and 9083 for its product API, so both adjacent numbers are taken; no other fleet service claims 9084), local compose PostgreSQL on `127.0.0.1:5437` with user/database `threads` (Instagram uses 5436; platform owns 5432).

### D3: Table set follows the AGENTS.md conceptual list

First-version `schema.sql` declares exactly: `accounts`, `credentials`, `posts`, `post_relations`, `media`, `captures`, `capture_resolutions`, `export_runs`, `export_records`, `raw_objects`, `tombstones`, `outbox_events`, `inbox_events` inside `threads_archive`. The README's planned-model list names older shapes (`profiles`, `capture_notes`, `export_snapshots`, `availability_observations`); AGENTS.md's persistence section is the binding in-repo statement of what this context owns, so the schema follows AGENTS.md and README's list is updated to match in the same change. Alternative considered: shipping only tables used by item 1 (none) — rejected: an empty schema proves nothing about vocabulary enforcement and every later item would edit it anyway; dev status makes in-place edits cheap.

### D4: Provenance vocabularies come from AGENTS.md, not the Instagram copy

- `acquisition_method`: `official_api | share_extension | browser_extension | telegram_capture | data_export | legacy_import`.
- `saved_authority`: `explicit_user_capture | export_observation | unknown`.

Unlike Instagram there is deliberately no `authoritative_platform_state` value: Threads exposes no supported native Saved-list surface, and the AGENTS.md rule says represent that state as unknown rather than inventing authority the provider cannot prove. The CHECK constraints make the misrepresentation physically unstorable.

### D5: Schema application stays startup-embedded and advisory-locked

`schema.sql` is `include_str!`ed into the library crate and applied inside one transaction guarded by a PostgreSQL advisory lock keyed on the schema, idempotently, at every boot. No migration ledger exists (development status). This is copied unchanged from the sibling including its rationale comments.

### D6: Configuration remains hand-rolled strict parsing

Finite closed key set under `RATATOSKR__`, every unknown key refused, all violations collected into one value-free report, secrets typed as `SecretString` and redacted in Debug/Serialize. Copied from the sibling rather than replaced with a config crate: the strictness tests are the contract, and no crate provides refusal semantics this exact.

## Risks / Trade-offs

- [Port collision if another service later claims 9084/5437] → `DEPLOYMENT_TARGET.md` in the workspace store is the allocation authority; the defaults are documented here and in DEVELOPMENT.md so a conflict is visible before deployment.
- [Mirroring copies Instagram-specific comments] → each adapted file is read and reworded where it names Instagram concepts; drift between siblings in *shared* files (toolchain, lint thresholds, workflow structure) is intentional and small.
- [Thirteen tables with no consumers yet could drift from later plan items] → acceptable under the no-migrations status: editing `schema.sql` in place is the sanctioned evolution path, and the catalog-exactness test updates with it.
- [Boot test needs Docker for PostgreSQL] → same requirement as every Rust sibling; compose.yaml is documented in DEVELOPMENT.md and the harness fails loudly when the server is absent rather than skipping.

## Migration Plan

Not applicable: greenfield scaffold on an empty database model. Deployment impact begins when the first environment runs the binary; the schema arrives with the process, not ahead of it.

## Open Questions

None. Port numbers, table names, and vocabularies were the only open choices and all are resolved above.
