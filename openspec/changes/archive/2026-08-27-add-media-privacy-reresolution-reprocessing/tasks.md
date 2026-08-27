## 1. First-Version Lifecycle Schema and Complete Deletion Inventory

- [x] 1.1 RED: extend `crates/threads-archive/tests/schema.rs` with `lifecycle_schema_exposes_policy_deletion_reresolution_and_reprocessing_state`; query a fresh database and confirm the assertion fails because the item-9 tables/columns and closed constraints are absent (add only a minimal compiling seam if needed), then record the expected missing inventory.
- [x] 1.2 GREEN: edit the current `schema.sql` in place to add constrained media-policy/deadline state plus `deletion_operations`, `deletion_effects`, `local_source_removals`, `blob_deletion_tasks`, `reresolution_runs`, `reresolution_items`, `export_reprocessing_runs`, and `export_reprocessing_items`; do not add migrations/tooling/version negotiation, and verify the task 1.1 test passes through `build-gate -- cargo test --locked -p ratatoskr-threads-archive --test schema lifecycle_schema_exposes_policy_deletion_reresolution_and_reprocessing_state`.
- [x] 1.3 RED: add `crates/threads-archive/tests/privacy_deletion.rs::deletion_classifies_every_owned_data_class`; compare the exact authoritative `threads_archive` table inventory plus BlobStore classes with capture and connection maps and confirm the assertion fails naming the unclassified classes, not a compile error.
- [x] 1.4 GREEN: add the closed `OwnedDataClass` inventory and total capture/connection classifications until task 1.3 passes; verify the maps have no duplicate/unknown class and run the whole `privacy_deletion` test target.

## 2. Media Policy and Reference-Safe Blob Retention

- [x] 2.1 RED: add `crates/threads-archive/tests/media_retention.rs::metadata_observation_never_downloads_without_authorized_policy`; use a recording transport and confirm the assertion fails because no explicit metadata-only policy decision exists.
- [x] 2.2 GREEN: implement the pure media-retention decision boundary with metadata-only as the default and verify task 2.1 passes without adding a mocking dependency.
- [x] 2.3 RED: add `crates/threads-archive/tests/media_retention.rs::archival_refuses_before_fetch_when_any_eligibility_or_budget_guard_is_unknown_or_exhausted`; table-drive the closed policy inputs and confirm the expected `MetadataOnly` reasons are missing.
- [x] 2.4 GREEN: enforce acquisition, rights, kind/MIME, URL lifetime, object-size, owner-storage, and explicit-action guards with an immutable fetch lease; verify task 2.3 and all media policy tests pass.
- [x] 2.5 RED: add `crates/threads-archive/tests/media_retention.rs::verified_bytes_are_committed_only_after_https_mime_size_and_digest_checks`; confirm a mismatched response currently lacks the asserted metadata-only result and leaves partial storage.
- [x] 2.6 GREEN: implement bounded streaming and verification through the approved Reqwest/RawObjectStore seams, atomically attach only fully verified provider bytes, preserve user-upload provenance, and verify task 2.5 passes.
- [x] 2.7 RED: add `crates/threads-archive/tests/media_retention.rs::expiring_one_reference_preserves_a_blob_still_referenced_elsewhere`; create two live references to one digest and confirm the current cleanup plan incorrectly lacks a retained-shared classification.
- [x] 2.8 GREEN: implement database-wide live-reference enumeration and digest-verified `delete_if_matches` scheduling so task 2.7 passes without holding a database connection during filesystem I/O.
- [x] 2.9 RED: add `crates/threads-archive/tests/media_retention.rs::failed_blob_delete_stays_pending_and_retries_to_verified_absence`; inject one filesystem deletion failure and confirm the expected pending-then-complete audit transition is absent.
- [x] 2.10 GREEN: implement idempotent BlobStore deletion task processing, safe failure classes, absence verification, and completion state; verify task 2.9 and the full media-retention test target pass.

## 3. Owner Privacy Deletion and Knowledge Removal Propagation

