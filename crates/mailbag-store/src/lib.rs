// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mail store: each account's folders and their messages as the latest
//! completed loads left them, in one SQLite file in the user's data directory
//! (specs/007-mail-storage, specs/008-folders). The mail worker writes a
//! load's result, the window reads it; both call it off GTK's thread. It
//! speaks the domain's types and hands its failures on as the domain's
//! `Failure`, with no wording.

mod content;
mod failure;
mod folders;
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

use failure::{StoreError, StoreOperation, storage_failure};
use folders::{
    delete_messages_without_folder, delete_unlisted_folders, read_folder_messages, stored_folder,
    upsert_folders, write_mailbox,
};
use mailbag_domain::{AccountId, Failure, Folder, FolderMembership, FolderRef, Message};
use open::{configure_connection, create_schema, open_store};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Mutex, PoisonError},
};

/// The store: one connection behind a lock, opened at the first use, so that
/// creating it does no I/O (specs/007-mail-storage/research.md §2).
pub struct Store {
    path: PathBuf,
    connection: Mutex<Option<Connection>>,
}

/// How a write of a load's result ended when it did not fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreWrite {
    Stored,
    /// The load was cancelled before the store took its result, so nothing
    /// was written.
    LoadCancelled,
}

/// A folder as the store holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredFolder {
    pub folder: Folder,
    /// Whether a load of it completed (specs/007-mail-storage FR-006).
    pub loaded: bool,
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

    /// Replaces the account's folder list with a completed one, in one
    /// transaction (specs/008-folders FR-001, FR-007): a folder no longer
    /// listed goes with its memberships and with the messages no other
    /// folder holds, a listed folder keeps its mail, a new one is not loaded.
    /// `folders` is never empty: an empty list is not stored. `load_cancelled`
    /// is asked under the store's lock, so a load cancelled because its
    /// account was excluded writes nothing, even when it finished meanwhile
    /// (specs/007-mail-storage/research.md §6).
    pub fn replace_folders(
        &self,
        account: &AccountId,
        folders: &[Folder],
        load_cancelled: impl FnOnce() -> bool,
    ) -> Result<StoreWrite, Failure> {
        self.with_connection(StoreOperation::Write, |connection| {
            if load_cancelled() {
                return Ok(StoreWrite::LoadCancelled);
            }
            let transaction = connection.transaction()?;
            delete_unlisted_folders(&transaction, account, folders)?;
            delete_messages_without_folder(&transaction, account)?;
            upsert_folders(&transaction, account, folders)?;
            transaction.commit()?;
            Ok(StoreWrite::Stored)
        })
    }

    /// Replaces the folder's messages with a completed load's, in their
    /// order, in one transaction (specs/008-folders FR-004): a failure leaves
    /// the previous state whole. A message another folder holds is kept once
    /// with the fields of this load; a message no folder holds any more is
    /// deleted. A folder the store does not hold fails the write.
    pub fn replace_mailbox(
        &self,
        folder: &FolderRef,
        messages: &[(Message, FolderMembership)],
        load_cancelled: impl FnOnce() -> bool,
    ) -> Result<StoreWrite, Failure> {
        self.with_connection(StoreOperation::Write, |connection| {
            if load_cancelled() {
                return Ok(StoreWrite::LoadCancelled);
            }
            let transaction = connection.transaction()?;
            write_mailbox(&transaction, folder, messages)?;
            transaction.commit()?;
            Ok(StoreWrite::Stored)
        })
    }

    /// The account's stored folders in no particular order; the window sorts
    /// them (specs/008-folders/research.md §7). Empty when no folder list was
    /// stored.
    pub fn read_folders(&self, account: &AccountId) -> Result<Vec<StoredFolder>, Failure> {
        self.with_connection(StoreOperation::Read, |connection| {
            let mut select = connection.prepare(
                "SELECT identity, name, parent, attributes, role, selectable, loaded \
                 FROM folder WHERE account = ?1",
            )?;
            let folders = select
                .query_map([account.as_str()], stored_folder)?
                .collect::<rusqlite::Result<_>>()?;
            Ok(folders)
        })
    }

    /// The folder's stored messages in the load's order, or `None` when no
    /// load of it completed or the store does not hold the folder.
    pub fn read_mailbox(&self, folder: &FolderRef) -> Result<Option<Vec<Message>>, Failure> {
        self.with_connection(StoreOperation::Read, |connection| {
            let loaded_folder: Option<i64> = connection
                .query_row(
                    "SELECT id FROM folder WHERE account = ?1 AND identity = ?2 AND loaded = 1",
                    params![folder.account.as_str(), folder.identity],
                    |row| row.get(0),
                )
                .optional()?;
            match loaded_folder {
                Some(folder_id) => Ok(Some(read_folder_messages(connection, folder_id)?)),
                None => Ok(None),
            }
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
                .prepare("SELECT account FROM folder UNION SELECT account FROM message")?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            // Only nonempty identifiers are ever stored.
            let removed_accounts: Vec<AccountId> = stored_accounts
                .iter()
                .filter_map(|account| AccountId::try_from(account.as_str()).ok())
                .filter(|account| !current_accounts.contains(account))
                .collect();
            // Memberships go with their folders and messages.
            for account in &removed_accounts {
                transaction.execute("DELETE FROM folder WHERE account = ?1", [account.as_str()])?;
                transaction
                    .execute("DELETE FROM message WHERE account = ?1", [account.as_str()])?;
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
