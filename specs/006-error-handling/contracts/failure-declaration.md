# Contract: The Failure Declaration

Shared by `mailbag-providers`, which declares, and `mailbag`, which shows.
The rules behind each field are in the [specification](../spec.md)
(FR-001 to FR-005, FR-009); this contract fixes the shape and the names.

## The type

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
    /// Runs the failed operation again: the window's refresh action.
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

| Carrier | Method | Channel in the window |
|---|---|---|
| A failed load | `LoadFailure::declare() -> DeclaredFailure` | The list's status page; Details opens the dialog |
| A short list | `IncompleteList::declare() -> DeclaredFailure` | The banner above the list; its button opens the dialog |
| A message's content | `ReceivedContent::declare() -> Option<DeclaredFailure>` (`None` for text) | The reader's status page in the body's place; no dialog |
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
  nothing is stored beyond the failure value the window already keeps.
- The action names: `Retry` is the window's `app.refresh-inbox`,
  `OnlineAccounts` is `app.accounts`; `failure_dialog::show_action_button` in
  `mailbag` is the one place that maps them to a label and an action name.
  A declaration never names a widget or an action string.
- `LoadFailure::cause_name`, `status` and `server_code` are the one source
  of the failure value, status and code for the record's error line (003
  FR-004) and for the technical details alike.
- Wording lives in the declaring code and follows FR-009 and AGENTS.md
  ("UI wording"); no document lists it.

## The panic string

`WorkerStopped` carries `Option<String>`: `<message> at <file>:<line>`, as
the panic hook received them. A panic in Mailbag's code has fixed text; a
library's panic may carry part of the text it was handling (FR-014).
Declared as the technical lines `Failure: WorkerStopped` and `Panic: …`.