- [x] 3.1 RED: add `crates/threads-archive/tests/privacy_deletion.rs::cross_owner_or_unknown_target_refuses_without_any_mutation`; snapshot database, blob inventory, and outbox, then confirm the expected owner-bound refusal behavior is absent.
- [x] 3.2 GREEN: implement validated capture/connection deletion targets and stable owner-scoped operation identity until task 3.1 passes.
- [x] 3.3 RED: add `crates/threads-archive/tests/privacy_deletion.rs::preview_matches_apply_counts_and_leaves_durable_state_unchanged`; confirm the expected deterministic per-class preview and zero-mutation assertion fail before a planner exists.
- [x] 3.4 GREEN: implement the pure deletion planner and bounded content-free report, make apply recompute the plan under target lock, and verify task 3.3 passes for both target kinds.
- [x] 3.5 RED: add `crates/threads-archive/tests/privacy_deletion.rs::deleting_one_duplicate_capture_preserves_the_shared_source_and_emits_no_removal`; confirm the current archive cannot produce the required retain/detach behavior.
- [x] 3.6 GREEN: implement capture-specific deletion plus cross-capture/cross-lane reference decisions until task 3.5 passes with the other capture, post, raw evidence, media, relations, and source still live.
- [x] 3.7 RED: add `crates/threads-archive/tests/privacy_deletion.rs::deleting_the_final_capture_commits_content_free_audit_and_one_typed_removal`; confirm the asserted atomic deletion counts and `SocialSourceRemoved(UserRequested)` outbox payload are absent.
- [x] 3.8 GREEN: implement final-source cleanup, content-free deletion audit/effects, local removal guard, and transactional removal publication using the pinned contract; verify task 3.7 passes and the payload contains no snapshot/body/note/URL.
- [x] 3.9 RED: add `crates/threads-archive/tests/privacy_deletion.rs::connection_deletion_erases_credentials_but_preserves_an_independent_explicit_capture`; confirm credential/account cleanup and capture authority preservation are not both achieved today.
- [x] 3.10 GREEN: implement connection-scoped credential/budget/checkpoint cleanup and shared post detach/retain logic until task 3.9 passes without changing explicit-capture acquisition or saved authority.
- [x] 3.11 RED: add `crates/threads-archive/tests/privacy_deletion.rs::connection_only_sources_each_emit_one_removal_and_replay_is_idempotent`; confirm the expected per-source event set and single retained operation/audit fail.
- [x] 3.12 GREEN: implement connection-only source enumeration, stable-order removal, per-source outbox facts, and completed-operation replay until task 3.11 passes.
- [x] 3.13 RED: add `crates/threads-archive/tests/social_publishing.rs::late_knowledge_completion_cannot_resurrect_a_locally_removed_source`; confirm a matching late completion is currently linked instead of skipped.
- [x] 3.14 GREEN: make Knowledge completion intake consult the local-removal guard and record late/replayed completions safely; verify task 3.13 and the full privacy-deletion/social-publishing targets pass.

## 4. Budgeted Public Re-Resolution

- [x] 4.1 RED: add `crates/threads-archive/tests/re_resolution_jobs.rs::selection_admits_only_due_live_transient_or_resolved_captures`; include resolved, transient failure, private, deleted, unsupported, not-due, and locally removed fixtures and confirm the eligible/skipped sets differ from the expected deterministic order.
- [x] 4.2 GREEN: implement due-policy state and deterministic eligibility selection without network I/O until task 4.1 passes.
- [x] 4.3 RED: add `crates/threads-archive/tests/re_resolution_jobs.rs::request_never_starts_when_any_run_or_provider_budget_guard_is_exhausted`; table-drive zero/exhausted item, request, byte, deadline, concurrency, and endpoint allowance, and confirm the recording resolver observes an unexpected call or missing skip class.
- [x] 4.4 GREEN: implement transactional claim/recheck and pre-I/O reservation for every finite run/provider guard, with no database connection held across HTTP; verify task 4.3 passes and observed counters never exceed their limits.
- [x] 4.5 RED: add `crates/threads-archive/tests/re_resolution_jobs.rs::unchanged_refresh_appends_evidence_without_duplicate_update`; confirm a second equal observation currently produces the wrong run/event accounting.
- [x] 4.6 GREEN: route accepted work through existing public-resolution/publishing semantics, classify unchanged versus updated, and verify task 4.5 passes.
- [x] 4.7 RED: add `crates/threads-archive/tests/re_resolution_jobs.rs::deletion_between_selection_and_claim_prevents_request_and_resurrection`; delete a selected candidate before admission and confirm the expected zero-call/zero-revision/zero-event assertion fails.
- [x] 4.8 GREEN: revalidate ownership, local-removal, policy, and budgets immediately before I/O; preserve prior projections on timeout/malformed/unavailable outcomes, and verify task 4.7 plus the full re-resolution target pass.

## 5. Parser-Version Export Reprocessing and Dry-Run Tooling

