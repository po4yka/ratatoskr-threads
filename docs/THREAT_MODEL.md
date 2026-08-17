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
