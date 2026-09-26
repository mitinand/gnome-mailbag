// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Each failure's action, advice and remote texts, and that the declaration
//! carries the lower layer's technical details. The wording itself is
//! reviewed in the code, not pinned here.

use super::*;

fn failure_of(kind: FailureKind) -> Failure {
    Failure {
        kind,
        remote_texts: Vec::new(),
        details: format!("Failure: {kind:?}"),
    }
}

fn remote_text(source: RemoteSource, text: &str) -> RemoteText {
    RemoteText {
        source,
        text: text.to_owned(),
    }
}

fn sources(declared: &DeclaredFailure) -> Vec<RemoteSource> {
    declared
        .remote_texts
        .iter()
        .map(|text| text.source)
        .collect()
}

#[test]
fn a_timeout_at_sign_in_offers_retry_without_advice() {
    let failure = failure_of(FailureKind::ServerNotResponding(ServerStep::SignIn));
    let declared = declare_failure(&failure);
    assert_eq!(declared.action, Some(FailureAction::Retry));
    assert_eq!(declared.advice, None);
    assert_eq!(declared.details, failure.details);
}

#[test]
fn a_rejected_sign_in_sends_to_online_accounts_with_the_alert_first() {
    let declared = declare_failure(&Failure {
        kind: FailureKind::ServerRejectedSignIn,
        remote_texts: vec![
            remote_text(RemoteSource::ServerAlert, "Password for <login> expired"),
            remote_text(RemoteSource::ServerReply, "<login> may not sign in now"),
        ],
        details: "Failure: ServerRejectedSignIn\nServer code: AUTHENTICATIONFAILED".to_owned(),
    });
    assert_eq!(declared.action, Some(FailureAction::OnlineAccounts));
    assert!(declared.advice.is_some());
    assert_eq!(
        sources(&declared),
        [RemoteSource::ServerAlert, RemoteSource::ServerReply]
    );
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
    for kind in [
        FailureKind::ServerUnavailable(ServerStep::SignIn),
        FailureKind::ServerStepFailed(ServerStep::SignIn),
    ] {
        let failure = Failure {
            details: format!("Failure: {kind:?}\nServer code: LIMIT"),
            ..failure_of(kind)
        };
        let declared = declare_failure(&failure);
        assert_eq!(declared.action, Some(FailureAction::Retry), "{kind:?}");
        assert_eq!(declared.advice, None, "{kind:?}");
        assert_eq!(declared.details, failure.details, "{kind:?}");
    }
}

#[test]
fn nothing_the_user_does_helps_a_missing_sign_in_method() {
    let declared = declare_failure(&failure_of(FailureKind::NoSignInMethod));
    assert_eq!(declared.action, None);
}

#[test]
fn a_failed_secure_connection_offers_retry_since_a_cut_handshake_looks_the_same() {
    let kind = FailureKind::ServerStepFailed(ServerStep::SecureConnection);
    let declared = declare_failure(&failure_of(kind));
    assert_eq!(declared.action, Some(FailureAction::Retry));
}

#[test]
fn the_mail_service_status_decides_between_online_accounts_and_retry() {
    let rejected = declare_failure(&failure_of(FailureKind::ServiceRejectedSignIn));
    assert_eq!(rejected.action, Some(FailureAction::OnlineAccounts));
    assert!(rejected.advice.is_some());

    let failed = declare_failure(&Failure {
        kind: FailureKind::RequestRefused,
        remote_texts: vec![remote_text(RemoteSource::ServiceMessage, "Service fault")],
        details: "Failure: RequestRefused\nStatus: 500\nService code: generalException".to_owned(),
    });
    assert_eq!(failed.action, Some(FailureAction::Retry));
    assert_eq!(sources(&failed), [RemoteSource::ServiceMessage]);
    assert_eq!(failed.remote_texts[0].text, "Service fault");
    assert!(!failed.explanation.contains("500"));
}

#[test]
fn an_answer_the_code_cannot_read_offers_retry() {
    let declared = declare_failure(&failure_of(FailureKind::UnexpectedAnswer));
    assert_eq!(declared.action, Some(FailureAction::Retry));
    assert_eq!(declared.advice, None);
    assert!(declared.details.contains("UnexpectedAnswer"));
}

#[test]
fn a_failed_connection_to_the_mail_service_carries_the_system_text() {
    let declared = declare_failure(&Failure {
        remote_texts: vec![remote_text(RemoteSource::System, "Connection refused")],
        ..failure_of(FailureKind::ServiceUnreachable)
    });
    assert_eq!(declared.action, Some(FailureAction::Retry));
    assert_eq!(sources(&declared), [RemoteSource::System]);
}

#[test]
fn an_account_without_encryption_sends_to_online_accounts() {
    let declared = declare_failure(&failure_of(FailureKind::EncryptionNotConfigured));
    assert_eq!(declared.action, Some(FailureAction::OnlineAccounts));
    assert!(declared.advice.is_some());
}

#[test]
fn a_stopped_worker_offers_retry_and_asks_for_a_report() {
    let declared = declare_failure(&failure_of(FailureKind::Stopped));
    assert_eq!(declared.action, Some(FailureAction::Retry));
    assert!(declared.advice.is_some());
}

#[test]
fn a_short_list_carries_the_refusal_only_when_the_server_refused() {
    let refused = declare_short_list(&IncompleteList::ServerRefused {
        reply: "Too many messages for <login>".to_owned(),
        code: Some("LIMIT".to_owned()),
    });
    assert_eq!(refused.action, Some(FailureAction::Retry));
    assert_eq!(sources(&refused), [RemoteSource::ServerReply]);
    assert_eq!(refused.details, "Server code: LIMIT");

    let more = declare_short_list(&IncompleteList::MoreAvailable);
    assert_eq!(more.action, None);
    assert!(more.remote_texts.is_empty());
    assert!(more.details.is_empty());
}

#[test]
fn only_a_message_without_text_declares_a_failure() {
    assert_eq!(
        declare_content(&ReceivedContent::Text("Hello".to_owned())),
        None
    );
    let encrypted = declare_content(&ReceivedContent::Explained(ContentExplanation::Encrypted))
        .expect("an encrypted message has no text");
    assert_eq!(encrypted.action, None);
    let not_returned =
        declare_content(&ReceivedContent::TextNotReturned).expect("a missing text is a failure");
    assert_eq!(not_returned.action, Some(FailureAction::Retry));
}
