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

fn folder(identity: &str) -> FolderRef {
    FolderRef {
        account: AccountId::try_from("synthetic").expect("synthetic account id"),
        identity: identity.to_owned(),
    }
}

/// Two reads started in turn whose answers arrive in the reverse order: the
/// older answer is dropped, whichever mailbox it was for, and so is an older
/// read of the folder lists (specs/007-mail-storage/research.md §6).
#[test]
fn an_older_reads_answer_never_replaces_a_newer_ones() {
    let mut shown = ShownMailbox::default();
    let first = shown.start_read(&folder("First"));
    let second = shown.start_read(&folder("Second"));
    assert!(shown.finish_read(second, Ok(None)));
    let failure = Failure::from_panic(FailureKind::StoredMailUnreadable, None);
    assert!(!shown.finish_read(first, Err(failure.clone())));
    assert_eq!(shown.folder, Some(folder("Second")));
    assert!(matches!(shown.stored, StoredMailbox::Read(None)));

    let mut lists = FolderListsRead::default();
    let older = lists.start_read();
    let newer = lists.start_read();
    assert!(lists.finish_read(newer, &Ok::<(), Failure>(())));
    assert!(!lists.finish_read(older, &Err::<(), Failure>(failure)));
    assert!(lists.failure.is_none() && !lists.reading);
}

/// A panic in store work on GIO's pool is the failure of that work's own
/// kind, with the panic's message and place in the details
/// (specs/006-error-handling FR-014): a read's panic is an unreadable store,
/// never a refresh that stopped.
#[test]
fn a_panic_on_the_pool_is_the_failure_of_the_work_it_stopped() {
    let context = glib::MainContext::new();
    let answer = context
        .with_thread_default(|| {
            context.block_on(run_on_pool(
                FailureKind::StoredMailUnreadable,
                || -> Result<(), Failure> { panic!("a read panicked on purpose") },
            ))
        })
        .expect("the test owns its context");
    let failure = answer.expect_err("the work panicked");
    assert_eq!(failure.kind, FailureKind::StoredMailUnreadable);
    assert!(
        failure
            .details
            .starts_with("Failure: StoredMailUnreadable\nPanic: a read panicked on purpose at "),
        "{}",
        failure.details
    );
}

/// A hidden account's mail may be deleted, so the window forgets what it read
/// of it, and a read still running for it is dropped when it answers.
#[test]
fn a_hidden_accounts_mailbox_is_forgotten_with_its_read_in_flight() {
    let hidden = folder("INBOX");
    let mut shown = ShownMailbox::default();
    let read = shown.start_read(&hidden);
    shown.forget_excluded(|_| false);
    assert!(!shown.finish_read(read, Ok(Some(Vec::new()))));
    assert!(!shown.holds(&hidden));
}
