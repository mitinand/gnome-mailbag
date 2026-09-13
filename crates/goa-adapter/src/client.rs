// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    AccountCheckError, ClientHandle, ErrorCause, GoaAdapter, GoaUpdates, SharedClientState,
    accounts::{
        AccountSnapshot, GOA_BUS_NAME, GOA_ROOT_PATH, OBJECT_MANAGER_INTERFACE, map_glib_error,
        parse_account_snapshot,
    },
};
use gio::prelude::*;
use std::{
    cell::RefCell,
    future::{Future, poll_fn},
    pin::pin,
    rc::Rc,
    sync::Arc,
    task::Poll,
    time::{Duration, Instant},
};

#[cfg(test)]
use crate::AccountUpdate;

mod worker_state;
use worker_state::GoaWorkerState;

#[derive(Clone, Copy)]
struct CheckTiming {
    attempt_timeout: Duration,
    health_interval: Duration,
}
impl Default for CheckTiming {
    fn default() -> Self {
        Self {
            attempt_timeout: Duration::from_secs(5),
            health_interval: Duration::from_secs(10),
        }
    }
}

// Private test connections cannot be selected in production builds.
enum BusTarget {
    Session,
    #[cfg(test)]
    Private(String),
}
pub(crate) fn start() -> (GoaAdapter, GoaUpdates) {
    spawn_worker(BusTarget::Session, CheckTiming::default())
}
#[cfg(test)]
fn start_for_test(address: String, timeout: Duration) -> (GoaAdapter, GoaUpdates) {
    spawn_worker(
        BusTarget::Private(address),
        CheckTiming {
            attempt_timeout: timeout,
            ..CheckTiming::default()
        },
    )
}

fn spawn_worker(target: BusTarget, timing: CheckTiming) -> (GoaAdapter, GoaUpdates) {
    let shared = Arc::new(SharedClientState::default());
    let client = GoaAdapter(Arc::new(ClientHandle {
        shared: shared.clone(),
    }));
    let shared_state = shared.clone();
    if std::thread::Builder::new()
        .name("goa-accounts".into())
        .spawn(move || {
            struct WorkerExitGuard(Arc<SharedClientState>);
            impl Drop for WorkerExitGuard {
                fn drop(&mut self) {
                    self.0.mark_worker_stopped();
                }
            }
            let _exit_guard = WorkerExitGuard(shared_state.clone());
            let context = glib::MainContext::new();
            let _ = context
                .with_thread_default(|| context.block_on(run_worker(shared_state, target, timing)));
        })
        .is_err()
    {
        shared.mark_worker_stopped();
    }
    (client, GoaUpdates { shared })
}

/// Cancel the whole attempt, including connection and activation, at one deadline.
/// Dropping pending GIO futures cancels their individual operations.
async fn await_account_check(
    shared: &SharedClientState,
    timeout: Duration,
    future: impl Future<Output = Result<AccountSnapshot, AccountCheckError>>,
) -> Option<Result<AccountSnapshot, AccountCheckError>> {
    let mut future = pin!(future);
    let mut deadline_timer = pin!(glib::timeout_future(timeout));
    poll_fn(|cx| {
        {
            let mut state = shared.lock();
            if state.stop_requested {
                return Poll::Ready(None);
            }
            state.command_waker = Some(cx.waker().clone());
            // Repeated refresh commands join the active request.
            state.refresh_requested = false;
        }
        if deadline_timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Some(Err(AccountCheckError::new(
                "account check",
                ErrorCause::Timeout,
            ))));
        }
        future.as_mut().poll(cx).map(Some)
    })
    .await
}

struct GoaConnection {
    connection: gio::DBusConnection,
    // GIO runs these callbacks on the GOA worker thread. Weak references prevent
    // callbacks from keeping its state alive or changing another client's accounts.
    _subscriptions: Vec<gio::SignalSubscription>,
}

