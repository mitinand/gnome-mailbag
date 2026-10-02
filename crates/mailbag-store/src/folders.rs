// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows behind the store's operations: folders, the messages they hold
//! and the memberships between them (specs/009-synchronization/data-model.md).
//! Each function runs inside the caller's transaction or read.

use crate::content::{content_columns, content_from_columns};
use mailbag_domain::{
    AccountId, DisplayFields, FlagChanges, Folder, FolderRef, FolderRole, FolderState, Message,
    MessageFlag, MessageFlags, MessageListRow, PendingChange, ReceivedContent,
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

/// The folder's saved state.
pub(crate) fn read_folder_state(
    connection: &Connection,
    folder_id: i64,
) -> rusqlite::Result<FolderState> {
    connection.query_row(
        "SELECT server_position, fill_place, synchronized FROM folder WHERE id = ?1",
        [folder_id],
        |row| {
            Ok(FolderState {
                server_position: row.get(0)?,
                fill_place: row.get(1)?,
                synchronized: row.get(2)?,
            })
        },
    )
}

/// The identity and the server's flags of every message the folder holds.
pub(crate) fn read_folder_identities(
    connection: &Connection,
    folder_id: i64,
) -> rusqlite::Result<HashMap<String, MessageFlags>> {
    connection
        .prepare(
            "SELECT identity, seen, flagged \
             FROM membership JOIN message ON message.id = membership.message \
             WHERE membership.folder = ?1",
        )?
        .query_map([folder_id], |row| {
            let flags = MessageFlags {
                seen: row.get(1)?,
                flagged: row.get(2)?,
            };
            Ok((row.get(0)?, flags))
        })?
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

/// Which of `identities` another folder of the folder's account holds.
pub(crate) fn read_identities_in_other_folders(
    connection: &Connection,
    folder: &FolderRef,
    identities: &[String],
) -> rusqlite::Result<HashSet<String>> {
    let mut select = connection.prepare(
        "SELECT 1 FROM message JOIN membership ON membership.message = message.id \
         JOIN folder ON folder.id = membership.folder \
         WHERE message.account = ?1 AND message.identity = ?2 AND folder.identity != ?3",
    )?;
    let mut held = HashSet::new();
    for identity in identities {
        if select.exists(params![folder.account.as_str(), identity, folder.identity])? {
            held.insert(identity.clone());
        }
    }
    Ok(held)
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

/// Writes the flags a server reported for the account's messages, leaving a
/// flag it did not name as stored. A pending value equal to the value written
/// ends; a differing one stays (specs/011-read-and-star/data-model.md).
pub(crate) fn set_flag_states(
    transaction: &Transaction,
    account: &AccountId,
    flag_states: &[(String, FlagChanges)],
) -> rusqlite::Result<()> {
    // `NULLIF` ends a pending value equal to the value written; a flag not
    // reported is NULL, which neither writes nor ends anything.
    let mut update = transaction.prepare(
        "UPDATE message SET seen = COALESCE(?3, seen), flagged = COALESCE(?4, flagged), \
         seen_pending = NULLIF(seen_pending, ?3), flagged_pending = NULLIF(flagged_pending, ?4) \
         WHERE account = ?1 AND identity = ?2",
    )?;
    for (identity, changes) in flag_states {
        update.execute(params![
            account.as_str(),
            identity,
            changes.seen,
            changes.flagged
        ])?;
    }
    Ok(())
}

/// Stores each full record once by its identity and relates it to the
/// folder. Its content replaces the stored one, except that a text not
/// downloaded never replaces a content another folder's cycle stored, and
/// a text the server did not return never replaces a stored text
/// (specs/009-synchronization/data-model.md). Its preview always replaces
/// the stored one (specs/010-message-list/data-model.md). Its flags end an
/// equal pending value, as `set_flag_states` does.
pub(crate) fn store_arrived(
    transaction: &Transaction,
    folder_id: i64,
    account: &AccountId,
    arrived: &[Message],
) -> rusqlite::Result<()> {
    let mut upsert_message = transaction.prepare(
        "INSERT INTO message (account, identity, subject, sender, recipients, received, seen, \
         flagged, content_kind, content_detail, preview) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
         ON CONFLICT (account, identity) DO UPDATE SET subject = excluded.subject, \
         sender = excluded.sender, recipients = excluded.recipients, \
         received = excluded.received, seen = excluded.seen, flagged = excluded.flagged, \
         seen_pending = NULLIF(seen_pending, excluded.seen), \
         flagged_pending = NULLIF(flagged_pending, excluded.flagged), \
         preview = excluded.preview, \
         content_kind = iif(excluded.content_kind = 'not_downloaded' \
         OR (excluded.content_kind = 'text_not_returned' AND content_kind = 'text'), \
         content_kind, excluded.content_kind), \
         content_detail = iif(excluded.content_kind = 'not_downloaded' \
         OR (excluded.content_kind = 'text_not_returned' AND content_kind = 'text'), \
         content_detail, excluded.content_detail) \
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
                message.flagged,
                content_kind,
                content_detail,
                message.preview,
            ],
            |row| row.get(0),
        )?;
        relate.execute(params![folder_id, message_id])?;
    }
    Ok(())
}

/// Relates messages the account already holds to the folder, with their
/// listed flags. A message the store no longer holds is left out; the
/// folder's next cycle fetches it.
pub(crate) fn relate_known(
    transaction: &Transaction,
    folder_id: i64,
    account: &AccountId,
    known_arrived: &[(String, MessageFlags)],
) -> rusqlite::Result<()> {
    let mut relate = transaction.prepare(
        "INSERT OR IGNORE INTO membership (folder, message) \
         SELECT ?1, id FROM message WHERE account = ?2 AND identity = ?3",
    )?;
    for (identity, _) in known_arrived {
        relate.execute(params![folder_id, account.as_str(), identity])?;
    }
    let flag_states: Vec<(String, FlagChanges)> = known_arrived
        .iter()
        .map(|(identity, flags)| {
            let changes = FlagChanges {
                seen: Some(flags.seen),
                flagged: Some(flags.flagged),
            };
            (identity.clone(), changes)
        })
        .collect();
    set_flag_states(transaction, account, &flag_states)
}

/// The changes of the folder's messages the user wants and the server may
/// not have yet, each with the server's value as stored.
pub(crate) fn read_pending_changes(
    connection: &Connection,
    folder_id: i64,
) -> rusqlite::Result<Vec<PendingChange>> {
    let mut select = connection.prepare(
        "SELECT identity, seen, seen_pending, flagged, flagged_pending \
         FROM membership JOIN message ON message.id = membership.message \
         WHERE membership.folder = ?1 \
         AND (seen_pending IS NOT NULL OR flagged_pending IS NOT NULL)",
    )?;
    let mut rows = select.query([folder_id])?;
    let mut changes = Vec::new();
    while let Some(row) = rows.next()? {
        let identity: String = row.get("identity")?;
        let flags = [
            (
                MessageFlag::Seen,
                row.get("seen")?,
                row.get("seen_pending")?,
            ),
            (
                MessageFlag::Flagged,
                row.get("flagged")?,
                row.get("flagged_pending")?,
            ),
        ];
        for (flag, server, wanted) in flags {
            if let Some(wanted) = wanted {
                changes.push(PendingChange {
                    identity: identity.clone(),
                    flag,
                    wanted,
                    server,
                });
            }
        }
    }
    Ok(changes)
}

/// Saves the folder's state.
pub(crate) fn write_folder_state(
    transaction: &Transaction,
    folder_id: i64,
    state: &FolderState,
) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE folder SET server_position = ?2, fill_place = ?3, synchronized = ?4 \
         WHERE id = ?1",
        params![
            folder_id,
            state.server_position,
            state.fill_place,
            state.synchronized
        ],
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

/// The rows of the messages a folder holds, with their previews but without
/// their content, newest
/// first by received date, then by the order they were stored in, newest
/// first; a message without a date comes last. Each flag is the user's
/// pending value where there is one, otherwise the server's.
pub(crate) fn read_listed_rows(
    connection: &Connection,
    folder_id: i64,
) -> rusqlite::Result<Vec<MessageListRow>> {
    connection
        .prepare(
            "SELECT identity, subject, sender, recipients, received, \
             COALESCE(seen_pending, seen) AS seen, \
             COALESCE(flagged_pending, flagged) AS flagged, preview \
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
                flagged: row.get("flagged")?,
                preview: row.get("preview")?,
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
