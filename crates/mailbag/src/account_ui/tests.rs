// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use goa_adapter::{AccountCheckError, AccountCheckResult, AccountDetails, AccountProvider};

#[test]
#[ignore = "requires a graphical GTK session"]
fn account_ui_transitions() {
    adw::init().expect("GTK display");
    let builder = gtk::Builder::from_string(include_str!("../../resources/ui/mailbag.ui"));
    let window: adw::Window = builder.object("window").unwrap();
    let retries = Rc::new(Cell::new(0));
    let calls = retries.clone();
    let ui = AccountUi::new(&builder, move || calls.set(calls.get() + 1));
    window.present();
    dispatch_pending();
    assert_eq!(ui.borrow().status.title(), "Checking accounts");
    let id = AccountId::try_from("synthetic-one").unwrap();
    let mut update = AccountUpdate {
        last_check: AccountCheckResult::Complete,
        ..Default::default()
    };
    update.accounts.insert(
        id.clone(),
        AccountDetails {
            provider: Some(AccountProvider::ImapSmtp),
            mail_enabled: Some(true),
            needs_attention: Some(false),
            mail_service_available: true,
            display_name: Some("<Synthetic>".into()),
            provider_name: None,
            email_address: None,
            icon_name: None,
        },
    );
    ui.borrow_mut().apply_update(&update);
    dispatch_pending();
    assert_eq!(ui.borrow().rows[&id].details.title(), "<Synthetic>");
    assert!(!ui.borrow().rows[&id].details.uses_markup());
    assert_eq!(ui.borrow().selection.selected(), gtk::INVALID_LIST_POSITION);
    let original = ui.borrow().rows[&id].root.clone();
    let selection = ui.borrow().selection.clone();
    selection.set_selected(0);
    assert_eq!(ui.borrow().accounts.selected_id(), Some(&id));
    assert_eq!(ui.borrow().status.title(), "");
    assert!(!ui.borrow().status.is_visible());
    update.accounts.get_mut(&id).unwrap().needs_attention = Some(true);
    ui.borrow_mut().apply_update(&update);
    assert!(!ui.borrow().rows[&id].retry.property::<bool>("visible"));
    assert!(ui.borrow().rows[&id].settings.property::<bool>("visible"));
    update.accounts.get_mut(&id).unwrap().needs_attention = Some(false);
    update.accounts.get_mut(&id).unwrap().display_name = Some("Renamed".into());
    ui.borrow_mut().apply_update(&update);
    assert_eq!(ui.borrow().rows[&id].root, original);
    update.last_check =
        AccountCheckResult::Failed(AccountCheckError::new("check", ErrorCause::Timeout));
    ui.borrow_mut().apply_update(&update);
    dispatch_pending();
    assert!(ui.borrow().rows[&id].problem.is_visible());
    assert!(
        !ui.borrow().rows[&id]
            .explanation
            .text()
            .contains("did not respond in time")
    );
    let problem = ui.borrow().rows[&id].problem.clone();
    assert!(problem.grab_focus());
    problem.popup();
    dispatch_pending();
    assert!(ui.borrow().rows[&id].popover.is_visible());
    assert_eq!(
        problem.tooltip_text(),
        Some(ui.borrow().rows[&id].explanation.text())
    );
    ui.borrow().rows[&id].retry.emit_clicked();
    assert_eq!(retries.get(), 1);
    assert_eq!(ui.borrow().accounts.selected_id(), Some(&id));
    assert!(
        ui.borrow()
            .status
            .description()
            .unwrap()
            .contains("did not respond in time")
    );
    ui.borrow().retry.emit_clicked();
    assert_eq!(retries.get(), 2);
    update.check_pending = true;
    ui.borrow_mut().apply_update(&update);
    assert!(!ui.borrow().retry.is_sensitive());
    assert!(
        ui.borrow()
            .status
            .description()
            .unwrap()
            .contains("did not respond in time")
    );
    assert_eq!(ui.borrow().accounts.selected_id(), Some(&id));
    update.check_pending = false;
    update.last_check = AccountCheckResult::Complete;
    ui.borrow_mut().apply_update(&update);
    assert!(!ui.borrow().rows[&id].problem.is_visible());
    assert!(!ui.borrow().rows[&id].popover.is_visible());
    assert!(contains_focus(&ui.borrow().rows[&id].root));
    update.accounts.clear();
    ui.borrow_mut().apply_update(&update);
    assert_eq!(ui.borrow().selection.selected(), gtk::INVALID_LIST_POSITION);
    assert_eq!(ui.borrow().status.title(), "No mail accounts");
    assert!(ui.borrow().online_accounts.is_visible());
    assert!(ui.borrow().notices.borrow().active.is_some());
    let notices = ui.borrow().notices.clone();
    AccountNotices::push(&notices, AccountHiddenNotice::Single("Second".into()));
    AccountNotices::push(&notices, AccountHiddenNotice::Single("Third".into()));
    assert!(matches!(
        notices.borrow().pending,
        Some(AccountHiddenNotice::Group(2))
    ));
    let active = notices.borrow().active.clone().unwrap();
    active.dismiss();
    assert!(
        notices
            .borrow()
            .active
            .as_ref()
            .unwrap()
            .title()
            .unwrap()
            .starts_with("2 accounts")
    );
    assert!(notices.borrow().pending.is_none());

    update.last_check =
        AccountCheckResult::Failed(AccountCheckError::new("check", ErrorCause::AccessDenied));
    ui.borrow_mut().apply_update(&update);
    assert_eq!(ui.borrow().status.title(), "Unable to get accounts");
    assert!(ui.borrow().retry.is_visible());
    assert!(!ui.borrow().online_accounts.is_visible());
    update.last_check = AccountCheckResult::Complete;
    let details = AccountDetails {
        provider: Some(AccountProvider::Google),
        mail_enabled: Some(false),
        needs_attention: Some(false),
        mail_service_available: true,
        display_name: Some("Returned".into()),
        provider_name: None,
        email_address: None,
        icon_name: None,
    };
    update.accounts.insert(id.clone(), details);
    ui.borrow_mut().apply_update(&update);
    assert_eq!(ui.borrow().status.title(), "No mail accounts");
    assert!(
        ui.borrow()
            .status
            .description()
            .unwrap()
            .contains("Enable Mail")
    );
    update.accounts.get_mut(&id).unwrap().provider = Some(AccountProvider::Other);
    ui.borrow_mut().apply_update(&update);
    assert!(ui.borrow().rows.is_empty());
    assert_eq!(ui.borrow().status.title(), "No mail accounts");
    assert!(
        !ui.borrow()
            .status
            .description()
            .unwrap()
            .contains("supported")
    );
    update.accounts.get_mut(&id).unwrap().provider = Some(AccountProvider::Google);
    update.accounts.get_mut(&id).unwrap().mail_enabled = Some(true);
    ui.borrow_mut().apply_update(&update);
    assert_eq!(ui.borrow().status.title(), "Select an account");
    assert_eq!(selection.selected(), gtk::INVALID_LIST_POSITION);
    let second_id = AccountId::try_from("synthetic-two").unwrap();
    update
        .accounts
        .insert(second_id.clone(), update.accounts[&id].clone());
    ui.borrow_mut().apply_update(&update);
    assert_ne!(
        ui.borrow().rows[&id].details.title(),
        ui.borrow().rows[&second_id].details.title()
    );
    selection.set_selected(0);
    dispatch_pending();
    assert!(ui.borrow().rows[&id].root.grab_focus());
    update.accounts.remove(&id);
    ui.borrow_mut().apply_update(&update);
    assert_eq!(selection.selected(), gtk::INVALID_LIST_POSITION);
    assert!(contains_focus(&ui.borrow().tree));
    ui.borrow().mail_split.set_collapsed(true);
    ui.borrow().folders_split.set_collapsed(true);
    let tree = ui.borrow().tree.clone();
    tree.emit_by_name::<()>("activate", &[&0_u32]);
    assert!(!ui.borrow().mail_split.shows_content());
    assert!(!ui.borrow().folders_split.shows_sidebar());
    assert_eq!(ui.borrow().accounts.selected_id(), Some(&second_id));
    let actions = gio::SimpleActionGroup::new();
    let action = gio::SimpleAction::new("accounts", None);
    let launches = Rc::new(Cell::new(0));
    let count = launches.clone();
    action.connect_activate(move |_, _| count.set(count.get() + 1));
    actions.add_action(&action);
    window.insert_action_group("app", Some(&actions));
    ui.borrow_mut()
        .show_settings_result(false, Some(crate::settings::LaunchError::Timeout));
    let settings_button = ui.borrow().online_accounts.clone();
    settings_button.emit_clicked();
    action.activate(None);
    assert_eq!(launches.get(), 2);
    ui.borrow_mut().apply_update(&update);
    assert!(
        ui.borrow()
            .status
            .description()
            .unwrap()
            .contains("Settings did not respond")
    );
    AccountNotices::push(&notices, AccountHiddenNotice::Group(3));
    let active = notices.borrow().active.clone().unwrap();
    active.dismiss();
    assert!(
        ui.borrow()
            .status
            .description()
            .unwrap()
            .contains("Settings did not respond")
    );
    ui.borrow_mut().show_settings_result(true, None);
    assert!(!settings_button.is_sensitive());
    ui.borrow_mut().show_settings_result(false, None);
    assert!(!ui.borrow().status.is_visible());
    window.destroy();
}

fn dispatch_pending() {
    let context = glib::MainContext::default();
    for _ in 0..100 {
        if !context.pending() {
            break;
        }
        context.iteration(false);
    }
}
