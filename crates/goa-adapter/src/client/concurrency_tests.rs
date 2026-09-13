// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::{
    tests::{await_check_result, await_with_timeout, start_test_client},
    *,
};
use crate::{
    test_bus::TestBus,
    test_goa::{GOA_ROOT_PATH, *},
};
use std::{
    collections::BTreeMap,
    sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Wake, Waker},
    thread,
};

#[derive(Default)]
struct WakeCounter(AtomicUsize);
impl Wake for WakeCounter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn make_client_without_worker() -> GoaClient {
    GoaClient(Arc::new(ClientHandle {
        shared: Arc::new(SharedClientState::default()),
    }))
}

#[test]
fn publication_racing_with_wait_registration_never_loses_update() {
    for sequence in 1..=100 {
        let client = make_client_without_worker();
        let shared = client.shared_for_test();
        let barrier = Arc::new(Barrier::new(2));
        let producer_barrier = barrier.clone();
        let producer = thread::spawn(move || {
            producer_barrier.wait();
            shared.publish_update(GoaAccountList {
                update_number: sequence,
                ..GoaAccountList::initial()
            });
        });
        barrier.wait();
        assert_eq!(
            await_with_timeout(client.next_account_update())
                .unwrap()
                .update_number,
            sequence
        );
        producer.join().unwrap();
    }
}

#[test]
fn idle_consumer_sleeps_and_burst_replaces_one_slot_with_one_wakeup() {
    let client = make_client_without_worker();
    let shared = client.shared_for_test();
    let wake_counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(wake_counter.clone());
    let mut context = Context::from_waker(&waker);
    let mut next_update = pin!(client.next_account_update());
    assert!(next_update.as_mut().poll(&mut context).is_pending());
    assert_eq!(wake_counter.0.load(Ordering::SeqCst), 0);
    for sequence in 1..=10_000 {
        shared.publish_update(GoaAccountList {
            update_number: sequence,
            ..GoaAccountList::initial()
        });
    }
    assert_eq!(wake_counter.0.load(Ordering::SeqCst), 1);
    let Poll::Ready(Some(update)) = next_update.as_mut().poll(&mut context) else {
        panic!("latest update missing")
    };
    assert_eq!(update.update_number, 10_000);
    assert!(!shared.lock().update_pending);
}

#[test]
fn unexpected_exit_reports_failure_after_previous_update_was_consumed() {
    let client = make_client_without_worker();
    let shared = client.shared_for_test();
    let mut previous =
        crate::accounts::parse_account_list(&make_account_reply(vec![make_account("one")]))
            .unwrap();
    previous.update_number = 7;
    shared.publish_update(previous.clone());
    await_with_timeout(client.next_account_update());
    shared.mark_worker_stopped();
    let failed = await_with_timeout(client.next_account_update()).unwrap();
    assert_eq!(failed.error.unwrap().cause, ErrorCause::WorkerStopped);
    assert_eq!(failed.accounts, previous.accounts);
    assert_eq!(failed.update_number, 8);
    assert!(await_with_timeout(client.next_account_update()).is_none());
}

#[test]
fn ten_thousand_signals_with_paused_consumer_keep_only_final_facts() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let client = start_test_client(&bus);
    await_check_result(&client);
    for sequence in 0..10_000 {
        goa.change_properties(
            ACCOUNT_INTERFACE,
            BTreeMap::from([("MailDisabled".into(), (sequence % 2 == 0).to_variant())]),
            vec![],
        );
    }
    goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::from([("ProviderName".into(), "Burst complete".to_variant())]),
        vec![],
    );
    // Inspect the latest account list without taking the update meant for the caller.
    let shared = client.shared_for_test();
    await_with_timeout(poll_fn(|cx| {
        let mut state = shared.lock();
        if state.latest_update.as_ref().is_some_and(|update| {
            update
                .accounts
                .values()
                .next()
                .unwrap()
                .provider_name
                .as_deref()
                == Some("Burst complete")
        }) {
            Poll::Ready(())
        } else {
            state.update_waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }));
    let update = await_with_timeout(client.next_account_update()).unwrap();
    assert_eq!(update.accounts.len(), 1);
    assert_eq!(
        update.accounts.values().next().unwrap().mail_disabled,
        Some(false)
    );
    assert_eq!(
        goa.calls.lock().unwrap().len(),
        1,
        "ordinary valid changes need no full request"
    );
    assert!(!shared.lock().update_pending);
    client.stop();
    assert!(await_with_timeout(client.next_account_update()).is_none());
}

#[test]
fn stop_wins_over_commands_during_idle_failure_and_signal_load() {
    for reply in [
        ReplyBehavior::AccessDenied,
        ReplyBehavior::Value(make_account_reply(vec![make_account("one")])),
    ] {
        let bus = TestBus::new();
        let goa = FakeGoaService::new(&bus.address, vec![reply]);
        let client = start_test_client(&bus);
        await_check_result(&client);
        for _ in 0..1000 {
            goa.change_properties(
                ACCOUNT_INTERFACE,
                BTreeMap::from([("AttentionNeeded".into(), true.to_variant())]),
                vec![],
            );
            client.refresh_accounts();
        }
        let stopped_at = Instant::now();
        client.stop();
        await_with_timeout(async { while client.next_account_update().await.is_some() {} });
        assert!(stopped_at.elapsed() < Duration::from_secs(1));
        let state = client.shared_for_test();
        let state = state.lock();
        assert!(state.worker_stopped);
        assert!(state.command_waker.is_none());
        assert!(state.update_waker.is_none());
    }
}

