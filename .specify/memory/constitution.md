<!-- Sync Impact Report
Version: 2.0.2 -> 2.1.0 (expanded guidance).
Modified principles:
- I. Necessary complexity only: require a current failure scenario and compare
  dependency costs with an in-project implementation.
- II. Concrete domain language -> Clear language and concrete names: cover code,
  documents and reviewability explicitly.
Governance: state amendment versioning and compliance review expectations.
Added sections: none. Removed sections: none.
Templates: unchanged; Spec Kit reads the constitution at runtime.
Deferred items: none.
-->
# Mailbag Constitution

## Core Principles

### I. Necessary complexity only

Use the simplest design that meets current requirements. Before adding an
abstraction, configuration option or recovery mechanism, identify a concrete
scenario in the current feature that fails without it and explain the consequence.
Defer mechanisms justified only by future features. Required security and
data-integrity safeguards remain part of the current requirements.

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

**Version**: 2.1.0 | **Ratified**: 2026-09-08 | **Last Amended**: 2026-09-12
