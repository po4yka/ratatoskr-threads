## Context

See proposal.md. The first-version schema already reserves `export_runs` and `export_records`,
and `RawObjectStore` already proves the local content-addressed immutable-raw pattern. Existing
capture, relation, and social-source persistence establish the owner and provenance vocabulary but
have no export receipt, archive safety boundary, parser, or completeness projection.

This repository stays in development: edit `schema.sql` in place, add no migration tooling,
maintain one API/contract version, and retain no browser-session access. The workspace BlobRef
contract requires service-owned content-addressed bytes and digest verification.

## Goals / Non-Goals

**Goals:**

- Receive one owner-authorized export as immutable evidence before any archive handling.
- Reject hostile or resource-exhausting ZIPs without touching outside the isolated extraction area
  or creating normalized projections.
- Convert one documented synthetic export layout into deterministic post and relation projections,
  retain unknown data raw-first, and compute coverage honestly against local captures.
- Reuse current source-fact vocabulary and relation semantics without inventing an export-specific
  cross-repository contract.

**Non-Goals:**

- Live native Saved synchronization, account-history completeness, deletion inference, browser
  automation, password or cookie collection, media-byte archival, parser reprocessing tooling,
  provider writes, or a migration/compatibility path.

## Decisions

### Receipt is a stream-to-content-addressed archive before inspection

Expose an owner-bound import command that consumes an async byte stream once, incrementally hashes
it, atomically creates the archive under its SHA-256 address, verifies a concurrent winner by
rehashing, and then inserts or returns the unique owner/digest run. The receipt creates a raw
object row and a durable `running` run before inspection; every later result changes only run
state and appends derived evidence.

This avoids buffering untrusted input and makes received evidence independent of parser success.
It is preferred to storing extracted files first, which loses the original artifact and lets a
parser influence what is retained.

### A maintained ZIP reader is constrained by a service-owned inspector

Use a pinned, maintained Rust ZIP reader only after repository-owner authorization for the new
production dependency. Metadata is inspected before extraction; each entry name is normalized and
checked, every declared limit is applied cumulatively, and data is copied into a unique private
directory only after its path and resource budget pass. The extractor creates files with
`create_new`, never follows archive-supplied links, never executes an entry, and destroys only its
known private temporary directory after parsing.

This is preferred to a hand-rolled ZIP decoder, whose format and decompressor correctness are a
security liability. Merely checking `Path::join` is rejected because it does not enforce entry,
depth, decompressed-byte, or ratio limits.

### Parser dispatch is version-first and raw-first

An inspected file inventory identifies one supported export layout and parser version. The parser
uses bounded readers and typed fixture shapes, derives stable provider identity/permalink/text and
relations, sorts all output by stable identity, and produces a staged result before a database
transaction. Each recognized section and unknown section/record receives raw-object evidence;
unknown output is a warning rather than a failed implicit schema guess.

This is preferred to parser heuristics across unknown layouts, which could silently reinterpret
provider data. Unsupported layouts have a failed or warning terminal state with no projection.

### One transaction reconciles derived projections and report

After immutable receipt and safe parsing, one database transaction writes export-record outcomes,
upserts posts by stable provider identity with `data_export` and `export_observation`, persists
relations through the established graph invariant, publishes source facts through the existing
outbox grammar, and writes the terminal report. The report compares sets, not record counts:
export identities, identifiable owner capture identities, their intersection, and each difference;
unresolved captures are named non-comparable. Replaying a completed owner/digest run returns its
prior outcome and does not repeat these writes.

This is preferred to per-section commits because a visible report must correspond to the exact
committed projection. An archive blob may survive a rolled-back derived transaction as harmless
immutable receipt evidence; it does not claim an import completed.

### Schema evolves in place and preserves raw evidence separately

Extend the existing current `schema.sql` run/raw-record vocabulary with only fields and constraints
needed to distinguish receipt, parser, safety failure, terminal outcome, and report. Raw archive
bytes have their own object kind and media type; raw section and unknown-record evidence remain
separate from normalized post revisions. No migration file, version negotiation, or legacy parser
path is introduced.

## Risks / Trade-offs

- [ZIP library or decompressor vulnerability] → pin and audit the dependency before addition,
  use narrow feature flags, enforce limits outside library defaults, and keep the hostile suite.
- [Large uploads exhaust disk before inspection] → bound receipt byte size while streaming and
  abort before an archive is committed beyond the configured receipt budget.
- [ZIP metadata lies about uncompressed size] → count actual emitted bytes during extraction and
  fail closed on a cumulative or per-entry breach.
- [A parser transaction fails after receipt] → retain the immutable archive and failed state, but
  roll back all normalized projections and do not emit a source fact or report.
- [Export content has no stable identity] → retain a raw warning/conflict; do not merge by author
  or text similarity and exclude it from comparable-coverage arithmetic.

## Migration Plan

1. Add the approved pinned archive dependency and edit `schema.sql` in place for fresh
   development databases; do not create a migration.
2. Deploy with bounded receipt and extraction limits configured; accept only the documented parser
   layout.
3. On rollback, stop new receipt calls. Previously stored immutable archives and completed
   observations remain first-version evidence; no schema conversion or deletion occurs.
