// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::accounts::{AccountList, AccountPage, AccountProblem, AccountRow, ExclusionReason};
use crate::settings::LaunchError;
use adw::{gio, glib, gtk, prelude::*};
use goa_adapter::{AccountUpdate, ErrorCause};
use mailbag_domain::AccountId;
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

struct AccountWidgets {
    id: AccountId,
    root: gtk::Box,
    details: adw::ActionRow,
    icon: gtk::Image,
    problem: gtk::MenuButton,
    popover: gtk::Popover,
    explanation: gtk::Label,
    retry: gtk::Button,
    settings: gtk::Button,
}

pub struct AccountUi {
    accounts: AccountList,
    rows: BTreeMap<AccountId, glib::BoxedAnyObject>,
    store: gio::ListStore,
    selection: gtk::SingleSelection,
    tree: gtk::ListView,
    mail_split: adw::NavigationSplitView,
    folders_split: adw::OverlaySplitView,
    retry_check: gio::SimpleAction,
    toasts: adw::ToastOverlay,
    on_selection_changed: RefCell<Option<Box<dyn Fn()>>>,
}

impl AccountUi {
    pub fn new(builder: &gtk::Builder) -> Rc<RefCell<Self>> {
        let tree: gtk::ListView = builder.object("folder_tree").expect("folder_tree");
        let mail_split = builder.object("mail_split").expect("mail_split");
        let folders_split = builder.object("folders_split").expect("folders_split");
        let toasts = builder.object("toasts").expect("toasts");
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::new(Some(store.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        tree.set_model(Some(&selection));
        tree.set_factory(Some(&create_row_factory()));
        let ui = Rc::new(RefCell::new(Self {
            accounts: AccountList::default(),
            rows: BTreeMap::new(),
            store,
            selection,
            tree: tree.clone(),
            mail_split,
            folders_split,
            retry_check: gio::SimpleAction::new("retry-accounts", None),
            toasts,
            on_selection_changed: RefCell::new(None),
        }));
        let weak = Rc::downgrade(&ui);
        tree.connect_activate(move |_, position| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            {
                let mut ui = ui.borrow_mut();
                let item = ui
                    .store
                    .item(position)
                    .unwrap()
                    .downcast::<glib::BoxedAnyObject>()
                    .unwrap();
                let id = item.borrow::<AccountWidgets>().id.clone();
                ui.accounts.select_account(id);
                ui.selection.set_selected(position);
                ui.mail_split.set_show_content(false);
                if ui.folders_split.is_collapsed() {
                    ui.folders_split.set_show_sidebar(false);
                }
            }
            // The window shows the newly selected account's mail; it must not
            // find this account list borrowed while it reads the selection.
            ui.borrow().notify_selection_changed();
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

    /// Called after the user selected an account, so the window can show that
    /// account's mail. Selecting never loads.
    pub fn connect_selection_changed(&self, on_selected: impl Fn() + 'static) {
        *self.on_selection_changed.borrow_mut() = Some(Box::new(on_selected));
    }

    /// Which account explanation the status page shows. Every page other than
    /// SelectedAccount covers the mail list and the reader.
    pub fn page(&self) -> AccountPage {
        self.accounts.page()
    }

    pub fn selected_id(&self) -> Option<&AccountId> {
        self.accounts.selected_id()
    }

    /// The load sequence of the selected account.
    pub fn selected_provider(&self) -> Option<MailProvider> {
        let id = self.accounts.selected_id()?;
        Some(self.accounts.visible_accounts()[id].provider)
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
            AccountPage::SelectAccount => ("Select an account", ""),
            AccountPage::SelectedAccount => ("", ""),
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
            AccountPage::Loading | AccountPage::SelectAccount | AccountPage::SelectedAccount => {
                None
            }
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

    pub fn shows_account(&self, id: &AccountId) -> bool {
        self.accounts.visible_accounts().contains_key(id)
    }

    fn notify_selection_changed(&self) {
        if let Some(on_selected) = self.on_selection_changed.borrow().as_ref() {
            on_selected();
        }
    }

    pub fn apply_update(&mut self, update: &AccountUpdate) {
        let hidden_notices = self.accounts.apply_update(update);
        let removed: Vec<_> = self
            .rows
            .keys()
            .filter(|id| !self.accounts.visible_accounts().contains_key(*id))
            .cloned()
            .collect();
        let mut removed_focus = false;
        for id in removed {
            let item = self.rows.remove(&id).unwrap();
            let row = item.borrow::<AccountWidgets>();
            if contains_focus(&row.root) || contains_focus(&row.popover) {
                removed_focus = true;
            }
            row.popover.popdown();
            if let Some(position) = self.store.find(&item) {
                self.store.remove(position);
            }
        }
        for (position, (id, account)) in self.accounts.visible_accounts().iter().enumerate() {
            let item = self.rows.entry(id.clone()).or_insert_with(|| {
                let item = glib::BoxedAnyObject::new(AccountWidgets::new(id, &self.retry_check));
                self.store.insert(position as u32, &item);
                item
            });
            item.borrow::<AccountWidgets>()
                .update(account, self.accounts.retry_pending());
        }
        let selected_position = self
            .accounts
            .selected_id()
            .and_then(|id| self.store.find(&self.rows[id]));
        self.selection
            .set_selected(selected_position.unwrap_or(gtk::INVALID_LIST_POSITION));
        if removed_focus {
            focus_widget(&self.tree);
        }
        for notice in hidden_notices {
            self.show_toast(&format!(
                "{} was removed or Mail was turned off in Online Accounts.",
                notice.label
            ));
        }
    }

    pub fn show_settings_error(&self, error: LaunchError) {
        self.show_toast(error.message());
    }

    /// Shows a short notice in the window's existing toast area.
    pub fn show_toast(&self, title: &str) {
        self.toasts
            .add_toast(adw::Toast::builder().title(title).use_markup(false).build());
    }
}

fn create_row_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        // Single-click activation otherwise also selects rows on hover.
        item.set_selectable(false);
        let object = item
            .item()
            .unwrap()
            .downcast::<glib::BoxedAnyObject>()
            .unwrap();
        item.set_child(Some(&object.borrow::<AccountWidgets>().root));
    });
    factory.connect_unbind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        if let Some(object) = item.item() {
            object
                .downcast::<glib::BoxedAnyObject>()
                .unwrap()
                .borrow::<AccountWidgets>()
                .popover
                .popdown();
        }
        item.set_child(None::<&gtk::Widget>);
    });
    factory
}

