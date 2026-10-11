## Context

See [proposal.md](proposal.md) and XR-021 CONTRACTS.md section R2-10. The seed variable and path table of R2-10 puts Threads on `RATATOSKR__BUS__NKEY_SEED_PATH` at `/etc/ratatoskr/threads.nkey`; the operator listener port 9084 is the allocation in the workspace `docs/DEPLOYMENT_TARGET.md`.

## Decisions

**Format.** The example is a systemd `EnvironmentFile`: one `KEY=value` per line, `#` comments, no quoting, no `export`. The test parses exactly that format, so a line the parser cannot read is a line systemd would also mishandle.

**The test loads the file, not a copy of its values.** `the_shipped_example_loads` reads `deploy/systemd/threads.conf.example`, replaces the absolute secret paths and the raw-evidence directory with temporary ones (the loader checks that they are absolute, and a deployment's real files do not exist on a build machine), loads the result through `Config::from_environment`, and asserts the flow-enabling values. It also asserts the literal text of the shipped file for the values that were substituted (seed path `/etc/ratatoskr/threads.nkey`, the `CHANGE-ME` database password), so substitution cannot hide a wrong default.

**No new production code.** Every key already exists in `config.rs`; the example states them.

## Risks / Trade-offs

- The example repeats the compiled defaults (attempts, batch size, poll interval) on purpose, so an operator sees them; if a default changes the test fails until the example is updated.
