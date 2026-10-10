//! A local HTTP fake of the public Threads oEmbed endpoint.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

/// What the fake answers to one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeReply {
    /// HTTP 200 with a valid Threads oEmbed document for the fake's permalink.
    Oembed,
    /// An empty JSON object with this status.
    Status(u16),
}

/// A plain-HTTP loopback endpoint that plays the public Threads oEmbed surface.
///
/// The nth request is answered with the nth reply; once the replies run out the last one repeats.
/// Hand [`FakeOembed::endpoint`] to
/// [`ApprovedOembedClient::for_loopback_test`](crate::public_resolution::ApprovedOembedClient::for_loopback_test).
#[derive(Debug)]
pub struct FakeOembed {
    address: SocketAddr,
    requests: Arc<AtomicUsize>,
    server: tokio::task::JoinHandle<()>,
}

impl FakeOembed {
    /// Starts the fake on a free loopback port, serving documents for `permalink`.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when no loopback port can be bound.
    pub async fn start(permalink: &str, replies: Vec<FakeReply>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&requests);
        let permalink = permalink.to_owned();
        let server = tokio::spawn(async move {
            while let Ok((stream, _peer)) = listener.accept().await {
                tokio::spawn(answer(
                    stream,
                    Arc::clone(&counter),
                    replies.clone(),
                    permalink.clone(),
                ));
            }
        });
        Ok(Self {
            address,
            requests,
            server,
        })
    }

    /// The endpoint URL, `http://127.0.0.1:<port>/oembed`.
    #[must_use]
    pub fn endpoint(&self) -> String {
        format!("http://{}/oembed", self.address)
    }

    /// How many requests the fake has received.
    #[must_use]
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for FakeOembed {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn answer(
    mut stream: tokio::net::TcpStream,
    counter: Arc<AtomicUsize>,
    replies: Vec<FakeReply>,
    permalink: String,
) {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(chunk.get(..read).unwrap_or_default()),
        }
    }
    let nth = counter.fetch_add(1, Ordering::SeqCst);
    let reply = replies
        .get(nth)
        .or_else(|| replies.last())
        .copied()
        .unwrap_or(FakeReply::Status(500));
    let _ignored = stream
        .write_all(response(reply, &permalink).as_bytes())
        .await;
    let _ignored = stream.shutdown().await;
}

fn response(reply: FakeReply, permalink: &str) -> String {
    let (status, body) = match reply {
        FakeReply::Oembed => (
            200,
            format!(
                r#"{{"provider_name":"Threads","provider_url":"https://www.threads.net/","url":"{permalink}","html":"<blockquote>Public text</blockquote>","type":"rich","version":"1.0"}}"#
            ),
        ),
        FakeReply::Status(status) => (status, "{}".to_owned()),
    };
    format!(
        "HTTP/1.1 {status} Reply\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
