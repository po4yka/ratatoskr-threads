# Threads connector domain model

## Terms

- **Account connection:** identity, credentials, scopes, capabilities, status.
- **Post:** provider content node.
- **Relation:** reply-to, thread-root, quote, repost, or other documented edge.
- **Capture:** explicit local save request with canonical permalink and context.
- **Acquisition method:** official API, share/browser capture, Data Export, or legacy import.
- **Saved authority:** explicit user capture, export observation, or documented provider authority.
- **Unavailable state:** private, deleted, unsupported, blocked, expired, or unresolved.
- **Export snapshot:** immutable archive and parser/completeness report.

## Invariants

1. Local capture is not native Saved state.
2. Official account and third-party capture lanes are distinct.
3. Relation graph preserves provider IDs/direction and handles missing nodes/cycles.
4. Privacy is never bypassed.
5. Raw exports precede parsing; unknown variants survive.
6. Missing export records do not prove deletion.
7. Read connection does not imply publishing consent.
