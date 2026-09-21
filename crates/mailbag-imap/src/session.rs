// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    Encryption, ImapAccount, ImapError, ImapFailure, ImapStep, ServerReply,
    transport::{self, GioStream, ServerConnection},
};
use async_imap::{
    Authenticator, Client, Session,
    error::{Error, StatusResponse},
    imap_proto::{Response, ResponseCode, Status},
    types::{Capabilities, Capability, UnsolicitedResponse},
};
use std::io;

/// Server text for a debug line: every occurrence of the sign-in name, in any
/// ASCII letter case and whatever its length, is replaced with `<login>`
/// (specs/003-logging/research.md §6). The rest of the text is kept as sent.
pub(crate) fn server_text_for_log(sign_in_name: &str, text: &str) -> String {
    let lowercase_text = text.to_ascii_lowercase();
    let lowercase_name = sign_in_name.to_ascii_lowercase();
    let mut logged_text = String::with_capacity(text.len());
    let mut copied = 0;
    // ASCII lowercasing keeps every byte position, so matches index `text`.
    for (start, _) in lowercase_text.match_indices(&lowercase_name) {
        logged_text.push_str(&text[copied..start]);
        logged_text.push_str("<login>");
        copied = start + lowercase_name.len();
    }
    logged_text.push_str(&text[copied..]);
    logged_text
}

/// A signed-in session with the Inbox open read-only.
pub(crate) struct InboxSession {
    pub(crate) session: Session<GioStream>,
    pub(crate) uid_validity: Option<u32>,
    pub(crate) message_count: u32,
    /// Dropped after `session`, closing the socket.
    pub(crate) connection: ServerConnection,
}

/// A failed step and the reason the server gave for it, if any.
pub(crate) struct StepFailure {
    pub(crate) failure: ImapFailure,
    pub(crate) server_reply: Option<ServerReply>,
}

impl From<ImapFailure> for StepFailure {
    fn from(failure: ImapFailure) -> Self {
        Self {
            failure,
            server_reply: None,
        }
    }
}

impl From<&StatusResponse> for ServerReply {
    fn from(status: &StatusResponse) -> Self {
        Self {
            code: status.code.clone(),
            text: status.text.clone(),
        }
    }
}

/// What the server said during an attempt that can explain its failure.
pub(crate) struct ServerNotices {
    /// Replaced with `<login>` wherever server text reaches the record.
    sign_in_name: String,
    /// ALERT texts, which RFC 3501 requires to reach the user.
    alerts: Vec<String>,
    /// The BYE with which the server closed the connection, for example
    /// when it shut down.
    bye: Option<ServerReply>,
}

impl ServerNotices {
    pub(crate) fn new(sign_in_name: &str) -> Self {
        Self {
            sign_in_name: sign_in_name.to_owned(),
            alerts: Vec::new(),
            bye: None,
        }
    }

    /// Keeps the ALERT and BYE texts among waiting unilateral responses.
    pub(crate) fn collect(
        &mut self,
        mut next_response: impl FnMut() -> Option<UnsolicitedResponse>,
    ) {
        while let Some(response) = next_response() {
            if let UnsolicitedResponse::Other(data) = response {
                self.keep(data.parsed());
            }
        }
    }

    fn keep(&mut self, response: &Response<'_>) {
        let (status, code, information) = match response {
            Response::Data {
                status,
                code,
                information,
            } => (Some(status), code, information),
            Response::Done {
                code, information, ..
            } => (None, code, information),
            _ => return,
        };
        let text = information.as_deref().unwrap_or_default();
        if matches!(code, Some(ResponseCode::Alert)) {
            tracing::info!("the server sent an alert");
            tracing::debug!(
                alert = server_text_for_log(&self.sign_in_name, text),
                "the server sent an alert"
            );
            self.alerts.push(text.to_owned());
        }
        if status == Some(&Status::Bye) {
            self.bye = Some(ServerReply {
                code: None,
                text: text.to_owned(),
            });
        }
    }

