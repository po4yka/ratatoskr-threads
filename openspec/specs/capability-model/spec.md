# capability-model Specification

## Purpose
Defines the provenance semantics every Threads acquisition lane inherits: what acquisition modes exist and whether they are supported, the strongest saved-authority claim each mode may make, how local constants map onto the published social-contract grammars, and why upstream availability is never confused with local preservation.

## Requirements

### Requirement: The capability matrix answers for every acquisition mode
The library SHALL expose a total lookup that, for each documented acquisition mode (`ExplicitCapture`, `PublicResolution`, `OwnAccountSync`, `DataExport`, `LegacyImport`), returns an explicit support status (`Supported`, `Planned`, or `NotSupported`), the closed set of wire acquisition-method values the mode produces, and the strongest saved-authority claim the mode is allowed to make. The mode inventory SHALL be exactly these five modes — no hidden sixth lane exists. A mode SHALL report `Planned` until the implementation plan item that builds its lane flips the status with a reviewed test change, and `Supported` from that item onward. Official OAuth capability discovery SHALL reconcile per-account results against this lookup and SHALL report unsupported matrix entries as unavailable rather than treating a provider scope as an enabled lane.

#### Scenario: Each mode resolves to its documented capability
- **WHEN** the capability of each acquisition mode is looked up
- **THEN** every lookup succeeds and reports one explicit support status, exactly the wire method values documented for that mode, and the authority ceiling documented for that mode

#### Scenario: No mode claims support while its lane is unimplemented
- **WHEN** the support statuses of the acquisition modes whose implementing plan items are still open are inspected
- **THEN** each of those modes reports `Planned`

#### Scenario: Exactly the implemented lanes report supported
- **WHEN** the support statuses of all five acquisition modes are inspected
- **THEN** exactly `ExplicitCapture`, `PublicResolution`, and `OwnAccountSync` report `Supported` and every remaining mode reports `Planned`

#### Scenario: Discovery does not enable a planned lane
- **WHEN** official account discovery observes scopes that permit own-content synchronization
- **THEN** it records the account capability but does not itself start an own-account synchronization

### Requirement: The native Saved list is a stated non-capability
Because no supported provider surface exposes a personal account's native Saved list on Threads, the capability matrix SHALL report native Saved-list synchronization as `NotSupported` together with that reason, and no acquisition mode's authority path SHALL be able to produce a claim that the user's native Saved membership is known from an explicit capture.

#### Scenario: Native Saved synchronization reports not-supported with its reason
- **WHEN** native Saved-list synchronization is looked up in the capability matrix
- **THEN** the answer is `NotSupported` carrying the reason that no supported provider surface exposes the personal Saved list

### Requirement: Authority ceilings are fixed per mode
Each acquisition mode SHALL carry a fixed authority ceiling: explicit capture and public resolution SHALL never exceed `explicit_user_capture`; own-post sync through the official API MAY reach `authoritative_platform_state`; data export SHALL never exceed `export_observation`; legacy import SHALL never exceed `legacy_observation`. No lookup or conversion SHALL raise a record's authority above its mode's ceiling.

#### Scenario: Only own-account sync reaches authoritative platform state
- **WHEN** the authority ceilings of all five modes are checked
- **THEN** only `OwnAccountSync` carries `authoritative_platform_state`, while `ExplicitCapture` and `PublicResolution` carry `explicit_user_capture`, `DataExport` carries `export_observation`, and `LegacyImport` carries `legacy_observation`

### Requirement: Local vocabularies align with the published social-contract grammars
The saved-authority values this service puts on the wire SHALL equal, value for value, the `SavedAuthority` vocabulary of the published `ratatoskr-social-contracts` crate at the revision recorded in the alignment review. The acquisition-method values SHALL contain every variant of that crate's `AcquisitionMethod` vocabulary exactly once, each produced by exactly one local acquisition mode, plus the single documented Threads extension `telegram_capture` owned by explicit capture; no other local method may exist.

#### Scenario: The local method set contains the contract set plus the documented extension
- **WHEN** the local acquisition-method set is enumerated and compared against the recorded contract vocabulary
- **THEN** the difference is exactly `telegram_capture`, every contract method is present, and each method including the extension is produced by exactly one acquisition mode

#### Scenario: The local authority set equals the contract set
- **WHEN** the reachable saved-authority set of the five mode ceilings is enumerated and compared against the recorded contract vocabulary
- **THEN** both sets are equal

### Requirement: Upstream status and preservation state stay separate vocabularies
What Threads last reported about a source (the upstream availability observation mirrored from the schema CHECK vocabulary) and what Ratatoskr holds locally (the preservation state) SHALL be distinct types with no implicit conversion between them. Applying any upstream availability value to any preservation state SHALL leave the preservation state unchanged: observing deletion upstream never demotes content already preserved.

#### Scenario: No observation changes what was preserved
- **WHEN** every upstream availability value is applied to every preservation state
- **THEN** each application leaves the preservation state unchanged, including a deleted-upstream observation against fully preserved content
