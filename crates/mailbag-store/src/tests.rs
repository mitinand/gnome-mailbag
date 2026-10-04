// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The store's promises: a folder list and a folder's messages read back as
//! they were written, each replaced whole or not at all, a message kept once
//! whatever folders list it, everything deleted with its account, and a file
//! that cannot be used discarded at the first use.

use super::*;
use crate::{test_directory::TestDirectory, test_record::CapturedRecord};
use mailbag_domain::{
    ContentExplanation, DisplayFields, FailureKind, FolderBatch, FolderNumbers, FolderRole,
    FolderState, Message, MessageFlag, MessageFlags, PendingChange, ReceivedContent,
};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

fn account(name: &str) -> AccountId {
    AccountId::try_from(name).expect("synthetic account id")
}

fn folder(identity: &str) -> Folder {
    Folder {
        identity: identity.to_owned(),
        name: identity.to_owned(),
        parent: None,
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
        flagged: false,
        content: ReceivedContent::Text(format!("Text of {identity}")),
        preview: format!("Preview of {identity}"),
    }
}

/// A text message received `day` days after the epoch, so rows read in the
/// order of their days, newest first.
fn dated_message(identity: &str, day: i64) -> Message {
    Message {
        received_unix: Some(day * 86_400),
        ..text_message(identity)
    }
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
        store_completed_cycle(&store, &folder_of(account, identity), messages, || false).unwrap();
    }
    store
}

/// The folder's stored messages whole, as the list reads their rows and the
/// reader their contents.
fn read_stored_messages(
    store: &Store,
    folder: &FolderRef,
) -> Result<Option<Vec<Message>>, Failure> {
    let Some(rows) = store.read_folder_rows(folder)? else {
        return Ok(None);
    };
    let messages = rows
        .into_iter()
        .map(|row| {
            let content = store
                .read_message_content(&folder.account, &row.identity)?
                .expect("a listed message is stored");
            Ok(Message {
                identity: row.identity,
                fields: row.fields,
                received_unix: row.received_unix,
                seen: row.seen,
                flagged: row.flagged,
                content,
                preview: row.preview,
            })
        })
        .collect::<Result<_, Failure>>()?;
    Ok(Some(messages))
}

/// The flags of a message that is not starred.
fn read_flags(seen: bool) -> MessageFlags {
    MessageFlags {
        seen,
        flagged: false,
    }
}

fn stored_message_count(store: &Store) -> i64 {
    store
        .with_connection(StoreOperation::Read, |connection| {
            Ok(connection.query_row("SELECT count(*) FROM message", [], |row| row.get(0))?)
        })
        .unwrap()
}

#[test]
fn a_folder_list_reads_back_with_every_role_its_parents_and_containers() {
    let store = Store::in_memory();
    let listed = account("listed");
    let mut folders = vec![
        Folder {
            selectable: false,
            ..folder("Projects")
        },
        Folder {
            name: "Reports".to_owned(),
            parent: Some("Projects".to_owned()),
            ..folder("Projects/Reports")
        },
    ];
    for role in FolderRole::ORDER {
        folders.push(Folder {
            role: Some(role),
            ..folder(&format!("{role:?}"))
        });
    }
    assert_eq!(
        store.replace_folders(&listed, &folders, || false),
        Ok(StoreWrite::Stored)
    );
    let mut stored = store.read_folders(&listed).unwrap();
    stored.sort_by(|left, right| left.identity.cmp(&right.identity));
    folders.sort_by(|left, right| left.identity.cmp(&right.identity));
    assert_eq!(stored, folders);
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
    stored.sort_by(|left, right| left.identity.cmp(&right.identity));
    assert_eq!(stored, [renamed_kept, folder("New")]);
    assert_eq!(
        read_stored_messages(&store, &folder_of(&listed, "Old")),
        Ok(None)
    );
    assert_eq!(
        read_stored_messages(&store, &folder_of(&listed, "New")),
        Ok(None)
    );
    assert_eq!(
        read_stored_messages(&store, &folder_of(&listed, "Kept")),
        Ok(Some(vec![shared]))
    );
    assert_eq!(stored_message_count(&store), 1);
}

