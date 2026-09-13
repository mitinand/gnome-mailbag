// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    AccountError, CheckStatus, ClientHandle, ErrorCause, GoaAccountList, GoaClient,
    SharedClientState,
    accounts::{GOA_BUS_NAME, GOA_ROOT_PATH, OBJECT_MANAGER_INTERFACE, parse_account_list},
};
use gio::prelude::*;
use std::{
    cell::Cell,
    future::{Future, poll_fn},
    pin::pin,
    rc::Rc,
    sync::Arc,
    task::Poll,
    time::{Duration, Instant},
};

const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);

// Private test connections cannot be selected in production builds.
enum BusTarget {
    Session,
    #[cfg(test)]
    Private(String),
}
pub(crate) fn start() -> GoaClient {
    spawn_worker(BusTarget::Session, ATTEMPT_TIMEOUT)
}
#[cfg(test)]
fn start_for_test(address: String, timeout: Duration) -> GoaClient {
    spawn_worker(BusTarget::Private(address), timeout)
}

fn spawn_worker(target: BusTarget, timeout: Duration) -> GoaClient {
    let shared = Arc::new(SharedClientState::default());
    shared.lock().check_pending = true;
    let client = GoaClient(Arc::new(ClientHandle {
        shared: shared.clone(),
    }));
    let worker_state = shared.clone();
    if std::thread::Builder::new()
        .name("goa-accounts".into())
        .spawn(move || {
            struct WorkerExitGuard(Arc<SharedClientState>);
            impl Drop for WorkerExitGuard {
                fn drop(&mut self) {
                    self.0.mark_worker_stopped();
                }
            }
            let _exit_guard = WorkerExitGuard(worker_state.clone());
            let context = glib::MainContext::new();
            let _ = context.with_thread_default(|| {
                context.block_on(run_worker(worker_state, target, timeout))
            });
        })
        .is_err()
    {
        shared.mark_worker_stopped();
    }
    client
}

