// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mail worker: a thread with its own GLib context that runs one load at
//! a time. It touches no widget.

use crate::{
    LoadFailure, LoadResult, gmail::load_gmail_inbox, imap::load_imap_inbox,
    microsoft365::load_microsoft365_inbox,
};
use futures_util::{
    FutureExt,
    future::{self, Either},
};
use goa_adapter::{GraphAccess, ImapAccess};
use std::{
    cell::{Cell, RefCell},
    panic::{self, AssertUnwindSafe},
    pin::pin,
    sync::Once,
    thread,
};

thread_local! {
    /// The last panic on this thread as `message at file:line`, written by the
    /// panic hook and taken by the load the panic stopped.
    static LAST_PANIC: Cell<Option<String>> = const { Cell::new(None) };
}

/// The mail worker. It runs one load at a time for the selected account and
/// keeps GTK's context free of mail access. Its thread starts with the first
/// load, and again if it ever stops.
#[derive(Default)]
pub(crate) struct MailWorker {
    pub(crate) loads: RefCell<Option<async_channel::Sender<LoadRequest>>>,
}

/// Which load sequence to run, with the access it needs. The access and the
/// sequence travel together, so they cannot disagree.
pub(crate) enum LoadKind {
    GenericImap(ImapAccess),
    Gmail(ImapAccess),
    /// `service_url` is Microsoft Graph's address, or a test service's.
    Microsoft365 {
        access: GraphAccess,
        service_url: String,
    },
    /// Panics inside the load, as a hostile message could make a parser do.
    #[cfg(test)]
    PanicsForTest,
}

pub(crate) struct LoadRequest {
    kind: LoadKind,
    /// Closed when the caller cancels or drops the load.
    cancelled: async_channel::Receiver<()>,
    outcome: async_channel::Sender<LoadResult>,
}

/// Cancels its load when dropped, which closes the connection.
pub(crate) struct LoadHandle {
    _cancel: async_channel::Sender<()>,
}

impl MailWorker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Loads the Inbox of the account the access data names. `on_finished`
    /// runs once on the calling GLib context, even when the worker stops,
    /// so a load always ends and Refresh Inbox becomes available again.
    pub(crate) fn load_inbox(
        &self,
        kind: LoadKind,
        on_finished: impl FnOnce(LoadResult) + 'static,
    ) -> LoadHandle {
        let (cancel, cancelled) = async_channel::bounded(1);
        let (sender, outcome) = async_channel::bounded(1);
        let request = LoadRequest {
            kind,
            cancelled,
            outcome: sender,
        };
        // The worker's queue is unbounded, so sending cannot block GTK.
        let accepted = self.queue().try_send(request).is_ok();
        glib::MainContext::ref_thread_default()
            .spawn_local(report_outcome(accepted.then_some(outcome), on_finished));
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
        thread::Builder::new()
            .name("mailbag-mail".to_owned())
            .spawn(move || tracing::dispatcher::with_default(&record, || run_worker(&requests)))
            .expect("start the mail worker thread");
        *loads = Some(sender.clone());
        sender
    }
}

/// Reports how the load ended. A panic inside a load ends only that load; a
/// worker thread that stopped anyway, for example on a panic while dropping a
/// cancelled load, leaves no outcome behind, and the window still hears that
/// the load is over.
pub(crate) async fn report_outcome(
    outcome: Option<async_channel::Receiver<LoadResult>>,
    on_finished: impl FnOnce(LoadResult),
) {
    let reported = match outcome {
        Some(outcome) => outcome.recv().await.ok(),
        None => None,
    };
    on_finished(reported.unwrap_or(LoadResult::Failed(LoadFailure::WorkerStopped(None))));
}

/// Runs loads until the last worker handle is dropped.
fn run_worker(requests: &async_channel::Receiver<LoadRequest>) {
    install_panic_hook();
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                while let Ok(request) = requests.recv().await {
                    let outcome = run_load(request.kind, &request.cancelled).await;
                    request.outcome.try_send(outcome).ok();
                }
            });
        })
        .expect("the mail worker owns its GLib context");
}

/// Runs one provider's load until it finishes or the caller cancels it.
async fn run_load(kind: LoadKind, cancelled: &async_channel::Receiver<()>) -> LoadResult {
    let mut load = Box::pin(load_catching_panics(kind));
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

/// Runs the provider's load sequence. A panic inside it ends this load as a
/// failure that carries the panic's message and place, and the worker goes
/// on with the next load (specs/006-error-handling FR-014).
async fn load_catching_panics(kind: LoadKind) -> LoadResult {
    // The kind is read once, here, to choose the sequence; no sequence asks
    // about the provider again (004 plan, decision D1).
    let load = async move {
        match kind {
            LoadKind::GenericImap(access) => {
                load_imap_inbox(access).await.map_err(LoadFailure::Imap)
            }
            LoadKind::Gmail(access) => load_gmail_inbox(access).await.map_err(LoadFailure::Imap),
            LoadKind::Microsoft365 {
                access,
                service_url,
            } => load_microsoft365_inbox(access, &service_url)
                .await
                .map_err(LoadFailure::MicrosoftGraph),
            #[cfg(test)]
            LoadKind::PanicsForTest => panic!("a load panicked on purpose"),
        }
    };
    // No state outlives a load, so nothing the panic interrupted is used
    // again. The payload is not read: the hook already kept the message.
    match AssertUnwindSafe(load).catch_unwind().await {
        Ok(Ok(batch)) => LoadResult::Received(batch),
        Ok(Err(failure)) => LoadResult::Failed(failure),
        Err(_) => LoadResult::Failed(LoadFailure::WorkerStopped(LAST_PANIC.take())),
    }
}

/// Keeps each panic's message and place on the thread where it happens, then
/// lets the previous hook report it to the error stream as before. The hook
/// serves the whole process, so it is installed once.
fn install_panic_hook() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let previous_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let message = info.payload_as_str().unwrap_or("panic");
            let panic = match info.location() {
                Some(place) => format!("{message} at {}:{}", place.file(), place.line()),
                None => message.to_owned(),
            };
            LAST_PANIC.set(Some(panic));
            previous_hook(info);
        }));
    });
}
