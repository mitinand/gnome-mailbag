// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Generic IMAP loads: the folder list, and the newest messages of one
//! folder with the text each row needs, each over one connection, asking the
//! server for nothing beyond RFC 3501 and the UTF-8 names it announces.

use crate::{
    batch::{BATCH_SIZE, ReceivedBatch},
    folders::imap_folders,
    imap_batch::{imap_account, load_batch_from_rows},
};
use goa_adapter::ImapAccess;
use mailbag_domain::{Folder, FolderRef};
use mailbag_imap::{ImapError, MailboxReader, OpenOptions, RowItems, list_mailboxes};

pub(crate) async fn list_imap_folders(access: ImapAccess) -> Result<Vec<Folder>, ImapError> {
    let listed = list_mailboxes(imap_account(access), OpenOptions::default()).await?;
    Ok(imap_folders(&listed))
}

pub(crate) async fn load_imap_mailbox(
    access: ImapAccess,
    folder: FolderRef,
) -> Result<ReceivedBatch, ImapError> {
    let mut reader = MailboxReader::open(
        imap_account(access),
        OpenOptions::default(),
        &folder.identity,
    )
    .await?;
    let listed = reader.fetch_rows(RowItems::Standard, BATCH_SIZE).await?;
    load_batch_from_rows(&mut reader, listed, folder).await
}