    /// The error for a failed step, with what the server said about it. Every
    /// failed step passes here, so the server's text is logged here as well.
    pub(crate) fn error(&mut self, failure: StepFailure) -> ImapError {
        let server_reply = failure.server_reply.or_else(|| self.bye.take());
        if let Some(reply) = &server_reply {
            tracing::debug!(
                code = reply.code.as_deref(),
                server_text = server_text_for_log(&self.sign_in_name, &reply.text),
                "the server's reply to the failed step"
            );
        }
        ImapError {
            failure: failure.failure,
            server_reply,
            alerts: std::mem::take(&mut self.alerts),
        }
    }
}

/// Connects securely, signs in and runs EXAMINE INBOX. ALERT and BYE texts
/// received over TLS are added to `notices`.
pub(crate) async fn open_inbox(
    account: &ImapAccount,
    socket_timeout_seconds: u32,
    notices: &mut ServerNotices,
) -> Result<InboxSession, StepFailure> {
    let (connection, identity) =
        transport::connect(&account.host, account.encryption, socket_timeout_seconds).await?;
    let client = match account.encryption {
        Encryption::ImplicitTls => {
            let tls = transport::start_tls(&connection, &identity, account.encryption).await?;
            let mut client = Client::new(GioStream::new(tls));
            read_greeting(&mut client, notices).await?;
            client
        }
        Encryption::StartTls => {
            upgrade_plaintext(&connection).await?;
            let tls = transport::start_tls(&connection, &identity, account.encryption).await?;
            // The server sends no second greeting after STARTTLS.
            Client::new(GioStream::new(tls))
        }
    };
    let mut session = sign_in(client, account, notices).await?;
    let examined = session.examine("INBOX").await;
    notices.collect(|| session.unsolicited_responses.try_recv().ok());
    let mailbox = examined.map_err(|error| command_failure(ImapStep::OpenInbox, &error))?;
    tracing::info!(messages = mailbox.exists, "Inbox opened");
    tracing::debug!(uid_validity = mailbox.uid_validity, "Inbox state");
    Ok(InboxSession {
        session,
        uid_validity: mailbox.uid_validity,
        message_count: mailbox.exists,
        connection,
    })
}

async fn read_greeting(
    client: &mut Client<GioStream>,
    notices: &mut ServerNotices,
) -> Result<(), StepFailure> {
    let greeting = client
        .read_response()
        .await
        .map_err(|error| io_failure(ImapStep::Connect, &error))?
        .ok_or(ImapFailure::Failed(ImapStep::Connect))?;
    notices.keep(greeting.parsed());
    match greeting.parsed() {
        Response::Data {
            status: Status::Ok, ..
        } => Ok(()),
        // BYE, for example at the server's connection limit, whose text the
        // notices keep; or PREAUTH, which needs no sign-in and is not supported.
        _ => Err(ImapFailure::Failed(ImapStep::Connect).into()),
    }
}

/// Runs STARTTLS on the unencrypted connection. Nothing received here is
/// trusted: a PREAUTH greeting could skip encryption, and bytes the server
/// sends after its STARTTLS reply are discarded with the plaintext client.
async fn upgrade_plaintext(connection: &ServerConnection) -> Result<(), StepFailure> {
    let mut plaintext = Client::new(GioStream::new(transport::plaintext_stream(connection)));
    let greeting = plaintext
        .read_response()
        .await
        .map_err(|error| io_failure(ImapStep::Connect, &error))?
        .ok_or(ImapFailure::Failed(ImapStep::Connect))?;
    match greeting.parsed() {
        Response::Data {
            status: Status::Ok, ..
        } => {}
        Response::Data {
            status: Status::PreAuth,
            ..
        } => return Err(ImapFailure::Failed(ImapStep::SecureConnection).into()),
        _ => return Err(ImapFailure::Failed(ImapStep::Connect).into()),
    }
    let capabilities = plaintext
        .capabilities()
        .await
        .map_err(|error| starttls_failure(&error))?;
    if !capabilities.has_str("STARTTLS") {
        return Err(ImapFailure::Failed(ImapStep::SecureConnection).into());
    }
    plaintext
        .run_command_and_check_ok("STARTTLS", None)
        .await
        .map_err(|error| starttls_failure(&error).into())
}

