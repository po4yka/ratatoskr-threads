//! Supervision of the background workers that must outlive the operator plane.
//!
//! A worker that returns before an orderly shutdown is a fault, whether it returned an error or
//! quietly finished: the service must stop reporting itself ready and exit non-zero instead of
//! logging the loss and carrying on. [`spawn`] therefore gives every worker a completion signal
//! that the supervising code selects on beside its own shutdown signal.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Which background worker a signal is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
    /// The NATS command consumer and outbox relay.
    Bus,
    /// The capture resolver.
    Resolver,
    /// The consumer of Knowledge's deletion acknowledgements.
    DeletionAcks,
}

impl fmt::Display for WorkerKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Bus => "bus",
            Self::Resolver => "resolver",
            Self::DeletionAcks => "deletion_acks",
        })
    }
}

/// Resolves when the supervisor asks the worker to stop.
#[derive(Debug)]
pub struct StopSignal(oneshot::Receiver<()>);

impl Future for StopSignal {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        // A dropped supervisor is a stop request as well.
        Pin::new(&mut self.0).poll(context).map(|_| ())
    }
}

/// A running background worker with its stop and completion signals.
#[derive(Debug)]
pub struct Worker {
    kind: WorkerKind,
    stop: oneshot::Sender<()>,
    exited: oneshot::Receiver<()>,
    task: JoinHandle<()>,
}

/// Starts `work` as a task that reports its own return.
///
/// `work` receives the [`StopSignal`] it must return on. Its result is logged by kind and then
/// signalled; the error text is the worker's own content-free error.
pub fn spawn<W, Fut, E>(kind: WorkerKind, work: W) -> Worker
where
    W: FnOnce(StopSignal) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    E: fmt::Display,
{
    let (stop, stop_signal) = oneshot::channel();
    let (done, exited) = oneshot::channel();
    let task = tokio::spawn(async move {
        match work(StopSignal(stop_signal)).await {
            Ok(()) => tracing::info!(worker = %kind, "the worker returned"),
            Err(error) => tracing::error!(worker = %kind, %error, "the worker stopped"),
        }
        let _ignored = done.send(());
    });
    Worker {
        kind,
        stop,
        exited,
        task,
    }
}

impl Worker {
    /// Resolves when the worker task has returned for any reason.
    ///
    /// Call it once: a worker that has been stopped on purpose also resolves it.
    pub async fn exited(&mut self) -> WorkerKind {
        let _ignored = (&mut self.exited).await;
        self.kind
    }

    /// Asks the worker to stop and waits up to `grace` for it to return.
    pub async fn stop(self, grace: Duration) {
        let _ignored = self.stop.send(());
        if tokio::time::timeout(grace, self.task).await.is_err() {
            tracing::warn!(worker = %self.kind, "the worker did not stop within the grace period");
        }
    }
}
