// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The failure the provider layer hands on: the kind of every value, the
//! remote texts in their order, the technical details, and the one error
//! line of a load it gives up.

use super::*;
use crate::test_record::CapturedRecord;
use mailbag_imap::{ImapError, ServerReply};

fn imap_failure(failure: ImapFailure, code: Option<&str>) -> LoadFailure {
    LoadFailure::Imap(ImapError {
        failure,
        server_reply: Some(ServerReply {
            code: code.map(str::to_owned),
            text: "<login> may not sign in now".to_owned(),
        }),
        alerts: Vec::new(),
    })
}

fn refused(status: u32, code: &str) -> LoadFailure {
    graph_failure(GraphFailure::Refused {
        status,
        code: Some(code.to_owned()),
    })
}

fn graph_failure(failure: GraphFailure) -> LoadFailure {
    LoadFailure::MicrosoftGraph(GraphError {
        failure,
        reason: Some("Service fault".to_owned()),
    })
}

#[test]
fn every_failure_value_has_its_kind() {
    use FailureKind as Kind;
    use ImapFailure::{Failed, TimedOut};
    let cases = [
        // Only a code that blames the credentials, or none, rejects them.
        (
            imap_failure(Failed(ImapStep::SignIn), Some("AUTHENTICATIONFAILED")),
            Kind::ServerRejectedSignIn,
        ),
        (
            imap_failure(Failed(ImapStep::SignIn), Some("authenticationfailed")),
            Kind::ServerRejectedSignIn,
        ),
        (
            imap_failure(Failed(ImapStep::SignIn), None),
            Kind::ServerRejectedSignIn,
        ),
        (
            imap_failure(Failed(ImapStep::SignIn), Some("UNAVAILABLE")),
            Kind::ServerUnavailable(ServerStep::SignIn),
        ),
        (
            imap_failure(Failed(ImapStep::SignIn), Some("LIMIT")),
            Kind::ServerStepFailed(ServerStep::SignIn),
        ),
        // The same codes at another step say nothing about the credentials.
        (
            imap_failure(Failed(ImapStep::OpenInbox), Some("AUTHENTICATIONFAILED")),
            Kind::ServerStepFailed(ServerStep::OpenInbox),
        ),
        (
            imap_failure(Failed(ImapStep::FetchMessages), Some("UNAVAILABLE")),
            Kind::ServerUnavailable(ServerStep::FetchMessages),
        ),
        (
            imap_failure(TimedOut(ImapStep::SignIn), None),
            Kind::ServerNotResponding(ServerStep::SignIn),
        ),
        (
            imap_failure(TimedOut(ImapStep::Connect), Some("UNAVAILABLE")),
            Kind::ServerUnavailable(ServerStep::Connect),
        ),
        (
            imap_failure(ImapFailure::NoSignInMethod, None),
            Kind::NoSignInMethod,
        ),
        (
            imap_failure(ImapFailure::InboxChanged, None),
            Kind::InboxChanged,
        ),
        (
            refused(401, "InvalidAuthenticationToken"),
            Kind::ServiceRejectedSignIn,
        ),
        (refused(500, "generalException"), Kind::RequestRefused),
        // A service's `UNAVAILABLE` is not an IMAP server's.
        (refused(503, "UNAVAILABLE"), Kind::RequestRefused),
        (
            graph_failure(GraphFailure::ConnectionFailed),
            Kind::ServiceUnreachable,
        ),
        (
            graph_failure(GraphFailure::TimedOut),
            Kind::ServiceNotResponding,
        ),
        (
            graph_failure(GraphFailure::InvalidReply),
            Kind::UnexpectedAnswer,
        ),
        (LoadFailure::WorkerStopped(None), Kind::Stopped),
    ];
    for (failure, kind) in cases {
        assert_eq!(failure.clone().into_failure().kind, kind, "{failure:?}");
    }
}

