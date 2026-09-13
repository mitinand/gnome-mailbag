// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{
    AccountCheckError, AccountId, AccountUpdate, CheckStatus, ErrorCause, SharedClientState,
    accounts::{
        ACCOUNT_INTERFACE, AccountSnapshot, GOA_ROOT_PATH, MAIL_INTERFACE,
        apply_account_properties, fits_account_limits, merge_account_facts,
    },
};
use glib::Variant;
use std::{collections::BTreeMap, sync::Arc, task::Waker};

/// Account data and pending checks, accessed only by the GOA worker thread.
pub(super) struct GoaWorkerState {
    pub account_list: AccountUpdate,
    pub account_paths: BTreeMap<String, AccountId>,
    pub goa_owner: Option<String>,
    pub account_change_number: u64,
    pub recheck_requested: bool,
    pub owner_appeared: bool,
    pub check_pending: bool,
    pub health_check_due: bool,
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
            owner_appeared: false,
            check_pending: false,
            health_check_due: false,
            worker_waker: None,
            shared,
        }
    }
    pub fn begin_check(&mut self, show_checking: bool) {
        self.check_pending = true;
        self.recheck_requested = false;
        self.owner_appeared = false;
        if show_checking {
            self.account_list.status = CheckStatus::Checking;
            self.account_list.membership_confirmed = false;
            self.publish_accounts();
        }
    }

    /// Update the account list and its D-Bus paths together, then notify the caller.
    pub fn finish_check(&mut self, result: Result<AccountSnapshot, AccountCheckError>) {
        let accepted = result.and_then(|snapshot| self.accept_snapshot(snapshot));
        if let Err(error) = accepted {
            self.account_list.status = CheckStatus::Failed;
            self.account_list.membership_confirmed = false;
            self.account_list.error = Some(error);
        }
        self.check_pending = false;
        self.recheck_requested = false;
        self.shared.lock().refresh_requested = false;
        self.publish_accounts();
    }

    fn accept_snapshot(&mut self, snapshot: AccountSnapshot) -> Result<(), AccountCheckError> {
        let AccountSnapshot {
            account_list: mut checked_list,
            account_paths,
        } = snapshot;
        self.owner_appeared = false;
        checked_list.update_number = self.account_list.update_number;
        if checked_list.membership_confirmed {
            self.account_list = checked_list;
        } else if let Err(error) =
            merge_account_facts(&mut self.account_list, checked_list, &account_paths)
        {
            // Keep previous accounts if the merged list is too large. Remove path
            // mappings that the reply shows are conflicting or belong to another ID.
            self.account_paths
                .retain(|path, id| account_paths.get(path) == Some(id));
            return Err(error);
        }
        self.account_paths = account_paths;
        Ok(())
    }

    pub fn publish_accounts(&mut self) {
        self.account_list.update_number += 1;
        self.shared.publish_update(self.account_list.clone());
    }
    pub fn report_failure(&mut self, operation: &'static str, cause: ErrorCause) {
        self.account_list.status = CheckStatus::Failed;
        self.account_list.membership_confirmed = false;
        self.account_list.error = Some(AccountCheckError::new(operation, cause));
        self.publish_accounts();
    }
    pub fn wake_worker(&mut self) {
        if let Some(waker) = self.worker_waker.take() {
            waker.wake();
        }
    }
    pub fn request_recheck(&mut self) {
        self.recheck_requested = true;
        self.wake_worker();
    }
    pub fn record_owner_change(&mut self, new_goa_owner: &str) {
        self.account_change_number += 1;
        self.goa_owner = (!new_goa_owner.is_empty()).then(|| new_goa_owner.to_owned());
        self.account_paths.clear();
        self.owner_appeared = !new_goa_owner.is_empty();
        self.report_failure("GOA process changed", ErrorCause::Unavailable);
        self.request_recheck();
    }
    pub fn apply_account_signal(&mut self, sender: &str, path: &str, member: &str, body: &Variant) {
        if self.goa_owner.as_deref() != Some(sender) {
            return;
        }
        if !path.starts_with(&format!("{GOA_ROOT_PATH}/")) && path != GOA_ROOT_PATH {
            return;
        }
        match member {
            "PropertiesChanged" if body.type_().as_str() == "(sa{sv}as)" => {
                let interface_value = body.child_value(0);
                let interface = interface_value.str().expect("validated interface name");
                if ![ACCOUNT_INTERFACE, MAIL_INTERFACE].contains(&interface) {
                    return;
                }
                self.account_change_number += 1;
                let changed_properties = body.child_value(1);
                let invalidated_properties = body.child_value(2);
                let identity_changed = interface == ACCOUNT_INTERFACE
                    && (changed_properties
                        .iter()
                        .any(|entry| entry.child_value(0).str() == Some("Id"))
                        || invalidated_properties
                            .iter()
                            .any(|field| field.str() == Some("Id")));
                if identity_changed {
                    self.report_failure("account identity changed", ErrorCause::InvalidList);
                }
                let Some(id) = self.account_paths.get(path) else {
                    self.request_recheck();
                    return;
                };
                let Some(account) = self.account_list.accounts.get_mut(id) else {
                    self.request_recheck();
                    return;
                };
                let mut previous_details = account.clone();
                let recheck_needed = apply_account_properties(
                    account,
                    interface,
                    &changed_properties,
                    &invalidated_properties,
                );
                if !fits_account_limits(&self.account_list, &self.account_paths) {
                    let updated_details = &self.account_list.accounts[id];
                    // Even if the new strings are too large, apply MailDisabled and
                    // AttentionNeeded changes and clear fields GOA marked as unknown.
                    previous_details.mail_enabled = updated_details.mail_enabled;
                    previous_details.needs_attention = updated_details.needs_attention;
                    previous_details.provider = updated_details.provider;
                    for (old_text, new_text) in [
                        (
                            &mut previous_details.provider_name,
                            &updated_details.provider_name,
                        ),
                        (
                            &mut previous_details.display_name,
                            &updated_details.display_name,
                        ),
                        (
                            &mut previous_details.email_address,
                            &updated_details.email_address,
                        ),
                        (&mut previous_details.icon_name, &updated_details.icon_name),
                    ] {
                        if new_text.is_none() {
                            *old_text = None;
                        }
                    }
                    self.account_list
                        .accounts
                        .insert(id.clone(), previous_details);
                    self.report_failure("account property update", ErrorCause::DataLimit);
                    self.request_recheck();
                    return;
                }
                self.publish_accounts();
                if recheck_needed || identity_changed {
                    self.request_recheck();
                }
            }
            "InterfacesRemoved" if body.type_().as_str() == "(oas)" => {
                let object_path = body.child_value(0);
                if !object_path
                    .str()
                    .expect("validated object path")
                    .starts_with(&format!("{GOA_ROOT_PATH}/"))
                {
                    return;
                }
                self.account_change_number += 1;
                if let Some(id) = self
                    .account_paths
                    .get(object_path.str().expect("validated object path"))
                    && body
                        .child_value(1)
                        .iter()
                        .any(|interface| interface.str() == Some(MAIL_INTERFACE))
                    && let Some(account) = self.account_list.accounts.get_mut(id)
                {
                    account.mail_service_available = false;
                    account.email_address = None;
                    self.publish_accounts();
                }
                self.request_recheck();
            }
            "InterfacesAdded" if body.type_().as_str() == "(oa{sa{sv}})" => {
                if !body
                    .child_value(0)
                    .str()
                    .expect("validated object path")
                    .starts_with(&format!("{GOA_ROOT_PATH}/"))
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
