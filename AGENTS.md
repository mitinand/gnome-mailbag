# AGENTS.md

Read and follow `.specify/memory/constitution.md`, including outside Spec Kit
workflows. Use the installed Spec Kit skills for SDD stages. For other work,
consult the requirements and design relevant to the task.

## Autonomy and scope

Choose local implementation details independently within accepted requirements.
Propose better designs with concrete reasons and trade-offs. Obtain maintainer
approval before changing agreed behavior, security or data-integrity guarantees,
or materially revising an approved architecture, including shared interfaces,
ownership and persisted formats. Update affected documents before implementing
an approved change.

Design drafts are proposals, not additional binding requirements.

Keep changes scoped to the task and preserve unrelated working-tree changes.

## Commits, PRs and review pauses

The maintainer creates commits and PRs. The agent prepares and checks changes;
it must not create commits or open PRs.

Implement one agreed portion suitable for a commit at a time, including its tests
and required checks. Then stop and report what changed, what was verified, any
remaining limitations, a suggested commit message and the intended PR.
Wait for maintainer review and an explicit instruction before starting the next
portion. Address review feedback within the current portion before moving on.

When generating implementation tasks, include these review pauses at the agreed
commit boundaries. A request to implement a feature or PR does not remove them.

## Preparing work for review

Before handing over code or documents, check the result against constitution
principles I and II.
Keep the main plan focused on behavior, scope and decisions requiring review;
put detailed protocols and internal mechanics in linked supporting documents.
Reference shared decisions instead of repeating their rationale across files.

## Naming

Name functions for their action, types for their role, and variables for their
contents. Prefer two or three clear words over vague names such as `value`,
`check`, `State` or `Shared`. Include units in quantities and distinguish pending
commands, current state and task wakers. Keep conventional names such as `new`,
`from_glib` and `lock` when the enclosing type makes their meaning clear. Apply
these rules to tests and fixtures as well as production code.

## UI layout

The layout in `crates/mailbag/resources/ui/` is approved. Preserve widget arrangement, spacing,
dimensions, adaptive layouts, and action/menu placement. UI implementation connects
data, handlers, and behavior that cannot be expressed in the forms; missing or
nonworking handlers do not justify changing the layout. Obtain explicit maintainer
approval before changing it.

Every widget that a `.ui` file can declare is declared there: pages, boxes,
labels, buttons, banners, dialogs and their children. Code binds data, sets
text and visibility and connects handlers; it instantiates a form only for
repeated items (a row, a block) and never builds layout with widget
constructors. A new widget is a change to the forms, approved as such.
Code written before this rule moves into the forms when its feature is next
changed.

## UI wording

User-visible text never names Mailbag inside a sentence and never speaks for it
("Mailbag could not…", "Mailbag cannot decrypt it"). Write impersonally or name
the real actor, as GNOME applications do: "Could not reach the mail server",
"No password was sent", "This message cannot be decrypted". The name appears
only as a title: the window, About. This holds for every feature and every
string, `.ui` files included.

Widget structure, style classes and dialog shapes follow a GNOME Workbench
Library demo when one fits, unless the maintainer decides otherwise for a case.
Say which demo was followed, or that none fits.

## Documentation

Keep the root `README.md` as the public introduction and getting-started guide.
Specifications under `specs/` are the source of truth for accepted requirements
and design decisions. Keep them self-contained; do not refer to local documents
excluded from version control.
One spec per feature. Specify a feature that owns a lasting domain for the
target application, even when the current UI is temporary. When a later stage
needs more from a closed feature, amend that feature's spec, plan and tasks;
do not create a new spec that adds requirements to an existing feature's domain.
Keep internal developer notes, implementation diaries, validation reports, and
inspection artifacts outside the repository; do not add duplicate design guides.
Agent instructions, required legal notices, and operational tooling files remain
in their appropriate locations. This applies to UI and icon work too.

## Commands

Use `scripts/setup.sh` for setup and `cargo run --locked` for native development.
Run `scripts/check.sh` before completion and report any checks that could not run.
