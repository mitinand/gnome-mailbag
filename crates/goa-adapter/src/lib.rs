// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

mod accounts;
use account_source::{AccountCheckError, AccountId, AccountUpdate, CheckStatus, ErrorCause};

#[cfg(test)]
#[path = "../../../tests/support/bus.rs"]
mod test_bus;
#[cfg(test)]
#[path = "../../../tests/support/goa.rs"]
mod test_goa;

mod client;

use std::{
    future::poll_fn,
    sync::{Arc, Mutex, MutexGuard},
    task::{Poll, Waker},
};

/// Sends refresh and stop commands to the GOA worker.
/// Clones share one GOA worker; dropping the last handle requests shutdown
/// without waiting for the worker to finish.
#[derive(Clone)]
pub struct GoaAdapter(Arc<ClientHandle>);
struct ClientHandle {
    shared: Arc<SharedClientState>,
}
impl Drop for ClientHandle {
    fn drop(&mut self) {
        self.shared.request_stop();
    }
}

#[derive(Default)]
struct ClientState {
    latest_update: Option<Arc<AccountUpdate>>,
    update_pending: bool,
    update_waker: Option<Waker>,
    command_waker: Option<Waker>,
    refresh_requested: bool,
    stop_requested: bool,
    worker_stopped: bool,
}
#[derive(Default)]
struct SharedClientState(Mutex<ClientState>);
impl SharedClientState {
    fn lock(&self) -> MutexGuard<'_, ClientState> {
        self.0.lock().expect("account exchange lock")
    }
    fn request_stop(&self) {
        let waker = {
            let mut state = self.lock();
            state.stop_requested = true;
            state.command_waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    fn publish_update(&self, update: AccountUpdate) {
        let waker = {
            let mut state = self.lock();
            if state.stop_requested {
                return;
            }
            state.latest_update = Some(Arc::new(update));
            state.update_pending = true;
            state.update_waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    fn mark_worker_stopped(&self) {
        // Keep the last account list after delivery so a worker crash can report
        // failure with the known accounts still present. Copy outside the lock.
        let last_update = self.lock().latest_update.clone();
        let mut failed_update = last_update
            .as_deref()
            .cloned()
            .unwrap_or_else(AccountUpdate::default);
        failed_update.update_number += 1;
        failed_update.status = CheckStatus::Failed;
        failed_update.membership_confirmed = false;
        failed_update.error = Some(AccountCheckError::new(
            "account worker",
            ErrorCause::SourceStopped,
        ));
        let waker = {
            let mut state = self.lock();
            if state.stop_requested {
                state.latest_update = None;
                state.update_pending = false;
            } else {
                state.latest_update = Some(Arc::new(failed_update));
                state.update_pending = true;
            }
            state.worker_stopped = true;
            state.command_waker = None;
            state.update_waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}
impl GoaAdapter {
    /// Start reading accounts and listening for GOA changes on the desktop session bus.
    /// Return command handles and the sole updates receiver without waiting for GOA.
    pub fn start() -> (Self, GoaUpdates) {
        client::start()
    }
    pub fn refresh_accounts(&self) {
        let waker = {
            let mut state = self.0.shared.lock();
            if state.stop_requested || state.worker_stopped {
                return;
            }
            state.refresh_requested = true;
            state.command_waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    pub fn stop(&self) {
        self.0.shared.request_stop();
    }
    #[cfg(test)]
    fn shared_for_test(&self) -> Arc<SharedClientState> {
        self.0.shared.clone()
    }
}

/// Sole receiver of the latest account list and check status.
/// Dropping it requests worker shutdown without waiting for the worker.
///
/// A pending read exclusively borrows the receiver:
/// ```compile_fail
/// use goa_adapter::GoaUpdates;
/// fn two_reads(updates: &mut GoaUpdates) {
///     let first = updates.next_account_update();
///     let second = updates.next_account_update();
///     drop((first, second));
/// }
/// ```
/// The receiver cannot be cloned:
/// ```compile_fail
/// use goa_adapter::GoaUpdates;
/// fn duplicate(updates: GoaUpdates) { let _ = updates.clone(); }
/// ```
pub struct GoaUpdates {
    shared: Arc<SharedClientState>,
}
impl GoaUpdates {
    /// Wait for the latest list; return None once the worker has stopped.
    pub async fn next_account_update(&mut self) -> Option<AccountUpdate> {
        poll_fn(|cx| {
            let mut state = self.shared.lock();
            if std::mem::take(&mut state.update_pending) {
                Poll::Ready(state.latest_update.clone())
            } else if state.worker_stopped {
                Poll::Ready(None)
            } else {
                state.update_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
        .map(|update| (*update).clone())
    }
}
impl Drop for GoaUpdates {
    fn drop(&mut self) {
        self.shared.request_stop();
    }
}
