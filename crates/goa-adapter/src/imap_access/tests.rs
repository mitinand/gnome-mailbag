// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::{ImapAccess, ImapCredential, ImapEncryption};
use crate::client::tests::{
    RecordedUpdates, dispatch_for, run_in_context, start_test_client, wait_until,
};
use crate::{AccessError, AccessRequest, AccountId, GoaAdapter, test_bus::TestBus, test_goa::*};
use gio::prelude::*;
use glib::Variant;
use std::{cell::RefCell, rc::Rc, time::Duration};

type AccessResults = Rc<RefCell<Vec<Result<ImapAccess, AccessError>>>>;

/// Starts observation and waits for its first read, so its connection exists.
fn start_observing(bus: &TestBus) -> (GoaAdapter, RecordedUpdates) {
    let (client, updates) = start_test_client(bus);
    updates.completed();
    (client, updates)
}

fn request_access(client: &GoaAdapter, account_id: &str) -> (AccessRequest, AccessResults) {
    let results = AccessResults::default();
    let recorded = results.clone();
    let context = glib::MainContext::ref_thread_default();
    let request =
        client.request_imap_access(&AccountId::try_from(account_id).unwrap(), move |result| {
            assert!(
                context.is_owner(),
                "completion must run on the adapter's context"
            );
            recorded.borrow_mut().push(result);
        });
    (request, results)
}

/// Waits for the completion, then checks that no second one follows.
fn completed_access(results: &AccessResults) -> Result<ImapAccess, AccessError> {
    wait_until(|| !results.borrow().is_empty());
    dispatch_for(Duration::from_millis(50));
    assert_eq!(results.borrow().len(), 1, "exactly one completion");
    results.borrow_mut().pop().unwrap()
}

fn successful_access(results: &AccessResults) -> ImapAccess {
    completed_access(results).unwrap_or_else(|error| panic!("access failed: {error:?}"))
}

fn failed_access(results: &AccessResults) -> AccessError {
    match completed_access(results) {
        Ok(_) => panic!("access unexpectedly succeeded"),
        Err(error) => error,
    }
}

/// The credential as a pair, so that a test compares its kind and its value
/// in one assertion. `ImapCredential` itself has no Debug or PartialEq.
fn credential(access: &ImapAccess) -> (&'static str, &str) {
    match &access.credential {
        ImapCredential::Password(password) => ("password", password),
        ImapCredential::AccessToken(token) => ("access token", token),
    }
}

fn account_with_mail_settings(id: &str, settings: &[(&str, Variant)]) -> Interfaces {
    let mut account = make_account(id);
    let mail = account.get_mut(MAIL_INTERFACE).unwrap();
    for (name, value) in settings {
        mail.insert((*name).into(), value.clone());
    }
    account
}