- [x] 5.1 RED: add `crates/threads-archive/tests/data_export_reprocessing.rs::reprocessing_refuses_tampered_receipts_and_unsupported_parser_versions_before_projection`; confirm the expected integrity/parser refusals and zero mutation are unavailable.
- [x] 5.2 GREEN: split receipt verification and introduce an explicit `(detected export version, parser version)` registry without fallback; verify task 5.1 passes using synthetic/redacted archives only.
- [x] 5.3 RED: add `crates/threads-archive/tests/data_export_reprocessing.rs::migration_dry_run_matches_apply_report_for_unchanged_state`; assert equal ordered classifications, counts, warnings, conflicts, completeness evidence, and prospective/applied digests apart from run identity/timestamps, and confirm the assertion fails because no shared plan exists.
- [x] 5.4 GREEN: implement the pure deterministic `ReprocessPlan`, canonical plan/state fingerprints, and report rendering shared by dry-run and apply until task 5.3 passes.
- [x] 5.5 RED: add `crates/threads-archive/tests/data_export_reprocessing.rs::dry_run_does_not_change_database_blob_outbox_or_checkpoint_state`; include normalized, unknown, and conflicting records and confirm the zero-durable-mutation contract fails or is unimplemented.
- [x] 5.6 GREEN: keep dry-run entirely read-only and content-safe, returning no bodies/notes/credentials/raw bytes/private paths; verify task 5.5 passes.
- [x] 5.7 RED: add `crates/threads-archive/tests/data_export_reprocessing.rs::apply_resumes_after_committed_checkpoint_and_completed_replay_adds_nothing`; inject interruption after one committed item and confirm the resumed/fresh reports or row/event counts differ.
- [x] 5.8 GREEN: implement bounded deterministic reprocessing chunks, transactional checkpoint/effect commits, plan-precondition validation, resume, and completed replay until task 5.7 passes.
- [x] 5.9 RED: add `crates/threads-archive/tests/data_export_reprocessing.rs::parser_omission_never_deletes_existing_capture_source_or_media`; confirm a deliberately omitted synthetic category is not yet reported with preserved state.
- [x] 5.10 GREEN: preserve prior projections and unknown evidence on parser omission, keep every derived record at `export_observation`, and verify task 5.9 plus the full Data Export targets pass.
- [x] 5.11 RED: add `services/threads-archive/tests/reprocess_export.rs::process_contract_separates_json_stdout_diagnostics_and_exit_codes`; exercise valid dry-run/apply, invalid grammar, bad configuration, operational failure, and broken pipe, and confirm the expected one-document stdout/status contract fails before the command exists.
- [x] 5.12 GREEN: extend the existing dependency-free command grammar with explicit `reprocess-export dry-run|apply` modes, stable required arguments, newline-terminated canonical JSON stdout, stderr-only diagnostics, and documented `0/1/2/78` exits; verify task 5.11 passes.

## 6. Telemetry, Documentation, and Full Gate

- [x] 6.1 RED: add `crates/threads-archive/tests/telemetry.rs::lifecycle_metrics_cover_bounded_outcomes_without_sensitive_labels`; confirm deletion/blob/re-resolution/reprocessing counters and durations are absent or expose a prohibited high-cardinality/content label.
- [x] 6.2 GREEN: add bounded operation/outcome telemetry for media admission, deletion phases, pending blob work, re-resolution budget skips/results, and reprocessing dry-run/apply/resume; verify task 6.1 and all telemetry tests pass without usernames, full URLs, post text, notes, credentials, or raw error bodies.
- [x] 6.3 Update `README.md`, `DEVELOPMENT.md`, `docs/{ARCHITECTURE,CAPABILITY_MATRIX,DATA_MODEL,DOMAIN,INTERFACES,REQUIREMENTS,TESTING,THREAT_MODEL}.md`, and the implementation-plan status for item 9; this cannot start from a failing behavior test because it is documentation, so verify `rg` shows media/privacy/re-resolution/reprocessing, producer/consumer compatibility, rollout/rollback, retention, and real-export limitations without any migration or later-major claim.
- [x] 6.4 Run targeted crate/service tests, then the exact full documented gate through the machine-wide build gate where compiler-backed: `git diff --check`, `openspec validate --all --strict`, `openspec validate --archived`, `build-gate -- cargo fetch --locked`, `build-gate -- cargo deny --locked check`, `cargo fmt --all -- --check`, `build-gate -- cargo clippy --workspace --all-targets --locked -- -D warnings`, `build-gate -- cargo build --workspace --locked`, `build-gate -- cargo test --workspace --locked`, `build-gate -- cargo test --workspace --locked --doc`, and `build-gate -- cargo build --workspace --locked --release`; record exact pass/blocker evidence.
- [x] 6.5 Review the final diff for scope, authority/provenance, privacy, shared-reference safety, no credential/content logging, no hidden/private API, no migration/tooling/version drift, stale generated artifacts, and every call site; verify `git status --short` contains only the intended item-9/OpenSpec changes.
