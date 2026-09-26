# Contract: The Failure Declaration

Between the layers that meet failures and the application, which declares
and shows them ([research §1](../research.md#1-where-the-declarations-live),
corrected on 2026-09-26). A lower layer hands over a failure in the domain's
terms, a value of `mailbag-domain`; `mailbag` writes the wording, chooses
the action and the channel. No protocol type reaches `mailbag`. The rules
behind each field are in the [specification](../spec.md) (FR-001 to FR-005,
FR-009); this contract fixes what each side provides and the shapes.

## The domain's types

In `mailbag-domain`, which depends on nothing in the workspace and is
reached by every layer.

```rust
/// A failure the user may be told about, as the layer that met it hands it on.
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

pub struct RemoteText {
    pub source: RemoteSource,
    pub text: String,
}

/// Who said the text. The application turns it into the block's heading.
pub enum RemoteSource {
    ServerAlert,
    ServerReply,
    ServiceMessage,
    System,
}

/// The steps of a session with a mail server.
pub enum ServerStep {
    Connect,
    SecureConnection,
    SignIn,
    OpenInbox,
    FetchMessages,
    FetchText,
}

/// Why fewer messages arrived than the Inbox offered.
pub enum IncompleteList {
    /// The server refused to finish the message list.
    ServerRefused { reply: String, code: Option<String> },
    /// The mail service offered more messages than one request holds.
    MoreAvailable,
}

/// A message's text, or why the reader shows none.
pub enum ReceivedContent {
    Text(String),
    Explained(ContentExplanation),
    StructureUnreadable,
    TextNotReturned,
}
```

`ContentExplanation` moves here from `mailbag-content` unchanged; that
crate returns it. `IncompleteList::technical_details()` gives the `Server
code:` line.

## The kinds

`FailureKind` has one variant per failure the window words differently
today, so that every text stays as it is. The layer that meets the failure
chooses the kind; reading a protocol's codes happens there.

| `FailureKind` | Produced by | From |
|---|---|---|
| `AccountSettingsUnavailable` | providers | `AccessError::Settings` |
| `EncryptionNotConfigured` | providers | `AccessError::NoEncryption` |
| `PasswordUnavailable` | providers | `AccessError::Password` |
| `AuthorizationUnavailable` | providers | `AccessError::AccessToken` |
| `OnlineAccountsNotResponding` | providers | `AccessError::Timeout` |
| `AccountRequestStopped` | providers | `AccessError::Cancelled`, which reaches no channel: the load reports a cancellation (FR-010); the variant keeps the conversion total |
| `ServerUnavailable(ServerStep)` | providers | An IMAP failure at any step whose code is `UNAVAILABLE` (RFC 5530) |
| `ServerRejectedSignIn` | providers | `ImapFailure::Failed(SignIn)` with `AUTHENTICATIONFAILED` or no code |
| `ServerStepFailed(ServerStep)` | providers | Any other `ImapFailure::Failed(step)` |
| `ServerNotResponding(ServerStep)` | providers | `ImapFailure::TimedOut(step)` |
| `NoSignInMethod` | providers | `ImapFailure::NoSignInMethod` |
| `InboxChanged` | providers | `ImapFailure::InboxChanged` |
| `ServiceUnreachable` | providers | `GraphFailure::ConnectionFailed` |
| `ServiceNotResponding` | providers | `GraphFailure::TimedOut` |
| `ServiceRejectedSignIn` | providers | `GraphFailure::Refused` with status 401 |
| `RequestRefused` | providers | `GraphFailure::Refused` with any other status |
| `UnexpectedAnswer` | providers | `GraphFailure::InvalidReply` |
| `Stopped` | whoever caught the panic | A panic on a worker thread, or a worker that vanished without one (FR-014) |
| `StorageFull` | the store | SQLite or the file system reports a full disk |
| `MailNotSaved` | the store | Any other failure of a write, the store's opening included |
| `StoredMailUnreadable` | the store | Any failure of a read, the store's opening included |

A later layer adds its own kinds under the same rule; the store's three were
added by [007](../../007-mail-storage/research.md#8-types-and-crates) on
2026-09-26.

## What a lower layer provides

| What | Why it stays below |
|---|---|
| The kind | Interpreting a protocol's codes belongs to the layer that speaks it |
| The remote texts: for IMAP the alerts (`ServerAlert`) then the reply (`ServerReply`); for Microsoft Graph the service's message after a refusal (`ServiceMessage`) or the platform's text otherwise (`System`) | Built where the failure is built, the sign-in name already replaced (research §3) |
| The technical details: `Failure:` with the kind (`ServerRejectedSignIn`, `ServerNotResponding(SignIn)`, `Stopped`), then the layer's own identifiers, `Status:`, `Server code:` or `Service code:`, `Panic:` | The status and the codes are the layer's own values; the kind names the failure the same way everywhere |
| The record's error line, written by the layer that gives the operation up, with `cause` naming the kind, as the `Failure:` line does (003 FR-004), and the status, the code and the number of alerts when it has them | Only that layer holds the status and the codes |

A lower layer never writes wording for the user, never names an action or
a widget.

## The declaration

Private to `mailbag`: built by `failure_declarations.rs`, shown by the
window, the reader and `failure_dialog.rs`.

```rust
pub struct DeclaredFailure {
    /// Names what failed, in a few words: fits one banner line at the
    /// list pane's narrowest width. The dialog's title, the status page's
    /// title, the toast's first sentence.
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
    /// Identifiers for the maintainer, as the lower layer wrote them.
    pub details: String,
}

pub enum FailureAction {
    /// Runs the failed operation again; the window chooses the operation
    /// from what carries the failure.
    Retry,
    /// Opens the system's Online Accounts settings.
    OnlineAccounts,
}
```

The block's heading comes from the source, in `mailbag`: "Alert from the
mail server", "Reply from the mail server", "Message from the mail
service", "From the system".

## Who declares

All in `mailbag/src/failure_declarations.rs`, one exhaustive `match` per
value, so a new variant without a declaration does not compile.

| Carrier | Function | Channel in the window |
|---|---|---|
| A failed operation | `declare_failure(&Failure) -> DeclaredFailure` | The list's failure page when nothing is stored; over stored rows, the banner (007); Details or the banner's button opens the dialog |
| A stored Inbox that cannot be read (added by [007](../../007-mail-storage/research.md#10-retrys-operation-for-a-failure-the-window-reads-itself)) | `declare_failure(&Failure)` | The list's failure page, whose Retry reads the stored Inbox again |
| A short list | `declare_short_list(&IncompleteList) -> DeclaredFailure` | The banner above the list; its button opens the dialog |
| A message's content | `declare_content(&ReceivedContent) -> Option<DeclaredFailure>` (`None` for text) | The reader's status page in the body's place; no dialog |
| A Settings launch | `LaunchError::message()` (in `mailbag`): one line, title and advice, no `DeclaredFailure`, since the toast shows nothing more | A toast |

A failed load reaches the window as `LoadResult::Failed(Failure)`; a panic on
the worker as a `Failure` of kind `Stopped`.

## What the window does with each field

| Field | Status page | Banner | Reader status page | Toast | Dialog |
|---|---|---|---|---|---|
| `title` | title, with the warning icon | the one line | title, with the warning icon | — | header bar title |
| `explanation`, `advice` | the escaped description, two paragraphs | — | the escaped description | — | two paragraphs |
| `action` | the action button (label, action name) | — | the action button | — | the action button, closes the dialog |
| `remote_texts` | — | — | — | — | one block each, in order, under its source's heading |
| `details` | — | — | — | — | the last block, "Technical details" |
| Details button | always: a failed load always has technical lines | the button is always there (every failure has an explanation) | never | never | — |

The copy button puts on the clipboard: the title, the explanation, the
advice, each remote text under its heading, the technical lines under
"Technical details", in that order, separated by blank lines, skipping
what is empty.

The toast shows `LaunchError::message()`, one line, title and advice, and
nothing of this table.

## Invariants

- Nothing in a failure or a declaration may hold what the record may not
  hold (003 FR-009): no password, token, sign-in name, address, subject or
  body. Server text arrives already masked (`<login>`); a declaration never
  formats a message's headers or body into any field.
- A declaration is built when a channel needs it, from the domain value;
  nothing is stored beyond the value the window already keeps. A failure
  that happens with no window open needs no wording until a window shows
  it.
- `mailbag` depends on no protocol crate; `failure_declarations.rs` matches
  domain types only.
- The action names: `Retry` runs the failed operation, which the window
  chooses from the carrier as a `RetriedOperation`: a load's failure, a
  short list and a message's content refresh the Inbox,
  `app.refresh-inbox`; a stored Inbox that cannot be read is read again,
  `app.read-stored-inbox` (amended by 007 on 2026-09-26). `OnlineAccounts`
  is `app.accounts`. `failure_dialog::show_action_button` in `mailbag` is
  the one place that maps them to a label and an action name. A declaration never names a
  widget or an action string.
- The technical details and the record's error line name the same kind
  and, for a load that met a remote failure, the same status and codes (a
  store failure's SQLite code is in the details and in the store's debug
  line, amended by 007 on 2026-09-26); the declaration copies the details as the
  lower layer wrote them.
- Wording lives in `failure_declarations.rs` and follows FR-009 and
  AGENTS.md ("UI wording"); no document lists it. The texts' headings are
  wording; the technical details are not and stay in English.

## The panic string

A caught panic is `<message> at <file>:<line>`, as the panic hook in
`mailbag-domain` received it. A panic in Mailbag's code has fixed text; a
library's panic may carry part of the text it was handling (FR-014). The
`Failure` of kind `Stopped` carries it in the technical line `Panic: …`
after `Failure: Stopped`. The panic is read on the thread where it
happened, by the code that sent the work there (research §4).
