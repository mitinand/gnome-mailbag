// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Loads one account's Inbox from its provider. This crate joins Online
//! Accounts, the protocol crate and the content crate; it owns no widget and
//! no application state.

mod batch;
mod failure;
mod gmail;
mod imap;
mod imap_batch;
mod microsoft365;
mod worker;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/support/record.rs"]
mod test_record;
#[cfg(test)]
mod tests;

pub use batch::{CancelsLoadOnDrop, LoadResult, MessageIdentity, ReceivedBatch, ReceivedMessage};

use batch::LoadFailure;
use goa_adapter::{AccessError, AccessRequest, AccountId, GoaAdapter, ImapAccess};
use std::{cell::RefCell, rc::Rc};
use worker::{LoadHandle, LoadKind, MailWorker};

/// Which load sequence an account needs. The window turns the account's
/// `AccountProvider` into this; that type does not reach this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailProvider {
    GenericImap,
    Gmail,
    Microsoft365,
}

/// Where Microsoft 365 mail is read.
const MICROSOFT_GRAPH: &str = "https://graph.microsoft.com/v1.0";

/// Starts one account's Inbox load and reports how it ended. The window loads
/// with Online Accounts and the mail worker; the graphical test reports
/// results without a server.
pub trait LoadsInbox {
    /// Reports the result once, on the calling GLib context. The returned
    /// step cancels the load when it is dropped.
    fn start_load(
        &self,
        account_id: &AccountId,
        provider: MailProvider,
        report: Box<dyn FnOnce(LoadResult)>,
    ) -> Box<dyn CancelsLoadOnDrop>;
}

/// Loads an Inbox with the account's Online Accounts settings and credential,
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
        provider: MailProvider,
        report: Box<dyn FnOnce(LoadResult)>,
    ) -> Box<dyn CancelsLoadOnDrop> {
        let transfer = Rc::new(RefCell::new(None));
        let started_transfer = transfer.clone();
        let worker = self.worker.clone();
        let requested_account = account_id.clone();
        let start_transfer = move |access: Result<LoadKind, AccessError>| match access {
            Ok(kind) => {
                *started_transfer.borrow_mut() = Some(worker.load_inbox(kind, report));
            }
            // The request was cancelled by an exclusion or by quitting.
            Err(AccessError::Cancelled) => report(LoadResult::Cancelled),
            // The load ends here, on GTK's context, before the worker is
            // involved.
            Err(error) => report(LoadFailure::OnlineAccounts(error).give_up(&requested_account)),
        };
        let request = match provider {
            MailProvider::GenericImap => request_imap_load(
                &self.accounts,
                account_id,
                LoadKind::GenericImap,
                start_transfer,
            ),
            MailProvider::Gmail => {
                request_imap_load(&self.accounts, account_id, LoadKind::Gmail, start_transfer)
            }
            MailProvider::Microsoft365 => {
                self.accounts
                    .request_graph_access(account_id, move |access| {
                        start_transfer(access.map(|access| {
                            tracing::info!("Online Accounts gave the access token");
                            LoadKind::Microsoft365 {
                                access,
                                service_url: MICROSOFT_GRAPH.to_owned(),
                            }
                        }))
                    })
            }
        };
        Box::new(LoadCancellation {
            _access: request,
            transfer,
        })
    }
}

/// Asks Online Accounts for an IMAP account's settings and credential;
/// `load_kind` names the IMAP sequence that runs with them.
fn request_imap_load(
    accounts: &GoaAdapter,
    account_id: &AccountId,
    load_kind: fn(ImapAccess) -> LoadKind,
    start_transfer: impl FnOnce(Result<LoadKind, AccessError>) + 'static,
) -> AccessRequest {
    accounts.request_imap_access(account_id, move |access| {
        start_transfer(access.map(|access| {
            tracing::info!(
                encryption = ?access.encryption,
                "Online Accounts gave the settings and credential"
            );
            load_kind(access)
        }))
    })
}

/// Cancels its load when dropped, at whichever step it has reached: the
/// Online Accounts request, or the transfer once the request answered.
struct LoadCancellation {
    /// Dropping it cancels a pending request; after the answer it is inert.
    _access: AccessRequest,
    /// The transfer, once Online Accounts answered. Dropping the handle cancels
    /// the transfer and closes its connection.
    transfer: Rc<RefCell<Option<LoadHandle>>>,
}

impl CancelsLoadOnDrop for LoadCancellation {}

impl Drop for LoadCancellation {
    fn drop(&mut self) {
        // Taken out of the cell first: the transfer's outcome arrives later on
        // this context, never inside this borrow.
        let transfer = self.transfer.borrow_mut().take();
        drop(transfer);
    }
}
