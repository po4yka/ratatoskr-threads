-- The Threads Archive database, in one file.
--
-- `ratatoskr-threads-archive` applies this at startup, to a fresh database. There is no migration
-- ledger and no incremental history: no database holds data that has to survive a schema change. A
-- schema change edits this file in place; the next fresh database has it.
--
-- One schema: `threads_archive` — everything the Threads bounded context owns. The table
-- inventory follows AGENTS.md's persistence vocabulary, which is the binding in-repo statement of
-- what this context owns.
--
-- Conventions, applied uniformly and stated once here:
--
--   * Identifiers are UUIDv7 minted by the application, never by the database. A database default
--     would produce v4, so there is deliberately no DEFAULT on any id column: a missing id is an
--     insert error rather than a silently wrong version.
--
--   * Closed vocabularies are `text` with a named CHECK, not a PostgreSQL enum: adding a value to
--     an enum cannot run inside one transaction and removing one is a table rewrite; a CHECK is
--     altered by one statement.
--
--   * Every timestamp is `timestamptz`. `timestamp` would silently record the server's local time.
--
--   * Hashes are stored in `bytea` and the column is named `*_hash`. No column here holds a
--     credential in a readable form: token material lives only as ciphertext produced inside this
--     bounded context (SECURITY.md).
--
--   * References to identifiers owned by other services (`*_ref`) or other schemas are plain uuid
--     columns with no REFERENCES clause. No foreign key crosses the schema boundary.
create schema threads_archive;

comment on schema threads_archive is
    'State owned exclusively by ratatoskr-threads. Accounts, own posts and replies, captures, '
    'public resolutions, Data Export imports, raw evidence, tombstones, and the event machinery.';

-- ---------------------------------------------------------------------------------------------
-- accounts
-- ---------------------------------------------------------------------------------------------
--
-- A connected Threads account. One row per provider account; the provider identity is stable while
-- usernames and display attributes are mutable display data.

create table threads_archive.accounts (
    account_id          uuid        primary key,
    user_ref            uuid        not null,
    provider_account_id text        not null,
    username            text        not null,
    account_type        text        not null,
    connection_status   text        not null,
    scopes              text        not null,
    connected_at        timestamptz not null,
    updated_at          timestamptz not null default now(),
    constraint accounts_provider_account_id_key unique (provider_account_id),
    constraint accounts_account_type_check
        check (account_type in ('personal', 'creator', 'business')),
    constraint accounts_connection_status_check
        check (connection_status in ('connected', 'reauthorization_required', 'revoked'))
);

comment on table threads_archive.accounts is
    'Connected Threads accounts. user_ref names the Ratatoskr owner and crosses no schema.';

-- ---------------------------------------------------------------------------------------------
-- credentials
-- ---------------------------------------------------------------------------------------------
--
-- Provider token material, encrypted inside this bounded context. No column holds plaintext.

create table threads_archive.credentials (
    credential_id             uuid        primary key,
    account_id                uuid        not null,
    access_token_ciphertext   bytea       not null,
    token_version             integer     not null,
    scopes                    text        not null,
    refresh_token_ciphertext  bytea,
    expires_at                timestamptz,
    rotated_at                timestamptz,
    created_at                timestamptz not null default now(),
    constraint credentials_account_id_fkey foreign key (account_id)
        references threads_archive.accounts (account_id)
);

comment on table threads_archive.credentials is
    'Encrypted OAuth token material for one account. Ciphertext only, versioned for rotation.';

-- Non-secret credential lifecycle evidence. Revocation deletes the matching credential row but
-- retains this event so operators can explain the account state without retaining token material.
create table threads_archive.credential_audit (
    audit_id    uuid        primary key,
    account_id  uuid        not null,
    event_kind  text        not null,
    occurred_at timestamptz not null default now(),
    constraint credential_audit_account_id_fkey foreign key (account_id)
        references threads_archive.accounts (account_id),
    constraint credential_audit_event_kind_check
        check (event_kind in ('connected', 'refreshed', 'revoked', 'reauthorization_required'))
);

