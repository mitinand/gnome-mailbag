// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_caught_panic_gives_its_message_and_place() {
    let panic = catch_panic(|| panic!("the work panicked on purpose")).unwrap_err();
    assert!(
        panic.starts_with("the work panicked on purpose at "),
        "{panic}"
    );
    assert!(panic.contains("panic/tests.rs:"), "{panic}");
    assert_eq!(catch_panic(|| 42), Ok(42));
}

#[test]
fn stopped_work_names_its_kind_and_the_panic() {
    let stopped = Failure::stopped(Some("boom at x.rs:1".to_owned()));
    assert_eq!(stopped.kind, FailureKind::Stopped);
    assert_eq!(stopped.details, "Failure: Stopped\nPanic: boom at x.rs:1");
    assert_eq!(Failure::stopped(None).details, "Failure: Stopped");
}
