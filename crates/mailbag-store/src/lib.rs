// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mail store: each account's Inbox as the latest completed load left it,
//! in one SQLite file in the user's data directory (specs/007-mail-storage).
//! The mail worker writes a load's messages, the window reads them; both call
//! it off GTK's thread. It speaks the domain's types and hands its failures on
//! as the domain's `Failure`, with no wording.

mod content;
mod failure;
mod open;

#[cfg(test)]
#[path = "../../../tests/support/test_directory.rs"]
mod test_directory;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/support/record.rs"]
mod test_record;
#[cfg(test)]
mod tests;

use content::{content_columns, content_from_columns};
use failure::{StoreError, StoreOperation, storage_failure};
use mailbag_domain::{AccountId, DisplayFields, Failure, Message};
use open::{configure_connection, create_schema, open_store};
use rusqlite::{Connection, Row, params, types::Type};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Mutex, PoisonError},
};

/// The store: one connection behind a lock, opened at the first use, so that
/// creating it does no I/O (research §2).
pub struct Store {
    path: PathBuf,
    connection: Mutex<Option<Connection>>,
}

/// How a write of a load's messages ended when it did not fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboxWrite {
    Stored,
    /// The load was cancelled before the store took its messages, so nothing
    /// was written.
    LoadCancelled,
}

impl Store {
    /// The store in the file at `path`, opened at the first use.
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            connection: Mutex::new(None),
        }
    }

    /// An empty store in memory, for tests.
    pub fn in_memory() -> Self {
        let connection = Connection::open_in_memory()
            .and_then(|mut connection| {
                create_schema(&mut connection)?;
                configure_connection(&connection)?;
                Ok(connection)
            })
            .expect("an empty store in memory");
        Self {
            path: PathBuf::new(),
            connection: Mutex::new(Some(connection)),
        }
    }

    /// Replaces the account's stored Inbox with a completed load's messages,
    /// in their order, in one transaction: a failure leaves the previous Inbox
    /// whole. `load_cancelled` is asked under the store's lock, so a load
    /// cancelled because its account was excluded writes nothing, even when
    /// it finished meanwhile (research §6).
    pub fn replace_inbox(
        &self,
        account: &AccountId,
        messages: &[Message],
        load_cancelled: impl FnOnce() -> bool,
    ) -> Result<InboxWrite, Failure> {
        self.with_connection(StoreOperation::Write, |connection| {
            if load_cancelled() {
                return Ok(InboxWrite::LoadCancelled);
            }
            let transaction = connection.transaction()?;
            let account = account.as_str();
            transaction.execute("DELETE FROM inbox WHERE account = ?1", [account])?;
            transaction.execute("INSERT INTO inbox (account) VALUES (?1)", [account])?;
            let mut insert = transaction.prepare(
                "INSERT INTO message (account, identity, subject, sender, recipients, received, \
                 seen, content_kind, content_detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for message in messages {
                let (content_kind, content_detail) = content_columns(&message.content);
                insert.execute(params![
                    account,
                    message.identity,
                    message.fields.subject,
                    message.fields.from,
                    message.fields.to,
                    message.received_unix,
                    message.seen,
                    content_kind,
                    content_detail,
                ])?;
            }
            drop(insert);
            transaction.commit()?;
            Ok(InboxWrite::Stored)
        })
    }

    /// The account's stored Inbox in the load's order, or `None` when no load
    /// of it completed.
    pub fn read_inbox(&self, account: &AccountId) -> Result<Option<Vec<Message>>, Failure> {
        self.with_connection(StoreOperation::Read, |connection| {
            let account = account.as_str();
            let stored: bool = connection.query_row(
                "SELECT EXISTS (SELECT 1 FROM inbox WHERE account = ?1)",
                [account],
                |row| row.get(0),
            )?;
            if !stored {
                return Ok(None);
            }
            let mut select = connection.prepare(
                "SELECT identity, subject, sender, recipients, received, seen, content_kind, \
                 content_detail FROM message WHERE account = ?1 ORDER BY id",
            )?;
            let messages = select
                .query_map([account], stored_message)?
                .collect::<rusqlite::Result<_>>()?;
            Ok(Some(messages))
        })
    }

    /// Deletes the stored mail of every account not in `current_accounts`,
    /// and returns those accounts for the record.
    pub fn delete_other_accounts(
        &self,
        current_accounts: &BTreeSet<AccountId>,
    ) -> Result<Vec<AccountId>, Failure> {
        self.with_connection(StoreOperation::Write, |connection| {
            let transaction = connection.transaction()?;
            let stored_accounts: Vec<String> = transaction
                .prepare("SELECT account FROM inbox")?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            // Only nonempty identifiers are ever stored.
            let removed_accounts: Vec<AccountId> = stored_accounts
                .iter()
                .filter_map(|account| AccountId::try_from(account.as_str()).ok())
                .filter(|account| !current_accounts.contains(account))
                .collect();
            for account in &removed_accounts {
                transaction.execute("DELETE FROM inbox WHERE account = ?1", [account.as_str()])?;
            }
            transaction.commit()?;
            Ok(removed_accounts)
        })
    }

    /// Runs `work` with the connection, opening the store at its first use,
    /// and hands a failure on as `operation`'s. A lock poisoned by a panic is
    /// taken over: SQLite rolled back the transaction the panic interrupted.
    fn with_connection<T>(
        &self,
        operation: StoreOperation,
        work: impl FnOnce(&mut Connection) -> Result<T, StoreError>,
    ) -> Result<T, Failure> {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if connection.is_none() {
            let opened =
                open_store(&self.path).map_err(|error| storage_failure(operation, &error))?;
            *connection = Some(opened);
        }
        let connection = connection.as_mut().expect("the store was opened above");
        work(connection).map_err(|error| storage_failure(operation, &error))
    }
}

/// Where `read_inbox` selects `content_kind`, for a failure that names it.
const CONTENT_KIND_COLUMN: usize = 6;

/// One stored message, from the columns `read_inbox` selects. The schema's
/// `CHECK` and its version keep unknown content codes out of the file, so a
/// code the store cannot read here means a damaged row, and the read fails.
fn stored_message(row: &Row) -> rusqlite::Result<Message> {
    let content_code: String = row.get("content_kind")?;
    let content =
        content_from_columns(&content_code, row.get("content_detail")?).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                CONTENT_KIND_COLUMN,
                Type::Text,
                format!("unknown content code {content_code}").into(),
            )
        })?;
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
    })
}
