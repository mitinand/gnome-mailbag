// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows behind the store's operations: folders, the messages they hold
//! and the memberships between them (specs/008-folders/data-model.md). Each
//! function runs inside the caller's transaction or read.

use crate::{
    StoredFolder,
    content::{content_columns, content_from_columns},
};
use mailbag_domain::{
    AccountId, DisplayFields, Folder, FolderMembership, FolderRef, FolderRole, Message,
};
use rusqlite::{Connection, Row, Transaction, params, types::Type};
use std::collections::BTreeSet;

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

/// Updates the listed folders the store holds, keeping whether they were
/// loaded, and inserts the others as not loaded.
pub(crate) fn upsert_folders(
    transaction: &Transaction,
    account: &AccountId,
    folders: &[Folder],
) -> rusqlite::Result<()> {
    let mut upsert = transaction.prepare(
        "INSERT INTO folder (account, identity, name, parent, attributes, role, selectable, \
         loaded) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0) \
         ON CONFLICT (account, identity) DO UPDATE SET name = excluded.name, \
         parent = excluded.parent, attributes = excluded.attributes, role = excluded.role, \
         selectable = excluded.selectable",
    )?;
    for folder in folders {
        upsert.execute(params![
            account.as_str(),
            folder.identity,
            folder.name,
            folder.parent,
            folder.attributes.join(" "),
            folder.role.map(FolderRole::as_code),
            folder.selectable,
        ])?;
    }
    Ok(())
}

/// Replaces the folder's memberships with the load's, stores each message
/// once by its identity with the load's fields, deletes the messages no
/// folder holds any more and marks the folder loaded. A folder the store
/// does not hold fails with no row found.
pub(crate) fn write_mailbox(
    transaction: &Transaction,
    folder: &FolderRef,
    messages: &[(Message, FolderMembership)],
) -> rusqlite::Result<()> {
    let folder_id: i64 = transaction.query_row(
        "SELECT id FROM folder WHERE account = ?1 AND identity = ?2",
        params![folder.account.as_str(), folder.identity],
        |row| row.get(0),
    )?;
    transaction.execute("DELETE FROM membership WHERE folder = ?1", [folder_id])?;
    let mut upsert_message = transaction.prepare(
        "INSERT INTO message (account, identity, subject, sender, recipients, received, seen, \
         content_kind, content_detail, labels) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
         ON CONFLICT (account, identity) DO UPDATE SET subject = excluded.subject, \
         sender = excluded.sender, recipients = excluded.recipients, \
         received = excluded.received, seen = excluded.seen, \
         content_kind = excluded.content_kind, content_detail = excluded.content_detail, \
         labels = excluded.labels \
         RETURNING id",
    )?;
    let mut insert_membership = transaction.prepare(
        "INSERT INTO membership (folder, message, uid, position) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (message, membership) in messages {
        let (content_kind, content_detail) = content_columns(&message.content);
        let labels = (!message.labels.is_empty()).then(|| message.labels.join("\n"));
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
                labels,
            ],
            |row| row.get(0),
        )?;
        insert_membership.execute(params![
            folder_id,
            message_id,
            membership.uid,
            membership.position
        ])?;
    }
    delete_messages_without_folder(transaction, &folder.account)?;
    transaction.execute("UPDATE folder SET loaded = 1 WHERE id = ?1", [folder_id])?;
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

/// The messages a folder holds, in its load's order.
pub(crate) fn read_folder_messages(
    connection: &Connection,
    folder_id: i64,
) -> rusqlite::Result<Vec<Message>> {
    connection
        .prepare(
            "SELECT identity, subject, sender, recipients, received, seen, content_kind, \
             content_detail, labels \
             FROM membership JOIN message ON message.id = membership.message \
             WHERE membership.folder = ?1 ORDER BY membership.position",
        )?
        .query_map([folder_id], stored_message)?
        .collect()
}

/// Where `read_folder_messages` selects `content_kind`, and `read_folders`
/// selects `role`, for a failure that names the column.
const CONTENT_KIND_COLUMN: usize = 6;
const ROLE_COLUMN: usize = 4;

/// One stored folder, from the columns `read_folders` selects. The schema's
/// `CHECK` and its version keep unknown role codes out of the file, so a code
/// the store cannot read here means a damaged row, and the read fails.
pub(crate) fn stored_folder(row: &Row) -> rusqlite::Result<StoredFolder> {
    let role_code: Option<String> = row.get("role")?;
    let role = role_code
        .map(|code| {
            FolderRole::from_code(&code)
                .ok_or_else(|| damaged_row(ROLE_COLUMN, format!("unknown role code {code}")))
        })
        .transpose()?;
    let attributes: String = row.get("attributes")?;
    Ok(StoredFolder {
        folder: Folder {
            identity: row.get("identity")?,
            name: row.get("name")?,
            parent: row.get("parent")?,
            attributes: attributes.split_whitespace().map(str::to_owned).collect(),
            role,
            selectable: row.get("selectable")?,
        },
        loaded: row.get("loaded")?,
    })
}

/// One stored message, from the columns `read_folder_messages` selects, with
/// the same rule for content codes as for role codes.
fn stored_message(row: &Row) -> rusqlite::Result<Message> {
    let content_code: String = row.get("content_kind")?;
    let content =
        content_from_columns(&content_code, row.get("content_detail")?).ok_or_else(|| {
            damaged_row(
                CONTENT_KIND_COLUMN,
                format!("unknown content code {content_code}"),
            )
        })?;
    let labels: Option<String> = row.get("labels")?;
    Ok(Message {
        identity: row.get("identity")?,
        fields: DisplayFields {
            subject: row.get("subject")?,
            from: row.get("sender")?,
            to: row.get("recipients")?,
        },
        received_unix: row.get("received")?,
        seen: row.get("seen")?,
        content,
        labels: labels
            .map(|labels| labels.split('\n').map(str::to_owned).collect())
            .unwrap_or_default(),
    })
}

fn damaged_row(column: usize, reason: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, reason.into())
}
