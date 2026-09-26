// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The copied report. The dialog's widgets are checked in the window's
//! graphical tests (`mail_ui/tests.rs`).

use super::*;
use crate::failure_declarations::{declare_failure, declare_short_list};
use mailbag_domain::{Failure, FailureKind, IncompleteList, RemoteSource, RemoteText, ServerStep};

#[test]
fn the_report_holds_the_dialog_text_in_its_order_with_the_sign_in_name_replaced() {
    // The IMAP crate replaces the sign-in name where it builds the failure.
    let failure = declare_failure(&Failure {
        kind: FailureKind::ServerRejectedSignIn,
        remote_texts: vec![
            RemoteText {
                source: RemoteSource::ServerAlert,
                text: "Password for <login> expired".to_owned(),
            },
            RemoteText {
                source: RemoteSource::ServerReply,
                text: "<login> may not sign in".to_owned(),
            },
        ],
        details: "Failure: ServerRejectedSignIn\nServer code: AUTHENTICATIONFAILED".to_owned(),
    });
    let report = report_text(&failure);
    let expected_order = [
        failure.title,
        failure.explanation.as_str(),
        failure.advice.expect("sign-in advice"),
        "Alert from the mail server:\nPassword for <login> expired",
        "Reply from the mail server:\n<login> may not sign in",
        "Technical details:\nFailure: ServerRejectedSignIn\nServer code: AUTHENTICATIONFAILED",
    ];
    assert_eq!(report, expected_order.join("\n\n"));
}

#[test]
fn the_report_leaves_out_what_the_failure_does_not_have() {
    let failure = declare_short_list(&IncompleteList::MoreAvailable);
    assert_eq!(
        report_text(&failure),
        format!("{}\n\n{}", failure.title, failure.explanation)
    );
}

#[test]
fn a_long_remote_text_leaves_the_later_blocks_in_the_report() {
    let failure = declare_failure(&Failure {
        kind: FailureKind::ServerStepFailed(ServerStep::OpenInbox),
        remote_texts: vec![RemoteText {
            source: RemoteSource::ServerReply,
            text: "a".repeat(70_000),
        }],
        details: "Failure: ServerStepFailed(OpenInbox)".to_owned(),
    });
    let report = report_text(&failure);
    assert!(
        report.ends_with("Technical details:\nFailure: ServerStepFailed(OpenInbox)"),
        "{}",
        &report[report.len() - 80..]
    );
}
