## 1. Public observation parser

- [x] 1.1 Add `crates/threads-archive/tests/public_resolution.rs` with a compiling skeleton parser and a `parses_supported_public_fixture_deterministically` assertion that fails because the skeleton cannot normalize the fixture's canonical permalink, stable post identity, public metadata, or parser version. Verification: `cargo test --locked -p ratatoskr-threads-archive --test public_resolution parses_supported_public_fixture_deterministically` fails at that assertion.
- [x] 1.2 Implement the strict approved-oEmbed observation parser and public-resolution types in `crates/threads-archive/src/public_resolution.rs`, with bounded validation and no private-session inputs, until `parses_supported_public_fixture_deterministically` passes. Verification: the exact command from 1.1 is green.

## 2. Raw-first revision persistence

- [x] 2.1 Extend `crates/threads-archive/tests/public_resolution.rs` with `re_resolution_appends_immutable_parser_versioned_revisions`, using a store skeleton that compiles and returns a typed not-implemented result, so the assertion that two resolutions retain two raw objects and two revisions fails. Verification: `cargo test --locked -p ratatoskr-threads-archive --test public_resolution re_resolution_appends_immutable_parser_versioned_revisions` fails at that assertion.
- [x] 2.2 Add the in-place `schema.sql` post-revision and explicit-target definition, service-owned immutable raw-object storage, and transactional SQLx resolution store until the revision test passes. Add `reqwest` with Rustls and `sha2` as pinned workspace dependencies because supported HTTPS fetch and SHA-256 BlobRef evidence cannot be provided by the existing dependencies; no migration tooling is added. Verification: the exact command from 2.1 is green.

## 3. Relation graph normalization

- [x] 3.1 Extend `crates/threads-archive/tests/public_resolution.rs` with fixture-thread assertions `stores_directed_reply_and_quote_edges`, `keeps_orphan_relation_explicit`, `rejects_reply_cycles_atomically`, and `normalizes_permuted_relations_deterministically`, against a compiling store skeleton. Verification: run each named test; every one fails because the graph has not been stored yet, not because the test does not compile.
- [x] 3.2 Implement relation graph normalization and deterministic ordered reads: store reply/quote relations as first-class directed rows, retain unresolved target identity, and reject reply cycles in the same transaction. Verification: `cargo test --locked -p ratatoskr-threads-archive --test public_resolution` is green.

## 4. Capability and documentation

- [x] 4.1 Change `crates/threads-archive/tests/capability.rs` so `only_implemented_lanes_claim_support` expects both `ExplicitCapture` and `PublicResolution` to be supported; the assertion must fail while public resolution remains planned. Verification: `cargo test --locked -p ratatoskr-threads-archive --test capability only_implemented_lanes_claim_support` fails at the support-status assertion.
- [x] 4.2 Mark `PublicResolution` supported in `capability.rs` until the 4.1 test passes. Verification: the exact command from 4.1 is green.
- [x] 4.3 Update README.md and DEVELOPMENT.md to state that implementation plan item 4 now provides approved public resolution, immutable revisions, and relation normalization, while items 5 through 9 remain planned. This task cannot start from a failing test because it is documentation. Verification: inspect the final documentation diff and `rg -n 'items 1 through 4|item 4' README.md DEVELOPMENT.md`.

## 5. Full validation

- [x] 5.1 Run the complete docs and product gates from DEVELOPMENT.md on a clean tree: `git diff --check`, `openspec validate --all --strict`, `openspec validate --archived`, `cargo fetch --locked`, `cargo deny --locked check`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`, `cargo test --workspace --locked`, `cargo test --workspace --locked --doc`, and `cargo build --workspace --locked --release`. Verification: every command exits zero.