#[test]
fn a_mailbox_reads_back_newest_first_with_every_field() {
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
    // Older as the number grows; absent fields stay absent.
    let messages: Vec<Message> = (0..)
        .zip(contents)
        .map(|(number, content)| Message {
            identity: format!("message-{number}"),
            fields: DisplayFields {
                subject: (number % 2 == 0).then(|| format!("Subject {number}")),
                from: (number % 3 != 0).then(|| format!("Sender {number}")),
                to: (number % 4 != 0).then(|| format!("Recipient {number}")),
            },
            received_unix: (number % 5 != 0).then_some(1_700_000_000 - i64::from(number)),
            seen: number % 2 == 1,
            flagged: number % 3 == 1,
            content,
            preview: if number % 3 == 0 {
                String::new()
            } else {
                format!("Preview {number}")
            },
        })
        .collect();
    let store = Store::in_memory();
    let loaded = account("loaded");
    store
        .replace_folders(&loaded, &[folder("INBOX")], || false)
        .unwrap();
    let inbox = folder_of(&loaded, "INBOX");
    assert_eq!(
        store_completed_cycle(&store, &inbox, &messages, || false),
        Ok(StoreWrite::Stored)
    );
    // Newest first; messages without a date last, the last stored first.
    let newest_first = [1, 2, 3, 4, 6, 7, 8, 9, 5, 0].map(|number| messages[number].clone());
    assert_eq!(
        read_stored_messages(&store, &inbox),
        Ok(Some(newest_first.to_vec()))
    );
}

#[test]
fn a_content_is_read_by_its_message_and_a_message_no_longer_stored_has_none() {
    let loaded = account("loaded");
    let store = store_with(&loaded, &[("INBOX", &[text_message("first")])]);
    assert_eq!(
        store.read_message_content(&loaded, "first"),
        Ok(Some(ReceivedContent::Text("Text of first".to_owned())))
    );
    store_completed_cycle(&store, &folder_of(&loaded, "INBOX"), &[], || false).unwrap();
    assert_eq!(store.read_message_content(&loaded, "first"), Ok(None));
    // Another account's message of the same identity is not this one's.
    let other = store_with(&account("other"), &[("INBOX", &[text_message("first")])]);
    assert_eq!(other.read_message_content(&loaded, "first"), Ok(None));
}

#[test]
fn a_new_load_of_a_mailbox_replaces_its_messages_and_deletes_those_left_nowhere() {
    let loaded = account("loaded");
    let store = store_with(
        &loaded,
        &[("INBOX", &[text_message("first"), text_message("second")])],
    );
    let newer = vec![text_message("third")];
    store_completed_cycle(&store, &folder_of(&loaded, "INBOX"), &newer, || false).unwrap();
    assert_eq!(
        read_stored_messages(&store, &folder_of(&loaded, "INBOX")),
        Ok(Some(newer))
    );
    assert_eq!(stored_message_count(&store), 1);
}

