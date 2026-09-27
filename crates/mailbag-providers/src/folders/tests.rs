// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use FolderRole::*;

fn mailbox(attributes: &[&str], name: &str) -> MailboxName {
    MailboxName {
        name: name.to_owned(),
        attributes: attributes
            .iter()
            .map(|&attribute| attribute.to_owned())
            .collect(),
        delimiter: Some("/".to_owned()),
    }
}

fn list(names: Vec<MailboxName>) -> MailboxList {
    MailboxList {
        names,
        utf8_names: false,
    }
}

/// Identity, shown name, parent, role and whether it opens.
type Summary = (String, String, Option<String>, Option<FolderRole>, bool);

fn summaries(folders: &[Folder]) -> Vec<Summary> {
    folders
        .iter()
        .map(|folder| {
            (
                folder.identity.clone(),
                folder.name.clone(),
                folder.parent.clone(),
                folder.role,
                folder.selectable,
            )
        })
        .collect()
}

fn summary(
    identity: &str,
    name: &str,
    parent: Option<&str>,
    role: Option<FolderRole>,
    selectable: bool,
) -> Summary {
    (
        identity.to_owned(),
        name.to_owned(),
        parent.map(str::to_owned),
        role,
        selectable,
    )
}

#[test]
fn imap_roles_come_from_attributes_and_the_inbox_name_only() {
    let folders = imap_folders(&list(vec![
        mailbox(&["\\HasNoChildren"], "Inbox"),
        // A server that marks only some of its system folders.
        mailbox(&["\\HasNoChildren", "\\Sent"], "Sent Messages"),
        mailbox(&["\\HasNoChildren"], "Drafts"),
        // Two marks: the first the server lists decides.
        mailbox(&["\\junk", "\\Trash"], "Spam"),
        // Two folders with one mark both keep it.
        mailbox(&["\\Trash"], "Deleted"),
        mailbox(&["\\Trash"], "Bin"),
        mailbox(&["\\Inbox"], "Arrivals"),
        mailbox(&["\\Memos"], "Notes"),
    ]));
    let roles: Vec<Option<FolderRole>> = folders.iter().map(|folder| folder.role).collect();
    assert_eq!(
        roles,
        [
            Some(Inbox),
            Some(Sent),
            None,
            Some(Junk),
            Some(Trash),
            Some(Trash),
            Some(Inbox),
            None
        ]
    );
}

#[test]
fn imap_folders_nest_under_listed_parents_and_containers_do_not_open() {
    let folders = imap_folders(&list(vec![
        mailbox(&["\\HasChildren", "\\Noselect"], "Projects"),
        mailbox(&["\\HasChildren"], "Projects/Reports"),
        mailbox(&["\\HasNoChildren"], "Projects/Reports/2026"),
        // Its parent is not listed (RFC 9051 §6.3.9.7).
        mailbox(&["\\HasNoChildren"], "Archive/Old"),
    ]));
    assert_eq!(
        summaries(&folders),
        [
            summary("Projects", "Projects", None, None, false),
            summary("Projects/Reports", "Reports", Some("Projects"), None, true),
            summary(
                "Projects/Reports/2026",
                "2026",
                Some("Projects/Reports"),
                None,
                true
            ),
            summary("Archive/Old", "Archive/Old", None, None, true),
        ]
    );
}

#[test]
fn imap_names_are_decoded_unless_the_server_sends_utf8() {
    // The example of RFC 3501 §5.1.3, nested.
    let names = vec![
        mailbox(&[], "&U,BTFw-"),
        mailbox(&[], "&U,BTFw-/&ZeVnLIqe-"),
        mailbox(&[], "Tom &- Jerry"),
    ];
    let decoded = imap_folders(&list(names.clone()));
    let shown: Vec<&str> = decoded.iter().map(|folder| folder.name.as_str()).collect();
    assert_eq!(shown, ["台北", "日本語", "Tom & Jerry"]);
    // The identity stays as sent: it opens the mailbox.
    assert_eq!(decoded[1].identity, "&U,BTFw-/&ZeVnLIqe-");
    let as_sent = imap_folders(&MailboxList {
        names,
        utf8_names: true,
    });
    assert_eq!(as_sent[2].name, "Tom &- Jerry");
}

#[test]
fn gmails_system_label_container_is_left_out_and_its_labels_move_up() {
    let folders = gmail_folders(&list(vec![
        mailbox(&["\\HasNoChildren"], "INBOX"),
        mailbox(&["\\HasChildren", "\\Noselect"], "[Gmail]"),
        mailbox(&["\\All", "\\HasNoChildren"], "[Gmail]/All Mail"),
        mailbox(&["\\HasNoChildren", "\\Sent"], "[Gmail]/Sent Mail"),
        mailbox(&["\\HasChildren"], "[Gmail]/Sent Mail/Old"),
        // A container of the user's own labels stays.
        mailbox(&["\\HasChildren", "\\Noselect"], "Travel"),
        mailbox(&["\\HasNoChildren"], "Travel/2026"),
    ]));
    assert_eq!(
        summaries(&folders),
        [
            summary("INBOX", "INBOX", None, Some(Inbox), true),
            summary("[Gmail]/All Mail", "All Mail", None, Some(AllMail), true),
            summary("[Gmail]/Sent Mail", "Sent Mail", None, Some(Sent), true),
            summary(
                "[Gmail]/Sent Mail/Old",
                "Old",
                Some("[Gmail]/Sent Mail"),
                None,
                true
            ),
            summary("Travel", "Travel", None, None, false),
            summary("Travel/2026", "2026", Some("Travel"), None, true),
        ]
    );
}

#[test]
fn microsoft_365_roles_come_from_well_known_names_and_the_root_is_left_out() {
    let graph_folder = |id: &str, parent: &str, well_known| GraphFolder {
        id: id.to_owned(),
        name: format!("Name of {id}"),
        parent_id: Some(parent.to_owned()),
        well_known,
    };
    // The mailbox has no archive, so no folder has that role.
    let folders = graph_folders(vec![
        graph_folder("inbox-id", "root-id", Some(WellKnownFolder::Inbox)),
        graph_folder("deleted-id", "root-id", Some(WellKnownFolder::DeletedItems)),
        graph_folder("projects-id", "root-id", None),
        graph_folder("reports-id", "projects-id", None),
    ]);
    assert_eq!(
        summaries(&folders),
        [
            summary("inbox-id", "Name of inbox-id", None, Some(Inbox), true),
            summary("deleted-id", "Name of deleted-id", None, Some(Trash), true),
            summary("projects-id", "Name of projects-id", None, None, true),
            summary(
                "reports-id",
                "Name of reports-id",
                Some("projects-id"),
                None,
                true
            ),
        ]
    );
}
