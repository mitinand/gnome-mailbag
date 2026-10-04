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
    read_folder_identities, read_folder_state, read_identities_in_other_folders, read_listed_rows,
    read_pending_changes, read_stored_identities, relate_known, set_flag_states, store_arrived,
    stored_content, stored_folder, stored_folder_id, upsert_folders, write_folder_state,
};
use mailbag_domain::{
    AccountId, Failure, Folder, FolderBatch, FolderRef, FolderState, MessageFlag, MessageFlags,
    MessageListRow, PendingChange, ReceivedContent,
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
    /// The identity and the server's flags of every message the folder holds.
    pub stored: HashMap<String, MessageFlags>,
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

    /// What a cycle needs of the folder at its start: its state and the
    /// messages it holds with the server's flags. A folder the store does not
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

    /// Which of a batch's identities the account already holds, so the
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

    /// Which of `identities` another folder of the folder's account holds,
    /// so a cycle reads such a message again before it applies a change
    /// that may be older than the other folder's state
    /// (specs/009-synchronization/research.md §5).
    pub fn identities_in_other_folders(
        &self,
        folder: &FolderRef,
        identities: &[String],
    ) -> Result<HashSet<String>, Failure> {
        self.with_connection(StoreOperation::Read, |connection| {
            Ok(read_identities_in_other_folders(
                connection, folder, identities,
            )?)
        })
    }

    /// Stores one batch of a cycle in one transaction, whole or not at all
    /// (specs/009-synchronization FR-008): removals, then the messages left
    /// in no folder, flags, full records, messages the account already
    /// held, and the folder's state when the batch carries one. The pending
    /// values stay (specs/011-read-and-star research §15).
    /// `load_cancelled` is asked under the store's lock, as for a folder list.
    pub fn store_batch(
        &self,
        folder: &FolderRef,
        batch: &FolderBatch,
        load_cancelled: impl FnOnce() -> bool,
    ) -> Result<StoreWrite, Failure> {
        self.with_connection(StoreOperation::Write, |connection| {
            if load_cancelled() {
                return Ok(StoreWrite::LoadCancelled);
            }
            let account = &folder.account;
            let transaction = connection.transaction()?;
            let folder_id = stored_folder_id(&transaction, folder)?;
            // Only a removal can leave a message in no folder.
            if !batch.removed.is_empty() {
                delete_memberships(&transaction, folder_id, account, &batch.removed)?;
                delete_messages_without_folder(&transaction, account)?;
            }
            set_flag_states(&transaction, account, &batch.flag_states)?;
            store_arrived(&transaction, folder_id, account, &batch.arrived)?;
            relate_known(&transaction, folder_id, account, &batch.known_arrived)?;
            if let Some(state) = &batch.state {
                write_folder_state(&transaction, folder_id, state)?;
            }
            transaction.commit()?;
            Ok(StoreWrite::Stored)
        })
    }

    /// The changes of the folder's messages the user wants and the server
    /// may not have yet (specs/011-read-and-star FR-007).
    pub fn read_pending_changes(&self, folder: &FolderRef) -> Result<Vec<PendingChange>, Failure> {
        self.with_connection(StoreOperation::Read, |connection| {
            let folder_id = stored_folder_id(connection, folder)?;
            Ok(read_pending_changes(connection, folder_id)?)
        })
    }

    /// Stores the value of `flag` the user wants for the account's message,
    /// whatever the server's value is: a command for that flag may be on its
    /// way and change it (specs/011-read-and-star FR-001, research §14). The
    /// window reads it as the message's state until the server agrees or
    /// refuses. A message the store no longer holds is left as it is.
    pub fn write_pending_flag(
        &self,
        account: &AccountId,
        identity: &str,
        flag: MessageFlag,
        wanted: bool,
    ) -> Result<(), Failure> {
        let (_, pending) = flag_columns(flag);
        self.with_connection(StoreOperation::Write, |connection| {
            connection.execute(
                &format!("UPDATE message SET {pending} = ?3 WHERE account = ?1 AND identity = ?2"),
                params![account.as_str(), identity, wanted],
            )?;
            Ok(())
        })
    }

    /// The cycle saw the server hold `value` of `flag` for the account's
    /// messages: it becomes their server value and ends a pending value
    /// equal to it; a newer wish for the other value stays
    /// (specs/011-read-and-star FR-007).
    pub fn settle_flags(
        &self,
        account: &AccountId,
        identities: &[String],
        flag: MessageFlag,
        value: bool,
    ) -> Result<(), Failure> {
        let (server, pending) = flag_columns(flag);
        let assignments = format!("{server} = ?3, {pending} = NULLIF({pending}, ?3)");
        self.update_messages(account, identities, &assignments, value)
    }

    /// The server refused `refused` of `flag` for the account's messages: a
    /// pending value equal to it ends; a newer wish for the other value stays
    /// (specs/011-read-and-star FR-010).
    pub fn drop_pending_flags(
        &self,
        account: &AccountId,
        identities: &[String],
        flag: MessageFlag,
        refused: bool,
    ) -> Result<(), Failure> {
        let (_, pending) = flag_columns(flag);
        let assignments = format!("{pending} = NULLIF({pending}, ?3)");
        self.update_messages(account, identities, &assignments, refused)
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

    /// Sets `assignments` on each of the account's messages in `identities`,
    /// in one transaction, with `value` as `?3`.
    fn update_messages(
        &self,
        account: &AccountId,
        identities: &[String],
        assignments: &str,
        value: bool,
    ) -> Result<(), Failure> {
        self.with_connection(StoreOperation::Write, |connection| {
            let transaction = connection.transaction()?;
            let mut update = transaction.prepare(&format!(
                "UPDATE message SET {assignments} WHERE account = ?1 AND identity = ?2"
            ))?;
            for identity in identities {
                update.execute(params![account.as_str(), identity, value])?;
            }
            drop(update);
            transaction.commit()?;
            Ok(())
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

/// The columns of `flag`: the server's value and the pending one
/// (specs/011-read-and-star/data-model.md).
fn flag_columns(flag: MessageFlag) -> (&'static str, &'static str) {
    match flag {
        MessageFlag::Seen => ("seen", "seen_pending"),
        MessageFlag::Flagged => ("flagged", "flagged_pending"),
    }
}
