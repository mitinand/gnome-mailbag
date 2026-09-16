// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::GoaAdapter;
use crate::{AccountId, AccountProvider, AccountUpdate};
use crate::{ErrorCause, test_bus::TestBus, test_goa::*};
use gio::prelude::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub(super) fn run_in_context(test: impl FnOnce()) {
    glib::MainContext::new().with_thread_default(test).unwrap();
}

pub(super) fn wait_until(mut condition: impl FnMut() -> bool) {
    glib::MainContext::ref_thread_default().block_on(async {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(Instant::now() < deadline, "account update deadline");
            glib::timeout_future(Duration::from_millis(1)).await;
        }
    });
}

pub(super) fn dispatch_for(duration: Duration) {
    glib::MainContext::ref_thread_default().block_on(glib::timeout_future(duration));
}

pub(super) struct RecordedUpdates(Rc<RefCell<VecDeque<AccountUpdate>>>);
impl RecordedUpdates {
    pub fn next(&self) -> AccountUpdate {
        wait_until(|| !self.0.borrow().is_empty());
        self.0.borrow_mut().pop_front().unwrap()
    }
    pub fn completed(&self) -> AccountUpdate {
        loop {
            let update = self.next();
            if !update.retry_pending {
                return update;
            }
        }
    }
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}

pub(super) fn start_test_client(bus: &TestBus) -> (GoaAdapter, RecordedUpdates) {
    let updates = Rc::new(RefCell::new(VecDeque::new()));
    let recorded = updates.clone();
    let context = glib::MainContext::ref_thread_default();
    let client = GoaAdapter::start_for_test(bus.address.clone(), 400, move |update| {
        assert!(
            context.is_owner(),
            "updates must run on the subscribing context"
        );
        recorded.borrow_mut().push_back(update.clone());
    });
    (client, RecordedUpdates(updates))
}

#[test]
fn startup_reads_accounts_and_recognizes_providers() {
    run_in_context(|| {
        let bus = TestBus::new();
        let providers = [
            ("imap_smtp", AccountProvider::ImapSmtp),
            ("google", AccountProvider::Google),
            ("ms_graph", AccountProvider::Microsoft365),
            ("exchange", AccountProvider::Other),
        ];
        let reply = make_account_reply(
            providers
                .iter()
                .map(|(name, _)| {
                    let mut account = make_account(name);
                    account
                        .get_mut(ACCOUNT_INTERFACE)
                        .unwrap()
                        .insert("ProviderType".into(), name.to_variant());
                    account
                })
                .collect(),
        );
        let goa = FakeGoaService::new(&bus.address, ReplyBehavior::Value(reply));
        let (_client, updates) = start_test_client(&bus);
        let update = updates.completed();
        assert!(update.last_check.is_complete());
        assert_eq!(update.accounts.len(), providers.len());
        for (name, expected) in providers {
            assert_eq!(
                update.accounts[&AccountId::try_from(name).unwrap()].provider,
                expected
            );
        }
        assert_eq!(goa.read_count(), 1);
    });
}

#[test]
fn absent_goa_reports_a_read_error_without_accounts() {
    run_in_context(|| {
        let bus = TestBus::new();
        let (_client, updates) = start_test_client(&bus);
        let update = updates.completed();
        assert_eq!(
            update.last_check.error().unwrap().cause,
            ErrorCause::Unavailable
        );
        assert!(update.accounts.is_empty());
    });
}

#[test]
fn failed_reads_keep_accounts_and_retry_keeps_the_error_until_completion() {
    for (response, cause) in [
        (ReplyBehavior::Hang, ErrorCause::Timeout),
        (ReplyBehavior::WrongType, ErrorCause::InvalidReply),
        (ReplyBehavior::AccessDenied, ErrorCause::AccessDenied),
    ] {
        run_in_context(|| {
            let bus = TestBus::new();
            let reply = make_account_reply(vec![make_account("one")]);
            let goa = FakeGoaService::new(&bus.address, ReplyBehavior::Value(reply.clone()));
            let (client, updates) = start_test_client(&bus);
            let accepted = updates.completed();
            goa.set_reply(response);
            client.refresh_accounts();
            assert!(updates.next().retry_pending);
            let failed = updates.completed();
            assert_eq!(failed.accounts, accepted.accounts);
            assert_eq!(failed.last_check.error().unwrap().cause, cause);
            assert!(!format!("{failed:?}").contains("synthetic-private-detail"));
            goa.set_reply(ReplyBehavior::Value(reply));
            client.refresh_accounts();
            let pending = updates.next();
            assert!(pending.retry_pending);
            assert_eq!(pending.last_check, failed.last_check);
            assert_eq!(pending.accounts, accepted.accounts);
            assert!(updates.completed().last_check.is_complete());
        });
    }
}

#[test]
fn rejected_record_keeps_the_entire_previous_list() {
    run_in_context(|| {
        let bus = TestBus::new();
        let goa = FakeGoaService::new(
            &bus.address,
            ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
        );
        let (client, updates) = start_test_client(&bus);
        let accepted = updates.completed();
        let mut invalid = make_account("two");
        invalid
            .get_mut(ACCOUNT_INTERFACE)
            .unwrap()
            .remove("MailDisabled");
        let mut renamed = make_account("one");
        renamed.get_mut(ACCOUNT_INTERFACE).unwrap().insert(
            "PresentationIdentity".into(),
            "Must not be applied".to_variant(),
        );
        goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![
            renamed, invalid,
        ])));
        client.refresh_accounts();
        let rejected = updates.completed();
        assert_eq!(rejected.accounts, accepted.accounts);
        assert_eq!(
            rejected.last_check.error().unwrap().cause,
            ErrorCause::InvalidReply
        );
    });
}

#[test]
fn stop_and_last_handle_drop_cancel_reads_and_prevent_late_callbacks() {
    for explicit_stop in [false, true] {
        run_in_context(|| {
            let bus = TestBus::new();
            let goa = FakeGoaService::new(&bus.address, ReplyBehavior::Hang);
            let (client, updates) = start_test_client(&bus);
            wait_until(|| goa.read_count() == 1);
            let clone = client.clone();
            drop(client);
            if explicit_stop {
                clone.stop();
                clone.stop();
                clone.refresh_accounts();
            } else {
                drop(clone);
            }
            goa.complete_held_reply(&make_account_reply(vec![make_account("late")]));
            goa.change_properties(
                ACCOUNT_INTERFACE,
                BTreeMap::new(),
                vec!["PresentationIdentity".into()],
            );
            dispatch_for(Duration::from_millis(40));
            assert!(updates.is_empty());
            assert_eq!(goa.read_count(), 1);
        });
    }
}
