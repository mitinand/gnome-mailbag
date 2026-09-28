// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sidebar: each account and, once its folder list is stored, its
//! folders as a tree (specs/008-folders FR-006, FR-009, FR-010). It shows the
//! selection `AccountList` owns and tells the window when the user changed
//! it. A list box shows a `TreeListModel`; each row is an action row whose
//! first prefix is the model's `TreeExpander` (specs/008-folders research §9).
//! No Workbench demo shows a tree in a list box: its "List Box" demo gives
//! the list, its "List View with a Tree" the model and the expander.

use crate::accounts::{
    AccountList, AccountPage, AccountProblem, AccountRow, ExclusionReason, Selection,
};
use crate::settings::LaunchError;
use adw::{gio, glib, gtk, prelude::*};
use goa_adapter::{AccountUpdate, ErrorCause};
use mailbag_domain::{AccountId, Folder, FolderRef, FolderRole};
use mailbag_providers::MailProvider;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[cfg(test)]
mod tests;

/// What the account page's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageAction {
    RetryCheck,
    OnlineAccounts,
}

/// One row of the tree, an account or a folder, with its widgets and the
/// store of its children.
struct SidebarNode {
    key: Selection,
    widgets: RowWidgets,
    children: gio::ListStore,
    /// The folder list an account's rows show, in identity order; empty for
    /// a folder.
    listed_folders: RefCell<Vec<Folder>>,
    /// The account row's problem button; `None` for a folder.
    problem: Option<ProblemWidgets>,
}

/// The widgets of folder-row.ui.
struct RowWidgets {
    row: adw::ActionRow,
    /// The tree's header above every account but the first.
    account_spacing: gtk::Separator,
    expander: gtk::TreeExpander,
    icon: gtk::Image,
    /// Hidden until unread counts arrive (specs/008-folders FR-013(a)).
    badge: gtk::Label,
}

/// The widgets of account-problem.ui.
struct ProblemWidgets {
    button: gtk::MenuButton,
    popover: gtk::Popover,
    explanation: gtk::Label,
    retry: gtk::Button,
    settings: gtk::Button,
}

pub struct SidebarUi {
    accounts: AccountList,
    account_nodes: BTreeMap<AccountId, glib::BoxedAnyObject>,
    root: gio::ListStore,
    tree_model: gtk::TreeListModel,
    tree: gtk::ListBox,
    mail_split: adw::NavigationSplitView,
    folders_split: adw::OverlaySplitView,
    retry_check: gio::SimpleAction,
    toasts: adw::ToastOverlay,
    on_selection_changed: RefCell<Option<Box<dyn Fn()>>>,
}

