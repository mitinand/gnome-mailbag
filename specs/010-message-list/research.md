# Research: Message list

Decisions that had alternatives or rested on a fact that was checked, in
the order the plan uses them. "Checked" names the source or the
experiment; the throwaway scripts of the feature-start are not kept.

## §1 One preview rule, made with the batch

**Decision**: A message's preview is made by `mailbag-content` from a
piece of the message's own text, by one rule for every provider, and is
carried in the batch that stores the message (`Message.preview`), so a
row and its preview are stored together (spec FR-003).

**Rationale**: The list must need no server while it is shown or scrolled
(FR-003), so the preview cannot be made on demand. Making it after the
rows, as a second pass per batch, would list rows without their third
line for a while and need one more store write and a "preview missing"
state; the batch already fetches structures and texts for recent
messages, so the preview piece joins that step.

**Alternatives considered**: on demand when a row is shown (network on
scroll, impossible offline); a second pass (more state, rows without
previews); previews only where a text is stored (old rows without a
third line, rejected by the maintainer).

## §2 Choosing the part: the web-page form first

**Decision**: The reader's walk over the message's part description
(`select_text_parts`) records, beside the plain parts it chooses, the
first `text/html` part that is not an attachment (no `attachment`
disposition, no file name unless marked inline); `select_preview_part`
returns that page section when there is one, else the first plain
section; the plain part is the fallback when the page yields no words
(spec FR-003(a), (e)). Encrypted and S/MIME-secured messages, and
structures the server could not describe, yield no part. (Changed at the
challenge: one walk owns the part rules instead of a second one.)

**Rationale**: The plain form of a newsletter is often empty or filler,
while its page holds the words the user would see. The reader keeps its
own choice (plain text) because it shows text, not a summary.

**Alternatives considered**: reuse the reader's choice (plain first):
worse previews for newsletters; a blind read of the first kilobytes of
the whole body without the structure: one round trip less, but a page
that starts with an attachment or a long head yields nothing, and the
piece must be parsed as a cut multipart body.

## §3 Reading a piece on IMAP

**Decision**: For every message a batch fetches rows for, the cycle
fetches its structure (already done for recent messages; now for all,
one command per batch of 100) and reads the chosen preview part
partially: `BODY.PEEK[<section>]<0.16384>` with the part's MIME header
(`<section>.MIME`, or the message header for a single-part message), one
command per distinct request shape, as texts are read today. Recent
messages still get their full plain text for the reader in the same
step. A message older than 30 days that has both forms gets the plain
part's piece in the same command as the page's, so FR-003(e)'s fallback
costs no second round trip (added at the challenge); a recent message's
plain text is fetched whole anyway.

**Checked**: the imap-proto fork parses the origin octet of a partial
response (`BodySection { index: Option<u32> }`, `parser/rfc3501/body.rs`),
and async-imap's `Fetch::section` matches by section path and ignores
the index, so no fork change is needed; only the command text in
`reader.rs` gains the `<0.N>` suffix. Structures in groups of 100 ran at
about 92 messages per second on the slowest of the probed servers
(009 research §3); a whole-folder structure request is never sent.

**Alternatives considered**: the server's own preview (RFC 8970
`PREVIEW`): none of the three probed servers announces it
(`CAPABILITY` before sign-in on Gmail, Yandex and iCloud; Gmail after
sign-in as well) — not relied on; the full part: a page of a newsletter
is often 50 to 200 kilobytes, the piece is enough for 400 characters.

## §4 Decoding a cut piece

**Decision**: The piece is given a clean cut, then decoded by the
reader's own `decode_text_part`: a trailing incomplete base64 group is
dropped first, and a replacement mark at the very end, left by a cut
multi-byte sequence, is dropped afterwards (spec FR-003(b)). A partial
quoted-printable escape needs no cut: the parser leaves it out (checked
at the implementation on 2026-09-30 by a test that cuts a
quoted-printable piece at every position). One decoder owns character sets,
encodings and their failures. (Changed at the challenge from an own
decoding path for pieces.)

