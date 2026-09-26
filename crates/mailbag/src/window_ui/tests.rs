// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's channels are tested through the scripted loader in
//! `mail_ui/tests.rs`, which owns the window's graphical tests.

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

/// Two reads started in turn whose answers arrive in the reverse order: the
/// older answer is dropped, whichever account it was for
/// (specs/007-mail-storage/research.md §6).
#[test]
fn an_older_reads_answer_never_replaces_a_newer_ones() {
    let account = |name| AccountId::try_from(name).expect("synthetic account id");
    let mut shown = ShownInbox::default();
    let first = shown.start_read(&account("first"));
    let second = shown.start_read(&account("second"));
    assert!(shown.finish_read(second, Ok(None)));
    let failure = Failure::stopped(None);
    assert!(!shown.finish_read(first, Err(failure)));
    assert_eq!(shown.account, Some(account("second")));
    assert!(matches!(shown.stored, StoredInbox::Read(None)));
}

/// A hidden account's mail may be deleted, so the window forgets what it read
/// of it, and a read still running for it is dropped when it answers.
#[test]
fn a_hidden_accounts_inbox_is_forgotten_with_its_read_in_flight() {
    let hidden = AccountId::try_from("hidden").expect("synthetic account id");
    let mut shown = ShownInbox::default();
    let read = shown.start_read(&hidden);
    shown.forget_excluded(|_| false);
    assert!(!shown.finish_read(read, Ok(Some(Vec::new()))));
    assert!(!shown.holds(&hidden));
}
