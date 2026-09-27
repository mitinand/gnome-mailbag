// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::FolderRole::{self, *};

#[test]
fn every_role_reads_back_from_its_stored_code_and_no_other_text_is_a_role() {
    for role in FolderRole::ORDER {
        assert_eq!(FolderRole::from_code(role.as_code()), Some(role));
    }
    assert_eq!(FolderRole::from_code("Inbox"), None);
    assert_eq!(FolderRole::from_code(""), None);
}

#[test]
fn starred_important_and_all_mail_are_the_views() {
    let views: Vec<FolderRole> = FolderRole::ORDER
        .into_iter()
        .filter(|role| role.is_view())
        .collect();
    assert_eq!(views, [Starred, Important, AllMail]);
}
