## MODIFIED Requirements

### Requirement: The capability matrix answers for every acquisition mode
The library SHALL expose a total lookup that, for each documented acquisition mode
(`ExplicitCapture`, `PublicResolution`, `OwnAccountSync`, `DataExport`, `LegacyImport`), returns
an explicit support status (`Supported`, `Planned`, or `NotSupported`), the closed set of wire
acquisition-method values the mode produces, and the strongest saved-authority claim the mode is
allowed to make. The mode inventory SHALL be exactly these five modes — no hidden sixth lane
exists. A mode SHALL report `Planned` until the implementation plan item that builds its lane flips
the status with a reviewed test change, and `Supported` from that item onward. Official OAuth
capability discovery SHALL reconcile per-account results against this lookup and SHALL report
unsupported matrix entries as unavailable rather than treating a provider scope as an enabled lane.

#### Scenario: Each mode resolves to its documented capability
- **WHEN** the capability of each acquisition mode is looked up
- **THEN** every lookup succeeds and reports one explicit support status, exactly the wire method
  values documented for that mode, and the authority ceiling documented for that mode

#### Scenario: No mode claims support while its lane is unimplemented
- **WHEN** the support statuses of the acquisition modes whose implementing plan items are still
  open are inspected
- **THEN** each of those modes reports `Planned`

#### Scenario: Exactly the implemented lanes report supported
- **WHEN** the support statuses of all five acquisition modes are inspected
- **THEN** exactly `ExplicitCapture`, `PublicResolution`, `OwnAccountSync`, and `DataExport`
  report `Supported`, while `LegacyImport` reports `Planned`

#### Scenario: Discovery does not enable a planned lane
- **WHEN** official account discovery observes scopes that permit own-content synchronization
- **THEN** it records the account capability but does not itself start an own-account synchronization
