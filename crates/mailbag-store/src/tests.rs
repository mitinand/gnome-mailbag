// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The store's promises: a folder list and a folder's messages read back as
//! they were written, each replaced whole or not at all, a message kept once
//! whatever folders list it, everything deleted with its account, and a file
//! that cannot be used discarded at the first use.

use super::*;
use crate::{test_directory::TestDirectory, test_record::CapturedRecord};
use mailbag_domain::{ContentExplanation, DisplayFields, FailureKind, FolderRole, ReceivedContent};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

fn account(name: &str) -> AccountId {
    AccountId::try_from(name).expect("synthetic account id")
}

fn folder(identity: &str) -> Folder {
    Folder {
        identity: identity.to_owned(),
        name: identity.to_owned(),
        parent: None,
        attributes: Vec::new(),
        role: None,
        selectable: true,
    }
}

fn folder_of(account: &AccountId, identity: &str) -> FolderRef {
    FolderRef {
        account: account.clone(),
        identity: identity.to_owned(),
    }
}

fn text_message(identity: &str) -> Message {
    Message {
        identity: identity.to_owned(),
        fields: DisplayFields::default(),
        received_unix: None,
        seen: false,
        content: ReceivedContent::Text(format!("Text of {identity}")),
        labels: Vec::new(),
    }
}

/// The messages in the load's order, without UIDs.
fn in_order(messages: &[Message]) -> Vec<(Message, FolderMembership)> {
    (0..)
        .zip(messages)
        .map(|(position, message)| {
            (
                message.clone(),
                FolderMembership {
                    uid: None,
                    position,
                },
            )
        })
        .collect()
}

/// A store holding the account's folders, each loaded with its messages.
fn store_with(account: &AccountId, loaded: &[(&str, &[Message])]) -> Store {
    let store = Store::in_memory();
    let folders: Vec<Folder> = loaded
        .iter()
        .map(|(identity, _)| folder(identity))
        .collect();
    store.replace_folders(account, &folders, || false).unwrap();
    for (identity, messages) in loaded {
        store
            .replace_mailbox(&folder_of(account, identity), &in_order(messages), || false)
            .unwrap();
    }
    store
}

fn stored_message_count(store: &Store) -> i64 {
    store
        .with_connection(StoreOperation::Read, |connection| {
            Ok(connection.query_row("SELECT count(*) FROM message", [], |row| row.get(0))?)
        })
        .unwrap()
}

#[test]
fn a_folder_list_reads_back_with_its_attributes_roles_parents_and_containers() {
    let store = Store::in_memory();
    let listed = account("listed");
    let folders = vec![
        Folder {
            attributes: vec!["\\HasNoChildren".to_owned()],
            role: Some(FolderRole::Inbox),
            ..folder("INBOX")
        },
        Folder {
            attributes: vec!["\\HasChildren".to_owned(), "\\Noselect".to_owned()],
            selectable: false,
            ..folder("Projects")
        },
        Folder {
            name: "Sent".to_owned(),
            parent: Some("Projects".to_owned()),
            attributes: vec!["\\Junk".to_owned(), "\\Sent".to_owned()],
            role: Some(FolderRole::Junk),
            ..folder("Projects/Sent")
        },
    ];
    assert_eq!(
        store.replace_folders(&listed, &folders, || false),
        Ok(StoreWrite::Stored)
    );
    let mut stored = store.read_folders(&listed).unwrap();
    stored.sort_by(|left, right| left.folder.identity.cmp(&right.folder.identity));
    let expected: Vec<StoredFolder> = folders
        .into_iter()
        .map(|folder| StoredFolder {
            folder,
            loaded: false,
        })
        .collect();
    assert_eq!(stored, expected);
    assert_eq!(store.read_folders(&account("never-listed")), Ok(Vec::new()));
}

