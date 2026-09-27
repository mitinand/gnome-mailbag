// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Gmail loads: the same steps as the Generic IMAP loads, with what Gmail
//! offers beyond RFC 3501 that they ask for — a named client, its own message
//! identifier and labels on every row — and without the container its system
//! labels are listed under.

use crate::{
    batch::{BATCH_SIZE, ReceivedBatch},
    folders::gmail_folders,
    imap_batch::{imap_account, load_batch_from_rows},
};
use goa_adapter::ImapAccess;
use mailbag_domain::{Folder, FolderRef};
use mailbag_imap::{
    ClientIdentity, ImapError, MailboxReader, MessageRow, OpenOptions, RowItems, list_mailboxes,
};

pub(crate) async fn list_gmail_folders(access: ImapAccess) -> Result<Vec<Folder>, ImapError> {
    let listed = list_mailboxes(imap_account(access), gmail_options()).await?;
    Ok(gmail_folders(&listed))
}

pub(crate) async fn load_gmail_mailbox(
    access: ImapAccess,
    folder: FolderRef,
) -> Result<ReceivedBatch, ImapError> {
    let mut reader =
        MailboxReader::open(imap_account(access), gmail_options(), &folder.identity).await?;
    let listed = reader
        .fetch_rows(RowItems::WithGmailAttributes, BATCH_SIZE)
        .await?;
    log_gmail_rows(&listed.rows);
    load_batch_from_rows(&mut reader, listed, folder).await
}

/// What Gmail is asked for beyond a Generic IMAP sign-in. Google asks clients
/// to name themselves and to leave a contact address
/// (specs/004-gmail-integration/research.md §6).
fn gmail_options() -> OpenOptions {
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

/// Writes Gmail's fields of each row to the record; the rows carry them on
/// into the batch. Label names are folder-like names and stay at debug
/// (specs/003-logging FR-010).
fn log_gmail_rows(rows: &[MessageRow]) {
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