comment on table threads_archive.credential_audit is
    'Non-secret official OAuth lifecycle evidence. Never contains token values or ciphertext.';

create table threads_archive.account_budgets (
    account_id     uuid        not null,
    endpoint_class text        not null,
    remaining      integer     not null,
    resets_at      timestamptz,
    request_id     text,
    observed_at    timestamptz not null default now(),
    primary key (account_id, endpoint_class),
    constraint account_budgets_account_id_fkey foreign key (account_id)
        references threads_archive.accounts (account_id),
    constraint account_budgets_endpoint_class_check check (length(endpoint_class) between 1 and 64),
    constraint account_budgets_remaining_check check (remaining >= 0),
    constraint account_budgets_request_id_check check (request_id is null or length(request_id) <= 256)
);

comment on table threads_archive.account_budgets is
    'Non-secret official API budget observations; budget state never authorizes a product capability.';

create table threads_archive.account_sync_checkpoints (
    account_id  uuid primary key,
    watermark   text,
    updated_at  timestamptz not null default now(),
    constraint account_sync_checkpoints_account_id_fkey foreign key (account_id)
        references threads_archive.accounts (account_id)
);

comment on table threads_archive.account_sync_checkpoints is
    'The last completed opaque official own-content scan watermark per account; partial scans never update it.';

-- ---------------------------------------------------------------------------------------------
-- raw_objects
-- ---------------------------------------------------------------------------------------------
--
-- Raw evidence before normalization: resolver responses, API payloads, export sections, unknown
-- records preserved for future parser versions, and separately-provenanced user uploads.

create table threads_archive.raw_objects (
    raw_object_id uuid        primary key,
    object_kind   text        not null,
    blob_ref      text        not null,
    content_hash  bytea       not null,
    byte_size     bigint      not null,
    media_type    text        not null,
    observed_at   timestamptz not null,
    constraint raw_objects_object_kind_check
        check (object_kind in
            ('oembed_response', 'api_response', 'export_archive', 'export_section', 'unknown_export_record',
             'user_upload'))
);

comment on table threads_archive.raw_objects is
    'Content-addressed raw evidence. Bodies live in the BlobStore; rows reference them.';

-- ---------------------------------------------------------------------------------------------
-- posts
-- ---------------------------------------------------------------------------------------------
--
-- Normalized provider posts: own posts and replies from the official lane or public posts reached
-- through supported resolution. Provenance is mandatory on every row.

create table threads_archive.posts (
    post_id            uuid        primary key,
    account_id         uuid,
    provider_post_id   text,
    permalink          text        not null,
    post_kind          text        not null,
    text_content       text,
    published_at       timestamptz,
    edited_at          timestamptz,
    acquisition_method text        not null,
    saved_authority    text        not null,
    upstream_status    text        not null,
    created_at         timestamptz not null default now(),
    updated_at         timestamptz not null default now(),
    constraint posts_provider_post_id_key unique (provider_post_id),
    constraint posts_permalink_key unique (permalink),
    constraint posts_account_id_fkey foreign key (account_id)
        references threads_archive.accounts (account_id),
    constraint posts_post_kind_check
        check (post_kind in ('post', 'reply', 'repost', 'quote')),
    constraint posts_acquisition_method_check
        check (acquisition_method in
            ('official_api', 'share_extension', 'browser_extension', 'telegram_capture',
             'public_resolution', 'data_export', 'legacy_import')),
    constraint posts_saved_authority_check
        check (saved_authority in
            ('explicit_user_capture', 'export_observation', 'authoritative_platform_state',
             'legacy_observation')),
    constraint posts_upstream_status_check
        check (upstream_status in
            ('active', 'deleted', 'private_or_inaccessible', 'author_unavailable',
             'temporarily_unavailable', 'unknown'))
);

comment on table threads_archive.posts is
    'Normalized post sources with mandatory acquisition and saved-authority provenance.';

