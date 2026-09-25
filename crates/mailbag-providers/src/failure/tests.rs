// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The technical details and the protocol facts the provider layer gives
//! about a failure.

use super::*;
use goa_adapter::AccessError;
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
    LoadFailure::MicrosoftGraph(GraphError {
        failure: GraphFailure::Refused {
            status,
            code: Some(code.to_owned()),
        },
        reason: Some("Service fault".to_owned()),
    })
}

#[test]
fn the_technical_details_carry_the_records_values_one_per_line() {
    let rejected = imap_failure(
        ImapFailure::Failed(ImapStep::SignIn),
        Some("AUTHENTICATIONFAILED"),
    );
    assert_eq!(
        rejected.technical_details(),
        "Failure: Failed(SignIn)\nServer code: AUTHENTICATIONFAILED"
    );
    assert_eq!(
        refused(500, "generalException").technical_details(),
        "Failure: Refused\nStatus: 500\nService code: generalException"
    );
    let panicked = LoadFailure::WorkerStopped(Some("boom at x.rs:1".to_owned()));
    assert_eq!(
        panicked.technical_details(),
        "Failure: WorkerStopped\nPanic: boom at x.rs:1"
    );
    let short = IncompleteList::ServerRefused(ServerReply {
        code: Some("LIMIT".to_owned()),
        text: String::new(),
    });
    assert_eq!(short.technical_details(), "Server code: LIMIT");
    assert_eq!(IncompleteList::MoreAvailable.technical_details(), "");
}

#[test]
fn only_a_code_that_blames_the_credentials_counts_as_a_rejected_credential() {
    let sign_in = ImapFailure::Failed(ImapStep::SignIn);
    for code in [
        None,
        Some("AUTHENTICATIONFAILED"),
        Some("authenticationfailed"),
    ] {
        assert!(
            imap_failure(sign_in, code).credentials_rejected(),
            "{code:?}"
        );
    }
    for code in [Some("UNAVAILABLE"), Some("LIMIT")] {
        assert!(
            !imap_failure(sign_in, code).credentials_rejected(),
            "{code:?}"
        );
    }
    // The same code at another step says nothing about the credentials.
    let open_inbox = imap_failure(ImapFailure::Failed(ImapStep::OpenInbox), None);
    assert!(!open_inbox.credentials_rejected());
    assert!(refused(401, "InvalidAuthenticationToken").credentials_rejected());
    assert!(!refused(500, "generalException").credentials_rejected());
    assert!(!LoadFailure::OnlineAccounts(AccessError::Password).credentials_rejected());
}

#[test]
fn only_an_imap_unavailable_code_is_a_temporary_outage() {
    for step in [ImapStep::SignIn, ImapStep::FetchMessages] {
        let failure = imap_failure(ImapFailure::Failed(step), Some("UNAVAILABLE"));
        assert!(failure.server_temporarily_unavailable(), "{step:?}");
    }
    let refused_sign_in = imap_failure(
        ImapFailure::Failed(ImapStep::SignIn),
        Some("AUTHENTICATIONFAILED"),
    );
    assert!(!refused_sign_in.server_temporarily_unavailable());
    assert!(!refused(503, "UNAVAILABLE").server_temporarily_unavailable());
}