fn encryption_flags(
    use_ssl: bool,
    use_tls: bool,
    accept_ssl_errors: bool,
) -> [(&'static str, Variant); 3] {
    [
        ("ImapUseSsl", use_ssl.to_variant()),
        ("ImapUseTls", use_tls.to_variant()),
        ("ImapAcceptSslErrors", accept_ssl_errors.to_variant()),
    ]
}

#[test]
fn access_uses_the_returned_object_path_and_keeps_the_host_port() {
    run_in_context(|| {
        let bus = TestBus::new();
        let reply = make_account_reply(vec![
            make_account("first"),
            account_with_mail_settings(
                "second",
                &[("ImapHost", "imap.example.invalid:1993".to_variant())],
            ),
            make_account("third"),
        ]);
        let goa = FakeGoaService::new(&bus.address, ReplyBehavior::Value(reply));
        let (client, _updates) = start_observing(&bus);
        let (_request, results) = request_access(&client, "second");
        let access = successful_access(&results);
        assert_eq!(access.account_id, AccountId::try_from("second").unwrap());
        assert_eq!(access.host, "imap.example.invalid:1993");
        assert_eq!(access.login, "synthetic-user");
        assert_eq!(access.encryption, ImapEncryption::ImplicitTls);
        assert_eq!(credential(&access), ("password", SYNTHETIC_PASSWORD));
        assert_eq!(
            goa.password_requests(),
            [PasswordRequest {
                object_path: account_object_path(1),
                password_key: "imap-password".into(),
            }]
        );
    });
}

#[test]
fn ssl_flag_selects_implicit_tls_before_the_starttls_flag() {
    for (use_ssl, use_tls, expected) in [
        (true, false, ImapEncryption::ImplicitTls),
        (false, true, ImapEncryption::StartTls),
        (true, true, ImapEncryption::ImplicitTls),
    ] {
        run_in_context(|| {
            let bus = TestBus::new();
            let account =
                account_with_mail_settings("one", &encryption_flags(use_ssl, use_tls, false));
            let _goa = FakeGoaService::new(
                &bus.address,
                ReplyBehavior::Value(make_account_reply(vec![account])),
            );
            let (client, _updates) = start_observing(&bus);
            let (_request, results) = request_access(&client, "one");
            assert_eq!(successful_access(&results).encryption, expected);
        });
    }
}

#[test]
fn both_encryption_flags_off_are_refused_before_the_password() {
    run_in_context(|| {
        let bus = TestBus::new();
        // Accepting certificate errors neither allows plaintext nor changes the refusal.
        let refused = account_with_mail_settings("refused", &encryption_flags(false, false, true));
        let accepted = account_with_mail_settings("accepted", &encryption_flags(true, false, true));
        let goa = FakeGoaService::new(
            &bus.address,
            ReplyBehavior::Value(make_account_reply(vec![refused, accepted])),
        );
        let (client, _updates) = start_observing(&bus);
        let (_request, results) = request_access(&client, "refused");
        assert_eq!(failed_access(&results), AccessError::NoEncryption);
        assert!(goa.password_requests().is_empty());

        let (_request, results) = request_access(&client, "accepted");
        assert_eq!(
            successful_access(&results).encryption,
            ImapEncryption::ImplicitTls
        );
        assert_eq!(goa.password_requests().len(), 1);
    });
}

#[test]
fn absent_account_and_missing_mail_interface_fail_as_settings() {
    let mut without_mail = make_account("one");
    without_mail.remove(MAIL_INTERFACE);
    for account in [make_account("other"), without_mail] {
        run_in_context(|| {
            let bus = TestBus::new();
            let goa = FakeGoaService::new(
                &bus.address,
                ReplyBehavior::Value(make_account_reply(vec![account])),
            );
            let (client, _updates) = start_observing(&bus);
            let (_request, results) = request_access(&client, "one");
            assert_eq!(failed_access(&results), AccessError::Settings);
            assert!(goa.password_requests().is_empty());
        });
    }
}

#[test]
fn service_errors_and_hangs_are_reported_for_their_step() {
    for (settings_reply, password_reply, expected) in [
        (ReplyBehavior::AccessDenied, None, AccessError::Settings),
        (ReplyBehavior::Hang, None, AccessError::Timeout),
        (
            ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
            Some(ReplyBehavior::AccessDenied),
            AccessError::Password,
        ),
        (
            ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
            Some(ReplyBehavior::Hang),
            AccessError::Timeout,
        ),
    ] {
        run_in_context(|| {
            let bus = TestBus::new();
            let goa = FakeGoaService::new(
                &bus.address,
                ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
            );
            let (client, _updates) = start_observing(&bus);
            goa.set_reply(settings_reply);
            let password_requested = password_reply.is_some();
            if let Some(password_reply) = password_reply {
                goa.set_password_reply(password_reply);
            }
            let (_request, results) = request_access(&client, "one");
            assert_eq!(failed_access(&results), expected);
            assert_eq!(
                goa.password_requests().len(),
                usize::from(password_requested)
            );
        });
    }
}

#[test]
fn cancelling_or_dropping_the_request_completes_once_as_cancelled() {
    run_in_context(|| {
        let bus = TestBus::new();
        let goa = FakeGoaService::new(
            &bus.address,
            ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
        );
        let (client, _updates) = start_observing(&bus);

        goa.set_password_reply(ReplyBehavior::Hang);
        let (request, results) = request_access(&client, "one");
        wait_until(|| !goa.password_requests().is_empty());
        request.cancel();
        assert_eq!(failed_access(&results), AccessError::Cancelled);

        goa.set_reply(ReplyBehavior::Hang);
        let reads_before = goa.read_count();
        let (request, results) = request_access(&client, "one");
        wait_until(|| goa.read_count() > reads_before);
        drop(request);
        assert_eq!(failed_access(&results), AccessError::Cancelled);
    });
}

#[test]
fn access_requests_neither_refresh_nor_change_observed_accounts() {
    run_in_context(|| {
        let bus = TestBus::new();
        let goa = FakeGoaService::new(
            &bus.address,
            ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
        );
        let (client, updates) = start_test_client(&bus);
        let observed = updates.completed();

        let (_request, results) = request_access(&client, "removed");
        assert_eq!(failed_access(&results), AccessError::Settings);
        let (_request, results) = request_access(&client, "one");
        successful_access(&results);

        dispatch_for(Duration::from_millis(50));
        assert!(updates.is_empty(), "no account update from access requests");
        // One observation read plus one settings read per request.
        assert_eq!(goa.read_count(), 3);
        client.refresh_accounts();
        assert_eq!(updates.completed().accounts, observed.accounts);
    });
}

#[test]
fn attention_needed_and_a_failed_observation_read_do_not_block_access() {
    run_in_context(|| {
        let bus = TestBus::new();
        let goa = FakeGoaService::new(&bus.address, ReplyBehavior::AccessDenied);
        let (client, updates) = start_test_client(&bus);
        // The read failed, but observation keeps its connection.
        assert!(updates.completed().last_check.error().is_some());
        let mut account = make_account("one");
        account
            .get_mut(ACCOUNT_INTERFACE)
            .unwrap()
            .insert("AttentionNeeded".into(), true.to_variant());
        goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![account])));
        let (_request, results) = request_access(&client, "one");
        let access = successful_access(&results);
        assert_eq!(credential(&access), ("password", SYNTHETIC_PASSWORD));
    });
}

