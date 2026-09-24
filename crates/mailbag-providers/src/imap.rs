// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Generic IMAP load: the newest Inbox messages with the text each row
//! needs, over one connection, asking the server for nothing beyond RFC 3501.

use crate::{
    batch::{BATCH_SIZE, ReceivedBatch},
    imap_batch::{imap_account, load_batch_from_rows},
};
use goa_adapter::ImapAccess;
use mailbag_imap::{ImapError, InboxReader, OpenOptions, RowItems};

pub(crate) async fn load_imap_inbox(access: ImapAccess) -> Result<ReceivedBatch, ImapError> {
    let account_id = access.account_id.clone();
    let mut reader = InboxReader::open(imap_account(access), OpenOptions::default()).await?;
    let listed = reader.fetch_rows(RowItems::Standard, BATCH_SIZE).await?;
    load_batch_from_rows(&mut reader, listed, account_id).await
}
