# Proposed goa-adapter IMAP Access Contract

**Approval required before portion 2:** This extends the shared
[F01 contract](../../001-goa-account-observation/contracts/accounts.md).
It is a proposal, not authorization to implement a shared interface change.
Existing observation and account exclusion remain unchanged.

## Public operation

`GoaAdapter::request_imap_access(account_id, on_step, on_complete)` returns an
opaque `ImapAccessRequest` with `cancel()`. Call it on the adapter's existing
GLib main context. Step notifications and completion run on that context.
While the context is running, complete exactly once, including cancellation,
so the caller knows its load has ended. Dropping the request cancels work.

| Public role | Proposed contents |
|---|---|
| ImapAccess | AccountId, original GOA host string, login, ImapEncryption and password. Owned data consumed by the caller; no sensitive Debug/Display or automatic Clone. |
| ImapEncryption | ImplicitTls or StartTls; no plaintext variant. |
| ImapAccessStep | Settings or Password; moving to Password confirms settings retrieval completed. |
| ImapAccessError | Step and a safe cause: unavailable, denied, timeout, invalid settings, no encryption configured, unsupported provider or cancelled. Reuse existing safe causes where applicable. |
| ImapAccessRequest | Main-context cancellation handle; retains no credentials after completion. |

Only owned Rust data crosses to the mail worker. Proxies and GOA object paths
stay on the adapter's context. The adapter uses asynchronous D-Bus calls with
GIO's finite call timeout, without another timer or retry loop. Mailbag cancels
the request on confirmed exclusion of the account or exit. `stop()` also cancels outstanding access
requests; late callbacks cannot restart stopped work.

## Preparation algorithm

1. Read GetManagedObjects through the normal GOA service and existing connection.
   A previous observation failure or AttentionNeeded does not block an attempt.
2. Find the exact opaque AccountId and use its returned object path; never
   construct a path from an account ID.
3. Read the current Generic IMAP provider, Mail availability, ImapHost,
   ImapUserName and encryption flags with their declared types. Missing required
   settings or an account that is no longer available returns a Settings failure.
4. Choose implicit TLS when ImapUseSsl is true; otherwise choose STARTTLS when
   ImapUseTls is true. When both are false, return a Settings failure with the
   no-encryption cause before notifying Password (spec US3-6). Never substitute
   implicit TLS for false/false. Ignore ImapAcceptSslErrors.
5. Notify Password and call
   `org.gnome.OnlineAccounts.PasswordBased.GetPassword("imap-password")` on that
   object's path. Return the owned access data or a Password failure.

Preserve the host syntax, including an explicit port, for GIO's address parser.
Do not fabricate missing required settings or silently change the login.
False/false never reaches the password request or a socket.

## Observation owns exclusion

This fresh read prepares access; it does not update the account list or repeat
F01's exclusion policy in the mail controller. If the request discovers an absent
or disabled account, return a failed Settings step. Do not clear received mail,
ask the observer to refresh, or emit a substitute account update from this path.

The existing observer independently reports confirmed exclusion. AccountList
then discards the account's mail and cancels its load. A failed or incomplete GOA
reply is not confirmation of removal. Until the observer confirms exclusion,
a failed access attempt is an ordinary load failure.

## Privacy and lifecycle

- Request access only for the selected account. No startup password scan.
- No EnsureCredentials, SMTP check, direct secret-store API or Mailbag password
  prompt. Password changes belong in Online Accounts.
- Move access data to the mail worker. Retain credentials only while the attempt
  may need them, including structure isolation, then release them on completion
  or cancellation. No credential cache between loads or secret-wiping framework.
- No passwords in AccountDetails, UI models, files, command arguments, diagnostics
  or formatted library errors. F01 observation and Retry Check remain independent.
- Cancellation is silent. Use weak window ownership for callbacks; do not retain
  the UI until an external service replies.

## Contract verification

Extend the existing private D-Bus fixture for the selected object/path and the
`imap-password` key; SSL, STARTTLS and false/false refused before Password;
AttentionNeeded; access after an observation failure; Settings versus Password
failure; cancellation/stop; and absent account versus missing Mail interface.

Assert that preparation failure never starts IMAP work or invokes an observer
refresh. A separate observer update must still exclude the account, and a late
result must not restore its mail. Use synthetic credentials
only, without raw reply logging.

References: [Mail properties](https://gnome.pages.gitlab.gnome.org/gnome-online-accounts/dbus-org.gnome.OnlineAccounts.Mail.html),
[PasswordBased](https://gnome.pages.gitlab.gnome.org/gnome-online-accounts/dbus-org.gnome.OnlineAccounts.PasswordBased.html),
[finite D-Bus calls](https://docs.gtk.org/gio/method.DBusConnection.call.html).
The false/false rule and its GOA source evidence are recorded in
[research](../research.md#10-goa-encryption-flags).
