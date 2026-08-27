//! Explicit, bounded scheduling for account synchronization work.

use std::time::Duration;

/// A worker that synchronizes one known account when a scheduled tick is due.
pub trait OwnAccountSyncWorker: Send + Sync {
    /// Runs one bounded account synchronization attempt.
    fn sync_account(&self, account_key: String) -> impl Future<Output = ()> + Send;
}

/// A periodic scheduler that remains disabled until given an interval.
#[derive(Debug, Clone, Copy)]
pub struct OwnAccountSyncScheduler {
    interval: Duration,
}

impl OwnAccountSyncScheduler {
    /// Creates an enabled scheduler for a non-zero interval.
    #[must_use]
    pub fn enabled(interval: Duration) -> Option<Self> {
        (!interval.is_zero()).then_some(Self { interval })
    }

    /// Waits for one due tick and invokes the supplied worker exactly once.
    pub async fn run_one_due_tick<W>(&self, worker: &W, account_key: String)
    where
        W: OwnAccountSyncWorker,
    {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.tick().await;
        ticker.tick().await;
        worker.sync_account(account_key).await;
    }
}
