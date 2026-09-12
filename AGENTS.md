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

## UI layout

The layout in `crates/mailbag/resources/ui/` is approved. Preserve widget arrangement, spacing,
dimensions, adaptive layouts, and action/menu placement. UI implementation connects
data, handlers, and behavior that cannot be expressed in the forms; missing or
nonworking handlers do not justify changing the layout. Obtain explicit maintainer
approval before changing it.

## Documentation

Keep the root `README.md` as the public introduction and getting-started guide.
Record accepted requirements and design decisions in the relevant specification.
Keep internal developer notes, implementation diaries, validation reports, and
inspection artifacts outside the repository; do not add duplicate design guides.
Agent instructions, required legal notices, and operational tooling files remain
in their appropriate locations. This applies to UI and icon work too.

## Commands

Use `scripts/setup.sh` for setup and `cargo run --locked` for native development.
Run `scripts/check.sh` before completion and report any checks that could not run.
