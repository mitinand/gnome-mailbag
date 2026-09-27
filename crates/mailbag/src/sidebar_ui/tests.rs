// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use goa_adapter::{AccountCheckError, AccountCheckResult, AccountDetails, AccountProvider};

#[test]
#[ignore = "requires a graphical GTK session"]
fn sidebar_transitions() {
    adw::init().expect("GTK display");
    crate::register_resources();
    let builder = gtk::Builder::from_string(include_str!("../../resources/ui/mailbag.ui"));
    let window: adw::Window = builder.object("window").unwrap();
    let ui = SidebarUi::new(&builder);
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
    let first_item = ui.borrow().account_nodes[&id].clone();
    let first_row = first_item.borrow::<SidebarNode>();
    assert_eq!(first_row.widgets.details.title(), "<Synthetic>");
    hover_row(&first_row.widgets.root);
    let selection = ui.borrow().selection_model.clone();
    assert_eq!(selection.selected(), gtk::INVALID_LIST_POSITION);
    assert!(ui.borrow().accounts.selection().is_none());
    let original = first_row.widgets.root.clone();
    let tree = ui.borrow().tree.clone();
    // An account without a folder list is a row that can be selected.
    tree.emit_by_name::<()>("activate", &[&0_u32]);
    assert_eq!(ui.borrow().accounts.selected_account(), Some(&id));
    update.accounts.get_mut(&id).unwrap().display_name = Some("<Renamed>".into());
    ui.borrow_mut().apply_update(&update);
    assert_eq!(
        ui.borrow().account_nodes[&id]
            .borrow::<SidebarNode>()
            .widgets
            .root,
        original
    );
    update.last_check =
        AccountCheckResult::Failed(AccountCheckError::new("check", ErrorCause::Timeout));
    ui.borrow_mut().apply_update(&update);
    dispatch_pending();
    assert_eq!(ui.borrow().account_nodes[&id], first_item);
    assert_eq!(first_row.widgets.root, original);
    update.last_check = AccountCheckResult::Complete;
    ui.borrow_mut().apply_update(&update);
    assert_eq!(ui.borrow().account_nodes[&id], first_item);
    // Space sets apart every account but the first.
    assert!(!first_row.widgets.account_spacing.is_visible());
    let second_id = AccountId::try_from("synthetic-earlier").unwrap();
    update
        .accounts
        .insert(second_id.clone(), update.accounts[&id].clone());
    ui.borrow_mut().apply_update(&update);
    assert_eq!(
        ui.borrow().root.item(0).unwrap(),
        ui.borrow().account_nodes[&second_id]
    );
    assert_eq!(ui.borrow().root.item(1).unwrap(), first_item);
    assert_eq!(selection.selected(), 1);
    assert!(
        !ui.borrow().account_nodes[&second_id]
            .borrow::<SidebarNode>()
            .widgets
            .account_spacing
            .is_visible()
    );
    for mail_enabled in [false, true] {
        update.accounts.get_mut(&second_id).unwrap().mail_enabled = mail_enabled;
        ui.borrow_mut().apply_update(&update);
        assert_eq!(selection.selected(), u32::from(mail_enabled));
        assert_eq!(ui.borrow().accounts.selected_account(), Some(&id));
        assert_eq!(
            ui.borrow().root.item(u32::from(mail_enabled)).unwrap(),
            first_item
        );
        assert_eq!(first_row.widgets.account_spacing.is_visible(), mail_enabled);
    }
    let list_item = first_row.widgets.root.parent().unwrap();
    assert!(list_item.activate());
    let second_row = ui.borrow().account_nodes[&second_id]
        .borrow::<SidebarNode>()
        .widgets
        .root
        .clone();
    hover_row(&second_row);
    assert_eq!(selection.selected(), 1);
    assert_eq!(ui.borrow().accounts.selected_account(), Some(&id));
    drop(first_row);

    // The account gets its folders: it becomes a heading over them, the
    // system folders first in their order with their icons, then the others
    // by the locale's collation, nested under their parents.
    let changes = Rc::new(Cell::new(0));
    let counted = changes.clone();
    ui.borrow()
        .connect_selection_changed(move || counted.set(counted.get() + 1));
    let folders = || {
        vec![
            folder("Zeta", None, None, true),
            folder("Projects", None, None, false),
            folder("Projects/Reports", Some("Projects"), None, true),
            folder("beta", None, None, true),
            folder("Ωμέγα", None, None, true),
            folder("Sent", None, Some(FolderRole::Sent), true),
            folder("Alpha", None, None, true),
            folder("Bin", None, Some(FolderRole::Trash), true),
            // The reserved name, as the server may case it (RFC 9051 §5.1).
            folder("inbox", None, Some(FolderRole::Inbox), true),
        ]
    };
    // The selected account's folders appeared, so it is no longer selected.
    assert!(ui.borrow_mut().show_folders(&id, folders()));
    assert!(ui.borrow().accounts.selection().is_none());
    let heading = ui.borrow().label_of(&id).unwrap();
    let other_account = ui.borrow().label_of(&second_id).unwrap();
    // User folders follow the locale's collation, not the code points,
    // which would put "beta" after "Zeta".
    let mut user_folders = vec!["Alpha", "Projects", "Zeta", "beta", "Ωμέγα"];
    user_folders.sort_by_key(|name| glib::CollationKey::from(name));
    let mut expected = vec![other_account.as_str(), &heading, "Inbox", "Bin", "Sent"];
    for name in user_folders {
        expected.push(name);
        if name == "Projects" {
            expected.push("Reports");
        }
    }
    assert_eq!(shown_rows(&ui.borrow()), expected);
    let icon = |title| row_detail(&ui.borrow(), title, |node| node.widgets.icon.icon_name());
    assert_eq!(
        icon("Inbox").as_deref(),
        Some("mailbag-folder-inbox-symbolic")
    );
    assert_eq!(
        icon("Sent").as_deref(),
        Some("mailbag-folder-sent-symbolic")
    );
    assert_eq!(icon("Reports").as_deref(), Some("folder-symbolic"));

    // A heading and a container do not react to the pointer and change
    // nothing when activated; a click on a folder's name selects it. The
    // tree activates no row on a click of its own, so the expander's arrow
    // only expands.
    assert!(!tree.is_single_click_activate());
    let reacts = |title: &str| {
        row_detail(&ui.borrow(), title, |node| {
            node.bound_item
                .upgrade()
                .expect("a shown row")
                .is_activatable()
        })
    };
    assert!(!reacts(&heading));
    assert!(!reacts("Projects"));
    assert!(reacts("Reports"));
    for title in [heading.as_str(), "Projects"] {
        activate(&tree, &ui, title);
        assert!(ui.borrow().accounts.selection().is_none(), "{title}");
    }
    click_name(&ui, "Reports");
    assert_eq!(
        ui.borrow().accounts.selection(),
        Some(&Selection::Mailbox(FolderRef {
            account: id.clone(),
            identity: "Projects/Reports".to_owned(),
        }))
    );

    // Collapsing the shown folder's parent clears the selection.
    let changes_before = changes.get();
    let projects = tree_row(&ui.borrow(), "Projects");
    projects.set_expanded(false);
    dispatch_pending();
    assert!(ui.borrow().accounts.selection().is_none());
    assert_eq!(selection.selected(), gtk::INVALID_LIST_POSITION);
    assert_eq!(changes.get(), changes_before + 1);
    // The same list again keeps the rows: the collapsed folder stays so.
    assert!(!ui.borrow_mut().show_folders(&id, folders()));
    assert!(!projects.is_expanded());
    projects.set_expanded(true);

    // A changed list rebuilds the subtree: the shown folder's row is marked
    // again and the keyboard stays in the tree; a folder gone from the list
    // is no longer selected.
    activate(&tree, &ui, "Zeta");
    let zeta_row = row_detail(&ui.borrow(), "Zeta", |node| node.widgets.root.clone());
    assert!(zeta_row.grab_focus());
    let mut with_gamma = folders();
    with_gamma.push(folder("Gamma", None, None, true));
    assert!(!ui.borrow_mut().show_folders(&id, with_gamma));
    // Checked before the window's own focus move after the next frame,
    // which lands in the tree only sometimes.
    assert!(contains_focus(&ui.borrow().tree));
    dispatch_pending();
    let zeta = position_of_title(&ui.borrow(), "Zeta");
    assert_eq!(selection.selected(), zeta);
    assert!(contains_focus(&ui.borrow().tree));
    let without_zeta: Vec<Folder> = folders()
        .into_iter()
        .filter(|folder| folder.identity != "Zeta")
        .collect();
    assert!(ui.borrow_mut().show_folders(&id, without_zeta));
    assert!(ui.borrow().accounts.selection().is_none());

    // An account whose server lists only containers stays selectable.
    ui.borrow_mut()
        .show_folders(&second_id, vec![folder("Shared", None, None, false)]);
    assert!(reacts(&other_account));
    activate(&tree, &ui, &other_account);
    assert_eq!(
        ui.borrow().accounts.selection(),
        Some(&Selection::Account(second_id.clone()))
    );

    dispatch_pending();
    assert!(
        ui.borrow().account_nodes[&id]
            .borrow::<SidebarNode>()
            .widgets
            .root
            .grab_focus()
    );
    update.accounts.remove(&id);
    ui.borrow_mut().apply_update(&update);
    assert!(contains_focus(&ui.borrow().tree));
    window.destroy();
}

