# Data model: Logging

Logging keeps no data between runs and adds no application state that the UI
reads. These are the few values it holds while a run with logging on lasts.

| Value | Owner | Contents | Lifetime |
|---|---|---|---|
| Chosen level | `mailbag::logging` | One of error, warning, info, debug | Set once at start; never changes during the run |
| Load span | `mailbag::inbox::RunningLoad` | Account identifier; clones used by the loader and its callbacks | From an accepted Refresh Inbox until the result is accepted or discarded; cancellation is recorded before dropping the handle |

With logging off none of these exist: no subscriber is installed.

Cancellation and result applicability remain decisions of `InboxController`,
using its existing cancellation handle and `show_result` check. Logging adds
no second load state machine. A cancelled load may still have a pending
callback; that callback does not write a second final line
([record contract](contracts/record.md#load-outcomes-and-cancellation)).

The completion summary uses only what the received batch already holds.
Messages that disappeared during a load are the difference between the rows
of the "message list loaded" line and the messages of the final line; no
count is carried for the log, and their UIDs are debug lines where they are
observed.

## What is deliberately not modelled

- No record of past operations, errors or lines is kept in memory
  ([spec, Clarifications](spec.md#clarifications)).
- No per-account or per-component level.
- No queue of lines and no count of lost lines; lines go straight to the
  standard error stream ([research §3](research.md#3-writing-to-the-standard-error-stream)).
- No structured form of a line after it is formatted; the format is not a
  contract for programs.