#[test]
fn a_new_folder_list_drops_unlisted_folders_with_their_own_mail_and_keeps_the_rest() {
    let listed = account("listed");
    let shared = text_message("shared");
    let store = store_with(
        &listed,
        &[
            ("Old", &[text_message("only-in-old"), shared.clone()]),
            ("Kept", std::slice::from_ref(&shared)),
        ],
    );
    let renamed_kept = Folder {
        name: "Kept 2026".to_owned(),
        ..folder("Kept")
    };
    store
        .replace_folders(&listed, &[renamed_kept.clone(), folder("New")], || false)
        .unwrap();
    let mut stored = store.read_folders(&listed).unwrap();
    stored.sort_by(|left, right| left.folder.identity.cmp(&right.folder.identity));
    assert_eq!(
        stored,
        [
            StoredFolder {
                folder: renamed_kept,
                loaded: true,
            },
            StoredFolder {
                folder: folder("New"),
                loaded: false,
            },
        ]
    );
    assert_eq!(store.read_mailbox(&folder_of(&listed, "Old")), Ok(None));
    assert_eq!(
        store.read_mailbox(&folder_of(&listed, "Kept")),
        Ok(Some(vec![shared]))
    );
    assert_eq!(stored_message_count(&store), 1);
}

#[test]
fn a_mailbox_reads_back_in_the_loads_order_with_every_field_uid_and_label() {
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
    let placed: Vec<(Message, FolderMembership)> = (0..)
        .zip(contents)
        .map(|(number, content)| {
            let message = Message {
                identity: format!("message-{number}"),
                fields: DisplayFields {
                    subject: (number % 2 == 0).then(|| format!("Subject {number}")),
                    from: (number % 3 != 0).then(|| format!("Sender {number}")),
                    to: (number % 4 != 0).then(|| format!("Recipient {number}")),
                },
                received_unix: (number % 5 != 0).then_some(1_700_000_000 + i64::from(number)),
                seen: number % 2 == 1,
                content,
                // A label name may hold a space.
                labels: match number {
                    0 => vec!["\\Important".to_owned(), "Project notes".to_owned()],
                    _ => Vec::new(),
                },
            };
            let membership = FolderMembership {
                uid: Some(100 - number),
                position: number,
            };
            (message, membership)
        })
        .collect();
    let store = Store::in_memory();
    let loaded = account("loaded");
    store
        .replace_folders(&loaded, &[folder("INBOX")], || false)
        .unwrap();
    let inbox = folder_of(&loaded, "INBOX");
    assert_eq!(
        store.replace_mailbox(&inbox, &placed, || false),
        Ok(StoreWrite::Stored)
    );
    let messages: Vec<Message> = placed.iter().map(|(message, _)| message.clone()).collect();
    assert_eq!(store.read_mailbox(&inbox), Ok(Some(messages)));
    let uids: Vec<u32> = store
        .with_connection(StoreOperation::Read, |connection| {
            Ok(connection
                .prepare("SELECT uid FROM membership ORDER BY position")?
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?)
        })
        .unwrap();
    assert_eq!(uids, (91..=100).rev().collect::<Vec<u32>>());
}

#[test]
fn a_new_load_of_a_mailbox_replaces_its_messages_and_deletes_those_left_nowhere() {
    let loaded = account("loaded");
    let store = store_with(
        &loaded,
        &[("INBOX", &[text_message("first"), text_message("second")])],
    );
    let newer = vec![text_message("third")];
    store
        .replace_mailbox(&folder_of(&loaded, "INBOX"), &in_order(&newer), || false)
        .unwrap();
    assert_eq!(
        store.read_mailbox(&folder_of(&loaded, "INBOX")),
        Ok(Some(newer))
    );
    assert_eq!(stored_message_count(&store), 1);
}

#[test]
fn a_message_in_two_folders_is_stored_once_with_its_latest_fields() {
    let loaded = account("loaded");
    let labelled = text_message("gmail:1");
    let store = store_with(
        &loaded,
        &[("Work", std::slice::from_ref(&labelled)), ("Travel", &[])],
    );
    let read_later = Message {
        seen: true,
        labels: vec!["Work".to_owned()],
        ..labelled
    };
    store
        .replace_mailbox(
            &folder_of(&loaded, "Travel"),
            &in_order(std::slice::from_ref(&read_later)),
            || false,
        )
        .unwrap();
    for identity in ["Work", "Travel"] {
        assert_eq!(
            store.read_mailbox(&folder_of(&loaded, identity)),
            Ok(Some(vec![read_later.clone()])),
            "{identity}"
        );
    }
    assert_eq!(stored_message_count(&store), 1);
}

