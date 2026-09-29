// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Gmail folder list, and what Gmail is asked for beyond RFC 3501: a
//! named client, and without the container its system labels are listed
//! under. Its label folders synchronize through the IMAP cycle.

use crate::{folders::gmail_folders, imap_texts::imap_account};
use goa_adapter::ImapAccess;
use mailbag_domain::Folder;
use mailbag_imap::{ClientIdentity, ImapError, MessageRow, OpenOptions, list_mailboxes};

pub(crate) async fn list_gmail_folders(access: ImapAccess) -> Result<Vec<Folder>, ImapError> {
    let listed = list_mailboxes(imap_account(access), gmail_options()).await?;
    Ok(gmail_folders(&listed))
}

/// What Gmail is asked for beyond a Generic IMAP sign-in. Google asks clients
/// to name themselves and to leave a contact address
/// (specs/004-gmail-integration/research.md §6).
pub(crate) fn gmail_options() -> OpenOptions {
    OpenOptions {
        client_identity: Some(ClientIdentity {
            name: "Mailbag".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            vendor: "Andrey Mitin".to_owned(),
            contact: "mitin.andrey@outlook.com".to_owned(),
            support_url: env!("CARGO_PKG_REPOSITORY").to_owned(),
        }),
    }
}

/// Writes Gmail's fields of each received row to the record
/// (specs/004-gmail-integration FR-008). Label names are folder-like names
/// and stay at debug (specs/003-logging FR-010).
pub(crate) fn log_gmail_rows(rows: &[MessageRow]) {
    for row in rows {
        let Some(gmail) = &row.gmail else {
            continue;
        };
        tracing::debug_span!("message", uid = row.uid).in_scope(|| {
            tracing::debug!(
                gmail_message_id = gmail.message_id,
                labels = ?gmail.labels,
                "Gmail's own fields of this message"
            );
        });
    }
}
