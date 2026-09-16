# Account State in Memory

All state belongs to the current run. F01 has no database, credentials or mail cache.

| Data | Owner | Definition |
|---|---|---|
| AccountId, AccountDetails, AccountCheckResult, AccountUpdate, AccountCheckError | goa-adapter::account_model | [Account contract](contracts/accounts.md) |
| Last accepted full list and read result | GOA observer | [Parsing and acceptance](contracts/observation.md#parsing-and-acceptance) |
| Active operation, refetch_needed, retry progress and subscriptions | GOA observer | [Read scheduling](contracts/observation.md#events-and-request-ordering) |
| Visible rows, selected ID, page state and excluded reasons | Mailbag AccountList | [Application representation](contracts/accounts.md#application-representation) |
| AccountHiddenNotice | Caller presenting the notice | [FR-017](spec.md#requirements) and [UI contract](contracts/ui.md#notices) |
| Row widgets, focus and explanations | GTK account UI | [UI contract](contracts/ui.md) |
| Settings launch_pending flag and error callback | Settings launcher | [Settings protocol](contracts/ui.md#settings-launch-protocol) |
| Displayed/queued account and Settings toasts | Standard AdwToastOverlay | [Notices](contracts/ui.md#notices) |

The source list describes the last successful read; AccountList describes the rows
actually displayed. Failed reads preserve both while changing the visible error.
A successful full list replaces source data, then AccountList applies eligibility
and generates notices only for actual displayed-to-hidden transitions.

Missing Mail and explicit disablement are separate typed facts. Required fields
have no unknown variants. There is no path/property cache, cross-thread snapshot
exchange, notification history or retained Settings error model.
