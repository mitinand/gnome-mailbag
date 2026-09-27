// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    ImapAccount, ImapError, ImapStep, MailboxList, MailboxName, OpenOptions,
    session::{
        self, SOCKET_TIMEOUT_SECONDS, ServerNotices, SignedInSession, StepFailure, command_failure,
    },
};
use async_imap::types::{Name, NameAttribute};
use futures_util::TryStreamExt;

/// Connects securely, signs in and lists every mailbox of the account with
/// `LIST "" "*"`. A refused or cut-short list is a failure, never a shorter
/// list, because the caller replaces what it knew with it.
pub async fn list_mailboxes(
    account: ImapAccount,
    options: OpenOptions,
) -> Result<MailboxList, ImapError> {
    let mut notices = ServerNotices::default();
    let listed = sign_in_and_list(&account, &options, &mut notices).await;
    listed.map_err(|failure| notices.error(&account.login, failure))
}

async fn sign_in_and_list(
    account: &ImapAccount,
    options: &OpenOptions,
    notices: &mut ServerNotices,
) -> Result<MailboxList, StepFailure> {
    let mut signed_in = session::sign_in_session(
        account,
        options,
        SOCKET_TIMEOUT_SECONDS,
        notices,
        ImapStep::ListMailboxes,
    )
    .await?;
    let names = list_names(&mut signed_in).await;
    notices.collect(&account.login);
    let names = names.map_err(|error| command_failure(ImapStep::ListMailboxes, &error))?;
    tracing::info!(mailboxes = names.len(), "mailbox list received");
    tracing::debug!(
        names = names
            .iter()
            .map(|mailbox| mailbox.name.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
        "mailbox names"
    );
    Ok(MailboxList {
        names,
        utf8_names: signed_in.utf8_names,
    })
}

/// Every name of one LIST command. RFC 6154 lets a server leave the
/// special-use attributes out of a plain LIST, so a server that announces
/// them is asked for them.
async fn list_names(
    signed_in: &mut SignedInSession,
) -> Result<Vec<MailboxName>, async_imap::error::Error> {
    // The library sends the pattern as written, so the return option can
    // follow it.
    let pattern = match signed_in.capabilities.has_str("SPECIAL-USE") {
        true => "* RETURN (SPECIAL-USE)",
        false => "*",
    };
    let names = signed_in.session.list(Some(""), Some(pattern)).await?;
    names.map_ok(|name| mailbox_name(&name)).try_collect().await
}

fn mailbox_name(name: &Name) -> MailboxName {
    MailboxName {
        name: name.name().to_owned(),
        attributes: name.attributes().iter().map(attribute_text).collect(),
        delimiter: name.delimiter().map(str::to_owned),
    }
}

/// An attribute as RFC 3501 and RFC 6154 spell it; the library keeps the
/// text of any other.
fn attribute_text(attribute: &NameAttribute<'_>) -> String {
    let text = match attribute {
        NameAttribute::NoInferiors => "\\Noinferiors",
        NameAttribute::NoSelect => "\\Noselect",
        NameAttribute::Marked => "\\Marked",
        NameAttribute::Unmarked => "\\Unmarked",
        NameAttribute::All => "\\All",
        NameAttribute::Archive => "\\Archive",
        NameAttribute::Drafts => "\\Drafts",
        NameAttribute::Flagged => "\\Flagged",
        NameAttribute::Junk => "\\Junk",
        NameAttribute::Sent => "\\Sent",
        NameAttribute::Trash => "\\Trash",
        NameAttribute::Extension(text) => text,
        // The library's list is closed today; a later version may add names.
        _ => return format!("{attribute:?}"),
    };
    text.to_owned()
}
