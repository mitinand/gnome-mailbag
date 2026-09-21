// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

mod account_model;
mod accounts;
mod client;
mod imap_access;
pub use account_model::{
    AccountCheckError, AccountCheckResult, AccountDetails, AccountId, AccountProvider,
    AccountUpdate, ErrorCause,
};
pub use client::GoaAdapter;
pub use imap_access::{ImapAccess, ImapAccessError, ImapAccessRequest, ImapEncryption};

#[cfg(test)]
#[path = "../../../tests/support/bus.rs"]
mod test_bus;
#[cfg(test)]
#[path = "../../../tests/support/goa.rs"]
mod test_goa;
#[cfg(test)]
#[path = "../../../tests/support/record.rs"]
mod test_record;
