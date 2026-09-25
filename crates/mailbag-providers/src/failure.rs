// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the user is told about each failure a load can end with: its title,
//! explanation, advice, action, the remote side's texts and the technical
//! details (specs/006-error-handling/contracts/failure-declaration.md). The
//! window chooses where to show it; the wording lives here.

#[cfg(test)]
mod tests;

use crate::{IncompleteList, LoadFailure, ReceivedContent};
use goa_adapter::AccessError;
use mailbag_content::ContentExplanation;
use mailbag_graph::{GraphError, GraphFailure};
use mailbag_imap::{ImapError, ImapFailure, ImapStep};

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
    /// Runs the failed operation again: the window's refresh action.
    Retry,
    /// Opens the system's Online Accounts settings.
    OnlineAccounts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteText {
    /// The block's heading, such as "Reply from the mail server".
    pub source: &'static str,
    /// The text as received, with `<login>` in place of the sign-in name.
    pub text: String,
}

const ALERT_FROM_SERVER: &str = "Alert from the mail server";
const REPLY_FROM_SERVER: &str = "Reply from the mail server";
const MESSAGE_FROM_SERVICE: &str = "Message from the mail service";
const FROM_SYSTEM: &str = "From the system";

/// Where a rejected sign-in sends the user, for every provider.
const CHECK_SIGN_IN: &str = "Check this account's sign-in in Online Accounts, then choose \
                             Refresh Inbox.";

impl LoadFailure {
    /// The failure of a load that delivered no mail.
    pub fn declare(&self) -> DeclaredFailure {
        let mut declared = match self {
            Self::OnlineAccounts(error) => declare_access_failure(*error),
            Self::Imap(error) => declare_imap_failure(error),
            Self::MicrosoftGraph(error) => declare_graph_failure(error),
            Self::WorkerStopped(_) => declare_worker_stopped(),
        };
        declared.details = self.technical_details();
        declared
    }

    /// The failure value the record's error line names, such as
    /// `Failed(SignIn)`, `Refused` or `WorkerStopped`. The values hold no
    /// server text, so they are named as they are.
    pub fn cause_name(&self) -> String {
        match self {
            Self::OnlineAccounts(error) => format!("{error:?}"),
            Self::Imap(error) => format!("{:?}", error.failure),
            // The status and the code are named on their own.
            Self::MicrosoftGraph(GraphError {
                failure: GraphFailure::Refused { .. },
                ..
            }) => "Refused".to_owned(),
            Self::MicrosoftGraph(error) => format!("{:?}", error.failure),
            Self::WorkerStopped(_) => "WorkerStopped".to_owned(),
        }
    }

    /// The mail service's status for a refused request.
    pub fn status(&self) -> Option<u32> {
        match self {
            Self::MicrosoftGraph(GraphError {
                failure: GraphFailure::Refused { status, .. },
                ..
            }) => Some(*status),
            _ => None,
        }
    }

    /// The server's or the service's machine-readable error code.
    pub fn server_code(&self) -> Option<&str> {
        match self {
            Self::Imap(error) => error.server_reply.as_ref()?.code.as_deref(),
            Self::MicrosoftGraph(GraphError {
                failure: GraphFailure::Refused { code, .. },
                ..
            }) => code.as_deref(),
            _ => None,
        }
    }

    /// The same values the record's error line carries, one per line.
    fn technical_details(&self) -> String {
        let mut lines = vec![format!("Failure: {}", self.cause_name())];
        if let Some(status) = self.status() {
            lines.push(format!("Status: {status}"));
        }
        if let Some(code) = self.server_code() {
            let label = match self {
                Self::MicrosoftGraph(_) => "Service code",
                _ => "Server code",
            };
            lines.push(format!("{label}: {code}"));
        }
        if let Self::WorkerStopped(Some(panic)) = self {
            lines.push(format!("Panic: {panic}"));
        }
        lines.join("\n")
    }
}

