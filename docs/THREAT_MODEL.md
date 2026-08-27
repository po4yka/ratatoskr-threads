# Threads connector threat model

## Assets

OAuth credentials, own/private content, captures/notes, relation graph, Data Exports, media, publishing authority, and privacy expectations.

## Threats and controls

- **Credential/account mix-up:** PKCE/state, encrypted least-privilege tokens, exact binding, refresh/revoke.
- **Unauthorized write:** separate scope/consent, idempotency, current capability check, audit.
- **Privacy bypass/hidden scraping:** prohibit passwords/cookies/hidden endpoints; supported APIs/public resolution only.
- **Malicious URL/archive:** strict classification plus file/count/size/decompression/path limits and isolated parsing.
- **Graph abuse/cycles:** bounded traversal, stable IDs, cycle detection, no recursive unbounded fetch.
- **Sensitive content leak:** owner authorization, protected blobs, safe events/logs, deletion propagation.
- **False authority:** typed acquisition/authority and accurate UI wording.
- **Capability drift:** refresh and graceful degradation.

Re-review for messaging, broad follower graph, automated publishing, private media retrieval, or public sharing.

## Item 9 controls and residual risk

- Media URLs, redirects, MIME, declared/actual size, digest, and storage budget fail closed before
  durable attachment. Shared content-addressed bytes are not deleted while referenced.
- Privacy operations bind owner and target under lock, keep audit content-free, erase ciphertext and
  content-bearing projections, and prevent late Knowledge completion from resurrection.
- Re-resolution cannot retry private/deleted/unsupported/removed sources and cannot start after any
  run/provider budget is exhausted. No cookie, private endpoint, or browser session is introduced.
- Export reprocessing verifies retained hash/length and exact parser; dry-run cannot write. Reports
  exclude bodies, notes, credentials, raw bytes, and private filesystem paths.

Residual risk: synthetic/redacted archives may not cover a future real export layout. Rollout keeps
apply disabled until authorized real-export review; rollback disables workers/apply while retaining
privacy facts, audit, and raw evidence required by policy. This tooling never changes database
schema versions and is not a database migration path.
