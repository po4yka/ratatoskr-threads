## 1. Test inputs and approved archive reader

- [x] 1.1 Add redacted synthetic ZIP builders and fixture export JSON in
  `crates/threads-archive/tests/data_export.rs`, including traversal, entry-limit, nested-path,
  compression-ratio, supported-version, unknown-section, and capture-coverage inputs; verify no
  fixture contains a credential, personal export, or native-Saved assertion. No RED applies:
  fixtures only supply deterministic input to following behavior tests.
- [x] 1.2 After repository-owner approval, add a pinned maintained ZIP-reader dependency with only
  needed features to the workspace and update `Cargo.lock`; verify `cargo metadata --locked
  --no-deps` succeeds. No RED applies: dependency declaration is configuration needed to compile
  the following tests.

## 2. Bounded hostile-archive inspection and extraction

- [x] 2.1 Add and run the RED integration test
  `zip_slip_is_refused_before_any_projection` in `crates/threads-archive/tests/data_export.rs`;
  it must compile and fail because a traversal or absolute-path archive is not returned as the
  typed path-safety refusal, or creates a normalized row. Verify with `build-gate -- cargo test
  --locked -p ratatoskr-threads-archive --test data_export zip_slip_is_refused_before_any_projection`.
- [x] 2.2 Implement the narrow owner-bound archive inspector/extractor path normalization and
  failed-run persistence necessary for `zip_slip_is_refused_before_any_projection`, then re-run
  that test green and assert no path escapes the dedicated extraction root.
- [x] 2.3 Add and run the RED integration test
  `entry_and_byte_limits_are_refused_before_parser_projection` in
  `crates/threads-archive/tests/data_export.rs`; it must fail because an excessive entry count or
  decompressed-byte archive is accepted or creates a normalized row. Verify with `build-gate --
  cargo test --locked -p ratatoskr-threads-archive --test data_export
  entry_and_byte_limits_are_refused_before_parser_projection`.
- [x] 2.4 Add cumulative entry-count, compressed/decompressed-byte, and actual-output enforcement,
  then re-run `entry_and_byte_limits_are_refused_before_parser_projection` green with the violated
  limit named in the run warning.
- [x] 2.5 Add and run the RED integration test
  `nesting_and_compression_ratio_limits_are_refused_deterministically` in
  `crates/threads-archive/tests/data_export.rs`; it must fail because an over-nested or
  high-ratio archive does not return its precise limit refusal. Verify with `build-gate -- cargo
  test --locked -p ratatoskr-threads-archive --test data_export
  nesting_and_compression_ratio_limits_are_refused_deterministically`.
- [x] 2.6 Implement path-depth and compression-ratio checks and re-run
  `nesting_and_compression_ratio_limits_are_refused_deterministically` green; then run the whole
  hostile suite with `build-gate -- cargo test --locked -p ratatoskr-threads-archive --test
  data_export`.

## 3. Immutable authenticated receipt and durable import state

- [x] 3.1 Add and run the RED integration test
  `streamed_receipt_hashes_and_retains_the_exact_archive_before_inspection` in
  `crates/threads-archive/tests/data_export.rs`; it must fail because the returned receipt's hash,
  byte length, raw BlobRef, or running run state differs from the supplied chunk stream. Verify
  with `build-gate -- cargo test --locked -p ratatoskr-threads-archive --test data_export
  streamed_receipt_hashes_and_retains_the_exact_archive_before_inspection`.
- [x] 3.2 Extend the current `schema.sql` definition in place for archive raw-object kind, receipt
  metadata, and terminal warning/error evidence, then implement streamed content-addressed receipt
  and durable owner-scoped run creation; re-run
  `streamed_receipt_hashes_and_retains_the_exact_archive_before_inspection` green. No migration
  file or migration tooling may be added.
