// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! One cycle of a folder (specs/009-synchronization FR-001): the provider's
//! way of learning the server's changes, stored as whole batches by one
//! store operation that does not know which way produced them (research §1).

mod graph;
mod imap;

use crate::{LoadFailure, LoadResult, store_load::BatchWriter, worker::LoadKind};
use graph::synchronize_graph_folder;
use imap::{IdentityRule, synchronize_imap_folder};
use mailbag_domain::FolderState;
use mailbag_graph::GraphError;
use mailbag_imap::ImapError;
use std::time::{SystemTime, UNIX_EPOCH};

/// How far back from a cycle's start texts are downloaded (spec FR-009).
const RECENT_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Runs one cycle of the folder `batches` writes into, choosing the
/// provider once (specs/004-gmail-integration plan, decision D1).
pub(crate) async fn synchronize_folder(
    kind: LoadKind,
    mut batches: BatchWriter<'_>,
) -> Result<LoadResult, LoadFailure> {
    let cycle = match kind {
        LoadKind::GenericImap(access) => {
            synchronize_imap_folder(access, IdentityRule::Generic, &mut batches).await
        }
        LoadKind::Gmail(access) => {
            synchronize_imap_folder(access, IdentityRule::Gmail, &mut batches).await
        }
        LoadKind::Microsoft365 {
            access,
            service_url,
            renewal,
        } => synchronize_graph_folder(access, service_url, renewal, &mut batches).await,
        #[cfg(test)]
        LoadKind::PanicsForTest(_) => panic!("a load panicked on purpose"),
    };
    match cycle {
        Ok(result) | Err(CycleEnd::Stopped(result)) => Ok(result),
        Err(CycleEnd::Failed(failure)) => Err(failure),
    }
}

/// Why a cycle stopped early: the server failed it, or the store failed or
/// found the load cancelled, which already is the load's result.
enum CycleEnd {
    Failed(LoadFailure),
    Stopped(LoadResult),
}

impl From<ImapError> for CycleEnd {
    fn from(error: ImapError) -> Self {
        Self::Failed(LoadFailure::Imap(error))
    }
}

impl From<GraphError> for CycleEnd {
    fn from(error: GraphError) -> Self {
        Self::Failed(LoadFailure::MicrosoftGraph(error))
    }
}

impl From<LoadResult> for CycleEnd {
    fn from(result: LoadResult) -> Self {
        Self::Stopped(result)
    }
}

/// The state a completed cycle leaves, with Microsoft 365's link for the
/// next round.
fn completed(server_position: Option<String>) -> FolderState {
    FolderState {
        server_position,
        fill_place: None,
        synchronized: true,
    }
}

/// The oldest received date whose text a cycle started at `cycle_start`
/// downloads, as seconds since the Unix epoch (spec FR-009).
fn recent_limit(cycle_start: SystemTime) -> i64 {
    let now = cycle_start
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    now - RECENT_SECONDS
}
