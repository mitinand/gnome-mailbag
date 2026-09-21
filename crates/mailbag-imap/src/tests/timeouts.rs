// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{expect_success, run, wait_until};
use crate::{
    ImapFailure, ImapStep, InboxReader, TextParts, TextRequest,
    test_server::{FaultKind, FaultyCommand, FixtureMessage, FixtureSetup, ImapFixture},
};
use futures_util::future::{self, Either};
use std::{pin::pin, time::Duration};

/// Seconds without progress that fail a read in these tests.
const SHORT_SOCKET_TIMEOUT: u32 = 2;

fn text_fixture(fault: FaultKind) -> ImapFixture {
    ImapFixture::start(FixtureSetup {
        messages: vec![FixtureMessage::plain_text(10, "a text that arrives slowly")],
        fault: Some((FaultyCommand::Text, fault)),
        ..FixtureSetup::default()
    })
}

/// Opens the Inbox and reads the text of message 10.
async fn read_text(fixture: &ImapFixture) -> Result<(), crate::ImapError> {
    let mut reader =
        InboxReader::open_with_short_socket_timeout(fixture.account(), SHORT_SOCKET_TIMEOUT)
            .await?;
    let requests = vec![TextRequest {
        uid: 10,
        parts: TextParts::SinglePartBody,
    }];
    reader.fetch_text(requests, |_, _| {}).await
}

#[test]
fn a_stalled_reply_times_out_but_a_slowly_arriving_one_does_not() {
    let stalled = text_fixture(FaultKind::Stall);
    let error = run(read_text(&stalled)).expect_err("a stalled server must time out");
    assert_eq!(error.failure, ImapFailure::TimedOut(ImapStep::FetchText));

    // Eight pieces 600 ms apart: longer than the timeout in total, never idle that long.
    let slow = text_fixture(FaultKind::Trickle(Duration::from_millis(600)));
    expect_success(run(read_text(&slow)));
}

#[test]
fn cancelling_during_a_pending_read_closes_the_connection() {
    let fixture = text_fixture(FaultKind::Stall);
    run(async {
        let load = pin!(read_text(&fixture));
        let cancel = pin!(glib::timeout_future(Duration::from_millis(300)));
        // Dropping the load, as quitting does, is the cancellation.
        assert!(matches!(
            future::select(load, cancel).await,
            Either::Right(_)
        ));
    });
    wait_until(|| fixture.log().closed_connections == 1);

    // A later load never reuses the interrupted session.
    let _ = run(read_text(&fixture));
    assert_eq!(fixture.log().connections, 2);
}
