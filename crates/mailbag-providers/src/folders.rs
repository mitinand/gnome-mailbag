// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A server's folder list as the application's folders: names for the user,
//! nesting and roles from what the server states, never from a folder's name
//! (specs/008-folders FR-003, FR-005, FR-006). One function per provider.

#[cfg(test)]
mod tests;

use mailbag_domain::{Folder, FolderRole};
use mailbag_graph::{GraphFolder, WellKnownFolder};
use mailbag_imap::{MailboxList, MailboxName, utf7};
use std::collections::BTreeSet;

/// The mailbox attributes that give a role (RFC 6154, RFC 8457), and
/// `\Inbox`, which some servers list for the Inbox.
const ROLE_ATTRIBUTES: [(&str, FolderRole); 9] = [
    ("\\Inbox", FolderRole::Inbox),
    ("\\Flagged", FolderRole::Starred),
    ("\\Important", FolderRole::Important),
    ("\\Junk", FolderRole::Junk),
    ("\\Trash", FolderRole::Trash),
    ("\\Archive", FolderRole::Archive),
    ("\\Drafts", FolderRole::Drafts),
    ("\\Sent", FolderRole::Sent),
    ("\\All", FolderRole::AllMail),
];

/// A Generic IMAP account's folders from its LIST reply.
pub(crate) fn imap_folders(list: &MailboxList) -> Vec<Folder> {
    let listed: BTreeSet<&str> = list
        .names
        .iter()
        .map(|mailbox| mailbox.name.as_str())
        .collect();
    list.names
        .iter()
        .map(|mailbox| {
            let parent = listed_parent(mailbox, &listed);
            // Under a listed parent the name is the part after it; a folder
            // shown directly under the account keeps its whole name.
            let shown = match (parent, &mailbox.delimiter) {
                (Some(parent), Some(delimiter)) => &mailbox.name[parent.len() + delimiter.len()..],
                _ => mailbox.name.as_str(),
            };
            Folder {
                identity: mailbox.name.clone(),
                name: match list.utf8_names {
                    true => shown.to_owned(),
                    false => utf7::decode(shown),
                },
                parent: parent.map(str::to_owned),
                role: imap_role(mailbox),
                selectable: !has_attribute(mailbox, "\\Noselect"),
            }
        })
        .collect()
}

/// A Gmail account's folders: as for Generic IMAP, without the container
/// Gmail lists its system labels under. The container is recognized by its
/// children's roles, since its name is localized; its children then sit
/// directly under the account.
pub(crate) fn gmail_folders(list: &MailboxList) -> Vec<Folder> {
    let mut folders = imap_folders(list);
    let containers: Vec<String> = folders
        .iter()
        .filter(|folder| folder.parent.is_none() && !folder.selectable)
        .filter(|container| {
            folders.iter().any(|folder| {
                folder.parent.as_ref() == Some(&container.identity) && folder.role.is_some()
            })
        })
        .map(|container| container.identity.clone())
        .collect();
    folders.retain(|folder| !containers.contains(&folder.identity));
    for folder in &mut folders {
        if folder
            .parent
            .as_ref()
            .is_some_and(|parent| containers.contains(parent))
        {
            folder.parent = None;
        }
    }
    folders
}

/// A Microsoft 365 account's folders. The well-known name that resolved to a
/// folder gives its role; a parent the listing leaves out, such as the
/// mailbox's root, puts the folder under the account.
pub(crate) fn graph_folders(listed: Vec<GraphFolder>) -> Vec<Folder> {
    let identities: BTreeSet<String> = listed.iter().map(|folder| folder.id.clone()).collect();
    listed
        .into_iter()
        .map(|folder| Folder {
            identity: folder.id,
            name: folder.name,
            parent: folder
                .parent_id
                .filter(|parent| identities.contains(parent)),
            role: folder.well_known.map(graph_role),
            selectable: true,
        })
        .collect()
}

/// The mailbox's parent by its hierarchy delimiter, when the server listed it.
fn listed_parent<'a>(mailbox: &MailboxName, listed: &BTreeSet<&'a str>) -> Option<&'a str> {
    let delimiter = mailbox.delimiter.as_deref()?;
    let (parent, _) = mailbox.name.rsplit_once(delimiter)?;
    listed.get(parent).copied()
}

/// The Inbox by its reserved name, in any case (RFC 9051 §5.1), otherwise
/// the first role attribute in the order the server listed them.
fn imap_role(mailbox: &MailboxName) -> Option<FolderRole> {
    if mailbox.name.eq_ignore_ascii_case("INBOX") {
        return Some(FolderRole::Inbox);
    }
    mailbox.attributes.iter().find_map(|attribute| {
        ROLE_ATTRIBUTES
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(attribute))
            .map(|(_, role)| *role)
    })
}

/// Attributes are atoms, compared without regard to case (RFC 3501 §9).
fn has_attribute(mailbox: &MailboxName, name: &str) -> bool {
    mailbox
        .attributes
        .iter()
        .any(|attribute| attribute.eq_ignore_ascii_case(name))
}

fn graph_role(well_known: WellKnownFolder) -> FolderRole {
    match well_known {
        WellKnownFolder::Inbox => FolderRole::Inbox,
        WellKnownFolder::Drafts => FolderRole::Drafts,
        WellKnownFolder::SentItems => FolderRole::Sent,
        WellKnownFolder::DeletedItems => FolderRole::Trash,
        WellKnownFolder::JunkEmail => FolderRole::Junk,
        WellKnownFolder::Archive => FolderRole::Archive,
    }
}
