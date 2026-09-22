// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Loads one account's Inbox from its provider. This crate joins Online
//! Accounts, the protocol crate and the content crate; it owns no widget and
//! no application state.

mod batch;
mod imap;
mod worker;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/support/record.rs"]
mod test_record;
#[cfg(test)]
mod tests;

pub use batch::{
    CancelsLoadOnDrop, LoadFailure, LoadResult, ReceivedBatch, ReceivedContent, ReceivedMessage,
    ServerFailure,
};
pub use worker::{LoadHandle, MailWorker};

use goa_adapter::{AccountId, GoaAdapter, ImapAccessError, ImapAccessRequest};
use std::{cell::RefCell, rc::Rc};

/// How one load ended.
#[derive(Debug)]
pub enum LoadOutcome {
    Loaded(ReceivedBatch),
    Failed(ServerFailure),
    /// The load was cancelled and its connection is closed.
    Cancelled,
    /// The mail worker stopped without a result, so nothing was loaded. The
    /// next refresh starts a new worker.
    WorkerStopped,
}

/// Starts one account's Inbox load and reports how it ended. The window loads
/// with Online Accounts and the mail worker; the graphical test reports
/// results without a server.
pub trait LoadsInbox {
    /// Reports the result once, on the calling GLib context. The returned
    /// step cancels the load when it is dropped.
    fn start_load(
        &self,
        account_id: &AccountId,
        report: Box<dyn FnOnce(LoadResult)>,
    ) -> Box<dyn CancelsLoadOnDrop>;
}

/// Loads an Inbox with the account's Online Accounts settings and password,
/// and the mail worker that speaks to the server.
pub struct MailLoader {
    accounts: GoaAdapter,
    worker: Rc<MailWorker>,
}

impl MailLoader {
    pub fn new(accounts: GoaAdapter) -> Self {
        Self {
            accounts,
            worker: Rc::new(MailWorker::new()),
        }
    }
}

impl LoadsInbox for MailLoader {
    fn start_load(
        &self,
        account_id: &AccountId,
        report: Box<dyn FnOnce(LoadResult)>,
    ) -> Box<dyn CancelsLoadOnDrop> {
        let step = Rc::new(RefCell::new(LoadStep::RequestingAccess(None)));
        let transfer_step = step.clone();
        let worker = self.worker.clone();
        let request = self
            .accounts
            .request_imap_access(account_id, move |access| match access {
                Ok(access) => {
                    tracing::info!(
                        encryption = ?access.encryption,
                        "Online Accounts gave the settings and password"
                    );
                    let transfer =
                        worker.load_inbox(access, move |outcome| report(load_result(outcome)));
                    *transfer_step.borrow_mut() = LoadStep::Transferring {
                        _transfer: transfer,
                    };
                }
                // The request was cancelled by an exclusion or by quitting.
                Err(ImapAccessError::Cancelled) => report(LoadResult::Cancelled),
                Err(error) => report(LoadResult::Failed(LoadFailure::OnlineAccounts(error))),
            });
        // Online Accounts answers later, except for the settings failure it
        // reports at once, which has already used the step above.
        if let LoadStep::RequestingAccess(pending) = &mut *step.borrow_mut() {
            *pending = Some(request);
        }
        Box::new(LoadCancellation(step))
    }
}

/// How far a load has come. Dropping a step cancels it.
enum LoadStep {
    /// None only between starting the request and holding it.
    RequestingAccess(Option<ImapAccessRequest>),
    /// Dropping the handle cancels the transfer and closes its connection.
    Transferring {
        _transfer: LoadHandle,
    },
    Cancelled,
}

/// Cancels its load when dropped, at whichever step the load has reached.
pub struct LoadCancellation(Rc<RefCell<LoadStep>>);

impl CancelsLoadOnDrop for LoadCancellation {}

impl Drop for LoadCancellation {
    fn drop(&mut self) {
        // The step is dropped after the cell is free, so that the step's own
        // completion callback can still use it.
        let step = std::mem::replace(&mut *self.0.borrow_mut(), LoadStep::Cancelled);
        drop(step);
    }
}

/// Reports a finished transfer the way the window stores it.
fn load_result(outcome: LoadOutcome) -> LoadResult {
    match outcome {
        LoadOutcome::Loaded(batch) => LoadResult::Received(batch),
        LoadOutcome::Failed(failure) => LoadResult::Failed(LoadFailure::Server(failure)),
        LoadOutcome::Cancelled => LoadResult::Cancelled,
        LoadOutcome::WorkerStopped => LoadResult::Failed(LoadFailure::WorkerStopped),
    }
}
