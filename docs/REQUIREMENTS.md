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
