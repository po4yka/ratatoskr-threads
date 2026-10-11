## ADDED Requirements

### Requirement: The example names the shared database and keeps raw objects on NVMe
`deploy/systemd/threads.conf.example` SHALL connect as the role `ratatoskr_threads` to the shared database `ratatoskr` on port 5432 and SHALL place the raw object root under `/mnt/nvme/ratatoskr/blobs/ratatoskr-threads`. The repository SHALL ship `deploy/postgres/role.sql`, which creates that role with `connect` and `create` on `ratatoskr`.

#### Scenario: The shipped example and role file agree
- **WHEN** the shipped example and `deploy/postgres/role.sql` are read
- **THEN** the database URL ends in `@127.0.0.1:5432/ratatoskr` with the role `ratatoskr_threads`, the raw object root is `/mnt/nvme/ratatoskr/blobs/ratatoskr-threads`, and the role file creates `ratatoskr_threads` and grants it `connect, create` on `ratatoskr`

### Requirement: Permalink bounds follow the shared grammar table
The permalink grammar SHALL accept a Threads handle of 30 characters and a post code of 128 characters on each approved host, and SHALL refuse a 31-character handle and a 129-character code.

#### Scenario: Boundary lengths
- **WHEN** a permalink with a 30-character handle and a 128-character code is canonicalized on each of the four hosts
- **THEN** it is accepted, and a 31-character handle or a 129-character code is refused
