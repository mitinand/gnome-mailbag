// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    AccountError, CheckStatus, ErrorCause, GoaAccountList, GoaClient, Handle, Shared,
    accounts::{MANAGER, NAME, ROOT, parse_list},
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
    spawn(BusTarget::Session, ATTEMPT_TIMEOUT)
}
#[cfg(test)]
fn start_for_test(address: String, timeout: Duration) -> GoaClient {
    spawn(BusTarget::Private(address), timeout)
}

fn spawn(target: BusTarget, timeout: Duration) -> GoaClient {
    let shared = Arc::new(Shared::default());
    shared.lock().busy = true;
    let client = GoaClient(Arc::new(Handle {
        shared: shared.clone(),
    }));
    let worker = shared.clone();
    if std::thread::Builder::new()
        .name("goa-accounts".into())
        .spawn(move || {
            struct Completion(Arc<Shared>);
            impl Drop for Completion {
                fn drop(&mut self) {
                    self.0.finish();
                }
            }
            let _completion = Completion(worker.clone());
            let context = glib::MainContext::new();
            let _ = context.with_thread_default(|| context.block_on(run(worker, target, timeout)));
        })
        .is_err()
    {
        shared.finish();
    }
    client
}

/// Cancel the whole attempt, including connection and activation, at one deadline.
/// Dropping pending GIO futures cancels their individual operations.
async fn attempt<T>(
    shared: &Shared,
    timeout: Duration,
    future: impl Future<Output = Result<T, AccountError>>,
) -> Option<Result<T, AccountError>> {
    let mut future = pin!(future);
    let mut timer = pin!(glib::timeout_future(timeout));
    poll_fn(|cx| {
        {
            let mut state = shared.lock();
            if state.stop {
                return Poll::Ready(None);
            }
            state.command = Some(cx.waker().clone());
        }
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Some(Err(AccountError::new(
                "account check",
                ErrorCause::Timeout,
            ))));
        }
        future.as_mut().poll(cx).map(Some)
    })
    .await
}

struct Observation {
    connection: gio::DBusConnection,
    changes: Rc<Cell<u64>>,
    // Dropping these unsubscribes without closing the shared desktop connection.
    _subscriptions: Vec<gio::SignalSubscription>,
}

async fn connect(target: &BusTarget) -> Result<Observation, AccountError> {
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
    let changes = Rc::new(Cell::new(0u64));
    let mut subscriptions = Vec::new();
    let counter = changes.clone();
    subscriptions.push(connection.subscribe_to_signal(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        Some(NAME),
        gio::DBusSignalFlags::NONE,
        move |_| counter.set(counter.get() + 1),
    ));
    for member in ["InterfacesAdded", "InterfacesRemoved"] {
        let counter = changes.clone();
        subscriptions.push(connection.subscribe_to_signal(
            Some(NAME),
            Some(MANAGER),
            Some(member),
            Some(ROOT),
            None,
            gio::DBusSignalFlags::NONE,
            move |_| counter.set(counter.get() + 1),
        ));
    }
    let counter = changes.clone();
    subscriptions.push(connection.subscribe_to_signal(
        Some(NAME),
        Some("org.freedesktop.DBus.Properties"),
        Some("PropertiesChanged"),
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            if signal.object_path.starts_with("/org/gnome/OnlineAccounts/") {
                // Until event application is added in portion 3, any property
                // signal invalidates a pending list, including malformed bodies.
                counter.set(counter.get() + 1);
            }
        },
    ));
    Ok(Observation {
        connection,
        changes,
        _subscriptions: subscriptions,
    })
}

fn remaining_millis(deadline: Instant) -> i32 {
    deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .clamp(1, i32::MAX as u128) as i32
}

async fn get_owner(
    connection: &gio::DBusConnection,
    deadline: Instant,
) -> Result<String, glib::Error> {
    let reply = connection
        .call_future(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetNameOwner",
            Some(&(NAME,).to_variant()),
            Some(glib::VariantTy::new("(s)").expect("owner reply signature")),
            gio::DBusCallFlags::NONE,
            remaining_millis(deadline),
        )
        .await?;
    Ok(reply.get::<(String,)>().expect("validated owner reply").0)
}

async fn check(
    observation: &Observation,
    deadline: Instant,
) -> Result<GoaAccountList, AccountError> {
    loop {
        if Instant::now() >= deadline {
            return Err(AccountError::new("account check", ErrorCause::Timeout));
        }
        let change_number = observation.changes.get();
        // This round trip also orders subscription setup before account acquisition.
        let owner = match get_owner(&observation.connection, deadline).await {
            Ok(owner) => owner,
            Err(error) if error.matches(gio::DBusError::NameHasNoOwner) => {
                observation
                    .connection
                    .call_future(
                        Some("org.freedesktop.DBus"),
                        "/org/freedesktop/DBus",
                        "org.freedesktop.DBus",
                        "StartServiceByName",
                        Some(&(NAME, 0u32).to_variant()),
                        Some(glib::VariantTy::new("(u)").expect("activation reply signature")),
                        gio::DBusCallFlags::NONE,
                        remaining_millis(deadline),
                    )
                    .await
                    .map_err(|e| AccountError::from_glib("activate GOA service", e))?;
                get_owner(&observation.connection, deadline)
                    .await
                    .map_err(|e| AccountError::from_glib("find GOA service", e))?
            }
            Err(error) => return Err(AccountError::from_glib("find GOA service", error)),
        };
        if observation.changes.get() != change_number {
            continue;
        }
        let reply = observation
            .connection
            .call_future(
                Some(&owner),
                ROOT,
                MANAGER,
                "GetManagedObjects",
                Some(&().to_variant()),
                Some(glib::VariantTy::new("(a{oa{sa{sv}}})").expect("GOA reply signature")),
                gio::DBusCallFlags::NONE,
                remaining_millis(deadline),
            )
            .await
            .map_err(|e| AccountError::from_glib("GetManagedObjects", e))?;
        if observation.changes.get() != change_number {
            continue;
        }
        return parse_list(&reply);
    }
}

async fn run(shared: Arc<Shared>, target: BusTarget, timeout: Duration) {
    let mut observation = None;
    let mut current = GoaAccountList::initial();
    let mut update_number = 0;
    loop {
        current.status = CheckStatus::Checking;
        current.complete = false;
        update_number += 1;
        current.update_number = update_number;
        shared.publish(current.clone());
        let deadline = Instant::now() + timeout;
        let result = attempt(&shared, timeout, async {
            if observation.is_none() {
                observation = Some(connect(&target).await?);
            }
            check(observation.as_ref().expect("connected observer"), deadline).await
        })
        .await;
        let Some(result) = result else {
            break;
        };
        match result {
            Ok(list) => {
                current = list;
            }
            Err(error) => {
                current.status = CheckStatus::Failed;
                current.complete = false;
                current.error = Some(error);
            }
        }
        update_number += 1;
        current.update_number = update_number;
        // End busy state before publication so a consumer can immediately retry.
        shared.lock().busy = false;
        shared.publish(current.clone());
        let again = poll_fn(|cx| {
            let mut state = shared.lock();
            if state.stop {
                return Poll::Ready(false);
            }
            if state.refresh {
                state.refresh = false;
                state.busy = true;
                return Poll::Ready(true);
            }
            state.command = Some(cx.waker().clone());
            Poll::Pending
        })
        .await;
        if !again {
            break;
        }
    }
    drop(observation);
    // One bounded context turn lets cancelled GIO completions release their state.
    glib::timeout_future(Duration::from_millis(1)).await;
}

#[cfg(test)]
mod tests;
