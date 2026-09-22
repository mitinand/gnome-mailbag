// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Gmail load: the same steps as a Generic IMAP load, with the three
//! things Gmail offers beyond RFC 3501 — UTF-8 names, a named client, and its
//! own message identifier and labels on every row.

use crate::{
    batch::{ReceivedBatch, ServerFailure},
    load::{load_batch_from_rows, server_account},
};
use goa_adapter::ImapAccess;
use mailbag_imap::{ClientIdentity, GmailRow, InboxReader, OpenOptions, RowItems};
use std::collections::BTreeMap;

pub(crate) async fn load_gmail_inbox(access: ImapAccess) -> Result<ReceivedBatch, ServerFailure> {
    let account_id = access.account_id.clone();
    let mut reader = InboxReader::open(server_account(access), gmail_options()).await?;
    let mut listed = reader.fetch_rows(RowItems::WithGmailAttributes).await?;
    let gmail_rows = take_gmail_rows(&mut listed.rows);
    let mut batch = load_batch_from_rows(&mut reader, listed, account_id).await?;
    for message in &mut batch.messages {
        message.gmail = gmail_rows.get(&message.uid).cloned();
    }
    Ok(batch)
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

/// Takes Gmail's fields off the rows and writes each one to the record, so
/// that the batch travels with them and the record shows what arrived. Label
/// names are folder-like names and stay at debug (specs/003-logging FR-010).
fn take_gmail_rows(rows: &mut [mailbag_imap::MessageRow]) -> BTreeMap<u32, GmailRow> {
    let mut gmail_rows = BTreeMap::new();
    for row in rows {
        let Some(gmail) = row.gmail.take() else {
            continue;
        };
        tracing::debug_span!("message", uid = row.uid).in_scope(|| {
            tracing::debug!(
                gmail_message_id = gmail.message_id,
                labels = ?gmail.labels,
                "Gmail's own fields of this message"
            );
        });
        gmail_rows.insert(row.uid, gmail);
    }
    gmail_rows
}
