// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! How a load that failed is handed to the application: as the domain's
//! `Failure`, whose kind, remote texts and technical details are read here
//! from the protocol values only this layer knows
//! (specs/006-error-handling/contracts/failure-declaration.md). The layer that
//! gives the load up also writes its error line (specs/003-logging FR-004).
//! The wording for the user is written by the application.

#[cfg(test)]
mod tests;

use crate::{LoadFailure, LoadResult};
use goa_adapter::AccessError;
use mailbag_domain::{AccountId, Failure, FailureKind, RemoteSource, RemoteText, ServerStep};
use mailbag_graph::{GraphError, GraphFailure};
use mailbag_imap::{ImapFailure, ImapStep};

impl LoadFailure {
    /// Ends the load: writes its one error line and hands the failure on.
    pub(crate) fn give_up(self, account: &AccountId) -> LoadResult {
        let alerts = match &self {
            Self::Imap(error) => error.alerts.len(),
            _ => 0,
        };
        log_load_failure(
            account,
            self.failure_kind(),
            self.status(),
            self.server_code(),
            alerts,
        );
        LoadResult::Failed(self.into_failure())
    }

    /// The failure in the domain's terms.
    pub(crate) fn into_failure(self) -> Failure {
        if let Self::WorkerStopped(panic) = self {
            return Failure::stopped(panic);
        }
        Failure {
            kind: self.failure_kind(),
            remote_texts: self.remote_texts(),
            details: self.technical_details(),
        }
    }

    /// What went wrong in the application's terms. The protocol's codes are
    /// read here: whether the server is only temporarily unavailable, and
    /// whether a rejected sign-in blames the credentials.
    fn failure_kind(&self) -> FailureKind {
        match self {
            Self::OnlineAccounts(error) => match error {
                AccessError::Settings => FailureKind::AccountSettingsUnavailable,
                AccessError::NoEncryption => FailureKind::EncryptionNotConfigured,
                AccessError::Password => FailureKind::PasswordUnavailable,
                AccessError::AccessToken => FailureKind::AuthorizationUnavailable,
                AccessError::Timeout => FailureKind::OnlineAccountsNotResponding,
                AccessError::Cancelled => FailureKind::AccountRequestStopped,
            },
            Self::Imap(error) => match error.failure {
                ImapFailure::Failed(step) | ImapFailure::TimedOut(step)
                    if self.server_temporarily_unavailable() =>
                {
                    FailureKind::ServerUnavailable(server_step(step))
                }
                _ if self.credentials_rejected() => FailureKind::ServerRejectedSignIn,
                ImapFailure::Failed(step) => FailureKind::ServerStepFailed(server_step(step)),
                ImapFailure::TimedOut(step) => FailureKind::ServerNotResponding(server_step(step)),
                ImapFailure::NoSignInMethod => FailureKind::NoSignInMethod,
                ImapFailure::InboxChanged => FailureKind::InboxChanged,
            },
            Self::MicrosoftGraph(error) => match error.failure {
                GraphFailure::ConnectionFailed => FailureKind::ServiceUnreachable,
                GraphFailure::TimedOut => FailureKind::ServiceNotResponding,
                GraphFailure::Refused { .. } if self.credentials_rejected() => {
                    FailureKind::ServiceRejectedSignIn
                }
                GraphFailure::Refused { .. } => FailureKind::RequestRefused,
                GraphFailure::InvalidReply => FailureKind::UnexpectedAnswer,
            },
            Self::WorkerStopped(_) => FailureKind::Stopped,
        }
    }

    /// What the remote side said: an IMAP server's alerts, which RFC 3501
    /// requires the user to see, then its reply; the mail service's message
    /// after a refusal, or the platform's text about a failed connection.
    fn remote_texts(&self) -> Vec<RemoteText> {
        let remote_text = |source, text: &String| RemoteText {
            source,
            text: text.clone(),
        };
        match self {
            Self::Imap(error) => {
                let alerts = error
                    .alerts
                    .iter()
                    .map(|alert| remote_text(RemoteSource::ServerAlert, alert));
                let reply = error
                    .server_reply
                    .iter()
                    .map(|reply| remote_text(RemoteSource::ServerReply, &reply.text));
                alerts.chain(reply).collect()
            }
            Self::MicrosoftGraph(error) => {
                let source = match error.failure {
                    GraphFailure::Refused { .. } => RemoteSource::ServiceMessage,
                    _ => RemoteSource::System,
                };
                error
                    .reason
                    .iter()
                    .map(|reason| remote_text(source, reason))
                    .collect()
            }
            Self::OnlineAccounts(_) | Self::WorkerStopped(_) => Vec::new(),
        }
    }

    /// The mail service's status for a refused request.
    fn status(&self) -> Option<u32> {
        match self {
            Self::MicrosoftGraph(GraphError {
                failure: GraphFailure::Refused { status, .. },
                ..
            }) => Some(*status),
            _ => None,
        }
    }

    /// The server's or the service's machine-readable error code.
    fn server_code(&self) -> Option<&str> {
        match self {
            Self::Imap(error) => error.server_reply.as_ref()?.code.as_deref(),
            Self::MicrosoftGraph(GraphError {
                failure: GraphFailure::Refused { code, .. },
                ..
            }) => code.as_deref(),
            _ => None,
        }
    }

    /// The kind and the values the record's error line carries, one `Label:
    /// value` per line, in English: they identify the failure in a report.
    fn technical_details(&self) -> String {
        let mut lines = vec![format!("Failure: {:?}", self.failure_kind())];
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
        lines.join("\n")
    }

    /// Whether the server or the service rejected the sign-in because of the
    /// credentials, as far as its answer tells: an IMAP sign-in refused with
    /// `AUTHENTICATIONFAILED` or without a code, or the mail service's status
    /// 401. Another code, such as a temporary `UNAVAILABLE`, says nothing
    /// about the credentials.
    fn credentials_rejected(&self) -> bool {
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
    fn server_temporarily_unavailable(&self) -> bool {
        matches!(self, Self::Imap(_))
            && self
                .server_code()
                .is_some_and(|code| code.eq_ignore_ascii_case("UNAVAILABLE"))
    }
}

/// The domain's name for an IMAP step.
fn server_step(step: ImapStep) -> ServerStep {
    match step {
        ImapStep::Connect => ServerStep::Connect,
        ImapStep::SecureConnection => ServerStep::SecureConnection,
        ImapStep::SignIn => ServerStep::SignIn,
        ImapStep::OpenInbox => ServerStep::OpenInbox,
        ImapStep::FetchMessages => ServerStep::FetchMessages,
        ImapStep::FetchText => ServerStep::FetchText,
    }
}

/// The single error line of a load that was given up: the failure's kind, the
/// mail service's status, the server's or the service's error code and the
/// number of alerts, never the server's text.
pub(crate) fn log_load_failure(
    account: &AccountId,
    kind: FailureKind,
    status: Option<u32>,
    code: Option<&str>,
    alerts: usize,
) {
    tracing::error!(
        account = account.as_str(),
        // Named by the domain's enumeration, never by server text, so it is
        // written without quotes, as the record has always named it.
        cause = ?kind,
        status,
        code,
        alerts = Some(alerts).filter(|alerts| *alerts > 0),
        "Inbox load failed"
    );
}
