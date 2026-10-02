# Quickstart: Message list

How the feature is checked. Automated checks run with `scripts/check.sh`;
the installed-build checks are the maintainer's, on a Flatpak build
(`README.md`), with an account of each provider.

## Automated

- `mailbag-content`: previews for every supported form (spec SC-001):
  plain text; a page only; both forms; base64 and quoted-printable; UTF-8,
  KOI8-R/Windows-1251 and ISO-8859-1; an attachment before the text; a
  nested message; a page with styles, scripts, a hidden opening line,
  images, links and comments; a piece cut inside a base64 group, a
  quoted-printable escape and a multi-byte character; a page of images
  alone; an encrypted message. Each preview starts with the message's
  first words and holds no markup, style, script, address or header text.
- `mailbag-imap`: a request with a limit sends `<0.N>` and its answer is
  matched; the scripted server answers partial sections.
- `mailbag-providers`: an IMAP batch stores rows with previews for recent
  and old messages, and an empty preview for a refused piece; a Microsoft
  365 batch stores `bodyPreview` normalised (spec SC-002 on the scripted
  servers).
- `mailbag-store`: the column is written and read.
- `mailbag`: unit tests of the next-message rule and the shown-rows
  derivation for every case of spec FR-007, with and without the filter
  (SC-003), and of the date wording against a fixed clock, with the
  locale's time form told from samples of both forms (SC-007); GUI tests,
  one per process: the filter lists the unread
  rows and keeps the open message (SC-004); read on opening after the
  timer and not before, and the stored state after a read (SC-009); the
  trash button removes the row, opens the next message and changes
  nothing in the store (SC-010); arrivals and removals of an animated
  load set the row objects' animation state and the model changes after
  the timer, while a folder shown anew changes at once (SC-005's states;
  its timing is checked on the installed build); a folder of 100 000 rows
  with previews (SC-006).

## Installed build

1. Refresh the largest folder of each account and note how long the
   first fill takes (written into the spec's Assumptions). Every row
   shows sender, date, subject and a two-line preview; the previews match
   the messages' opening words as another client shows them, including
   newsletters and messages in Cyrillic (SC-001, SC-008). Stop the
   network and scroll the whole list: every row keeps its preview
   (SC-002).
2. Open an unread message: its dot goes out about a second later; open
   another within a second: the first keeps its dot (SC-009).
3. Hover a row: the trash icon slides in after the date, moving the
   date aside, and turns red under the pointer; press it on the open message: the row slides shut and the
   message the rule names opens (SC-010, SC-005). Try each case of spec
   FR-007 by choosing rows with read and unread neighbours (SC-003).
4. Turn on "Show unread only": only unread rows remain; the open message
   stays while open and leaves when another is opened; a folder with no
   unread message says "No unread messages" (SC-004).
5. Refresh the folder: messages read by opening and removed by the trash
   button are listed again as the store holds them (spec FR-009, FR-010).
6. Mark a message read and delete another in a different client, then
   refresh while the folder is shown at its top: the removed row slides
   shut, the read state changes in place; receive a new message and
   refresh: its row slides open at the top and stays in view (SC-005).
   Turn animations off in Settings › Accessibility and repeat: the
   changes are immediate.
7. Check the dates: today's message shows its time in the locale's
   12- or 24-hour form, yesterday's "Yesterday", this week's the weekday,
   older ones the day and month or the date (SC-007).
8. In a narrow window, press the trash button on the list page: the next
   message opens without the reader's page coming forward.
