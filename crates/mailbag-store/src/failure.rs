// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A store operation's failure in the domain's terms: its kind from what
//! SQLite or the file system reported, their words as technical details
//! (specs/006-error-handling/contracts/failure-declaration.md). The wording for
//! the user is written by the application; the error line by whoever gives
//! the operation up.

use mailbag_domain::{Failure, FailureKind};
use rusqlite::ErrorCode;
use std::io;

/// Whether the failed operation read the store or changed it; opening the
/// store belongs to the operation that opened it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum StoreOperation {
    Read,
    Write,
}

/// What failed inside the store: SQLite, or the file system while the store's
/// directory and files were prepared.
#[derive(Debug)]
pub(crate) enum StoreError {
    Sqlite(rusqlite::Error),
    File(io::Error),
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::File(error)
    }
}

/// The failure of `operation`: a full disk whatever the operation, otherwise
/// by what it did (research §8). The technical details name the kind and
/// what SQLite or the file system said, which a debug line repeats.
pub(crate) fn storage_failure(operation: StoreOperation, error: &StoreError) -> Failure {
    let (disk_full, cause) = match error {
        StoreError::Sqlite(error) => match error.sqlite_error_code() {
            Some(code) => (
                code == ErrorCode::DiskFull,
                format!("SQLite: {code:?}: {error}"),
            ),
            None => (false, format!("SQLite: {error}")),
        },
        StoreError::File(error) => (
            error.kind() == io::ErrorKind::StorageFull,
            format!("File: {error}"),
        ),
    };
    let kind = match operation {
        _ if disk_full => FailureKind::StorageFull,
        StoreOperation::Write => FailureKind::MailNotSaved,
        StoreOperation::Read => FailureKind::StoredMailUnreadable,
    };
    tracing::debug!(failure = ?kind, cause, "a mail store operation failed");
    Failure {
        kind,
        remote_texts: Vec::new(),
        details: format!("Failure: {kind:?}\n{cause}"),
    }
}