impl AccountWidgets {
    fn new(id: &AccountId, retry_check: &gio::SimpleAction) -> Self {
        let builder = gtk::Builder::from_string(include_str!("../resources/ui/folder-row.ui"));
        let root: gtk::Box = builder.object("folder_row").unwrap();
        root.set_focusable(true);
        let details: adw::ActionRow = builder.object("folder_details").unwrap();
        let badge: gtk::Label = builder.object("folder_badge").unwrap();
        details.remove(&badge);
        details.set_focusable(false);
        details.add_css_class("heading");
        let problem = gtk::MenuButton::builder()
            .icon_name("dialog-warning-symbolic")
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        problem.add_css_class("flat");
        let popover = gtk::Popover::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let explanation = gtk::Label::builder()
            .wrap(true)
            .max_width_chars(30)
            .use_markup(false)
            .build();
        let retry = gtk::Button::with_label("Retry Check");
        let retry_check = retry_check.clone();
        retry.connect_clicked(move |_| retry_check.activate(None));
        content.append(&explanation);
        content.append(&retry);
        let settings = gtk::Button::with_label("Online Accounts");
        settings.set_action_name(Some("app.accounts"));
        settings.set_visible(false);
        content.append(&settings);
        popover.set_child(Some(&content));
        problem.set_popover(Some(&popover));
        details.add_suffix(&problem);
        Self {
            id: id.clone(),
            root,
            details,
            icon: builder.object("folder_icon").unwrap(),
            problem,
            popover,
            explanation,
            retry,
            settings,
        }
    }

    fn update(&self, account: &AccountRow, retry_pending: bool) {
        self.details.set_title(&account.label);
        self.icon.set_icon_name(Some(account.icon_name));
        let explanation = account
            .problems
            .iter()
            .map(problem_text)
            .collect::<Vec<_>>()
            .join("\n");
        if explanation.is_empty() {
            if contains_focus(&self.problem) || contains_focus(&self.popover) {
                focus_widget(&self.root);
            }
            self.popover.popdown();
        }
        self.problem.set_visible(!explanation.is_empty());
        self.problem.set_tooltip_text(Some(&explanation));
        self.problem
            .update_property(&[gtk::accessible::Property::Label(&explanation)]);
        self.explanation.set_text(&explanation);
        self.settings
            .set_visible(account.problems.contains(&AccountProblem::AttentionNeeded));
        self.retry.set_visible(
            account
                .problems
                .iter()
                .any(|problem| *problem != AccountProblem::AttentionNeeded),
        );
        show_check_progress(&self.retry, retry_pending);
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

fn focus_widget(widget: &impl IsA<gtk::Widget>) {
    if let Some(root) = widget.root() {
        root.set_focus(Some(widget));
    }
}

fn contains_focus(widget: &impl IsA<gtk::Widget>) -> bool {
    widget
        .root()
        .and_then(|root| root.focus())
        .is_some_and(|focus| focus == *widget.as_ref() || focus.is_ancestor(widget))
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
        ErrorCause::AccessDenied => {
            "Access to Online Accounts was denied. Check that Mailbag has permission to use Online Accounts."
        }
        ErrorCause::Timeout => "Online Accounts did not respond in time. Try checking again.",
        ErrorCause::InvalidReply => {
            "Online Accounts returned an incomplete or invalid account list. Try checking again."
        }
    }
}
