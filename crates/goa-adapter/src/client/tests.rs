// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::{
    CheckStatus, ErrorCause,
    test_bus::TestBus,
    test_goa::{self as fixture, FakeGoaService, ReplyBehavior},
};
use std::{
    future::Future,
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
    time::{Duration, Instant},
};

struct ThreadWake(thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
// Deliberately never iterate GLib's default context (or any GLib context).
pub(super) fn await_with_timeout<T>(future: impl Future<Output = T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(3);
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "client outer test deadline");
        thread::park_timeout(remaining);
    }
}
pub(super) fn start_test_client(bus: &TestBus) -> (crate::GoaAdapter, crate::GoaUpdates) {
    start_for_test(bus.address.clone(), Duration::from_millis(400))
}
pub(super) fn await_check_result(updates: &mut crate::GoaUpdates) -> crate::AccountUpdate {
    loop {
        let update = await_with_timeout(updates.next_account_update()).expect("client open");
        if update.status != CheckStatus::Checking {
            return update;
        }
    }
}
fn assert_only_account_reads(goa: &FakeGoaService) {
    let calls = goa.calls.lock().unwrap();
    assert!(!calls.is_empty());
    for call in calls.iter() {
        assert!(
            call.destination.starts_with(':'),
            "request must target a unique GOA owner"
        );
        assert_eq!(call.path, fixture::GOA_ROOT_PATH);
        assert_eq!(call.interface, fixture::OBJECT_MANAGER_INTERFACE);
        assert_eq!(call.method, "GetManagedObjects");
        assert_eq!(call.body_type, "()");
    }
}
#[test]
fn initial_accounts_arrive_without_default_context_and_use_exact_read_protocol() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(fixture::make_account_reply(vec![
            fixture::make_account("one"),
        ]))],
    );
    let (client, mut updates) = start_test_client(&bus);
    let update = await_check_result(&mut updates);
    assert_eq!(update.status, CheckStatus::Ready);
    assert!(update.membership_confirmed);
    assert_eq!(update.accounts.len(), 1);
    assert_only_account_reads(&goa);
    client.stop();
    assert!(await_with_timeout(updates.next_account_update()).is_none());
}
#[test]
fn healthy_empty_and_absent_service_are_different() {
    let bus = TestBus::new();
    let (client, mut updates) = start_test_client(&bus);
    let update = await_check_result(&mut updates);
    assert_eq!(update.status, CheckStatus::Failed);
    assert!(!update.membership_confirmed);
    client.stop();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(fixture::make_account_reply(vec![]))],
    );
    let (_client, mut updates) = start_test_client(&bus);
    let update = await_check_result(&mut updates);
    assert!(update.membership_confirmed);
    assert!(update.accounts.is_empty());
    assert_eq!(update.status, CheckStatus::Ready);
    assert_only_account_reads(&goa);
}
#[test]
fn startup_timeout_covers_initial_acquisition() {
    let bus = TestBus::new();
    let _goa = FakeGoaService::new(&bus.address, vec![ReplyBehavior::Hang]);
    let started = Instant::now();
    let (_client, mut updates) = start_test_client(&bus);
    let update = await_check_result(&mut updates);
    assert_eq!(update.error.unwrap().cause, ErrorCause::Timeout);
    assert!(!update.membership_confirmed);
    assert!(started.elapsed() < Duration::from_secs(2));
}
#[test]
fn wrong_reply_and_remote_error_are_safe_failures() {
    for (response, cause) in [
        (ReplyBehavior::WrongType, ErrorCause::InvalidReply),
        (ReplyBehavior::AccessDenied, ErrorCause::AccessDenied),
    ] {
        let bus = TestBus::new();
        let goa = FakeGoaService::new(&bus.address, vec![response]);
        let (_client, mut updates) = start_test_client(&bus);
        let update = await_check_result(&mut updates);
        assert_eq!(update.status, CheckStatus::Failed);
        assert!(!update.membership_confirmed);
        assert!(!format!("{update:?}").contains("synthetic-private-detail"));
        let error = update.error.unwrap();
        assert_eq!(error.cause, cause);
        assert!(error.domain.is_some());
        assert!(error.code.is_some());
        assert_only_account_reads(&goa);
    }
}
#[test]
fn change_during_initial_check_rejects_the_old_reply() {
    let bus = TestBus::new();
    let stale_reply = fixture::make_account_reply(vec![fixture::make_account("one")]);
    let mut disabled_account = fixture::make_account("one");
    disabled_account
        .get_mut(fixture::ACCOUNT_INTERFACE)
        .unwrap()
        .insert("MailDisabled".into(), true.to_variant());
    let goa = FakeGoaService::new(
        &bus.address,
        vec![
            ReplyBehavior::ChangeBeforeCompletion {
                stale_reply,
                mail_disabled: true,
            },
            ReplyBehavior::Value(fixture::make_account_reply(vec![disabled_account])),
        ],
    );
    let (_client, mut updates) = start_test_client(&bus);
    let update = await_check_result(&mut updates);
    assert_eq!(
        update.accounts.values().next().unwrap().mail_enabled,
        Some(false)
    );
    assert!(goa.calls.lock().unwrap().len() >= 2);
    assert_only_account_reads(&goa);
}
#[test]
fn repeated_stale_replies_cannot_extend_attempt_deadline() {
    let bus = TestBus::new();
    let stale_reply = fixture::make_account_reply(vec![fixture::make_account("one")]);
    let _goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::ChangeBeforeCompletion {
            stale_reply,
            mail_disabled: true,
        }],
    );
    let start = Instant::now();
    let (_client, mut updates) = start_test_client(&bus);
    let update = await_check_result(&mut updates);
    assert_eq!(update.error.unwrap().cause, ErrorCause::Timeout);
    assert!(start.elapsed() < Duration::from_secs(2));
}
#[test]
fn refresh_keeps_old_data_on_failure_and_can_be_repeated() {
    let bus = TestBus::new();
    let reply = fixture::make_account_reply(vec![fixture::make_account("one")]);
    let goa = FakeGoaService::new(
        &bus.address,
        vec![
            ReplyBehavior::Value(reply.clone()),
            ReplyBehavior::AccessDenied,
            ReplyBehavior::Value(reply),
        ],
    );
    let (client, mut updates) = start_test_client(&bus);
    let first = await_check_result(&mut updates);
    client.refresh_accounts();
    let failed = await_check_result(&mut updates);
    assert_eq!(failed.status, CheckStatus::Failed);
    assert!(!failed.membership_confirmed);
    assert_eq!(first.accounts, failed.accounts);
    assert!(failed.update_number > first.update_number);
    client.refresh_accounts();
    assert_eq!(await_check_result(&mut updates).status, CheckStatus::Ready);
    assert_only_account_reads(&goa);
}
#[test]
fn stop_and_last_handle_drop_finish_pending_worker() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(&bus.address, vec![ReplyBehavior::Hang]);
    let (client, _updates) = start_test_client(&bus);
    goa.wait_for_calls(1);
    let shared = client.shared_for_test();
    let clone = client.clone();
    drop(client);
    assert!(!shared.lock().stop_requested);
    drop(clone);
    let start = Instant::now();
    await_with_timeout(std::future::poll_fn(|cx| {
        let mut state = shared.lock();
        if state.worker_stopped {
            Poll::Ready(())
        } else {
            state.update_waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }));
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn repeated_refresh_reuses_pending_check_and_stop_is_idempotent() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(&bus.address, vec![ReplyBehavior::Hang]);
    let (client, mut updates) = start_test_client(&bus);
    goa.wait_for_calls(1);
    for _ in 0..100 {
        client.refresh_accounts();
    }
    let update = await_check_result(&mut updates);
    assert_eq!(update.error.unwrap().cause, ErrorCause::Timeout);
    assert_eq!(goa.calls.lock().unwrap().len(), 1);
    client.stop();
    client.stop();
    assert!(await_with_timeout(updates.next_account_update()).is_none());
}

#[test]
fn malformed_membership_is_not_reported_as_healthy_empty() {
    let bus = TestBus::new();
    let mut damaged = fixture::make_account("one");
    damaged
        .get_mut(fixture::ACCOUNT_INTERFACE)
        .unwrap()
        .remove("Id");
    let _goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(fixture::make_account_reply(vec![
            damaged,
            fixture::make_account("two"),
        ]))],
    );
    let (_client, mut updates) = start_test_client(&bus);
    let update = await_check_result(&mut updates);
    assert_eq!(update.status, CheckStatus::Failed);
    assert_eq!(update.error.unwrap().cause, ErrorCause::InvalidList);
    assert!(!update.membership_confirmed);
    assert_eq!(update.accounts.len(), 1);
}

#[test]
fn unavailable_connection_is_reported_by_the_worker() {
    let bus = TestBus::new();
    let address = bus.address.clone();
    drop(bus);
    let (_client, mut updates) = start_for_test(address, Duration::from_millis(400));
    let update = await_check_result(&mut updates);
    assert_eq!(update.status, CheckStatus::Failed);
    assert_eq!(update.error.unwrap().operation, "connect to session bus");
    assert!(!update.membership_confirmed);
}
