# goa-adapter IMAP Access Contract

**Approved by the maintainer 2026-09-19. Revised 2026-09-19 after the
maintainer's review of portion 2:** one flat error, no step callback, no
fallback bus connection and no request tracking in `stop()`. This extends the
shared [F01 contract](../../001-goa-account-observation/contracts/accounts.md).
Existing observation and account exclusion remain unchanged.

## Public operation

`GoaAdapter::request_imap_access(account_id, on_complete)` returns an opaque
`ImapAccessRequest` with `cancel()`. Call it on the adapter's GLib main context;
completion runs on that context exactly once, including cancellation, so the
caller knows its load has ended. Dropping the request cancels it.

| Public role | Contents |
|---|---|
| ImapAccess | AccountId, original GOA host string, login, ImapEncryption and password. Owned data consumed by the caller; no sensitive Debug/Display or automatic Clone. |
| ImapEncryption | ImplicitTls or StartTls; no plaintext variant. |
| ImapAccessError | Settings: Online Accounts did not return the settings. NoEncryption: neither SSL nor STARTTLS is set (US3-6). Password: Online Accounts did not return the password. Timeout: Online Accounts did not answer in time at either step. Cancelled. |
| ImapAccessRequest | Cancellation handle owned by the load; retains no credentials. |

Only owned Rust data crosses to the mail worker. Proxies and GOA object paths
stay on the adapter's context. The request uses the observer's existing bus
connection; without one there is no account list to select from, and the
request fails as Settings, reported on the context's next turn like every
other answer (never inside the call). D-Bus calls are asynchronous with GIO's finite call
timeout, without another timer or retry loop. A D-Bus timeout at either step is
Timeout: for example, GetPassword waits while the keyring asks to be unlocked.
Any other D-Bus error is the failure of its step. The load owns the request;
Mailbag cancels the load on confirmed exclusion of the account or exit, which
drops the request.

## Preparation algorithm

1. Read GetManagedObjects over the observer's connection. A failed previous
   observation read or AttentionNeeded does not block an attempt.
2. Find the exact opaque AccountId and use its returned object path; never
   construct a path from an account ID.
3. Read the Mail interface's ImapHost, ImapUserName and encryption flags. An
   account that is no longer listed, a missing Mail interface or a missing
   setting is Settings. GIO checks the reply signature against the type passed
   to the call; empty values from edited configuration fail later at address
   parsing or sign-in.
4. Choose implicit TLS when ImapUseSsl is true; otherwise choose STARTTLS when
   ImapUseTls is true. When both are false, return NoEncryption before the
   password is requested (spec US3-6). Never substitute implicit TLS for
   false/false. Ignore ImapAcceptSslErrors.
5. Call `org.gnome.OnlineAccounts.PasswordBased.GetPassword("imap-password")`
   on that object's path. Return the owned access data or Password.

Preserve the host syntax, including an explicit port, for GIO's address parser.
Do not fabricate missing required settings or silently change the login.
False/false never reaches the password request or a socket.

## Observation owns exclusion

This fresh read prepares access; it does not update the account list or repeat
F01's exclusion policy in the mail controller. If the request discovers an absent
or disabled account, return Settings. Do not clear received mail, ask the
observer to refresh, or emit a substitute account update from this path.

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
`imap-password` key. Cover success with the returned object path and a host
with an explicit port; SSL chosen over STARTTLS; false/false refused before the
password with ImapAcceptSslErrors changing nothing; an absent account and a
missing Mail interface as Settings; a service error at each step as Settings or
Password; a hang at each step as Timeout; cancelling and dropping the request as
one Cancelled; AttentionNeeded and a failed observation read over the existing
connection.

Assert that preparation failure never starts IMAP work or invokes an observer
refresh. A separate observer update must still exclude the account, and a late
result must not restore its mail. Use synthetic credentials
only, without raw reply logging.

