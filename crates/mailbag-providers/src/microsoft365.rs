// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Microsoft 365 folder list, and a message's list fields as the
//! application keeps them. Its folders synchronize through the Microsoft 365
//! cycle; the service renders texts itself, so no MIME is read
//! (specs/005-microsoft-graph-integration/spec.md FR-005).

use crate::folders::graph_folders;
use goa_adapter::GraphAccess;
use mailbag_content::display_names;
use mailbag_domain::{DisplayFields, Folder};
use mailbag_graph::{GraphError, GraphMessage, Mailbox, list_folders};

pub(crate) async fn list_microsoft365_folders(
    access: GraphAccess,
    service_url: &str,
) -> Result<Vec<Folder>, GraphError> {
    let listed = list_folders(service_url, &access.access_token).await?;
    Ok(graph_folders(listed))
}

/// The message's subject, sender and recipients for the list and the reader;
/// its identifier and read state go to the record at debug.
pub(crate) fn received_fields(message: &GraphMessage) -> DisplayFields {
    tracing::debug_span!("message", immutable_id = message.immutable_id.as_str()).in_scope(|| {
        tracing::debug!(
            received_unix = message.received_unix,
            is_read = message.is_read,
            "message received"
        );
    });
    DisplayFields {
        subject: message.subject.clone(),
        from: display_names(message.from.iter().map(name_and_address)),
        to: display_names(message.to.iter().map(name_and_address)),
    }
}

fn name_and_address(mailbox: &Mailbox) -> (Option<&str>, Option<&str>) {
    (mailbox.name.as_deref(), mailbox.address.as_deref())
}
