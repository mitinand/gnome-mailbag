// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::{
    CheckStatus, ErrorCause,
    test_bus::Bus,
    test_goa::{self as fixture, Goa, Reply},
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
fn wait<T>(future: impl Future<Output = T>) -> T {
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
fn start_on(bus: &Bus) -> crate::GoaClient {
    start_for_test(bus.address.clone(), Duration::from_millis(400))
}
fn result(client: &crate::GoaClient) -> crate::GoaAccountList {
    loop {
        let update = wait(client.next_account_update()).expect("client open");
        if update.status != CheckStatus::Checking {
            return update;
        }
    }
}
fn only_read_calls(goa: &Goa) {
    let calls = goa.calls.lock().unwrap();
    assert!(!calls.is_empty());
    for call in calls.iter() {
        assert!(
            call.destination.starts_with(':'),
            "request must target a unique GOA owner"
        );
        assert_eq!(call.path, fixture::ROOT);
        assert_eq!(call.interface, fixture::MANAGER);
        assert_eq!(call.method, "GetManagedObjects");
        assert_eq!(call.body_type, "()");
    }
}
#[test]
fn initial_accounts_arrive_without_default_context_and_use_exact_read_protocol() {
    let bus = Bus::new();
    let goa = Goa::new(
        &bus.address,
        vec![Reply::Value(fixture::reply(vec![fixture::account("one")]))],
    );
    let client = start_on(&bus);
    let update = result(&client);
    assert_eq!(update.status, CheckStatus::Ready);
    assert!(update.complete);
    assert_eq!(update.accounts.len(), 1);
    only_read_calls(&goa);
    client.stop();
    assert!(wait(client.next_account_update()).is_none());
}
#[test]
fn healthy_empty_and_absent_service_are_different() {
    let bus = Bus::new();
    let client = start_on(&bus);
    let update = result(&client);
    assert_eq!(update.status, CheckStatus::Failed);
    assert!(!update.complete);
    client.stop();
    let goa = Goa::new(&bus.address, vec![Reply::Value(fixture::reply(vec![]))]);
    let client = start_on(&bus);
    let update = result(&client);
    assert!(update.complete);
    assert!(update.accounts.is_empty());
    assert_eq!(update.status, CheckStatus::Ready);
    only_read_calls(&goa);
}
#[test]
fn startup_timeout_covers_initial_acquisition() {
    let bus = Bus::new();
    let _goa = Goa::new(&bus.address, vec![Reply::Hang]);
    let started = Instant::now();
    let client = start_on(&bus);
    let update = result(&client);
    assert_eq!(update.error.unwrap().cause, ErrorCause::Timeout);
    assert!(!update.complete);
    assert!(started.elapsed() < Duration::from_secs(2));
}
#[test]
fn wrong_reply_and_remote_error_are_safe_failures() {
    for (response, cause) in [
        (Reply::WrongType, ErrorCause::InvalidReply),
        (Reply::Error, ErrorCause::AccessDenied),
    ] {
        let bus = Bus::new();
        let goa = Goa::new(&bus.address, vec![response]);
        let client = start_on(&bus);
        let update = result(&client);
        assert_eq!(update.status, CheckStatus::Failed);
        assert!(!update.complete);
        assert!(!format!("{update:?}").contains("synthetic-private-detail"));
        let error = update.error.unwrap();
        assert_eq!(error.cause, cause);
        assert!(error.domain.is_some());
        assert!(error.code.is_some());
        only_read_calls(&goa);
    }
}
#[test]
fn change_during_initial_check_rejects_the_old_reply() {
    let bus = Bus::new();
    let old = fixture::reply(vec![fixture::account("one")]);
    let mut changed = fixture::account("one");
    changed
        .get_mut(fixture::ACCOUNT)
        .unwrap()
        .insert("MailDisabled".into(), true.to_variant());
    let goa = Goa::new(
        &bus.address,
        vec![
            Reply::ChangeBeforeCompletion {
                old,
                disabled: true,
            },
            Reply::Value(fixture::reply(vec![changed])),
        ],
    );
    let client = start_on(&bus);
    let update = result(&client);
    assert_eq!(
        update.accounts.values().next().unwrap().mail_disabled,
        Some(true)
    );
    assert!(goa.calls.lock().unwrap().len() >= 2);
    only_read_calls(&goa);
}
#[test]
fn repeated_stale_replies_cannot_extend_attempt_deadline() {
    let bus = Bus::new();
    let old = fixture::reply(vec![fixture::account("one")]);
    let _goa = Goa::new(
        &bus.address,
        vec![Reply::ChangeBeforeCompletion {
            old,
            disabled: true,
        }],
    );
    let start = Instant::now();
    let client = start_on(&bus);
    let update = result(&client);
    assert_eq!(update.error.unwrap().cause, ErrorCause::Timeout);
    assert!(start.elapsed() < Duration::from_secs(2));
}
#[test]
fn refresh_keeps_old_data_on_failure_and_can_be_repeated() {
    let bus = Bus::new();
    let reply = fixture::reply(vec![fixture::account("one")]);
    let goa = Goa::new(
        &bus.address,
        vec![
            Reply::Value(reply.clone()),
            Reply::Error,
            Reply::Value(reply),
        ],
    );
    let client = start_on(&bus);
    let first = result(&client);
    client.refresh_accounts();
    let failed = result(&client);
    assert_eq!(failed.status, CheckStatus::Failed);
    assert!(!failed.complete);
    assert_eq!(first.accounts, failed.accounts);
    assert!(failed.update_number > first.update_number);
    client.refresh_accounts();
    assert_eq!(result(&client).status, CheckStatus::Ready);
    only_read_calls(&goa);
}
#[test]
fn stop_and_last_handle_drop_finish_pending_worker() {
    let bus = Bus::new();
    let goa = Goa::new(&bus.address, vec![Reply::Hang]);
    let client = start_on(&bus);
    goa.wait_for_calls(1);
    let shared = client.shared_for_test();
    let clone = client.clone();
    drop(client);
    assert!(!shared.lock().stop);
    drop(clone);
    let start = Instant::now();
    wait(std::future::poll_fn(|cx| {
        let mut state = shared.lock();
        if state.closed {
            Poll::Ready(())
        } else {
            state.consumer = Some(cx.waker().clone());
            Poll::Pending
        }
    }));
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn repeated_refresh_reuses_pending_check_and_stop_is_idempotent() {
    let bus = Bus::new();
    let goa = Goa::new(&bus.address, vec![Reply::Hang]);
    let client = start_on(&bus);
    goa.wait_for_calls(1);
    for _ in 0..100 {
        client.refresh_accounts();
    }
    let update = result(&client);
    assert_eq!(update.error.unwrap().cause, ErrorCause::Timeout);
    assert_eq!(goa.calls.lock().unwrap().len(), 1);
    client.stop();
    client.stop();
    assert!(wait(client.next_account_update()).is_none());
}

#[test]
fn malformed_membership_is_not_reported_as_healthy_empty() {
    let bus = Bus::new();
    let mut damaged = fixture::account("one");
    damaged.get_mut(fixture::ACCOUNT).unwrap().remove("Id");
    let _goa = Goa::new(
        &bus.address,
        vec![Reply::Value(fixture::reply(vec![
            damaged,
            fixture::account("two"),
        ]))],
    );
    let client = start_on(&bus);
    let update = result(&client);
    assert_eq!(update.status, CheckStatus::Failed);
    assert_eq!(update.error.unwrap().cause, ErrorCause::InvalidMembership);
    assert!(!update.complete);
    assert_eq!(update.accounts.len(), 1);
}

#[test]
fn unavailable_connection_is_reported_by_the_worker() {
    let bus = Bus::new();
    let address = bus.address.clone();
    drop(bus);
    let client = start_for_test(address, Duration::from_millis(400));
    let update = result(&client);
    assert_eq!(update.status, CheckStatus::Failed);
    assert_eq!(update.error.unwrap().operation, "connect to session bus");
    assert!(!update.complete);
}