#[test]
fn oversized_list_keeps_previous_accounts_and_later_check_recovers() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let client = start_test_client(&bus);
    let original = await_check_result(&client);
    goa.set_reply(ReplyBehavior::Value(make_account_reply(
        (0..4097)
            .map(|i| make_account(&format!("synthetic-{i}")))
            .collect(),
    )));
    client.refresh_accounts();
    let failed = await_check_result(&client);
    assert_eq!(failed.error.unwrap().cause, ErrorCause::DataLimit);
    assert_eq!(failed.accounts, original.accounts);
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![])));
    client.refresh_accounts();
    let recovered = await_check_result(&client);
    assert!(recovered.membership_confirmed);
    assert!(recovered.accounts.is_empty());
    client.stop();
}

#[test]
fn stop_cancels_connection_authentication_without_waiting_for_server() {
    use std::os::unix::net::UnixListener;
    let directory = std::env::temp_dir().join(format!("mailbag-auth-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let socket_path = directory.join("socket");
    let listener = UnixListener::bind(&socket_path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = start_for_test(
        format!("unix:path={}", socket_path.display()),
        Duration::from_secs(5),
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    let _connection = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "connection fixture deadline");
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    };
    let stopped_at = Instant::now();
    client.stop();
    await_with_timeout(async { while client.next_account_update().await.is_some() {} });
    assert!(stopped_at.elapsed() < Duration::from_secs(1));
    drop(listener);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn incomplete_lists_cannot_accumulate_accounts_beyond_the_limit() {
    let bus = TestBus::new();
    let initial = (0..3000)
        .map(|i| make_account(&format!("old-{i}")))
        .collect();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(initial))],
    );
    let client = start_for_test(bus.address.clone(), Duration::from_secs(2));
    let original = await_check_result(&client);
    let mut incoming: Vec<_> = (0..3000)
        .map(|i| make_account(&format!("new-{i}")))
        .collect();
    incoming[0].get_mut(ACCOUNT_INTERFACE).unwrap().remove("Id");
    goa.set_reply(ReplyBehavior::Value(make_account_reply(incoming)));
    client.refresh_accounts();
    let failed = await_check_result(&client);
    assert_eq!(failed.error.unwrap().cause, ErrorCause::DataLimit);
    assert_eq!(failed.accounts, original.accounts);
    assert!(!failed.membership_confirmed);
    client.stop();
}

#[test]
fn refresh_commands_at_completion_reliably_wake_next_wait() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![]))],
    );
    let client = start_test_client(&bus);
    let mut previous_update_number = await_check_result(&client).update_number;
    for _ in 0..100 {
        // The caller has received the check result, but the worker may not yet
        // be waiting for commands. A refresh sent now must still reach it.
        client.refresh_accounts();
        let update = await_check_result(&client);
        assert_eq!(update.status, CheckStatus::Ready);
        assert!(update.update_number > previous_update_number);
        previous_update_number = update.update_number;
    }
    assert_eq!(goa.calls.lock().unwrap().len(), 101);
    client.stop();
}

#[test]
fn total_string_limit_does_not_discard_disable_in_the_same_signal() {
    let bus = TestBus::new();
    let mut objects = make_object_map(
        (0..1024)
            .map(|i| make_account(&format!("synthetic-{i}")))
            .collect(),
    );
    let first_path =
        glib::variant::ObjectPath::try_from(format!("{GOA_ROOT_PATH}/Accounts/account_0")).unwrap();
    objects
        .get_mut(&first_path)
        .unwrap()
        .get_mut(ACCOUNT_INTERFACE)
        .unwrap()
        .insert("ProviderName".into(), "x".to_variant());
    // Keep each field within its limit and the total string size one byte below its limit.
    let initial_bytes: usize = objects
        .iter()
        .map(|(path, interfaces)| {
            path.as_str().len()
                + interfaces
                    .values()
                    .flat_map(|fields| fields.values())
                    .filter_map(|field| field.get::<String>())
                    .map(|text| text.len())
                    .sum::<usize>()
        })
        .sum();
    let mut remaining_bytes = 16 * 1024 * 1024 - 1 - initial_bytes;
    for (path, interfaces) in &mut objects {
        for (interface, property) in [
            (ACCOUNT_INTERFACE, "ProviderType"),
            (ACCOUNT_INTERFACE, "ProviderName"),
            (ACCOUNT_INTERFACE, "PresentationIdentity"),
            (MAIL_INTERFACE, "EmailAddress"),
        ] {
            if path == &first_path && property == "ProviderName" {
                continue;
            }
            let fields = interfaces.get_mut(interface).unwrap();
            let old_string_bytes = fields
                .get(property)
                .and_then(|field| field.get::<String>())
                .map_or(0, |text| text.len());
            let added_bytes = remaining_bytes.min(4096 - old_string_bytes);
            if added_bytes > 0 {
                fields.insert(
                    property.into(),
                    "x".repeat(old_string_bytes + added_bytes).to_variant(),
                );
                remaining_bytes -= added_bytes;
            }
        }
    }
    assert_eq!(remaining_bytes, 0);
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value((objects,).to_variant())],
    );
    let client = start_for_test(bus.address.clone(), Duration::from_secs(2));
    assert_eq!(await_check_result(&client).status, CheckStatus::Ready);
    goa.set_reply(ReplyBehavior::Hang);
    goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::from([
            ("ProviderName".into(), "x".repeat(4096).to_variant()),
            ("MailDisabled".into(), true.to_variant()),
        ]),
        vec![],
    );
    let failed = await_check_result(&client);
    assert_eq!(failed.error.unwrap().cause, ErrorCause::DataLimit);
    assert!(
        failed
            .accounts
            .values()
            .any(|account| account.mail_disabled == Some(true))
    );
    client.stop();
}
