// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the user is told about each failure: its title, explanation, advice,
//! action and the remote side's texts under their headings, written once per
//! failure kind (specs/006-error-handling/contracts/failure-declaration.md).
//! The layer that met the failure gives its kind, the remote texts and the
//! technical details in the domain's terms; the window chooses where to show
//! it.

#[cfg(test)]
mod tests;

use crate::failure_dialog::RetriedOperation;
use mailbag_domain::{
    ContentExplanation, Failure, FailureKind, IncompleteList, ReceivedContent, RemoteSource,
    RemoteText, ServerStep,
};

/// One failure as the user sees it, whatever channel shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredFailure {
    /// Names what failed, in a few words: fits one banner line at the
    /// list pane's narrowest width.
    pub title: &'static str,
    /// What happened, in plain words. May carry a name from the message,
    /// such as a character set, as inert text. Never a code, a status, a
    /// protocol term or words of the server.
    pub explanation: String,
    /// What to do next, when there is a next step the action does not say.
    pub advice: Option<&'static str>,
    /// The one action that can change the outcome, or none.
    pub action: Option<FailureAction>,
    /// What the remote side said in words, in the order shown.
    pub remote_texts: Vec<RemoteText>,
    /// Identifiers for the maintainer, one `Label: value` per line; empty
    /// when the failure has nothing beyond its explanation.
    pub details: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureAction {
    /// Runs the failed operation again; the window chooses the operation from
    /// what carries the failure.
    Retry,
    /// Opens the system's Online Accounts settings.
    OnlineAccounts,
}

/// The words that name the failed load: the account's folder list when Retry
/// refreshes the account, otherwise the mailbox (specs/008-folders FR-011).
fn choose_by_load(
    retried: RetriedOperation,
    folder_list: &'static str,
    mailbox: &'static str,
) -> &'static str {
    match retried {
        RetriedOperation::RefreshAccount => folder_list,
        RetriedOperation::RefreshMailbox | RetriedOperation::ReadStoredMail => mailbox,
    }
}

/// The heading of a remote text's block: who said it.
pub fn remote_heading(source: RemoteSource) -> &'static str {
    match source {
        RemoteSource::ServerAlert => "Alert from the mail server",
        RemoteSource::ServerReply => "Reply from the mail server",
        RemoteSource::ServiceMessage => "Message from the mail service",
        RemoteSource::System => "From the system",
    }
}

