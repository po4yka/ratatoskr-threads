## MODIFIED Requirements

### Requirement: Provenance vocabularies are enforced closed
The capture and post records SHALL carry an acquisition-method column and a saved-authority column constrained by named CHECK constraints to their documented closed vocabularies (`official_api | share_extension | browser_extension | telegram_capture | public_resolution | data_export | legacy_import` for acquisition; `explicit_user_capture | export_observation | authoritative_platform_state | legacy_observation` for authority). Inserting any other value SHALL be refused by the database. No stored value SHALL assert membership in a native Threads Saved list: no supported provider surface exposes one, so no vocabulary value carries that meaning.

#### Scenario: Unknown acquisition method is refused
- **WHEN** a row is inserted into a provenance-bearing table with an acquisition method outside the closed vocabulary
- **THEN** the insert fails with the named CHECK constraint

#### Scenario: Documented authority values are accepted
- **WHEN** rows are inserted using each documented saved-authority value, including `explicit_user_capture`
- **THEN** all inserts succeed

#### Scenario: Public resolution is accepted on provenance-bearing tables
- **WHEN** rows are inserted into both provenance-bearing tables using acquisition method `public_resolution`
- **THEN** all inserts succeed

#### Scenario: The former unknown authority value is refused
- **WHEN** a row is inserted into a provenance-bearing table with saved authority `unknown`
- **THEN** the insert fails with the named CHECK constraint

## ADDED Requirements

### Requirement: Relation kinds follow the published open token grammar
The `post_relations.relation_kind` column SHALL accept exactly the tokens matching the published social-contract relation-kind grammar (lowercase letters, digits, and underscores, starting with a letter, at most 32 characters) and SHALL refuse anything else, so provider edge kinds beyond `reply`, `quote`, and `repost` are preserved losslessly instead of being refused or misfiled.

#### Scenario: A well-formed relation kind beyond the documented three is accepted
- **WHEN** a post-relation edge is inserted with the well-formed relation kind `mention`
- **THEN** the insert succeeds

#### Scenario: A malformed relation kind is refused
- **WHEN** a post-relation edge is inserted with a relation kind violating the grammar, such as an uppercase letter, an empty string, or a 33-character token
- **THEN** the insert fails with the named CHECK constraint
