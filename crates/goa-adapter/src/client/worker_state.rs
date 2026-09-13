// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{
    AccountCheckError, AccountCheckResult, AccountId, AccountUpdate, ErrorCause, SharedClientState,
    accounts::{
        ACCOUNT_INTERFACE, AccountSnapshot, GOA_ROOT_PATH, MAIL_INTERFACE,
        apply_account_properties, has_account_properties, validate_account_limits,
    },
};
use glib::Variant;
use std::{collections::BTreeMap, sync::Arc, task::Waker};

/// Accepted account facts and request scheduling, accessed only by the worker.
pub(super) struct GoaWorkerState {
    pub account_list: AccountUpdate,
    pub account_paths: BTreeMap<String, AccountId>,
    pub goa_owner: Option<String>,
    pub account_change_number: u64,
    pub recheck_requested: bool,
    pub worker_waker: Option<Waker>,
    shared: Arc<SharedClientState>,
}
impl GoaWorkerState {
    pub fn new(shared: Arc<SharedClientState>) -> Self {
        Self {
            account_list: AccountUpdate::default(),
            account_paths: BTreeMap::new(),
            goa_owner: None,
            account_change_number: 0,
            recheck_requested: false,
            worker_waker: None,
            shared,
        }
    }
    pub fn begin_check(&mut self) {
        self.account_list.check_pending = true;
        self.recheck_requested = false;
        self.publish_accounts();
    }
    pub fn finish_check(&mut self, result: Result<AccountSnapshot, AccountCheckError>) {
        if let Err(error) = result.and_then(|snapshot| self.accept_snapshot(snapshot)) {
            self.account_paths.clear();
            self.account_list.last_check = AccountCheckResult::Failed(error);
        }
        self.account_list.check_pending = false;
        self.recheck_requested = false;
        self.shared.lock().refresh_requested = false;
        self.publish_accounts();
    }

