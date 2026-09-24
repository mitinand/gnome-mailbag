// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

mod access_calls;
mod account_model;
mod accounts;
mod client;
mod graph_access;
mod imap_access;
pub use access_calls::{AccessError, AccessRequest};
pub use account_model::{
    AccountCheckError, AccountCheckResult, AccountDetails, AccountId, AccountProvider,
    AccountUpdate, ErrorCause,
};
pub use client::GoaAdapter;
pub use graph_access::GraphAccess;
pub use imap_access::{ImapAccess, ImapCredential, ImapEncryption};

#[cfg(test)]
#[path = "../../../tests/support/service_thread.rs"]
mod service_thread;
#[cfg(test)]
#[path = "../../../tests/support/bus.rs"]
mod test_bus;
#[cfg(test)]
#[path = "../../../tests/support/goa.rs"]
mod test_goa;
