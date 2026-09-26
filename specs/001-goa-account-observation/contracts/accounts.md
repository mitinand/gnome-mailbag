# Shared Account Data Contract

`goa-adapter` exports account types from its private `account_model` module.
The module defines ordinary Rust data. The adapter decodes GOA; Mailbag owns
[eligibility and presentation](../spec.md#requirements).

## Data

| Type / field | Meaning |
|---|---|
| AccountId | Nonempty opaque GOA ID; stable across renames, comparable, with ordinary derived Debug; `as_str` gives its text for the record of [003](../../003-logging/spec.md). Defined in `mailbag-domain`, whose `TryFrom<&str>` refuses an empty identifier; `goa-adapter` turns that refusal into its invalid reply (amended by [007](../../007-mail-storage/research.md#8-types-and-crates) on 2026-09-26) |
| AccountProvider | ImapSmtp, Google, Microsoft365 or Other; recognition, not application support |
| AccountDetails.provider | Required AccountProvider |
| mail_enabled | Required bool, the inverse of GOA MailDisabled |
| mail_service_available | Whether the full reply contains the Mail interface |
| needs_attention | Required bool; an attention request does not diagnose authentication |
| display_name, email_address | Optional nonempty display strings; empty or unavailable text permits the label fallback |
| AccountCheckResult | NotChecked, Complete or Failed(AccountCheckError) |
| AccountUpdate | Accepted account map, last_check and retry_pending |
| AccountCheckError | Safe operation and cause: unavailable, access denied, timeout or invalid reply |

`Complete` means one entire response was accepted. It confirms which accounts
exist, including an empty list. Unsupported providers and a missing Mail interface
remain valid records. `Failed` describes a rejected read and retains the previous
account map. `NotChecked` has no accepted list yet.

`retry_pending` describes an explicit user retry, independently of `last_check`.
Automatic reads do not set it. Starting a retry leaves the previous list and
error unchanged; the flag stays set through a requested follow-up read and clears
when the sequence finishes. Initial loading follows NotChecked, not this flag.
The transport's active-read flag is private.

ProviderName, ProviderIcon, diagnostic domain/code and per-field unknown states
are not part of this contract. There is no source-stopped result or publication
counter.

## Validation and diagnostics

Decode each account's required ID, provider and booleans with their declared GOA
types. A missing or mistyped required field rejects the whole reply as one read
error. An optional display field with the wrong type is also a decoding error;
an absent/empty display string uses the normal fallback. Do not construct partial
records or derive account identity from old paths.

GOA and GIO already construct unique account objects and typed properties; do not
add separate duplicate/identity-repair protocols. The [GOA contract](observation.md#limits)
defines the retained bounds. No trimming or truncation may turn rejected data into
a successful partial account list.

Render source strings as plain text. AccountId's derived Debug includes its value;
AccountDetails keeps its display fields hidden. Debug/log output excludes
addresses, labels, credentials and remote error messages. An account ID reaches
the application's record only as the `account` field that
[003](../../003-logging/spec.md) defines. Map transport errors to the safe causes above; account rules
do not interpret GIO codes. Expected cancellation is silent.

## Application representation

`AccountList::apply_update(&AccountUpdate)` applies an accepted list and its read
status to visible rows. AccountList owns selection, account problems, page state,
retry presentation and exclusions. Widgets consume these decisions.
AccountPage includes the read-error cause and gives that error priority over
selection and empty-list explanations. Row problems describe account availability
directly; no separate availability classification is needed by the UI.

Rows are keyed by AccountId. Labels use display_name, then email_address, then
“Mail account”. Duplicate labels use a stable current-run distinguishing number
without exposing IDs. Provider icons follow [the UI contract](ui.md).

A failed read preserves rows, labels and selection while marking observation
unconfirmed. A complete list allows confirmed removal or explicit disablement to
hide rows. A supported enabled account without Mail is unavailable: retain a known
row; at cold start show the relevant explanation and Retry instead of claiming
there are no mail accounts. Attention remains distinct from both states.
Unsupported providers use the general account-setup guidance even when Mail is
disabled: enabling Mail alone cannot make those accounts eligible.

`apply_update` returns one AccountHiddenNotice, containing the displayed label,
for each row actually hidden under FR-017. The caller presents it immediately
through the standard toast queue. No notice history or separate delivery protocol
is needed.