impl IncompleteList {
    /// Why the list on screen holds fewer messages than the Inbox offered.
    pub fn declare(&self) -> DeclaredFailure {
        match self {
            Self::ServerRefused(reply) => DeclaredFailure {
                title: "Some messages not loaded",
                explanation: "The mail server stopped sending the message list, so some \
                              messages are missing."
                    .to_owned(),
                advice: None,
                action: Some(FailureAction::Retry),
                remote_texts: vec![RemoteText {
                    source: REPLY_FROM_SERVER,
                    text: reply.text.clone(),
                }],
                details: reply
                    .code
                    .as_ref()
                    .map(|code| format!("Server code: {code}"))
                    .unwrap_or_default(),
            },
            Self::MoreAvailable => DeclaredFailure {
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
}

impl ReceivedContent {
    /// Why the reader shows no text for this message; `None` for a text.
    pub fn declare(&self) -> Option<DeclaredFailure> {
        let (title, explanation, action) = match self {
            Self::Text(_) => return None,
            Self::Explained(explanation) => {
                let (title, explanation) = explain_content(explanation);
                (title, explanation, None)
            }
            Self::StructureUnreadable => (
                "Message unreadable",
                "The mail server could not describe this message, so its content could not be \
                 read."
                    .to_owned(),
                None,
            ),
            Self::TextNotReturned => (
                "Text not received",
                "This message's text was not received.".to_owned(),
                Some(FailureAction::Retry),
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

/// Online Accounts did not give what the load needs; no server was asked.
fn declare_access_failure(error: AccessError) -> DeclaredFailure {
    let (title, explanation, advice, action) = match error {
        AccessError::Settings => (
            "Account settings unavailable",
            "This account's settings could not be read from Online Accounts.",
            Some("Check this account in Online Accounts, then choose Refresh Inbox."),
            FailureAction::OnlineAccounts,
        ),
        AccessError::NoEncryption => (
            "No encryption configured",
            "This account has no encryption configured, so no password was requested and no \
             connection was made.",
            Some(
                "Choose SSL or STARTTLS for this account in Online Accounts, then choose \
                  Refresh Inbox.",
            ),
            FailureAction::OnlineAccounts,
        ),
        AccessError::Password => (
            "Password unavailable",
            "This account's password could not be read from Online Accounts, so no sign-in \
             was attempted.",
            Some(CHECK_SIGN_IN),
            FailureAction::OnlineAccounts,
        ),
        AccessError::AccessToken => (
            "Authorization unavailable",
            "This account's authorization could not be read from Online Accounts, so no \
             sign-in was attempted.",
            Some(CHECK_SIGN_IN),
            FailureAction::OnlineAccounts,
        ),
        AccessError::Timeout => (
            "Online Accounts not responding",
            "Online Accounts did not answer in time, so no sign-in was attempted.",
            None,
            FailureAction::Retry,
        ),
        // The load reports a cancellation instead (FR-010); this arm keeps
        // the declaration total.
        AccessError::Cancelled => (
            "Loading stopped",
            "Loading this Inbox stopped before it finished.",
            None,
            FailureAction::Retry,
        ),
    };
    DeclaredFailure {
        title,
        explanation: explanation.to_owned(),
        advice,
        action: Some(action),
        remote_texts: Vec::new(),
        details: String::new(),
    }
}

fn declare_imap_failure(error: &ImapError) -> DeclaredFailure {
    let server_code = error
        .server_reply
        .as_ref()
        .and_then(|reply| reply.code.as_deref());
    let (title, explanation) = match error.failure {
        // A temporary outage, whatever step met it (RFC 5530).
        _ if server_code.is_some_and(|code| code.eq_ignore_ascii_case("UNAVAILABLE")) => (
            "Server unavailable",
            "The mail server is temporarily unavailable.",
        ),
        ImapFailure::Failed(step) => (failed_step_title(step), failed_step_explanation(step)),
        ImapFailure::TimedOut(step) => ("Server not responding", waiting_step_explanation(step)),
        ImapFailure::NoSignInMethod => (
            "No sign-in method",
            "The mail server offers no supported sign-in method, so no password was sent.",
        ),
        ImapFailure::InboxChanged => (
            "Inbox changed",
            "The messages being loaded are no longer in this Inbox.",
        ),
    };
    let (action, advice) = match error.failure {
        _ if credential_may_be_wrong(error) => {
            (Some(FailureAction::OnlineAccounts), Some(CHECK_SIGN_IN))
        }
        // Repeating meets the same certificate or the same server offer.
        ImapFailure::Failed(ImapStep::SecureConnection) | ImapFailure::NoSignInMethod => {
            (None, None)
        }
        _ => (Some(FailureAction::Retry), None),
    };
    // An alert is what RFC 3501 requires the user to see, so it comes first.
    let alerts = error.alerts.iter().map(|alert| RemoteText {
        source: ALERT_FROM_SERVER,
        text: alert.clone(),
    });
    let reply = error.server_reply.iter().map(|reply| RemoteText {
        source: REPLY_FROM_SERVER,
        text: reply.text.clone(),
    });
    DeclaredFailure {
        title,
        explanation: explanation.to_owned(),
        advice,
        action,
        remote_texts: alerts.chain(reply).collect(),
        details: String::new(),
    }
}

/// A rejected sign-in points to the sign-in only when the server blamed the
/// credentials or gave no code; another code, such as a temporary
/// UNAVAILABLE, says nothing about the credential.
fn credential_may_be_wrong(error: &ImapError) -> bool {
    if error.failure != ImapFailure::Failed(ImapStep::SignIn) {
        return false;
    }
    match error
        .server_reply
        .as_ref()
        .and_then(|reply| reply.code.as_deref())
    {
        None => true,
        Some(code) => code.eq_ignore_ascii_case("AUTHENTICATIONFAILED"),
    }
}

fn failed_step_title(step: ImapStep) -> &'static str {
    match step {
        ImapStep::Connect => "Server unreachable",
        ImapStep::SecureConnection => "Secure connection failed",
        ImapStep::SignIn => "Sign-in rejected",
        ImapStep::OpenInbox => "Inbox not opened",
        ImapStep::FetchMessages => "Message list not received",
        ImapStep::FetchText => "Message text not received",
    }
}

fn failed_step_explanation(step: ImapStep) -> &'static str {
    match step {
        ImapStep::Connect => "The mail server could not be reached.",
        ImapStep::SecureConnection => {
            "A verified encrypted connection to the mail server could not be established, so \
             no password was sent."
        }
        ImapStep::SignIn => "The mail server rejected sign-in.",
        ImapStep::OpenInbox => "The mail server did not open the Inbox.",
        ImapStep::FetchMessages => "The mail server did not send this Inbox's messages.",
        ImapStep::FetchText => "The mail server did not send the text of these messages.",
    }
}

fn waiting_step_explanation(step: ImapStep) -> &'static str {
    match step {
        ImapStep::Connect => "The mail server did not answer the connection.",
        ImapStep::SecureConnection => {
            "The mail server stopped responding while the encrypted connection was being set up."
        }
        ImapStep::SignIn => "The mail server stopped responding during sign-in.",
        ImapStep::OpenInbox => "The mail server stopped responding while opening the Inbox.",
        ImapStep::FetchMessages => {
            "The mail server stopped responding while sending this Inbox's messages."
        }
        ImapStep::FetchText => "The mail server stopped responding while sending the message text.",
    }
}

fn declare_graph_failure(error: &GraphError) -> DeclaredFailure {
    let (title, explanation, advice, action) = match &error.failure {
        GraphFailure::ConnectionFailed => (
            "Service unreachable",
            "The mail service could not be reached.",
            None,
            FailureAction::Retry,
        ),
        GraphFailure::TimedOut => (
            "Service not responding",
            "The mail service stopped responding.",
            None,
            FailureAction::Retry,
        ),
        GraphFailure::Refused { status: 401, .. } => (
            "Sign-in rejected",
            "The mail service rejected sign-in.",
            Some(CHECK_SIGN_IN),
            FailureAction::OnlineAccounts,
        ),
        // Any other status: the general arm, with the status in the details.
        GraphFailure::Refused { .. } => (
            "Request failed",
            "The mail service refused the request.",
            None,
            FailureAction::Retry,
        ),
        GraphFailure::InvalidReply => (
            "Unexpected answer",
            "The mail service answered in an unexpected form.",
            None,
            FailureAction::Retry,
        ),
    };
    // A refusal carries the service's own message; any other reason is the
    // platform's text about the connection.
    let source = match error.failure {
        GraphFailure::Refused { .. } => MESSAGE_FROM_SERVICE,
        _ => FROM_SYSTEM,
    };
    DeclaredFailure {
        title,
        explanation: explanation.to_owned(),
        advice,
        action: Some(action),
        remote_texts: error
            .reason
            .iter()
            .map(|text| RemoteText {
                source,
                text: text.clone(),
            })
            .collect(),
        details: String::new(),
    }
}

fn declare_worker_stopped() -> DeclaredFailure {
    DeclaredFailure {
        title: "Refresh stopped",
        explanation: "Loading this Inbox stopped because of an internal error.".to_owned(),
        advice: Some("If this happens again, report it with the technical details."),
        action: Some(FailureAction::Retry),
        remote_texts: Vec::new(),
        details: String::new(),
    }
}
