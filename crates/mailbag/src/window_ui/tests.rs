// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use mailbag_imap::ServerReply;

fn rejected_sign_in(code: Option<&str>, text: &str) -> LoadFailure {
    LoadFailure::Server(ServerFailure {
        failure: ImapFailure::Failed(ImapStep::SignIn),
        server_reply: Some(ServerReply {
            code: code.map(str::to_owned),
            text: text.to_owned(),
        }),
        alerts: Vec::new(),
    })
}

#[test]
fn every_failed_step_names_itself() {
    let cases = [
        (
            LoadFailure::OnlineAccounts(AccessError::Settings),
            "Account settings unavailable",
            "this account's settings",
        ),
        (
            LoadFailure::OnlineAccounts(AccessError::NoEncryption),
            "No encryption configured",
            "No password was requested",
        ),
        (
            LoadFailure::OnlineAccounts(AccessError::Password),
            "Password unavailable",
            "No server sign-in was attempted",
        ),
        (
            LoadFailure::OnlineAccounts(AccessError::AccessToken),
            "Authorization unavailable",
            "this account's authorization from Online Accounts",
        ),
        (
            LoadFailure::OnlineAccounts(AccessError::Timeout),
            "Online Accounts did not respond",
            "in time",
        ),
        (
            LoadFailure::Server(ImapFailure::Failed(ImapStep::Connect).into()),
            "Unable to reach the mail server",
            "could not reach",
        ),
        (
            LoadFailure::Server(ImapFailure::Failed(ImapStep::SecureConnection).into()),
            "Secure connection failed",
            "sent no password",
        ),
        (
            LoadFailure::Server(ImapFailure::Failed(ImapStep::OpenInbox).into()),
            "Unable to open the Inbox",
            "did not open the Inbox",
        ),
        (
            LoadFailure::Server(ImapFailure::Failed(ImapStep::FetchMessages).into()),
            "Unable to get the message list",
            "did not return this Inbox's messages",
        ),
        (
            LoadFailure::Server(ImapFailure::Failed(ImapStep::FetchText).into()),
            "Unable to get the message text",
            "did not return the text",
        ),
        (
            LoadFailure::Server(ImapFailure::TimedOut(ImapStep::OpenInbox).into()),
            "The mail server stopped responding",
            "while opening the Inbox",
        ),
        (
            LoadFailure::Server(ImapFailure::NoSignInMethod.into()),
            "No supported sign-in method",
            "no sign-in method Mailbag supports",
        ),
        (
            LoadFailure::Server(ImapFailure::InboxChanged.into()),
            "The Inbox changed while loading",
            "Try Refresh Inbox again",
        ),
        (
            LoadFailure::WorkerStopped,
            "Mail could not be loaded",
            "Try Refresh Inbox again",
        ),
    ];
    for (failure, title, explanation) in cases {
        let status = failure_status(&failure);
        assert_eq!(status.title, title, "{failure:?}");
        assert!(
            status.explanation.contains(explanation),
            "{failure:?}: {}",
            status.explanation
        );
    }
}

#[test]
fn a_rejected_sign_in_points_to_the_sign_in_only_when_the_server_blames_it() {
    for code in [None, Some("authenticationfailed")] {
        let status = failure_status(&rejected_sign_in(code, "Invalid credentials"));
        // One sentence for every provider: a Google account has no password
        // to change (specs/004-gmail-integration/research.md §9).
        assert!(
            status
                .explanation
                .contains("Check this account's sign-in in Online Accounts."),
            "{code:?}: {}",
            status.explanation
        );
        assert!(status.explanation.contains("Invalid credentials"));
    }
    // A temporary server problem says nothing about the credential.
    let status = failure_status(&rejected_sign_in(
        Some("UNAVAILABLE"),
        "Service temporarily unavailable",
    ));
    assert!(
        !status.explanation.contains("password"),
        "{}",
        status.explanation
    );
    assert!(
        status
            .explanation
            .contains("Service temporarily unavailable"),
        "{}",
        status.explanation
    );
}

#[test]
fn server_and_alert_text_reach_the_page_as_bounded_plain_text() {
    let failure = LoadFailure::Server(ServerFailure {
        failure: ImapFailure::Failed(ImapStep::OpenInbox),
        server_reply: Some(ServerReply {
            code: None,
            text: format!("<b>{}</b>\0", "долгая причина".repeat(10_000)),
        }),
        alerts: vec!["Alert <i>text</i>".to_owned()],
    });
    let explanation = failure_status(&failure).explanation;
    assert!(
        explanation.contains("<b>долгая причина"),
        "{explanation:.80}"
    );
    assert!(explanation.contains("Alert <i>text</i>"));
    assert!(!explanation.contains('\0'));
    // Only the server's own text is bounded; the step sentences stay whole.
    assert!(explanation.len() < 66_000, "{}", explanation.len());
}

#[test]
fn further_messages_on_offer_are_noticed_without_claiming_a_failure() {
    assert_eq!(
        incomplete_list_notice(Some("Work".to_owned()), &IncompleteList::MoreAvailable),
        "Not all messages in Work were loaded: the mail service offered more than one request holds."
    );
}

#[test]
fn an_account_without_mail_never_claims_an_empty_inbox() {
    // Both loadable providers get the hint; the others are told plainly that
    // Mailbag cannot load their mail yet.
    for provider in [AccountProvider::ImapSmtp, AccountProvider::Google] {
        let status = nothing_loaded_status(Some(provider));
        assert_eq!(status.title, "No mail loaded");
        assert!(
            status.explanation.contains("Refresh Inbox"),
            "{provider:?}: {}",
            status.explanation
        );
    }
    for provider in [AccountProvider::Microsoft365, AccountProvider::Other] {
        let status = nothing_loaded_status(Some(provider));
        assert_eq!(status.title, "No mail loaded");
        assert!(
            !status.explanation.contains("Refresh Inbox"),
            "{provider:?}: {}",
            status.explanation
        );
    }
}

/// The one place a provider becomes a load sequence.
#[test]
fn only_generic_imap_and_google_accounts_can_be_loaded() {
    use crate::accounts::mail_provider;
    assert_eq!(
        mail_provider(AccountProvider::ImapSmtp),
        Some(MailProvider::GenericImap)
    );
    assert_eq!(
        mail_provider(AccountProvider::Google),
        Some(MailProvider::Gmail)
    );
    for provider in [AccountProvider::Microsoft365, AccountProvider::Other] {
        assert_eq!(mail_provider(provider), None, "{provider:?}");
    }
}