#[test]
fn the_exported_interface_chooses_the_credential() {
    run_in_context(|| {
        let bus = TestBus::new();
        let accounts = vec![
            make_account("password-account"),
            make_google_account("google-account"),
        ];
        let goa = FakeGoaService::new(
            &bus.address,
            ReplyBehavior::Value(make_account_reply(accounts)),
        );
        let (client, _updates) = start_observing(&bus);

        let (_request, results) = request_access(&client, "google-account");
        let access = successful_access(&results);
        assert_eq!(
            credential(&access),
            ("access token", SYNTHETIC_ACCESS_TOKEN)
        );
        assert_eq!(access.host, "imap.gmail.com");
        assert_eq!(
            goa.access_token_requests(),
            [AccessTokenRequest {
                object_path: account_object_path(1),
            }]
        );
        // A Google account holds no password, so none is asked for.
        assert!(goa.password_requests().is_empty());

        let (_request, results) = request_access(&client, "password-account");
        let access = successful_access(&results);
        assert_eq!(credential(&access), ("password", SYNTHETIC_PASSWORD));
        assert_eq!(goa.password_requests().len(), 1);
        assert_eq!(goa.access_token_requests().len(), 1);
    });
}

#[test]
fn a_refused_or_held_access_token_is_reported_like_a_password() {
    for (token_reply, expected) in [
        (ReplyBehavior::AccessDenied, AccessError::AccessToken),
        (ReplyBehavior::Hang, AccessError::Timeout),
    ] {
        run_in_context(|| {
            let bus = TestBus::new();
            let goa = FakeGoaService::new(
                &bus.address,
                ReplyBehavior::Value(make_account_reply(vec![make_google_account("one")])),
            );
            let (client, _updates) = start_observing(&bus);
            goa.set_access_token_reply(token_reply);
            let (_request, results) = request_access(&client, "one");
            assert_eq!(failed_access(&results), expected);
            assert_eq!(goa.access_token_requests().len(), 1);
        });
    }
}

#[test]
fn an_object_with_neither_credential_interface_fails_before_asking_for_one() {
    run_in_context(|| {
        let bus = TestBus::new();
        let mut account = make_account("one");
        account.remove(PASSWORD_BASED_INTERFACE);
        let goa = FakeGoaService::new(
            &bus.address,
            ReplyBehavior::Value(make_account_reply(vec![account])),
        );
        let (client, _updates) = start_observing(&bus);
        let (_request, results) = request_access(&client, "one");
        assert_eq!(failed_access(&results), AccessError::Settings);
        assert!(goa.password_requests().is_empty());
        assert!(goa.access_token_requests().is_empty());
    });
}

#[test]
fn without_the_observer_connection_the_request_fails_later_as_settings() {
    run_in_context(|| {
        let bus = TestBus::new();
        let _goa = FakeGoaService::new(
            &bus.address,
            ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
        );
        // The observer has not connected yet: nothing has been dispatched.
        let (client, _updates) = start_test_client(&bus);
        let (_request, results) = request_access(&client, "one");
        assert!(
            results.borrow().is_empty(),
            "the answer never arrives inside the call"
        );
        assert_eq!(failed_access(&results), AccessError::Settings);
    });
}

#[test]
fn a_request_without_connection_cancelled_before_its_turn_is_cancelled() {
    run_in_context(|| {
        let bus = TestBus::new();
        let _goa = FakeGoaService::new(
            &bus.address,
            ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
        );
        let (client, _updates) = start_test_client(&bus);
        let (request, results) = request_access(&client, "one");
        request.cancel();
        assert_eq!(failed_access(&results), AccessError::Cancelled);
    });
}
