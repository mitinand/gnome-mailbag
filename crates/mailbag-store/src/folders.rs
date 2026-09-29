// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows behind the store's operations: folders, the messages they hold
//! and the memberships between them (specs/009-synchronization/data-model.md).
//! Each function runs inside the caller's transaction or read.

use crate::content::{content_columns, content_from_columns};
use mailbag_domain::{
    AccountId, DisplayFields, Folder, FolderRef, FolderRole, FolderState, Message, MessageListRow,
    ReceivedContent,
};
use rusqlite::{Connection, Row, Transaction, params, types::Type};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Deletes the account's folders that `listed` does not hold, with their
/// memberships.
pub(crate) fn delete_unlisted_folders(
    transaction: &Transaction,
    account: &AccountId,
    listed: &[Folder],
) -> rusqlite::Result<()> {
    let listed: BTreeSet<&str> = listed
        .iter()
        .map(|folder| folder.identity.as_str())
        .collect();
    let stored: Vec<(i64, String)> = transaction
        .prepare("SELECT id, identity FROM folder WHERE account = ?1")?
        .query_map([account.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (folder_id, identity) in stored {
        if !listed.contains(identity.as_str()) {
            transaction.execute("DELETE FROM folder WHERE id = ?1", [folder_id])?;
        }
    }
    Ok(())
}

/// Updates the listed folders the store holds, keeping their state, and
/// inserts the others as never synchronized.
pub(crate) fn upsert_folders(
    transaction: &Transaction,
    account: &AccountId,
    folders: &[Folder],
) -> rusqlite::Result<()> {
    let mut upsert = transaction.prepare(
        "INSERT INTO folder (account, identity, name, parent, role, selectable, synchronized) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0) \
         ON CONFLICT (account, identity) DO UPDATE SET name = excluded.name, \
         parent = excluded.parent, role = excluded.role, selectable = excluded.selectable",
    )?;
    for folder in folders {
        upsert.execute(params![
            account.as_str(),
            folder.identity,
            folder.name,
            folder.parent,
            folder.role.map(role_code),
            folder.selectable,
        ])?;
    }
    Ok(())
}

/// The store's row of the folder; a folder the store does not hold fails
/// with no row found.
pub(crate) fn stored_folder_id(
    connection: &Connection,
    folder: &FolderRef,
) -> rusqlite::Result<i64> {
    connection.query_row(
        "SELECT id FROM folder WHERE account = ?1 AND identity = ?2",
        params![folder.account.as_str(), folder.identity],
        |row| row.get(0),
    )
}

/// Replaces the folder's memberships with the load's, stores each message
/// once by its identity with the load's fields, deletes the messages no
/// folder holds any more and marks the folder synchronized. A folder the
/// store does not hold fails with no row found.
pub(crate) fn write_mailbox(
    transaction: &Transaction,
    folder: &FolderRef,
    messages: &[Message],
) -> rusqlite::Result<()> {
    let folder_id = stored_folder_id(transaction, folder)?;
    transaction.execute("DELETE FROM membership WHERE folder = ?1", [folder_id])?;
    let mut upsert_message = transaction.prepare(
        "INSERT INTO message (account, identity, subject, sender, recipients, received, seen, \
         content_kind, content_detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT (account, identity) DO UPDATE SET subject = excluded.subject, \
         sender = excluded.sender, recipients = excluded.recipients, \
         received = excluded.received, seen = excluded.seen, \
         content_kind = excluded.content_kind, content_detail = excluded.content_detail \
         RETURNING id",
    )?;
    let mut insert_membership =
        transaction.prepare("INSERT INTO membership (folder, message) VALUES (?1, ?2)")?;
    for message in messages {
        let (content_kind, content_detail) = content_columns(&message.content);
        let message_id: i64 = upsert_message.query_row(
            params![
                folder.account.as_str(),
                message.identity,
                message.fields.subject,
                message.fields.from,
                message.fields.to,
                message.received_unix,
                message.seen,
                content_kind,
                content_detail,
            ],
            |row| row.get(0),
        )?;
        insert_membership.execute(params![folder_id, message_id])?;
    }
    delete_messages_without_folder(transaction, &folder.account)?;
    transaction.execute(
        "UPDATE folder SET synchronized = 1 WHERE id = ?1",
        [folder_id],
    )?;
    Ok(())
}

/// The folder's saved state.
pub(crate) fn read_folder_state(
    connection: &Connection,
    folder_id: i64,
) -> rusqlite::Result<FolderState> {
    connection.query_row(
        "SELECT server_position, synchronized FROM folder WHERE id = ?1",
        [folder_id],
        |row| {
            Ok(FolderState {
                server_position: row.get(0)?,
                synchronized: row.get(1)?,
            })
        },
    )
}

/// The identity and read state of every message the folder holds.
pub(crate) fn read_folder_identities(
    connection: &Connection,
    folder_id: i64,
) -> rusqlite::Result<HashMap<String, bool>> {
    connection
        .prepare(
            "SELECT identity, seen FROM membership JOIN message ON message.id = membership.message \
             WHERE membership.folder = ?1",
        )?
        .query_map([folder_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

/// Which of `identities` the account holds.
pub(crate) fn read_stored_identities(
    connection: &Connection,
    account: &AccountId,
    identities: &[String],
) -> rusqlite::Result<HashSet<String>> {
    let mut select =
        connection.prepare("SELECT 1 FROM message WHERE account = ?1 AND identity = ?2")?;
    let mut stored = HashSet::new();
    for identity in identities {
        if select.exists(params![account.as_str(), identity])? {
            stored.insert(identity.clone());
        }
    }
    Ok(stored)
}

/// Deletes the folder's memberships of messages proven gone from it.
pub(crate) fn delete_memberships(
    transaction: &Transaction,
    folder_id: i64,
    account: &AccountId,
    removed: &[String],
) -> rusqlite::Result<()> {
    let mut delete = transaction.prepare(
        "DELETE FROM membership WHERE folder = ?1 AND message = \
         (SELECT id FROM message WHERE account = ?2 AND identity = ?3)",
    )?;
    for identity in removed {
        delete.execute(params![folder_id, account.as_str(), identity])?;
    }
    Ok(())
}

/// Sets the read state of the account's messages.
pub(crate) fn set_read_states(
    transaction: &Transaction,
    account: &AccountId,
    read_states: &[(String, bool)],
) -> rusqlite::Result<()> {
    let mut update =
        transaction.prepare("UPDATE message SET seen = ?3 WHERE account = ?1 AND identity = ?2")?;
    for (identity, seen) in read_states {
        update.execute(params![account.as_str(), identity, seen])?;
    }
    Ok(())
}

/// Stores each full record once by its identity and relates it to the
/// folder. Its content replaces the stored one, except that a text not
/// downloaded never replaces a content another folder's cycle stored.
pub(crate) fn store_arrived(
    transaction: &Transaction,
    folder_id: i64,
    account: &AccountId,
    arrived: &[Message],
) -> rusqlite::Result<()> {
    let mut upsert_message = transaction.prepare(
        "INSERT INTO message (account, identity, subject, sender, recipients, received, seen, \
         content_kind, content_detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT (account, identity) DO UPDATE SET subject = excluded.subject, \
         sender = excluded.sender, recipients = excluded.recipients, \
         received = excluded.received, seen = excluded.seen, \
         content_kind = iif(excluded.content_kind = 'not_downloaded', content_kind, \
         excluded.content_kind), \
         content_detail = iif(excluded.content_kind = 'not_downloaded', content_detail, \
         excluded.content_detail) \
         RETURNING id",
    )?;
    let mut relate = transaction
        .prepare("INSERT OR IGNORE INTO membership (folder, message) VALUES (?1, ?2)")?;
    for message in arrived {
        let (content_kind, content_detail) = content_columns(&message.content);
        let message_id: i64 = upsert_message.query_row(
            params![
                account.as_str(),
                message.identity,
                message.fields.subject,
                message.fields.from,
                message.fields.to,
                message.received_unix,
                message.seen,
                content_kind,
                content_detail,
            ],
            |row| row.get(0),
        )?;
        relate.execute(params![folder_id, message_id])?;
    }
    Ok(())
}

/// Relates messages the account already holds to the folder, with their
/// listed read state. A message the store no longer holds is left out; the
/// folder's next cycle fetches it.
pub(crate) fn relate_known(
    transaction: &Transaction,
    folder_id: i64,
    account: &AccountId,
    known_arrived: &[(String, bool)],
) -> rusqlite::Result<()> {
    let mut relate = transaction.prepare(
        "INSERT OR IGNORE INTO membership (folder, message) \
         SELECT ?1, id FROM message WHERE account = ?2 AND identity = ?3",
    )?;
    for (identity, _) in known_arrived {
        relate.execute(params![folder_id, account.as_str(), identity])?;
    }
    set_read_states(transaction, account, known_arrived)
}

/// Saves the folder's state.
pub(crate) fn write_folder_state(
    transaction: &Transaction,
    folder_id: i64,
    state: &FolderState,
) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE folder SET server_position = ?2, synchronized = ?3 WHERE id = ?1",
        params![folder_id, state.server_position, state.synchronized],
    )?;
    Ok(())
}

/// Deletes the account's messages that no folder holds.
pub(crate) fn delete_messages_without_folder(
    transaction: &Transaction,
    account: &AccountId,
) -> rusqlite::Result<()> {
    transaction.execute(
        "DELETE FROM message WHERE account = ?1 \
         AND NOT EXISTS (SELECT 1 FROM membership WHERE membership.message = message.id)",
        [account.as_str()],
    )?;
    Ok(())
}

/// The rows of the messages a folder holds, without their content, newest
/// first by received date, then by the order they were stored in, newest
/// first; a message without a date comes last.
pub(crate) fn read_listed_rows(
    connection: &Connection,
    folder_id: i64,
) -> rusqlite::Result<Vec<MessageListRow>> {
    connection
        .prepare(
            "SELECT identity, subject, sender, recipients, received, seen \
             FROM membership JOIN message ON message.id = membership.message \
             WHERE membership.folder = ?1 ORDER BY message.received DESC, message.id DESC",
        )?
        .query_map([folder_id], |row| {
            Ok(MessageListRow {
                identity: row.get("identity")?,
                fields: DisplayFields {
                    subject: row.get("subject")?,
                    from: row.get("sender")?,
                    to: row.get("recipients")?,
                },
                received_unix: row.get("received")?,
                seen: row.get("seen")?,
            })
        })?
        .collect()
}

/// Where `read_message_content` selects `content_kind`, and `read_folders`
/// selects `role`, for a failure that names the column.
const CONTENT_KIND_COLUMN: usize = 0;
const ROLE_COLUMN: usize = 3;

/// One stored folder, from the columns `read_folders` selects. The schema's
/// `CHECK` and its version keep unknown role codes out of the file, so a code
/// the store cannot read here means a damaged row, and the read fails.
pub(crate) fn stored_folder(row: &Row) -> rusqlite::Result<Folder> {
    let code: Option<String> = row.get("role")?;
    let role = code
        .map(|code| {
            role_from_code(&code)
                .ok_or_else(|| damaged_row(ROLE_COLUMN, format!("unknown role code {code}")))
        })
        .transpose()?;
    Ok(Folder {
        identity: row.get("identity")?,
        name: row.get("name")?,
        parent: row.get("parent")?,
        role,
        selectable: row.get("selectable")?,
    })
}

/// The role as the schema's `CHECK` lists it (specs/008-folders/data-model.md).
fn role_code(role: FolderRole) -> &'static str {
    match role {
        FolderRole::Inbox => "inbox",
        FolderRole::Starred => "starred",
        FolderRole::Important => "important",
        FolderRole::Junk => "junk",
        FolderRole::Trash => "trash",
        FolderRole::Archive => "archive",
        FolderRole::Drafts => "drafts",
        FolderRole::Sent => "sent",
        FolderRole::AllMail => "all_mail",
    }
}

/// The role a stored code names; `None` for any other text.
fn role_from_code(code: &str) -> Option<FolderRole> {
    FolderRole::ORDER
        .into_iter()
        .find(|role| role_code(*role) == code)
}

/// One stored message's content, from the columns `read_message_content`
/// selects, with the same rule for content codes as for role codes.
pub(crate) fn stored_content(row: &Row) -> rusqlite::Result<ReceivedContent> {
    let content_code: String = row.get("content_kind")?;
    content_from_columns(&content_code, row.get("content_detail")?).ok_or_else(|| {
        damaged_row(
            CONTENT_KIND_COLUMN,
            format!("unknown content code {content_code}"),
        )
    })
}

fn damaged_row(column: usize, reason: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, reason.into())
}
