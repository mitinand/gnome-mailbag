// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A scripted IMAP server for tests, on its own thread and GLib context. It
//! serves synthetic messages over loopback, can misbehave on purpose and
//! records command names, UIDs, section names and how often credentials
//! arrived, never the credentials or message text.

use crate::{Encryption, ImapAccount};
use futures_util::io::{AsyncReadExt, AsyncWriteExt};
use gio::prelude::*;
use std::{
    cell::Cell,
    collections::BTreeMap,
    error::Error,
    path::PathBuf,
    process::Command,
    rc::Rc,
    sync::{Arc, Mutex, Once, atomic, mpsc},
    thread,
    time::Duration,
};

pub const TEST_LOGIN: &str = "synthetic-user";
pub const TEST_PASSWORD: &str = "synthetic-password";

type ServeResult = Result<(), Box<dyn Error>>;

fn workspace_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// Makes GIO trust the test CA in this process, generating the test
/// certificates with tools/make-certs.sh when they are missing. Production
/// code has no such option; it always uses the system trust.
pub fn trust_test_certificates() {
    static TRUST: Once = Once::new();
    TRUST.call_once(|| {
        TEST_CERTIFICATES_TRUSTED.store(true, atomic::Ordering::Relaxed);
        let directory = workspace_path("target/test-certs");
        if !directory.join("expired.pem").exists() {
            let status = Command::new(workspace_path("tools/make-certs.sh"))
                .status()
                .expect("run tools/make-certs.sh (it needs openssl)");
            assert!(status.success(), "tools/make-certs.sh failed");
        }
        let database =
            gio::TlsFileDatabase::new(directory.join("ca.pem")).expect("load the test CA");
        gio::TlsBackend::default().set_default_database(Some(&database));
    });
}

/// Whether this process replaced GIO's trust database with the test CA.
/// Acceptance against the host's own trust store must run in a process that
/// never started a fixture.
pub fn test_certificates_trusted() -> bool {
    TEST_CERTIFICATES_TRUSTED.load(atomic::Ordering::Relaxed)
}

static TEST_CERTIFICATES_TRUSTED: atomic::AtomicBool = atomic::AtomicBool::new(false);

/// A synthetic message as the server stores it.
#[derive(Clone, Debug)]
pub struct FixtureMessage {
    pub uid: u32,
    pub seen: bool,
    /// The message header, ending with an empty line.
    pub header: Vec<u8>,
    /// The BODYSTRUCTURE reply.
    pub structure: String,
    /// Body sections by name, such as `1`, `2` and `2.MIME`.
    pub sections: BTreeMap<String, Vec<u8>>,
}

impl FixtureMessage {
    pub fn plain_text(uid: u32, text: &str) -> Self {
        Self {
            uid,
            seen: false,
            header: message_header(uid, "text/plain; charset=utf-8"),
            structure: text_structure("PLAIN", text),
            sections: BTreeMap::from([("1".to_owned(), text.as_bytes().to_vec())]),
        }
    }

    /// A multipart/mixed message with text parts of the given subtypes.
    pub fn multipart(uid: u32, parts: &[(&str, &str)]) -> Self {
        let mut sections = BTreeMap::new();
        let mut structure = String::from("(");
        for (number, (subtype, text)) in (1..).zip(parts) {
            let mime_header = format!("Content-Type: text/{subtype}; charset=utf-8\r\n\r\n");
            sections.insert(format!("{number}.MIME"), mime_header.into_bytes());
            sections.insert(number.to_string(), text.as_bytes().to_vec());
            structure.push_str(&text_structure(&subtype.to_ascii_uppercase(), text));
        }
        structure.push_str(" \"MIXED\" (\"BOUNDARY\" \"fixture\") NIL NIL NIL)");
        Self {
            uid,
            seen: false,
            header: message_header(uid, "multipart/mixed; boundary=fixture"),
            structure,
            sections,
        }
    }