create table threads_archive.post_revisions (
    revision_id    uuid        primary key,
    post_id        uuid        not null,
    raw_object_id  uuid        not null,
    parser_version text        not null,
    observed_at    timestamptz not null,
    constraint post_revisions_post_id_fkey foreign key (post_id)
        references threads_archive.posts (post_id),
    constraint post_revisions_raw_object_id_fkey foreign key (raw_object_id)
        references threads_archive.raw_objects (raw_object_id)
);

comment on table threads_archive.post_revisions is
    'Append-only normalized projections of immutable public/provider raw evidence.';

comment on constraint posts_acquisition_method_check on threads_archive.posts is
    'How this record was obtained. Closed vocabulary; never silently upgraded. The values equal '
    'the published social-contract grammar plus telegram_capture, the documented Threads '
    'capture-client lane.';
comment on constraint posts_saved_authority_check on threads_archive.posts is
    'What the acquisition proves about saved state, equal to the published social-contract '
    'grammar value for value: explicit_user_capture and public_resolution records may never '
    'exceed explicit_user_capture; official_api own-account records may carry '
    'authoritative_platform_state; exports prove export_observation; monolith migrations prove '
    'legacy_observation. Threads exposes no native Saved surface, so no value asserts '
    'membership in one.';

-- ---------------------------------------------------------------------------------------------
-- post_relations
-- ---------------------------------------------------------------------------------------------

create table threads_archive.post_relations (
    relation_id              uuid primary key,
    referencing_post_id      uuid not null,
    target_post_id           uuid,
    target_provider_post_id  text not null,
    target_permalink         text,
    relation_kind            text not null,
    constraint post_relations_referencing_target_kind_key
        unique (referencing_post_id, target_provider_post_id, relation_kind),
    constraint post_relations_referencing_post_id_fkey foreign key (referencing_post_id)
        references threads_archive.posts (post_id),
    constraint post_relations_target_post_id_fkey foreign key (target_post_id)
        references threads_archive.posts (post_id),
    constraint post_relations_relation_kind_check
        check (relation_kind ~ '^[a-z][a-z0-9_]{0,31}$')
);

comment on table threads_archive.post_relations is
    'Directed reply, quote, and repost edges from referencing post to stable target provider identity. '
    'An unavailable target remains an explicit relation without an invented post.';

comment on constraint post_relations_relation_kind_check on threads_archive.post_relations is
    'The published social-contract relation-kind grammar: lowercase letters, digits, and '
    'underscores, starting with a letter, at most 32 characters. reply, quote, and repost are '
    'the kinds modelled today; a well-formed provider edge kind beyond them is preserved '
    'losslessly instead of being refused or misfiled.';

-- ---------------------------------------------------------------------------------------------
-- media
-- ---------------------------------------------------------------------------------------------
--
-- Media metadata attached to one post. Media-byte archival is a separate capability: media_state
-- states honestly whether only metadata or complete bytes are held.

create table threads_archive.media (
    media_id          uuid        primary key,
    post_id           uuid        not null,
    provider_media_id text,
    media_kind        text        not null,
    ordinal           integer     not null default 0,
    mime_type         text,
    width_px          integer,
    height_px         integer,
    duration_ms       bigint,
    blob_ref          text,
    content_hash      bytea,
    byte_size         bigint,
    media_state       text        not null,
    retention_class   text        not null default 'metadata_only',
    retention_deadline timestamptz,
    retention_reason  text,
    observed_at       timestamptz not null,
    constraint media_post_ordinal_key unique (post_id, ordinal),
    constraint media_post_id_fkey foreign key (post_id)
        references threads_archive.posts (post_id),
    constraint media_media_kind_check
        check (media_kind in ('image', 'video', 'carousel')),
    constraint media_media_state_check
        check (media_state in ('metadata_only', 'bytes_archived')),
    constraint media_retention_class_check
        check (retention_class in ('metadata_only', 'explicit_archive', 'policy_archive')),
    constraint media_retention_reason_check
        check (retention_reason is null or length(retention_reason) between 1 and 64),
    constraint media_archived_evidence_check check (
        (media_state = 'metadata_only' and blob_ref is null and content_hash is null
            and byte_size is null and retention_class = 'metadata_only')
        or
        (media_state = 'bytes_archived' and blob_ref is not null and content_hash is not null
            and byte_size is not null and byte_size >= 0
            and retention_class in ('explicit_archive', 'policy_archive'))
    )
);

