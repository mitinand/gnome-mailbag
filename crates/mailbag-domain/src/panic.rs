// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A panic's message and place, kept on the thread where it happened. The
//! code that sends work to a thread catches the panic there and reads its
//! message there (specs/006-error-handling/research.md §4).

use std::{cell::Cell, panic, sync::Once};

thread_local! {
    /// The last panic on this thread as `message at file:line`, written by the
    /// panic hook and taken by the work the panic stopped.
    static LAST_PANIC: Cell<Option<String>> = const { Cell::new(None) };
}

/// Keeps each panic's message and place on the thread where it happens, then
/// lets the previous hook report it to the error stream as before. The hook
/// serves the whole process, so it is installed once.
pub fn install_panic_hook() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let previous_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let message = info.payload_as_str().unwrap_or("panic");
            let panic = match info.location() {
                Some(place) => format!("{message} at {}:{}", place.file(), place.line()),
                None => message.to_owned(),
            };
            LAST_PANIC.set(Some(panic));
            previous_hook(info);
        }));
    });
}

/// The last panic on this thread as `message at file:line`, once; `None` when
/// the hook is not installed or no panic happened since the last call.
pub fn take_panic() -> Option<String> {
    LAST_PANIC.take()
}
