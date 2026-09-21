# Data model: Logging

Logging keeps no data between runs and adds no application state that the UI
reads. These are the few values it holds while a run with logging on lasts.

| Value | Owner | Contents | Lifetime |
|---|---|---|---|
| Chosen level | `mailbag::logging` | One of error, warning, info, debug | Set once at start; never changes during the run |
| Line queue | `mailbag::logging` | A bounded number of formatted lines waiting for the writer (1,024 to start with; an internal value) | The run |
| Lost-line count | `mailbag::logging` | Lines dropped since the writer last reported them | Reset each time it is reported |
| Operation counter | `mailbag::logging` | The last number given to a load | The run; starts at 1 |
| Account labels | `mailbag::logging` | For identifiers Online Accounts did not generate: identifier → `account-N` | The run. Generated identifiers need no entry |
| Load span | `mailbag::inbox_load` | Account label, operation identifier | From Refresh Inbox to the load's result |

With logging off none of these exist: no subscriber is installed, no thread is
started, and operations take no numbers.

## Lost lines

A line that does not fit into the queue, or whose write fails, is counted and
dropped. The writer keeps taking lines from the queue while writes fail, so
the queue cannot stay full because of a reader that went away, and it tries
to write again with every line. The first write that succeeds is preceded by
the count. This is behavior of the writer, not a state anyone else reads.

## What is deliberately not modelled

- No record of past operations, errors or lines is kept in memory
  ([spec, Clarifications](spec.md#clarifications)).
- No per-account or per-component level.
- No structured form of a line after it is formatted; the format is not a
  contract for programs.