impl SidebarUi {
    pub fn new(builder: &gtk::Builder) -> Rc<RefCell<Self>> {
        let tree: gtk::ListBox = builder.object("folder_tree").expect("folder_tree");
        let mail_split = builder.object("mail_split").expect("mail_split");
        let folders_split = builder.object("folders_split").expect("folders_split");
        let toasts = builder.object("toasts").expect("toasts");
        let root = gio::ListStore::new::<glib::BoxedAnyObject>();
        let tree_model = gtk::TreeListModel::new(root.clone(), false, true, child_rows);
        tree.bind_model(Some(&tree_model), create_row);
        let header_accounts = root.clone();
        tree.set_header_func(move |row, before| set_account_spacing(&header_accounts, row, before));
        let ui = Rc::new(RefCell::new(Self {
            accounts: AccountList::default(),
            account_nodes: BTreeMap::new(),
            root,
            tree_model: tree_model.clone(),
            tree: tree.clone(),
            mail_split,
            folders_split,
            retry_check: gio::SimpleAction::new("retry-accounts", None),
            toasts,
            on_selection_changed: RefCell::new(None),
        }));
        let selecting = Rc::downgrade(&ui);
        tree.connect_row_selected(move |_, row| {
            let Some(ui) = selecting.upgrade() else {
                return;
            };
            // The sidebar is borrowed while it changes the tree itself, so its
            // own marking and rebuilds end here; only the user's choice goes on.
            let Ok(mut sidebar) = ui.try_borrow_mut() else {
                return;
            };
            let Some(position) = row.and_then(|row| u32::try_from(row.index()).ok()) else {
                // Unselecting all (Ctrl+Shift+A) leaves the selection shown.
                sidebar.show_selection();
                return;
            };
            let selected = sidebar.select_at(position);
            drop(sidebar);
            // The window shows the newly selected mail; it must not find the
            // sidebar borrowed while it reads the selection.
            if selected {
                ui.borrow().notify_selection_changed();
            }
        });
        let activating = Rc::downgrade(&ui);
        tree.connect_row_activated(move |_, _| {
            if let Some(ui) = activating.upgrade() {
                ui.borrow().show_list_on_narrow_window();
            }
        });
        let collapsing = Rc::downgrade(&ui);
        tree_model.connect_items_changed(move |_, position, removed, _| {
            let Some(ui) = collapsing.upgrade() else {
                return;
            };
            // Only rows the user collapsed away: the sidebar's own changes
            // find it borrowed.
            let Ok(mut sidebar) = ui.try_borrow_mut() else {
                return;
            };
            if removed == 0 {
                return;
            }
            sidebar.keep_focus_on_collapsed(position);
            // Collapsing the shown mailbox's parent or account clears the
            // selection (specs/008-folders FR-010).
            let cleared = sidebar.clear_hidden_selection();
            drop(sidebar);
            if cleared {
                ui.borrow().notify_selection_changed();
            }
        });
        ui.borrow().mail_split.set_show_content(false);
        ui
    }

    /// Retry Check, for the application to publish as `app.retry-accounts`.
    pub fn retry_check_action(&self) -> &gio::SimpleAction {
        &self.retry_check
    }

    pub fn connect_retry_check(&self, retry_check: impl Fn() + 'static) {
        self.retry_check.connect_activate(move |_, _| {
            tracing::info!("Retry Check requested");
            retry_check();
        });
    }

    /// Called after the user selected a row or collapsed the selected
    /// mailbox away, so the window can show the selected mail. Selecting
    /// never loads. `apply_update` and `show_folders` clear a selection
    /// without calling it; their caller shows the result.
    pub fn connect_selection_changed(&self, on_changed: impl Fn() + 'static) {
        *self.on_selection_changed.borrow_mut() = Some(Box::new(on_changed));
    }

    /// Which account explanation the status page shows. Every page other than
    /// AccountsShown covers the mail list and the reader.
    pub fn page(&self) -> AccountPage {
        self.accounts.page()
    }

    pub fn selection(&self) -> Option<&Selection> {
        self.accounts.selection()
    }

    /// The load sequence of the selected account.
    pub fn selected_provider(&self) -> Option<MailProvider> {
        let account = self.accounts.selected_account()?;
        Some(self.accounts.visible_accounts()[account].provider)
    }

    /// The accounts the sidebar shows.
    pub fn shown_accounts(&self) -> Vec<AccountId> {
        self.accounts.visible_accounts().keys().cloned().collect()
    }

    /// Title and description of the account page, for the window to show.
    pub fn page_text(&self) -> (&'static str, &'static str) {
        match self.accounts.page() {
            AccountPage::Loading => ("Loading accounts", ""),
            AccountPage::MailUnavailable => (
                "Mail settings unavailable",
                "Unable to get mail settings from Online Accounts. Try checking again.",
            ),
            AccountPage::ReadFailed(cause) => ("Unable to get accounts", check_error_text(cause)),
            AccountPage::NoAccounts | AccountPage::NoEligibleAccounts => (
                "No mail accounts",
                if self
                    .accounts
                    .excluded_reasons()
                    .contains(&ExclusionReason::MailDisabled)
                    && !self
                        .accounts
                        .excluded_reasons()
                        .contains(&ExclusionReason::UnsupportedProvider)
                {
                    "Enable Mail for your account in Online Accounts."
                } else {
                    "Add a mail account or enable Mail in Online Accounts."
                },
            ),
            // The window shows the mail instead.
            AccountPage::AccountsShown => ("", ""),
        }
    }

