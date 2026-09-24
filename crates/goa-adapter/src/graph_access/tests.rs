// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::GraphAccess;
use crate::client::tests::{
    RecordedUpdates, dispatch_for, run_in_context, start_test_client, wait_until,
};
use crate::{AccessError, AccessRequest, AccountId, GoaAdapter, test_bus::TestBus, test_goa::*};
use std::{cell::RefCell, rc::Rc, time::Duration};

type AccessResults = Rc<RefCell<Vec<Result<GraphAccess, AccessError>>>>;

/// Starts observation and waits for its first read, so its connection exists.
fn start_observing(bus: &TestBus) -> (GoaAdapter, RecordedUpdates) {
    let (client, updates) = start_test_client(bus);
    updates.completed();
    (client, updates)
}

fn start_goa(bus: &TestBus, accounts: Vec<Interfaces>) -> FakeGoaService {
    FakeGoaService::new(
        &bus.address,
        ReplyBehavior::Value(make_account_reply(accounts)),
    )
}

fn request_access(client: &GoaAdapter, account_id: &str) -> (AccessRequest, AccessResults) {
    let results = AccessResults::default();
    let recorded = results.clone();
    let context = glib::MainContext::ref_thread_default();
    let request =
        client.request_graph_access(&AccountId::try_from(account_id).unwrap(), move |result| {
            assert!(
                context.is_owner(),
                "completion must run on the adapter's context"
            );
            recorded.borrow_mut().push(result);
        });
    (request, results)
}

/// Waits for the completion, then checks that no second one follows.
fn completed_access(results: &AccessResults) -> Result<GraphAccess, AccessError> {
    wait_until(|| !results.borrow().is_empty());
    dispatch_for(Duration::from_millis(50));
    assert_eq!(results.borrow().len(), 1, "exactly one completion");
    results.borrow_mut().pop().unwrap()
}

fn failed_access(results: &AccessResults) -> AccessError {
    match completed_access(results) {
        Ok(_) => panic!("access unexpectedly succeeded"),
        Err(error) => error,
    }
}

#[test]
fn a_microsoft_365_account_gives_its_access_token_without_mail_settings() {
    run_in_context(|| {
        let bus = TestBus::new();
        let goa = start_goa(
            &bus,
            vec![make_account("first"), make_microsoft365_account("second")],
        );
        let (client, _updates) = start_observing(&bus);
        let (_request, results) = request_access(&client, "second");
        let access =
            completed_access(&results).unwrap_or_else(|error| panic!("access failed: {error:?}"));
        assert_eq!(access.account_id, AccountId::try_from("second").unwrap());
        assert_eq!(access.access_token, SYNTHETIC_ACCESS_TOKEN);
        assert_eq!(
            goa.access_token_requests(),
            [AccessTokenRequest {
                object_path: account_object_path(1),
            }]
        );
        assert!(goa.password_requests().is_empty());
    });
}

#[test]
fn an_account_that_is_not_listed_fails_as_settings_before_any_credential() {
    run_in_context(|| {
        let bus = TestBus::new();
        let goa = start_goa(&bus, vec![make_microsoft365_account("other")]);
        let (client, _updates) = start_observing(&bus);
        let (_request, results) = request_access(&client, "one");
        assert_eq!(failed_access(&results), AccessError::Settings);
        assert!(goa.access_token_requests().is_empty());
    });
}

#[test]
fn an_object_without_oauth2_fails_as_settings() {
    run_in_context(|| {
        let bus = TestBus::new();
        let goa = start_goa(&bus, vec![make_account("password-account")]);
        let (client, _updates) = start_observing(&bus);
        let (_request, results) = request_access(&client, "password-account");
        assert_eq!(failed_access(&results), AccessError::Settings);
        assert!(goa.access_token_requests().is_empty());
        assert!(goa.password_requests().is_empty());
    });
}

#[test]
fn a_refused_or_held_access_token_is_reported_for_its_step() {
    for (token_reply, expected) in [
        (ReplyBehavior::AccessDenied, AccessError::AccessToken),
        (ReplyBehavior::Hang, AccessError::Timeout),
    ] {
        run_in_context(|| {
            let bus = TestBus::new();
            let goa = start_goa(&bus, vec![make_microsoft365_account("one")]);
            let (client, _updates) = start_observing(&bus);
            goa.set_access_token_reply(token_reply);
            let (_request, results) = request_access(&client, "one");
            assert_eq!(failed_access(&results), expected);
            assert_eq!(goa.access_token_requests().len(), 1);
        });
    }
}

#[test]
fn cancelling_or_dropping_the_request_completes_once_as_cancelled() {
    run_in_context(|| {
        let bus = TestBus::new();
        let goa = start_goa(&bus, vec![make_microsoft365_account("one")]);
        let (client, _updates) = start_observing(&bus);
        goa.set_access_token_reply(ReplyBehavior::Hang);

        let (request, results) = request_access(&client, "one");
        wait_until(|| goa.access_token_requests().len() == 1);
        request.cancel();
        assert_eq!(failed_access(&results), AccessError::Cancelled);

        let (request, results) = request_access(&client, "one");
        wait_until(|| goa.access_token_requests().len() == 2);
        drop(request);
        assert_eq!(failed_access(&results), AccessError::Cancelled);
    });
}
