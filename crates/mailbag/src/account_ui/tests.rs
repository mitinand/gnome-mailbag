// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use goa_adapter::{AccountCheckError, AccountCheckResult, AccountDetails, AccountProvider};

#[test]
#[ignore = "requires a graphical GTK session"]
fn account_ui_transitions() {
    adw::init().expect("GTK display");
    crate::register_resources();
    let builder = gtk::Builder::from_string(include_str!("../../resources/ui/mailbag.ui"));
    let window: adw::Window = builder.object("window").unwrap();
    let ui = AccountUi::new(&builder);
    window.present();
    dispatch_pending();
    let id = AccountId::try_from("synthetic-one").unwrap();
    let mut update = AccountUpdate {
        last_check: AccountCheckResult::Complete,
        ..Default::default()
    };
    update.accounts.insert(
        id.clone(),
        AccountDetails {
            provider: AccountProvider::ImapSmtp,
            mail_enabled: true,
            needs_attention: false,
            mail_service_available: true,
            display_name: Some("<Synthetic>".into()),
            email_address: Some("synthetic@example.invalid".into()),
        },
    );
    ui.borrow_mut().apply_update(&update);
    dispatch_pending();
    let first_item = ui.borrow().rows[&id].clone();
    let first_row = first_item.borrow::<AccountWidgets>();
    assert_eq!(first_row.details.title(), "<Synthetic>");
    hover_row(&first_row.root);
    assert_eq!(ui.borrow().selection.selected(), gtk::INVALID_LIST_POSITION);
    assert!(ui.borrow().accounts.selected_id().is_none());
    let original = first_row.root.clone();
    let selection = ui.borrow().selection.clone();
    let tree = ui.borrow().tree.clone();
    tree.emit_by_name::<()>("activate", &[&0_u32]);
    assert_eq!(ui.borrow().accounts.selected_id(), Some(&id));
    update.accounts.get_mut(&id).unwrap().display_name = Some("<Renamed>".into());
    ui.borrow_mut().apply_update(&update);
    assert_eq!(
        ui.borrow().rows[&id].borrow::<AccountWidgets>().root,
        original
    );
    update.last_check =
        AccountCheckResult::Failed(AccountCheckError::new("check", ErrorCause::Timeout));
    ui.borrow_mut().apply_update(&update);
    dispatch_pending();
    assert_eq!(ui.borrow().rows[&id], first_item);
    assert_eq!(first_row.root, original);
    update.last_check = AccountCheckResult::Complete;
    ui.borrow_mut().apply_update(&update);
    assert_eq!(ui.borrow().rows[&id], first_item);
    let second_id = AccountId::try_from("synthetic-earlier").unwrap();
    update
        .accounts
        .insert(second_id.clone(), update.accounts[&id].clone());
    ui.borrow_mut().apply_update(&update);
    assert_eq!(
        ui.borrow().store.item(0).unwrap(),
        ui.borrow().rows[&second_id]
    );
    assert_eq!(ui.borrow().store.item(1).unwrap(), first_item);
    assert_eq!(selection.selected(), 1);
    for mail_enabled in [false, true] {
        update.accounts.get_mut(&second_id).unwrap().mail_enabled = mail_enabled;
        ui.borrow_mut().apply_update(&update);
        assert_eq!(selection.selected(), u32::from(mail_enabled));
        assert_eq!(ui.borrow().accounts.selected_id(), Some(&id));
        assert_eq!(
            ui.borrow().store.item(u32::from(mail_enabled)).unwrap(),
            first_item
        );
    }
    assert_eq!(
        ui.borrow().store.item(0).unwrap(),
        ui.borrow().rows[&second_id]
    );
    let list_item = ui.borrow().rows[&id]
        .borrow::<AccountWidgets>()
        .root
        .parent()
        .unwrap();
    assert!(list_item.activate());
    let second_row = ui.borrow().rows[&second_id]
        .borrow::<AccountWidgets>()
        .root
        .clone();
    hover_row(&second_row);
    assert_eq!(selection.selected(), 1);
    assert_eq!(ui.borrow().accounts.selected_id(), Some(&id));
    dispatch_pending();
    assert!(
        ui.borrow().rows[&id]
            .borrow::<AccountWidgets>()
            .root
            .grab_focus()
    );
    update.accounts.remove(&id);
    ui.borrow_mut().apply_update(&update);
    assert_eq!(selection.selected(), gtk::INVALID_LIST_POSITION);
    assert!(contains_focus(&ui.borrow().tree));
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

fn hover_row(row: &impl IsA<gtk::Widget>) {
    let motion = row
        .parent()
        .unwrap()
        .observe_controllers()
        .iter::<glib::Object>()
        .find_map(|controller| {
            controller
                .unwrap()
                .downcast::<gtk::EventControllerMotion>()
                .ok()
        })
        .expect("account row pointer controller");
    motion.emit_by_name::<()>("enter", &[&1_f64, &1_f64]);
}