comment on table threads_archive.media is
    'Per-post media metadata. bytes_archived claims completeness of bytes, never provider authority.';

-- ---------------------------------------------------------------------------------------------
-- captures
-- ---------------------------------------------------------------------------------------------
--
-- An explicit Ratatoskr capture proves the user saved an item TO Ratatoskr at captured_at. It does
-- not prove membership in any native list — Threads exposes none. The saved-authority CHECK makes
-- the misrepresentation physically unstorable.

create table threads_archive.captures (
    capture_id         uuid        primary key,
    user_ref           uuid        not null,
    post_id            uuid,
    idempotency_key    text        not null,
    canonical_url      text        not null,
    original_url       text        not null,
    acquisition_method text        not null,
    saved_authority    text        not null,
    client_source      text        not null,
    status             text        not null,
    note               text,
    captured_at        timestamptz not null,
    next_resolution_at timestamptz,
    created_at         timestamptz not null default now(),
    constraint captures_post_id_fkey foreign key (post_id)
        references threads_archive.posts (post_id),
    constraint captures_user_ref_idempotency_key_key unique (user_ref, idempotency_key),
    constraint captures_acquisition_method_check
        check (acquisition_method in
            ('official_api', 'share_extension', 'browser_extension', 'telegram_capture',
             'public_resolution', 'data_export', 'legacy_import')),
    constraint captures_saved_authority_check
        check (saved_authority in
            ('explicit_user_capture', 'export_observation', 'authoritative_platform_state',
             'legacy_observation')),
    constraint captures_client_source_check
        check (client_source in
            ('ios_share_extension', 'android_share_target', 'browser_extension', 'telegram')),
    constraint captures_status_check
        check (status in ('accepted', 'resolved', 'unavailable', 'failed'))
);

comment on table threads_archive.captures is
    'Explicit user captures. post_id stays open while the item is unresolved or unavailable. '
    'original_url preserves the submitted URL text byte-for-byte beside the canonical permalink.';

comment on constraint captures_acquisition_method_check on threads_archive.captures is
    'How the capture reached this service. Closed vocabulary; enforced by the database. The '
    'values equal the published social-contract grammar plus telegram_capture, the documented '
    'Threads capture-client lane.';
comment on constraint captures_saved_authority_check on threads_archive.captures is
    'The authority the capture proves, equal to the published social-contract grammar value for '
    'value. ExplicitUserCapture is the honest ceiling for a share; no value here may claim '
    'membership in a native platform Saved list.';

-- ---------------------------------------------------------------------------------------------
-- capture_resolutions
-- ---------------------------------------------------------------------------------------------
--
-- One row per supported public-resolution attempt against a capture. A failed or partial
-- resolution is stored truthfully; it never deletes or downgrades the capture itself.

create table threads_archive.capture_resolutions (
    resolution_id    uuid        primary key,
    capture_id       uuid        not null,
    outcome          text        not null,
    resolver_version text,
    raw_object_id    uuid,
    observed_at      timestamptz not null,
    constraint capture_resolutions_capture_id_fkey foreign key (capture_id)
        references threads_archive.captures (capture_id),
    constraint capture_resolutions_raw_object_id_fkey foreign key (raw_object_id)
        references threads_archive.raw_objects (raw_object_id),
    constraint capture_resolutions_outcome_check
        check (outcome in ('resolved', 'partial', 'unavailable', 'resolver_failed'))
);