    /// A `multipart/related` whose `start` parameter names the text part by
    /// its Content-ID, with a resource before it.
    pub fn related_with_start(uid: u32, text: &str) -> Self {
        let text_id = "<text@fixture.invalid>";
        let mut sections = BTreeMap::new();
        sections.insert(
            "1.MIME".to_owned(),
            b"Content-Type: image/png\r\n\r\n".to_vec(),
        );
        sections.insert("1".to_owned(), b"resource".to_vec());
        sections.insert(
            "2.MIME".to_owned(),
            format!("Content-Type: text/plain; charset=utf-8\r\nContent-ID: {text_id}\r\n\r\n")
                .into_bytes(),
        );
        sections.insert("2".to_owned(), text.as_bytes().to_vec());
        let resource =
            "(\"IMAGE\" \"PNG\" NIL \"<image@fixture.invalid>\" NIL \"BASE64\" 8 NIL NIL NIL NIL)";
        let body = format!(
            "(\"TEXT\" \"PLAIN\" (\"CHARSET\" \"UTF-8\") \"{text_id}\" NIL \"8BIT\" {} {} NIL NIL NIL NIL)",
            text.len(),
            text.lines().count()
        );
        Self {
            uid,
            seen: false,
            header: message_header(
                uid,
                &format!("multipart/related; boundary=fixture; start=\"{text_id}\""),
            ),
            structure: format!(
                "({resource}{body} \"RELATED\" (\"BOUNDARY\" \"fixture\" \"START\" \"{text_id}\") NIL NIL NIL)"
            ),
            sections,
        }
    }

    /// A message whose BODYSTRUCTURE nests message/rfc822 parts `depth` levels
    /// deep, beyond what the IMAP parser accepts when `depth` exceeds 32.
    pub fn deeply_nested(uid: u32, depth: usize) -> Self {
        let wrapper = "(\"MESSAGE\" \"RFC822\" NIL NIL NIL \"7BIT\" 10 (NIL NIL NIL NIL NIL NIL NIL NIL NIL NIL) ";
        let structure = format!(
            "{}{}{}",
            wrapper.repeat(depth),
            text_structure("PLAIN", "x"),
            " 1)".repeat(depth)
        );
        Self {
            structure,
            ..Self::plain_text(uid, "x")
        }
    }
}

fn message_header(uid: u32, content_type: &str) -> Vec<u8> {
    format!(
        "From: Sender {uid} <sender@example.invalid>\r\nTo: reader@example.invalid\r\n\
         Subject: Message {uid}\r\nContent-Type: {content_type}\r\n\r\n"
    )
    .into_bytes()
}

fn text_structure(subtype: &str, text: &str) -> String {
    format!(
        "(\"TEXT\" \"{subtype}\" (\"CHARSET\" \"UTF-8\") NIL NIL \"8BIT\" {} {} NIL NIL NIL NIL)",
        text.len(),
        text.lines().count()
    )
}

/// What the unencrypted part of a STARTTLS connection does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartTlsBehavior {
    Offered,
    NotOffered,
    Rejected,
    /// Sends an extra plaintext line right after the STARTTLS reply.
    InjectAfterReply,
    /// Greets with PREAUTH instead of OK.
    PreauthGreeting,
}

/// Which command's first response misbehaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultyCommand {
    Structures,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// Sends part of the response, then stays silent.
    Stall,
    /// Sends part of the response line, then breaks the connection without
    /// a TLS close notification, like a lost network.
    Close,
    /// Closes the connection properly, but inside a literal.
    TruncatedLiteral,
    /// Announces a 2 GB literal, then stays silent.
    HugeLiteral,
    /// Sends the response in small pieces with this pause between them.
    Trickle(Duration),
    /// Says BYE, as a server that shuts down, and closes the connection.
    Bye,
}

