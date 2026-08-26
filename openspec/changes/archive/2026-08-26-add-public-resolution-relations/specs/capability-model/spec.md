## MODIFIED Requirements

### Requirement: Only implemented lanes claim support

The capability matrix SHALL report `Supported` only for lanes whose implementing plan item has landed with its reviewed tests. Explicit capture and public resolution SHALL report `Supported`; own-account sync, Data Export, and legacy import SHALL report `Planned` until their implementing plan items land.

#### Scenario: Exactly the implemented lanes report supported

- **WHEN** the support statuses of all five acquisition modes are inspected
- **THEN** exactly `ExplicitCapture` and `PublicResolution` report `Supported` and every remaining
  mode reports `Planned`