fn folder(
    identity: &str,
    parent: Option<&str>,
    role: Option<FolderRole>,
    selectable: bool,
) -> Folder {
    Folder {
        identity: identity.to_owned(),
        name: identity.rsplit('/').next().unwrap().to_owned(),
        parent: parent.map(str::to_owned),
        role,
        selectable,
    }
}

/// The titles of the rows the tree shows, in order.
fn shown_rows(ui: &SidebarUi) -> Vec<String> {
    (0..ui.tree_model.n_items())
        .map(|position| {
            let node = ui.node_at(position).unwrap();
            node.borrow::<SidebarNode>()
                .widgets
                .details
                .title()
                .to_string()
        })
        .collect()
}

fn position_of_title(ui: &SidebarUi, title: &str) -> u32 {
    shown_rows(ui)
        .iter()
        .position(|shown| shown == title)
        .unwrap_or_else(|| panic!("{title} is not shown")) as u32
}

fn tree_row(ui: &SidebarUi, title: &str) -> gtk::TreeListRow {
    ui.tree_model
        .item(position_of_title(ui, title))
        .and_downcast()
        .unwrap()
}

/// Reads a detail of the row titled `title`.
fn row_detail<T>(ui: &SidebarUi, title: &str, read: impl Fn(&SidebarNode) -> T) -> T {
    let node = ui.node_at(position_of_title(ui, title)).unwrap();
    let node = node.borrow::<SidebarNode>();
    read(&node)
}

/// Activates the row titled `title`, as a click does; the sidebar is not
/// borrowed meanwhile, since activation changes it.
fn activate(tree: &gtk::ListView, ui: &Rc<RefCell<SidebarUi>>, title: &str) {
    let position = position_of_title(&ui.borrow(), title);
    tree.emit_by_name::<()>("activate", &[&position]);
}

/// Emits the click gesture on the name of the row titled `title`.
fn click_name(ui: &Rc<RefCell<SidebarUi>>, title: &str) {
    let name = row_detail(&ui.borrow(), title, |node| node.widgets.details.clone());
    let click = name
        .observe_controllers()
        .iter::<glib::Object>()
        .find_map(|controller| controller.unwrap().downcast::<gtk::GestureClick>().ok())
        .expect("the name's click gesture");
    click.emit_by_name::<()>("released", &[&1_i32, &1_f64, &1_f64]);
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
