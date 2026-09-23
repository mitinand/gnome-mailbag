// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mail worker: a thread with its own GLib context that runs one load at
//! a time. It touches no widget.

use crate::{LoadOutcome, MailProvider, gmail::load_gmail_inbox, imap::load_imap_inbox};
use futures_util::future::{self, Either};
use goa_adapter::ImapAccess;
use std::{cell::RefCell, pin::pin, thread};

/// The mail worker. It runs one load at a time for the selected account and
/// keeps GTK's context free of mail access. Its thread starts with the first
/// load, and again if it ever stops.
#[derive(Default)]
pub struct MailWorker {
    pub(crate) loads: RefCell<Option<async_channel::Sender<LoadRequest>>>,
}

pub(crate) struct LoadRequest {
    access: ImapAccess,
    provider: MailProvider,
    /// Closed when the caller cancels or drops the load.
    cancelled: async_channel::Receiver<()>,
    outcome: async_channel::Sender<LoadOutcome>,
}

/// Cancels its load when dropped, which closes the connection.
pub struct LoadHandle {
    _cancel: async_channel::Sender<()>,
}

impl MailWorker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads the Inbox of the account the access data names. `on_finished`
    /// runs once on the calling GLib context, even when the worker stops,
    /// so a load always ends and Refresh Inbox becomes available again.
    pub fn load_inbox(
        &self,
        access: ImapAccess,
        provider: MailProvider,
        on_finished: impl FnOnce(LoadOutcome) + 'static,
    ) -> LoadHandle {
        let (cancel, cancelled) = async_channel::bounded(1);
        let (sender, outcome) = async_channel::bounded(1);
        let request = LoadRequest {
            access,
            provider,
            cancelled,
            outcome: sender,
        };
        // The worker's queue is unbounded, so sending cannot block GTK.
        let accepted = self.worker().try_send(request).is_ok();
        glib::MainContext::ref_thread_default()
            .spawn_local(report_outcome(accepted.then_some(outcome), on_finished));
        LoadHandle { _cancel: cancel }
    }

    /// The running worker's queue, starting its thread when there is none or
    /// when the last one stopped, which closed its queue.
    fn worker(&self) -> async_channel::Sender<LoadRequest> {
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

/// Reports how the load ended. A worker that stopped, for example because its
/// thread panicked on hostile input, leaves no outcome behind; the window
/// still hears that the load is over.
pub(crate) async fn report_outcome(
    outcome: Option<async_channel::Receiver<LoadOutcome>>,
    on_finished: impl FnOnce(LoadOutcome),
) {
    let reported = match outcome {
        Some(outcome) => outcome.recv().await.ok(),
        None => None,
    };
    on_finished(reported.unwrap_or(LoadOutcome::WorkerStopped));
}

/// Runs loads until the last worker handle is dropped.
fn run_worker(requests: &async_channel::Receiver<LoadRequest>) {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                while let Ok(request) = requests.recv().await {
                    let outcome =
                        run_load(request.access, request.provider, &request.cancelled).await;
                    request.outcome.try_send(outcome).ok();
                }
            });
        })
        .expect("the mail worker owns its GLib context");
}

/// Runs one provider's load until it finishes or the caller cancels it.
async fn run_load(
    access: ImapAccess,
    provider: MailProvider,
    cancelled: &async_channel::Receiver<()>,
) -> LoadOutcome {
    // The provider is read once, here, to choose the sequence; neither
    // sequence asks about it again (004 plan, decision D1).
    let mut load = Box::pin(async move {
        match provider {
            MailProvider::GenericImap => load_imap_inbox(access).await,
            MailProvider::Gmail => load_gmail_inbox(access).await,
        }
    });
    match future::select(&mut load, pin!(cancelled.recv())).await {
        Either::Left((Ok(batch), _)) => LoadOutcome::Loaded(batch),
        Either::Left((Err(failure), _)) => LoadOutcome::Failed(failure),
        Either::Right(_) => {
            // Dropping the unfinished load closes its connection, before the
            // outcome tells the window that the load has ended.
            drop(load);
            LoadOutcome::Cancelled
        }
    }
}
