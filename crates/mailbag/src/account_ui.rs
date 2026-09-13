// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::accounts::{
    AccountHiddenNotice, AccountList, AccountPage, AccountProblem, ExclusionReason,
};
use adw::{gio, glib, gtk, prelude::*};
use goa_adapter::{AccountField, AccountId, AccountUpdate, ErrorCause};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

#[cfg(test)]
mod tests;

struct AccountWidgets {
    root: gtk::Box,
    details: adw::ActionRow,
    icon: gtk::Image,
    problem: gtk::MenuButton,
    popover: gtk::Popover,
    explanation: gtk::Label,
    retry: gtk::Button,
    settings: gtk::Button,
    item: glib::BoxedAnyObject,
}

pub struct AccountUi {
    accounts: AccountList,
    settings_error: Option<crate::settings::LaunchError>,
    settings_pending: bool,
    rows: BTreeMap<AccountId, AccountWidgets>,
    store: gio::ListStore,
    selection: gtk::SingleSelection,
    reconciling: Rc<Cell<bool>>,
    tree: gtk::ListView,
    status: adw::StatusPage,
    retry: gtk::Button,
    online_accounts: gtk::Button,
    mail_split: adw::NavigationSplitView,
    folders_split: adw::OverlaySplitView,
    retry_check: Rc<dyn Fn()>,
    notices: Rc<RefCell<AccountNotices>>,
}

