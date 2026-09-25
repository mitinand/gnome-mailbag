// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    ClientIdentity, Credential, Encryption, ImapAccount, ImapError, ImapFailure, ImapStep,
    OpenOptions, ServerReply,
    transport::{self, GioStream, ServerConnection},
};
use async_imap::{
    Authenticator, Client, Session,
    error::{Error, StatusResponse},
    imap_proto::{Response, ResponseCode, Status},
    types::{Capability, UnsolicitedResponse},
};
use std::{borrow::Cow, collections::HashMap, io};

/// Server text with every occurrence of the sign-in name, in any ASCII letter
/// case and whatever its length, replaced with `<login>`
/// (specs/003-logging/research.md §6). The rest of the text is kept as sent.
/// It serves the error and the record alike: a text is replaced once, where
/// it enters an error or a debug line, never twice.
pub(crate) fn replace_sign_in_name(sign_in_name: &str, text: &str) -> String {
    let lowercase_text = text.to_ascii_lowercase();
    let lowercase_name = sign_in_name.to_ascii_lowercase();
    let mut replaced_text = String::with_capacity(text.len());
    let mut copied = 0;
    // ASCII lowercasing keeps every byte position, so matches index `text`.
    for (start, _) in lowercase_text.match_indices(&lowercase_name) {
        replaced_text.push_str(&text[copied..start]);
        replaced_text.push_str("<login>");
        copied = start + lowercase_name.len();
    }
    replaced_text.push_str(&text[copied..]);
    replaced_text
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

/// What the server said during an attempt that can explain its failure. The
/// sign-in name is passed to the two functions that write a line with server
/// text; no field keeps it for the record's sake (specs/003-logging FR-017).
#[derive(Default)]
pub(crate) struct ServerNotices {
    /// The unilateral responses of the current connection, read from here
    /// only: the fork's own copy of the channel is never drained.
    responses: Option<async_channel::Receiver<UnsolicitedResponse>>,
    /// ALERT texts, which RFC 3501 requires to reach the user.
    alerts: Vec<String>,
    /// The BYE with which the server closed the connection, for example
    /// when it shut down.
    bye: Option<ServerReply>,
}

impl ServerNotices {
    /// Follows a new connection's unilateral responses; a reconnection
    /// replaces the previous connection's channel.
    pub(crate) fn follow(&mut self, responses: async_channel::Receiver<UnsolicitedResponse>) {
        self.responses = Some(responses);
    }

    /// Keeps the ALERT and BYE texts among the waiting unilateral responses.
    pub(crate) fn collect(&mut self, sign_in_name: &str) {
        let Some(responses) = self.responses.clone() else {
            return;
        };
        while let Ok(response) = responses.try_recv() {
            if let UnsolicitedResponse::Other(data) = response {
                self.keep(sign_in_name, data.parsed());
            }
        }
    }

    fn keep(&mut self, sign_in_name: &str, response: &Response<'_>) {
        let (status, code, information) = match response {
            // imap-proto parses the ENABLED reply into the same response as a
            // CAPABILITY list, so the line names both, rather than claiming to
            // know which arrived. Gmail sends its full list only after sign-in
            // (research.md §2, §4).
            Response::Capabilities(announced) => {
                let announced: Vec<Capability> = announced.iter().map(Capability::from).collect();
                tracing::debug!(
                    names = capability_names(&announced),
                    "the server announced capabilities or enabled extensions"
                );
                return;
            }
            // The reply to our own ID command arrives here, like any other
            // untagged list; its tagged result is checked where it was sent.
            Response::Id(fields) => return log_server_identification(fields.as_ref()),
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
            // The only line whose level is chosen at run time: that an alert
            // arrived belongs to info, its text to debug (FR-010, FR-011).
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    alert = replace_sign_in_name(sign_in_name, text),
                    "the server sent an alert"
                );
            } else {
                tracing::info!("the server sent an alert");
            }
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
    /// failed step passes here, so the sign-in name is replaced in the
    /// server's texts here, and the reply is logged here as well.
    pub(crate) fn error(&mut self, sign_in_name: &str, failure: StepFailure) -> ImapError {
        self.collect(sign_in_name);
        let mut server_reply = failure.server_reply.or_else(|| self.bye.take());
        if let Some(reply) = &mut server_reply {
            reply.text = replace_sign_in_name(sign_in_name, &reply.text);
            tracing::debug!(
                code = reply.code.as_deref(),
                server_text = reply.text,
                "the server's reply to the failed step"
            );
        }
        let mut alerts = std::mem::take(&mut self.alerts);
        for alert in &mut alerts {
            *alert = replace_sign_in_name(sign_in_name, alert);
        }
        ImapError {
            failure: failure.failure,
            server_reply,
            alerts,
        }
    }
}

/// Connects securely, signs in, runs what `options` asks for and then
/// EXAMINE INBOX. ALERT and BYE texts received over TLS are added to `notices`.
pub(crate) async fn open_inbox(
    account: &ImapAccount,
    options: &OpenOptions,
    socket_timeout_seconds: u32,
    notices: &mut ServerNotices,
) -> Result<InboxSession, StepFailure> {
    let (connection, identity) =
        transport::connect(&account.host, account.encryption, socket_timeout_seconds).await?;
    let client = match account.encryption {
        Encryption::ImplicitTls => {
            let tls = transport::start_tls(&connection, &identity, account.encryption).await?;
            let mut client = Client::new(GioStream::new(tls));
            notices.follow(client.unsolicited_responses().clone());
            read_greeting(&mut client, &account.login, notices).await?;
            client
        }
        Encryption::StartTls => {
            upgrade_plaintext(&connection).await?;
            let tls = transport::start_tls(&connection, &identity, account.encryption).await?;
            // The server sends no second greeting after STARTTLS.
            let client = Client::new(GioStream::new(tls));
            notices.follow(client.unsolicited_responses().clone());
            client
        }
    };
    let mut session = sign_in(client, account, notices).await?;
    if options.readable_names {
        // RFC 5161 allows ENABLE only before a mailbox is selected.
        offer_readable_names(&mut session, &account.login).await?;
    }
    if let Some(identity) = &options.client_identity {
        identify_client(&mut session, identity, &account.login).await?;
    }
    // Both commands answer with an untagged list, which belongs to the record
    // before the Inbox is opened.
    notices.collect(&account.login);
    let examined = session.examine("INBOX").await;
    notices.collect(&account.login);
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
    sign_in_name: &str,
    notices: &mut ServerNotices,
) -> Result<(), StepFailure> {
    let greeting = client
        .read_response()
        .await
        .map_err(|error| io_failure(ImapStep::Connect, &error))?
        .ok_or(ImapFailure::Failed(ImapStep::Connect))?;
    notices.keep(sign_in_name, greeting.parsed());
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

/// Signs in with the method the credential needs: XOAUTH2 for an access token,
/// otherwise AUTHENTICATE PLAIN when offered and LOGIN unless the server
/// disables it. A rejected sign-in is not retried with another method.
async fn sign_in(
    mut client: Client<GioStream>,
    account: &ImapAccount,
    notices: &mut ServerNotices,
) -> Result<Session<GioStream>, StepFailure> {
    let capabilities = client.capabilities().await;
    notices.collect(&account.login);
    let capabilities = capabilities.map_err(|error| command_failure(ImapStep::SignIn, &error))?;
    tracing::info!(
        capabilities = capability_names(capabilities.iter()),
        "server capabilities"
    );
    let (method, signed_in) = match &account.credential {
        Credential::AccessToken(token) if capabilities.has_str("AUTH=XOAUTH2") => {
            let credentials = XOAuth2Credentials {
                login: &account.login,
                token,
                sent: false,
            };
            ("XOAUTH2", client.authenticate("XOAUTH2", credentials).await)
        }
        // Google documents XOAUTH2 for IMAP; no other mechanism carries a token.
        Credential::AccessToken(_) => return Err(ImapFailure::NoSignInMethod.into()),
        Credential::Password(password) if capabilities.has_str("AUTH=PLAIN") => {
            let credentials = PlainCredentials {
                login: &account.login,
                password,
                sent: false,
            };
            ("PLAIN", client.authenticate("PLAIN", credentials).await)
        }
        Credential::Password(password) if !capabilities.has_str("LOGINDISABLED") => {
            // The fork sends a non-ASCII login or password as a literal.
            ("LOGIN", client.login(&account.login, password).await)
        }
        Credential::Password(_) => return Err(ImapFailure::NoSignInMethod.into()),
    };
    match signed_in {
        Ok(session) => {
            notices.collect(&account.login);
            tracing::info!(method, "signed in");
            Ok(session)
        }
        // What the server said before refusing waits in the channel the
        // notices follow; the error collects it.
        Err((error, _client)) => Err(command_failure(ImapStep::SignIn, &error)),
    }
}

/// A capability list as the server named it, for the record.
fn capability_names<'a>(capabilities: impl IntoIterator<Item = &'a Capability>) -> String {
    capabilities
        .into_iter()
        .map(|capability| match capability {
            Capability::Imap4rev1 => "IMAP4rev1".to_owned(),
            Capability::Auth(mechanism) => format!("AUTH={mechanism}"),
            Capability::Atom(name) => name.clone(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Offers UTF-8 mailbox and label names. A server that refuses keeps sending
/// modified UTF-7, which the names then carry into the batch as they are.
async fn offer_readable_names(
    session: &mut Session<GioStream>,
    sign_in_name: &str,
) -> Result<(), StepFailure> {
    match session.run_command_and_check_ok("ENABLE UTF8=ACCEPT").await {
        Ok(()) => tracing::debug!("the server accepted UTF-8 names"),
        Err(Error::No(status) | Error::Bad(status)) => tracing::debug!(
            code = status.code.as_deref(),
            server_text = replace_sign_in_name(sign_in_name, &status.text),
            "the server refused UTF-8 names"
        ),
        // A broken connection, not a refusal: the Inbox cannot follow.
        Err(error) => return Err(command_failure(ImapStep::OpenInbox, &error)),
    }
    Ok(())
}

/// Names this client to the server, as Gmail asks clients to do. The server's
/// own reply is an untagged list that the notices log; a refusal is logged
/// here and leaves the load going.
///
/// `Session::id` is not used: the fork reads its reply without checking the
/// command's completion, so a NO or BAD would pass for a reply without fields.
async fn identify_client(
    session: &mut Session<GioStream>,
    identity: &ClientIdentity,
    sign_in_name: &str,
) -> Result<(), StepFailure> {
    let identification = [
        ("name", &identity.name),
        ("version", &identity.version),
        ("vendor", &identity.vendor),
        ("contact", &identity.contact),
        ("support-url", &identity.support_url),
    ]
    .map(|(field, value)| format!("{} {}", quoted(field), quoted(value)))
    .join(" ");
    match session
        .run_command_and_check_ok(format!("ID ({identification})"))
        .await
    {
        Ok(()) => {}
        Err(Error::No(status) | Error::Bad(status)) => tracing::debug!(
            code = status.code.as_deref(),
            server_text = replace_sign_in_name(sign_in_name, &status.text),
            "the server refused the identification"
        ),
        Err(error) => return Err(command_failure(ImapStep::OpenInbox, &error)),
    }
    Ok(())
}

/// An IMAP quoted string: a backslash and a quote inside it are escaped.
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', r"\\").replace('"', "\\\""))
}

/// The three fields of a server's identification that may reach the record.
/// The rest carries this computer's public address and an opaque session
/// token (specs/003-logging FR-009).
fn log_server_identification(fields: Option<&HashMap<Cow<'_, str>, Cow<'_, str>>>) {
    let field = |name| {
        fields
            .and_then(|fields| fields.get(name))
            .map(|value| &**value)
    };
    tracing::debug!(
        name = field("name"),
        vendor = field("vendor"),
        version = field("version"),
        "the server identified itself"
    );
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

/// The XOAUTH2 initial response Google documents. A wrong token is answered
/// with a challenge carrying the server's error as JSON, which the protocol
/// requires the client to acknowledge with an empty line before the refusal
/// arrives (specs/004-gmail-integration/research.md §2).
struct XOAuth2Credentials<'a> {
    login: &'a str,
    token: &'a str,
    sent: bool,
}

impl Authenticator for XOAuth2Credentials<'_> {
    type Response = Vec<u8>;

    fn process(&mut self, _challenge: &[u8]) -> Vec<u8> {
        if std::mem::replace(&mut self.sent, true) {
            return Vec::new();
        }
        format!("user={}\x01auth=Bearer {}\x01\x01", self.login, self.token).into_bytes()
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
