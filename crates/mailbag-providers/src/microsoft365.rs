// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Microsoft 365 loads: the folder list, and one request to Microsoft
//! Graph for the newest messages of one folder with their text, turned into
//! the batch every provider delivers. The service renders the text itself,
//! so no MIME is read (specs/005-microsoft-graph-integration/spec.md FR-005).

use crate::{
    batch::{BATCH_SIZE, ReceivedBatch, ReceivedMessage},
    folders::graph_folders,
};
use goa_adapter::GraphAccess;
use mailbag_content::display_names;
use mailbag_domain::{DisplayFields, Folder, FolderRef, IncompleteList, ReceivedContent};
use mailbag_graph::{GraphError, GraphMessage, Mailbox, list_folders, list_mailbox_messages};

pub(crate) async fn list_microsoft365_folders(
    access: GraphAccess,
    service_url: &str,
) -> Result<Vec<Folder>, GraphError> {
    let listed = list_folders(service_url, &access.access_token).await?;
    Ok(graph_folders(listed))
}

pub(crate) async fn load_microsoft365_mailbox(
    access: GraphAccess,
    service_url: &str,
    folder: FolderRef,
) -> Result<ReceivedBatch, GraphError> {
    let page = list_mailbox_messages(
        service_url,
        &access.access_token,
        &folder.identity,
        BATCH_SIZE,
    )
    .await?;
    // A full batch is complete however many messages the folder holds; only
    // a page the service cut short while offering more is not (spec FR-003).
    let cut_short = page.more_available && page.messages.len() < BATCH_SIZE as usize;
    Ok(ReceivedBatch {
        folder,
        messages: page.messages.into_iter().map(received_message).collect(),
        incomplete: cut_short.then_some(IncompleteList::MoreAvailable),
    })
}

fn received_message(message: GraphMessage) -> ReceivedMessage {
    tracing::debug_span!("message", immutable_id = message.immutable_id.as_str()).in_scope(|| {
        tracing::debug!(
            received_unix = message.received_unix,
            is_read = message.is_read,
            "message received"
        );
    });
    ReceivedMessage {
        fields: DisplayFields {
            subject: message.subject,
            from: display_names(message.from.iter().map(name_and_address)),
            to: display_names(message.to.iter().map(name_and_address)),
        },
        graph_id: message.immutable_id,
        internal_date: message.received_unix,
        seen: message.is_read,
        content: match message.body_text {
            Some(text) => ReceivedContent::Text(text),
            None => ReceivedContent::TextNotReturned,
        },
    }
}

fn name_and_address(mailbox: &Mailbox) -> (Option<&str>, Option<&str>) {
    (mailbox.name.as_deref(), mailbox.address.as_deref())
}