impl AccountUi {
    pub fn new(builder: &gtk::Builder, retry_check: impl Fn() + 'static) -> Rc<RefCell<Self>> {
        let tree: gtk::ListView = builder.object("folder_tree").expect("folder_tree");
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::new(Some(store.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        tree.set_model(Some(&selection));
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
            let object = item
                .item()
                .unwrap()
                .downcast::<glib::BoxedAnyObject>()
                .unwrap();
            let (_, root, _) = &*object.borrow::<(AccountId, gtk::Box, gtk::Popover)>();
            item.set_child(Some(root));
        });
        factory.connect_unbind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            if let Some(object) = item.item() {
                object
                    .downcast::<glib::BoxedAnyObject>()
                    .unwrap()
                    .borrow::<(AccountId, gtk::Box, gtk::Popover)>()
                    .2
                    .popdown();
            }
            item.set_child(None::<&gtk::Widget>);
        });
        tree.set_factory(Some(&factory));
        let status: adw::StatusPage = builder.object("account_status").expect("account_status");
        let retry = gtk::Button::with_label("Retry Check");
        let online_accounts = gtk::Button::with_label("Online Accounts");
        online_accounts.set_action_name(Some("app.accounts"));
        let actions = gtk::Box::new(gtk::Orientation::Vertical, 0);
        actions.set_halign(gtk::Align::Center);
        actions.append(&retry);
        actions.append(&online_accounts);
        status.set_child(Some(&actions));
        let retry_check: Rc<dyn Fn()> = Rc::new(retry_check);
        let retry_callback = retry_check.clone();
        retry.connect_clicked(move |_| retry_callback());
        let reconciling = Rc::new(Cell::new(false));
        let ui = Rc::new(RefCell::new(Self {
            accounts: AccountList::default(),
            settings_error: None,
            settings_pending: false,
            rows: BTreeMap::new(),
            store,
            selection: selection.clone(),
            reconciling: reconciling.clone(),
            tree: tree.clone(),
            status,
            retry,
            online_accounts,
            mail_split: builder.object("mail_split").expect("mail_split"),
            folders_split: builder.object("folders_split").expect("folders_split"),
            retry_check,
            notices: AccountNotices::new(builder.object("toasts").expect("toasts")),
        }));
        let weak = Rc::downgrade(&ui);
        selection.connect_selected_notify(move |selection| {
            if reconciling.get() {
                return;
            }
            if let Some(ui) = weak.upgrade() {
                let id = selection.selected_item().map(|item| {
                    item.downcast::<glib::BoxedAnyObject>()
                        .unwrap()
                        .borrow::<(AccountId, gtk::Box, gtk::Popover)>()
                        .0
                        .clone()
                });
                let mut ui = ui.borrow_mut();
                ui.accounts.select_account(id);
                ui.show_status();
            }
        });
        let weak = Rc::downgrade(&ui);
        tree.connect_activate(move |_, position| {
            if let Some(ui) = weak.upgrade() {
                let selection = ui.borrow().selection.clone();
                selection.set_selected(position);
                let ui = ui.borrow();
                ui.mail_split.set_show_content(false);
                if ui.folders_split.is_collapsed() {
                    ui.folders_split.set_show_sidebar(false);
                }
            }
        });
        ui.borrow().mail_split.set_show_content(false);
        ui.borrow().show_status();
        ui
    }

    pub fn apply_update(&mut self, update: &AccountUpdate) {
        let notice = self.accounts.apply_update(update);
        self.reconciling.set(true);
        let removed: Vec<_> = self
            .rows
            .keys()
            .filter(|id| !self.accounts.visible_accounts().contains_key(*id))
            .cloned()
            .collect();
        let mut removed_focus = false;
        for id in removed {
            let row = self.rows.remove(&id).unwrap();
            if contains_focus(&row.root) || contains_focus(&row.popover) {
                removed_focus = true;
            }
            row.popover.popdown();
            if let Some(position) = self.store.find(&row.item) {
                self.store.remove(position);
            }
        }
        for (id, account) in self.accounts.visible_accounts() {
            let row = self.rows.entry(id.clone()).or_insert_with(|| {
                let row = AccountWidgets::new(id, self.retry_check.clone());
                self.store.append(&row.item);
                row
            });
            row.details.set_title(&account.label);
            row.details.set_subtitle(&match &account.email_address {
                Some(email) => format!("{} · {email}", account.provider_name),
                None => account.provider_name.clone(),
            });
            row.icon.set_icon_name(Some(&account.icon_name));
            let explanation = account
                .problems
                .iter()
                .map(problem_text)
                .collect::<Vec<_>>()
                .join("\n");
            if explanation.is_empty() {
                if contains_focus(&row.problem) || contains_focus(&row.popover) {
                    focus_widget(&row.root);
                }
                row.popover.popdown();
            }
            row.problem.set_visible(!explanation.is_empty());
            row.problem.set_tooltip_text(Some(&explanation));
            row.problem
                .update_property(&[gtk::accessible::Property::Label(&explanation)]);
            row.explanation.set_text(&explanation);
            row.settings
                .set_visible(account.problems.contains(&AccountProblem::AttentionNeeded));
            row.retry
                .set_visible(account.problems.iter().any(|problem| {
                    !matches!(
                        problem,
                        AccountProblem::AttentionNeeded | AccountProblem::UnsupportedProvider
                    )
                }));
            if self
                .accounts
                .check_error()
                .is_some_and(|error| error.cause == ErrorCause::SourceStopped)
            {
                row.retry.set_visible(false);
            }
            row.retry.set_sensitive(!self.accounts.check_pending());
            row.retry.set_label(if self.accounts.check_pending() {
                "Checking…"
            } else {
                "Retry Check"
            });
        }
        let selected_position = self
            .accounts
            .selected_id()
            .and_then(|id| self.store.find(&self.rows[id].item));
        self.selection
            .set_selected(selected_position.unwrap_or(gtk::INVALID_LIST_POSITION));
        self.reconciling.set(false);
        self.show_status();
        if removed_focus {
            focus_widget(&self.tree);
        }
        if let Some(notice) = notice {
            AccountNotices::push(&self.notices, notice);
        }
    }

    pub fn show_settings_result(
        &mut self,
        pending: bool,
        error: Option<crate::settings::LaunchError>,
    ) {
        self.settings_pending = pending;
        self.settings_error = error;
        self.show_status();
        if let Some(error) = error {
            AccountNotices::show_settings_error(&self.notices, error);
        }
    }

    fn show_status(&self) {
        let (title, description) = if let Some(error) = self.accounts.check_error() {
            ("Unable to get accounts", check_error_text(error.cause))
        } else {
            match self.accounts.page() {
                AccountPage::Checking => ("Checking accounts", ""),
                AccountPage::Unavailable => ("Unable to get accounts", "Try checking again."),
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
        };
        let description = match self.settings_error {
            Some(error) if description.is_empty() => error.message().to_owned(),
            Some(error) => format!("{description}\n{}", error.message()),
            None => description.to_owned(),
        };
        let title = if title.is_empty() && self.settings_error.is_some() {
            "Could not open Online Accounts"
        } else {
            title
        };
        self.status.set_visible(!title.is_empty());
        self.status.set_title(title);
        self.status.set_description(Some(&description));
        self.retry.set_visible(
            self.accounts
                .check_error()
                .is_some_and(|error| error.cause != ErrorCause::SourceStopped),
        );
        self.retry.set_sensitive(!self.accounts.check_pending());
        self.retry.set_label(if self.accounts.check_pending() {
            "Checking…"
        } else {
            "Retry Check"
        });
        self.online_accounts.set_visible(
            matches!(
                self.accounts.page(),
                AccountPage::NoAccounts | AccountPage::NoEligibleAccounts
            ) || self.settings_error.is_some(),
        );
        self.online_accounts.set_sensitive(!self.settings_pending);
    }
}

