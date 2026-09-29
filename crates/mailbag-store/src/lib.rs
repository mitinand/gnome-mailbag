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
    delete_memberships, delete_messages_without_folder, delete_unlisted_folders,
    read_folder_identities, read_folder_state, read_listed_rows, read_stored_identities,
    relate_known, set_read_states, store_arrived, stored_content, stored_folder, stored_folder_id,
    upsert_folders, write_folder_state, write_mailbox,
};
use mailbag_domain::{
    AccountId, Failure, Folder, FolderPortion, FolderRef, FolderState, Message, MessageListRow,
    ReceivedContent,
};
use open::{configure_connection, create_schema, open_store};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::PathBuf,
    sync::{Mutex, PoisonError},
};

/// The store: one connection behind a lock, opened at the first use, so that
/// creating it does no I/O (specs/007-mail-storage/research.md §2).
pub struct Store {
    path: PathBuf,
    connection: Mutex<Option<Connection>>,
}

/// What a cycle reads of its folder at its start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderSync {
    pub state: FolderState,
    /// The identity and read state of every message the folder holds.
    pub stored: HashMap<String, bool>,
}

/// How a write of a load's result ended when it did not fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreWrite {
    Stored,
    /// The load was cancelled before the store took its result, so nothing
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

    /// Replaces the folder's messages with a completed load's, in one
    /// transaction (specs/008-folders FR-004): a failure leaves
    /// the previous state whole. A message another folder holds is kept once
    /// with the fields of this load; a message no folder holds any more is
    /// deleted. A folder the store does not hold fails the write.
    pub fn replace_mailbox(
        &self,
        folder: &FolderRef,
        messages: &[Message],
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

    /// What a cycle needs of the folder at its start: its state and the
    /// messages it holds with their read state. A folder the store does not
    /// hold fails as a write would, since the cycle cannot store into it.
    pub fn read_folder_sync(&self, folder: &FolderRef) -> Result<FolderSync, Failure> {
        self.with_connection(StoreOperation::Write, |connection| {
            let folder_id = stored_folder_id(connection, folder)?;
            Ok(FolderSync {
                state: read_folder_state(connection, folder_id)?,
                stored: read_folder_identities(connection, folder_id)?,
            })
        })
    }

    /// Which of a portion's identities the account already holds, so the
    /// cycle relates them without fetching them.
    pub fn stored_identities(
        &self,
        account: &AccountId,
        identities: &[String],
    ) -> Result<HashSet<String>, Failure> {
        self.with_connection(StoreOperation::Read, |connection| {
            Ok(read_stored_identities(connection, account, identities)?)
        })
    }

    /// Stores one portion of a cycle in one transaction, whole or not at all
    /// (specs/009-synchronization FR-008): removals, then the messages left
    /// in no folder, read states, full records, messages the account already
    /// held, and the folder's state when the portion carries one.
    /// `load_cancelled` is asked under the store's lock, as for a folder list.
    pub fn store_portion(
        &self,
        folder: &FolderRef,
        portion: &FolderPortion,
        load_cancelled: impl FnOnce() -> bool,
    ) -> Result<StoreWrite, Failure> {
        self.with_connection(StoreOperation::Write, |connection| {
            if load_cancelled() {
                return Ok(StoreWrite::LoadCancelled);
            }
            let account = &folder.account;
            let transaction = connection.transaction()?;
            let folder_id = stored_folder_id(&transaction, folder)?;
            delete_memberships(&transaction, folder_id, account, &portion.removed)?;
            delete_messages_without_folder(&transaction, account)?;
            set_read_states(&transaction, account, &portion.read_states)?;
            store_arrived(&transaction, folder_id, account, &portion.arrived)?;
            relate_known(&transaction, folder_id, account, &portion.known_arrived)?;
            if let Some(state) = &portion.state {
                write_folder_state(&transaction, folder_id, state)?;
            }
            transaction.commit()?;
            Ok(StoreWrite::Stored)
        })
    }

    /// The account's stored folders in no particular order; the window sorts
    /// them (specs/008-folders/research.md §7). Empty when no folder list was
    /// stored.
    pub fn read_folders(&self, account: &AccountId) -> Result<Vec<Folder>, Failure> {
        self.with_connection(StoreOperation::Read, |connection| {
            let mut select = connection.prepare(
                "SELECT identity, name, parent, role, selectable FROM folder WHERE account = ?1",
            )?;
            let folders = select
                .query_map([account.as_str()], stored_folder)?
                .collect::<rusqlite::Result<_>>()?;
            Ok(folders)
        })
    }

    /// The folder's stored messages as the list shows them, without their
    /// content, newest first; `None` when the folder never completed a cycle
    /// and holds no message ("no mail loaded"), or the store does not hold it.
    pub fn read_folder_rows(
        &self,
        folder: &FolderRef,
    ) -> Result<Option<Vec<MessageListRow>>, Failure> {
        self.with_connection(StoreOperation::Read, |connection| {
            let Some(folder_id) = stored_folder_id(connection, folder).optional()? else {
                return Ok(None);
            };
            let rows = read_listed_rows(connection, folder_id)?;
            let synchronized = read_folder_state(connection, folder_id)?.synchronized;
            Ok((synchronized || !rows.is_empty()).then_some(rows))
        })
    }

    /// The content of the account's message, which the reader shows when it
    /// is opened; `None` when the store no longer holds the message.
    pub fn read_message_content(
        &self,
        account: &AccountId,
        identity: &str,
    ) -> Result<Option<ReceivedContent>, Failure> {
        self.with_connection(StoreOperation::Read, |connection| {
            let content = connection
                .query_row(
                    "SELECT content_kind, content_detail FROM message \
                     WHERE account = ?1 AND identity = ?2",
                    params![account.as_str(), identity],
                    stored_content,
                )
                .optional()?;
            Ok(content)
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
                .prepare("SELECT DISTINCT account FROM folder")?
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
