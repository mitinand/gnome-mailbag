// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Loads one account's Inbox on the mail worker: a thread with its own GLib
//! context that joins the protocol and content crates. It touches no widget.

#[cfg(test)]
mod tests;

use crate::inbox::{
    CancelsLoadOnDrop, LoadFailure, LoadResult, ReceivedBatch, ReceivedContent, ReceivedMessage,
    ServerFailure,
};
use adw::glib;
use futures_util::future::{self, Either};
use goa_adapter::{
    AccountId, GoaAdapter, ImapAccess, ImapAccessError, ImapAccessRequest, ImapEncryption,
};
use mailbag_content::{
    ContentExplanation, MimePart, TextSelection, decode_display_fields, decode_text_part,
    join_message_text, select_text_parts,
};
use mailbag_imap::{
    Credential, Encryption, ImapAccount, ImapFailure, InboxReader, MessagePart, MessageText,
    OpenOptions, RowItems, TextParts, TextRequest,
};
use std::{cell::RefCell, collections::BTreeMap, pin::pin, rc::Rc, thread};

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

/// The mail worker. It runs one load at a time for the selected account and
/// keeps GTK's context free of mail access. Its thread starts with the first
/// load, and again if it ever stops.
#[derive(Default)]
pub struct MailWorker {
    loads: RefCell<Option<async_channel::Sender<LoadRequest>>>,
}

struct LoadRequest {
    access: ImapAccess,
    /// Closed when the caller cancels or drops the load.
    cancelled: async_channel::Receiver<()>,
    outcome: async_channel::Sender<LoadOutcome>,
}

/// Cancels its load when dropped, which closes the connection.
pub struct LoadHandle {
    _cancel: async_channel::Sender<()>,
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
        on_finished: impl FnOnce(LoadOutcome) + 'static,
    ) -> LoadHandle {
        let (cancel, cancelled) = async_channel::bounded(1);
        let (sender, outcome) = async_channel::bounded(1);
        let request = LoadRequest {
            access,
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
async fn report_outcome(
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
                    let outcome = run_load(request.access, &request.cancelled).await;
                    request.outcome.try_send(outcome).ok();
                }
            });
        })
        .expect("the mail worker owns its GLib context");
}

async fn run_load(access: ImapAccess, cancelled: &async_channel::Receiver<()>) -> LoadOutcome {
    let mut load = Box::pin(load_inbox_batch(access));
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

/// Reads the newest Inbox messages and decodes the text every row needs.
async fn load_inbox_batch(access: ImapAccess) -> Result<ReceivedBatch, ServerFailure> {
    let account_id = access.account_id.clone();
    let mut reader = InboxReader::open(server_account(access), OpenOptions::default()).await?;
    let listed = reader.fetch_rows(RowItems::Standard).await?;
    let rows = listed.rows;
    let window = rows.len();
    let uids: Vec<u32> = rows.iter().map(|row| row.uid).collect();
    let structures = reader.fetch_structures(&uids).await?;

    let mut selections = BTreeMap::new();
    let mut requests = Vec::new();
    for (uid, structure) in &structures {
        let Some(part) = structure else {
            continue;
        };
        let selection = tracing::debug_span!("message", uid)
            .in_scope(|| select_text_parts(&describe_part(part)));
        if let TextSelection::Parts(sections) = &selection {
            requests.push(TextRequest {
                uid: *uid,
                parts: text_parts(part, sections),
            });
        }
        selections.insert(*uid, selection);
    }

    // Each message's text is decoded as the reader reports it, and the raw
    // MIME is released with the request group it belongs to.
    let mut texts = BTreeMap::new();
    reader
        .fetch_text(requests, |uid, text| {
            let _message = tracing::debug_span!("message", uid).entered();
            // A message absent here disappeared from the Inbox during the load.
            if let Some(content) = decode_message_text(&text) {
                texts.insert(uid, content);
            }
        })
        .await?;

    let messages: Vec<ReceivedMessage> = rows
        .into_iter()
        .filter_map(|row| {
            let content = match structures.get(&row.uid)? {
                // The server could not describe this message.
                None => ReceivedContent::Explained(ContentExplanation::UnreadableStructure),
                Some(_) => match selections.get(&row.uid)? {
                    TextSelection::Explained(explanation) => {
                        ReceivedContent::Explained(explanation.clone())
                    }
                    // A message missing here disappeared during the load.
                    TextSelection::Parts(_) => texts.remove(&row.uid)?,
                },
            };
            Some(ReceivedMessage {
                uid: row.uid,
                fields: tracing::debug_span!("message", uid = row.uid)
                    .in_scope(|| decode_display_fields(&row.list_headers)),
                internal_date: row.internal_date,
                seen: row.seen,
                content,
            })
        })
        .collect();
    // Every message of a window that was not empty disappeared, for example
    // because another client moved them. Older mail outside the window may
    // still be there, so this is not an empty Inbox.
    if messages.is_empty() && window > 0 {
        return Err(ImapFailure::InboxChanged.into());
    }
    Ok(ReceivedBatch {
        account_id,
        uid_validity: reader.uid_validity(),
        messages,
        list_refusal: listed.refusal,
    })
}

fn server_account(access: ImapAccess) -> ImapAccount {
    ImapAccount {
        host: access.host,
        login: access.login,
        credential: Credential::Password(access.password),
        encryption: match access.encryption {
            ImapEncryption::ImplicitTls => Encryption::ImplicitTls,
            ImapEncryption::StartTls => Encryption::StartTls,
        },
    }
}

/// The part description the content rules walk. Both trees have the same
/// shape, so a selected part's path is its IMAP section number.
fn describe_part(part: &MessagePart) -> MimePart {
    MimePart {
        section: part.section.clone(),
        media_type: part.media_type.clone(),
        media_subtype: part.media_subtype.clone(),
        parameters: part.parameters.clone(),
        disposition: part.disposition.clone(),
        content_id: part.content_id.clone(),
        children: part.children.iter().map(describe_part).collect(),
    }
}

/// How the selected sections are read: a single-part message needs the
/// message header, a multipart leaf its own MIME header.
fn text_parts(root: &MessagePart, sections: &[Vec<u32>]) -> TextParts {
    match root.children.is_empty() {
        true => TextParts::SinglePartBody,
        false => TextParts::MultipartLeaves(sections.to_vec()),
    }
}

/// Decodes one message's received text, or explains why there is none.
/// `None` means the message disappeared from the Inbox.
fn decode_message_text(text: &MessageText) -> Option<ReceivedContent> {
    let parts = match text {
        MessageText::Received(parts) => parts,
        MessageText::NotReturned => {
            return Some(ReceivedContent::Explained(
                ContentExplanation::TextNotReturned,
            ));
        }
        MessageText::Disappeared => return None,
    };
    let decoded: Result<Vec<String>, ContentExplanation> = parts
        .iter()
        .map(|part| decode_text_part(&part.header, &part.body))
        .collect();
    Some(match decoded {
        Ok(texts) => ReceivedContent::Text(join_message_text(&texts)),
        // One unreadable part leaves no complete text to show.
        Err(explanation) => ReceivedContent::Explained(explanation),
    })
}
