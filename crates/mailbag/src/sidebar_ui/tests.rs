// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use goa_adapter::{AccountCheckError, AccountCheckResult, AccountDetails, AccountProvider};
use std::cell::Cell;

#[test]
#[ignore = "requires a graphical GTK session"]
fn sidebar_transitions() {
    adw::init().expect("GTK display");
    // The test checks states, not transitions: a split view still animating
    // leaves its rows hidden, and the window then moves a focus it would
    // otherwise keep.
    gtk::Settings::default()
        .expect("GTK settings")
        .set_gtk_enable_animations(false);
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
    assert_eq!(first_row.widgets.row.title(), "<Synthetic>");
    let tree = ui.borrow().tree.clone();
    assert_eq!(selected_position(&tree), None);
    assert!(ui.borrow().accounts.selection().is_none());
    let original = first_row.widgets.row.clone();
    // An account without a folder list is a row that can be selected.
    assert!(first_row.widgets.row.is_activatable());
    select_position(&tree, 0);
    assert_eq!(ui.borrow().accounts.selected_account(), Some(&id));
    update.accounts.get_mut(&id).unwrap().display_name = Some("<Renamed>".into());
    ui.borrow_mut().apply_update(&update);
    assert_eq!(
        ui.borrow().account_nodes[&id]
            .borrow::<SidebarNode>()
            .widgets
            .row,
        original
    );
    update.last_check =
        AccountCheckResult::Failed(AccountCheckError::new("check", ErrorCause::Timeout));
    ui.borrow_mut().apply_update(&update);
    dispatch_pending();
    assert_eq!(ui.borrow().account_nodes[&id], first_item);
    assert_eq!(first_row.widgets.row, original);
    update.last_check = AccountCheckResult::Complete;
    ui.borrow_mut().apply_update(&update);
    assert_eq!(ui.borrow().account_nodes[&id], first_item);
    // Space above the row sets apart every account but the first.
    assert!(!spaced(&first_row));
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
    assert_eq!(selected_position(&tree), Some(1));
    assert!(!spaced(
        &ui.borrow().account_nodes[&second_id].borrow::<SidebarNode>()
    ));
    for mail_enabled in [false, true] {
        update.accounts.get_mut(&second_id).unwrap().mail_enabled = mail_enabled;
        ui.borrow_mut().apply_update(&update);
        assert_eq!(selected_position(&tree), Some(u32::from(mail_enabled)));
        assert_eq!(ui.borrow().accounts.selected_account(), Some(&id));
        assert_eq!(
            ui.borrow().root.item(u32::from(mail_enabled)).unwrap(),
            first_item
        );
        assert_eq!(spaced(&first_row), mail_enabled);
    }
    // Enter on the selected row, or the tree selecting it again as Tab
    // entering it does, changes nothing; unselecting all (Ctrl+Shift+A)
    // leaves it marked.
    assert!(first_row.widgets.row.grab_focus());
    tree.emit_by_name::<()>("activate-cursor-row", &[]);
    tree.emit_by_name::<()>(
        "row-selected",
        &[&first_row.widgets.row.upcast_ref::<gtk::ListBoxRow>()],
    );
    tree.unselect_all();
    assert_eq!(selected_position(&tree), Some(1));
    assert_eq!(ui.borrow().accounts.selected_account(), Some(&id));

    // On a narrow window a click or Enter shows the selected mail's list;
    // the arrow keys keep the sidebar.
    let folders_split = ui.borrow().folders_split.clone();
    folders_split.set_collapsed(true);
    folders_split.set_show_sidebar(true);
    move_cursor(&tree, -1);
    assert_eq!(selected_position(&tree), Some(0));
    assert!(folders_split.shows_sidebar());
    move_cursor(&tree, 1);
    tree.emit_by_name::<()>("activate-cursor-row", &[]);
    assert!(!folders_split.shows_sidebar());
    folders_split.set_collapsed(false);
    folders_split.set_show_sidebar(true);
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
    // The expander comes before the icon, as the form means it.
    row_detail(&ui.borrow(), "Projects", |node| {
        assert_eq!(
            node.widgets.expander.next_sibling().as_ref(),
            Some(node.widgets.icon.upcast_ref())
        );
    });

    // A heading and a container do not react to the pointer and cannot be
    // selected; a click on a folder selects it.
    let reacts = |title: &str| {
        row_detail(&ui.borrow(), title, |node| {
            node.widgets.row.is_activatable()
        })
    };
    assert!(!reacts(&heading));
    assert!(!reacts("Projects"));
    assert!(reacts("Reports"));
    for title in [heading.as_str(), "Projects"] {
        select_title(&tree, &ui, title);
        assert!(ui.borrow().accounts.selection().is_none(), "{title}");
    }

    // The arrow keys select the folder they reach; a heading they pass
    // changes nothing.
    select_title(&tree, &ui, "Inbox");
    let inbox_row = row_detail(&ui.borrow(), "Inbox", |node| node.widgets.row.clone());
    assert!(inbox_row.grab_focus());
    move_cursor(&tree, -1);
    assert_eq!(
        ui.borrow().accounts.selection(),
        Some(&Selection::Mailbox(FolderRef {
            account: id.clone(),
            identity: "inbox".to_owned(),
        }))
    );
    move_cursor(&tree, 2);
    assert_eq!(
        ui.borrow().accounts.selection(),
        Some(&Selection::Mailbox(FolderRef {
            account: id.clone(),
            identity: "Bin".to_owned(),
        }))
    );
    select_title(&tree, &ui, "Reports");
    assert_eq!(
        ui.borrow().accounts.selection(),
        Some(&Selection::Mailbox(FolderRef {
            account: id.clone(),
            identity: "Projects/Reports".to_owned(),
        }))
    );

    // Collapsing the shown folder's parent clears the selection. The focus
    // rests on the tree's own row, which the platform outlines, and the row's
    // expander collapses it; the row passes real key presses on to the
    // expander, which only a hand check can press (quickstart step 5).
    let changes_before = changes.get();
    let projects = tree_row(&ui.borrow(), "Projects");
    let (projects_row, expander) = row_detail(&ui.borrow(), "Projects", |node| {
        (node.widgets.row.clone(), node.widgets.expander.clone())
    });
    assert!(projects_row.grab_focus());
    dispatch_pending();
    let focus = gtk::prelude::RootExt::focus(&window).expect("the tree has the focus");
    assert_eq!(focus, *projects_row.upcast_ref::<gtk::Widget>());
    assert_eq!(focus.parent().as_ref(), Some(tree.upcast_ref()));
    assert_eq!(expander.list_row().as_ref(), Some(&projects));
    expander
        .activate_action("listitem.collapse", None)
        .expect("the expander's keys");
    dispatch_pending();
    assert!(!projects.is_expanded());
    assert!(ui.borrow().accounts.selection().is_none());
    assert_eq!(selected_position(&tree), None);
    assert_eq!(changes.get(), changes_before + 1);
    // Tab leaves the tree after the row, not after every row.
    window.child_focus(gtk::DirectionType::TabForward);
    dispatch_pending();
    assert!(!contains_focus(&ui.borrow().tree));
    // The same list again keeps the rows: the collapsed folder stays so.
    assert!(!ui.borrow_mut().show_folders(&id, folders()));
    assert!(!projects.is_expanded());
    projects.set_expanded(true);

    // Collapsing it with the pointer while the shown folder's row has the
    // focus: the focus moves to the collapsed row, and nothing is selected.
    select_title(&tree, &ui, "Reports");
    let reports_row = row_detail(&ui.borrow(), "Reports", |node| node.widgets.row.clone());
    assert!(reports_row.grab_focus());
    projects.set_expanded(false);
    run_frames();
    let focus = gtk::prelude::RootExt::focus(&window).expect("the tree keeps the focus");
    assert_eq!(focus, *projects_row.upcast_ref::<gtk::Widget>());
    assert!(ui.borrow().accounts.selection().is_none());
    projects.set_expanded(true);

    // A changed list rebuilds the subtree: the shown folder's row is marked
    // again and takes the keyboard focus; a folder gone from the list is no
    // longer selected.
    select_title(&tree, &ui, "Zeta");
    let zeta_row = row_detail(&ui.borrow(), "Zeta", |node| node.widgets.row.clone());
    assert!(zeta_row.grab_focus());
    let mut with_gamma = folders();
    with_gamma.push(folder("Gamma", None, None, true));
    assert!(!ui.borrow_mut().show_folders(&id, with_gamma));
    // Checked before the window's own focus move after the next frame.
    let new_zeta_row = row_detail(&ui.borrow(), "Zeta", |node| node.widgets.row.clone());
    assert_eq!(focused(&window), Some(new_zeta_row.clone().upcast()));
    run_frames();
    let zeta = position_of_title(&ui.borrow(), "Zeta");
    assert_eq!(selected_position(&tree), Some(zeta));
    assert_eq!(focused(&window), Some(new_zeta_row.upcast()));
    // The rebuilt subtree's old rows are freed.
    let old_zeta_row = zeta_row.downgrade();
    drop(zeta_row);
    assert!(released(&old_zeta_row));
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
    select_title(&tree, &ui, &other_account);
    assert_eq!(
        ui.borrow().accounts.selection(),
        Some(&Selection::Account(second_id.clone()))
    );

    dispatch_pending();
    assert!(
        ui.borrow().account_nodes[&id]
            .borrow::<SidebarNode>()
            .widgets
            .row
            .grab_focus()
    );
    update.accounts.remove(&id);
    ui.borrow_mut().apply_update(&update);
    // The keyboard stays on the selected row.
    let other_row = ui.borrow().account_nodes[&second_id]
        .borrow::<SidebarNode>()
        .widgets
        .row
        .clone();
    assert_eq!(focused(&window), Some(other_row.upcast()));
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
            node.borrow::<SidebarNode>().widgets.row.title().to_string()
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

/// Selects the row titled `title`, as a click does; the sidebar is not
/// borrowed meanwhile, since the selection changes it.
fn select_title(tree: &gtk::ListBox, ui: &Rc<RefCell<SidebarUi>>, title: &str) {
    let position = position_of_title(&ui.borrow(), title);
    select_position(tree, position);
}

fn select_position(tree: &gtk::ListBox, position: u32) {
    tree.select_row(tree.row_at_index(position as i32).as_ref());
}

fn selected_position(tree: &gtk::ListBox) -> Option<u32> {
    tree.selected_row().map(|row| row.index() as u32)
}

/// Moves the keyboard focus by `rows`, as the arrow keys do.
fn move_cursor(tree: &gtk::ListBox, rows: i32) {
    tree.emit_by_name::<()>(
        "move-cursor",
        &[&gtk::MovementStep::DisplayLines, &rows, &false, &false],
    );
    dispatch_pending();
}

/// Whether the tree shows the spacer above this account's row.
fn spaced(node: &SidebarNode) -> bool {
    let spacing = &node.widgets.account_spacing;
    node.widgets.row.header().as_ref() == Some(spacing.upcast_ref()) && spacing.parent().is_some()
}

fn focused(window: &adw::Window) -> Option<gtk::Widget> {
    gtk::prelude::RootExt::focus(window)
}

/// Whether the object is freed once pending events ran; the accessibility
/// bus may hold it for a few milliseconds.
fn released<T: glib::object::ObjectType>(object: &glib::WeakRef<T>) -> bool {
    for _ in 0..100 {
        dispatch_pending();
        if object.upgrade().is_none() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

/// Runs the main loop over a few frames: the window moves a focus that left
/// with its row only after the next frame.
fn run_frames() {
    for _ in 0..15 {
        dispatch_pending();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
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
