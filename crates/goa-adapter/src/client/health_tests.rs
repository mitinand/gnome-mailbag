// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later
use super::{
    tests::{await_check_result, await_with_timeout},
    *,
};
use crate::{test_bus::TestBus, test_goa::*};
use std::{collections::BTreeMap, thread};

fn start_timed_client(bus: &TestBus) -> (GoaAdapter, GoaUpdates) {
    spawn_worker(
        BusTarget::Private(bus.address.clone()),
        CheckTiming {
            attempt_timeout: Duration::from_millis(80),
            health_interval: Duration::from_millis(160),
        },
    )
}
fn await_ready(updates: &mut GoaUpdates) -> Arc<AccountUpdate> {
    await_with_timeout(async {
        loop {
            let update = updates.next_account_update().await.unwrap();
            if update.last_check.is_complete() && !update.check_pending {
                break update;
            }
        }
    })
}

#[test]
fn production_timing_matches_contract() {
    let timing = CheckTiming::default();
    assert_eq!(timing.attempt_timeout, Duration::from_secs(5));
    assert_eq!(timing.health_interval, Duration::from_secs(10));
}

#[test]
fn health_check_repairs_missed_event_and_later_signals_patch_only_changed_fields() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let (client, mut updates) = start_timed_client(&bus);
    await_check_result(&mut updates);
    let mut repaired = make_account("one");
    repaired
        .get_mut(ACCOUNT_INTERFACE)
        .unwrap()
        .insert("ProviderName".into(), "Checked provider".to_variant());
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![repaired])));
    let update = await_ready(&mut updates);
    assert_eq!(
        update
            .accounts
            .values()
            .next()
            .unwrap()
            .provider_name
            .as_deref(),
        Some("Checked provider")
    );
    goa.change_properties(
        ACCOUNT_INTERFACE,
        BTreeMap::from([("AttentionNeeded".into(), true.to_variant())]),
        vec![],
    );
    let update = await_with_timeout(updates.next_account_update()).unwrap();
    let account = update.accounts.values().next().unwrap();
    assert_eq!(account.needs_attention, Some(true));
    assert_eq!(account.provider_name.as_deref(), Some("Checked provider"));
    client.stop();
    await_with_timeout(async { while updates.next_account_update().await.is_some() {} });
    let stopped_calls = goa.calls.lock().unwrap().len();
    thread::sleep(Duration::from_millis(200));
    assert_eq!(goa.calls.lock().unwrap().len(), stopped_calls);
}

#[test]
fn routine_pending_check_preserves_ready_state_and_manual_refresh_joins_it() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let (client, mut updates) = spawn_worker(
        BusTarget::Private(bus.address.clone()),
        CheckTiming {
            attempt_timeout: Duration::from_secs(2),
            health_interval: Duration::from_millis(160),
        },
    );
    await_check_result(&mut updates);
    goa.set_reply(ReplyBehavior::Hang);
    goa.wait_for_calls(2);
    let update = await_with_timeout(updates.next_account_update()).unwrap();
    assert!(update.check_pending);
    assert!(update.last_check.is_complete());
    assert_eq!(update.accounts.len(), 1);
    client.refresh_accounts();
    assert_eq!(
        goa.calls.lock().unwrap().len(),
        2,
        "manual refresh reuses the held check"
    );
    let reply = make_account_reply(vec![make_account("one")]);
    goa.complete_held_reply(&reply);
    goa.set_reply(ReplyBehavior::Value(reply));
    assert!(await_check_result(&mut updates).last_check.is_complete());
    client.stop();
}

#[test]
fn failed_checks_wait_for_periodic_tick_and_same_owner_recovers() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(&bus.address, vec![ReplyBehavior::AccessDenied]);
    let (client, mut updates) = start_timed_client(&bus);
    goa.wait_for_calls(4);
    {
        let calls = goa.calls.lock().unwrap();
        // Skip the first request: connecting can shorten the gap to the first timer tick.
        for pair in calls[1..4].windows(2) {
            assert!(
                pair[1].received_at.duration_since(pair[0].received_at)
                    >= Duration::from_millis(100),
                "failed checks must wait for the next periodic tick"
            );
        }
    }
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![
        make_account("one"),
    ])));
    assert!(await_ready(&mut updates).last_check.is_complete());
    client.stop();
}

#[test]
fn busy_ticks_are_skipped_without_queued_catchup() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(&bus.address, vec![ReplyBehavior::Hang]);
    let (client, mut updates) = spawn_worker(
        BusTarget::Private(bus.address.clone()),
        CheckTiming {
            attempt_timeout: Duration::from_millis(240),
            health_interval: Duration::from_millis(40),
        },
    );
    goa.wait_for_calls(1);
    let failure = await_check_result(&mut updates);
    assert_eq!(
        failure.last_check.error().unwrap().cause,
        ErrorCause::Timeout
    );
    goa.wait_for_calls(2);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(goa.calls.lock().unwrap().len(), 2);
    client.stop();
}

#[test]
fn delayed_context_dispatch_produces_one_due_tick_without_catchup_burst() {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| {
            context.block_on(async {
                let worker_state = Rc::new(RefCell::new(GoaWorkerState::new(Arc::new(
                    SharedClientState::default(),
                ))));
                let timer = start_health_timer(&worker_state, Duration::from_millis(30));
                glib::timeout_future(Duration::from_millis(1)).await;
                // Pause the worker long enough to miss several timer ticks.
                thread::sleep(Duration::from_millis(120));
                glib::timeout_future(Duration::from_millis(1)).await;
                assert!(worker_state.borrow().recheck_requested);
                worker_state.borrow_mut().begin_check();
                worker_state
                    .borrow_mut()
                    .finish_check(crate::accounts::parse_account_snapshot(
                        &make_account_reply(vec![]),
                    ));
                glib::timeout_future(Duration::from_millis(10)).await;
                assert!(!worker_state.borrow().recheck_requested);
                drop(timer);
                glib::timeout_future(Duration::from_millis(40)).await;
                assert!(!worker_state.borrow().recheck_requested);
            })
        })
        .unwrap();
}

#[test]
fn silent_service_fails_by_health_deadline_and_recovers_without_owner_change() {
    let bus = TestBus::new();
    let goa = FakeGoaService::new(
        &bus.address,
        vec![ReplyBehavior::Value(make_account_reply(vec![
            make_account("one"),
        ]))],
    );
    let (client, mut updates) = start_timed_client(&bus);
    let original = await_check_result(&mut updates);
    goa.set_reply(ReplyBehavior::Hang);
    let stopped_answering = Instant::now();
    let failure = await_check_result(&mut updates);
    assert_eq!(
        failure.last_check.error().unwrap().cause,
        ErrorCause::Timeout
    );
    assert_eq!(failure.accounts, original.accounts);
    assert!(!failure.last_check.is_complete());
    assert!(stopped_answering.elapsed() < Duration::from_millis(500));
    goa.set_reply(ReplyBehavior::Value(make_account_reply(vec![
        make_account("one"),
    ])));
    let recovered = await_ready(&mut updates);
    assert!(recovered.last_check.is_complete());
    assert_eq!(recovered.accounts, original.accounts);
    client.stop();
}
