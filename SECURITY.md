# Security Policy for Ratatoskr Threads

Report vulnerabilities privately. Do not publish access tokens, private posts, account exports, production payloads, publishing credentials, or user captures.

Security review is required for OAuth, capabilities, publishing/replies, capture URLs, public resolution, unavailable/private content, media, Data Export parsing, archive limits, relation traversal, deletion, and logging.

Baseline: official least-privilege API; separate write consent; no passwords/cookies/hidden endpoints; explicit capture only; validate URLs/archives and bound resources; owner-authorize all records; treat text/media metadata as hostile; preserve provenance and uncertainty; never bypass privacy controls.
