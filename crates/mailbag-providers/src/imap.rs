// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Generic IMAP folder list, over one connection, asking the server for
//! nothing beyond RFC 3501 and the UTF-8 names it announces. Its folders
//! synchronize through the IMAP cycle.

use crate::{folders::imap_folders, imap_texts::imap_account};
use goa_adapter::ImapAccess;
use mailbag_domain::Folder;
use mailbag_imap::{ImapError, OpenOptions, list_mailboxes};

pub(crate) async fn list_imap_folders(access: ImapAccess) -> Result<Vec<Folder>, ImapError> {
    let listed = list_mailboxes(imap_account(access), OpenOptions::default()).await?;
    Ok(imap_folders(&listed))
}
