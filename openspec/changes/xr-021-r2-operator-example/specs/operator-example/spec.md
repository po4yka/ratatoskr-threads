## ADDED Requirements

### Requirement: A loadable operator example ships with the service
The repository SHALL ship `deploy/systemd/threads.conf.example` stating every key the explicit-capture flow needs: the operator listener, the database URL with a placeholder secret, the bus URL, the absolute nkey seed path `/etc/ratatoskr/threads.nkey`, and the public-resolution keys including the approved oEmbed endpoint and the raw-evidence directory. The file SHALL load through the service's own configuration loader once its absolute paths name existing temporary locations.

#### Scenario: The shipped example loads
- **WHEN** the example is loaded through the configuration loader with temporary files substituted for the absolute secret and evidence paths
- **THEN** the configuration is valid and carries operator address `127.0.0.1:9084`, the example's bus URL, the oEmbed endpoint `https://graph.threads.com/oembed`, the example's attempt, batch and interval values, and a database URL