comment on table threads_archive.capture_resolutions is
    'Public resolver/oEmbed observations per capture, with the raw response kept as evidence.';

-- ---------------------------------------------------------------------------------------------
-- social-source projections
-- ---------------------------------------------------------------------------------------------
--
-- One tenant can hold a provider post through several capture intents. The
-- projection keeps that library identity stable while its revisions are
-- append-only, allowing the outbox and Knowledge linkage to name exact facts.

create table threads_archive.social_sources (
    social_source_id uuid        primary key,
    user_ref         uuid        not null,
    post_id          uuid        not null,
    first_capture_id uuid,
    created_at       timestamptz not null default now(),
    constraint social_sources_user_post_key unique (user_ref, post_id),
    constraint social_sources_post_id_fkey foreign key (post_id)
        references threads_archive.posts (post_id),
    constraint social_sources_first_capture_id_fkey foreign key (first_capture_id)
        references threads_archive.captures (capture_id)
);

comment on table threads_archive.social_sources is
    'Tenant-scoped stable identities for normalized Threads posts published as SocialSource facts; first_capture_id is null for official account observations.';

create table threads_archive.social_source_revisions (
    source_revision_id uuid        primary key,
    social_source_id   uuid        not null,
    content_digest     text        not null,
    snapshot           jsonb       not null,
    observed_at        timestamptz not null,
    constraint social_source_revisions_source_digest_key
        unique (social_source_id, content_digest),
    constraint social_source_revisions_social_source_id_fkey foreign key (social_source_id)
        references threads_archive.social_sources (social_source_id)
);

comment on table threads_archive.social_source_revisions is
    'Immutable published normalized source revisions, keyed by the digest Knowledge uses for linkage.';

create table threads_archive.social_analysis_links (
    completion_event_id uuid        primary key,
    user_ref            uuid        not null,
    social_source_id    uuid        not null,
    content_digest      text        not null,
    completed_at        timestamptz not null,
    linked_at           timestamptz not null default now(),
    constraint social_analysis_links_social_source_id_fkey foreign key (social_source_id)
        references threads_archive.social_sources (social_source_id),
    constraint social_analysis_links_source_digest_fkey foreign key (social_source_id, content_digest)
        references threads_archive.social_source_revisions (social_source_id, content_digest)
);

comment on table threads_archive.social_analysis_links is
    'Privacy-safe Knowledge completion linkage for an exact published source revision; no result body or Knowledge run identity is stored.';

-- ---------------------------------------------------------------------------------------------
-- export_runs
-- ---------------------------------------------------------------------------------------------
--
-- One immutable Data Export archive per run: its hash, its BlobStore reference, and who parsed it.
-- Absence of a category in one export never proves deletion downstream.

create table threads_archive.export_runs (
    run_id              uuid        primary key,
    user_ref            uuid        not null,
    archive_hash        bytea       not null,
    archive_blob_ref    text        not null,
    archive_byte_size   bigint      not null,
    detected_version    text,
    parser_version      text        not null,
    outcome             text        not null,
    records_processed   bigint      not null default 0,
    warnings_summary    text,
    completeness_report jsonb,
    started_at          timestamptz not null default now(),
    finished_at         timestamptz,
    constraint export_runs_user_archive_hash_key unique (user_ref, archive_hash),
    constraint export_runs_outcome_check
        check (outcome in ('running', 'completed', 'completed_with_warnings', 'failed'))
);

comment on table threads_archive.export_runs is
    'One restartable parse/reconcile pass over an imported archive, with its completeness evidence.';

-- ---------------------------------------------------------------------------------------------
-- export_records
-- ---------------------------------------------------------------------------------------------
--
-- Per-record outcomes of an export run: normalized records, unknown records and sections retained
-- losslessly, conflicts left unresolved rather than merged, and warnings.

