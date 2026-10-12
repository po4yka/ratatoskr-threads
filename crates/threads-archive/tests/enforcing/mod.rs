//! A private, authorization-enabled `nats-server` for the tests that need the deployed ACL: the
//! Threads stanza shipped in `deploy/nats/identity.conf` and an administrator standing in for Edge.

#![allow(
    clippy::expect_used,
    clippy::panic,
    dead_code,
    reason = "assertions in a test binary; each test binary uses a part of these helpers"
)]

use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use uuid::Uuid;

pub(crate) const HOST: &str = "127.0.0.1";

/// A private `nats-server` with `JetStream` whose only users are an administrator standing in for
/// Edge and the Threads stanza shipped in `deploy/nats/identity.conf`.
pub(crate) struct Enforcing {
    child: Child,
    port: u16,
    pub(crate) directory: PathBuf,
}

impl Enforcing {
    pub(crate) fn start(threads_public: &str, admin_public: &str) -> Self {
        let stanza = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/nats/identity.conf"
        ))
        .expect("deploy/nats/identity.conf is shipped");
        let stanza: String = stanza
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("nkey:") {
                    format!("    nkey: {threads_public}")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let port = std::net::TcpListener::bind((HOST, 0))
            .expect("a probe listener")
            .local_addr()
            .expect("a local address")
            .port();
        let directory = std::env::temp_dir().join(format!(
            "ratatoskr-threads-inbox-prefix-{}",
            Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let config = format!(
            "listen: {HOST}:{port}\njetstream {{ store_dir: \"{store}\" }}\nauthorization {{\n  \
             users = [\n{stanza}\n    {{ nkey: {admin_public} }}\n  ]\n}}\n",
            store = directory.join("js").display(),
        );
        let config_path = directory.join("nats.conf");
        std::fs::write(&config_path, config).expect("the broker configuration is written");
        let child = Command::new("nats-server")
            .arg("-c")
            .arg(&config_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("nats-server is on PATH (CI copies it out of the NATS image)");
        let broker = Self {
            child,
            port,
            directory,
        };
        let listening = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            TcpStream::connect((HOST, port)).is_ok()
        });
        assert!(listening, "the enforcing broker starts listening");
        broker
    }

    pub(crate) fn url(&self) -> String {
        format!("nats://{HOST}:{}", self.port)
    }
}

impl Drop for Enforcing {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
