// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

mod accounts;
pub use accounts::*;

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

/// Delivers the latest account list and check status to one caller at a time.
/// Clones share one GOA worker; dropping the last handle requests shutdown
/// without waiting for the worker to finish.
#[derive(Clone)]
pub struct GoaClient(Arc<ClientHandle>);
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
    latest_update: Option<Arc<GoaAccountList>>,
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
    fn publish_update(&self, update: GoaAccountList) {
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
            .unwrap_or_else(GoaAccountList::initial);
        failed_update.update_number += 1;
        failed_update.status = CheckStatus::Failed;
        failed_update.membership_confirmed = false;
        failed_update.error = Some(AccountError::new(
            "account worker",
            ErrorCause::WorkerStopped,
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
impl GoaClient {
    /// Start reading accounts and listening for GOA changes on the desktop session bus.
    /// Return immediately while the worker connects and requests the account list.
    pub fn start() -> Self {
        client::start()
    }
    pub async fn next_account_update(&self) -> Option<GoaAccountList> {
        poll_fn(|cx| {
            let mut state = self.0.shared.lock();
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