create table threads_archive.export_records (
    record_id          uuid        primary key,
    run_id             uuid        not null,
    record_kind        text        not null,
    category           text,
    provider_record_id text,
    raw_object_id      uuid,
    payload            jsonb,
    processed_at       timestamptz not null default now(),
    constraint export_records_run_id_fkey foreign key (run_id)
        references threads_archive.export_runs (run_id),
    constraint export_records_raw_object_id_fkey foreign key (raw_object_id)
        references threads_archive.raw_objects (raw_object_id),
    constraint export_records_record_kind_check
        check (record_kind in
            ('normalized', 'unknown_record', 'unknown_section', 'conflict', 'warning'))
);

comment on table threads_archive.export_records is
    'What one export run did per record, including every unknown record it refused to drop.';

-- ---------------------------------------------------------------------------------------------
-- tombstones
-- ---------------------------------------------------------------------------------------------
--
-- Upstream availability over time. Missing partial API/resolver output is not deletion evidence:
-- only an observed tombstone records deleted or unavailable state.

create table threads_archive.tombstones (
    tombstone_id     uuid        primary key,
    post_id          uuid,
    capture_id       uuid,
    availability     text        not null,
    reason_code      text,
    resolver_version text,
    observed_at      timestamptz not null,
    constraint tombstones_post_id_fkey foreign key (post_id)
        references threads_archive.posts (post_id),
    constraint tombstones_capture_id_fkey foreign key (capture_id)
        references threads_archive.captures (capture_id),
    constraint tombstones_subject_check
        check (post_id is not null or capture_id is not null),
    constraint tombstones_availability_check
        check (availability in
            ('active', 'deleted', 'private_or_inaccessible', 'author_unavailable',
             'temporarily_unavailable', 'unknown'))
);

comment on table threads_archive.tombstones is
    'Upstream availability over time. Absence of a newer observation never implies deletion.';

-- ---------------------------------------------------------------------------------------------
-- outbox_events
-- ---------------------------------------------------------------------------------------------

create table threads_archive.outbox_events (
    event_id        uuid        primary key,
    event_type      text        not null,
    aggregate_type  text        not null,
    aggregate_id    uuid        not null,
    payload         jsonb       not null,
    correlation_id  uuid,
    causation_id    uuid,
    occurred_at     timestamptz not null,
    published_at    timestamptz,
    attempt_count   integer     not null default 0,
    next_attempt_at timestamptz,
    constraint outbox_events_aggregate_type_check
        check (aggregate_type in ('capture', 'post', 'account', 'export_run'))
);

comment on table threads_archive.outbox_events is
    'Transactional outbox. Rows become at-least-once publications; replay converges.';

create index outbox_events_unpublished_idx
    on threads_archive.outbox_events (next_attempt_at)
    where published_at is null;

-- ---------------------------------------------------------------------------------------------
-- inbox_events
-- ---------------------------------------------------------------------------------------------

create table threads_archive.inbox_events (
    consumer_name  text        not null,
    event_id       uuid        not null,
    consumed_at    timestamptz not null,
    handler_outcome text       not null,
    constraint inbox_events_consumer_name_event_id_pkey primary key (consumer_name, event_id),
    constraint inbox_events_handler_outcome_check
        check (handler_outcome in ('processed', 'rejected', 'skipped', 'locally_removed'))
);

comment on table threads_archive.inbox_events is
    'Consumer inbox deduplication under at-least-once delivery.';

-- ---------------------------------------------------------------------------------------------
-- item-9 lifecycle operations
-- ---------------------------------------------------------------------------------------------
--
-- These are first-version definitions, not a migration ledger. Deletion audit is deliberately
-- structural: it contains identifiers, closed classes and counts, never content-bearing JSON.

