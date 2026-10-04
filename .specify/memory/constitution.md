<!-- Sync Impact Report
Version: 2.4.0 -> 2.4.1 (wording).
Modified principles: VIII, the scripted servers' default: the behaviour
that gives the application the least to infer from, not "the required
behaviour", which a standard that only recommends does not define.
Rationale: the consistency analyses of 2026-10-05 read the old wording as
the opposite of its intent for a STORE that changes nothing, where RFC 7162
§3.1.11 recommends leaving the mod-sequence; the scripted server does, and
that default is what exposed the hole VIII was written for.
Previous report (2.3.0 -> 2.4.0, new principle):
Modified principles: none.
Added principles:
- VIII. A fact about a message comes from the server's report of it: the
  outcome of the application's own change is never inferred from an OK, from
  the absence of a report or from a folder's numbers; a folder's numbers
  stand for its messages only where a standard requires the implication, and
  then decide how much is read, never what counts as proof; code relies on
  what a standard requires, handles what it recommends or allows both ways,
  and the scripted servers show the required behaviour by default.
Rationale: the reviews of read-and-star and of synchronization's state pass
on 2026-10-04 found holes with one cause: a flag command was confirmed by
inference (an OK, then a later listing, then unchanged folder numbers)
instead of by the server's report of the message; a server following the
standard's recommendation for a command that changes nothing escaped all
three, and the scripted server, built on the same inference, hid it.
Previous report (2.2.0 -> 2.3.0): added principle VII, Gmail first, each
provider on its own terms, after the 010 live check tuned preview reading on
one IMAP server's measurements and applied it to every server.
Added sections: none. Removed sections: none.
Templates: unchanged; Spec Kit reads the constitution at runtime.
Deferred items: none.
-->
# Mailbag Constitution

## Core Principles

### I. Necessary complexity only

Use the simplest design that meets current requirements. Before adding a
requirement, abstraction, configuration option or recovery mechanism, identify a
concrete scenario in the current feature that can occur with the supported
dependencies and fails without it, and explain the consequence. Defer mechanisms
justified only by future features. Required security and data-integrity safeguards
remain part of the current requirements.

Specifications list an edge case only when it passes this test, and state its
user-visible result. Record other considered cases as out of scope without
adding requirements.

Before adding a dependency, compare the functionality needed now with the cost of
implementing and maintaining it in the project. Consider correctness, testing,
maintenance, security updates, licensing and packaging. Record the choice and its
main trade-off in the relevant design or PR. Choose the lower overall cost for the
required behavior; minimizing dependency count alone is not a goal.

### II. Clear language and concrete names

Write code and documentation for a maintainer who knows the application but did
not participate in the design. Use familiar words and name the account, message,
folder, user action or system operation involved. Names must reveal what a value
represents or what an operation does.

Explain behavior through concrete situations before describing implementation.
Avoid invented terminology, vague umbrella names and chains of technical
qualifiers. Use established technical terms where they add precision, explain
unfamiliar terms at first use, and keep them within the relevant technical layer.
A glossary must not compensate for unnecessarily obscure writing.

Reviewability is a requirement: readers must be able to identify the behavior,
reason for a change and important trade-offs without reconstructing the author's
terminology.

### III. Explicit failures and truthful state

Surface failures promptly, preserve their cause and stop work that depends on
success. Never disguise failure or uncertainty as success, empty data or a
fabricated default. Keep diagnostics privacy-safe; never include credentials or
personal mail in logs or test fixtures. Components report errors; the UI presents
failed user actions and persistent problems that affect use.

Recoverable failures must not crash the whole application; expected cancellation
is not an error. Acknowledge durable local changes only after commit and distinguish
them from remote confirmation.

### IV. One owner per business rule

Each business rule has one authoritative owner. Callers use that owner's interface
rather than independently duplicating its policy. Different provider implementations
may fulfill the same contract without requiring a shared abstraction.

### V. Responsive, bounded work

Keep networking, SQL, blocking I/O and substantial computation off GTK's main
thread. Bound resource use and retries; prioritize reading and explicit user
actions over background work without violating transaction or data-integrity
guarantees.

### VI. Evidence before completion

Verify changed behavior in proportion to its risk, including failure and recovery
cases when relevant. Use automated tests for logic and invariants, and manual
verification where appropriate. Security, durability and compatibility claims need
matching evidence. Record what was checked and what remains unverified; component
tests do not prove installed-Flatpak integration.

### VII. Gmail first, each provider on its own terms

Gmail is the first-priority provider: most users are expected to have a Gmail
account. Design, measure and tune each capability for Gmail first. Treat each
provider by its own documentation and measured behaviour rather than as a case of
a generic protocol: Gmail by Google's documentation, including its IMAP
extensions and the Gmail API; Microsoft 365 by Microsoft Graph's.
Provider-specific logic is the expected design when the provider's documentation
or a measurement shows a better path for that provider; Principle I still
requires that reason.

### VIII. A fact about a message comes from the server's report of it

A fact about a message on the server, its flags, its presence in a folder, the
outcome of a change the application made, is established by the server's
report of that message: a listing or a fetch that names it with the values in
question, or the answer to the request about it on a service that answers per
message. The outcome of the application's own change is never inferred from
an `OK` that does not name the message, from the absence of a report or from
a folder's numbers. A folder's numbers stand for the state of its messages
only where a standard requires the implication, as RFC 3501 requires of
UIDNEXT when messages arrive and RFC 7162 of a message's mod-sequence when
its flags change; they then decide how much is read, never what counts as
proof of a change.

Code relies only on what a standard requires. A behaviour a standard
recommends or allows is handled both ways, and the scripted servers used in
tests show by default the behaviour that gives the application the least to
infer from, with the other behind an explicit setting when a test needs it,
so that a test cannot pass on an assumption. Extensions decide how much is
read, never what counts as proof.

## Public Repository Language

All repository content and maintainer-authored issues, PRs and release notes must
be in English. Translated application content requires an approved amendment;
private discussion is unrestricted.

## Governance

These principles are binding for design, implementation and review.

Routine maintenance needs no artificial feature. A feature may span several
coherent, reviewable PRs.

Amendments require maintainer approval, a recorded rationale and a version update;
revise affected documents accordingly. Do not weaken a principle merely to justify
an implementation. Review design and code changes against these principles. Use
semantic versioning for amendments; constitution versions are independent of
application releases.

**Version**: 2.4.1 | **Ratified**: 2026-09-08 | **Last Amended**: 2026-10-05