    /// The button the account page offers beside its text.
    pub fn page_action(&self) -> Option<PageAction> {
        match self.accounts.page() {
            AccountPage::ReadFailed(_) | AccountPage::MailUnavailable => {
                Some(PageAction::RetryCheck)
            }
            AccountPage::NoAccounts | AccountPage::NoEligibleAccounts => {
                Some(PageAction::OnlineAccounts)
            }
            AccountPage::Loading | AccountPage::AccountsShown => None,
        }
    }

    /// Whether a Retry Check is running, so its buttons show progress.
    pub fn retry_pending(&self) -> bool {
        self.accounts.retry_pending()
    }

    /// The disambiguated row label, for the mail list title.
    pub fn label_of(&self, id: &AccountId) -> Option<String> {
        Some(self.accounts.visible_accounts().get(id)?.label.clone())
    }

    /// The shown name of a folder the tree shows.
    pub fn folder_name(&self, folder: &FolderRef) -> Option<String> {
        let key = Selection::Mailbox(folder.clone());
        let node = self.node_at(self.position_of(&key)?)?;
        Some(node.borrow::<SidebarNode>().widgets.row.title().to_string())
    }

    pub fn shows_account(&self, id: &AccountId) -> bool {
        self.accounts.visible_accounts().contains_key(id)
    }

    fn notify_selection_changed(&self) {
        if let Some(on_changed) = self.on_selection_changed.borrow().as_ref() {
            on_changed();
        }
    }

    pub fn apply_update(&mut self, update: &AccountUpdate) {
        let hidden_notices = self.accounts.apply_update(update);
        let removed: Vec<_> = self
            .account_nodes
            .keys()
            .filter(|id| !self.accounts.visible_accounts().contains_key(*id))
            .cloned()
            .collect();
        let tree_had_focus = contains_focus(&self.tree);
        for id in removed {
            let item = self.account_nodes.remove(&id).unwrap();
            let node = item.borrow::<SidebarNode>();
            let problem = node.problem.as_ref().expect("an account row");
            problem.popover.popdown();
            if let Some(position) = self.root.find(&item) {
                self.root.remove(position);
            }
            node.release_rows();
        }
        for (position, (id, account)) in self.accounts.visible_accounts().iter().enumerate() {
            let item = self.account_nodes.entry(id.clone()).or_insert_with(|| {
                let item = glib::BoxedAnyObject::new(SidebarNode::account(id, &self.retry_check));
                self.root.insert(position as u32, &item);
                item
            });
            item.borrow::<SidebarNode>()
                .show_account(account, self.accounts.retry_pending());
        }
        self.show_selection();
        // The keyboard stays in the tree when its account row is removed: on
        // the selected row, else the first.
        if tree_had_focus && !contains_focus(&self.tree) {
            let row = self
                .tree
                .selected_row()
                .or_else(|| self.tree.row_at_index(0));
            if let Some(row) = row {
                row.grab_focus();
            }
        }
        for notice in hidden_notices {
            self.show_toast(&format!(
                "{} was removed or Mail was turned off in Online Accounts.",
                notice.label
            ));
        }
    }

