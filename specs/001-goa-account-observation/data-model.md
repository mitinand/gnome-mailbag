# Account State in Memory

All state belongs to the current run; F01 has no database, credentials or mail cache.

| Data | Owner | Definition |
|---|---|---|
| AccountId, AccountDetails, AccountCheckResult, AccountUpdate, AccountCheckError | goa-adapter::account_model | [Shared account contract](contracts/accounts.md) |
| Parsed account records and D-Bus paths | GOA decoder | [Parsing and acceptance](contracts/observation.md#parsing-and-acceptance) |
| Current account facts, valid path mappings and active request | GOA worker | [GOA client contract](contracts/observation.md) |
| Latest shared snapshot and waiting task | GOA exchange | [Delivery and shutdown](contracts/observation.md#delivery-and-shutdown) |
| Visible AccountRow values, selected ID, AccountPage and excluded reasons | Mailbag AccountList | [Application representation](contracts/accounts.md#application-representation) |
| AccountHiddenNotice | Caller presenting the notice | [FR-017](spec.md#requirements) and [UI contract](contracts/ui.md#notices) |
| Row objects, focus, popovers and toast presentation | GTK account UI | [UI contract](contracts/ui.md) |
| Pending launch and last launch error | Settings launcher | [Settings protocol](contracts/ui.md#settings-launch-protocol) |

The adapter retains source facts; AccountList retains the rows actually applied by
the UI. These serve different purposes: replacing a pending source snapshot does
not itself hide a displayed row or produce a notice. No intermediate-change
history or acknowledgment protocol connects them.
