## Purpose

Defines when Threads media bytes may be archived, how metadata-only and byte-complete states remain truthful, and how expiry or deletion reclaims owned blobs safely.

## ADDED Requirements

### Requirement: Media-byte archival is explicit and policy-bound

The service SHALL keep media metadata separate from media-byte archival and SHALL default every observation to metadata-only. It SHALL archive bytes only for an authenticated owner request or an explicitly enabled retention policy whose eligibility decision validates the supported acquisition lane, permitted media kind and MIME, redirect chain, declared and received size, URL lifetime, rights classification, and remaining per-object and per-owner storage budgets. A failed or indeterminate eligibility check SHALL retain metadata only and SHALL not claim media completeness.

#### Scenario: metadata does not trigger an automatic download
- **WHEN** public resolution observes media metadata without an explicit archival request or enabled eligible policy
- **THEN** no media bytes are fetched or stored and the source remains truthfully metadata-only

#### Scenario: a budget guard refuses bytes before storage
- **WHEN** an otherwise eligible media response exceeds its object or owner storage budget
- **THEN** the service stores no partial blob, records the bounded refusal class, and keeps the media record metadata-only

### Requirement: Archived bytes are verified and separately provenanced

Before a media record becomes byte-complete, the service SHALL verify the final HTTPS origin and redirects, MIME, actual byte length, digest, and any available dimensions or duration against policy. Provider-derived bytes and user-uploaded evidence SHALL have distinct provenance and SHALL never be substituted for one another.

#### Scenario: mismatched bytes never become complete media
- **WHEN** fetched bytes have a disallowed MIME, size, digest, or final URL
- **THEN** the media record does not reference those bytes as archived media and the safe failure contains no credential or content body

#### Scenario: user evidence stays a separate artifact
- **WHEN** a user uploads a screenshot for an unavailable Threads permalink
- **THEN** its record remains user-uploaded evidence and is never reported as provider-fetched canonical media

### Requirement: Retention expiry is reference-safe and auditable

The service SHALL evaluate retention from persisted policy decisions and deadlines rather than from provider absence. On expiry or owner deletion it SHALL remove a service-owned blob only after no live retained record references it, SHALL downgrade or remove the corresponding media projection truthfully, and SHALL retain only non-sensitive audit counts and policy reason. A failed BlobStore deletion SHALL remain pending and retryable; completion SHALL not be reported until the referenced bytes are gone or verified absent.

#### Scenario: a shared digest survives one expired reference
- **WHEN** one media record expires while another live owner-authorized record references the same content digest
- **THEN** the first reference is removed but the content-addressed blob remains readable for the live reference

#### Scenario: failed physical deletion is not reported complete
- **WHEN** database state no longer serves an expired media object but deletion of its unreferenced blob fails
- **THEN** the audit reports pending blob deletion and a retry can finish it without restoring the media projection