    /// Replaces the account's folders in the tree with its stored folder
    /// list: the system folders first in their order, then the others in
    /// the locale's order, at every level. The account becomes a heading once
    /// it has a folder that can be opened. An unchanged list keeps the rows,
    /// so what the user collapsed stays collapsed. Returns whether the
    /// selection was cleared because the tree no longer shows it: a mailbox
    /// gone from the list, or an account whose folders appeared
    /// (specs/008-folders FR-010).
    pub fn show_folders(&mut self, account: &AccountId, mut folders: Vec<Folder>) -> bool {
        let Some(item) = self.account_nodes.get(account).cloned() else {
            return false;
        };
        folders.sort_by(|left, right| left.identity.cmp(&right.identity));
        if *item.borrow::<SidebarNode>().listed_folders.borrow() == folders {
            return false;
        }
        let openable = folders.iter().any(|folder| folder.selectable);
        let mut by_parent: BTreeMap<Option<String>, Vec<Folder>> = BTreeMap::new();
        for folder in &folders {
            by_parent
                .entry(folder.parent.clone())
                .or_default()
                .push(folder.clone());
        }
        let tree_had_focus = contains_focus(&self.tree);
        {
            let node = item.borrow::<SidebarNode>();
            let old_folders: Vec<glib::BoxedAnyObject> = node.children.iter().flatten().collect();
            node.children.remove_all();
            for folder in old_folders {
                folder.borrow::<SidebarNode>().release_rows();
            }
            fill_folders(&node.children, account, None, &by_parent);
            node.set_selectable(!openable);
            node.widgets
                .expander
                .set_hide_expander(node.children.n_items() == 0);
            *node.listed_folders.borrow_mut() = folders;
        }
        let selection_gone = self
            .accounts
            .selection()
            .filter(|selection| selection.account() == account)
            .is_some_and(|selection| !self.shows_selectable(selection));
        if selection_gone {
            self.accounts.clear_selection();
        }
        self.show_selection();
        // The keyboard stays in the tree when its row is rebuilt away: on the
        // selected row, else the account's.
        if tree_had_focus && !contains_focus(&self.tree) {
            let account_row = item.borrow::<SidebarNode>().widgets.row.clone();
            match self.tree.selected_row() {
                Some(row) => row.grab_focus(),
                None => account_row.grab_focus(),
            };
        }
        selection_gone
    }

    /// Shows a short notice in the window's existing toast area.
    pub fn show_toast(&self, title: &str) {
        self.toasts
            .add_toast(adw::Toast::builder().title(title).use_markup(false).build());
    }

    pub fn show_settings_error(&self, error: LaunchError) {
        self.show_toast(error.message());
    }

    /// Selects the row the user chose at `position`, by a click, the arrow
    /// keys or Tab; returns whether the selection changed. The tree selects
    /// only rows that can be selected, and selects the selected row again
    /// when Tab enters it.
    fn select_at(&mut self, position: u32) -> bool {
        let Some(item) = self.node_at(position) else {
            return false;
        };
        let node = item.borrow::<SidebarNode>();
        if self.accounts.selection() == Some(&node.key) {
            return false;
        }
        self.accounts.select(node.key.clone());
        self.mail_split.set_show_content(false);
        true
    }

    /// A click or Enter on a row shows the selected mail's list on a narrow
    /// window; the arrow keys keep the sidebar (specs/008-folders FR-010).
    fn show_list_on_narrow_window(&self) {
        if self.folders_split.is_collapsed() {
            self.folders_split.set_show_sidebar(false);
        }
    }

    /// Gives the keyboard focus to the row collapsed at `position - 1` when
    /// it left with a hidden row: the window would move it to the tree's
    /// first row, which selects that row (specs/008-folders FR-010).
    fn keep_focus_on_collapsed(&self, position: u32) {
        let focus_left = self
            .tree
            .root()
            .is_some_and(|root| root.focus().is_none_or(|focus| focus.root().is_none()));
        let collapsed = position
            .checked_sub(1)
            .and_then(|collapsed| self.tree.row_at_index(collapsed as i32));
        if focus_left && let Some(collapsed) = collapsed {
            collapsed.grab_focus();
        }
    }

    /// Clears a selection whose row the user collapsed away; returns whether
    /// it did.
    fn clear_hidden_selection(&mut self) -> bool {
        let hidden = self
            .accounts
            .selection()
            .is_some_and(|selection| self.position_of(selection).is_none());
        if hidden {
            self.accounts.clear_selection();
        }
        hidden
    }

    /// Marks the selected row in the tree, or none.
    fn show_selection(&self) {
        let row = self
            .accounts
            .selection()
            .and_then(|selection| self.position_of(selection))
            .and_then(|position| self.tree.row_at_index(position as i32));
        match row {
            Some(row) => self.tree.select_row(Some(&row)),
            None => self.tree.unselect_all(),
        }
    }