/// A failure to secure the connection, or a timeout. Server text received
/// before TLS may come from an attacker, so it is not kept.
fn starttls_failure(error: &Error) -> ImapFailure {
    match error {
        Error::Io(error) => io_failure(ImapStep::SecureConnection, error),
        _ => ImapFailure::Failed(ImapStep::SecureConnection),
    }
}

/// Signs in with AUTHENTICATE PLAIN when offered, otherwise LOGIN unless the
/// server disables it. A rejected sign-in is not retried with another method.
async fn sign_in(
    mut client: Client<GioStream>,
    account: &ImapAccount,
    notices: &mut ServerNotices,
) -> Result<Session<GioStream>, StepFailure> {
    let capabilities = client.capabilities().await;
    notices.collect(|| client.unsolicited_responses().try_recv().ok());
    let capabilities = capabilities.map_err(|error| command_failure(ImapStep::SignIn, &error))?;
    tracing::info!(
        capabilities = capability_names(&capabilities),
        "server capabilities"
    );
    let (method, signed_in) = if capabilities.has_str("AUTH=PLAIN") {
        let credentials = PlainCredentials {
            login: &account.login,
            password: &account.password,
            sent: false,
        };
        ("PLAIN", client.authenticate("PLAIN", credentials).await)
    } else if !capabilities.has_str("LOGINDISABLED") {
        // The fork sends a non-ASCII login or password as a literal.
        (
            "LOGIN",
            client.login(&account.login, &account.password).await,
        )
    } else {
        return Err(ImapFailure::NoSignInMethod.into());
    };
    match signed_in {
        Ok(session) => {
            notices.collect(|| session.unsolicited_responses.try_recv().ok());
            tracing::info!(method, "signed in");
            Ok(session)
        }
        Err((error, client)) => {
            notices.collect(|| client.unsolicited_responses().try_recv().ok());
            Err(command_failure(ImapStep::SignIn, &error))
        }
    }
}

/// The capability list as the server named it, for the record.
fn capability_names(capabilities: &Capabilities) -> String {
    capabilities
        .iter()
        .map(|capability| match capability {
            Capability::Imap4rev1 => "IMAP4rev1".to_owned(),
            Capability::Auth(mechanism) => format!("AUTH={mechanism}"),
            Capability::Atom(name) => name.clone(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The SASL PLAIN response. async-imap adds the base64 framing.
struct PlainCredentials<'a> {
    login: &'a str,
    password: &'a str,
    sent: bool,
}

impl Authenticator for PlainCredentials<'_> {
    type Response = Vec<u8>;

    // An unexpected second challenge gets an empty answer: credentials are sent once.
    fn process(&mut self, _challenge: &[u8]) -> Vec<u8> {
        if std::mem::replace(&mut self.sent, true) {
            return Vec::new();
        }
        [
            b"\0",
            self.login.as_bytes(),
            b"\0",
            self.password.as_bytes(),
        ]
        .concat()
    }
}

/// The failure of a command, with the server's text when it answered NO or
/// BAD, or closed the connection with BYE.
pub(crate) fn command_failure(step: ImapStep, error: &Error) -> StepFailure {
    match error {
        Error::No(status) | Error::Bad(status) | Error::Bye(status) => StepFailure {
            failure: ImapFailure::Failed(step),
            server_reply: Some(ServerReply::from(status)),
        },
        Error::Io(error) => io_failure(step, error).into(),
        _ => ImapFailure::Failed(step).into(),
    }
}

fn io_failure(step: ImapStep, error: &io::Error) -> ImapFailure {
    // Only GIO reports timeouts; async-imap's own errors use other kinds.
    if error.kind() == io::ErrorKind::TimedOut {
        ImapFailure::TimedOut(step)
    } else {
        ImapFailure::Failed(step)
    }
}
