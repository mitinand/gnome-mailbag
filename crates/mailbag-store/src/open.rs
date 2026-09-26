// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opening the store's file at its first use: its private directory, what an
//! existing file holds, and a fresh schema. A file written with another
//! structure, a file that is not a store and a damaged store are discarded,
//! since nothing is converted before the first release
//! (specs/007-mail-storage/research.md §4).

use crate::failure::StoreError;
use rusqlite::{Connection, ErrorCode};
use std::{
    fs::{self, DirBuilder, Permissions},
    io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
};

const SCHEMA: &str = include_str!("schema.sql");

/// What the file at the store's path holds.
enum ExistingStore {
    Usable,
    Empty,
    /// Not usable, for the reason the record names.
    Discard(&'static str),
}

/// Opens the store at `path`: an empty or discarded file gets the schema.
pub(crate) fn open_store(path: &Path) -> Result<Connection, StoreError> {
    if let Some(directory) = path.parent() {
        create_private_directory(directory)?;
    }
    let mut connection = Connection::open(path)?;
    match examine_existing(&connection)? {
        ExistingStore::Usable => {}
        ExistingStore::Empty => create_schema(&mut connection)?,
        ExistingStore::Discard(reason) => {
            drop(connection);
            discard_store(path, reason)?;
            connection = Connection::open(path)?;
            create_schema(&mut connection)?;
        }
    }
    configure_connection(&connection)?;
    Ok(connection)
}

/// The store's directory, readable by the user only: a Flatpak's data
/// directories are readable by others, and only the home directory's rights
/// keep them private (research §7). An existing directory gets the same
/// rights, which `create` leaves as they are, so a copy restored with wider
/// rights becomes private again.
fn create_private_directory(directory: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    fs::set_permissions(directory, Permissions::from_mode(0o700))
}

/// Tells an empty file, a usable store and one to discard apart. Any other
/// failure, such as a file that cannot be read, discards nothing: the
/// operation fails and the next one tries again.
fn examine_existing(connection: &Connection) -> Result<ExistingStore, StoreError> {
    let read = || -> rusqlite::Result<ExistingStore> {
        let version: i32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let tables: i64 =
            connection.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))?;
        if version == 0 && tables == 0 {
            return Ok(ExistingStore::Empty);
        }
        if version != schema_version() {
            return Ok(ExistingStore::Discard("structure changed"));
        }
        let check: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        Ok(match check.as_str() {
            "ok" => ExistingStore::Usable,
            _ => ExistingStore::Discard("damaged"),
        })
    };
    match read() {
        Ok(found) => Ok(found),
        Err(error) => match error.sqlite_error_code() {
            Some(ErrorCode::NotADatabase) => Ok(ExistingStore::Discard("not a store")),
            Some(ErrorCode::DatabaseCorrupt) => Ok(ExistingStore::Discard("damaged")),
            _ => Err(error.into()),
        },
    }
}

/// Removes the store's file with its write-ahead log and shared-memory files,
/// and says why in the record.
fn discard_store(path: &Path, reason: &str) -> Result<(), StoreError> {
    for suffix in ["", "-wal", "-shm"] {
        let mut file_name = path.as_os_str().to_owned();
        file_name.push(suffix);
        match fs::remove_file(PathBuf::from(file_name)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
    }
    tracing::warn!(reason, "the mail store was discarded and starts empty");
    Ok(())
}

/// Creates the tables in an empty store and records their version, in one
/// transaction: a store interrupted halfway would otherwise hold tables of
/// version 0 and be discarded at the next start as of another structure.
pub(crate) fn create_schema(connection: &mut Connection) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(SCHEMA)?;
    transaction.pragma_update(None, "user_version", schema_version())?;
    transaction.commit()
}

/// A committed load survives a crash; after a power loss the latest one may
/// be missing, never part of it (research §5). The cascade from an Inbox to
/// its messages needs foreign keys, which SQLite leaves off by default.
pub(crate) fn configure_connection(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
    )
}

/// The store's version: a 32-bit FNV-1a hash of the schema's text, so that
/// any change to the schema changes it without a manual step.
pub(crate) fn schema_version() -> i32 {
    let hash = SCHEMA.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    // SQLite keeps the version as a signed 32-bit integer; the bits are kept.
    hash as i32
}