/// Cancel the whole attempt, including connection and activation, at one deadline.
/// Dropping pending GIO futures cancels their individual operations.
async fn run_with_deadline<T>(
    shared: &SharedClientState,
    timeout: Duration,
    future: impl Future<Output = Result<T, AccountError>>,
) -> Option<Result<T, AccountError>> {
    let mut future = pin!(future);
    let mut deadline_timer = pin!(glib::timeout_future(timeout));
    poll_fn(|cx| {
        {
            let mut state = shared.lock();
            if state.stop_requested {
                return Poll::Ready(None);
            }
            state.command_waker = Some(cx.waker().clone());
        }
        if deadline_timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Some(Err(AccountError::new(
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
    change_version: Rc<Cell<u64>>,
    // Dropping these unsubscribes without closing the shared desktop connection.
    _subscriptions: Vec<gio::SignalSubscription>,
}

async fn connect_and_subscribe(target: &BusTarget) -> Result<GoaConnection, AccountError> {
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
    .map_err(|e| AccountError::from_glib("connect to session bus", e))?;
    let change_version = Rc::new(Cell::new(0u64));
    let mut subscriptions = Vec::new();
    let callback_version = change_version.clone();
    subscriptions.push(connection.subscribe_to_signal(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        Some(GOA_BUS_NAME),
        gio::DBusSignalFlags::NONE,
        move |_| callback_version.set(callback_version.get() + 1),
    ));
    for member in ["InterfacesAdded", "InterfacesRemoved"] {
        let callback_version = change_version.clone();
        subscriptions.push(connection.subscribe_to_signal(
            Some(GOA_BUS_NAME),
            Some(OBJECT_MANAGER_INTERFACE),
            Some(member),
            Some(GOA_ROOT_PATH),
            None,
            gio::DBusSignalFlags::NONE,
            move |_| callback_version.set(callback_version.get() + 1),
        ));
    }
    let callback_version = change_version.clone();
    subscriptions.push(connection.subscribe_to_signal(
        Some(GOA_BUS_NAME),
        Some("org.freedesktop.DBus.Properties"),
        Some("PropertiesChanged"),
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            if signal.object_path.starts_with("/org/gnome/OnlineAccounts/") {
                // Until event application is added in portion 3, any property
                // signal invalidates a pending list, including malformed bodies.
                callback_version.set(callback_version.get() + 1);
            }
        },
    ));
    Ok(GoaConnection {
        connection,
        change_version,
        _subscriptions: subscriptions,
    })
}

fn remaining_millis(deadline: Instant) -> i32 {
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
            remaining_millis(deadline),
        )
        .await?;
    Ok(reply.get::<(String,)>().expect("validated owner reply").0)
}

async fn fetch_accounts(
    goa_connection: &GoaConnection,
    deadline: Instant,
) -> Result<GoaAccountList, AccountError> {
    loop {
        if Instant::now() >= deadline {
            return Err(AccountError::new("account check", ErrorCause::Timeout));
        }
        let request_version = goa_connection.change_version.get();
        // This round trip also orders subscription setup before account acquisition.
        let owner = match resolve_goa_owner(&goa_connection.connection, deadline).await {
            Ok(owner) => owner,
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
                        remaining_millis(deadline),
                    )
                    .await
                    .map_err(|e| AccountError::from_glib("activate GOA service", e))?;
                resolve_goa_owner(&goa_connection.connection, deadline)
                    .await
                    .map_err(|e| AccountError::from_glib("find GOA service", e))?
            }
            Err(error) => return Err(AccountError::from_glib("find GOA service", error)),
        };
        if goa_connection.change_version.get() != request_version {
            continue;
        }
        let reply = goa_connection
            .connection
            .call_future(
                Some(&owner),
                GOA_ROOT_PATH,
                OBJECT_MANAGER_INTERFACE,
                "GetManagedObjects",
                Some(&().to_variant()),
                Some(glib::VariantTy::new("(a{oa{sa{sv}}})").expect("GOA reply signature")),
                gio::DBusCallFlags::NONE,
                remaining_millis(deadline),
            )
            .await
            .map_err(|e| AccountError::from_glib("GetManagedObjects", e))?;
        if goa_connection.change_version.get() != request_version {
            continue;
        }
        return parse_account_list(&reply);
    }
}

async fn run_worker(shared: Arc<SharedClientState>, target: BusTarget, timeout: Duration) {
    let mut goa_connection = None;
    let mut current_accounts = GoaAccountList::initial();
    let mut update_number = 0;
    loop {
        current_accounts.status = CheckStatus::Checking;
        current_accounts.membership_confirmed = false;
        update_number += 1;
        current_accounts.update_number = update_number;
        shared.publish_update(current_accounts.clone());
        let deadline = Instant::now() + timeout;
        let result = run_with_deadline(&shared, timeout, async {
            if goa_connection.is_none() {
                goa_connection = Some(connect_and_subscribe(&target).await?);
            }
            fetch_accounts(
                goa_connection.as_ref().expect("connected observer"),
                deadline,
            )
            .await
        })
        .await;
        let Some(result) = result else {
            break;
        };
        match result {
            Ok(list) => {
                current_accounts = list;
            }
            Err(error) => {
                current_accounts.status = CheckStatus::Failed;
                current_accounts.membership_confirmed = false;
                current_accounts.error = Some(error);
            }
        }
        update_number += 1;
        current_accounts.update_number = update_number;
        // End busy state before publication so a consumer can immediately retry.
        shared.lock().check_pending = false;
        shared.publish_update(current_accounts.clone());
        let refresh_requested = poll_fn(|cx| {
            let mut state = shared.lock();
            if state.stop_requested {
                return Poll::Ready(false);
            }
            if state.refresh_requested {
                state.refresh_requested = false;
                state.check_pending = true;
                return Poll::Ready(true);
            }
            state.command_waker = Some(cx.waker().clone());
            Poll::Pending
        })
        .await;
        if !refresh_requested {
            break;
        }
    }
    drop(goa_connection);
    // One bounded context turn lets cancelled GIO completions release their state.
    glib::timeout_future(Duration::from_millis(1)).await;
}

#[cfg(test)]
mod tests;
