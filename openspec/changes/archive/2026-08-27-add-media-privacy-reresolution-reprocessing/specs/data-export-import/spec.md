## ADDED Requirements

### Requirement: An immutable receipt supports explicit parser reprocessing

An owner-authorized retained export receipt SHALL remain addressable as the source for a `data-export-reprocessing` dry-run or apply operation. The original receipt digest, BlobStore reference, detected export version, and initial import evidence SHALL remain immutable while each reprocessing operation records its own target parser version, result, and completeness evidence.

#### Scenario: reprocessing preserves the original import receipt
- **WHEN** the same retained archive is applied under another registered parser version
- **THEN** its original receipt and initial import report remain byte-for-byte unchanged and the new parser evidence is separately addressable

### Requirement: Reprocessing preserves import safety and authority

Every reprocessing mode SHALL reuse the existing hostile-archive limits, path validation, unknown-section retention, owner scope, `export_observation` authority ceiling, and absence-without-deletion semantics. A parser version SHALL not weaken those controls or upgrade export evidence to native Saved-list authority.

#### Scenario: a new parser cannot reinterpret authority
- **WHEN** reprocessing derives additional normalized records from a retained export
- **THEN** every derived record remains an export observation and no result claims authoritative native Saved membership