#[test]
fn a_message_in_two_folders_is_stored_once_with_its_latest_fields_and_preview() {
    let loaded = account("loaded");
    let labelled = text_message("gmail:1");
    let store = store_with(
        &loaded,
        &[("Work", std::slice::from_ref(&labelled)), ("Travel", &[])],
    );
    let read_later = Message {
        seen: true,
        preview: "Edited elsewhere".to_owned(),
        ..labelled
    };
    store_completed_cycle(
        &store,
        &folder_of(&loaded, "Travel"),
        std::slice::from_ref(&read_later),
        || false,
    )
    .unwrap();
    for identity in ["Work", "Travel"] {
        assert_eq!(
            read_stored_messages(&store, &folder_of(&loaded, identity)),
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
        read_stored_messages(&store, &folder_of(&loaded, "Emptied")),
        Ok(Some(Vec::new()))
    );
    assert_eq!(
        read_stored_messages(&store, &folder_of(&loaded, "Unloaded")),
        Ok(None)
    );
    assert_eq!(
        read_stored_messages(&store, &folder_of(&loaded, "Unknown")),
        Ok(None)
    );
}

#[test]
fn a_load_of_a_folder_the_store_does_not_hold_is_not_saved() {
    let loaded = account("loaded");
    let store = store_with(&loaded, &[("INBOX", &[])]);
    let failure = store_completed_cycle(
        &store,
        &folder_of(&loaded, "Unknown"),
        &[text_message("first")],
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
        store_completed_cycle(
            &store,
            &folder_of(&loaded, "INBOX"),
            &[text_message("second")],
            || true
        ),
        Ok(StoreWrite::LoadCancelled)
    );
    assert_eq!(store.read_folders(&loaded).unwrap().len(), 1);
    assert_eq!(
        read_stored_messages(&store, &folder_of(&loaded, "INBOX")),
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
        store_completed_cycle(
            &store,
            &folder_of(account, "INBOX"),
            &[text_message("other")],
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
        read_stored_messages(&store, &folder_of(&kept, "INBOX")),
        Ok(Some(vec![text_message("kept")]))
    );
}

#[test]
fn a_write_that_fails_midway_leaves_the_previous_state_whole() {
    let refreshed = account("refreshed");
    let previous = vec![dated_message("second", 2), dated_message("first", 1)];
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
    let failure = store_completed_cycle(
        &store,
        &inbox,
        &[text_message("third"), text_message("fourth")],
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
    assert_eq!(read_stored_messages(&store, &inbox), Ok(Some(previous)));
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
    let failure = store_completed_cycle(&store, &inbox, &[large], || false).unwrap_err();
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
    assert_eq!(read_stored_messages(&store, &inbox), Ok(Some(previous)));
}

#[test]
fn a_store_opened_again_from_its_file_reads_the_same_mail() {
    let directory = TestDirectory::new();
    let loaded = account("loaded");
    let messages = vec![dated_message("second", 2), dated_message("first", 1)];
    let store = Store::at(directory.store_path());
    store
        .replace_folders(&loaded, &[folder("INBOX")], || false)
        .unwrap();
    store_completed_cycle(&store, &folder_of(&loaded, "INBOX"), &messages, || false).unwrap();
    drop(store);
    let reopened = Store::at(directory.store_path());
    assert_eq!(
        read_stored_messages(&reopened, &folder_of(&loaded, "INBOX")),
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
        store_completed_cycle(&store, &inbox, &large, || false).unwrap();
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

/// A store listing the account's folders, none synchronized yet.
fn store_listing(account: &AccountId, folders: &[&str]) -> Store {
    let store = Store::in_memory();
    let listed: Vec<Folder> = folders.iter().map(|identity| folder(identity)).collect();
    store.replace_folders(account, &listed, || false).unwrap();
    store
}

fn completed(server_position: Option<&str>) -> Option<FolderState> {
    Some(FolderState {
        server_position: server_position.map(str::to_owned),
        fill_place: None,
        synchronized: true,
        numbers: None,
    })
}

fn identities_of(store: &Store, folder: &FolderRef) -> Vec<String> {
    store
        .read_folder_rows(folder)
        .unwrap()
        .expect("the folder has rows")
        .into_iter()
        .map(|row| row.identity)
        .collect()
}

#[test]
fn a_batch_removes_changes_relates_and_adds_in_one_write() {
    let synced = account("synced");
    let store = store_listing(&synced, &["INBOX", "Work"]);
    let (inbox, work) = (folder_of(&synced, "INBOX"), folder_of(&synced, "Work"));
    let first = FolderBatch {
        arrived: vec![
            dated_message("kept", 3),
            dated_message("gone", 2),
            dated_message("read", 1),
        ],
        state: completed(None),
        ..FolderBatch::default()
    };
    store.store_batch(&inbox, &first, || false).unwrap();
    let elsewhere = FolderBatch {
        arrived: vec![dated_message("shared", 4)],
        ..FolderBatch::default()
    };
    store.store_batch(&work, &elsewhere, || false).unwrap();

    let second = FolderBatch {
        removed: vec!["gone".to_owned()],
        flag_states: vec![("read".to_owned(), read_flags(true))],
        known_arrived: vec![("shared".to_owned(), read_flags(true))],
        arrived: vec![dated_message("new", 5)],
        state: None,
    };
    assert_eq!(
        store.store_batch(&inbox, &second, || false),
        Ok(StoreWrite::Stored)
    );
    assert_eq!(
        identities_of(&store, &inbox),
        ["new", "shared", "kept", "read"]
    );
    let sync = store.read_folder_sync(&inbox).unwrap();
    assert_eq!(
        sync.stored,
        HashMap::from(
            [
                ("new", false),
                ("shared", true),
                ("kept", false),
                ("read", true)
            ]
            .map(|(identity, seen)| (identity.to_owned(), read_flags(seen)))
        )
    );
    // The removed message was in no other folder, so it is gone; the shared
    // one stays in its first folder too.
    assert_eq!(store.read_message_content(&synced, "gone"), Ok(None));
    assert_eq!(identities_of(&store, &work), ["shared"]);
    assert_eq!(stored_message_count(&store), 4);
}

#[test]
fn a_related_or_read_message_keeps_its_preview() {
    let synced = account("synced");
    let store = store_listing(&synced, &["INBOX", "Work"]);
    let (inbox, work) = (folder_of(&synced, "INBOX"), folder_of(&synced, "Work"));
    let elsewhere = FolderBatch {
        arrived: vec![dated_message("shared", 1)],
        ..FolderBatch::default()
    };
    store.store_batch(&work, &elsewhere, || false).unwrap();
    let related = FolderBatch {
        known_arrived: vec![("shared".to_owned(), read_flags(true))],
        flag_states: vec![("shared".to_owned(), read_flags(true))],
        ..FolderBatch::default()
    };
    store.store_batch(&inbox, &related, || false).unwrap();
    for folder in [&inbox, &work] {
        let rows = store.read_folder_rows(folder).unwrap().unwrap();
        assert_eq!(rows[0].preview, "Preview of shared", "{folder:?}");
    }
}

#[test]
fn a_text_not_downloaded_never_replaces_a_stored_content() {
    let synced = account("synced");
    let store = store_listing(&synced, &["INBOX", "Archive"]);
    let recent = dated_message("message", 1);
    let with_text = FolderBatch {
        arrived: vec![recent.clone()],
        ..FolderBatch::default()
    };
    store
        .store_batch(&folder_of(&synced, "INBOX"), &with_text, || false)
        .unwrap();
    let without_text = FolderBatch {
        arrived: vec![Message {
            seen: true,
            content: ReceivedContent::NotDownloaded,
            ..recent.clone()
        }],
        ..FolderBatch::default()
    };
    store
        .store_batch(&folder_of(&synced, "Archive"), &without_text, || false)
        .unwrap();
    assert_eq!(
        store.read_message_content(&synced, "message"),
        Ok(Some(recent.content.clone()))
    );
    // Its fields are the latest record's all the same: it is read now.
    let inbox = store
        .read_folder_sync(&folder_of(&synced, "INBOX"))
        .unwrap();
    assert!(inbox.stored["message"].seen);
    // A text replaces a text.
    let newer = FolderBatch {
        arrived: vec![Message {
            content: ReceivedContent::Text("Newer".to_owned()),
            ..recent
        }],
        ..FolderBatch::default()
    };
    store
        .store_batch(&folder_of(&synced, "Archive"), &newer, || false)
        .unwrap();
    assert_eq!(
        store.read_message_content(&synced, "message"),
        Ok(Some(ReceivedContent::Text("Newer".to_owned())))
    );
}

/// A text the service did not return, for a message whose fields it
/// reported again, leaves the stored text in place; it replaces any other
/// content, and a text replaces it.
#[test]
fn a_text_not_returned_never_replaces_a_stored_text() {
    let synced = account("synced");
    let store = store_listing(&synced, &["INBOX"]);
    let inbox = folder_of(&synced, "INBOX");
    let recent = dated_message("message", 1);
    let store_content = |content: ReceivedContent| {
        let batch = FolderBatch {
            arrived: vec![Message {
                content,
                ..recent.clone()
            }],
            ..FolderBatch::default()
        };
        store.store_batch(&inbox, &batch, || false).unwrap();
        store.read_message_content(&synced, "message").unwrap()
    };
    let text = ReceivedContent::Text("Text".to_owned());
    assert_eq!(store_content(text.clone()), Some(text.clone()));
    assert_eq!(
        store_content(ReceivedContent::TextNotReturned),
        Some(text.clone())
    );
    assert_eq!(store_content(ReceivedContent::NotDownloaded), Some(text));
    assert_eq!(
        store_content(ReceivedContent::StructureUnreadable),
        Some(ReceivedContent::StructureUnreadable)
    );
    assert_eq!(
        store_content(ReceivedContent::TextNotReturned),
        Some(ReceivedContent::TextNotReturned)
    );
    let newer = ReceivedContent::Text("Newer".to_owned());
    assert_eq!(store_content(newer.clone()), Some(newer));
}

#[test]
fn the_folder_state_changes_only_with_a_batch_that_carries_it() {
    let synced = account("synced");
    let store = store_listing(&synced, &["INBOX"]);
    let inbox = folder_of(&synced, "INBOX");
    let never = FolderState::default();
    assert_eq!(store.read_folder_sync(&inbox).unwrap().state, never);
    let completing = FolderBatch {
        state: completed(Some("position")),
        ..FolderBatch::default()
    };
    store.store_batch(&inbox, &completing, || false).unwrap();
    let without_state = FolderBatch {
        arrived: vec![dated_message("message", 1)],
        ..FolderBatch::default()
    };
    store.store_batch(&inbox, &without_state, || false).unwrap();
    assert_eq!(
        store.read_folder_sync(&inbox).unwrap().state,
        completed(Some("position")).unwrap()
    );
    // A new folder list keeps a listed folder's state.
    store
        .replace_folders(&synced, &[folder("INBOX")], || false)
        .unwrap();
    assert_eq!(
        store.read_folder_sync(&inbox).unwrap().state,
        completed(Some("position")).unwrap()
    );
}

#[test]
fn a_cancelled_or_failing_batch_leaves_the_folder_as_it_was() {
    let synced = account("synced");
    let store = store_listing(&synced, &["INBOX"]);
    let inbox = folder_of(&synced, "INBOX");
    let first = FolderBatch {
        arrived: vec![dated_message("first", 1)],
        state: completed(None),
        ..FolderBatch::default()
    };
    store.store_batch(&inbox, &first, || false).unwrap();
    let before = store.read_folder_sync(&inbox).unwrap();
    let changes = FolderBatch {
        removed: vec!["first".to_owned()],
        arrived: vec![dated_message("second", 2), dated_message("third", 3)],
        state: Some(FolderState::default()),
        ..FolderBatch::default()
    };
    assert_eq!(
        store.store_batch(&inbox, &changes, || true),
        Ok(StoreWrite::LoadCancelled)
    );
    assert_eq!(store.read_folder_sync(&inbox).unwrap(), before);
    store
        .with_connection(StoreOperation::Write, |connection| {
            Ok(connection.execute_batch(
                "CREATE TEMP TRIGGER fail_the_third_message BEFORE INSERT ON main.message \
                 WHEN NEW.identity = 'third' \
                 BEGIN SELECT RAISE(ABORT, 'a failure for the test'); END;",
            )?)
        })
        .unwrap();
    let failure = store.store_batch(&inbox, &changes, || false).unwrap_err();
    assert_eq!(failure.kind, FailureKind::MailNotSaved);
    assert_eq!(store.read_folder_sync(&inbox).unwrap(), before);
    assert_eq!(identities_of(&store, &inbox), ["first"]);
}

#[test]
fn a_folder_the_store_does_not_hold_takes_no_batch() {
    let synced = account("synced");
    let store = store_listing(&synced, &["INBOX"]);
    let unknown = folder_of(&synced, "Unknown");
    assert_eq!(
        store.read_folder_sync(&unknown).unwrap_err().kind,
        FailureKind::MailNotSaved
    );
    let batch = FolderBatch {
        arrived: vec![dated_message("message", 1)],
        ..FolderBatch::default()
    };
    assert_eq!(
        store
            .store_batch(&unknown, &batch, || false)
            .unwrap_err()
            .kind,
        FailureKind::MailNotSaved
    );
    assert_eq!(stored_message_count(&store), 0);
}

/// "No mail loaded" and an empty folder differ: a folder shows rows once a
/// batch stored some, and an empty list only after a completed cycle.
#[test]
fn a_folder_without_rows_is_empty_only_after_a_completed_cycle() {
    let synced = account("synced");
    let store = store_listing(&synced, &["INBOX"]);
    let inbox = folder_of(&synced, "INBOX");
    assert_eq!(store.read_folder_rows(&inbox), Ok(None));
    let not_completed = Some(FolderState::default());
    let started = FolderBatch {
        state: not_completed.clone(),
        ..FolderBatch::default()
    };
    store.store_batch(&inbox, &started, || false).unwrap();
    assert_eq!(store.read_folder_rows(&inbox), Ok(None));
    let some_rows = FolderBatch {
        arrived: vec![dated_message("message", 1)],
        ..FolderBatch::default()
    };
    store.store_batch(&inbox, &some_rows, || false).unwrap();
    assert_eq!(identities_of(&store, &inbox), ["message"]);
    let emptied = FolderBatch {
        removed: vec!["message".to_owned()],
        state: completed(None),
        ..FolderBatch::default()
    };
    store.store_batch(&inbox, &emptied, || false).unwrap();
    assert_eq!(store.read_folder_rows(&inbox), Ok(Some(Vec::new())));
    assert_eq!(
        store.read_folder_rows(&folder_of(&synced, "Unknown")),
        Ok(None)
    );
}

#[test]
fn stored_identities_are_those_any_folder_of_the_account_holds() {
    let synced = account("synced");
    let store = store_listing(&synced, &["INBOX", "Work"]);
    for (identity, name) in [("in-inbox", "INBOX"), ("in-work", "Work")] {
        let batch = FolderBatch {
            arrived: vec![dated_message(identity, 1)],
            ..FolderBatch::default()
        };
        store
            .store_batch(&folder_of(&synced, name), &batch, || false)
            .unwrap();
    }
    let other = account("other");
    let asked = ["in-inbox", "in-work", "unknown"].map(str::to_owned);
    assert_eq!(
        store.stored_identities(&synced, &asked),
        Ok(HashSet::from(["in-inbox", "in-work"].map(str::to_owned)))
    );
    assert_eq!(store.stored_identities(&other, &asked), Ok(HashSet::new()));
}

#[test]
fn identities_in_other_folders_are_those_another_folder_of_the_account_holds() {
    let synced = account("synced");
    let store = store_listing(&synced, &["INBOX", "Archive"]);
    let (inbox, archive) = (folder_of(&synced, "INBOX"), folder_of(&synced, "Archive"));
    let store_in = |folder: &FolderRef, identity: &str| {
        let batch = FolderBatch {
            arrived: vec![dated_message(identity, 1)],
            ..FolderBatch::default()
        };
        store.store_batch(folder, &batch, || false).unwrap();
    };
    store_in(&inbox, "only-here");
    store_in(&archive, "only-there");
    store_in(&inbox, "both");
    store_in(&archive, "both");
    let asked = ["only-here", "only-there", "both", "unknown"].map(str::to_owned);
    assert_eq!(
        store.identities_in_other_folders(&inbox, &asked),
        Ok(HashSet::from(["only-there", "both"].map(str::to_owned)))
    );
}

/// The numbers of a folder's latest state pass are kept with its state,
/// survive a folder list replacement, and are absent until a pass stored
/// them (specs/009-synchronization FR-005, data model).
#[test]
fn a_folders_pass_numbers_are_kept_with_its_state() {
    let synced = account("synced");
    let inbox = folder_of(&synced, "INBOX");
    let store = store_listing(&synced, &["INBOX"]);
    let stored_numbers = || store.read_folder_sync(&inbox).unwrap().state.numbers;
    assert_eq!(stored_numbers(), None);
    let numbers = FolderNumbers {
        uid_validity: Some(7),
        message_count: 3,
        uid_next: Some(41),
        highest_modseq: Some(1 << 40),
    };
    let state_with = |numbers| FolderBatch {
        state: Some(FolderState {
            synchronized: true,
            numbers,
            ..FolderState::default()
        }),
        ..FolderBatch::default()
    };
    store
        .store_batch(&inbox, &state_with(Some(numbers)), || false)
        .unwrap();
    assert_eq!(stored_numbers(), Some(numbers));
    // A new folder list keeps the folder's state, the numbers included.
    store
        .replace_folders(&synced, &[folder("INBOX"), folder("Work")], || false)
        .unwrap();
    assert_eq!(stored_numbers(), Some(numbers));
    // A server without CONDSTORE, UIDVALIDITY or UIDNEXT leaves those empty.
    let count_only = FolderNumbers {
        uid_validity: None,
        uid_next: None,
        highest_modseq: None,
        ..numbers
    };
    store
        .store_batch(&inbox, &state_with(Some(count_only)), || false)
        .unwrap();
    assert_eq!(stored_numbers(), Some(count_only));
    // A state without numbers, as Microsoft 365 writes, clears them.
    store
        .store_batch(&inbox, &state_with(None), || false)
        .unwrap();
    assert_eq!(stored_numbers(), None);
}

/// Stores `messages` as the folder's whole content, as a completed cycle
/// leaves it: stored messages not among them leave.
fn store_completed_cycle(
    store: &Store,
    folder: &FolderRef,
    messages: &[Message],
    load_cancelled: impl FnOnce() -> bool,
) -> Result<StoreWrite, Failure> {
    let removed = store
        .read_folder_rows(folder)?
        .unwrap_or_default()
        .into_iter()
        .map(|row| row.identity)
        .filter(|identity| !messages.iter().any(|message| message.identity == *identity))
        .collect();
    let batch = FolderBatch {
        removed,
        arrived: messages.to_vec(),
        state: Some(FolderState {
            server_position: None,
            fill_place: None,
            synchronized: true,
            numbers: None,
        }),
        ..FolderBatch::default()
    };
    store.store_batch(folder, &batch, load_cancelled)
}

/// A message with the server's flags, received on day 1.
fn message_with_flags(identity: &str, seen: bool, flagged: bool) -> Message {
    Message {
        seen,
        flagged,
        ..dated_message(identity, 1)
    }
}

fn pending(identity: &str, flag: MessageFlag, wanted: bool) -> PendingChange {
    PendingChange {
        identity: identity.to_owned(),
        flag,
        wanted,
    }
}

/// The folder's pending changes by identity, the read state first.
fn pending_of(store: &Store, folder: &FolderRef) -> Vec<PendingChange> {
    let mut changes = store.read_pending_changes(folder).unwrap();
    changes.sort_by_key(|change| (change.identity.clone(), change.flag == MessageFlag::Flagged));
    changes
}

/// Each row's identity, read state and star, by identity.
fn flags_of(store: &Store, folder: &FolderRef) -> Vec<(String, bool, bool)> {
    let mut rows: Vec<_> = store
        .read_folder_rows(folder)
        .unwrap()
        .expect("the folder has rows")
        .into_iter()
        .map(|row| (row.identity, row.seen, row.flagged))
        .collect();
    rows.sort();
    rows
}

#[test]
fn rows_show_the_wanted_flags_and_a_cycle_reads_the_servers() {
    use MessageFlag::{Flagged, Seen};
    let synced = account("synced");
    let inbox = folder_of(&synced, "INBOX");
    let messages = [
        message_with_flags("none-pending", false, true),
        message_with_flags("both-set", false, false),
        message_with_flags("both-cleared", true, true),
        message_with_flags("replaced", false, false),
    ];
    let store = store_with(&synced, &[("INBOX", &messages)]);
    let wishes = [
        ("both-set", Seen, true),
        ("both-set", Flagged, true),
        ("both-cleared", Seen, false),
        ("both-cleared", Flagged, false),
        // A newer wish replaces the older, also one equal to the server's.
        ("replaced", Flagged, true),
        ("replaced", Flagged, false),
    ];
    for (identity, flag, wanted) in wishes {
        store
            .write_pending_flag(&synced, identity, flag, wanted)
            .unwrap();
    }
    assert_eq!(
        flags_of(&store, &inbox),
        [
            ("both-cleared".to_owned(), false, false),
            ("both-set".to_owned(), true, true),
            ("none-pending".to_owned(), false, true),
            ("replaced".to_owned(), false, false),
        ]
    );
    let sync = store.read_folder_sync(&inbox).unwrap();
    assert_eq!(sync.stored["both-set"], read_flags(false));
    assert_eq!(
        pending_of(&store, &inbox),
        [
            pending("both-cleared", Seen, false),
            pending("both-cleared", Flagged, false),
            pending("both-set", Seen, true),
            pending("both-set", Flagged, true),
            pending("replaced", Flagged, false),
        ]
    );
}

#[test]
fn a_server_value_written_leaves_the_pending_values() {
    use MessageFlag::{Flagged, Seen};
    let synced = account("synced");
    let (inbox, work) = (folder_of(&synced, "INBOX"), folder_of(&synced, "Work"));
    let in_inbox = ["named", "differing", "arrived"]
        .map(|identity| message_with_flags(identity, false, false));
    let store = store_with(
        &synced,
        &[
            ("INBOX", &in_inbox),
            ("Work", &[message_with_flags("related", false, false)]),
        ],
    );
    let wishes = [
        ("named", Seen, true),
        ("named", Flagged, true),
        ("differing", Flagged, true),
        ("arrived", Seen, true),
        ("related", Flagged, true),
    ];
    for (identity, flag, wanted) in wishes {
        store
            .write_pending_flag(&synced, identity, flag, wanted)
            .unwrap();
    }
    // Each write of the report reaches the server value, equal to the wish
    // or not, and none ends the wish.
    let report = FolderBatch {
        flag_states: vec![
            // Both flags as the server holds them: read and unstarred.
            ("named".to_owned(), read_flags(true)),
            ("differing".to_owned(), read_flags(false)),
        ],
        arrived: vec![message_with_flags("arrived", true, false)],
        known_arrived: vec![(
            "related".to_owned(),
            MessageFlags {
                seen: false,
                flagged: true,
            },
        )],
        ..FolderBatch::default()
    };
    store.store_batch(&inbox, &report, || false).unwrap();
    assert_eq!(
        pending_of(&store, &inbox),
        [
            pending("arrived", Seen, true),
            pending("differing", Flagged, true),
            pending("named", Seen, true),
            pending("named", Flagged, true),
            pending("related", Flagged, true),
        ]
    );
    assert_eq!(
        pending_of(&store, &work),
        [pending("related", Flagged, true)]
    );
    let stored = store.read_folder_sync(&inbox).unwrap().stored;
    let server_flags = |identity: &str| (stored[identity].seen, stored[identity].flagged);
    assert_eq!(
        ["named", "differing", "arrived", "related"].map(server_flags),
        [(true, false), (false, false), (true, false), (false, true)]
    );
}

#[test]
fn a_command_ends_only_a_pending_value_equal_to_its_own() {
    use MessageFlag::{Flagged, Seen};
    let synced = account("synced");
    let inbox = folder_of(&synced, "INBOX");
    let messages = [
        "starred",
        "unstarred-meanwhile",
        "refused",
        "changed-meanwhile",
    ]
    .map(|identity| message_with_flags(identity, false, false));
    let store = store_with(&synced, &[("INBOX", &messages)]);
    let wishes = [
        ("starred", Flagged, true),
        // Starred, sent, and unstarred while the command was out.
        ("unstarred-meanwhile", Flagged, false),
        ("refused", Seen, true),
        ("changed-meanwhile", Seen, false),
    ];
    for (identity, flag, wanted) in wishes {
        store
            .write_pending_flag(&synced, identity, flag, wanted)
            .unwrap();
    }
    let sent = ["starred", "unstarred-meanwhile"].map(str::to_owned);
    store.settle_flags(&synced, &sent, Flagged, true).unwrap();
    let refused = ["refused", "changed-meanwhile"].map(str::to_owned);
    store
        .drop_pending_flags(&synced, &refused, Seen, true)
        .unwrap();
    assert_eq!(
        pending_of(&store, &inbox),
        [
            pending("changed-meanwhile", Seen, false),
            pending("unstarred-meanwhile", Flagged, false),
        ]
    );
    assert_eq!(
        flags_of(&store, &inbox),
        [
            ("changed-meanwhile".to_owned(), false, false),
            ("refused".to_owned(), false, false),
            ("starred".to_owned(), false, true),
            ("unstarred-meanwhile".to_owned(), false, false),
        ]
    );
}

#[test]
fn a_store_opened_again_from_its_file_reads_the_pending_changes() {
    let directory = TestDirectory::new();
    let loaded = account("loaded");
    let inbox = folder_of(&loaded, "INBOX");
    let store = Store::at(directory.store_path());
    store
        .replace_folders(&loaded, &[folder("INBOX")], || false)
        .unwrap();
    store_completed_cycle(&store, &inbox, &[dated_message("message", 1)], || false).unwrap();
    store
        .write_pending_flag(&loaded, "message", MessageFlag::Flagged, true)
        .unwrap();
    drop(store);
    let reopened = Store::at(directory.store_path());
    assert_eq!(
        pending_of(&reopened, &inbox),
        [pending("message", MessageFlag::Flagged, true)]
    );
}