**Checked** (throwaway crate, mail-parser 0.11.9): `MessageParser`
decodes a base64 body cut at a group boundary and a quoted-printable body
cut between escapes; cut inside an escape it still decoded the complete
prefix as a single entity, and inside a base64 group the reader's
`decode_entity` rejects the piece on purpose, hence the clean cut. A cut
multipart body with a plain part parses; a cut HTML body converts to
readable words; an 8-bit body cut inside a multi-byte character ends in
one replacement mark.

## §5 The words of a web page

**Decision**: HTML becomes words through mail-parser's `html_to_text`,
after a space is inserted before every block-level tag (`p`, `div`,
`br`, `li`, `tr`, `td`, `th`, `h1`–`h6`, `table`, `ul`, `ol`,
`blockquote`, `pre`, `hr`, `section`, `article`, `header`, `footer`,
`dd`, `dt`, opening or closing), so that adjoining blocks do not run
together (spec FR-003(c)).

**Checked** (throwaway crate): `html_to_text` drops `script`, `style`,
the head with its `title`, image alternative text and comments, decodes
entities and turns `br` into a line break; without the inserted space
"Hello Andrey,Your order" and "Item oneItem two" run together, with it
they are apart; a lone `<` in text swallows the words up to the next
`>` (valid HTML escapes it, accepted); a piece cut inside a `style`
element yields nothing, which FR-003(e) handles by the plain fallback.
It keeps the text of `nav`, `header` and `footer` elements and of
elements hidden by style (`display:none`), so a newsletter's hidden
opening line may lead the preview; accepted (§15).

**Alternatives considered**: an HTML parsing crate: a new dependency for
one function the existing one already provides (constitution I); own
tag stripping: would have to decode entities too.

## §6 Normalising the words

**Decision**: In `mailbag-content`, with the standard library only: CR,
LF and every character `char::is_whitespace` accepts, plus the braille
blank U+2800 and the Mongolian vowel separator U+180E, become one space
per run; control characters (`char::is_control`) and the common
invisible formatting characters (soft hyphen U+00AD, U+200B–U+200F,
U+202A–U+202E, U+2060–U+2064, U+2066–U+206F, U+FEFF) are removed;
combining marks are kept, since decomposed text (Vietnamese, some
Cyrillic) would lose its accents; the result is trimmed and cut at 400
characters on a character boundary; the row's two-line label cuts it
further (spec FR-002, FR-003(d)). Placeholders in square brackets stay
(§15).

**Alternatives considered**: a Unicode-category crate for the Cf class:
a dependency for a dozen code points.

## §7 Microsoft 365: the service's preview

**Decision**: `bodyPreview` joins the fields asked for with every delta
entry and single message (`CHANGE_FIELDS`), and is the source text of
the preview, normalised by §6; no body is read for it. A partial entry
(a read-state change) carries no preview and changes no record, as
today.

**Checked**: the message resource documents `bodyPreview` as "the first
255 characters of the message body. It is in text format" (Microsoft
Graph v1.0 reference). The delta query returns it under `$select`:
checked on 2026-10-01 with one live request of the cycle's `$select` and
order to a personal account's Inbox, whose first page of 10 entries all
carried a non-empty `bodyPreview` of at most 255 characters. The
scripted service returns it in tests.

## §8 The store

**Decision**: `message.preview TEXT NOT NULL`, empty when the message
has none; written with every arrived record (a fresh preview comes with
every fetched row, so the upsert takes the new value); read with the
rows. The schema hash changes, so the store is discarded once at start
(007 FR-012).

## §9 The row form

**Decision**: `message-row.ui` stays a `GtkListItem` template built into a
`GtkBuilderListItemFactory`; its child is a slide-down `GtkRevealer`
(§10) around a `GtkOverlay`: the row's box (indicator column with the
dot; a content column with the sender and date line, the subject, the
two-line preview) and, as the overlay's overlay child at the end and the
bottom, a crossfade `GtkRevealer` with the flat round trash button. A
`GtkEventControllerMotion` declared as a child of the row's box reveals
the button when the pointer enters and hides it when it leaves, through
signal handlers named in the form and provided by a
`gtk::BuilderRustScope` given to the factory; the button's `clicked`
handler receives the list item (`object="GtkListItem"`) and hands its
message to the window (spec FR-002, FR-010). Every widget is declared in
the form; code binds handlers only.

