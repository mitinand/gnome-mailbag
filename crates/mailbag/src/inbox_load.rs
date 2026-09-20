// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Loads one account's Inbox on the mail worker: a thread with its own GLib
//! context that joins the protocol and content crates. It touches no widget.

#[cfg(test)]
mod tests;

use crate::inbox::{LoadFailure, ReceivedBatch, ReceivedContent, ReceivedMessage};
use adw::glib;
use futures_util::future::{self, Either};
use goa_adapter::{ImapAccess, ImapEncryption};
use mailbag_content::{
    ContentExplanation, MimePart, TextSelection, decode_display_fields, decode_text_part,
    join_message_text, select_text_parts,
};
use mailbag_imap::{
    Encryption, ImapAccount, ImapFailure, InboxReader, MessagePart, MessageText, TextParts,
    TextRequest,
};
use std::{cell::RefCell, collections::BTreeMap, pin::pin, thread};

/// How one load ended.
#[derive(Debug)]
pub enum LoadOutcome {
    Loaded(ReceivedBatch),
    Failed(LoadFailure),
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
        thread::Builder::new()
            .name("mailbag-mail".to_owned())
            .spawn(move || run_worker(&requests))
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
async fn load_inbox_batch(access: ImapAccess) -> Result<ReceivedBatch, LoadFailure> {
    let account_id = access.account_id.clone();
    let mut reader = InboxReader::open(server_account(access)).await?;
    let rows = reader.fetch_rows().await?;
    let window = rows.len();
    let uids: Vec<u32> = rows.iter().map(|row| row.uid).collect();
    let structures = reader.fetch_structures(&uids).await?;

    let mut selections = BTreeMap::new();
    let mut requests = Vec::new();
    for (uid, structure) in &structures {
        let Some(part) = structure else {
            continue;
        };
        let selection = select_text_parts(&describe_part(part));
        if let TextSelection::Parts(sections) = &selection {
            requests.push(TextRequest {
                uid: *uid,
                parts: text_parts(part, sections),
            });
        }
        selections.insert(*uid, selection);
    }

    // Each message's text is decoded as it arrives, so no raw MIME is kept.
    let mut texts = BTreeMap::new();
    reader
        .fetch_text(requests, |uid, text| {
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
                fields: decode_display_fields(&row.list_headers),
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
    })
}

fn server_account(access: ImapAccess) -> ImapAccount {
    ImapAccount {
        host: access.host,
        login: access.login,
        password: access.password,
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