create table threads_archive.deletion_operations (
    operation_id uuid        primary key,
    user_ref     uuid        not null,
    target_kind  text        not null,
    target_id    uuid        not null,
    reason       text        not null,
    state        text        not null,
    requested_at timestamptz not null,
    updated_at   timestamptz not null default now(),
    finished_at  timestamptz,
    constraint deletion_operations_target_check
        check (target_kind in ('capture', 'connection')),
    constraint deletion_operations_reason_check
        check (reason in ('user_requested', 'retention_policy')),
    constraint deletion_operations_state_check
        check (state in ('planned', 'database_committed', 'pending_blob_deletion', 'complete', 'failed')),
    constraint deletion_operations_owner_operation_key unique (user_ref, operation_id)
);

comment on table threads_archive.deletion_operations is
    'Content-free owner deletion audit and idempotency identity; target rows may be physically gone.';

create table threads_archive.deletion_effects (
    operation_id   uuid   not null,
    data_class     text   not null,
    action         text   not null,
    affected_count bigint not null,
    primary key (operation_id, data_class, action),
    constraint deletion_effects_operation_id_fkey foreign key (operation_id)
        references threads_archive.deletion_operations (operation_id),
    constraint deletion_effects_data_class_check
        check (data_class ~ '^[a-z][a-z0-9_]{0,63}$'),
    constraint deletion_effects_action_check
        check (action in ('delete', 'detach', 'retain_audit', 'retain_shared', 'not_applicable')),
    constraint deletion_effects_count_check check (affected_count >= 0)
);

comment on table threads_archive.deletion_effects is
    'Bounded per-class row/blob counts for one deletion; no payload, URL, note, token, or body.';

create table threads_archive.local_source_removals (
    user_ref         uuid        not null,
    social_source_id uuid        not null,
    post_id          uuid        not null,
    operation_id     uuid        not null,
    reason           text        not null,
    removed_at       timestamptz not null,
    primary key (user_ref, social_source_id),
    constraint local_source_removals_operation_id_fkey foreign key (operation_id)
        references threads_archive.deletion_operations (operation_id),
    constraint local_source_removals_reason_check
        check (reason in ('user_requested', 'retention_policy'))
);

comment on table threads_archive.local_source_removals is
    'Content-free local-library removal guard; distinct from provider availability tombstones.';

create table threads_archive.blob_deletion_tasks (
    task_id            uuid        primary key,
    operation_id       uuid,
    blob_ref           text        not null,
    content_hash       bytea       not null,
    state              text        not null,
    attempt_count      integer     not null default 0,
    last_failure_class text,
    created_at         timestamptz not null default now(),
    updated_at         timestamptz not null default now(),
    completed_at       timestamptz,
    constraint blob_deletion_tasks_operation_id_fkey foreign key (operation_id)
        references threads_archive.deletion_operations (operation_id),
    constraint blob_deletion_tasks_blob_key unique (blob_ref, content_hash),
    constraint blob_deletion_tasks_state_check
        check (state in ('pending', 'complete')),
    constraint blob_deletion_tasks_attempt_count_check check (attempt_count >= 0),
    constraint blob_deletion_tasks_failure_class_check
        check (last_failure_class is null or last_failure_class in
            ('storage_unavailable', 'digest_mismatch', 'still_referenced'))
);

comment on table threads_archive.blob_deletion_tasks is
    'Durable idempotent deletion of an unreferenced service-owned content-addressed object.';

create table threads_archive.reresolution_runs (
    run_id               uuid        primary key,
    user_ref             uuid        not null,
    state                text        not null,
    max_items            integer     not null,
    max_requests         integer     not null,
    max_response_bytes   bigint      not null,
    max_concurrency      integer     not null,
    deadline_at          timestamptz not null,
    items_admitted       integer     not null default 0,
    requests_reserved    integer     not null default 0,
    response_bytes       bigint      not null default 0,
    started_at           timestamptz not null default now(),
    finished_at          timestamptz,
    constraint reresolution_runs_state_check
        check (state in ('running', 'completed', 'completed_with_failures', 'failed')),
    constraint reresolution_runs_positive_limits_check check (
        max_items > 0 and max_requests > 0 and max_response_bytes > 0 and max_concurrency > 0
    ),
    constraint reresolution_runs_counters_check check (
        items_admitted >= 0 and items_admitted <= max_items
        and requests_reserved >= 0 and requests_reserved <= max_requests
        and response_bytes >= 0 and response_bytes <= max_response_bytes
    )
);

