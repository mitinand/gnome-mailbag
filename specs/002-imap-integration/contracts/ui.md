# IMAP Integration UI Contract

Preserve the approved geometry, navigation, breakpoints, spacing and control
placement. The Refresh Inbox menu addition is already approved. Reuse the
existing loading box and spinner; no widget replacement or reader redesign.

## One page decision

WindowUi projects account and mail state after selection, account updates, load
start and completion. AccountUi and MailUi must not independently overwrite
list_stack. AccountList remains the owner of F01 rules.

| Priority | Existing surface |
|---|---|
| F01 page is not SelectedAccount | Existing account loading/error/setup/missing-Mail page and its controls. This takes priority over mail states. |
| Selected account's load is running | Existing status page says Loading Inbox with the spinner; it does not name the current step. |
| Selected account has a batch with rows | Message list. |
| Selected account has a confirmed empty batch | Empty-Inbox status. |
| Selected account's last load failed | Status names the failed step; Refresh Inbox tries again. |
| Nothing loaded for the selected account | Neutral status saying nothing has been loaded; for Generic IMAP it points to Refresh Inbox. Never “Inbox is empty”. |

A GOA failure lets F01's page cover the list and reader; received mail is not
touched. Confirmed exclusion discards the account's mail. An account failure and
a mail failure remain separate diagnoses. F01's Retry Check remains account
observation, never a mail refresh.

## Controls and feedback

- Add `app.refresh-inbox` immediately after Synchronization Status. Enable it
  for the selected, last-known enabled Generic IMAP account while Idle.
  AttentionNeeded or a temporary GOA failure does not prohibit an explicit
  attempt; the access request determines the actual failing step.
- Refresh Inbox is the only way to load; selecting an account never loads.
  Disable Refresh while a load is running; do not queue it.
- Show `sync_button_list` only while a load is running. Select the existing
  spinner in `sync_icon_list`. Do not show a success checkmark or warning icon,
  replace the box, open `sync_popover` or implement `app.sync-status`. Keep
  Synchronization Status unavailable in this feature.
- A failed load is shown on its account's status page. No toast, persistent
  synchronization error panel, progress percentages, notification history or
  successful-load notice. Cancellation is silent.

ALERT handling is deliberately small: include any ALERT text received during
the failed attempt in that failure's explanation. Use plain-text setters. An ALERT on an otherwise successful attempt does
not create a notification, fail the load or establish a separate UI subsystem.
See the limited protocol-support decision in [research](../research.md#6-ui-and-account-ownership).

## List and reader binding

| Existing widget | Behavior |
|---|---|
| list_title | “Inbox”; subtitle is the disambiguated account label. Without a selection: “Mailbag”, empty subtitle. |
| list_page title | “Inbox” for the selected account, including narrow navigation. |
| messages | Keep GtkListBox. Bind a GListStore using bind_model and construct message-row.ui rows; no replacement ListView or generic factory framework. |
| sender / subject / time | Received display fields and INTERNALDATE in local presentation. Date does not determine row position. |
| dot | Show for unread; keep its decorative role. Put “Unread”/“Read” in the row's accessible description. |
| preview / thread_count / trash_reveal | Remain hidden. |
| singleton_slot / envelope_slot | Instantiate existing message-content.ui and envelope.ui once; populate locally when a message opens. |
| reader_subject / reader_sender / single_date | Received subject, sender and received date. |
| reader_to | Received To recipients as plain text; hide when absent. |
| reader_body | Inert plain text: at most the first 65,536 UTF-8 bytes, cut at a character boundary, without an explanation. No HTML conversion, markup or external requests. |
| attachment_button / reader_location | Hidden; attachment and folder views are outside scope. |
| star_button / message_menu / demo_button / mail-changing controls | Preserve placement; keep insensitive and without action handlers. |
| reader_stack | Use unselected/message. Do not enter offline/body-loading prototype pages. |

Opening selects the received UID and uses existing mail_split navigation.
Refresh clears the list, selection and reader before loading. Text
selection/copy is local.

Keep GtkLabel. Body, header and ALERT text are cut at the 64 KiB presentation
boundary at a character boundary, without an explanation or marker. This bounds
presentation, not download or MIME-part size; the stored text is not truncated.
Replace embedded NUL before GTK APIs; never interpret remote text as markup.

## Failure wording

| Failed step | Meaning to communicate |
|---|---|
| Settings | Unable to get IMAP settings from Online Accounts. |
| Encryption setting | The account has no encryption configured; choose SSL or STARTTLS for it in Online Accounts. No password was requested and no connection was made. |
| Password | Unable to get the password from Online Accounts; no server sign-in was attempted. |
| Online Accounts timeout | Online Accounts did not respond in time. |
| Connection | Unable to reach the mail server. |
| Secure connection | Unable to establish a verified encrypted connection. |
| Server sign-in | The mail server rejected sign-in; the password can be changed in Online Accounts. Do not claim that rejection proves a wrong password. |
| Inbox / metadata / text | Identify which receiving step failed. |
| Inactivity | State which step stopped responding; no user-facing application size-limit error. |
| Unsupported content | Explain it in the message reader without failing the batch. |
| Unreadable structure | Explain in the reader that this message's content could not be read; its row stays in the list. |

A library response-limit failure is an acquisition failure, never a clipped
message or a successful empty result.

## Acceptance

Use [quickstart.md](../quickstart.md). Preserve F01 accessibility and test keyboard
access to Refresh Inbox and received rows, including the spoken read/unread
state. Check navigation and quitting during a stall. Do not repeat a full
keyboard/pointer/touch/narrow-width matrix as SC-006.

Verify that selection never loads, that refresh clears and then loads, a failed
load and a repeated refresh, switching accounts during a load, F01 page priority,
a row with an unreadable structure, inert ALERT text and display clipping at
64 KiB. The threshold is an acceptance choice to test in Mailbag, not a claim
that GTK guarantees responsiveness at that size.
