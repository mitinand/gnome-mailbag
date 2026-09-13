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

/// One consumer takes the latest state. Clones share one worker; dropping the last
/// handle requests shutdown without blocking the caller.
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
    pending_update: Option<GoaAccountList>,
    update_waker: Option<Waker>,
    command_waker: Option<Waker>,
    refresh_requested: bool,
    check_pending: bool,
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
            state.pending_update = Some(update);
            state.update_waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    fn mark_worker_stopped(&self) {
        let waker = {
            let mut state = self.lock();
            if !state.stop_requested {
                let mut update = state
                    .pending_update
                    .take()
                    .unwrap_or_else(GoaAccountList::initial);
                update.status = CheckStatus::Failed;
                update.membership_confirmed = false;
                update.error = Some(AccountError::new(
                    "account worker",
                    ErrorCause::WorkerStopped,
                ));
                state.pending_update = Some(update);
            } else {
                state.pending_update = None;
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
    /// Start observation on the desktop session bus without waiting for it.
    pub fn start() -> Self {
        client::start()
    }
    pub async fn next_account_update(&self) -> Option<GoaAccountList> {
        poll_fn(|cx| {
            let mut state = self.0.shared.lock();
            if let Some(update) = state.pending_update.take() {
                Poll::Ready(Some(update))
            } else if state.worker_stopped {
                Poll::Ready(None)
            } else {
                state.update_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
    pub fn refresh_accounts(&self) {
        let waker = {
            let mut state = self.0.shared.lock();
            if state.stop_requested || state.worker_stopped || state.check_pending {
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
