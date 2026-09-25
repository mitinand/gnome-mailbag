# Contract: The Failure Declaration

Between the layers that meet failures and the application, which declares
and shows them ([research §1](../research.md#1-where-the-declarations-live)).
A lower layer hands over a typed failure value; `mailbag` writes the
wording, chooses the action and the channel. The rules behind each field
are in the [specification](../spec.md) (FR-001 to FR-005, FR-009); this
contract fixes what each side provides and the declaration's shape.

## What a lower layer provides

Today one lower layer reaches the window, `mailbag-providers`, with three
failure values: `LoadFailure`, `IncompleteList` and `ReceivedContent`. For
each it provides:

| What | Where | Why it stays below |
|---|---|---|
| The typed value itself | `LoadFailure`, `IncompleteList`, `ReceivedContent` in `batch.rs` | The layer's own terms: step, outcome, status |
| The remote side's texts | the fields of the protocol errors inside the value (`ImapError::alerts`, `server_reply`; `GraphError::reason`) | Built where the failure is built, the sign-in name already replaced (research §3) |
| Technical details | `LoadFailure::technical_details()`, `IncompleteList::technical_details()`: one `Label: value` line per identifier, in English | They name the layer's own values, the same as the record's error line |
| The record's values | `LoadFailure::cause_name`, `status`, `server_code` | One source for the error line (003 FR-004) and the details |
| Protocol facts | `LoadFailure::credentials_rejected()`: the server or the service rejected the sign-in because of the credentials, as far as its code tells (IMAP `AUTHENTICATIONFAILED` or no code at sign-in; Graph status 401); `LoadFailure::server_temporarily_unavailable()`: IMAP `UNAVAILABLE` | Interpreting a protocol's codes belongs to the layer that speaks it |

A lower layer never writes wording for the user, never names an action or
a widget. A later layer (the store, a synchronization engine) provides the
same kinds of things for its own values.

## The type

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
    /// Identifiers for the maintainer, one `Label: value` per line; empty
    /// when the failure has nothing beyond its explanation.
    pub details: String,
}

pub enum FailureAction {
    /// Runs the failed operation again; the window chooses the operation
    /// from what carries the failure.
    Retry,
    /// Opens the system's Online Accounts settings.
    OnlineAccounts,
}

pub struct RemoteText {
    /// The block's heading: "Alert from the mail server", "Reply from the
    /// mail server", "Message from the mail service", "From the system".
    pub source: &'static str,
    /// The text as received, with `<login>` in place of the sign-in name.
    pub text: String,
}
```

## Who declares

All in `mailbag/src/failure_declarations.rs`, one exhaustive `match` per
value, so a new variant without a declaration does not compile.

| Carrier | Function | Channel in the window |
|---|---|---|
| A failed load | `declare_load_failure(&LoadFailure) -> DeclaredFailure` | The list's failure page; Details opens the dialog |
| A short list | `declare_short_list(&IncompleteList) -> DeclaredFailure` | The banner above the list; its button opens the dialog |
| A message's content | `declare_content(&ReceivedContent) -> Option<DeclaredFailure>` (`None` for text) | The reader's status page in the body's place; no dialog |
| A Settings launch | `LaunchError::message()` (in `mailbag`): one line, title and advice, no `DeclaredFailure`, since the toast shows nothing more | A toast |
| A panic on the worker | `LoadFailure::WorkerStopped(Option<String>)`, the panic's message and place, declared as a failed load | As a failed load; the panic as one technical line |

`LoadFailure::OnlineAccounts(AccessError::Cancelled)` never reaches a
channel; the load reports `LoadResult::Cancelled` instead (FR-010).

## What the window does with each field

| Field | Status page | Banner | Reader status page | Toast | Dialog |
|---|---|---|---|---|---|
| `title` | title, with the warning icon | the one line | title, with the warning icon | — | header bar title |
| `explanation`, `advice` | the escaped description, two paragraphs | — | the escaped description | — | two paragraphs |
| `action` | the action button (label, action name) | — | the action button | — | the action button, closes the dialog |
| `remote_texts` | — | — | — | — | one block each, in order |
| `details` | — | — | — | — | the last block, "Technical details" |
| Details button | always: a failed load always has technical lines | the button is always there (every failure has an explanation) | never | never | — |

The copy button puts on the clipboard: the title, the explanation, the
advice, each remote text under its source, the technical lines under
"Technical details", in that order, separated by blank lines, skipping
what is empty.

The toast shows `LaunchError::message()`, one line, title and advice, and
nothing of
this table.

## Invariants

- Nothing in a declaration may hold what the record may not hold
  (003 FR-009): no password, token, sign-in name, address, subject or body.
  Server text arrives already masked (`<login>`); a declaration never
  formats a message's headers or body into any field.
- A declaration is built when a channel needs it, from the failure value;
  nothing is stored beyond the failure value the window already keeps. A
  failure that happens with no window open needs no wording until a window
  shows it.
- The action names: `Retry` runs the failed operation, which the window
  chooses from the carrier; every carrier today is a load, so it is
  `app.refresh-inbox`. `OnlineAccounts` is `app.accounts`.
  `failure_dialog::show_action_button` in `mailbag` is the one place that
  maps them to a label and an action name. A declaration never names a
  widget or an action string.
- The technical details and the record's error line come from the same
  lower-layer values (`cause_name`, `status`, `server_code`); the
  declaration copies the details as the lower layer wrote them.
- Wording lives in `failure_declarations.rs` and follows FR-009 and
  AGENTS.md ("UI wording"); no document lists it. The texts' headings are
  wording; the technical details are not and stay in English.

## The panic string

`WorkerStopped` carries `Option<String>`: `<message> at <file>:<line>`, as
the panic hook received them. A panic in Mailbag's code has fixed text; a
library's panic may carry part of the text it was handling (FR-014).
Its technical lines `Failure: WorkerStopped` and `Panic: …` come from
`LoadFailure::technical_details()`. The panic is read on the thread where it
happened, by the code that sent the work there (research §4).
