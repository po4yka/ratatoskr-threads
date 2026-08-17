# Threads connector interfaces

## Inbound

OAuth connect/callback/refresh/revoke, capability refresh, own-content sync, explicit capture resolve, Data Export import, re-resolve, optional publish/reply, privacy/delete, and operation commands.

## Outbound

Account/capability/post/relation/capture/export/social-source/upstream-status/write-result events, Knowledge triggers, and safe progress/results.

## Rules

Commands include owner/account/operation/idempotency. Capture includes canonical URL, captured time, acquisition, and optional note/collections. Provider credentials remain local. Public resolution records method/version. Relation events reference stable provider nodes and allow unresolved endpoints. Writes validate current capability/scope and explicit consent. Errors distinguish unsupported, auth/reauth, private/unavailable, invalid URL/archive, limits, policy, write conflict, and transient provider failure.
