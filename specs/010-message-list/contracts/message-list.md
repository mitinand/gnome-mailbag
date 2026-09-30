# Contract: Message list

Interfaces this feature changes or adds between components. Names are
binding across crates; bodies and private helpers are the plan's.

## Domain types (`mailbag-domain`)

- `Message.preview: String` — the preview a batch stores with the record
  (spec FR-003); empty when there is none.
- `MessageListRow.preview: String` — the stored preview as the list reads
  it.

## Content rules (`mailbag-content`)

- `pub const PREVIEW_PIECE_BYTES: u32 = 16_384` — how much of the chosen
  part is read for a preview (spec Assumptions).
- `pub fn select_preview_part(root: &MimePart) -> Option<PreviewPart>` —
  the part to read, `PreviewPart { section: Vec<u32>, is_html: bool }`:
  the first non-attachment `text/html` part the reader's walk met, else
  its first plain part; `None` when the message has no such part
  (research §2).
- `pub fn preview_of_piece(mime_header: &[u8], piece: &[u8], is_html: bool)
  -> String` — gives the piece a clean cut and decodes it with the
  reader's decoder (research §4), turns a page into words (research §5)
  and normalises them (research §6); empty when nothing readable comes
  out.
- `pub fn preview_of_text(text: &str) -> String` — normalises a text the
  service or a full read already provides (Microsoft 365's
  `bodyPreview`).

## IMAP reading (`mailbag-imap`)

- `TextRequest { uid, parts, limit: Option<u32> }` — a request with a
  limit reads each body section as `BODY.PEEK[<section>]<0.<limit>>`;
  requests are grouped by `(parts, limit)`. The received part is the
  header and the cut body; matching ignores the origin octet.

## Microsoft Graph (`mailbag-graph`)

- `GraphMessage.body_preview: Option<String>` — from `bodyPreview`, asked
  for with every full entry and single message (`CHANGE_FIELDS`).

## Store (`mailbag-store`)

- `message.preview` column ([data-model.md](../data-model.md)); the
  reads are unchanged in shape.

## Cycles (`mailbag-providers`)

- Every arrived `Message` of a batch carries its preview: on IMAP from
  the batch's structures and a partial read of the chosen part, with the
  plain part's piece as well for a message older than 30 days that has
  both forms (`imap_texts::read_contents` returns the preview with the
  content); on Microsoft 365 from `body_preview`. A refusal of the piece stores an
  empty preview with the row, as a refused text is stored today (009
  FR-009); a temporary refusal fails the cycle as before.

## Window (`mailbag`)

- `MailUi::show_rows(account_id, rows, change: ListChange)` with
  `ListChange::{AtOnce, Animated}` — the window passes `AtOnce` when the
  folder shown differs from the previous read's or the list had no rows,
  `Animated` otherwise (research §10); a new read empties the
  read-in-window set (research §11).
- `MailUi::set_unread_filter(bool)` — the toggle's state (spec FR-008).
- `MailUi::remove_in_window(identity)` — the trash button's removal (spec
  FR-010); applies FR-007 and the leaving animation; in a narrow window
  the reader's page is not brought forward.
- Row object `MessageItem` properties, bound in the row template:
  `sender`, `subject`, `date-text`, `preview`, `unread`,
  `read-state-text`, `shown`, `transition-ms`.
- Signal handlers the row template names, provided through the factory's
  `gtk::BuilderRustScope`: `row_entered`, `row_left` (the motion
  controller, with the trash revealer as their object), `trash_row` (with
  the list item as its object).

## Forms (`crates/mailbag/resources/ui/`)

- `message-row.ui`: the row as research §9 describes; ids `row_reveal`,
  `dot`, `sender`, `time`, `subject`, `preview`, `trash_reveal`, `trash`.
- `mailbag.ui`: `unread_filter` becomes sensitive and drives FR-008; the
  list's status page gets the wording "No unread messages" / "Every
  message in this folder is read." when the filter leaves no row.