comment on table threads_archive.reresolution_runs is
    'Finite owner-scoped public re-resolution budget and bounded aggregate outcome.';

create table threads_archive.reresolution_items (
    run_id          uuid        not null,
    capture_id      uuid        not null,
    state           text        not null,
    skip_reason     text,
    attempt_count   integer     not null default 0,
    response_bytes  bigint      not null default 0,
    claimed_at      timestamptz,
    finished_at     timestamptz,
    primary key (run_id, capture_id),
    constraint reresolution_items_run_id_fkey foreign key (run_id)
        references threads_archive.reresolution_runs (run_id),
    constraint reresolution_items_state_check
        check (state in ('candidate', 'claimed', 'updated', 'unchanged', 'skipped', 'failed')),
    constraint reresolution_items_skip_reason_check
        check (skip_reason is null or skip_reason in
            ('not_due', 'not_live', 'privacy_terminal', 'unsupported', 'item_budget',
             'request_budget', 'byte_budget', 'deadline', 'concurrency', 'provider_budget')),
    constraint reresolution_items_counters_check
        check (attempt_count >= 0 and response_bytes >= 0)
);

comment on table threads_archive.reresolution_items is
    'Deterministic candidate and outcome evidence; capture_id remains after local deletion for audit.';

create table threads_archive.export_reprocessing_runs (
    reprocessing_run_id uuid        primary key,
    operation_id       uuid        not null,
    export_run_id      uuid        not null,
    user_ref           uuid        not null,
    detected_version   text        not null,
    parser_version     text        not null,
    state              text        not null,
    plan_fingerprint   text        not null,
    state_fingerprint  text        not null,
    checkpoint_item_key text,
    report             jsonb,
    started_at         timestamptz not null default now(),
    updated_at         timestamptz not null default now(),
    finished_at        timestamptz,
    constraint export_reprocessing_runs_export_run_id_fkey foreign key (export_run_id)
        references threads_archive.export_runs (run_id),
    constraint export_reprocessing_runs_owner_operation_key unique (user_ref, operation_id),
    constraint export_reprocessing_runs_state_check
        check (state in ('running', 'completed', 'completed_with_warnings', 'failed')),
    constraint export_reprocessing_runs_version_check
        check (length(detected_version) between 1 and 128 and length(parser_version) between 1 and 128),
    constraint export_reprocessing_runs_fingerprint_check
        check (length(plan_fingerprint) = 64 and length(state_fingerprint) = 64)
);

comment on table threads_archive.export_reprocessing_runs is
    'Separate parser-version operation over one immutable original export receipt; never a schema migration.';

create table threads_archive.export_reprocessing_items (
    reprocessing_run_id uuid   not null,
    item_key            text   not null,
    classification      text   not null,
    state               text   not null,
    prospective_digest  text,
    applied_digest      text,
    primary key (reprocessing_run_id, item_key),
    constraint export_reprocessing_items_run_id_fkey foreign key (reprocessing_run_id)
        references threads_archive.export_reprocessing_runs (reprocessing_run_id),
    constraint export_reprocessing_items_item_key_check check (length(item_key) between 1 and 512),
    constraint export_reprocessing_items_classification_check
        check (classification in
            ('normalized', 'unknown_record', 'unknown_section', 'conflict', 'warning', 'omitted')),
    constraint export_reprocessing_items_state_check
        check (state in ('planned', 'applied', 'skipped', 'conflict', 'warning')),
    constraint export_reprocessing_items_digest_check check (
        (prospective_digest is null or length(prospective_digest) = 64)
        and (applied_digest is null or length(applied_digest) = 64)
    )
);

comment on table threads_archive.export_reprocessing_items is
    'Content-free deterministic checkpoint and digest outcome for one retained-export item.';
