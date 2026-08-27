# Threads connector testing strategy

Required tests:

- OAuth binding, credentials, refresh/revoke, scopes, capability drift, and write consent.
- Permalink classification/canonicalization and malicious URLs.
- Post/reply/quote/repost/thread-root normalization, missing nodes, duplicate edges, and cycles.
- Explicit capture idempotency/provenance and public/private/deleted/unsupported resolution.
- Safe Data Export import: schema versions, zip/path/decompression limits, unknown records, duplicates, partial assets.
- Optional publishing/reply idempotency and error/audit matrix.
- Missing-data versus deletion semantics, privacy deletion, schema initialization, outbox/inbox replay, no-secret/content logging.
- Planned workspace capture -> Threads -> Knowledge flow.

Fixtures are synthetic or authorized; no personal account is required in CI.

## Test-first

A change is planned before it is built, and the plan is a task list in which behaviour arrives in
pairs: one task adds a failing test, the next makes it pass. `openspec/config.yaml` carries that
rule, which is what puts it into every planning and implementation request rather than only into this
document.

The loop:

1. Write the test the scenario names. Run it. Confirm it fails, and read the failure — a test that
   fails because it does not compile has proved nothing about the behaviour.
2. Write the smallest change that makes it pass. Run it again.
3. Refactor only once it is green, adding no test and changing no behaviour.

Two checks stand behind this, and neither of them can see the order:

- `openspec validate --archived`, in `.github/workflows/openspec.yml`, fails when a change was
  archived with a task left unticked.
- A step in `.github/workflows/fleet.yml` fails when this repository holds a manifest and a `ci.yml`
  that never runs a test.

`ratatoskr-workspace/docs/QUALITY_GATES.md` records why the order itself is not checkable, rather
than leaving the gap to be discovered.

## Item 9 suites

- `media_retention`: default metadata-only, full eligibility matrix, verified promotion, shared
  references, and retryable digest-bound deletion.
- `privacy_deletion` plus `social_publishing`: exact storage enumeration, owner refusal, preview/apply
  fidelity, duplicate/final/connection behavior, replay, and late Knowledge completion guard.
- `re_resolution_jobs`: deterministic due selection, every finite pre-I/O budget, unchanged refresh,
  and deletion-between-selection-and-claim.
- `data_export_reprocessing` and service `reprocess_export`: receipt/parser refusal, dry-run/apply
  fidelity and zero mutation, resume/replay, omission preservation, stdout/stderr/exits/broken pipe.

Fixtures contain no credentials, personal exports, real URLs with private content, or user notes.
Passing them is local synthetic evidence, not proof against a real protected export. Compiler-backed
tests and the full workspace gate run through `build-gate` on development Macs.
