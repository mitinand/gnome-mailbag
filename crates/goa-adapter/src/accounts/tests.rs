// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::test_goa::{make_account, make_account_reply};
use gio::prelude::*;

#[test]
fn required_fields_optional_strings_and_non_account_objects() {
    for property in ["Id", "ProviderType", "MailDisabled", "AttentionNeeded"] {
        let mut account = make_account("one");
        account.get_mut(ACCOUNT_INTERFACE).unwrap().remove(property);
        assert!(parse_accounts(&make_account_reply(vec![account.clone()])).is_err());
        account
            .get_mut(ACCOUNT_INTERFACE)
            .unwrap()
            .insert(property.into(), 42u32.to_variant());
        assert!(parse_accounts(&make_account_reply(vec![account])).is_err());
    }

    for absent in [true, false] {
        let mut account = make_account("one");
        for (interface, property) in [
            (ACCOUNT_INTERFACE, "PresentationIdentity"),
            (MAIL_INTERFACE, "EmailAddress"),
        ] {
            let properties = account.get_mut(interface).unwrap();
            if absent {
                properties.remove(property);
            } else {
                properties.insert(property.into(), "".to_variant());
            }
        }
        let accounts = parse_accounts(&make_account_reply(vec![account])).unwrap();
        let details = accounts.values().next().unwrap();
        assert!(details.display_name.is_none());
        assert!(details.email_address.is_none());
    }
    let mut invalid = make_account("one");
    invalid
        .get_mut(MAIL_INTERFACE)
        .unwrap()
        .insert("EmailAddress".into(), false.to_variant());
    assert!(parse_accounts(&make_account_reply(vec![invalid])).is_err());

    let unrelated = BTreeMap::from([("org.gnome.OnlineAccounts.Manager".into(), BTreeMap::new())]);
    assert!(
        parse_accounts(&make_account_reply(vec![unrelated]))
            .unwrap()
            .is_empty()
    );
    assert!(
        parse_accounts(&make_account_reply(vec![]))
            .unwrap()
            .is_empty()
    );
}
