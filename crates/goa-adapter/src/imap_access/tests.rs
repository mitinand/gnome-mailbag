// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::{ImapAccess, ImapAccessError, ImapAccessRequest, ImapEncryption};
use crate::client::tests::{
    RecordedUpdates, dispatch_for, run_in_context, start_test_client, wait_until,
};
use crate::{AccountId, GoaAdapter, test_bus::TestBus, test_goa::*};
use gio::prelude::*;
use glib::Variant;
use std::{cell::RefCell, rc::Rc, time::Duration};

type AccessResults = Rc<RefCell<Vec<Result<ImapAccess, ImapAccessError>>>>;

/// Starts observation and waits for its first read, so its connection exists.
fn start_observing(bus: &TestBus) -> (GoaAdapter, RecordedUpdates) {
    let (client, updates) = start_test_client(bus);
    updates.completed();
    (client, updates)
}

fn request_access(client: &GoaAdapter, account_id: &str) -> (ImapAccessRequest, AccessResults) {
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
fn completed_access(results: &AccessResults) -> Result<ImapAccess, ImapAccessError> {
    wait_until(|| !results.borrow().is_empty());
    dispatch_for(Duration::from_millis(50));
    assert_eq!(results.borrow().len(), 1, "exactly one completion");
    results.borrow_mut().pop().unwrap()
}

fn successful_access(results: &AccessResults) -> ImapAccess {
    completed_access(results).unwrap_or_else(|error| panic!("access failed: {error:?}"))
}

fn failed_access(results: &AccessResults) -> ImapAccessError {
    match completed_access(results) {
        Ok(_) => panic!("access unexpectedly succeeded"),
        Err(error) => error,
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
        assert_eq!(access.password, SYNTHETIC_PASSWORD);
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
        assert_eq!(failed_access(&results), ImapAccessError::NoEncryption);
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
            assert_eq!(failed_access(&results), ImapAccessError::Settings);
            assert!(goa.password_requests().is_empty());
        });
    }
}

#[test]
fn service_errors_and_hangs_are_reported_for_their_step() {
    for (settings_reply, password_reply, expected) in [
        (ReplyBehavior::AccessDenied, None, ImapAccessError::Settings),
        (ReplyBehavior::Hang, None, ImapAccessError::Timeout),
        (
            ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
            Some(ReplyBehavior::AccessDenied),
            ImapAccessError::Password,
        ),
        (
            ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
            Some(ReplyBehavior::Hang),
            ImapAccessError::Timeout,
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
        assert_eq!(failed_access(&results), ImapAccessError::Cancelled);

        goa.set_reply(ReplyBehavior::Hang);
        let reads_before = goa.read_count();
        let (request, results) = request_access(&client, "one");
        wait_until(|| goa.read_count() > reads_before);
        drop(request);
        assert_eq!(failed_access(&results), ImapAccessError::Cancelled);
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
        assert_eq!(failed_access(&results), ImapAccessError::Settings);
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
        assert_eq!(successful_access(&results).password, SYNTHETIC_PASSWORD);
    });
}
