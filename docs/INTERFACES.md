# Threads connector interfaces

## Inbound

OAuth connect/callback/refresh/revoke, capability refresh, own-content sync, explicit capture resolve, Data Export import, re-resolve, optional publish/reply, privacy/delete, and operation commands.

## Outbound

Account/capability/post/relation/capture/export/social-source/upstream-status/write-result events, Knowledge triggers, and safe progress/results.

## Rules

Commands include owner/account/operation/idempotency. Capture includes canonical URL, captured time, acquisition, and optional note/collections. Provider credentials remain local. Public resolution records method/version. Relation events reference stable provider nodes and allow unresolved endpoints. Writes validate current capability/scope and explicit consent. Errors distinguish unsupported, auth/reauth, private/unavailable, invalid URL/archive, limits, policy, write conflict, and transient provider failure.

## Item 9 interfaces

- Privacy preview/apply names authenticated owner, stable operation id, and exactly one capture or
  connection. Reports contain only closed class/action/count entries.
- Final local deletion emits `social.source.removed.v1` with source id, owner, `user_requested`, and
  removal time. It carries no snapshot, URL, text, note, or credential. Existing consumers remain
  compatible; Knowledge must consume the additive removal event before deletion is considered
  propagated.
- Re-resolution workers claim and reserve before calling only the supported public resolver.
- `reprocess-export dry-run|apply` requires owner, retained run id, and exact parser; apply also
  requires an operation id. Stdout is one JSON report, stderr diagnostics, exits `0/1/2/78`.

Rollout enables additive consumers first, then producers/workers. Rollback disables worker and CLI
apply entry points without retracting already published privacy facts or content-free audit.