**Checked** (throwaway GTK 4.22 scripts): a controller declared as a
`<child>` of a widget in a list item template is added to that widget;
`<signal handler="…" object="trash_reveal"/>` and `object="GtkListItem"`
resolve inside the template and the handlers run when the signals are
emitted; `gtk::BuilderRustScope` exists in gtk4 0.11.4 and hands the named
object to the callback; since a signal with an object is swapped by
default, the object comes first among the values (checked at the
implementation on 2026-10-01). Cambalache cannot open a
`GtkListItem` template (009 research §9), so the form is edited as text
and presented for approval as a diff with a rendering.

**Alternatives considered**: a factory with code-built rows: the layout
would leave the forms (AGENTS.md); revealing the button only for the
open message: the maintainer wants it on hover; revealing it while the
row has the keyboard focus: dropped at the challenge, since the button
cannot be pressed from the keyboard until moving and deleting, and a
focus controller on the row's box may not even see the list item's
focus.

## §10 Animations in a list view

**Decision**: The row template's outer revealer (slide-down, 220 ms)
binds `reveal-child` and `transition-duration` to two properties of the
row object (`shown`, `transition-ms`). An animated change first closes
the rows that leave (`shown` false with the duration) and leaves the
list's model as it is; a timeout of 280 ms then applies the latest wanted
rows by the ordinary difference update, which takes the closed rows out
and inserts arriving rows with `shown` false, revealed on the second
frame after the insert. Closings started meanwhile, such as a second row
sent to the trash, are counted, and the list changes after the last. A
list off screen changes at once, since it draws no frames. When the list
was at its top before an insert, the list
is scrolled to its first row right after it. A change animates unless
the folder shown differs from the previous read's (the rows replace
another folder's or an empty list), the unread filter changed what is
listed, or the toolkit's animation setting is off; a first fill's
batches are appended below the rows shown, so only rows in view move
(spec FR-006; the condition "only a folder whose latest refresh
completed" was dropped at the challenge, since telling a fill from a
refresh needed the folder's state from before the load and one more
store answer). A closed row cannot be opened; a change at once, such as
a folder shown anew, takes closed rows out with the rest, and the
timeout then finds nothing to do. (Changed at the implementation on
2026-10-01: the first version kept leaving rows in the model through
later differences, merged by date, which made every step of the list
tell leaving rows apart; closing first and changing the model once
needs no state beyond `shown`.)

**Checked** (throwaway GTK 4.22 scripts, one scenario per process): the
list view has no animation of its own (its reference lists none; a
removed row vanishes at once); a revealer revealed from an idle callback
jumps open, since its widget is not mapped yet, and revealed on the
second frame it slides open; three rows revealed at the top while the
list stood at its top grew above the viewport, because the list keeps
its anchor row in place — `scroll_to(0)` right after the insert keeps
them in view, and when the list is scrolled down the rows in view stay;
a leaving row shrinks over the frames and the timer removes it without
a jump; rows recycled after a removal show no partial height in
isolated runs (one frame at a partial height was seen once in a combined
run, judged invisible); the toolkit's animation setting makes the
revealer change at once.

**Why the timer**: the form cannot call back into code when a transition
ends, and a leaving row may have scrolled off screen, where no widget
reports it; one timeout per change ends every leaving row of that change.
The animation state lives in the row object, not the widget, because
list views reuse widgets.


## §11 The unread filter as derived rows

**Decision**: The window keeps the folder's stored rows and derives the
rows the list shows: all of them, or, with the filter on, the unread ones
and the open message; every change (a new read, the toggle, another
message opened) updates the list by the existing difference update, so
arrivals, removals and rows leaving because they are read all go
through one path and animate or not by §10 (spec FR-008). The
window-only read state (spec FR-009) is a set of identities that the
read-on-opening timeout fills and a new read of the stored rows empties;
the derivation and the read-state reset of the difference update both
treat a row in the set as read, so a toggle or another opening does not
bring the dot back (found at the challenge).

