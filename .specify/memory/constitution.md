<!-- Sync Impact Report
Version: 2.0.1 -> 2.0.2. Remove duplicated Spec Kit workflow instructions.
Core principles unchanged. Specification Ownership removed; governance condensed.
Constitution Checks and versioning mechanics remain in Spec Kit skills.
Deferred items: none.
-->
# Mailbag Constitution

## Core Principles

### I. Necessary complexity only

Use the simplest design that meets current requirements. Additional abstractions,
configuration or recovery paths must solve a concrete current problem, not a
hypothetical future need. Required security and data-integrity safeguards are
not speculative complexity.

### II. Concrete domain language

Name business functions, types and modules after concrete mail or application
entities and actions. Use established technical terms where appropriate; explain
necessary unfamiliar terminology in plain language.

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
an implementation. Constitution versions are independent of application releases.

**Version**: 2.0.2 | **Ratified**: 2026-09-08 | **Last Amended**: 2026-09-09
