// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The store's promises: an Inbox read back as it was written, replaced
//! whole or not at all, deleted with its account, and a file that cannot be
//! used discarded at the first use.

use super::*;
use crate::test_record::CapturedRecord;
use mailbag_domain::{ContentExplanation, FailureKind, ReceivedContent};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

fn account(name: &str) -> AccountId {
    AccountId::try_from(name).expect("synthetic account id")
}

fn text_message(uid: u32) -> Message {
    Message {
        identity: format!("uid:{uid}"),
        fields: DisplayFields::default(),
        received_unix: None,
        seen: false,
        content: ReceivedContent::Text(format!("Text {uid}")),
    }
}

/// A directory of its own under the system's temporary directory, removed
/// with what it holds when dropped.
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "mailbag-store-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("a test directory");
        Self(path)
    }

    /// Where the application keeps its store, below this directory.
    fn store_path(&self) -> PathBuf {
        self.0.join("mailbag").join("mail.sqlite")
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

#[test]
fn every_content_code_and_field_reads_back_in_the_loads_order() {
    use ContentExplanation::*;
    let contents = [
        ReceivedContent::Text("Hello".to_owned()),
        ReceivedContent::Explained(NoPlainText { has_html: false }),
        ReceivedContent::Explained(NoPlainText { has_html: true }),
        ReceivedContent::Explained(Encrypted),
        ReceivedContent::Explained(SecuredWithSMime),
        ReceivedContent::Explained(UnknownCharset("x-unknown".to_owned())),
        ReceivedContent::Explained(UnknownEncoding("x-unknown".to_owned())),
        ReceivedContent::Explained(Undecodable),
        ReceivedContent::StructureUnreadable,
        ReceivedContent::TextNotReturned,
    ];
    // Newest first, as a load delivers them; absent fields stay absent.
    let messages: Vec<Message> = (0..)
        .zip(contents)
        .map(|(number, content)| Message {
            identity: format!("uid:{}", 100 - number),
            fields: DisplayFields {
                subject: (number % 2 == 0).then(|| format!("Subject {number}")),
                from: (number % 3 != 0).then(|| format!("Sender {number}")),
                to: (number % 4 != 0).then(|| format!("Recipient {number}")),
            },
            received_unix: (number % 5 != 0).then_some(1_700_000_000 + number),
            seen: number % 2 == 1,
            content,
        })
        .collect();
    let store = Store::in_memory();
    let loaded = account("loaded");
    assert_eq!(
        store.replace_inbox(&loaded, &messages, || false),
        Ok(InboxWrite::Stored)
    );
    assert_eq!(store.read_inbox(&loaded), Ok(Some(messages)));
}

#[test]
fn a_completed_load_replaces_only_its_own_accounts_inbox() {
    let store = Store::in_memory();
    let (refreshed, other) = (account("refreshed"), account("other"));
    let other_inbox = vec![text_message(7)];
    store.replace_inbox(&other, &other_inbox, || false).unwrap();
    store
        .replace_inbox(&refreshed, &[text_message(1), text_message(2)], || false)
        .unwrap();
    let newer = vec![text_message(3)];
    store.replace_inbox(&refreshed, &newer, || false).unwrap();
    assert_eq!(store.read_inbox(&refreshed), Ok(Some(newer)));
    assert_eq!(store.read_inbox(&other), Ok(Some(other_inbox)));
}

#[test]
fn an_empty_stored_inbox_is_not_an_inbox_never_loaded() {
    let store = Store::in_memory();
    let emptied = account("emptied");
    assert_eq!(store.read_inbox(&emptied), Ok(None));
    store.replace_inbox(&emptied, &[], || false).unwrap();
    assert_eq!(store.read_inbox(&emptied), Ok(Some(Vec::new())));
}

#[test]
fn a_cancelled_load_writes_nothing() {
    let store = Store::in_memory();
    let (stored, never_loaded) = (account("stored"), account("never-loaded"));
    let previous = vec![text_message(1)];
    store.replace_inbox(&stored, &previous, || false).unwrap();
    for account in [&stored, &never_loaded] {
        assert_eq!(
            store.replace_inbox(account, &[text_message(2)], || true),
            Ok(InboxWrite::LoadCancelled)
        );
    }
    assert_eq!(store.read_inbox(&stored), Ok(Some(previous)));
    assert_eq!(store.read_inbox(&never_loaded), Ok(None));
}

#[test]
fn keeping_accounts_deletes_every_other_accounts_mail_and_names_them() {
    let store = Store::in_memory();
    let (kept, removed, mail_off) = (account("kept"), account("removed"), account("mail-off"));
    for account in [&kept, &removed, &mail_off] {
        store
            .replace_inbox(account, &[text_message(1)], || false)
            .unwrap();
    }
    let deleted = store
        .keep_accounts(&BTreeSet::from([kept.clone()]))
        .unwrap();
    assert_eq!(
        BTreeSet::from_iter(deleted),
        BTreeSet::from([mail_off.clone(), removed.clone()])
    );
    assert_eq!(store.read_inbox(&removed), Ok(None));
    assert_eq!(store.read_inbox(&mail_off), Ok(None));
    assert_eq!(store.read_inbox(&kept), Ok(Some(vec![text_message(1)])));
}

#[test]
fn a_write_that_fails_midway_leaves_the_previous_inbox_whole() {
    let store = Store::in_memory();
    let refreshed = account("refreshed");
    let previous = vec![text_message(1), text_message(2)];
    store
        .replace_inbox(&refreshed, &previous, || false)
        .unwrap();
    store
        .with_connection(StoreOperation::Write, |connection| {
            Ok(connection.execute_batch(
                "CREATE TEMP TRIGGER fail_the_second_message BEFORE INSERT ON main.message \
                 WHEN (SELECT count(*) FROM main.message) = 1 \
                 BEGIN SELECT RAISE(ABORT, 'a failure for the test'); END;",
            )?)
        })
        .unwrap();
    let failure = store
        .replace_inbox(&refreshed, &[text_message(3), text_message(4)], || false)
        .unwrap_err();
    assert_eq!(failure.kind, FailureKind::MailNotSaved);
    assert!(
        failure
            .details
            .starts_with("Failure: MailNotSaved\nSQLite: "),
        "{}",
        failure.details
    );
    assert_eq!(store.read_inbox(&refreshed), Ok(Some(previous)));
}

#[test]
fn a_store_opened_again_from_its_file_reads_the_same_inbox() {
    let directory = TestDirectory::new();
    let loaded = account("loaded");
    let messages = vec![text_message(2), text_message(1)];
    Store::at(directory.store_path())
        .replace_inbox(&loaded, &messages, || false)
        .unwrap();
    let reopened = Store::at(directory.store_path());
    assert_eq!(reopened.read_inbox(&loaded), Ok(Some(messages)));
}

#[test]
fn a_store_that_cannot_be_used_starts_empty_with_one_warning_naming_why() {
    let written_by_another_build = |path: &Path| {
        let connection = Connection::open(path).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
    };
    let not_a_store = |path: &Path| fs::write(path, [0x5a_u8; 4096]).unwrap();
    // Every page after the first, which holds the list of tables.
    let damaged = |path: &Path| {
        let mut bytes = fs::read(path).unwrap();
        bytes[4096..].fill(0x5a);
        fs::write(path, bytes).unwrap();
    };
    /// Makes the store's file unusable in one way.
    type SpoilStore = fn(&Path);
    let cases: [(&str, SpoilStore); 3] = [
        ("structure changed", written_by_another_build),
        ("not a store", not_a_store),
        ("damaged", damaged),
    ];
    let loaded = account("loaded");
    for (reason, spoil) in cases {
        let directory = TestDirectory::new();
        let path = directory.store_path();
        let large: Vec<Message> = (1..=20)
            .map(|uid| Message {
                content: ReceivedContent::Text("x".repeat(1_000)),
                ..text_message(uid)
            })
            .collect();
        Store::at(path.clone())
            .replace_inbox(&loaded, &large, || false)
            .unwrap();
        spoil(&path);
        let record = CapturedRecord::start(tracing::Level::WARN);
        let store = Store::at(path);
        assert_eq!(store.read_inbox(&loaded), Ok(None), "{reason}");
        let warnings = record.lines_at("WARN");
        assert_eq!(warnings.len(), 1, "{reason}: {}", record.text());
        assert!(warnings[0].contains(reason), "{reason}: {}", warnings[0]);
        // The fresh store works.
        let stored = vec![text_message(1)];
        store.replace_inbox(&loaded, &stored, || false).unwrap();
        assert_eq!(store.read_inbox(&loaded), Ok(Some(stored)), "{reason}");
    }
}

#[test]
fn a_directory_that_cannot_be_created_fails_the_operation_and_deletes_nothing() {
    let directory = TestDirectory::new();
    // A file stands where the store's directory would be created.
    let blocking_file = directory.0.join("mailbag");
    fs::write(&blocking_file, "not a directory").unwrap();
    let store = Store::at(blocking_file.join("mail.sqlite"));
    let loaded = account("loaded");
    let read = store.read_inbox(&loaded).unwrap_err();
    assert_eq!(read.kind, FailureKind::StoredMailUnreadable);
    assert!(read.details.contains("\nFile: "), "{}", read.details);
    let write = store.replace_inbox(&loaded, &[], || false).unwrap_err();
    assert_eq!(write.kind, FailureKind::MailNotSaved);
    assert_eq!(
        fs::read_to_string(&blocking_file).unwrap(),
        "not a directory"
    );
}

#[test]
fn the_stores_directory_is_readable_by_the_user_only() {
    let directory = TestDirectory::new();
    let store = Store::at(directory.store_path());
    assert_eq!(store.read_inbox(&account("loaded")), Ok(None));
    let store_directory = fs::metadata(directory.0.join("mailbag")).unwrap();
    assert_eq!(store_directory.permissions().mode() & 0o777, 0o700);
}
