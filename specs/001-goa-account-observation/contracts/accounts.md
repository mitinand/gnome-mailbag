# Shared Account Data Contract

`account-model` defines account data and validity checks without GTK/GIO,
transport, commands or recovery. `goa-adapter` translates GOA; Mailbag applies
[the feature requirements](../spec.md#requirements). There is one public account
format and no generic source trait or registry.

## Data

| Type / field | Meaning |
|---|---|
| AccountId | Valid, nonempty opaque source ID; comparable and redacted in Debug |
| AccountProvider | ImapSmtp, Google, Microsoft365 or Other; recognition, not application support |
| AccountDetails.provider | Some(provider) for valid information; None for missing/invalid information |
| mail_enabled | Some(true) enabled, Some(false) explicitly disabled, None unknown |
| mail_service_available | Source reports a Mail service; false does not mean disabled |
| needs_attention | Known attention flag or None; no authentication diagnosis |
| display_name, provider_name, email_address, icon_name | Optional validated display data |
| AccountCheckResult | NotChecked, Complete or Failed(AccountCheckError) |
| AccountUpdate | Account map, last_check: AccountCheckResult, check_pending: bool |
| AccountCheckError | Common cause, operation and optional safe source diagnostic domain/code |

`Complete` confirms membership, including absence, even when an identifiable
account has invalid fields. `Failed` contains its cause; `NotChecked` has no
previous observation. Membership and check errors are derived from this one result.

`check_pending` describes the current request independently of `last_check`.
Starting any check, including a manual retry that joins background work, leaves
that result and the account facts unchanged. First discovery has NotChecked;
retry after failure retains Failed(error). Presentation follows FR-005/012:
pending controls do not make confirmed rows unconfirmed or hide unresolved errors.
A finished check clears check_pending and supplies its result. There is no public
publication counter; the adapter rejects obsolete replies before publishing.

## Validation and diagnostics

`invalid_fields()` derives unknown provider/mail-enabled/attention fields from
AccountDetails. Optional invalid text becomes None, including empty email addresses.
IDs and text reject empty, control-containing or oversized strings. Icons must be
theme names, not file paths or URIs. Adapters validate source types first.
The GOA adapter owns total data limits under [its contract](observation.md#limits).

Error causes are unavailable, access denied, timeout, invalid reply, invalid list,
data limit and stopped source. Account rules use these categories, not GIO codes.
Adapters whitelist diagnostic strings and omit remote error messages. Debug output
omits IDs and personal display values. Expected cancellation is not an error.

## Application representation

`AccountList::apply_update(&AccountUpdate)` applies the latest received snapshot
when updating displayed rows. It borrows source data; the receiver can share an
immutable snapshot without a deep copy. AccountList owns visible rows, selection,
last check result, pending controls and excluded reasons.

AccountRow keeps usable display fields during uncertainty. Confirmed missing
optional data uses fallbacks: Mail account, the supported provider name or Online
Accounts, and mail-unread-symbolic. Supported fallback names are IMAP / SMTP,
Google and Microsoft 365. Equal row names use a stable current-run distinguishing
number without exposing IDs. Selection is by AccountId.

Row retention, eligibility and selection follow FR-003–010; AccountPage represents
the FR-005 status. AccountHiddenNotice returns Single(display label) or Group(count)
for an applied exclusion under FR-017. The caller owns its lifetime; AccountList
keeps no notice history. GTK behavior belongs to [the UI contract](ui.md).
