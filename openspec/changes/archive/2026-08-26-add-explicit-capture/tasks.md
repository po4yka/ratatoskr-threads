## 1. Failing tests: permalink canonicalization

- [x] 1.1 Add `crates/threads-archive/tests/permalink.rs` against a skeleton `src/permalink.rs` whose `canonicalize` always returns a placeholder error, so the failures are assertion-level and not compile errors. Tests named for the scenarios: every row of the documented variant table (apex and www hosts on both provider domains, http with explicit default port, tracking query string plus fragment, trailing slash) yields exactly `https://www.threads.net/@<handle>/post/<code>`; handle case folding converges two spellings to one permalink; post-code case stays verbatim so case-differing codes stay distinct; the result carries the original input byte-for-byte; refusals name their rule — foreign host and subdomains, profile URL, path missing `/post/`, empty handle, empty code, syntactically invalid URL, `/t/<code>` short form unsupported at intake. Verification: `cargo test -p ratatoskr-threads-archive --test permalink --locked` fails on those assertions.

- [x] 1.2 Implement `src/permalink.rs` until green and export it through `lib.rs`: the documented grammar, the single canonical form per D1/D2 of the design, typed errors naming each violated rule, original-input preservation. Verification: `cargo test -p ratatoskr-threads-archive --test permalink --locked` green.

## 2. Failing tests: capture provenance types

- [x] 2.1 Extend `crates/threads-archive/tests/capture.rs` with non-database tests against a skeleton `src/capture.rs`: every documented method/client pairing (`share_extension`+`ios_share_extension`, `share_extension`+`android_share_target`, `browser_extension`+`browser_extension`, `telegram_capture`+`telegram`) builds a validated request; mismatched pairings such as `browser_extension`+`telegram` are refused by an error naming the pairing rule; the record type exposes saved authority as fixed `explicit_user_capture` — no request field can influence it; a request whose URL fails canonicalization is refused before any storage call would be reached. Verification: `cargo test -p ratatoskr-threads-archive --test capture --locked` fails on those assertions while 1.x stays green.

- [x] 2.2 Implement the validated request/record types in `src/capture.rs` until green (validation rules only; persistence arrives with section 3). Verification: `cargo test -p ratatoskr-threads-archive --test capture --locked` green for the non-database tests.

## 3. Failing tests: idempotent submit and truthful fallback

- [x] 3.1 Extend `crates/threads-archive/tests/capture.rs` with database tests using `test_support::TestDatabase` against a store whose methods are stubbed to return "not implemented" errors, so failures are assertion-level: submitting stores one row with pinned authority, requested method/client, canonical permalink, the original URL text byte-for-byte, note, and an acceptance-stamped captured time; an identical replay returns the identical record and still leaves one row; a replay through different raw text that canonicalizes equal converges on the stored record; two keys over one permalink produce two independent rows each with its own captured time; recording a deleted observation writes a tombstone subject = capture with availability `deleted`, a resolution row with outcome `unavailable`, and flips status to `unavailable`; the private observation does the same with availability `private_or_inaccessible`; recording a resolver failure writes outcome `resolver_failed`, writes no tombstone, and leaves status `accepted`; after any fallback the note, captured time, raw URL, and canonical permalink remain unchanged. Verification: `cargo test -p ratatoskr-threads-archive --test capture --locked` fails exactly on these new scenarios.

- [x] 3.2 Make the storage layer real until green: edit `schema.sql` in place adding `captures.original_url text not null`; add pinned workspace `chrono` (default features off) plus the sqlx `chrono` feature so `timestamptz` maps to `DateTime<Utc>`; implement `CaptureStore::submit` (insert-or-converge on `(user_ref, idempotency_key)`) and `CaptureStore::record_observation` (evidence-class mapping per D6, transactional fallback writes). Verification: `cargo test -p ratatoskr-threads-archive --test capture --locked` green. The dependency line itself cannot start from a failing test: configuration.

## 4. Capability matrix flip

- [x] 4.1 Modify `crates/threads-archive/tests/capability.rs`: replace the scenario asserting that no mode reports `Supported` with one asserting exactly `ExplicitCapture` reports `Supported` and the other four report `Planned`. Verification: `cargo test -p ratatoskr-threads-archive --test capability --locked` fails on that new assertion because all modes currently report `Planned`.

- [x] 4.2 Flip `AcquisitionMode::capability` so `ExplicitCapture` reports `SupportStatus::Supported`. Verification: `cargo test -p ratatoskr-threads-archive --test capability --locked` green.

## 5. Documentation consistency

- [x] 5.1 Update README.md status blockquote and project-status section plus DEVELOPMENT.md opening status line: implementation plan item 3 exists (explicit capture intake, permalink canonicalization, idempotent replay, unavailable fallback); items 4 through 9 remain planned. This task cannot start from a failing test: documentation.

- [x] 5.2 Cross-check docs/DOMAIN.md and docs/TESTING.md statements about captures against shipped behavior and fix prose drift; confirm every spec scenario added by this change names the test that executes it. This task cannot start from a failing test: documentation consistency.

## 6. Full gate

- [x] 6.1 Run the complete gate from DEVELOPMENT.md on a clean tree — `git diff --check`, `openspec validate --all --strict`, `openspec validate --archived`, `cargo fetch --locked`, `cargo deny --locked check`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`, `cargo test --workspace --locked`, `cargo test --workspace --locked --doc`, `cargo build --workspace --locked --release` — and record the results. Verification: every command exits zero.
