## ADDED Requirements

### Requirement: Every outbox event type is published and none can block the rest
The relay SHALL publish `platform.operation.reported.v1`, `social.source.captured.v1`, `social.source.updated.v1` and `social.source.removed.v1` to `evt.` plus the event type. The outbox table SHALL refuse any other event type. A row whose publication fails SHALL record an attempt count and a next-attempt time so it does not starve later rows.

#### Scenario: A removal row is published and later rows follow
- **WHEN** a removal row followed by a captured row are in the outbox
- **THEN** both reach the broker and are marked published

#### Scenario: An unknown event type cannot be stored
- **WHEN** an outbox row with an unlisted event type is inserted
- **THEN** the database refuses it
