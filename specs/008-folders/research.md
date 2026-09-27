# Research: Folders

Decisions that had alternatives or needed a check, with what was checked
and how. The behaviour they serve is in [spec.md](spec.md); the persisted
form in [data-model.md](data-model.md).

## 1. Refresh Account as an action of its own

**Decision**: the folder list is obtained by Refresh Account, a maintenance
action in the main menu; Refresh Mailbox loads messages only.
**Rationale**: the maintainer's choice at clarification; keeps a mailbox
refresh at today's cost and makes the folder list an explicit, testable
step. The target application obtains lists without the user (spec
FR-013(e)), so the action is temporary.
**Alternatives**: the list with every load (one command more per refresh,
no second action; rejected at clarification); discovery when an account is
expanded (a load the user did not start; rejected).

## 2. Two defects of the pinned IMAP libraries

**Checked in source** (the fork repositories pinned in `Cargo.toml`, branch
`mailbag`):
- async-imap `src/parse.rs` `parse_names`/`filter_sync`: the LIST stream
  ends at the tagged completion and drops its status, so "two names, then
  NO" equals "two names, then OK". `parse_fetches` already ends with
  `Err(Error::No/Bad)` for FETCH (the fork's earlier fix). Without the fix a
  refused list would look complete and Refresh Account would delete stored
  folders with their mail (spec FR-001).
- imap-proto `src/parser/core.rs` `quoted`: nom's `escaped` keeps the
  escape characters, and async-imap's `quote!` escapes again on EXAMINE, so
  a name with `"` or `\` does not round-trip.
**Decision**: fix both on fork branches (`fix/list-completion-status`,
`fix/unescape-quoted`), tag and pin as the other patches; ~20 and ~15
lines with tests. No upstream contact.
**Alternatives**: read the raw response stream in `mailbag-imap` instead of
the library's LIST (more code in the application for a library defect);
declare such names unsupported (rejected by the maintainer: the standard
allows them).

## 3. Readable names: UTF-8 by capability, else modified UTF-7

**Checked on live servers** (2026-09-26): two servers answered OK to
`ENABLE UTF8=ACCEPT` and enabled nothing (the untagged `ENABLED` list was
empty), so names stayed in modified UTF-7; a third advertises
`UTF8=ACCEPT` and sends UTF-8 names after ENABLE (checked in 004).
**Decision**: send ENABLE only when the post-sign-in CAPABILITY lists
`UTF8=ACCEPT` or `UTF8=ONLY` (RFC 6855 §6: the latter includes the former
and requires the ENABLE), and treat names as UTF-8 then; otherwise decode
modified
UTF-7 (RFC 3501 §5.1.3) with a decoder of ~50 lines in `mailbag-imap`; a
name that cannot be decoded is shown as sent. The raw name stays the
folder's identity and is what EXAMINE receives.
**Alternatives**: the `utf7-imap` crate (a dependency for 50 lines);
reading the `ENABLED` reply (the fork parses it; the capability decides the
same and is already read for the sign-in method).

## 4. Roles and attributes

**Checked**: RFC 6154 makes every special-use attribute optional and allows
several per mailbox and several mailboxes per attribute; RFC 8457 adds
`\Important`; the IANA registry lists the rest (`\Memos`, `\Scheduled`,
`\Snoozed`, RFC 9979). Live servers mark from all system folders to only
some. Graph v1.0 has no role field; `$select=wellKnownName` is refused
(400), so roles come from `GET /me/mailFolders/{well-known}` (checked
2026-09-27).
**Decision**: every attribute a server lists is stored per folder; the
providers map attributes and well-known names to the application's nine
roles (spec FR-003); the first role mark wins; no name table. A server that
advertises `SPECIAL-USE` is asked with `RETURN (SPECIAL-USE)`, since RFC
6154 only lets a server include the attributes in a plain LIST.
**Alternatives**: a name table with localized names (the maintainer:
roles from the server only); Graph's beta `wellKnownName` (not in v1.0).

## 5. The Microsoft 365 folder list

**Checked on a live account** (2026-09-26): `/me/mailFolders/delta` returned
the whole tree flat in one page with a `deltaLink`, equal to the union of
the root listing and `childFolders` traversal; the default page is 10 with
`@odata.nextLink`; folder ids are identical with and without
`Prefer: IdType="ImmutableId"`; `childFolderCount` counts children the
service does not list.
**Decision**: read the delta listing page by page as the folder list (the
same mechanism synchronization will use), leave hidden folders out in the
Graph crate itself, count an id repeated across pages once with the last
occurrence winning and drop `@removed` entries (Graph's delta overview
allows both in the first round and warns against assuming an order), ignore `childFolderCount`, resolve the six well-known
names with 404 meaning "no such folder", and open a folder's messages by
`/me/mailFolders/{id}/messages`.
**Alternatives**: root listing plus recursive `childFolders` (documented
for full traversal; more requests; the delta listing is documented as the
way to keep a local copy of all folders).

## 6. Message identity and the membership relation

**Decision**: one `message` row per account and provider identity
(`gmail:<X-GM-MSGID>`, `graph:<id>`, `imap:<folder identity>/<uid>` for
Generic IMAP, whose message has no identity beyond its place); a
`membership` row per folder the message belongs to, carrying the UID and
the position; a mailbox load replaces the folder's memberships and upserts
messages by identity (spec FR-004).
**Rationale**: the target model of 007 FR-003, decided at clarification;
Geary's schema (`MessageTable` + `MessageLocationTable` with the UID as
`ordering`) and Thunderbird's ADR 0002 (a message key of its own, the UID
per folder) arrive at the same shape.
**Alternatives**: one folder column on the message with Gmail duplicates
per label (rejected at clarification); matching messages across folders by
date and size as Geary does (guessing; rejected).

## 7. Sidebar order and collation

**Decision**: roles first in the spec's order, then the user's folders by
the locale's collation at each level, through `glib::CollationKey` (GLib's
`g_utf8_collate_key`: "linguistically correct rules for the current
locale"; keys are recommended for sorting many strings). The sidebar sorts
siblings when it builds an account's subtree; the store returns folders
unordered, because `mailbag-store` does not depend on GLib and the order
belongs to the domain's `FolderRole::ORDER` and the sidebar (plan challenge
2026-09-27).
**Alternatives**: sorting in the store (a new dependency edge store → GLib
for a few dozen names); code-point order (wrong for mixed scripts); ICU (a
dependency the platform already covers).

## 8. Performance of the relation

**Checked by measurement** (SQLite in Python, 2026-09-27): a store of one
million messages and 1.33 million memberships in ten accounts of thirty
folders; the newest 100 of a folder by position 9 ms, a whole folder of
3 300 rows 9 ms, a clear-and-refill of one folder with orphan checks 62 ms,
deleting a folder with 6 700 memberships 113 ms, the unread counts of one
account's thirty folders 185 ms. All off GTK's thread; the last is the
counts feature's concern, not this one's.

## 9. The sidebar tree

**Checked**: `GtkTreeListModel` with `autoexpand` builds the tree from
child models; `GtkTreeExpander` indents one expander width per level with
no limit (GTK documentation); `SingleSelection` loses its selection when an
ancestor collapses (an earlier offline probe), which the spec turns into
a rule (FR-010). The approved `folder-row.ui` already wraps the row in a
`GtkTreeExpander`; Workbench's "List View with a Tree" is the demo followed.
**Decision**: account nodes in a `ListStore` updated in place by identity,
as today; each account's folder subtree rebuilt when its stored list
changed (`remove_all` and fill in sidebar order), since the spec keeps no
expansion memory and a Refresh Account is a maintenance action; an unchanged
list keeps the rows, so what the user collapsed stays collapsed; the
selection lives in the window's state, as today, and the shown mailbox's row
is marked again after the rebuild (plan challenge 2026-09-27: an
in-place diff would have to move rows across parents and reorder siblings
for ~50 lines with no requirement behind it).

## 10. Gmail

**Checked** (Google's documentation and the 004 probe): labels are
mailboxes, renamed with the standard RENAME; the container the system
labels live under is `\Noselect`; system labels carry SPECIAL-USE
attributes; `X-GM-MSGID` is the message's identity across labels;
`X-GM-LABELS` names the labels a message carries, leaving out the opened
one. Nothing documented identifies a label across a rename.
**Decision**: the Gmail provider drops the `\Noselect` container whose
children carry role attributes and lifts its children; memberships come
from loads only, labels are stored with the message (spec US4, FR-004); a
renamed label is a new folder. The container is found by its children's
attributes, not by its name, which is localized.

## 11. Where a failed Refresh Account shows

**Decision**: by 006 FR-006 without a new rule: the account's load failed →
the status page when nothing is shown, the banner over the shown mailbox's
rows otherwise (007 FR-005); Retry repeats Refresh Account
(`RetriedOperation::RefreshAccount`). An empty completed list is not a
failure: nothing is stored or shown differently, and the record says so
(the hiding of the account for the run was dropped on 2026-09-27: no
supported provider lists no mailbox). Each account
keeps one latest outcome with its target, as 007 FR-005 already says of
"the latest refresh of the account": a mailbox outcome shows when that
mailbox is shown, a folder-list outcome whenever the account's mailbox or
the account is shown, and a later load of the account replaces it, so two
notices never compete (plan challenge 2026-09-27). Folder lists that cannot
be read from the store are shown as 007 FR-013 shows an unreadable stored
list: the failure page with Details and a Retry that reads again; a silent
"not loaded" would hide stored mail without a word (review, 2026-09-27).
That Retry is the stored mail's one Retry, which reads the folder lists and
the shown mailbox together (2026-09-27, before the window was built).