References: [Mail properties](https://gnome.pages.gitlab.gnome.org/gnome-online-accounts/dbus-org.gnome.OnlineAccounts.Mail.html),
[PasswordBased](https://gnome.pages.gitlab.gnome.org/gnome-online-accounts/dbus-org.gnome.OnlineAccounts.PasswordBased.html),
[finite D-Bus calls](https://docs.gtk.org/gio/method.DBusConnection.call.html).
The false/false rule and its GOA source evidence are recorded in
[research](../research.md#10-goa-encryption-flags).

## Amendment by 004 (proposed and approved by the maintainer 2026-09-23)

Google accounts hold no password: their object exports
`org.gnome.OnlineAccounts.OAuth2Based` instead of `PasswordBased`
([004 research §1](../../004-gmail-integration/research.md)). The contract
changes in the credential step only.

| Public role | Change |
|---|---|
| ImapAccess | `password: String` becomes `credential: ImapCredential`. Same ownership rules; no sensitive Debug/Display, no Clone. |
| ImapCredential | `Password(String)` or `AccessToken(String)`. |
| ImapAccessError | New variant `AccessToken`: Online Accounts did not return the access token. `Password` keeps its meaning for password accounts. |

Preparation algorithm, step 5 becomes:

5. Choose the credential by the interface the account's object exports:
   `OAuth2Based` → call `GetAccessToken()` and keep the token; the returned
   `expires_in` is read for the signature and discarded, because GOA renews a
   token that is close to expiry before returning it. Otherwise
   `PasswordBased` → `GetPassword("imap-password")` as today. An object with
   neither interface fails as Settings. The kind is never chosen from the
   provider type.

Everything else stays: one request, one completion, the observer's connection,
GIO's call timeout, no EnsureCredentials, no credential cache, and the same
privacy rules with the token treated exactly as a password. The window's
wording for the new error is "Unable to get this account's authorization from
Online Accounts. No server sign-in was attempted."

## Amendment by 005 (proposed and approved by the maintainer 2026-09-23)

A Microsoft 365 account holds no server settings: its object exports
`org.gnome.OnlineAccounts.OAuth2Based` and a Mail interface that carries the
address and nothing else ([005 research §1](../../005-microsoft-graph-integration/research.md)).
The contract gains a second operation and provider-neutral names for the
types both operations share.

| Public role | Change |
|---|---|
| `request_graph_access(account_id, on_complete)` | New operation, same calling rules as `request_imap_access`: on the adapter's GLib context, one completion, cancellation by `cancel()` or drop. |
| GraphAccess | AccountId and the access token. Owned data consumed by the caller; no sensitive Debug/Display, no Clone. |
| AccessError | `ImapAccessError` renamed; the variants and their meanings stay. For the Graph operation only Settings, AccessToken, Timeout and Cancelled can occur. |
| AccessRequest | `ImapAccessRequest` renamed; one cancellation handle type for both operations. |

Preparation algorithm of the Graph operation:

1. Read GetManagedObjects over the observer's connection, as step 1 of the
   IMAP operation.
2. Find the exact opaque AccountId and use its returned object path; never
   construct a path from an account ID. An account that is no longer listed
   is Settings.
3. Require `OAuth2Based` on that object; an object without it is Settings. No
   Mail setting is read and no encryption flag is examined.
4. Call `GetAccessToken()`; keep the token, discard `expires_in`. A D-Bus
   error is AccessToken; a timeout is Timeout; cancellation is Cancelled.

Everything else stays: one request, one completion, the observer's
connection, GIO's call timeout, no EnsureCredentials, no credential cache,
and the same privacy rules with the token treated exactly as a password.
The window's wording for Settings becomes provider-neutral: "Unable to get
this account's settings from Online Accounts."

Contract verification: extend the private D-Bus fixture with a Microsoft 365
object (`ms_graph`, Mail with the address only, `OAuth2Based`). Cover the
token returned; the account absent; an object without `OAuth2Based`;
`GetAccessToken` answered with an error; a held reply as Timeout; cancelling
and dropping the request as one Cancelled. Assert that no Mail setting is
required and that the IMAP operation's tests are unchanged by the renames.
