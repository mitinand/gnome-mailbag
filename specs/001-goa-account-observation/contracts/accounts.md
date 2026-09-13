# Shared Account Data Contract

`account-source` contains account data and correctness checks only. It has no
GTK/GIO dependencies, worker, commands, delivery queue or recovery policy.
`goa-adapter` translates its protocol directly into these types; Mailbag decides
provider support, rows, selection and notices. Only application wiring knows the
concrete adapter. No second public GOA account format is retained.

| Data | Meaning |
|---|---|
| AccountId | Valid, nonempty source ID; opaque, comparable and redacted in Debug |
| AccountProvider | ImapSmtp, Google, Microsoft365 or Other; recognition, not application support |
| AccountDetails.provider | Some(provider) for valid information; None for missing/invalid information |
| mail_enabled | Some(true) enabled, Some(false) explicitly disabled, None unknown |
| mail_service_available | Whether the source reports a mail service; false never implies disabled |
| needs_attention | Some(true) needs attention, Some(false) does not, None unknown; no authentication diagnosis |
| display_name, provider_name, email_address, icon_name | Optional validated display data, never identifiers |
| AccountUpdate | Current account map, completeness, check status, safe error and update number |
| AccountCheckError | Common cause plus operation and optional safe source diagnostic domain/code |

An AccountUpdate can confirm membership despite errors in individual accounts.
An incomplete update cannot prove removal. Explicit mail_enabled=false remains
usable independently of unrelated unknown fields. A successful check does not
clear needs_attention=true.

`invalid_fields()` derives unknown required fields from the current values; there
is no duplicate stored list. Shared validation rejects empty/control-containing
or oversized identifiers and provides text/icon checks and string-size accounting.
Adapters validate source field types before using these checks. Optional invalid
text uses None; empty email addresses are allowed as absent display information.
The initial update is checking with no confirmed accounts.

Common error categories distinguish unavailable, access denied, timeout, invalid
reply, invalid account list, data limit and stopped source. Preserve safe diagnostic
operation/domain/code; application rules branch on categories, never GIO codes.
The adapter must whitelist diagnostic strings and omit arbitrary remote error text.
Debug output omits account IDs and all personal display values.

Data values are ordinary Rust types. This contract adds no source trait, registry,
account creation API or persistence. Existing transport size limits and scheduling
remain responsibilities of the adapter under its own contract.