#[test]
fn a_folder_never_loaded_is_not_an_empty_one() {
    let loaded = account("loaded");
    let store = store_with(&loaded, &[("Emptied", &[])]);
    store
        .replace_folders(&loaded, &[folder("Emptied"), folder("Unloaded")], || false)
        .unwrap();
    assert_eq!(
        store.read_mailbox(&folder_of(&loaded, "Emptied")),
        Ok(Some(Vec::new()))
    );
    assert_eq!(
        store.read_mailbox(&folder_of(&loaded, "Unloaded")),
        Ok(None)
    );
    assert_eq!(store.read_mailbox(&folder_of(&loaded, "Unknown")), Ok(None));
}

#[test]
fn a_load_of_a_folder_the_store_does_not_hold_is_not_saved() {
    let loaded = account("loaded");
    let store = store_with(&loaded, &[("INBOX", &[])]);
    let failure = store
        .replace_mailbox(
            &folder_of(&loaded, "Unknown"),
            &in_order(&[text_message("first")]),
            || false,
        )
        .unwrap_err();
    assert_eq!(failure.kind, FailureKind::MailNotSaved);
    assert_eq!(stored_message_count(&store), 0);
}

#[test]
fn a_cancelled_load_writes_nothing() {
    let loaded = account("loaded");
    let previous = vec![text_message("first")];
    let store = store_with(&loaded, &[("INBOX", &previous)]);
    assert_eq!(
        store.replace_folders(&loaded, &[folder("Other")], || true),
        Ok(StoreWrite::LoadCancelled)
    );
    assert_eq!(
        store.replace_mailbox(
            &folder_of(&loaded, "INBOX"),
            &in_order(&[text_message("second")]),
            || true
        ),
        Ok(StoreWrite::LoadCancelled)
    );
    assert_eq!(store.read_folders(&loaded).unwrap().len(), 1);
    assert_eq!(
        store.read_mailbox(&folder_of(&loaded, "INBOX")),
        Ok(Some(previous))
    );
}

#[test]
fn keeping_accounts_deletes_every_other_accounts_folders_and_mail_and_names_them() {
    let (kept, removed, mail_off) = (account("kept"), account("removed"), account("mail-off"));
    let store = store_with(&kept, &[("INBOX", &[text_message("kept")])]);
    for account in [&removed, &mail_off] {
        store
            .replace_folders(account, &[folder("INBOX")], || false)
            .unwrap();
        store
            .replace_mailbox(
                &folder_of(account, "INBOX"),
                &in_order(&[text_message("other")]),
                || false,
            )
            .unwrap();
    }
    let deleted = store
        .delete_other_accounts(&BTreeSet::from([kept.clone()]))
        .unwrap();
    assert_eq!(
        BTreeSet::from_iter(deleted),
        BTreeSet::from([mail_off.clone(), removed.clone()])
    );
    for account in [&removed, &mail_off] {
        assert_eq!(store.read_folders(account), Ok(Vec::new()));
    }
    assert_eq!(stored_message_count(&store), 1);
    assert_eq!(
        store.read_mailbox(&folder_of(&kept, "INBOX")),
        Ok(Some(vec![text_message("kept")]))
    );
}

#[test]
fn a_write_that_fails_midway_leaves_the_previous_state_whole() {
    let refreshed = account("refreshed");
    let previous = vec![text_message("first"), text_message("second")];
    let store = store_with(&refreshed, &[("INBOX", &previous)]);
    store
        .with_connection(StoreOperation::Write, |connection| {
            Ok(connection.execute_batch(
                "CREATE TEMP TRIGGER fail_the_fourth_message BEFORE INSERT ON main.message \
                 WHEN NEW.identity = 'fourth' \
                 BEGIN SELECT RAISE(ABORT, 'a failure for the test'); END;",
            )?)
        })
        .unwrap();
    let inbox = folder_of(&refreshed, "INBOX");
    let failure = store
        .replace_mailbox(
            &inbox,
            &in_order(&[text_message("third"), text_message("fourth")]),
            || false,
        )
        .unwrap_err();
    assert_eq!(failure.kind, FailureKind::MailNotSaved);
    assert!(
        failure
            .details
            .starts_with("Failure: MailNotSaved\nSQLite: "),
        "{}",
        failure.details
    );
    assert_eq!(store.read_mailbox(&inbox), Ok(Some(previous)));
}

