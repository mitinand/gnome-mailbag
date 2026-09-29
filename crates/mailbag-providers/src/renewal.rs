// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! Asking Online Accounts for an account's access again while its cycle runs
//! on the mail worker, whose thread cannot call the adapter: the request goes
//! to GTK's context and the answer comes back
//! (specs/009-synchronization/research.md §13).

use goa_adapter::{GoaAdapter, ImapAccess};
use mailbag_domain::AccountId;

/// Where the answer to one renewal request goes.
type RenewalReply = async_channel::Sender<Option<ImapAccess>>;

/// A running load's way to ask for its account's access again.
pub(crate) struct AccessRenewal {
    requests: async_channel::Sender<RenewalReply>,
}

impl AccessRenewal {
    /// The access Online Accounts gives now; `None` when it gave none or
    /// the load's requests are no longer answered.
    pub(crate) async fn renew(&self) -> Option<ImapAccess> {
        let (reply, answer) = async_channel::bounded(1);
        self.requests.send(reply).await.ok()?;
        answer.recv().await.ok().flatten()
    }
}

/// A renewal for a load of `account_id`, answered on the calling context,
/// GTK's, with the request the load started with. It answers until the load
/// drops its renewal.
pub(crate) fn answer_renewals(accounts: GoaAdapter, account_id: AccountId) -> AccessRenewal {
    let (requests, received) = async_channel::unbounded::<RenewalReply>();
    glib::spawn_future_local(async move {
        while let Ok(reply) = received.recv().await {
            let (sender, answer) = async_channel::bounded(1);
            // Kept until Online Accounts answers; dropping it would cancel it.
            let _request = accounts.request_imap_access(&account_id, move |access| {
                sender.try_send(access.ok()).ok();
            });
            let access = answer.recv().await.ok().flatten();
            tracing::info!(
                account = account_id.as_str(),
                given = access.is_some(),
                "Online Accounts was asked for the access again"
            );
            reply.try_send(access).ok();
        }
    });
    AccessRenewal { requests }
}

#[cfg(test)]
impl AccessRenewal {
    /// A renewal the test answers through the returned receiver.
    pub(crate) fn answered_by_test() -> (Self, async_channel::Receiver<RenewalReply>) {
        let (requests, received) = async_channel::unbounded();
        (Self { requests }, received)
    }
}