#[derive(Clone, Debug)]
pub struct FixtureSetup {
    pub encryption: Encryption,
    /// Certificate from target/test-certs: `localhost`, `unknown-ca`,
    /// `wrong-host` or `expired`.
    pub certificate: &'static str,
    pub starttls: StartTlsBehavior,
    pub greeting: String,
    /// Optional TLS CAPABILITY replies; `{tag}` is replaced with the command tag.
    pub capability_reply: Option<String>,
    pub offers_plain: bool,
    pub login_disabled: bool,
    /// Credentials the server accepts; `None` accepts any.
    pub credentials: Option<(String, String)>,
    /// An untagged response sent before the sign-in continuation request, such
    /// as `* OK [ALERT] Maintenance tonight`.
    pub notice_before_sign_in: Option<String>,
    /// Writes capability and system flag names in lower case, which RFC 3501
    /// allows for atoms.
    pub lowercase_protocol_names: bool,
    /// Reply to a rejected sign-in; `{tag}` is replaced with the command tag.
    pub rejection: String,
    pub messages: Vec<FixtureMessage>,
    pub uid_validity: u32,
    /// EXAMINE completion, optionally preceded by notices; accepts `{tag}`.
    pub examine_completion: String,
    /// Closes TLS after FLAGS, before the EXAMINE count and completion.
    pub close_during_examine: bool,
    /// UIDVALIDITY for every connection after the first.
    pub uid_validity_after_reconnect: Option<u32>,
    /// Listed by the message-list FETCH, gone from later UID FETCH commands.
    pub vanishing_uid: Option<u32>,
    /// Messages that disappear once their structure has been read: text
    /// commands leave them out, as when another client moves them meanwhile.
    pub vanishing_text_uids: Vec<u32>,
    /// Sends flag changes for all requested messages before their requested data.
    pub interleave_flag_changes: bool,
    /// Answers FETCH with messages and items in reverse order.
    pub reverse_order: bool,
    /// Return each requested field in its own FETCH response. UID FETCH repeats
    /// UID; ordinary FETCH returns UID only in its own response.
    pub split_fetch_responses: bool,
    /// With split text responses, expunge the first mailbox message after the
    /// first field. Tests request only later messages, whose numbers then shift.
    pub expunge_during_text: bool,
    /// A tagged FETCH completion without the tag, for example `OK [ALERT] ...`.
    pub fetch_completion: Option<String>,
    /// Sends NIL instead of this message's body sections.
    pub nil_body_uid: Option<u32>,
    /// Leaves out this message's body sections.
    pub missing_body_uid: Option<u32>,
    /// Messages the server cannot return, for example because they are
    /// damaged: every FETCH that names one leaves it out and completes with NO.
    pub unfetchable_uids: Vec<u32>,
    pub fault: Option<(FaultyCommand, FaultKind)>,
}

impl Default for FixtureSetup {
    fn default() -> Self {
        Self {
            encryption: Encryption::ImplicitTls,
            certificate: "localhost",
            starttls: StartTlsBehavior::Offered,
            greeting: "* OK Mailbag test server ready".to_owned(),
            capability_reply: None,
            offers_plain: true,
            login_disabled: false,
            credentials: Some((TEST_LOGIN.to_owned(), TEST_PASSWORD.to_owned())),
            notice_before_sign_in: None,
            lowercase_protocol_names: false,
            rejection: "{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n".to_owned(),
            messages: Vec::new(),
            uid_validity: 1,
            examine_completion: "{tag} OK [READ-ONLY] done\r\n".to_owned(),
            close_during_examine: false,
            uid_validity_after_reconnect: None,
            vanishing_uid: None,
            vanishing_text_uids: Vec::new(),
            interleave_flag_changes: false,
            reverse_order: false,
            split_fetch_responses: false,
            expunge_during_text: false,
            fetch_completion: None,
            nil_body_uid: None,
            missing_body_uid: None,
            unfetchable_uids: Vec::new(),
            fault: None,
        }
    }
}

/// What the server observed.
#[derive(Clone, Debug, Default)]
pub struct FixtureLog {
    pub connections: usize,
    pub closed_connections: usize,
    /// Command names in order, prefixed with `plaintext` before STARTTLS.
    pub commands: Vec<String>,
    pub fetches: Vec<RecordedFetch>,
    /// Sign-in commands that carried credentials, with or without TLS.
    pub credentials_received: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedFetch {
    /// Connections are numbered from 1 in the order they arrived.
    pub connection: usize,
    pub by_uid: bool,
    pub message_set: String,
    pub items: Vec<String>,
}

pub struct ImapFixture {
    port: u16,
    encryption: Encryption,
    log: Arc<Mutex<FixtureLog>>,
    main_loop: glib::MainLoop,
    thread: Option<thread::JoinHandle<()>>,
}

impl ImapFixture {
    /// Listens on a free loopback port.
    pub fn start(setup: FixtureSetup) -> Self {
        Self::start_on_port(setup, 0).expect("listen on loopback")
    }