/// The failure of an operation that delivered nothing, such as a load, which
/// `retried` repeats.
pub fn declare_failure(failure: &Failure, retried: RetriedOperation) -> DeclaredFailure {
    // Where a rejected sign-in sends the user, for every provider.
    let check_sign_in = choose_by_load(
        retried,
        "Check this account's sign-in in Online Accounts, then choose Refresh Account.",
        "Check this account's sign-in in Online Accounts, then choose Refresh Mailbox.",
    );
    let (title, explanation, advice, action) = match failure.kind {
        FailureKind::AccountSettingsUnavailable => (
            "Account settings unavailable",
            "This account's settings could not be read from Online Accounts.",
            Some(choose_by_load(
                retried,
                "Check this account in Online Accounts, then choose Refresh Account.",
                "Check this account in Online Accounts, then choose Refresh Mailbox.",
            )),
            Some(FailureAction::OnlineAccounts),
        ),
        FailureKind::EncryptionNotConfigured => (
            "No encryption configured",
            "This account has no encryption configured, so no password was requested and no \
             connection was made.",
            Some(choose_by_load(
                retried,
                "Choose SSL or STARTTLS for this account in Online Accounts, then choose \
                 Refresh Account.",
                "Choose SSL or STARTTLS for this account in Online Accounts, then choose \
                 Refresh Mailbox.",
            )),
            Some(FailureAction::OnlineAccounts),
        ),
        FailureKind::PasswordUnavailable => (
            "Password unavailable",
            "This account's password could not be read from Online Accounts, so no sign-in \
             was attempted.",
            Some(check_sign_in),
            Some(FailureAction::OnlineAccounts),
        ),
        FailureKind::AuthorizationUnavailable => (
            "Authorization unavailable",
            "This account's authorization could not be read from Online Accounts, so no \
             sign-in was attempted.",
            Some(check_sign_in),
            Some(FailureAction::OnlineAccounts),
        ),
        FailureKind::OnlineAccountsNotResponding => (
            "Online Accounts not responding",
            "Online Accounts did not answer in time, so no sign-in was attempted.",
            None,
            Some(FailureAction::Retry),
        ),
        // The load reports a cancellation instead (FR-010); this arm keeps
        // the declaration total.
        FailureKind::AccountRequestStopped => (
            "Loading stopped",
            choose_by_load(
                retried,
                "Loading the mailbox list stopped before it finished.",
                "Loading this mailbox stopped before it finished.",
            ),
            None,
            Some(FailureAction::Retry),
        ),
        FailureKind::ServerUnavailable(_) => (
            "Server unavailable",
            "The mail server is temporarily unavailable.",
            None,
            Some(FailureAction::Retry),
        ),
        FailureKind::ServerRejectedSignIn => (
            failed_step_title(ServerStep::SignIn),
            failed_step_explanation(ServerStep::SignIn),
            Some(check_sign_in),
            Some(FailureAction::OnlineAccounts),
        ),
        // A failed secure connection gets Retry: a refused certificate and a
        // handshake cut short arrive as the same failure.
        FailureKind::ServerStepFailed(step) => (
            failed_step_title(step),
            failed_step_explanation(step),
            None,
            Some(FailureAction::Retry),
        ),
        FailureKind::ServerNotResponding(step) => (
            "Server not responding",
            waiting_step_explanation(step),
            None,
            Some(FailureAction::Retry),
        ),
        // Repeating meets the same server offer.
        FailureKind::NoSignInMethod => (
            "No sign-in method",
            "The mail server offers no supported sign-in method, so no password was sent.",
            None,
            None,
        ),
        FailureKind::MailboxChanged => (
            "Mailbox changed",
            "The messages being loaded are no longer in this mailbox.",
            None,
            Some(FailureAction::Retry),
        ),
        FailureKind::ServiceUnreachable => (
            "Service unreachable",
            "The mail service could not be reached.",
            None,
            Some(FailureAction::Retry),
        ),
        FailureKind::ServiceNotResponding => (
            "Service not responding",
            choose_by_load(
                retried,
                "The mail service stopped responding while sending the mailbox list.",
                "The mail service stopped responding.",
            ),
            None,
            Some(FailureAction::Retry),
        ),
        FailureKind::ServiceRejectedSignIn => (
            "Sign-in rejected",
            "The mail service rejected sign-in.",
            Some(check_sign_in),
            Some(FailureAction::OnlineAccounts),
        ),
        // Any other status: the general arm, with the status in the details.
        FailureKind::RequestRefused => (
            "Request failed",
            choose_by_load(
                retried,
                "The mail service refused the request for the mailbox list.",
                "The mail service refused the request.",
            ),
            None,
            Some(FailureAction::Retry),
        ),
        FailureKind::UnexpectedAnswer => (
            "Unexpected answer",
            choose_by_load(
                retried,
                "The mail service sent the mailbox list in an unexpected form.",
                "The mail service answered in an unexpected form.",
            ),
            None,
            Some(FailureAction::Retry),
        ),
        FailureKind::Stopped => (
            "Refresh stopped",
            choose_by_load(
                retried,
                "Loading the mailbox list stopped because of an internal error.",
                "Loading this mailbox stopped because of an internal error.",
            ),
            Some("If this happens again, report it with the technical details."),
            Some(FailureAction::Retry),
        ),
        // A full disk can also meet a read: opening the store writes its files.
        FailureKind::StorageFull => (
            "Not enough disk space",
            "There is not enough free disk space to store mail.",
            Some("Free some disk space, then try again."),
            Some(FailureAction::Retry),
        ),
        FailureKind::MailNotSaved => (
            "Mail not saved",
            choose_by_load(
                retried,
                "The received mailbox list could not be saved.",
                "The received messages could not be saved.",
            ),
            None,
            Some(FailureAction::Retry),
        ),
        FailureKind::StoredMailUnreadable => (
            "Stored mail unreadable",
            "The stored mail could not be read.",
            None,
            Some(FailureAction::Retry),
        ),
    };
    DeclaredFailure {
        title,
        explanation: explanation.to_owned(),
        advice,
        action,
        remote_texts: failure.remote_texts.clone(),
        details: failure.details.clone(),
    }
}

/// Why the list on screen holds fewer messages than the mailbox offered.
pub fn declare_short_list(incomplete: &IncompleteList) -> DeclaredFailure {
    match incomplete {
        IncompleteList::ServerRefused { reply, .. } => DeclaredFailure {
            title: "Some messages not loaded",
            explanation: "The mail server stopped sending the message list, so some \
                          messages are missing."
                .to_owned(),
            advice: None,
            action: Some(FailureAction::Retry),
            remote_texts: vec![RemoteText {
                source: RemoteSource::ServerReply,
                text: reply.clone(),
            }],
            details: incomplete.technical_details(),
        },
        IncompleteList::MoreAvailable => DeclaredFailure {
            title: "Not all messages loaded",
            explanation: "The mail service offered more messages than one load brings. \
                          The newest are shown."
                .to_owned(),
            advice: None,
            action: None,
            remote_texts: Vec::new(),
            details: String::new(),
        },
    }
}

