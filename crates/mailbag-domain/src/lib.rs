// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The definitions every layer of the application shares: the account, its
//! folders, the message as the application keeps and shows it, and how a
//! failure is handed on in the application's terms rather than a protocol's
//! (specs/006-error-handling/research.md §1). Only the application words them
//! for the user. The crate depends on nothing in the workspace, so every layer
//! can reach it.

mod panic;
#[cfg(test)]
mod tests;

pub use panic::{catch_panic, install_panic_hook, take_panic};

use std::fmt;

/// An Online Accounts account's identifier: nonempty, opaque and stable
/// across renames (specs/001-goa-account-observation/contracts/accounts.md).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(String);

impl AccountId {
    /// The identifier's text, by which the record names the account.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An empty text is not an account identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmptyAccountId;

impl TryFrom<&str> for AccountId {
    type Error = EmptyAccountId;

    fn try_from(id: &str) -> Result<Self, Self::Error> {
        if id.is_empty() {
            Err(EmptyAccountId)
        } else {
            Ok(Self(id.to_owned()))
        }
    }
}

/// A folder of an account as its provider lists it (specs/008-folders
/// FR-002). The window shows it as a mailbox.
#[derive(Clone, PartialEq, Eq)]
pub struct Folder {
    /// What the provider opens it by: the IMAP or Gmail mailbox name as the
    /// server sent it, the Microsoft 365 folder identifier.
    pub identity: String,
    /// The server's name for display: under a listed parent the part after
    /// the parent's name and the delimiter, otherwise the whole name. The
    /// window shows the reserved IMAP name INBOX as "Inbox".
    pub name: String,
    /// The parent's identity; `None` directly under the account.
    pub parent: Option<String>,
    pub role: Option<FolderRole>,
    /// Whether the folder can be opened; one that cannot is a container.
    pub selectable: bool,
}

/// What the application makes of a folder's server roles (specs/008-folders
/// FR-003).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderRole {
    Inbox,
    Starred,
    Important,
    Junk,
    Trash,
    Archive,
    Drafts,
    Sent,
    AllMail,
}

impl FolderRole {
    /// The order of the system folders in the sidebar (specs/008-folders FR-009).
    pub const ORDER: [Self; 9] = [
        Self::Inbox,
        Self::Starred,
        Self::Important,
        Self::Junk,
        Self::Trash,
        Self::Archive,
        Self::Drafts,
        Self::Sent,
        Self::AllMail,
    ];

    /// Whether the folder collects messages that live in other folders, as
    /// RFC 6154 and RFC 8457 describe these roles. On IMAP a view allows no
    /// move and no delete, because the standard does not define their effect
    /// on the message's real folder (specs/008-folders FR-013(d)).
    pub fn is_view(self) -> bool {
        matches!(self, Self::Starred | Self::Important | Self::AllMail)
    }
}

/// A folder of one account, as a mailbox load and the window address it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FolderRef {
    pub account: AccountId,
    /// The folder's `Folder::identity`.
    pub identity: String,
}

/// A message as the application keeps and shows it: what a load received,
/// what the store holds and what the window lists and reads.
#[derive(Clone, PartialEq, Eq)]
pub struct Message {
    /// The message's identity within its account, by which the store keeps
    /// it once however many folders list it: `gmail:<X-GM-MSGID>`,
    /// `graph:<immutable id>`, or `imap:<folder identity>/<uid>` for a
    /// Generic IMAP message, which has no identity beyond its place.
    pub identity: String,
    pub fields: DisplayFields,
    /// The received date as seconds since the Unix epoch.
    pub received_unix: Option<i64>,
    /// The read state as the server last reported it.
    pub seen: bool,
    pub content: ReceivedContent,
}

/// Subject, sender and recipients for the list and the reader.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplayFields {
    pub subject: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// A failure the user may be told about, as the layer that met it hands it on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// What went wrong, in the application's terms: the stable reason the
    /// wording is chosen by.
    pub kind: FailureKind,
    /// What the remote side said in words, in the order shown, with the
    /// sign-in name already replaced by `<login>`.
    pub remote_texts: Vec<RemoteText>,
    /// Identifiers for a report, one `Label: value` per line, in English;
    /// empty when there is nothing beyond the kind.
    pub details: String,
}