    /// This is the only place where a decoded reply is combined with previous facts.
    fn accept_snapshot(&mut self, snapshot: AccountSnapshot) -> Result<(), AccountCheckError> {
        let AccountSnapshot {
            accounts,
            account_paths,
            list_error,
        } = snapshot;
        let accepted_accounts = if list_error.is_some() {
            let mut combined_accounts = self.account_list.accounts.clone();
            combined_accounts.extend(accounts);
            validate_account_limits(combined_accounts.iter(), &account_paths)?;
            combined_accounts
        } else {
            accounts
        };
        self.account_list.accounts = accepted_accounts;
        self.account_list.last_check =
            list_error.map_or(AccountCheckResult::Complete, AccountCheckResult::Failed);
        self.account_paths = account_paths;
        Ok(())
    }
    pub fn publish_accounts(&self) {
        self.shared.publish_update(self.account_list.clone());
    }
    pub fn report_failure(&mut self, operation: &'static str, cause: ErrorCause) {
        self.account_list.last_check =
            AccountCheckResult::Failed(AccountCheckError::new(operation, cause));
        self.publish_accounts();
    }
    pub fn request_recheck(&mut self) {
        self.recheck_requested = true;
        if let Some(waker) = self.worker_waker.take() {
            waker.wake();
        }
    }
    pub fn record_owner_change(&mut self, new_goa_owner: &str) {
        self.account_change_number += 1;
        self.goa_owner = (!new_goa_owner.is_empty()).then(|| new_goa_owner.to_owned());
        self.account_paths.clear();
        self.report_failure("GOA process changed", ErrorCause::Unavailable);
        self.request_recheck();
    }
    pub fn apply_account_signal(&mut self, sender: &str, path: &str, member: &str, body: &Variant) {
        if self.goa_owner.as_deref() != Some(sender)
            || (!path.starts_with("/org/gnome/OnlineAccounts/") && path != GOA_ROOT_PATH)
        {
            return;
        }
        match member {
            "PropertiesChanged" if body.type_().as_str() == "(sa{sv}as)" => {
                let interface_value = body.child_value(0);
                let interface = interface_value.str().expect("validated interface name");
                let changed = body.child_value(1);
                let invalidated = body.child_value(2);
                if !has_account_properties(interface, &changed, &invalidated) {
                    return;
                }
                self.account_change_number += 1;
                let identity_changed = interface == ACCOUNT_INTERFACE
                    && (changed
                        .iter()
                        .any(|entry| entry.child_value(0).str() == Some("Id"))
                        || invalidated.iter().any(|field| field.str() == Some("Id")));
                if identity_changed {
                    self.account_paths.remove(path);
                    self.report_failure("account identity changed", ErrorCause::InvalidList);
                    self.request_recheck();
                    return;
                }
                let Some(id) = self.account_paths.get(path) else {
                    self.request_recheck();
                    return;
                };
                let Some(previous) = self.account_list.accounts.get(id) else {
                    self.request_recheck();
                    return;
                };
                let mut candidate = previous.clone();
                apply_account_properties(&mut candidate, interface, &changed, &invalidated);
                let recheck_needed = !candidate.invalid_fields().is_empty();
                let candidate_accounts =
                    self.account_list
                        .accounts
                        .iter()
                        .map(|(account_id, details)| {
                            (
                                account_id,
                                if account_id == id {
                                    &candidate
                                } else {
                                    details
                                },
                            )
                        });
                if validate_account_limits(candidate_accounts, &self.account_paths).is_err() {
                    // Optional display text can be omitted. Account flags still apply,
                    // and AccountList retains its usable display while the check is failed.
                    candidate.provider_name = None;
                    candidate.display_name = None;
                    candidate.email_address = None;
                    candidate.icon_name = None;
                    self.account_list.accounts.insert(id.clone(), candidate);
                    self.report_failure("account property update", ErrorCause::DataLimit);
                    self.request_recheck();
                    return;
                }
                if &candidate != previous {
                    self.account_list.accounts.insert(id.clone(), candidate);
                    self.publish_accounts();
                }
                if recheck_needed {
                    self.request_recheck();
                }
            }
            "InterfacesRemoved" if body.type_().as_str() == "(oas)" => {
                let object_path = body.child_value(0);
                let account_path = object_path.str().expect("validated object path");
                let removed = body.child_value(1);
                let account_removed = removed
                    .iter()
                    .any(|interface| interface.str() == Some(ACCOUNT_INTERFACE));
                let mail_removed = removed
                    .iter()
                    .any(|interface| interface.str() == Some(MAIL_INTERFACE));
                if !account_path.starts_with("/org/gnome/OnlineAccounts/")
                    || !(account_removed || mail_removed)
                {
                    return;
                }
                self.account_change_number += 1;
                let mail_details_changed = if mail_removed
                    && let Some(id) = self.account_paths.get(account_path)
                    && let Some(account) = self.account_list.accounts.get_mut(id)
                    && (account.mail_service_available || account.email_address.is_some())
                {
                    account.mail_service_available = false;
                    account.email_address = None;
                    true
                } else {
                    false
                };
                if account_removed && self.account_paths.remove(account_path).is_some() {
                    self.report_failure("account interface removed", ErrorCause::InvalidList);
                } else if mail_details_changed {
                    self.publish_accounts();
                }
                self.request_recheck();
            }
            "InterfacesAdded" if body.type_().as_str() == "(oa{sa{sv}})" => {
                let interfaces = body.child_value(1);
                if !body
                    .child_value(0)
                    .str()
                    .expect("validated object path")
                    .starts_with("/org/gnome/OnlineAccounts/")
                    || !interfaces.iter().any(|entry| {
                        matches!(
                            entry.child_value(0).str(),
                            Some(ACCOUNT_INTERFACE | MAIL_INTERFACE)
                        )
                    })
                {
                    return;
                }
                self.account_change_number += 1;
                self.request_recheck();
            }
            _ => {
                self.account_change_number += 1;
                self.report_failure("GOA account signal", ErrorCause::InvalidReply);
                self.request_recheck();
            }
        }
    }
}