    fn shows_selectable(&self, selection: &Selection) -> bool {
        self.position_of(selection)
            .and_then(|position| self.node_at(position))
            .is_some_and(|item| item.borrow::<SidebarNode>().widgets.row.is_selectable())
    }

    /// The row of `key` among the rows the tree shows, which leaves out the
    /// children of collapsed rows.
    fn position_of(&self, key: &Selection) -> Option<u32> {
        (0..self.tree_model.n_items()).find(|&position| {
            self.node_at(position)
                .is_some_and(|item| item.borrow::<SidebarNode>().key == *key)
        })
    }

    /// The row of an account, or of one of its folders, for the window's
    /// graphical test to select.
    #[cfg(test)]
    pub fn position_of_row(&self, account: &AccountId, folder: Option<&str>) -> Option<u32> {
        let key = match folder {
            Some(identity) => Selection::Mailbox(FolderRef {
                account: account.clone(),
                identity: identity.to_owned(),
            }),
            None => Selection::Account(account.clone()),
        };
        self.position_of(&key)
    }

    fn node_at(&self, position: u32) -> Option<glib::BoxedAnyObject> {
        node_at(&self.tree_model, position)
    }
}

fn node_at(tree_model: &gtk::TreeListModel, position: u32) -> Option<glib::BoxedAnyObject> {
    tree_model
        .item(position)
        .and_downcast::<gtk::TreeListRow>()?
        .item()
        .and_downcast()
}

/// The children of a row: an account's folders, which it may gain later, or
/// a folder's subfolders. A folder without any cannot be expanded.
fn child_rows(item: &glib::Object) -> Option<gio::ListModel> {
    let node = item.downcast_ref::<glib::BoxedAnyObject>()?;
    let node = node.borrow::<SidebarNode>();
    let is_account = matches!(node.key, Selection::Account(_));
    (is_account || node.children.n_items() > 0).then(|| node.children.clone().upcast())
}

/// Fills `store` with the folders under `parent`, each with its subfolders:
/// the system folders first in `FolderRole::ORDER`, then by the locale's
/// collation of their names (specs/008-folders FR-009).
fn fill_folders(
    store: &gio::ListStore,
    account: &AccountId,
    parent: Option<&str>,
    by_parent: &BTreeMap<Option<String>, Vec<Folder>>,
) {
    let Some(siblings) = by_parent.get(&parent.map(str::to_owned)) else {
        return;
    };
    let mut siblings: Vec<&Folder> = siblings.iter().collect();
    siblings.sort_by_cached_key(|folder| {
        let role_rank = folder
            .role
            .and_then(|role| {
                FolderRole::ORDER
                    .iter()
                    .position(|ordered| *ordered == role)
            })
            .unwrap_or(FolderRole::ORDER.len());
        (role_rank, glib::CollationKey::from(&folder.name))
    });
    for folder in siblings {
        let node = SidebarNode::folder(account, folder);
        fill_folders(&node.children, account, Some(&folder.identity), by_parent);
        store.append(&glib::BoxedAnyObject::new(node));
    }
}

/// The row that shows a tree row: its node's action row, whose expander
/// follows the tree row.
fn create_row(item: &glib::Object) -> gtk::Widget {
    let tree_row = item.downcast_ref::<gtk::TreeListRow>().expect("a tree row");
    let node = tree_row
        .item()
        .and_downcast::<glib::BoxedAnyObject>()
        .expect("a sidebar node");
    let node = node.borrow::<SidebarNode>();
    node.widgets.expander.set_list_row(Some(tree_row));
    node.widgets.row.clone().upcast()
}

/// Puts the spacer above every account but the first (specs/008-folders,
/// Assumptions). The account is found by its row: while rows are removed the
/// tree's positions already name other rows.
fn set_account_spacing(
    accounts: &gio::ListStore,
    row: &gtk::ListBoxRow,
    before: Option<&gtk::ListBoxRow>,
) {
    let spacing = before.and_then(|_| {
        accounts
            .iter::<glib::BoxedAnyObject>()
            .flatten()
            .find_map(|item| {
                let node = item.borrow::<SidebarNode>();
                (node.widgets.row.upcast_ref::<gtk::ListBoxRow>() == row)
                    .then(|| node.widgets.account_spacing.clone())
            })
    });
    row.set_header(spacing.as_ref());
}

