// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Each failure's action, advice, remote texts and technical details. The
//! wording itself is reviewed in the code, not pinned here.

use super::*;
use mailbag_imap::ServerReply;

fn imap_failure(failure: ImapFailure, code: Option<&str>, alerts: &[&str]) -> LoadFailure {
    LoadFailure::Imap(ImapError {
        failure,
        server_reply: Some(ServerReply {
            code: code.map(str::to_owned),
            text: "<login> may not sign in now".to_owned(),
        }),
        alerts: alerts.iter().map(|alert| (*alert).to_owned()).collect(),
    })
}

fn graph_failure(failure: GraphFailure, reason: &str) -> LoadFailure {
    LoadFailure::MicrosoftGraph(GraphError {
        failure,
        reason: Some(reason.to_owned()),
    })
}

fn refused(status: u32, code: &str) -> GraphFailure {
    GraphFailure::Refused {
        status,
        code: Some(code.to_owned()),
    }
}

fn sources(declared: &DeclaredFailure) -> Vec<&'static str> {
    declared
        .remote_texts
        .iter()
        .map(|text| text.source)
        .collect()
}

#[test]
fn the_technical_details_carry_the_records_values_one_per_line() {
    let rejected = imap_failure(
        ImapFailure::Failed(ImapStep::SignIn),
        Some("AUTHENTICATIONFAILED"),
        &[],
    )
    .declare();
    assert_eq!(
        rejected.details,
        "Failure: Failed(SignIn)\nServer code: AUTHENTICATIONFAILED"
    );
    let refused = graph_failure(refused(500, "generalException"), "Service fault").declare();
    assert_eq!(
        refused.details,
        "Failure: Refused\nStatus: 500\nService code: generalException"
    );
    let panicked = LoadFailure::WorkerStopped(Some("boom at x.rs:1".to_owned())).declare();
    assert_eq!(
        panicked.details,
        "Failure: WorkerStopped\nPanic: boom at x.rs:1"
    );
    let short = IncompleteList::ServerRefused(ServerReply {
        code: Some("LIMIT".to_owned()),
        text: String::new(),
    })
    .declare();
    assert_eq!(short.details, "Server code: LIMIT");
}

#[test]
fn a_timeout_at_sign_in_offers_retry_without_advice() {
    let failure = LoadFailure::Imap(ImapFailure::TimedOut(ImapStep::SignIn).into());
    let declared = failure.declare();
    assert_eq!(declared.action, Some(FailureAction::Retry));
    assert_eq!(declared.advice, None);
}

#[test]
fn a_rejected_sign_in_sends_to_online_accounts_with_the_alert_first() {
    let declared = imap_failure(
        ImapFailure::Failed(ImapStep::SignIn),
        Some("AUTHENTICATIONFAILED"),
        &["Password for <login> expired"],
    )
    .declare();
    assert_eq!(declared.action, Some(FailureAction::OnlineAccounts));
    assert!(declared.advice.is_some());
    assert_eq!(sources(&declared), [ALERT_FROM_SERVER, REPLY_FROM_SERVER]);
    assert_eq!(
        declared.remote_texts[0].text,
        "Password for <login> expired"
    );
    assert_eq!(declared.remote_texts[1].text, "<login> may not sign in now");
    // The server's words and the code stay out of the explanation (FR-009).
    for server_words in ["<login>", "AUTHENTICATIONFAILED"] {
        assert!(!declared.explanation.contains(server_words));
    }
}

#[test]
fn a_temporary_outage_and_an_unknown_code_offer_retry_with_the_code_in_the_details() {
    for code in ["UNAVAILABLE", "LIMIT"] {
        let declared =
            imap_failure(ImapFailure::Failed(ImapStep::SignIn), Some(code), &[]).declare();
        assert_eq!(declared.action, Some(FailureAction::Retry), "{code}");
        assert_eq!(declared.advice, None, "{code}");
        assert!(declared.details.contains(code), "{code}");
    }
}

#[test]
fn nothing_the_user_does_helps_a_missing_sign_in_method() {
    let declared = LoadFailure::Imap(ImapFailure::NoSignInMethod.into()).declare();
    assert_eq!(declared.action, None);
}

#[test]
fn a_failed_secure_connection_offers_retry_since_a_cut_handshake_looks_the_same() {
    let failure = ImapFailure::Failed(ImapStep::SecureConnection);
    let declared = LoadFailure::Imap(failure.into()).declare();
    assert_eq!(declared.action, Some(FailureAction::Retry));
}

#[test]
fn the_mail_service_status_decides_between_online_accounts_and_retry() {
    let rejected =
        graph_failure(refused(401, "InvalidAuthenticationToken"), "Token expired").declare();
    assert_eq!(rejected.action, Some(FailureAction::OnlineAccounts));
    assert!(rejected.advice.is_some());

    let failed = graph_failure(refused(500, "generalException"), "Service fault").declare();
    assert_eq!(failed.action, Some(FailureAction::Retry));
    assert_eq!(sources(&failed), [MESSAGE_FROM_SERVICE]);
    assert_eq!(failed.remote_texts[0].text, "Service fault");
    assert!(!failed.explanation.contains("500"));
}

#[test]
fn an_answer_the_code_cannot_read_offers_retry() {
    let declared = graph_failure(GraphFailure::InvalidReply, "not the expected shape").declare();
    assert_eq!(declared.action, Some(FailureAction::Retry));
    assert_eq!(declared.advice, None);
    assert!(declared.details.contains("InvalidReply"));
}

#[test]
fn a_failed_connection_to_the_mail_service_carries_the_system_text() {
    let declared = graph_failure(GraphFailure::ConnectionFailed, "Connection refused").declare();
    assert_eq!(declared.action, Some(FailureAction::Retry));
    assert_eq!(sources(&declared), [FROM_SYSTEM]);
}

#[test]
fn an_account_without_encryption_sends_to_online_accounts() {
    let declared = LoadFailure::OnlineAccounts(AccessError::NoEncryption).declare();
    assert_eq!(declared.action, Some(FailureAction::OnlineAccounts));
    assert!(declared.advice.is_some());
}

#[test]
fn a_stopped_worker_offers_retry_and_asks_for_a_report() {
    let declared = LoadFailure::WorkerStopped(None).declare();
    assert_eq!(declared.action, Some(FailureAction::Retry));
    assert!(declared.advice.is_some());
}

#[test]
fn a_short_list_carries_the_refusal_only_when_the_server_refused() {
    let refused = IncompleteList::ServerRefused(ServerReply {
        code: Some("LIMIT".to_owned()),
        text: "Too many messages for <login>".to_owned(),
    })
    .declare();
    assert_eq!(refused.action, Some(FailureAction::Retry));
    assert_eq!(sources(&refused), [REPLY_FROM_SERVER]);

    let more = IncompleteList::MoreAvailable.declare();
    assert_eq!(more.action, None);
    assert!(more.remote_texts.is_empty());
    assert!(more.details.is_empty());
}

#[test]
fn only_a_message_without_text_declares_a_failure() {
    assert_eq!(ReceivedContent::Text("Hello".to_owned()).declare(), None);
    let encrypted = ReceivedContent::Explained(ContentExplanation::Encrypted)
        .declare()
        .expect("an encrypted message has no text");
    assert_eq!(encrypted.action, None);
    let not_returned = ReceivedContent::TextNotReturned
        .declare()
        .expect("a missing text is a failure");
    assert_eq!(not_returned.action, Some(FailureAction::Retry));
}
