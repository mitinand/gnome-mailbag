// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! What only the provider layer knows about a failure beyond its value: the
//! technical details for a report, the values the record's error line names,
//! and what a protocol's codes mean
//! (specs/006-error-handling/contracts/failure-declaration.md). The wording
//! for the user is written by the application.

#[cfg(test)]
mod tests;

use crate::{IncompleteList, LoadFailure};
use mailbag_graph::{GraphError, GraphFailure};
use mailbag_imap::{ImapFailure, ImapStep};

impl LoadFailure {
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

    /// The same values the record's error line carries, one `Label: value`
    /// per line, in English: they identify the failure in a report.
    pub fn technical_details(&self) -> String {
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

    /// Whether the server or the service rejected the sign-in because of the
    /// credentials, as far as its answer tells: an IMAP sign-in refused with
    /// `AUTHENTICATIONFAILED` or without a code, or the mail service's status
    /// 401. Another code, such as a temporary `UNAVAILABLE`, says nothing
    /// about the credentials.
    pub fn credentials_rejected(&self) -> bool {
        match self {
            Self::Imap(error) => {
                error.failure == ImapFailure::Failed(ImapStep::SignIn)
                    && self
                        .server_code()
                        .is_none_or(|code| code.eq_ignore_ascii_case("AUTHENTICATIONFAILED"))
            }
            Self::MicrosoftGraph(error) => {
                matches!(error.failure, GraphFailure::Refused { status: 401, .. })
            }
            Self::OnlineAccounts(_) | Self::WorkerStopped(_) => false,
        }
    }

    /// Whether the IMAP server said it is temporarily unavailable (RFC 5530
    /// `UNAVAILABLE`), whatever step met it.
    pub fn server_temporarily_unavailable(&self) -> bool {
        matches!(self, Self::Imap(_))
            && self
                .server_code()
                .is_some_and(|code| code.eq_ignore_ascii_case("UNAVAILABLE"))
    }
}

impl IncompleteList {
    /// The refusal's server code as a `Server code:` line, or nothing.
    pub fn technical_details(&self) -> String {
        match self {
            Self::ServerRefused(reply) => reply
                .code
                .as_ref()
                .map(|code| format!("Server code: {code}"))
                .unwrap_or_default(),
            Self::MoreAvailable => String::new(),
        }
    }
}
