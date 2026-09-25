// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The copied report. The dialog's widgets are checked in the window's
//! graphical test (`mail_ui/tests.rs`), the one GTK test of this crate.

use super::*;
use mailbag_imap::{ImapError, ImapFailure, ImapStep, ServerReply};
use mailbag_providers::LoadFailure;

#[test]
fn the_report_holds_the_dialog_text_in_its_order_with_the_sign_in_name_replaced() {
    // The IMAP crate replaces the sign-in name where it builds the failure.
    let failure = LoadFailure::Imap(ImapError {
        failure: ImapFailure::Failed(ImapStep::SignIn),
        server_reply: Some(ServerReply {
            code: Some("AUTHENTICATIONFAILED".to_owned()),
            text: "<login> may not sign in".to_owned(),
        }),
        alerts: vec!["Password for <login> expired".to_owned()],
    })
    .declare();
    let report = report_text(&failure);
    let expected_order = [
        failure.title,
        failure.explanation.as_str(),
        failure.advice.expect("sign-in advice"),
        "Alert from the mail server:\nPassword for <login> expired",
        "Reply from the mail server:\n<login> may not sign in",
        "Technical details:\nFailure: Failed(SignIn)\nServer code: AUTHENTICATIONFAILED",
    ];
    assert_eq!(report, expected_order.join("\n\n"));
}

#[test]
fn the_report_leaves_out_what_the_failure_does_not_have() {
    let failure = mailbag_providers::IncompleteList::MoreAvailable.declare();
    assert_eq!(
        report_text(&failure),
        format!("{}\n\n{}", failure.title, failure.explanation)
    );
}

#[test]
fn a_long_remote_text_leaves_the_later_blocks_in_the_report() {
    let failure = LoadFailure::Imap(ImapError {
        failure: ImapFailure::Failed(ImapStep::OpenInbox),
        server_reply: Some(ServerReply {
            code: None,
            text: "a".repeat(70_000),
        }),
        alerts: Vec::new(),
    })
    .declare();
    let report = report_text(&failure);
    assert!(
        report.ends_with("Technical details:\nFailure: Failed(OpenInbox)"),
        "{}",
        &report[report.len() - 80..]
    );
}
