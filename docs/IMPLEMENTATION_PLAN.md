# Threads connector implementation plan

1. Scaffold service, config, telemetry, health, errors, and `threads_archive` schema.
2. Define capability, acquisition, authority, upstream status, and relation contracts.
3. Implement explicit capture, permalink canonicalization, idempotency, and unavailable fallback.
4. Implement supported public resolution and relation graph normalization.
5. Publish SocialSource and integrate with Knowledge.
6. Add encrypted official OAuth and account capability discovery.
7. Add own-post/reply synchronization.
8. Implement immutable safe Data Export import and completeness report.
9. **Complete:** media retention, privacy deletion propagation, budgeted re-resolution, and restartable parser reprocessing with dry-run reports.
10. Add optional publishing/reply only after separate consent ADR and tests.

Definition of Done: provenance/relations are correct, no hidden session access exists, imports are safe, writes explicit, private data authorized, schema/events/tests and the planned workspace flow pass.