/// What went wrong: one variant per failure the application words
/// differently. The layer that meets the failure chooses the kind; reading a
/// protocol's codes happens there
/// (specs/006-error-handling/contracts/failure-declaration.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// Online Accounts did not list the account, or not with the settings the
    /// load needs.
    AccountSettingsUnavailable,
    /// The account has neither SSL nor STARTTLS set in Online Accounts, so no
    /// password was requested.
    EncryptionNotConfigured,
    /// Online Accounts did not return the password.
    PasswordUnavailable,
    /// Online Accounts did not return the access token of an OAuth account.
    AuthorizationUnavailable,
    /// Online Accounts did not answer in time.
    OnlineAccountsNotResponding,
    /// The request to Online Accounts was cancelled. A cancelled load reports
    /// a cancellation instead, so no window shows this kind.
    AccountRequestStopped,
    /// The mail server said at this step that it is temporarily unavailable
    /// (RFC 5530 `UNAVAILABLE`).
    ServerUnavailable(ServerStep),
    /// The mail server rejected the sign-in with `AUTHENTICATIONFAILED` or
    /// without a code, which blames the credentials.
    ServerRejectedSignIn,
    /// The step failed otherwise: the server refused it, the connection broke
    /// or a reply could not be read.
    ServerStepFailed(ServerStep),
    /// The mail server stopped responding during the step.
    ServerNotResponding(ServerStep),
    /// The mail server offers no sign-in method the account can use.
    NoSignInMethod,
    /// The mailbox was replaced, or all its listed messages disappeared,
    /// during the load.
    MailboxChanged,
    /// The mail service could not be reached: no connection, a refused
    /// certificate or a broken transfer.
    ServiceUnreachable,
    /// The mail service stopped responding.
    ServiceNotResponding,
    /// The mail service refused the request with status 401.
    ServiceRejectedSignIn,
    /// The mail service refused the request with any other status.
    RequestRefused,
    /// The mail service's answer was not in the documented form.
    UnexpectedAnswer,
    /// A panic stopped the work, or the thread doing it vanished without one.
    Stopped,
    /// The store met a full disk, whatever the operation: SQLite's
    /// `SQLITE_FULL`, or the file system's own report while the store's
    /// files were prepared.
    StorageFull,
    /// Any other failure of a write to the store, its opening included.
    MailNotSaved,
    /// Any failure of a read from the store, its opening included.
    StoredMailUnreadable,
}

/// The steps of a session with a mail server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStep {
    Connect,
    /// TLS, certificate verification or STARTTLS.
    SecureConnection,
    SignIn,
    ListFolders,
    OpenMailbox,
    /// The message list or the messages' structures.
    FetchMessages,
    FetchText,
}

/// Words of the remote side, and who said them.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteText {
    pub source: RemoteSource,
    pub text: String,
}

/// Who said a remote text. The application turns it into the block's heading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteSource {
    /// An IMAP ALERT, which RFC 3501 requires the user to see.
    ServerAlert,
    /// The mail server's reply to the step that failed.
    ServerReply,
    /// The mail service's message about a refused request.
    ServiceMessage,
    /// The platform's text about a failed connection.
    System,
}

/// Why fewer messages arrived than the Inbox offered.
#[derive(Clone, PartialEq, Eq)]
pub enum IncompleteList {
    /// The server refused to finish the message list: its reply, with the
    /// sign-in name replaced, and its code.
    ServerRefused { reply: String, code: Option<String> },
    /// The mail service offered more messages than one request holds.
    MoreAvailable,
}

impl IncompleteList {
    /// The refusal's server code as a `Server code:` line, or nothing.
    pub fn technical_details(&self) -> String {
        match self {
            Self::ServerRefused {
                code: Some(code), ..
            } => format!("Server code: {code}"),
            Self::ServerRefused { code: None, .. } | Self::MoreAvailable => String::new(),
        }
    }
}

/// The text of a message, or why the reader shows none.
#[derive(Clone, PartialEq, Eq)]
pub enum ReceivedContent {
    Text(String),
    /// Why the content rules found no text to show.
    Explained(ContentExplanation),
    /// The server could not describe the message, so nothing was read.
    StructureUnreadable,
    /// The server or the service did not return the message's text.
    TextNotReturned,
}

/// Why a message shows no text, in terms the reader explains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentExplanation {
    /// No supported plain text. HTML-only mail is the usual case.
    NoPlainText {
        has_html: bool,
    },
    Encrypted,
    /// S/MIME, which this feature neither decrypts nor verifies.
    SecuredWithSMime,
    /// A character set mail-parser does not know.
    UnknownCharset(String),
    /// A Content-Transfer-Encoding no client knows.
    UnknownEncoding(String),
    /// The MIME entity itself could not be read.
    Undecodable,
}

impl ContentExplanation {
    /// Content this version does not show by design, as opposed to content
    /// that could not be read.
    pub fn is_by_design(&self) -> bool {
        matches!(
            self,
            Self::NoPlainText { .. } | Self::Encrypted | Self::SecuredWithSMime
        )
    }
}

// The remote side's words and received mail are shown to the user, never
// written to diagnostics; server text reaches the record only at debug, where
// the failure is built (specs/003-logging).
impl fmt::Debug for Folder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Folder")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Message")
            .field("identity", &self.identity)
            .field("seen", &self.seen)
            .field("content", &self.content)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for RemoteText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteText")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for IncompleteList {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServerRefused { code, .. } => formatter
                .debug_struct("ServerRefused")
                .field("code", code)
                .finish_non_exhaustive(),
            Self::MoreAvailable => write!(formatter, "MoreAvailable"),
        }
    }
}

impl fmt::Debug for ReceivedContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => write!(formatter, "Text({} characters)", text.chars().count()),
            Self::Explained(explanation) => write!(formatter, "Explained({explanation:?})"),
            Self::StructureUnreadable => write!(formatter, "StructureUnreadable"),
            Self::TextNotReturned => write!(formatter, "TextNotReturned"),
        }
    }
}
