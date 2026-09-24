// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Gmail load: the same steps as a Generic IMAP load, with the three
//! things Gmail offers beyond RFC 3501 — UTF-8 names, a named client, and its
//! own message identifier and labels on every row.

use crate::{
    batch::{BATCH_SIZE, ReceivedBatch},
    imap_batch::{imap_account, load_batch_from_rows},
};
use goa_adapter::ImapAccess;
use mailbag_imap::{ClientIdentity, ImapError, InboxReader, MessageRow, OpenOptions, RowItems};

pub(crate) async fn load_gmail_inbox(access: ImapAccess) -> Result<ReceivedBatch, ImapError> {
    let account_id = access.account_id.clone();
    let mut reader = InboxReader::open(imap_account(access), gmail_options()).await?;
    let listed = reader
        .fetch_rows(RowItems::WithGmailAttributes, BATCH_SIZE)
        .await?;
    log_gmail_rows(&listed.rows);
    load_batch_from_rows(&mut reader, listed, account_id).await
}

/// What Gmail is asked for beyond a Generic IMAP sign-in. Google asks clients
/// to name themselves and to leave a contact address
/// (specs/004-gmail-integration/research.md §6).
fn gmail_options() -> OpenOptions {
    OpenOptions {
        readable_names: true,
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
