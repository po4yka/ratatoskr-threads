## ADDED Requirements

### Requirement: A stopped bus worker makes the service not ready and exits
When the NATS worker returns for any reason before an orderly shutdown, the service SHALL report readiness false and SHALL exit with a non-zero status. It SHALL NOT log the error and remain ready.

#### Scenario: Broker loss flips readiness and the exit code
- **WHEN** the broker is stopped after the service became ready
- **THEN** `/health/ready` answers 503 within the probe interval and the process exits with a non-zero status
