//! Scheduled own-account synchronization tests.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ratatoskr_threads_archive_service::scheduler::{OwnAccountSyncScheduler, OwnAccountSyncWorker};
#[derive(Debug, Default)]
struct FakeWorker {
    calls: AtomicUsize,
}

impl OwnAccountSyncWorker for FakeWorker {
    async fn sync_account(&self, _account_key: String) {
        self.calls.fetch_add(1, Ordering::Relaxed);
    }
}

#[tokio::test(start_paused = true)]
async fn scheduled_sync_tick_invokes_the_account_worker_once() {
    let scheduler = OwnAccountSyncScheduler::enabled(Duration::from_mins(1))
        .expect("non-zero interval enables scheduling");
    let worker = FakeWorker::default();
    scheduler
        .run_one_due_tick(&worker, "fixture-account".to_owned())
        .await;

    assert_eq!(worker.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn zero_interval_keeps_scheduling_disabled() {
    assert!(OwnAccountSyncScheduler::enabled(Duration::ZERO).is_none());
}
