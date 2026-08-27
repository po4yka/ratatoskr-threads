# Threads connector requirements

## Goals

1. Connect supported Threads accounts through official OAuth/capabilities.
2. Archive own accessible posts/replies and their relation graph when permitted.
3. Accept explicit captures from mobile/browser clients.
4. Resolve public permalinks through supported mechanisms with provenance.
5. Import user-provided Data Exports safely and support optional separately consented publishing.

## Non-goals

Authoritative native Saved synchronization when no supported endpoint exists, private-content bypass, password/cookie login, hidden API interception, or stealth scraping.

## Requirements

- Acquisition and saved authority are explicit on every record.
- Post, reply, quote, repost, and thread-root relations are versioned and cycle-safe.
- Explicit capture proves Ratatoskr intent, not native Saved membership.
- Private/unavailable content remains explicit status unless user provides an artifact.
- Raw exports/unknown records precede normalization.
- Provider writes require separate capability, consent, idempotency, and audit.

First slice: explicit public permalink -> resolution -> relation-aware SocialSource -> Knowledge indexing -> unavailable fallback.

## Item 9 lifecycle requirements

- Media byte retention defaults to metadata-only and requires affirmative rights, supported
  acquisition/type/MIME, sufficient URL lifetime, known finite sizes, owner budget, and explicit
  action where required.
- Capture/connection deletion must enumerate every owned table/blob class, refuse cross-owner or
  unknown targets without mutation, preserve shared holdings, erase final content and credentials,
  propagate one typed Knowledge removal fact, and replay idempotently.
- Automatic public re-resolution must admit only due live retryable captures and reserve finite
  item/request/byte/deadline/concurrency/provider budgets before I/O.
- Parser reprocessing must verify the retained receipt and exact parser, make dry-run read-only and
  report-identical to apply, preserve omitted/unknown projections, checkpoint apply, and replay.

Normal acceptance uses synthetic/redacted archives. A real protected Threads export remains an
explicit external validation gap and must not be claimed from fixture coverage.