**Alternatives considered**: a filter model over the row objects: a row
that becomes read would vanish at once instead of leaving as a removed
row, so a second mechanism would be needed for the animation, and the
open message's exception would need a filter that changes on every
opening.


## §12 The next message

**Decision**: When the user takes the open message's row out of the
list (the trash button; later moving and deleting), the window takes the
rows above and below it in the list as shown, other leaving rows left
out, and opens the row the rule names (spec FR-007) before the leaving
animation starts, by the same path as a click, so the reader and the
highlight follow; in a narrow window the reader's page is not brought
forward, so the user stays in the list. The rule is a pure function of
the two neighbours' states (none, read, unread), unit-tested by its seven
cases. The read state used is the row's as shown, which the window-only
read state (FR-009) may have changed. A refresh that removes the open
message closes the reader instead (009 FR-013): the window never opens a
message the user did not choose (decided at the challenge).

## §13 Read on opening, in the window only

**Decision**: Opening a message starts one timeout of one second; when it
fires, the identity joins the window's read set (§11) and the row
object's `unread` becomes false, which hides the dot and counts for
FR-007 and FR-008; opening another message or the message's leaving
drops a pending timeout. A new read of the stored rows empties the set
and sets every row's read state from the store again, which is how the
stored state returns (spec FR-009). Nothing is sent or stored.

## §14 Date wording

**Decision**: The row's date is made when the row object's `date-text`
is read: the received date and the current time are taken in the local
zone; the same day gives the time without seconds in the locale's form,
the day before "Yesterday", two to six days before `%A`, the same year
`%-d %B`, earlier `%x`; no usable date gives an empty string (spec
FR-004). The reader keeps `%c`. The locale's form is told by its own
full time format: when `%X` contains the locale's AM/PM marker (`%p`),
the time is `%-I:%M %p`, otherwise `%H:%M`; no locale offers a time
without seconds directly. (The maintainer's decision of 2026-09-30,
"option 2": the locale alone, after a fixed 24-hour form and the
desktop's Time Format setting were weighed.)

**Checked** (GLib 2.88 on this desktop): `%-d %B` gives "30 September",
`%A` the weekday, `%-I:%M %p` "10:44 PM", `%H:%M` "22:44", `%X`
includes seconds ("10:14:01 PM" in an en_US locale), `%p` "PM", `%x` the
locale's short date. That every 12-hour locale puts its `%p` text into
`%X` is inferred from glibc's locale definitions (`t_fmt` with `%p` where
`am_pm` is used); the rule is a pure function of the two sample strings
and is unit-tested with samples of both forms.

**Alternatives considered**: the desktop's `clock-format` setting
(`org.gnome.desktop.interface`), which GNOME's own applications read: a
sandboxed application reaches it only through the desktop portal's
settings interface (`org.freedesktop.portal.Settings.ReadOne`, checked
with `gdbus` to answer `'24h'` here and to announce changes), about 55
lines for one key; rejected by the maintainer as not worth it now.
GSettings directly would need dconf access in the manifest. A user who
switched the desktop setting away from the locale's form sees the
locale's form (spec Clarifications).

## §15 Considered and not handled

- Looking further than the two neighbours for an unread message: the
  maintainer's rule names the neighbours only.
- A newsletter's hidden opening line ("preheader") and navigation text
  at the top of a page lead its preview; removing elements hidden by
  style would need a style-aware parser.
- Placeholders in square brackets are left in a preview: a page's image
  text never reaches the words, and removing brackets would cut the
  sender's own words (decided at the challenge).
- Reaching the trash button by keyboard, and showing it while the row
  has the keyboard focus: with moving and deleting (spec FR-011(a)).
- A uniform row height when the preview is empty: the row is one line
  shorter; the list view estimates heights per row.
- Rows bound long ago keep their date wording until they are bound
  again; a day boundary passing while the window stays open is not
  watched.
