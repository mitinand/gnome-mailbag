// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::FolderRole::{self, *};

#[test]
fn starred_important_and_all_mail_are_the_views() {
    let views: Vec<FolderRole> = FolderRole::ORDER
        .into_iter()
        .filter(|role| role.is_view())
        .collect();
    assert_eq!(views, [Starred, Important, AllMail]);
}
