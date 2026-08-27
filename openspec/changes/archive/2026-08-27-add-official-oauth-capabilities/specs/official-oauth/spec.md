## Purpose

Defines the secure, provider-authorised account connection that makes actual Threads account capabilities observable without exposing credentials or inventing unsupported authority.

## ADDED Requirements

### Requirement: Official credentials remain encrypted and owner-bound
The service SHALL encrypt OAuth access and refresh credentials before they reach durable storage, bind ciphertext to its owning account, record its key version, and reject tampered, mismatched, or malformed envelopes without rendering secret material.

#### Scenario: Stored credential round trip
- **WHEN** a valid provider credential is recorded for an account and then loaded with the same account binding and key version
- **THEN** the original credential values are available to the official adapter while no persisted field contains either plaintext value

#### Scenario: Credential envelope is invalid for a different owner
- **WHEN** a sealed credential is presented for a different account or with altered bytes
- **THEN** loading is refused without exposing either credential value

### Requirement: Refresh replaces the active grant atomically
The service SHALL use the official refresh surface only for a connected account, replace its encrypted active grant and expiry atomically on success, and preserve a truthful reauthorization-required state when the provider rejects the grant.

#### Scenario: Successful refresh updates the active grant
- **WHEN** the provider returns a valid refreshed official grant
- **THEN** the stored active credential, scopes, and expiry represent that grant and the prior credential cannot be loaded as active

#### Scenario: Refresh rejection requires reauthorization
- **WHEN** the provider completes a refresh response that invalidates the current grant
- **THEN** the account is marked as requiring reauthorization and no new active credential is asserted

### Requirement: Revocation scrubs local credential material completely
The service SHALL mark a revoked connection, remove all encrypted credential material and refresh metadata in the same durable operation, and retain only non-secret audit evidence of the lifecycle transition.

#### Scenario: Revoke removes all credential material
- **WHEN** a connected account is revoked
- **THEN** querying the account finds no access token ciphertext, refresh token ciphertext, token expiry, or granted scopes while the account reports revoked and an audit record remains

### Requirement: Discovery is reconciled with the capability matrix
The service SHALL derive account capabilities from the provider's granted scopes and observed account type, reconcile them with the local capability matrix, and persist both supported capabilities and explicit unavailable reasons. It SHALL not turn grant presence into own-content synchronization, native Saved-list membership, or publishing consent.

#### Scenario: Granted scopes are narrower than a feature requirement
- **WHEN** official discovery observes an account without a required scope
- **THEN** that capability is recorded unavailable with the missing scope and no feature is reported enabled

#### Scenario: Native Saved support remains unavailable
- **WHEN** official discovery completes for any connected account
- **THEN** native Saved-list synchronization remains unavailable with the matrix reason regardless of granted scopes

### Requirement: Official budget observations are bounded and attributable
The service SHALL record non-secret official API budget observations with the account, provider endpoint class, remaining allowance, reset time when supplied, and provider request identifier when supplied. Invalid or contradictory budget observations SHALL be refused without replacing the last valid observation.

#### Scenario: A valid official response updates its budget observation
- **WHEN** discovery receives a valid remaining allowance and reset time from the official provider surface
- **THEN** the account budget record reflects the new observation without storing a credential or response body