    /// Listens on the given loopback port, or a free one for 0.
    pub fn start_on_port(setup: FixtureSetup, port: u16) -> Result<Self, String> {
        trust_test_certificates();
        let encryption = setup.encryption;
        let log = Arc::new(Mutex::new(FixtureLog::default()));
        let server_log = log.clone();
        let (ready, started) = mpsc::channel();
        let thread = thread::spawn(move || {
            let context = glib::MainContext::new();
            context
                .with_thread_default(|| {
                    let listener = gio::SocketListener::new();
                    let address = gio::InetSocketAddress::new(
                        &gio::InetAddress::new_loopback(gio::SocketFamily::Ipv4),
                        port,
                    );
                    let bound = listener
                        .add_address(
                            &address,
                            gio::SocketType::Stream,
                            gio::SocketProtocol::Tcp,
                            None::<&glib::Object>,
                        )
                        .map_err(|error| error.to_string());
                    let port = match bound {
                        Ok(bound) => bound.downcast::<gio::InetSocketAddress>().unwrap().port(),
                        Err(error) => {
                            ready.send(Err(error)).unwrap();
                            return;
                        }
                    };
                    let main_loop = glib::MainLoop::new(Some(&context), false);
                    ready.send(Ok((port, main_loop.clone()))).unwrap();
                    let server = Rc::new(Server {
                        setup,
                        log: server_log,
                        fault_pending: Cell::new(true),
                    });
                    context.spawn_local(async move {
                        while let Ok((connection, _)) = listener.accept_future().await {
                            glib::spawn_future_local(server.clone().serve(connection));
                        }
                    });
                    main_loop.run();
                })
                .unwrap();
        });
        let (port, main_loop) = started.recv().unwrap()?;
        Ok(Self {
            port,
            encryption,
            log,
            main_loop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// An account for this server with the test credentials.
    pub fn account(&self) -> ImapAccount {
        self.account_with_password(TEST_PASSWORD)
    }

    pub fn account_with_password(&self, password: &str) -> ImapAccount {
        ImapAccount {
            host: format!("localhost:{}", self.port),
            login: TEST_LOGIN.to_owned(),
            password: password.to_owned(),
            encryption: self.encryption,
        }
    }

    pub fn log(&self) -> FixtureLog {
        self.log.lock().unwrap().clone()
    }
}

impl Drop for ImapFixture {
    fn drop(&mut self) {
        let main_loop = self.main_loop.clone();
        self.main_loop.context().invoke(move || main_loop.quit());
        self.thread.take().unwrap().join().unwrap();
    }
}

struct Server {
    setup: FixtureSetup,
    log: Arc<Mutex<FixtureLog>>,
    /// The configured fault happens once per server.
    fault_pending: Cell<bool>,
}

impl Server {
    fn record(&self, change: impl FnOnce(&mut FixtureLog)) {
        change(&mut self.log.lock().unwrap());
    }

    async fn serve(self: Rc<Self>, connection: gio::SocketConnection) {
        let connection_number = {
            let mut log = self.log.lock().unwrap();
            log.connections += 1;
            log.connections
        };
        // Errors only mean that the client went away.
        let _ = self.serve_connection(&connection, connection_number).await;
        self.record(|log| log.closed_connections += 1);
    }

    async fn serve_connection(
        &self,
        connection: &gio::SocketConnection,
        connection_number: usize,
    ) -> ServeResult {
        let certificate = gio::TlsCertificate::from_files(
            workspace_path(&format!("target/test-certs/{}.pem", self.setup.certificate)),
            workspace_path(&format!("target/test-certs/{}.key", self.setup.certificate)),
        )?;
        if self.setup.encryption == Encryption::StartTls
            && !self.serve_plaintext(connection).await?
        {
            return Ok(());
        }
        let tls = gio::TlsServerConnection::new(connection, Some(&certificate))?;
        tls.handshake_future(glib::Priority::DEFAULT).await?;
        let mut io = Io::new(tls.upcast(), connection.socket());
        if self.setup.encryption == Encryption::ImplicitTls {
            io.send(format!("{}\r\n", self.setup.greeting)).await?;
        }
        self.serve_session(&mut io, connection_number).await
    }

    /// The unencrypted part of a STARTTLS connection. Returns whether TLS starts.
    async fn serve_plaintext(
        &self,
        connection: &gio::SocketConnection,
    ) -> Result<bool, Box<dyn Error>> {
        let behavior = self.setup.starttls;
        let mut io = Io::new(connection.clone().upcast(), connection.socket());
        if behavior == StartTlsBehavior::PreauthGreeting {
            io.send("* PREAUTH signed in without TLS\r\n").await?;
        } else {
            io.send(format!("{}\r\n", self.setup.greeting)).await?;
        }
        while let Some(command) = io.command().await? {
            let (tag, name, _) = split_command(&command);
            self.record(|log| log.commands.push(format!("plaintext {name}")));
            match name.as_str() {
                "CAPABILITY" => {
                    let starttls = match (
                        behavior == StartTlsBehavior::NotOffered,
                        self.setup.lowercase_protocol_names,
                    ) {
                        (true, _) => "",
                        (false, true) => " starttls",
                        (false, false) => " STARTTLS",
                    };
                    let disabled = match self.setup.lowercase_protocol_names {
                        true => "logindisabled",
                        false => "LOGINDISABLED",
                    };
                    io.send(format!(
                        "* CAPABILITY IMAP4rev1 {disabled}{starttls}\r\n{tag} OK done\r\n"
                    ))
                    .await?;
                }
                "STARTTLS" if behavior == StartTlsBehavior::Rejected => {
                    io.send(format!("{tag} NO TLS is unavailable\r\n")).await?;
                }
                "STARTTLS" if behavior != StartTlsBehavior::NotOffered => {
                    let mut reply = format!("{tag} OK Begin TLS\r\n");
                    if behavior == StartTlsBehavior::InjectAfterReply {
                        reply.push_str("* OK [ALERT] injected before TLS\r\n");
                    }
                    io.send(reply).await?;
                    return Ok(true);
                }
                "LOGIN" => {
                    self.record(|log| log.credentials_received += 1);
                    io.send(format!("{tag} NO [PRIVACYREQUIRED] Use TLS\r\n"))
                        .await?;
                }
                _ => io.send(format!("{tag} BAD Not before TLS\r\n")).await?,
            }
        }
        Ok(false)
    }

    async fn serve_session(&self, io: &mut Io, connection_number: usize) -> ServeResult {
        while let Some(command) = io.command().await? {
            let (tag, name, arguments) = split_command(&command);
            self.record(|log| log.commands.push(name.clone()));
            match name.as_str() {
                "CAPABILITY" => {
                    if let Some(reply) = &self.setup.capability_reply {
                        io.send(reply.replace("{tag}", &tag)).await?;
                        continue;
                    }
                    let plain = if self.setup.offers_plain {
                        " AUTH=PLAIN"
                    } else {
                        ""
                    };
                    let disabled = match (
                        self.setup.login_disabled,
                        self.setup.lowercase_protocol_names,
                    ) {
                        (false, _) => "",
                        (true, true) => " logindisabled",
                        (true, false) => " LOGINDISABLED",
                    };
                    io.send(format!(
                        "* CAPABILITY IMAP4rev1{plain}{disabled}\r\n{tag} OK done\r\n"
                    ))
                    .await?;
                }
                "AUTHENTICATE" => {
                    if let Some(notice) = &self.setup.notice_before_sign_in {
                        io.send(format!("{notice}\r\n")).await?;
                    }
                    io.send("+ \r\n").await?;
                    let Some(line) = io.read_line().await? else {
                        return Ok(());
                    };
                    let response = glib::base64_decode(&String::from_utf8_lossy(&line));
                    let fields: Vec<&[u8]> = response.split(|byte| *byte == 0).collect();
                    let accepted = match fields.as_slice() {
                        [_, login, password] => self.check_credentials(login, password),
                        _ => false,
                    };
                    self.reply_to_sign_in(io, &tag, accepted).await?;
                }
                "LOGIN" => {
                    let accepted = match login_arguments(&arguments).as_slice() {
                        [login, password] => {
                            self.check_credentials(login.as_bytes(), password.as_bytes())
                        }
                        _ => false,
                    };
                    self.reply_to_sign_in(io, &tag, accepted).await?;
                }
                "EXAMINE" | "SELECT" => {
                    if self.setup.close_during_examine {
                        io.send("* FLAGS (\\Seen)\r\n").await?;
                        io.stream.close().await?;
                        return Ok(());
                    }
                    let uid_validity = match self.setup.uid_validity_after_reconnect {
                        Some(uid_validity) if connection_number > 1 => uid_validity,
                        _ => self.setup.uid_validity,
                    };
                    let count = self.setup.messages.len();
                    io.send(format!(
                        "* {count} EXISTS\r\n* 0 RECENT\r\n* FLAGS (\\Seen)\r\n\
                         * OK [UIDVALIDITY {uid_validity}] UIDs valid\r\n"
                    ))
                    .await?;
                    io.send(self.setup.examine_completion.replace("{tag}", &tag))
                        .await?;
                }
                "FETCH" | "UID FETCH" => {
                    let by_uid = name == "UID FETCH";
                    if !self
                        .fetch(io, connection_number, &tag, &arguments, by_uid)
                        .await?
                    {
                        return Ok(());
                    }
                }
                "LOGOUT" => {
                    io.send(format!("* BYE Logging out\r\n{tag} OK done\r\n"))
                        .await?;
                    return Ok(());
                }
                _ => io.send(format!("{tag} BAD Unsupported\r\n")).await?,
            }
        }
        Ok(())
    }

    fn check_credentials(&self, login: &[u8], password: &[u8]) -> bool {
        self.record(|log| log.credentials_received += 1);
        self.setup
            .credentials
            .as_ref()
            .is_none_or(|(expected_login, expected_password)| {
                login == expected_login.as_bytes() && password == expected_password.as_bytes()
            })
    }

    async fn reply_to_sign_in(&self, io: &mut Io, tag: &str, accepted: bool) -> ServeResult {
        if accepted {
            io.send(format!("{tag} OK Signed in\r\n")).await?;
        } else {
            io.send(self.setup.rejection.replace("{tag}", tag)).await?;
        }
        Ok(())
    }

    /// Answers FETCH or UID FETCH. Returns false when a fault ended the session.
    async fn fetch(
        &self,
        io: &mut Io,
        connection: usize,
        tag: &str,
        arguments: &str,
        by_uid: bool,
    ) -> Result<bool, Box<dyn Error>> {
        let (message_set, items) = arguments.split_once(' ').unwrap_or((arguments, "()"));
        let mut items = split_items(items);
        self.record(|log| {
            log.fetches.push(RecordedFetch {
                connection,
                by_uid,
                message_set: message_set.to_owned(),
                items: items.clone(),
            });
        });
        let faulty_command = if items.iter().any(|item| item == "BODYSTRUCTURE") {
            Some(FaultyCommand::Structures)
        } else if items
            .iter()
            .any(|item| body_section(item).is_some_and(is_body_part))
        {
            Some(FaultyCommand::Text)
        } else {
            None
        };
        let mut messages = self.select_messages(message_set, by_uid);
        if faulty_command == Some(FaultyCommand::Text) {
            messages.retain(|(_, message)| !self.setup.vanishing_text_uids.contains(&message.uid));
        }
        let requested_count = messages.len();
        messages.retain(|(_, message)| !self.setup.unfetchable_uids.contains(&message.uid));
        let completion =
            self.setup
                .fetch_completion
                .as_deref()
                .unwrap_or(if messages.len() < requested_count {
                    "NO Some messages could not be FETCHed"
                } else {
                    "OK FETCH done"
                });
        if self.setup.reverse_order {
            messages.reverse();
            items.reverse();
        }
        let item_groups: Vec<Vec<String>> = if self.setup.split_fetch_responses {
            items
                .iter()
                .filter(|item| !by_uid || item.as_str() != "UID")
                .map(|item| {
                    let mut group = Vec::new();
                    if by_uid {
                        group.push("UID".to_owned());
                    }
                    group.push(item.clone());
                    group
                })
                .collect()
        } else {
            vec![items]
        };
        if self.setup.interleave_flag_changes {
            for (sequence_number, message) in &messages {
                io.send(format!(
                    "* {sequence_number} FETCH (UID {} FLAGS (\\Seen))\r\n",
                    message.uid
                ))
                .await?;
            }
        }
        let mut expunged = false;
        for (sequence_number, message) in messages {
            let prefix = format!("* {sequence_number} FETCH (UID {}", message.uid);
            match self.setup.fault {
                Some((command, kind))
                    if Some(command) == faulty_command && self.fault_pending.replace(false) =>
                {
                    let response = self.response(message, &item_groups.concat(), sequence_number);
                    if !misbehave(io, command, kind, &prefix, response).await? {
                        return Ok(false);
                    }
                }
                _ => {
                    for group in &item_groups {
                        let number = sequence_number - usize::from(expunged);
                        io.send(self.response(message, group, number)).await?;
                        if self.setup.expunge_during_text
                            && faulty_command == Some(FaultyCommand::Text)
                            && !expunged
                        {
                            io.send("* 1 EXPUNGE\r\n").await?;
                            expunged = true;
                        }
                    }
                }
            }
        }
        io.send(format!("{tag} {completion}\r\n")).await?;
        Ok(true)
    }

    /// Messages in the set with their sequence numbers.
    fn select_messages(&self, message_set: &str, by_uid: bool) -> Vec<(usize, &FixtureMessage)> {
        let ranges: Vec<(u32, u32)> = message_set
            .split(',')
            .map(|range| match range.split_once(':') {
                Some((low, high)) => (low.parse().unwrap(), high.parse().unwrap()),
                None => (range.parse().unwrap(), range.parse().unwrap()),
            })
            .collect();
        (1..)
            .zip(&self.setup.messages)
            .filter(|(sequence_number, message)| {
                let key = if by_uid {
                    message.uid
                } else {
                    *sequence_number as u32
                };
                ranges
                    .iter()
                    .any(|(low, high)| (*low..=*high).contains(&key))
                    && !(by_uid && self.setup.vanishing_uid == Some(message.uid))
            })
            .collect()
    }

    fn response(
        &self,
        message: &FixtureMessage,
        items: &[String],
        sequence_number: usize,
    ) -> Vec<u8> {
        let mut fields: Vec<Vec<u8>> = Vec::new();
        for item in items {
            match item.as_str() {
                "UID" => fields.push(format!("UID {}", message.uid).into_bytes()),
                "FLAGS" => {
                    let flags = match (message.seen, self.setup.lowercase_protocol_names) {
                        (true, true) => "\\seen",
                        (true, false) => "\\Seen",
                        (false, _) => "",
                    };
                    fields.push(format!("FLAGS ({flags})").into_bytes());
                }
                "INTERNALDATE" => {
                    fields.push(b"INTERNALDATE \"17-Sep-2026 10:00:00 +0300\"".to_vec())
                }
                "BODYSTRUCTURE" => {
                    fields.push(format!("BODYSTRUCTURE {}", message.structure).into_bytes());
                }
                _ => {
                    let Some(section) = body_section(item) else {
                        continue;
                    };
                    let body_part = is_body_part(section);
                    if body_part && self.setup.missing_body_uid == Some(message.uid) {
                        continue;
                    }
                    if body_part && self.setup.nil_body_uid == Some(message.uid) {
                        fields.push(format!("BODY[{section}] NIL").into_bytes());
                        continue;
                    }
                    let data = if section.to_ascii_uppercase().starts_with("HEADER") {
                        &message.header
                    } else {
                        &message.sections[section]
                    };
                    let mut field = format!("BODY[{section}] {{{}}}\r\n", data.len()).into_bytes();
                    field.extend(data);
                    fields.push(field);
                }
            }
        }
        let mut response = format!("* {sequence_number} FETCH (").into_bytes();
        response.extend(fields.join(&b' '));
        response.extend(b")\r\n");
        response
    }
}

/// Sends a misbehaving response. Returns false when the session must end.
async fn misbehave(
    io: &mut Io,
    command: FaultyCommand,
    kind: FaultKind,
    prefix: &str,
    response: Vec<u8>,
) -> Result<bool, Box<dyn Error>> {
    let attribute = match command {
        FaultyCommand::Structures => "BODYSTRUCTURE (\"TEXT\" \"PLAIN\" NIL NIL",
        FaultyCommand::Text => "BODY[1]",
    };
    match kind {
        FaultKind::Stall => {
            io.send(format!("{prefix} {attribute} {{100}}\r\nabc"))
                .await?
        }
        FaultKind::Close => {
            io.send(format!("{prefix} {attribute}")).await?;
            io.socket.close()?;
            return Ok(false);
        }
        FaultKind::TruncatedLiteral => {
            io.send(format!("{prefix} {attribute} {{100}}\r\nabc"))
                .await?;
            return Ok(false);
        }
        FaultKind::HugeLiteral => {
            io.send(format!(
                "{prefix} {attribute} {{2000000000}}\r\n{}",
                "a".repeat(64)
            ))
            .await?
        }
        FaultKind::Bye => {
            io.send("* BYE Server is restarting\r\n").await?;
            return Ok(false);
        }
        FaultKind::Trickle(pause) => {
            for piece in response.chunks(response.len().div_ceil(8)) {
                io.send(piece).await?;
                glib::timeout_future(pause).await;
            }
            return Ok(true);
        }
    }
    // Stay silent until the client gives up.
    while io.read_line().await?.is_some() {}
    Ok(false)
}

/// The section name inside `BODY[...]` or `BODY.PEEK[...]`.
fn body_section(item: &str) -> Option<&str> {
    let upper = item.to_ascii_uppercase();
    if !(upper.starts_with("BODY[") || upper.starts_with("BODY.PEEK[")) {
        return None;
    }
    let start = item.find('[')? + 1;
    Some(&item[start..item.rfind(']')?])
}

/// Whether a section names a part body rather than a header.
fn is_body_part(section: &str) -> bool {
    let upper = section.to_ascii_uppercase();
    section.starts_with(|character: char| character.is_ascii_digit()) && !upper.ends_with("MIME")
}

/// Splits `(A B[C D] E)` into `A`, `B[C D]` and `E`.
fn split_items(items: &str) -> Vec<String> {
    let items = items.trim();
    let items = items
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(items);
    let mut result = Vec::new();
    let mut depth = 0_i32;
    let mut current = String::new();
    for character in items.chars() {
        match character {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ' ' if depth == 0 => {
                result.extend((!current.is_empty()).then(|| std::mem::take(&mut current)));
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    result.extend((!current.is_empty()).then_some(current));
    result
}

/// Tag, command name with a `UID` prefix where present, and arguments.
fn split_command(command: &[u8]) -> (String, String, String) {
    let text = String::from_utf8_lossy(command);
    let mut words = text.splitn(3, ' ');
    let tag = words.next().unwrap_or_default().to_owned();
    let mut name = words.next().unwrap_or_default().to_ascii_uppercase();
    let mut arguments = words.next().unwrap_or_default().to_owned();
    if name == "UID" {
        let (inner_name, inner_arguments) = arguments.split_once(' ').unwrap_or((&arguments, ""));
        name = format!("UID {}", inner_name.to_ascii_uppercase());
        arguments = inner_arguments.to_owned();
    }
    (tag, name, arguments)
}

/// The arguments of a LOGIN command: quoted strings, with `\"` and `\\`
/// unescaped, and literals.
fn login_arguments(arguments: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = arguments;
    while let Some(start) = rest.find(['"', '{']) {
        rest = &rest[start..];
        if let Some(literal) = rest.strip_prefix('{') {
            let (length, data) = literal.split_once("}\r\n").unwrap();
            let length: usize = length.parse().unwrap();
            values.push(data[..length].to_owned());
            rest = &data[length..];
            continue;
        }
        let mut value = String::new();
        let mut end = rest.len();
        let mut characters = rest.char_indices().skip(1);
        while let Some((index, character)) = characters.next() {
            match character {
                '\\' => value.extend(characters.next().map(|(_, escaped)| escaped)),
                '"' => {
                    end = index + 1;
                    break;
                }
                _ => value.push(character),
            }
        }
        values.push(value);
        rest = &rest[end..];
    }
    values
}

/// Line and literal reading over a GIO stream.
struct Io {
    stream: gio::IOStreamAsyncReadWrite<gio::IOStream>,
    buffer: Vec<u8>,
    /// The TCP socket underneath, for breaking the connection abruptly.
    socket: gio::Socket,
}

impl Io {
    fn new(stream: gio::IOStream, socket: gio::Socket) -> Self {
        Self {
            stream: stream.into_async_read_write().unwrap(),
            buffer: Vec::new(),
            socket,
        }
    }

    async fn fill(&mut self) -> std::io::Result<bool> {
        let mut chunk = [0_u8; 4096];
        let count = self.stream.read(&mut chunk).await?;
        self.buffer.extend_from_slice(&chunk[..count]);
        Ok(count > 0)
    }

    async fn read_line(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(end) = self.buffer.windows(2).position(|pair| pair == b"\r\n") {
                let line = self.buffer[..end].to_vec();
                self.buffer.drain(..end + 2);
                return Ok(Some(line));
            }
            if !self.fill().await? {
                return Ok(None);
            }
        }
    }

    async fn send(&mut self, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
        self.stream.write_all(bytes.as_ref()).await?;
        self.stream.flush().await
    }

    /// Reads one command, accepting synchronizing literals.
    async fn command(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let mut command = Vec::new();
        loop {
            let Some(line) = self.read_line().await? else {
                return Ok(None);
            };
            command.extend_from_slice(&line);
            let Some(length) = literal_length(&line) else {
                return Ok(Some(command));
            };
            self.send(b"+ Ready\r\n").await?;
            command.extend_from_slice(b"\r\n");
            while self.buffer.len() < length {
                if !self.fill().await? {
                    return Ok(None);
                }
            }
            command.extend(self.buffer.drain(..length));
        }
    }
}

fn literal_length(line: &[u8]) -> Option<usize> {
    let inner = line.strip_suffix(b"}")?;
    let open = inner.iter().rposition(|byte| *byte == b'{')?;
    std::str::from_utf8(&inner[open + 1..]).ok()?.parse().ok()
}
