# explicit-capture-intake Specification

## Purpose
Defines what an explicit capture stores, how provenance is pinned to the explicit-capture lane and its platform capture grammar, how duplicate submissions converge deterministically, and which fallback records unavailability produces — truthfully, without inventing provider evidence.

## Requirements

### Requirement: Capture records carry explicit-capture provenance only
Every accepted submission SHALL store `saved_authority = explicit_user_capture` — the request type SHALL carry no authority field at all — with an acquisition method owned by the explicit-capture mode (`share_extension`, `browser_extension`, or `telegram_capture`). The client source SHALL pair with the acquisition method under exactly the documented mapping: `share_extension` with `ios_share_extension` or `android_share_target`, `browser_extension` with `browser_extension`, `telegram_capture` with `telegram`. A mismatched pairing SHALL be refused by a named rule before anything is stored. Each stored capture SHALL retain the original submitted URL text byte-for-byte alongside the canonical permalink, and its captured time SHALL be stamped once at acceptance and never rewritten by replay or fallback.

#### Scenario: Every documented lane pairing is accepted
- **WHEN** one submission per documented method/client pairing is submitted with otherwise valid input
- **THEN** each is stored with its requested acquisition method, its client source, and saved authority exactly `explicit_user_capture`

#### Scenario: A mismatched pairing is refused before storage
- **WHEN** a submission pairs a method with a client source outside the documented mapping, such as `browser_extension` with `telegram`
- **THEN** intake fails naming the pairing rule and no capture row appears

#### Scenario: The raw input text survives next to the canonical permalink
- **WHEN** any submission is accepted
- **THEN** the stored capture carries both the canonical permalink and the original submitted URL text unchanged

### Requirement: Replay of a submission converges deterministically
For one `(user_ref, idempotency_key)` pair at most one capture row SHALL ever exist. Replaying a submission with the same pair SHALL return the already-stored record unchanged — same capture id, same captured time, same status, same canonical URL — including when the replayed raw URL text differs from the original but canonicalizes to the same permalink. Submitting the same canonical permalink under a different idempotency key SHALL create an independent capture row, because distinct local saves are distinct user intent.

#### Scenario: An identical replay returns the stored record and creates no second row
- **WHEN** the same submission is applied twice with the same owner and idempotency key
- **THEN** both applications return the identical record and exactly one capture row exists

#### Scenario: A replay through different raw URL text converges on the stored record
- **WHEN** a replay uses a differently spelled raw URL that canonicalizes to the stored capture's permalink
- **THEN** the replay returns the stored record unchanged and no new row appears

#### Scenario: A distinct key over the same permalink creates an independent capture
- **WHEN** the same permalink is submitted twice by the same owner under two different idempotency keys
- **THEN** two independent capture rows exist, each with its own captured time

### Requirement: Unavailability is recorded truthfully by evidence class
When intake learns a captured post could not be resolved, the fallback records SHALL follow the evidence: an observed `deleted` or `private_or_inaccessible` state SHALL write a tombstone naming the capture as subject with that availability, a reason code, and the observation time, plus a `capture_resolutions` row with outcome `unavailable`, and SHALL mark the capture status `unavailable`; a resolver failure SHALL write only a `capture_resolutions` row with outcome `resolver_failed`, SHALL write no tombstone (missing output is never deletion evidence), and SHALL leave the capture status `accepted`. Every fallback shape SHALL preserve the capture's note, captured time, original URL text, and canonical permalink.

#### Scenario: Observed deletion writes tombstone-backed unavailable state
- **WHEN** a deleted observation is recorded against a stored capture
- **THEN** a tombstone row exists for that capture with availability `deleted`, a resolution row with outcome `unavailable` exists, and the capture status is `unavailable`

#### Scenario: Observed privacy writes the same truthful fallback shape
- **WHEN** a private-or-inaccessible observation is recorded against a stored capture
- **THEN** the fallback shape equals the deletion case except the availability is `private_or_inaccessible`

#### Scenario: A resolver failure never fabricates deletion evidence
- **WHEN** a resolver failure is recorded against a stored capture
- **THEN** a resolution row with outcome `resolver_failed` exists, no tombstone row exists for that capture, and the capture status remains `accepted`

#### Scenario: The user's context survives every fallback
- **WHEN** any fallback is recorded against a capture carrying a note
- **THEN** the note, the captured time, the original URL text, and the canonical permalink all remain unchanged on the stored capture