async fn connect_and_subscribe(
    target: &BusTarget,
    worker_state: &Rc<RefCell<GoaWorkerState>>,
) -> Result<GoaConnection, AccountCheckError> {
    let connection = match target {
        BusTarget::Session => gio::bus_get_future(gio::BusType::Session).await,
        #[cfg(test)]
        BusTarget::Private(address) => {
            gio::DBusConnection::for_address_future(
                address,
                gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                    | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                None::<&gio::DBusAuthObserver>,
            )
            .await
        }
    }
    .map_err(|error| map_glib_error("connect to session bus", error))?;
    let mut subscriptions = Vec::new();
    let callback_state = Rc::downgrade(worker_state);
    subscriptions.push(connection.subscribe_to_signal(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        Some(GOA_BUS_NAME),
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let Some(worker_state) = callback_state.upgrade() else {
                return;
            };
            if let Some((name, _, goa_owner)) = signal.parameters.get::<(String, String, String)>()
                && name == GOA_BUS_NAME
            {
                worker_state.borrow_mut().record_owner_change(&goa_owner);
            }
        },
    ));
    for (interface, member, path) in [
        (
            OBJECT_MANAGER_INTERFACE,
            "InterfacesAdded",
            Some(GOA_ROOT_PATH),
        ),
        (
            OBJECT_MANAGER_INTERFACE,
            "InterfacesRemoved",
            Some(GOA_ROOT_PATH),
        ),
        ("org.freedesktop.DBus.Properties", "PropertiesChanged", None),
    ] {
        let callback_state = Rc::downgrade(worker_state);
        subscriptions.push(connection.subscribe_to_signal(
            Some(GOA_BUS_NAME),
            Some(interface),
            Some(member),
            path,
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                if let Some(worker_state) = callback_state.upgrade() {
                    worker_state.borrow_mut().apply_account_signal(
                        signal.sender_name,
                        signal.object_path,
                        member,
                        signal.parameters,
                    );
                }
            },
        ));
    }
    Ok(GoaConnection {
        connection,
        _subscriptions: subscriptions,
    })
}

fn remaining_timeout_ms(deadline: Instant) -> i32 {
    deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .clamp(1, i32::MAX as u128) as i32
}

async fn resolve_goa_owner(
    connection: &gio::DBusConnection,
    deadline: Instant,
) -> Result<String, glib::Error> {
    let reply = connection
        .call_future(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetNameOwner",
            Some(&(GOA_BUS_NAME,).to_variant()),
            Some(glib::VariantTy::new("(s)").expect("owner reply signature")),
            gio::DBusCallFlags::NONE,
            remaining_timeout_ms(deadline),
        )
        .await?;
    Ok(reply.get::<(String,)>().expect("validated owner reply").0)
}

async fn fetch_accounts(
    goa_connection: &GoaConnection,
    deadline: Instant,
    worker_state: &RefCell<GoaWorkerState>,
) -> Result<AccountSnapshot, AccountCheckError> {
    loop {
        if Instant::now() >= deadline {
            return Err(AccountCheckError::new("account check", ErrorCause::Timeout));
        }
        let request_change_number = worker_state.borrow().account_change_number;
        // Waiting for the bus reply lets it process our earlier signal subscriptions
        // before we request the account list.
        let goa_owner = match resolve_goa_owner(&goa_connection.connection, deadline).await {
            Ok(goa_owner) => goa_owner,
            Err(error) if error.matches(gio::DBusError::NameHasNoOwner) => {
                goa_connection
                    .connection
                    .call_future(
                        Some("org.freedesktop.DBus"),
                        "/org/freedesktop/DBus",
                        "org.freedesktop.DBus",
                        "StartServiceByName",
                        Some(&(GOA_BUS_NAME, 0u32).to_variant()),
                        Some(glib::VariantTy::new("(u)").expect("activation reply signature")),
                        gio::DBusCallFlags::NONE,
                        remaining_timeout_ms(deadline),
                    )
                    .await
                    .map_err(|e| map_glib_error("activate GOA service", e))?;
                resolve_goa_owner(&goa_connection.connection, deadline)
                    .await
                    .map_err(|e| map_glib_error("find GOA service", e))?
            }
            Err(error) => return Err(map_glib_error("find GOA service", error)),
        };
        if worker_state.borrow().account_change_number != request_change_number {
            continue;
        }
        {
            let mut worker_state = worker_state.borrow_mut();
            if worker_state.goa_owner.as_deref() != Some(&goa_owner) {
                worker_state.account_paths.clear();
            }
            worker_state.goa_owner = Some(goa_owner.clone());
        }
        let reply = goa_connection
            .connection
            .call_future(
                Some(&goa_owner),
                GOA_ROOT_PATH,
                OBJECT_MANAGER_INTERFACE,
                "GetManagedObjects",
                Some(&().to_variant()),
                Some(glib::VariantTy::new("(a{oa{sa{sv}}})").expect("GOA reply signature")),
                gio::DBusCallFlags::NONE,
                remaining_timeout_ms(deadline),
            )
            .await;
        if worker_state.borrow().account_change_number != request_change_number {
            continue;
        }
        let reply = reply.map_err(|error| map_glib_error("GetManagedObjects", error))?;
        return parse_account_snapshot(&reply);
    }
}