impl RowWidgets {
    fn new() -> Self {
        let builder = gtk::Builder::from_string(include_str!("../resources/ui/folder-row.ui"));
        let row: adw::ActionRow = builder.object("folder_row").unwrap();
        let expander: gtk::TreeExpander = builder.object("folder_expander").unwrap();
        forward_expander_keys(&row, &expander);
        Self {
            row,
            account_spacing: builder.object("account_spacing").unwrap(),
            expander,
            icon: builder.object("folder_icon").unwrap(),
            badge: builder.object("folder_badge").unwrap(),
        }
    }
}

impl SidebarNode {
    fn account(id: &AccountId, retry_check: &gio::SimpleAction) -> Self {
        let widgets = RowWidgets::new();
        // An account row never shows a count; its problem button takes the place.
        widgets.row.remove(&widgets.badge);
        widgets.row.add_css_class("heading");
        widgets.expander.set_hide_expander(true);
        let builder = gtk::Builder::from_string(include_str!("../resources/ui/account-problem.ui"));
        let problem = ProblemWidgets {
            button: builder.object("account_problem").unwrap(),
            popover: builder.object("account_problem_popover").unwrap(),
            explanation: builder.object("account_problem_explanation").unwrap(),
            retry: builder.object("account_problem_retry").unwrap(),
            settings: builder.object("account_problem_settings").unwrap(),
        };
        let retry_check = retry_check.clone();
        problem
            .retry
            .connect_clicked(move |_| retry_check.activate(None));
        widgets.row.add_suffix(&problem.button);
        let node = Self {
            key: Selection::Account(id.clone()),
            widgets,
            children: gio::ListStore::new::<glib::BoxedAnyObject>(),
            listed_folders: RefCell::default(),
            problem: Some(problem),
        };
        // Until it has a folder that can be opened (specs/008-folders FR-009).
        node.set_selectable(true);
        node
    }

    fn folder(account: &AccountId, folder: &Folder) -> Self {
        let widgets = RowWidgets::new();
        widgets.row.set_title(shown_name(folder));
        widgets.icon.set_icon_name(Some(folder_icon(folder.role)));
        let node = Self {
            key: Selection::Mailbox(FolderRef {
                account: account.clone(),
                identity: folder.identity.clone(),
            }),
            widgets,
            children: gio::ListStore::new::<glib::BoxedAnyObject>(),
            listed_folders: RefCell::default(),
            problem: None,
        };
        node.set_selectable(folder.selectable);
        node
    }

    /// Whether the row can be selected: a folder that can be opened, or an
    /// account without such a folder. A row that cannot, a heading or a
    /// container, does not react to the pointer (specs/008-folders FR-009).
    fn set_selectable(&self, selectable: bool) {
        self.widgets.row.set_selectable(selectable);
        self.widgets.row.set_activatable(selectable);
    }

    /// Lets go of the tree rows that this row and its folders' rows hold: a
    /// tree row holds its node, so a removed node is freed only then.
    fn release_rows(&self) {
        self.widgets
            .expander
            .set_list_row(None::<&gtk::TreeListRow>);
        for folder in self.children.iter::<glib::BoxedAnyObject>().flatten() {
            folder.borrow::<SidebarNode>().release_rows();
        }
    }

