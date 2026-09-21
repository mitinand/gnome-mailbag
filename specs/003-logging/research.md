# Research: Logging

Decisions for the [plan](plan.md). Each section gives the decision, the reason
and the alternatives that were rejected.

## 1. One owner of the logging domain

**Decision**: The `tracing` crate is what every Mailbag crate writes events
with. `tracing-subscriber` supplies the span registry, the level filter and
its one-line formatter, which writes to the standard error stream (§3);
Mailbag supplies the time (§4). `tracing-subscriber` is used
with `default-features = false, features = ["std", "registry", "fmt"]`, which
leaves out ANSI colors, the environment filter and the bridge from the `log`
crate.

**Why**: Three needs decide it.

- *Which message a line is about.* `mailbag-content` consists of functions
  that know nothing about messages, and `mailbag-imap` reads a structure
  without being told which message it belongs to. A span around one message
  carries its UID to every event inside it, in any crate, without passing a
  parameter through each function. When several accounts load interleaved on
  one thread, spans keep working where a thread-local "current message" would
  not. The account is not carried this way: one load runs at a time and the
  controller's own lines name it (spec FR-013).
- *Crate rules.* `mailbag-content` must not depend on GLib
  ([002 research §9](../002-imap-integration/research.md#9-crate-layout));
  `scripts/check.sh` enforces it. `tracing` has no such dependency.
- *Cost when off.* Without a subscriber a `tracing` call site is one cached
  check, and its field expressions are not evaluated (FR-001).

**Alternatives rejected**

- *The `log` crate.* `mailbag-imap` compiles it with `max_level_off` in both
  profiles, because async-imap writes whole input buffers through it. That
  switch is global to the binary: turning `log` on for Mailbag turns the
  library's trace on as well, and the privacy guarantee would then rest on a
  runtime filter by target instead of on code that is not compiled. It also has
  no context propagation. FR-012 stays satisfied only if `log` stays off.
- *GLib structured logging.* It cannot be used from `mailbag-content`. Its
  error level aborts the process, so Mailbag's "error" would have to be GLib's
  "critical". It has no context that follows a load across crates and
  threads. Its line format and its filter by environment variable are not
  ours to set without replacing the process-wide writer function, which would
  also take over GTK's and GLib's own messages, against FR-001. By default it
  writes info and debug to the standard output stream, against FR-003.
- *A hand-written facade.* Levels, a macro and a writer are small, but span
  storage and propagation through futures are what `tracing` exists for;
  rewriting them is the more expensive choice under constitution I.
- *A formatting layer of Mailbag's own.* It would fix the columns and escape
  every value whatever way it was passed. The library's formatter already
  prints the time, level, module, span fields and event fields on one line and
  writes string fields escaped; the layout is for people and the spec does not
  prescribe it. What the library does not protect is the message text and
  values passed for display, which §9 closes with a rule and a check. Storing
  span fields and walking nested spans again is not worth a chosen bracket.
- *`tracing-appender`.* Nothing here needs a non-blocking writer or file
  rotation (§3).

**Cost**: checked on 2026-09-21 in a scratch project with `tracing` 0.1.44
(`default-features = false, features = ["std"]`, so no proc macro is built)
and `tracing-subscriber` 0.3.23 with the features above. Seven crates are new
to `Cargo.lock`: `tracing`, `tracing-core`, `tracing-subscriber`,
`sharded-slab`, `thread_local`, `lazy_static` and `once_cell`; `cfg-if` and
`pin-project-lite` are there already. `tracing-log`, the `log` crate and ANSI
color crates are not pulled in. All seven are MIT or MIT OR Apache-2.0, which
`deny.toml` allows, and each ships its license files, so no notice exception
is needed. `cargo-sources.json` is regenerated. Portion 1 repeated the check
in the workspace on 2026-09-21: 0.1.44 and 0.3.23 were still the current
releases, and `Cargo.lock` pins them with `tracing-core` 0.1.36. The results
above and below held.

**What the formatter does**, from the same probe:

- A string field, borrowed or owned, is written escaped: a value holding a
  line break, quotes and a NUL came out as `"INBOX\nFAKE line \"q\" \0end"`
  on one line, in an event field and in a span field alike.
- A value passed with the `%` sigil is written raw: the same value broke the
  event into two lines. This is the gap §9 closes.
- Each event reaches the writer as one `write` call holding one whole line,
  so lines of different threads do not mix on the stream.
- A timer and a writer of our own are accepted; colors can be turned off.
- By default a write that fails is reported with `eprintln!`, which panics
  when the standard error stream itself cannot be written.
  `log_internal_errors(false)` turns the report off, so the line is dropped
  (FR-016).
- The line gives the time, the level, the enclosing spans with their fields,
  the module, the message and the event's fields, in that order; the layout is
  the library's and is for people to read (spec FR-014).

**Coexistence with the compiled-out library trace**: nothing changes in
`mailbag-imap`'s `log` dependency. `tracing`'s optional `log` feature and
`tracing-subscriber`'s `tracing-log` feature stay off, so no event crosses
between the two systems. `scripts/check.sh` gains a check that `tracing-log`
is absent from the dependency tree.

## 2. Turning logging on

**Decision**: `--log-level=<level>` is a GApplication main option, handled in
`handle-local-options`, which runs in the started process before it contacts a
running instance.

1. Without the option: return "continue"; nothing is installed.
2. Parse the value. Not one of `error`, `warning`, `info`, `debug`: print
   the accepted levels on the standard error stream and exit with status 1.
3. Register the application. If it is remote, a Mailbag is already running:
   print that logging was not turned on and that Mailbag must be quit first,
   exit with status 1. The running instance receives nothing.
4. Otherwise install the subscriber and write the first line.

**Why**: The primary instance never sees the second start's options unless
they are forwarded, and forwarding would mean changing a running process's
logging, which the spec excludes. Registering explicitly is how GApplication
lets a start learn that it is remote.

**Checked** on 2026-09-21 with a GApplication of GIO 0.22.9 on the session
bus, using this sequence:

- A first start with the option: not registered before `register()`, not
  remote after it; it became the primary instance and ran.
- A second start with the option while the first ran: remote after
  `register()`; it printed the message and ended with status 1, and the
  primary instance received no activation.
- A second start with an unknown level: the message, status 1, nothing
  registered, the primary instance undisturbed. The same without a running
  instance.
- A second start without the option: status 0 and the primary instance was
  activated, as today.
- `--help` lists `--log-level=LEVEL` with its description.

One consequence: `register()` emits the application's `startup` signal before
it returns, so `startup` handlers run before logging is on. Mailbag's only
`startup` handler registers its resources and has nothing to log, and the
window is built in `activate`, so no Mailbag event is lost; a later `startup`
handler that must be logged would have to move the start of logging. Not
checked here: the same sequence for the installed Flatpak, which the last
portion's acceptance covers.

**Alternative rejected**: An environment variable (excluded by the spec).

## 3. Writing to the standard error stream

**Decision**: The formatter writes each line straight to the standard error
stream, on the thread where the event happens. A write that fails is ignored.
There is no queue, no writer thread and no count of lost lines.

**Why**: The two ways a person gets a record, a terminal and `2> file`, do not
block. A full disk makes the write fail; it does not make it wait. The one
case that waits is a pipe whose reader has stopped, such as `… 2>&1 | less`
left unscrolled: the pipe fills, the next write waits, and Mailbag with it,
until the person scrolls. They set that up themselves, see it at once and can
undo it. GLib writes its own warnings to the same stream in the same way,
from the main thread, in every GNOME application.

Constitution V keeps blocking I/O off GTK's main thread. The maintainer
accepted on 2026-09-21 that output which exists only on request, of a few
lines per user action on the main thread, written the way GLib already
writes, is within that principle; most lines of a load come from the mail
worker's thread in any case.

**Alternative rejected**: A bounded queue with a writer thread, a count of
lost lines reported in the record and a bounded wait at quit. It would cover
the unread pipe at the cost of about a hundred lines, a thread, two service
lines that bypass the formatter, and tests with blocking and failing writers.
That is more mechanism than the one self-inflicted case deserves
(constitution I). It can be added behind the same formatter later without
changing any event.

Lines of one thread keep the order of their events. Nothing orders events
that happen at the same moment on different threads.

## 4. Time, versions and build kind

**Decision**: The line's time comes from `glib::DateTime::now_local()`,
formatted with milliseconds and the UTC offset, and is given to the library's
formatter as its timer. It lives in the `mailbag` crate, which already
depends on GLib.

The first line reads the version from the package, GTK and libadwaita versions
from their runtime functions, and the build kind from `/.flatpak-info`: when
the file exists, the build is Flatpak and the file's `runtime` key names the
runtime and its version; otherwise the line gives the operating system's
pretty name from GLib.

**Alternative rejected**: The `time` or `chrono` crates for local time. `time`
refuses to read the local offset in a multi-threaded process; `chrono` is in
the lock file only as someone else's dependency. GLib already does this.

## 5. Naming an account

**Finding**: GNOME Online Accounts 3.58 generates identifiers as
`account_<unix time>_<counter>` (`generate_new_id` in `goadaemon.c`). It also
accepts identifiers it did not generate: from an administrator's template
file, and from the `Id` entry of `AddAccount`'s details. Such an identifier is
arbitrary text, and a generated one holds the account's creation time.
`~/.config/goa-1.0/accounts.conf` has a section per identifier with the
account's provider and name.

**Decision**: A line names an account by its identifier, in the field
`account`; lines that observe an account add its provider type in
`provider`. The identifier stays the same across runs whatever other
accounts are added or removed, and the person who owns the record can find
the account it names. `goa-adapter`'s `AccountId::as_str` gives its text; the
001 account contract is amended for it.

The costs are accepted: a generated identifier shows when the account was
added, and an identifier from a template or an application's request is
written as it is, escaped like every string field. Both appear at every
level, and the README says so.

**Alternatives rejected**

- *A number given within the record*, with the provider type, such as
  `account-7 imap`, used first. With ten IMAP accounts it did not say which
  account a line meant, and the numbers shifted when an account was added or
  removed, so two records could not be compared.
- *The identifier in its generated form only*, and a number otherwise. It
  adds code for identifiers that a desktop set up through Settings does not
  have.

## 6. Server text and the sign-in name

**Decision**: `mailbag-imap` knows the sign-in name and owns one function that
returns server text for the log, with every occurrence of the
sign-in name replaced by `<login>`, whatever its length. The comparison
ignores ASCII case. A very short name can also match inside ordinary words of
the server's sentence; a damaged sentence is the accepted price, a name left
in the record is not.

The debug line with server text is written in `mailbag-imap`, where the name
and the reply are both at hand. The reply travels to the load and the UI
unchanged, as today; no sanitized copy crosses the crate boundary. The load
still writes the one error line, with the response code and without the text.

This changed a 002 design rule; the amended wording is in
[002 IMAP reading](../002-imap-integration/contracts/imap-reading.md) and
[002 data model](../002-imap-integration/data-model.md).

## 7. What a message's parts and headers may contribute

Which values of a mail message are available, and which of them the record may
carry. Where each line is written is a matter for the code that writes it.

- **A part's transfer encoding and size** come from the server's description of
  the part, which Mailbag already reads, so describing a part costs no request
  and no change to the part models.
- **A file name** is never recorded, and neither is its shape: nothing in
  Mailbag uses the name yet. That a text part was left out because it is a file
  may be recorded by the part's section number instead. The shape and the
  extension of a name arrive with the feature that handles attachments.

  For that feature, checked on 2026-09-21 by parsing nine part descriptions
  with the pinned IMAP parser: a value arrives as the server sent it. An
  encoded word (`=?UTF-8?B?…?=`) and the RFC 2231 forms (`UTF-8''%D0…`,
  continuations as separate parameters) are not decoded. Raw UTF-8 arrives
  intact. Bytes that are not UTF-8 are already replaced by U+FFFD, one per
  byte in the probe, and the original bytes are gone. Backslash escapes
  inside a quoted value are left in place. Parameter names keep the server's
  letter case; the part model lowers it.
- **A content identifier** may hold a domain, so it is never recorded. The
  choice of a related set's root depends on the set's `start` parameter and on
  those identifiers; whether `start` named a part can be recorded, the
  identifiers cannot.
- **An attached message's envelope** is a field of the same description and is
  never read for the record, so nothing has to be removed from it.
- **A description that cannot be parsed** leaves no data behind: the parser
  reports a failure and no structure reaches Mailbag. The two outcomes a reader
  can tell apart are that the server refused the request and that the reply
  could not be parsed; the description itself is not recorded.
- **A list header that did not decode** is not recorded at all, not even by
  its name. The row stays usable without it, the decoder reports no cause, and
  whoever turns such a report into a fixture has to examine the message by hand
  anyway. A description of the raw value's encoding was rejected for the same
  reason.
- **Why a TLS handshake failed** is not carried by the failure value the UI
  explains. GIO reports it at the point of failure as an error and as
  certificate flags, and only there: the error's code does not help, because
  glib-networking up to 2.90.0 reports a port that expects STARTTLS, which
  GnuTLS 3.8 answers with an unexpected packet, as `Misc`, and only the text,
  "An unexpected TLS packet was received", says what happened. That text
  consists of fixed phrases of the TLS library with no server data, so it is
  the one library error text the record may carry, together with the names of
  the certificate checks that failed and never a certificate's own fields. The
  TLS version of a successful handshake needs gio's `v2_70` feature; the GNOME
  50 runtime has a much newer GLib.
- **Capabilities and the TLS version** are already at hand, the capabilities
  before sign-in and the version after the handshake; no command is added for
  the record.

## 8. Testing a record

**Decision**: A test checks that no subscriber is installed without the
option, so no event can become a line on any code path; one start of Mailbag
without the option shows empty streams. A stray print outside `tracing` is a
matter for review. Results and unverified items go in the handoff report outside the repository, not in
quickstart.md.

The logging setup takes its output as a parameter. The
application passes the standard error stream; tests pass a buffer. A test
installs its subscriber for its own thread; the mail worker captures the
dispatcher of the thread that starts it and uses it on the worker thread, so
events of a load reach the test's buffer. In the application this captures the
global subscriber and changes nothing.

Privacy is checked by running loads against the scripted server of
`mailbag-imap`, whose fixtures contain marker strings, once at info and once
at debug, and searching the buffer (SC-002). A writer that fails every write
shows that a load still finishes (SC-008).

## 9. Values that reach a line

**Decision**: The library's formatter escapes string fields: a line break or
a control character in a value is written in escaped form, so one event stays
one line. It does not escape the message text or a value passed for display
with the `%` sigil. Two rules close the gap, and both are checked:

- the message of an event is fixed text written in the source; anything that
  comes from mail, a server or the system is a field;
- external strings are passed as plain string fields, never with `%`.

The sigil expands to a call of `tracing::field::display`, so `clippy.toml`
lists that function under Clippy's `disallowed-methods`, and the Clippy run of
`scripts/check.sh` rejects every such field in the four crates. Clippy sees the
expanded code, so a `%` operator, a comment or a string is never mistaken for
the sigil; it reports the first line of the event macro. A test writes a
folder name and a server sentence that contain line breaks, quotes and a NUL
and expects one line per event.

## 10. What the record deliberately does not do

Four decisions of 2026-09-21 that no other section holds.

- **The command line only.** `--log-level` is the one way to turn logging on.
  It appears in `--help`, passes through `flatpak run` unchanged, and a second
  start can say that logging was not turned on. An environment variable would
  do none of this and would be easy to leave set.
- **No option that writes to a file.** The record goes to the standard error
  stream and the person redirects it, so the host shell owns the file and the
  Flatpak sandbox needs no permission. A record left on for days needs a file
  that stays bounded; no such run exists before background synchronization,
  so that option arrives with it as an amendment to the spec.
- **Nothing is kept while logging is off**: no error file and no short record
  in memory written out when an error happens. A person learns about an error
  from the UI, at once and with technical details; the log is not a
  notification. Errors that happen with nobody present arrive with background
  synchronization, and a record for them is decided then.
- **The error line names failure values, not the UI's wording.** The error
  presentation feature rewrites that wording, and sharing it now would mean
  reworking UI code for the record. The record names the failure value the UI
  explains, such as `cause=TimedOut(SignIn)`.

## 11. Levels, durations and lists

Three more decisions of the specification, with what was rejected.

- **Levels are separated by outcome and granularity, not by privacy.** How an
  operation ended separates error, warning and info; how closely a line looks
  separates info, which speaks about an account and an operation, from debug,
  which speaks about a message, a part and a server. The privacy limits are laid
  over these two axes and mostly follow from them. *Rejected*: a level of its
  own for what is sensitive — a fifth level, or a privacy tier per field. It
  would have to be explained to every later feature, and a person choosing a
  level would have to reason about two scales at once; the values that must
  never be recorded (§6, §7) are excluded at every level instead, which needs no
  scale.
- **No line carries a duration.** Every line has its time with milliseconds, and
  the lines of one operation follow each other, so a duration is the difference
  between two lines, for a failed step as well. *Rejected*: a duration field,
  and a total on the operation's final line. The total helped only a record at
  level error, which has no first line to measure from, and a person reporting a
  problem is asked for debug; timing a decoding step in fractions of a
  millisecond is a matter for a benchmark. A duration field can be added to any
  line later, because the layout is not a contract.
- **No specification keeps a list of events or of fields.** The code that writes
  a line is the source of what is written, and review checks it there.
  *Rejected*: a list of every line, or of every field name, kept in this
  feature. Both were tried during the implementation and deleted: a list here
  forces a later feature either to come back and amend 003 or to keep a list of
  its own, and either way the list repeats the code and goes out of step with
  it. Only the few field names that cross crates are fixed, in the spec.