/// Request a periodic account check only when no check is running.
/// Keep at most one pending request; do not queue missed timer ticks.
struct HealthTimer(glib::JoinHandle<()>);
impl Drop for HealthTimer {
    fn drop(&mut self) {
        self.0.abort();
    }
}
fn start_health_timer(
    worker_state: &Rc<RefCell<GoaWorkerState>>,
    interval: Duration,
) -> HealthTimer {
    let worker_state = Rc::downgrade(worker_state);
    HealthTimer(
        glib::MainContext::ref_thread_default().spawn_local(async move {
            loop {
                glib::timeout_future(interval).await;
                let Some(worker_state) = worker_state.upgrade() else {
                    break;
                };
                let mut worker_state = worker_state.borrow_mut();
                if !worker_state.account_list.check_pending {
                    worker_state.request_recheck();
                }
            }
        }),
    )
}

async fn run_worker(shared: Arc<SharedClientState>, target: BusTarget, timing: CheckTiming) {
    let worker_state = Rc::new(RefCell::new(GoaWorkerState::new(shared.clone())));
    let health_timer = start_health_timer(&worker_state, timing.health_interval);
    let mut goa_connection = None;
    loop {
        worker_state.borrow_mut().begin_check();
        let deadline = Instant::now() + timing.attempt_timeout;
        let result = await_account_check(&shared, timing.attempt_timeout, async {
            if goa_connection.is_none() {
                goa_connection = Some(connect_and_subscribe(&target, &worker_state).await?);
            }
            fetch_accounts(
                goa_connection.as_ref().expect("connected observer"),
                deadline,
                &worker_state,
            )
            .await
        })
        .await;
        let Some(result) = result else {
            break;
        };
        worker_state.borrow_mut().finish_check(result);
        let next_check = poll_fn(|cx| {
            let mut commands = shared.lock();
            if commands.stop_requested {
                return Poll::Ready(None);
            }
            commands.command_waker = Some(cx.waker().clone());
            let manual_check = std::mem::take(&mut commands.refresh_requested);
            drop(commands);
            let mut worker_state = worker_state.borrow_mut();
            worker_state.worker_waker = Some(cx.waker().clone());
            if manual_check || worker_state.recheck_requested {
                return Poll::Ready(Some(()));
            }
            Poll::Pending
        })
        .await;
        if next_check.is_none() {
            break;
        }
    }
    drop(health_timer);
    drop(goa_connection);
    worker_state.borrow_mut().worker_waker = None;
    drop(worker_state);
    // Let GIO finish callbacks for cancelled requests without waiting for GOA replies.
    glib::timeout_future(Duration::from_millis(1)).await;
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod event_tests;

#[cfg(test)]
mod health_tests;

#[cfg(test)]
mod concurrency_tests;

#[cfg(test)]
mod account_list_tests;