impl AccountWidgets {
    fn new(id: &AccountId, retry_check: Rc<dyn Fn()>) -> Self {
        let builder = gtk::Builder::from_string(include_str!("../resources/ui/folder-row.ui"));
        let root: gtk::Box = builder.object("folder_row").unwrap();
        root.set_focusable(true);
        let details: adw::ActionRow = builder.object("folder_details").unwrap();
        let badge: gtk::Label = builder.object("folder_badge").unwrap();
        details.remove(&badge);
        details.set_focusable(false);
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
        retry.connect_clicked(move |_| retry_check());
        content.append(&explanation);
        content.append(&retry);
        let settings = gtk::Button::with_label("Online Accounts");
        settings.set_action_name(Some("app.accounts"));
        settings.set_visible(false);
        content.append(&settings);
        popover.set_child(Some(&content));
        problem.set_popover(Some(&popover));
        details.add_suffix(&problem);
        let item = glib::BoxedAnyObject::new((id.clone(), root.clone(), popover.clone()));
        Self {
            root,
            details,
            icon: builder.object("folder_icon").unwrap(),
            problem,
            popover,
            explanation,
            retry,
            settings,
            item,
        }
    }
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
        AccountProblem::InvalidField(AccountField::Provider) => {
            "Online Accounts did not provide the account type. Try checking again."
        }
        AccountProblem::InvalidField(AccountField::MailEnabled) => {
            "Online Accounts did not report whether Mail is enabled. Try checking again."
        }
        AccountProblem::InvalidField(AccountField::Attention) => {
            "Online Accounts did not report whether this account needs attention. Try checking again."
        }
        AccountProblem::MailUnavailable => {
            "Unable to get this account's mail settings. Try checking again."
        }
        AccountProblem::UnsupportedProvider => "This account provider is not supported.",
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
        ErrorCause::InvalidReply | ErrorCause::InvalidList => {
            "Online Accounts returned an incomplete or invalid account list. Try checking again."
        }
        ErrorCause::DataLimit => "The account list is too large for Mailbag to process.",
        ErrorCause::SourceStopped => "Account updates stopped. Restart Mailbag to try again.",
    }
}

/// One visible notice and one pending aggregate; no per-update toast queue.
struct AccountNotices {
    overlay: adw::ToastOverlay,
    active: Option<adw::Toast>,
    pending: Option<AccountHiddenNotice>,
}
impl AccountNotices {
    fn show_settings_error(owner: &Rc<RefCell<Self>>, error: crate::settings::LaunchError) {
        let active = owner.borrow().active.clone();
        if let Some(toast) = active {
            toast.set_title(error.message());
        } else {
            Self::show_toast(owner, error.message().to_owned());
        }
    }

    fn new(overlay: adw::ToastOverlay) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            overlay,
            active: None,
            pending: None,
        }))
    }
    fn push(owner: &Rc<RefCell<Self>>, notice: AccountHiddenNotice) {
        let mut notices = owner.borrow_mut();
        if notices.active.is_some() {
            notices.pending = Some(match notices.pending.take() {
                None => notice,
                Some(previous) => {
                    AccountHiddenNotice::Group(notice_count(&previous) + notice_count(&notice))
                }
            });
            return;
        }
        let title = match notice {
            AccountHiddenNotice::Single(label) => {
                format!("{label} was removed or Mail was turned off in Online Accounts.")
            }
            AccountHiddenNotice::Group(count) => {
                format!("{count} accounts were removed or had Mail turned off in Online Accounts.")
            }
        };
        drop(notices);
        Self::show_toast(owner, title);
    }
    fn show_toast(owner: &Rc<RefCell<Self>>, title: String) {
        let mut notices = owner.borrow_mut();
        let toast = adw::Toast::builder().title(title).use_markup(false).build();
        let weak = Rc::downgrade(owner);
        toast.connect_dismissed(move |_| {
            if let Some(owner) = weak.upgrade() {
                let pending = {
                    let mut notices = owner.borrow_mut();
                    notices.active = None;
                    notices.pending.take()
                };
                if let Some(pending) = pending {
                    Self::push(&owner, pending);
                }
            }
        });
        notices.active = Some(toast.clone());
        let overlay = notices.overlay.clone();
        drop(notices);
        overlay.add_toast(toast);
    }
}
fn notice_count(notice: &AccountHiddenNotice) -> usize {
    match notice {
        AccountHiddenNotice::Single(_) => 1,
        AccountHiddenNotice::Group(count) => *count,
    }
}
