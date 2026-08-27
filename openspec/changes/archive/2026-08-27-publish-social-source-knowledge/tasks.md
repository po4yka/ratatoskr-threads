## 1. Contract pin and source projection foundation

- [x] 1.1 Add the exact `ratatoskr-social-contracts` Git dependency at
  `9a9cdead0c689b946a52648eb76cc40158bd3c7b` and the required envelope dependency to the
  workspace, update `Cargo.lock`, and verify `cargo metadata --locked --no-deps` resolves both
  packages. This is dependency configuration, so it cannot begin with a behavioural failing test.
- [x] 1.2 Add a compiling RED integration test
  `crates/threads-archive/tests/social_publishing.rs::resolved_capture_appends_contract_conformant_captured_fact`.
  Resolve a synthetic capture and assert that the stored outbox is empty where one typed
  `social.source.captured.v1` envelope with Threads provenance is required; run it with
  `build-gate -- cargo nextest run --locked -p ratatoskr-threads-archive --test social_publishing resolved_capture_appends_contract_conformant_captured_fact`
  and confirm the asserted outbox count fails.
- [x] 1.3 Implement the tenant-scoped source projection, canonical snapshot/digest construction,
  and transactional captured-fact outbox append in the current schema definition until
  `resolved_capture_appends_contract_conformant_captured_fact` passes with an envelope that
  round-trips through the published contract.

## 2. Knowledge completion linkage

- [x] 2.1 Add a compiling RED integration test
  `crates/threads-archive/tests/social_publishing.rs::matching_knowledge_completion_links_once_to_the_exact_source_revision`.
  Deliver a typed `knowledge.analysis.completed.v1` completion for the captured source, assert the
  expected `(owner, social_source_id, content_digest)` linkage is absent, and confirm the
  assertion—not compilation—fails under the focused build-gated nextest command.
- [x] 2.2 Implement inbox event-id deduplication and observational completion-link persistence
  without Knowledge run ids or result bodies until
  `matching_knowledge_completion_links_once_to_the_exact_source_revision` passes for both first
  delivery and redelivery.
- [x] 2.3 Add a compiling RED integration test
  `crates/threads-archive/tests/social_publishing.rs::foreign_or_stale_knowledge_completion_does_not_link`.
  Deliver owner- and digest-mismatched completions, assert no linkage exists, and confirm the
  assertion fails before implementation.
- [x] 2.4 Implement owner and digest matching so
  `foreign_or_stale_knowledge_completion_does_not_link` passes while an older exact-digest
  completion remains retrievable as superseded evidence.

## 3. Tombstone availability propagation

- [x] 3.1 Add a compiling RED integration test
  `crates/threads-archive/tests/social_publishing.rs::post_tombstone_appends_deleted_upstream_update_without_erasing_evidence`.
  Establish a preserved source, record its deletion tombstone, and assert the next fact is missing
  where a typed `social.source.updated.v1` with `deleted_upstream` is required; run the focused
  build-gated nextest command and confirm that assertion fails.
- [x] 3.2 Implement post-tombstone source revision publication so
  `post_tombstone_appends_deleted_upstream_update_without_erasing_evidence` passes, preserving
  the stored post/capture evidence and never emitting `social.source.removed.v1` for a provider
  deletion.
- [x] 3.3 Add a compiling RED integration test
  `crates/threads-archive/tests/social_publishing.rs::unavailable_only_capture_publishes_no_social_source_fact`.
  Record a synthetic unavailable-only capture, assert a social-source fact is present, and confirm
  that deliberately incorrect expectation fails.
- [x] 3.4 Make the unavailable-only publication guard explicit until
  `unavailable_only_capture_publishes_no_social_source_fact` passes without weakening the
  tombstone path for an already preserved post.

## 4. Documentation and full validation

- [x] 4.1 Update `README.md` and `DEVELOPMENT.md` with implemented social-source publishing,
  fact-as-request semantics, completion linkage, and the provider-tombstone distinction; this is
  documentation of already tested behaviour and cannot begin with a failing behaviour test.
- [x] 4.2 Run the full documented gate through `build-gate` where compiler-backed: `cargo fetch
  --locked`, `cargo deny --locked check`, `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`, `cargo test
  --workspace --locked`, `cargo test --workspace --locked --doc`, and `cargo build --workspace
  --locked --release`; also run `git diff --check`, `openspec validate --all --strict`, and
  `openspec validate --archived`. Verify all named commands pass before marking this task done.
