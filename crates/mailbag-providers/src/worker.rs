// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mail worker: a thread with its own GLib context that runs one load at
//! a time and writes what it received into the store. It touches no widget.

use crate::{
    LoadEvent, LoadFailure, LoadResult, LoadTarget,
    cycle::synchronize_folder,
    gmail::list_gmail_folders,
    imap::list_imap_folders,
    microsoft365::list_microsoft365_folders,
    renewal::AccessRenewal,
    store_load::{BatchWriter, store_folder_list},
};
use futures_util::{
    FutureExt,
    future::{self, Either},
};
use goa_adapter::{GraphAccess, ImapAccess};
use mailbag_domain::{AccountId, Folder, install_panic_hook, take_panic};
use mailbag_store::Store;
use std::{cell::RefCell, panic::AssertUnwindSafe, pin::pin, sync::Arc, thread};

/// The mail worker. It runs one load at a time for the selected account and
/// keeps GTK's context free of mail access and of the store's writes. Its
/// thread starts with the first load, and again if it ever stops.
pub(crate) struct MailWorker {
    pub(crate) loads: RefCell<Option<async_channel::Sender<LoadRequest>>>,
    store: Arc<Store>,
}

/// Which load sequence to run, with the access it needs. The access and the
/// sequence travel together, so they cannot disagree.
pub(crate) enum LoadKind {
    GenericImap(ImapAccess),
    Gmail(ImapAccess),
    /// `service_url` is Microsoft Graph's address, or a test service's; the
    /// renewal is used once after the service refused the token.
    Microsoft365 {
        access: GraphAccess,
        service_url: String,
        renewal: AccessRenewal,
    },
    /// Panics inside the load, as a hostile message could make a parser do.
    #[cfg(test)]
    PanicsForTest(AccountId),
}

impl LoadKind {
    /// The account the load reads.
    fn account_id(&self) -> &AccountId {
        match self {
            Self::GenericImap(access) | Self::Gmail(access) => &access.account_id,
            Self::Microsoft365 { access, .. } => &access.account_id,
            #[cfg(test)]
            Self::PanicsForTest(account_id) => account_id,
        }
    }
}

pub(crate) struct LoadRequest {
    kind: LoadKind,
    target: LoadTarget,
    /// Closed when the caller cancels or drops the load.
    cancelled: async_channel::Receiver<()>,
    /// Unbounded, so the final result is never dropped behind a batch
    /// event the caller has not read yet.
    events: async_channel::Sender<LoadEvent>,
}

/// Cancels its load when dropped, which closes the connection.
pub(crate) struct LoadHandle {
    _cancel: async_channel::Sender<()>,
}

impl MailWorker {
    pub(crate) fn new(store: Arc<Store>) -> Self {
        Self {
            loads: RefCell::new(None),
            store,
        }
    }

    /// Loads `target` of the account the access data names. `on_event`
    /// runs on the calling GLib context for each stored batch, then once
    /// with the end, even when the worker stops, so a load always ends and
    /// the refresh actions become available again.
    pub(crate) fn start_load(
        &self,
        kind: LoadKind,
        target: LoadTarget,
        on_event: impl FnMut(LoadEvent) + 'static,
    ) -> LoadHandle {
        let account_id = kind.account_id().clone();
        let record_name = target.record_name();
        let (cancel, cancelled) = async_channel::bounded(1);
        let (sender, events) = async_channel::unbounded();
        let request = LoadRequest {
            kind,
            target,
            cancelled,
            events: sender,
        };
        // The worker's queue is unbounded, so sending cannot block GTK.
        let accepted = self.queue().try_send(request).is_ok();
        glib::MainContext::ref_thread_default().spawn_local(report_events(
            account_id,
            record_name,
            accepted.then_some(events),
            on_event,
        ));
        LoadHandle { _cancel: cancel }
    }

    /// The running worker's queue, starting its thread when there is none or
    /// when the last one stopped, which closed its queue.
    fn queue(&self) -> async_channel::Sender<LoadRequest> {
        let mut loads = self.loads.borrow_mut();
        if let Some(running) = loads.as_ref().filter(|loads| !loads.is_closed()) {
            return running.clone();
        }
        let (sender, requests) = async_channel::unbounded();
        // The application's subscriber is global; a test's belongs to the
        // thread that starts the worker (specs/003-logging/research.md §8).
        let record = tracing::dispatcher::get_default(Clone::clone);
        let store = self.store.clone();
        thread::Builder::new()
            .name("mailbag-mail".to_owned())
            .spawn(move || {
                tracing::dispatcher::with_default(&record, || run_worker(&requests, &store))
            })
            .expect("start the mail worker thread");
        *loads = Some(sender.clone());
        sender
    }
}

