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
pub struct GoaClient(Arc<Handle>);
struct Handle {
    shared: Arc<Shared>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

#[derive(Default)]
struct State {
    update: Option<GoaAccountList>,
    consumer: Option<Waker>,
    command: Option<Waker>,
    refresh: bool,
    busy: bool,
    stop: bool,
    closed: bool,
}
#[derive(Default)]
struct Shared(Mutex<State>);
impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().expect("account exchange lock")
    }
    fn stop(&self) {
        let wake = {
            let mut s = self.lock();
            s.stop = true;
            s.command.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    fn publish(&self, update: GoaAccountList) {
        let wake = {
            let mut s = self.lock();
            if s.stop {
                return;
            }
            s.update = Some(update);
            s.consumer.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    fn finish(&self) {
        let wake = {
            let mut s = self.lock();
            if !s.stop {
                let mut update = s.update.take().unwrap_or_else(GoaAccountList::initial);
                update.status = CheckStatus::Failed;
                update.complete = false;
                update.error = Some(AccountError::new(
                    "account worker",
                    ErrorCause::WorkerStopped,
                ));
                s.update = Some(update);
            } else {
                s.update = None;
            }
            s.closed = true;
            s.command = None;
            s.consumer.take()
        };
        if let Some(wake) = wake {
            wake.wake();
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
            let mut s = self.0.shared.lock();
            if let Some(update) = s.update.take() {
                Poll::Ready(Some(update))
            } else if s.closed {
                Poll::Ready(None)
            } else {
                s.consumer = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
    pub fn refresh_accounts(&self) {
        let wake = {
            let mut s = self.0.shared.lock();
            if s.stop || s.closed || s.busy {
                return;
            }
            s.refresh = true;
            s.command.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    pub fn stop(&self) {
        self.0.shared.stop();
    }
    #[cfg(test)]
    fn shared_for_test(&self) -> Arc<Shared> {
        self.0.shared.clone()
    }
}