/// Why the reader shows no text for this message; `None` for a text.
pub fn declare_content(content: &ReceivedContent) -> Option<DeclaredFailure> {
    let (title, explanation, action) = match content {
        ReceivedContent::Text(_) => return None,
        ReceivedContent::Explained(explanation) => {
            let (title, explanation) = explain_content(explanation);
            (title, explanation, None)
        }
        ReceivedContent::StructureUnreadable => (
            "Message unreadable",
            "The mail server could not describe this message, so its content could not be \
             read."
                .to_owned(),
            None,
        ),
        // A refresh does not fetch a stored message's text again, so there is
        // nothing to repeat (specs/009-synchronization, Clarifications).
        ReceivedContent::TextNotReturned => (
            "Text not received",
            "This message's text was not received.".to_owned(),
            None,
        ),
        ReceivedContent::NotDownloaded => (
            "Text not downloaded",
            "The text of this message was not downloaded. Texts are downloaded for messages \
             received in the last 30 days."
                .to_owned(),
            None,
        ),
    };
    Some(DeclaredFailure {
        title,
        explanation,
        advice: None,
        action,
        remote_texts: Vec::new(),
        details: String::new(),
    })
}

/// The title and the explanation of content the rules found no text in.
fn explain_content(explanation: &ContentExplanation) -> (&'static str, String) {
    match explanation {
        ContentExplanation::NoPlainText { has_html: true } => (
            "HTML version only",
            "This message has only an HTML version, which cannot be shown yet.".to_owned(),
        ),
        ContentExplanation::NoPlainText { has_html: false } => {
            ("No text", "This message has no text to show.".to_owned())
        }
        ContentExplanation::Encrypted => (
            "Encrypted message",
            "This message is encrypted and cannot be decrypted.".to_owned(),
        ),
        ContentExplanation::SecuredWithSMime => (
            "Secured message",
            "This message is secured with S/MIME and cannot be read.".to_owned(),
        ),
        // The sender chose the name; the window shows it as inert text.
        ContentExplanation::UnknownCharset(charset) => (
            "Unknown character set",
            format!("This message uses an unknown character set: {charset}"),
        ),
        ContentExplanation::UnknownEncoding(encoding) => (
            "Unknown encoding",
            format!("This message's text uses an unknown encoding: {encoding}"),
        ),
        ContentExplanation::Undecodable => (
            "Text unreadable",
            "This message's text could not be read.".to_owned(),
        ),
    }
}

fn failed_step_title(step: ServerStep) -> &'static str {
    match step {
        ServerStep::Connect => "Server unreachable",
        ServerStep::SecureConnection => "Secure connection failed",
        ServerStep::SignIn => "Sign-in rejected",
        ServerStep::ListFolders => "Mailbox list not received",
        ServerStep::OpenMailbox => "Mailbox not opened",
        ServerStep::FetchMessages => "Message list not received",
        ServerStep::FetchText => "Message text not received",
    }
}

fn failed_step_explanation(step: ServerStep) -> &'static str {
    match step {
        ServerStep::Connect => "The mail server could not be reached.",
        ServerStep::SecureConnection => {
            "A verified encrypted connection to the mail server could not be established, so \
             no password was sent."
        }
        ServerStep::SignIn => "The mail server rejected sign-in.",
        ServerStep::ListFolders => "The mail server did not send the mailbox list.",
        ServerStep::OpenMailbox => "The mail server did not open this mailbox.",
        ServerStep::FetchMessages => "The mail server did not send this mailbox's messages.",
        ServerStep::FetchText => "The mail server did not send the text of these messages.",
    }
}

fn waiting_step_explanation(step: ServerStep) -> &'static str {
    match step {
        ServerStep::Connect => "The mail server did not answer the connection.",
        ServerStep::SecureConnection => {
            "The mail server stopped responding while the encrypted connection was being set up."
        }
        ServerStep::SignIn => "The mail server stopped responding during sign-in.",
        ServerStep::ListFolders => {
            "The mail server stopped responding while sending the mailbox list."
        }
        ServerStep::OpenMailbox => "The mail server stopped responding while opening this mailbox.",
        ServerStep::FetchMessages => {
            "The mail server stopped responding while sending this mailbox's messages."
        }
        ServerStep::FetchText => {
            "The mail server stopped responding while sending the message text."
        }
    }
}