/// Passes the load's events on until it ends. A panic inside a load ends
/// only that load; a worker thread that stopped anyway, for example on a
/// panic while dropping a cancelled load, leaves no end behind, and the
/// window still hears that the load is over.
pub(crate) async fn report_events(
    account_id: AccountId,
    record_name: &'static str,
    events: Option<async_channel::Receiver<LoadEvent>>,
    mut on_event: impl FnMut(LoadEvent),
) {
    if let Some(events) = events {
        while let Ok(event) = events.recv().await {
            let finished = matches!(event, LoadEvent::Finished(_));
            on_event(event);
            if finished {
                return;
            }
        }
    }
    on_event(LoadEvent::Finished(
        LoadFailure::WorkerStopped(None).give_up(&account_id, record_name),
    ));
}

/// Runs loads until the last worker handle is dropped.
fn run_worker(requests: &async_channel::Receiver<LoadRequest>, store: &Store) {
    install_panic_hook();
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                while let Ok(request) = requests.recv().await {
                    let outcome = run_load(
                        request.kind,
                        request.target,
                        store,
                        &request.cancelled,
                        &request.events,
                    )
                    .await;
                    request.events.try_send(LoadEvent::Finished(outcome)).ok();
                }
            });
        })
        .expect("the mail worker owns its GLib context");
}

/// Runs one load and its writes until they finish or the caller cancels the
/// load.
async fn run_load(
    kind: LoadKind,
    target: LoadTarget,
    store: &Store,
    cancelled: &async_channel::Receiver<()>,
    events: &async_channel::Sender<LoadEvent>,
) -> LoadResult {
    let mut load = Box::pin(load_catching_panics(kind, target, store, cancelled, events));
    match future::select(&mut load, pin!(cancelled.recv())).await {
        Either::Left((outcome, _)) => outcome,
        Either::Right(_) => {
            // Dropping the unfinished load closes its connection, before the
            // outcome tells the window that the load has ended.
            drop(load);
            LoadResult::Cancelled
        }
    }
}

/// Runs the folder list's load and its write, or a folder's cycle. A panic
/// inside ends this load as a failure that carries the panic's message and
/// place, and the worker goes on with the next load
/// (specs/006-error-handling FR-014).
async fn load_catching_panics(
    kind: LoadKind,
    target: LoadTarget,
    store: &Store,
    cancelled: &async_channel::Receiver<()>,
    events: &async_channel::Sender<LoadEvent>,
) -> LoadResult {
    let account_id = kind.account_id().clone();
    let record_name = target.record_name();
    let load = async move {
        match target {
            LoadTarget::FolderList => {
                let account = kind.account_id().clone();
                // A load cancelled while it waited for the store writes
                // nothing (specs/007-mail-storage/research.md §6).
                Ok(store_folder_list(
                    store,
                    &account,
                    list_folders(kind).await?,
                    || cancelled.is_closed(),
                ))
            }
            LoadTarget::Mailbox(folder) => {
                synchronize_folder(kind, BatchWriter::new(store, folder, cancelled, events)).await
            }
        }
    };
    // A panic in the store rolled its transaction back, and no other state
    // outlives a load, so nothing the panic interrupted is used again. The
    // payload is not read: the hook already kept the message.
    match AssertUnwindSafe(load).catch_unwind().await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(failure)) => failure.give_up(&account_id, record_name),
        Err(_) => LoadFailure::WorkerStopped(take_panic()).give_up(&account_id, record_name),
    }
}

// The kind is read once, in the function below and in the cycle, to choose
// the sequence; no sequence asks about the provider again (004 plan,
// decision D1).

/// The provider's folder-list sequence.
async fn list_folders(kind: LoadKind) -> Result<Vec<Folder>, LoadFailure> {
    match kind {
        LoadKind::GenericImap(access) => list_imap_folders(access).await.map_err(LoadFailure::Imap),
        LoadKind::Gmail(access) => list_gmail_folders(access).await.map_err(LoadFailure::Imap),
        LoadKind::Microsoft365 {
            access,
            service_url,
            ..
        } => list_microsoft365_folders(access, &service_url)
            .await
            .map_err(LoadFailure::MicrosoftGraph),
        #[cfg(test)]
        LoadKind::PanicsForTest(_) => panic!("a load panicked on purpose"),
    }
}