#[test]
fn the_remote_texts_come_in_the_order_they_are_shown() {
    let rejected = LoadFailure::Imap(ImapError {
        failure: ImapFailure::Failed(ImapStep::SignIn),
        server_reply: Some(ServerReply {
            code: Some("AUTHENTICATIONFAILED".to_owned()),
            text: "<login> may not sign in now".to_owned(),
        }),
        alerts: vec!["Password for <login> expired".to_owned()],
    });
    let texts = |failure: LoadFailure| -> Vec<(RemoteSource, String)> {
        let failure = failure.into_failure();
        let texts = failure.remote_texts.into_iter();
        texts.map(|text| (text.source, text.text)).collect()
    };
    assert_eq!(
        texts(rejected),
        [
            (
                RemoteSource::ServerAlert,
                "Password for <login> expired".to_owned()
            ),
            (
                RemoteSource::ServerReply,
                "<login> may not sign in now".to_owned()
            ),
        ]
    );
    let service_text = |source| vec![(source, "Service fault".to_owned())];
    assert_eq!(
        texts(refused(500, "generalException")),
        service_text(RemoteSource::ServiceMessage)
    );
    assert_eq!(
        texts(graph_failure(GraphFailure::ConnectionFailed)),
        service_text(RemoteSource::System)
    );
    assert!(texts(LoadFailure::OnlineAccounts(AccessError::Timeout)).is_empty());
}

#[test]
fn the_technical_details_name_the_kind_and_the_records_values() {
    let details = |failure: LoadFailure| failure.into_failure().details;
    assert_eq!(
        details(imap_failure(
            ImapFailure::Failed(ImapStep::SignIn),
            Some("AUTHENTICATIONFAILED"),
        )),
        "Failure: ServerRejectedSignIn\nServer code: AUTHENTICATIONFAILED"
    );
    assert_eq!(
        details(imap_failure(ImapFailure::TimedOut(ImapStep::SignIn), None)),
        "Failure: ServerNotResponding(SignIn)"
    );
    assert_eq!(
        details(refused(500, "generalException")),
        "Failure: RequestRefused\nStatus: 500\nService code: generalException"
    );
    assert_eq!(
        details(LoadFailure::WorkerStopped(Some(
            "boom at x.rs:1".to_owned()
        ))),
        "Failure: Stopped\nPanic: boom at x.rs:1"
    );
}

#[test]
fn a_load_given_up_is_one_error_line_naming_its_kind() {
    let refused_sign_in = ImapError {
        failure: ImapFailure::Failed(ImapStep::SignIn),
        server_reply: Some(ServerReply {
            code: Some("AUTHENTICATIONFAILED".to_owned()),
            text: "private server text".to_owned(),
        }),
        alerts: vec!["private alert".to_owned()],
    };
    let failures = [
        (
            LoadFailure::Imap(refused_sign_in),
            r#"cause=ServerRejectedSignIn code="AUTHENTICATIONFAILED" alerts=1"#,
        ),
        (
            LoadFailure::Imap(ImapFailure::TimedOut(ImapStep::FetchText).into()),
            "cause=ServerNotResponding(FetchText)",
        ),
        (
            LoadFailure::OnlineAccounts(AccessError::Timeout),
            "cause=OnlineAccountsNotResponding",
        ),
        (
            LoadFailure::MicrosoftGraph(GraphError {
                failure: GraphFailure::Refused {
                    status: 401,
                    code: Some("InvalidAuthenticationToken".to_owned()),
                },
                reason: Some("private server text".to_owned()),
            }),
            r#"cause=ServiceRejectedSignIn status=401 code="InvalidAuthenticationToken""#,
        ),
        (LoadFailure::WorkerStopped(None), "cause=Stopped"),
    ];
    let account = AccountId::try_from("account_1726920000_1").expect("synthetic account id");
    for (failure, fields) in failures {
        let record = CapturedRecord::start(tracing::Level::DEBUG);
        let result = failure.give_up(&account);
        assert!(matches!(result, LoadResult::Failed(_)), "{result:?}");
        let text = record.text();
        let errors = record.lines_at("ERROR");
        assert_eq!(errors.len(), 1, "{text}");
        assert!(
            errors[0].contains("Inbox load failed")
                && errors[0].contains(r#"account="account_1726920000_1""#)
                && errors[0].contains(fields),
            "{fields}: {}",
            errors[0]
        );
        assert!(
            !text.contains("private") && !text.contains(" WARN "),
            "{text}"
        );
    }
}
