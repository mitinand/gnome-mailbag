// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's channels are tested through the scripted loader in
//! `mail_ui/tests.rs`, which owns the one graphical test.

use super::*;

/// The one place a provider becomes a load sequence.
#[test]
fn generic_imap_google_and_microsoft_365_accounts_can_be_loaded() {
    use crate::accounts::mail_provider;
    use goa_adapter::AccountProvider;
    assert_eq!(
        mail_provider(AccountProvider::ImapSmtp),
        Some(MailProvider::GenericImap)
    );
    assert_eq!(
        mail_provider(AccountProvider::Google),
        Some(MailProvider::Gmail)
    );
    assert_eq!(
        mail_provider(AccountProvider::Microsoft365),
        Some(MailProvider::Microsoft365)
    );
    assert_eq!(mail_provider(AccountProvider::Other), None);
}
