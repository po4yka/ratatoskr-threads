## Purpose

Defines the provenance semantics every Threads acquisition lane inherits: what acquisition modes exist and whether they are supported, the strongest saved-authority claim each mode may make, how local constants map onto the published social-contract grammars, and why upstream availability is never confused with local preservation.

## MODIFIED Requirements

### Requirement: The capability matrix answers for every acquisition mode
The library SHALL expose a total lookup that, for each documented acquisition mode (`ExplicitCapture`, `PublicResolution`, `OwnAccountSync`, `DataExport`, `LegacyImport`), returns an explicit support status (`Supported`, `Planned`, or `NotSupported`), the closed set of wire acquisition-method values the mode produces, and the strongest saved-authority claim the mode is allowed to make. The mode inventory SHALL be exactly these five modes — no hidden sixth lane exists. A mode SHALL report `Planned` until the implementation plan item that builds its lane flips the status with a reviewed test change, and `Supported` from that item onward.

#### Scenario: Each mode resolves to its documented capability
- **WHEN** the capability of each acquisition mode is looked up
- **THEN** every lookup succeeds and reports one explicit support status, exactly the wire method values documented for that mode, and the authority ceiling documented for that mode

#### Scenario: No mode claims support while its lane is unimplemented
- **WHEN** the support statuses of the acquisition modes whose implementing plan items are still open are inspected
- **THEN** each of those modes reports `Planned`

#### Scenario: Only implemented lanes claim support
- **WHEN** the support statuses of all five acquisition modes are inspected
- **THEN** exactly the modes whose implementing plan items have landed report `Supported` — explicit capture alone today — and every remaining mode reports `Planned`
