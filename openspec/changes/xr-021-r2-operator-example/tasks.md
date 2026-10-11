## 1. Operator example

- [x] 1.1 RED: `services/threads-archive/tests/boot.rs::the_shipped_example_loads` loads `deploy/systemd/threads.conf.example` through `Config::from_environment` and expects operator `127.0.0.1:9084`, the bus URL, `RATATOSKR__BUS__NKEY_SEED_PATH=/etc/ratatoskr/threads.nkey`, the public-resolution keys and the database URL placeholder. Fails today (the file is absent).
- [x] 1.2 GREEN: add `deploy/systemd/threads.conf.example` and the `DEVELOPMENT.md` pointer.