/// A full disk is its own failure, so the window can advise freeing space,
/// and the previous state stays whole. The store's size limit stands in for
/// the disk: SQLite reports both as `SQLITE_FULL`.
#[test]
fn a_full_disk_is_storage_full_and_leaves_the_previous_state_whole() {
    let refreshed = account("refreshed");
    let previous = vec![text_message("first")];
    let store = store_with(&refreshed, &[("INBOX", &previous)]);
    store
        .with_connection(StoreOperation::Write, |connection| {
            let pages: i64 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
            Ok(connection.execute_batch(&format!("PRAGMA max_page_count = {pages}"))?)
        })
        .unwrap();
    let large = Message {
        content: ReceivedContent::Text("x".repeat(64 * 1024)),
        ..text_message("large")
    };
    let inbox = folder_of(&refreshed, "INBOX");
    let failure = store
        .replace_mailbox(&inbox, &in_order(&[large]), || false)
        .unwrap_err();
    assert_eq!(
        failure.kind,
        FailureKind::StorageFull,
        "{}",
        failure.details
    );
    assert!(
        failure
            .details
            .starts_with("Failure: StorageFull\nSQLite: "),
        "{}",
        failure.details
    );
    assert_eq!(store.read_mailbox(&inbox), Ok(Some(previous)));
}

#[test]
fn a_store_opened_again_from_its_file_reads_the_same_mail() {
    let directory = TestDirectory::new();
    let loaded = account("loaded");
    let messages = vec![text_message("second"), text_message("first")];
    let store = Store::at(directory.store_path());
    store
        .replace_folders(&loaded, &[folder("INBOX")], || false)
        .unwrap();
    store
        .replace_mailbox(&folder_of(&loaded, "INBOX"), &in_order(&messages), || false)
        .unwrap();
    drop(store);
    let reopened = Store::at(directory.store_path());
    assert_eq!(
        reopened.read_mailbox(&folder_of(&loaded, "INBOX")),
        Ok(Some(messages))
    );
}

/// A store of another structure, such as one written before folders, is
/// discarded like one that is not a store or is damaged.
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
    let inbox = folder_of(&loaded, "INBOX");
    for (reason, spoil) in cases {
        let directory = TestDirectory::new();
        let path = directory.store_path();
        let large: Vec<Message> = (1..=20)
            .map(|number| Message {
                content: ReceivedContent::Text("x".repeat(1_000)),
                ..text_message(&format!("message-{number}"))
            })
            .collect();
        let store = Store::at(path.clone());
        store
            .replace_folders(&loaded, &[folder("INBOX")], || false)
            .unwrap();
        store
            .replace_mailbox(&inbox, &in_order(&large), || false)
            .unwrap();
        drop(store);
        spoil(&path);
        let record = CapturedRecord::start(tracing::Level::WARN);
        let store = Store::at(path);
        assert_eq!(store.read_folders(&loaded), Ok(Vec::new()), "{reason}");
        let warnings = record.lines_at("WARN");
        assert_eq!(warnings.len(), 1, "{reason}: {}", record.text());
        assert!(warnings[0].contains(reason), "{reason}: {}", warnings[0]);
        // The fresh store works.
        assert_eq!(
            store.replace_folders(&loaded, &[folder("INBOX")], || false),
            Ok(StoreWrite::Stored),
            "{reason}"
        );
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
    let read = store.read_folders(&loaded).unwrap_err();
    assert_eq!(read.kind, FailureKind::StoredMailUnreadable);
    assert!(read.details.contains("\nFile: "), "{}", read.details);
    let write = store
        .replace_folders(&loaded, &[folder("INBOX")], || false)
        .unwrap_err();
    assert_eq!(write.kind, FailureKind::MailNotSaved);
    assert_eq!(
        fs::read_to_string(&blocking_file).unwrap(),
        "not a directory"
    );
}

#[test]
fn the_stores_directory_is_readable_by_the_user_only() {
    // A new directory, and one that existed with wider rights, as a copy
    // restored from a backup may.
    for existing_rights in [None, Some(0o755)] {
        let directory = TestDirectory::new();
        let store_directory = directory.0.join("mailbag");
        if let Some(rights) = existing_rights {
            fs::create_dir(&store_directory).unwrap();
            fs::set_permissions(&store_directory, fs::Permissions::from_mode(rights)).unwrap();
        }
        let store = Store::at(directory.store_path());
        assert_eq!(store.read_folders(&account("loaded")), Ok(Vec::new()));
        let rights = fs::metadata(&store_directory).unwrap().permissions().mode() & 0o777;
        assert_eq!(rights, 0o700, "{existing_rights:?}");
    }
}
