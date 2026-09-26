// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A panic's message and place, kept on the thread where it happened. The
//! code that sends work to a thread catches the panic there and reads its
//! message there (specs/006-error-handling/research.md §4), and the work ends
//! as a failure of the operation it ran (006 FR-014).

#[cfg(test)]
mod tests;

use crate::{Failure, FailureKind};
use std::{
    cell::Cell,
    panic::{self, AssertUnwindSafe},
    sync::Once,
};

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

/// Runs `work` on this thread and catches a panic inside it, which it returns
/// as the panic's message and place. The work's state is not used after a
/// panic, so it need not be unwind safe; a lock the panic poisoned is taken
/// over by its owner.
pub fn catch_panic<T>(work: impl FnOnce() -> T) -> Result<T, String> {
    install_panic_hook();
    // The hook, installed above, kept the message; the payload is not read.
    panic::catch_unwind(AssertUnwindSafe(work))
        .map_err(|_| take_panic().unwrap_or_else(|| "panic".to_owned()))
}

impl Failure {
    /// The failure of an operation a panic stopped, of the operation's own
    /// kind (`Stopped` for a load, `StoredMailUnreadable` for a read of the
    /// store), with the panic's message and place, or of a thread that
    /// vanished without one.
    pub fn from_panic(kind: FailureKind, panic: Option<String>) -> Self {
        let mut details = format!("Failure: {kind:?}");
        if let Some(panic) = panic {
            details.push_str(&format!("\nPanic: {panic}"));
        }
        Self {
            kind,
            remote_texts: Vec::new(),
            details,
        }
    }
}
