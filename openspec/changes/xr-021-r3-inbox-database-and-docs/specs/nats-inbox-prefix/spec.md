## ADDED Requirements

### Requirement: The broker connection uses the service's own inbox prefix
Both constructors of the service's broker connection SHALL set the inbox prefix `_INBOX_threads`, so every JetStream reply reaches the service on a subject its stanza allows it to subscribe to. The repository SHALL ship `deploy/nats/identity.conf`, a copy of the `THREADS` stanza of the deployed ACL.

#### Scenario: JetStream requests complete under the shipped stanza
- **WHEN** a broker that enforces only the shipped stanza (plus an administrator that provisions the streams and the durable) is connected through the service's nkey connection function
- **THEN** the durable verifies, a message can be pulled from it and an operation report publishes with an acknowledgement