    fn show_account(&self, account: &AccountRow, retry_pending: bool) {
        let problem = self.problem.as_ref().expect("an account row");
        self.widgets.row.set_title(&account.label);
        self.widgets.icon.set_icon_name(Some(account.icon_name));
        let explanation = account
            .problems
            .iter()
            .map(problem_text)
            .collect::<Vec<_>>()
            .join("\n");
        if explanation.is_empty() {
            if contains_focus(&problem.button) || contains_focus(&problem.popover) {
                self.widgets.row.grab_focus();
            }
            problem.popover.popdown();
        }
        problem.button.set_visible(!explanation.is_empty());
        problem.button.set_tooltip_text(Some(&explanation));
        problem
            .button
            .update_property(&[gtk::accessible::Property::Label(&explanation)]);
        problem.explanation.set_text(&explanation);
        problem
            .settings
            .set_visible(account.problems.contains(&AccountProblem::AttentionNeeded));
        problem.retry.set_visible(
            account
                .problems
                .iter()
                .any(|problem| *problem != AccountProblem::AttentionNeeded),
        );
        show_check_progress(&problem.retry, retry_pending);
    }
}

/// A Retry Check button, insensitive and saying so while a check runs.
pub fn show_check_progress(button: &gtk::Button, pending: bool) {
    button.set_sensitive(!pending);
    button.set_label(if pending {
        "Checking…"
    } else {
        "Retry Check"
    });
}

/// The keyboard focus rests on the row, which the platform outlines; the
/// keys that expand and collapse it are its expander's, so the row passes
/// them on (specs/008-folders, Assumptions).
fn forward_expander_keys(row: &adw::ActionRow, expander: &gtk::TreeExpander) {
    let keys = gtk::EventControllerKey::new();
    let row_expander = expander.downgrade();
    keys.connect_key_pressed(move |keys, _, _, _| match row_expander.upgrade() {
        Some(expander) if keys.forward(&expander) => glib::Propagation::Stop,
        _ => glib::Propagation::Proceed,
    });
    row.add_controller(keys);
}

fn contains_focus(widget: &impl IsA<gtk::Widget>) -> bool {
    widget
        .root()
        .and_then(|root| root.focus())
        .is_some_and(|focus| focus == *widget.as_ref() || focus.is_ancestor(widget))
}

/// The name a folder is shown under: its server's, except the reserved IMAP
/// name INBOX, a word of the protocol rather than a name
/// (specs/008-folders FR-005).
fn shown_name(folder: &Folder) -> &str {
    if folder.identity.eq_ignore_ascii_case("INBOX") {
        "Inbox"
    } else {
        &folder.name
    }
}

/// The icon of a folder with this role; the `mailbag-` ones are bundled with
/// the application, the others come from the icon theme
/// (specs/008-folders/contracts/folders.md).
fn folder_icon(role: Option<FolderRole>) -> &'static str {
    match role {
        Some(FolderRole::Inbox) => "mailbag-folder-inbox-symbolic",
        Some(FolderRole::Starred) => "mailbag-folder-starred-symbolic",
        Some(FolderRole::Important) => "mailbag-folder-important-symbolic",
        Some(FolderRole::Junk) => "mail-mark-junk-symbolic",
        Some(FolderRole::Trash) => "user-trash-symbolic",
        Some(FolderRole::Archive) => "mailbag-folder-archive-symbolic",
        Some(FolderRole::Drafts) => "document-edit-symbolic",
        Some(FolderRole::Sent) => "mailbag-folder-sent-symbolic",
        Some(FolderRole::AllMail) | None => "folder-symbolic",
    }
}

fn problem_text(problem: &AccountProblem) -> &'static str {
    match problem {
        AccountProblem::CheckUnconfirmed => {
            "This account could not be checked. Try checking again."
        }
        AccountProblem::MailUnavailable => {
            "Unable to get this account's mail settings. Try checking again."
        }
        AccountProblem::AttentionNeeded => {
            "Open Online Accounts to resolve a problem with this account."
        }
    }
}

fn check_error_text(cause: ErrorCause) -> &'static str {
    match cause {
        ErrorCause::Unavailable => "Online Accounts is unavailable. Try checking again.",
        ErrorCause::AccessDenied => "Access to Online Accounts was denied.",
        ErrorCause::Timeout => "Online Accounts did not respond in time. Try checking again.",
        ErrorCause::InvalidReply => {
            "Online Accounts returned an incomplete or invalid account list. Try checking again."
        }
    }
}