- [x] 3.3 Add and run the RED integration test
  `same_owner_receipt_replays_without_rewriting_and_owner_boundary_is_preserved` in
  `crates/threads-archive/tests/data_export.rs`; it must fail because same-owner retry creates a
  second run or another owner can converge on the first owner's run. Verify with `build-gate --
  cargo test --locked -p ratatoskr-threads-archive --test data_export
  same_owner_receipt_replays_without_rewriting_and_owner_boundary_is_preserved`.
- [x] 3.4 Implement owner/digest idempotency and concurrent immutable-object verification, then
  re-run `same_owner_receipt_replays_without_rewriting_and_owner_boundary_is_preserved` green.

## 4. Versioned deterministic parser and projections

- [x] 4.1 Add and run the RED integration test
  `supported_fixture_normalizes_deterministic_export_posts_and_relations` in
  `crates/threads-archive/tests/data_export.rs`; it must fail because equivalent reordered fixture
  archives produce unequal ordered post/relation projections, a wrong parser version, or missing
  `data_export`/`export_observation` provenance. Verify with `build-gate -- cargo test --locked
  -p ratatoskr-threads-archive --test data_export
  supported_fixture_normalizes_deterministic_export_posts_and_relations`.
- [x] 4.2 Implement supported-layout detection, version-pinned parser dispatch, sorted normalized
  records, relation staging, and one projection transaction; re-run
  `supported_fixture_normalizes_deterministic_export_posts_and_relations` green.
- [x] 4.3 Add and run the RED integration test
  `unknown_export_section_is_retained_as_raw_evidence_with_warning` in
  `crates/threads-archive/tests/data_export.rs`; it must fail because the unknown section has no
  raw record/warning or parser output asserts native Saved membership. Verify with `build-gate --
  cargo test --locked -p ratatoskr-threads-archive --test data_export
  unknown_export_section_is_retained_as_raw_evidence_with_warning`.
- [x] 4.4 Persist unknown-section and unknown-record raw evidence with warnings, and re-run
  `unknown_export_section_is_retained_as_raw_evidence_with_warning` green without guessing an
  unsupported parser or authority claim.

## 5. Idempotent reconciliation and completeness evidence

- [x] 5.1 Add and run the RED integration test
  `replayed_export_preserves_one_projection_and_absence_never_tombstones_capture` in
  `crates/threads-archive/tests/data_export.rs`; it must fail because replay duplicates a post,
  relation, or source fact, or a capture absent from the fixture is modified/tombstoned. Verify
  with `build-gate -- cargo test --locked -p ratatoskr-threads-archive --test data_export
  replayed_export_preserves_one_projection_and_absence_never_tombstones_capture`.
- [x] 5.2 Reuse stable provider identity, existing relation validation, and existing source-fact
  outbox grammar for Data Export reconciliation, then re-run
  `replayed_export_preserves_one_projection_and_absence_never_tombstones_capture` green.
- [x] 5.3 Add and run the RED integration test
  `completeness_report_counts_overlap_differences_and_non_comparable_captures` in
  `crates/threads-archive/tests/data_export.rs`; it must fail because the report does not count
  two matches, one export-only identity, one capture-only comparable identity, and one
  non-comparable capture from the synthetic fixture, or makes a deletion/native-Saved claim.
  Verify with `build-gate -- cargo test --locked -p ratatoskr-threads-archive --test data_export
  completeness_report_counts_overlap_differences_and_non_comparable_captures`.
- [x] 5.4 Implement set-based owner-scoped completeness calculation and terminal-report persistence,
  then re-run `completeness_report_counts_overlap_differences_and_non_comparable_captures` green.

## 6. Capability, documentation, and gates

- [x] 6.1 Add and run the RED unit test `data_export_is_supported_after_item_eight` in
  `crates/threads-archive/src/capability.rs`; it must fail because `AcquisitionMode::DataExport`
  remains `Planned`. Verify with `build-gate -- cargo test --locked -p
  ratatoskr-threads-archive data_export_is_supported_after_item_eight`.
- [x] 6.2 Mark `DataExport` supported only after the receipt, hostile suite, parser, reconciliation,
  and report paths are implemented, then re-run `data_export_is_supported_after_item_eight` green.
- [x] 6.3 Update `README.md`, `DEVELOPMENT.md`, and relevant architecture/capability documentation
  with receipt limits, parser scope, raw-retention semantics, report meaning, and explicit
  non-capabilities; verify they claim neither native Saved synchronization nor deletion from export
  absence. No RED applies: this records behavior covered by the preceding tests.
- [x] 6.4 Run `git diff --check`, `openspec validate --all --strict`, and `openspec validate
  --archived`; verify all documentation and OpenSpec gates pass.
- [x] 6.5 Run the complete documented product gate through `build-gate --`, including
  PostgreSQL-backed workspace tests, docs tests, lint, deny, debug, and release builds; verify
  every command exits successfully.
- [x] 6.6 Inspect the final diff and tick only tasks with observed verification; the required
  dedicated-worktree commit, `main` integration, remote verification, and cleanup run immediately
  after this OpenSpec archive according to the user-requested delivery procedure. No RED applies:
  this is delivery preparation after all behavior tests are green.
