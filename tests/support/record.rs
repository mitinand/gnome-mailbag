// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The current thread's log events written into memory, one line each, for
//! tests of what a crate logs (specs/003-logging). Tests assert on fields and
//! markers, not on the layout.

use std::{
    io,
    sync::{Arc, Mutex, OnceLock},
};
use tracing::{Dispatch, Subscriber, subscriber::DefaultGuard, subscriber::NoSubscriber};
use tracing_subscriber::fmt::MakeWriter;

/// Recording stops when dropped.
pub struct CapturedRecord {
    written: RecordBuffer,
    _recording: DefaultGuard,
}

impl CapturedRecord {
    /// Records with the library's one-line formatter.
    pub fn start(most_detailed_event: tracing::Level) -> Self {
        Self::start_with(|output| {
            tracing_subscriber::fmt()
                .with_max_level(most_detailed_event)
                .with_ansi(false)
                .without_time()
                .with_writer(output)
                .finish()
        })
    }

    /// Records with the subscriber built over the record's buffer.
    pub fn start_with<S>(subscriber_for: impl FnOnce(RecordBuffer) -> S) -> Self
    where
        S: Subscriber + Send + Sync + 'static,
    {
        keep_every_record_asked();
        let written = RecordBuffer::default();
        let recording = tracing::subscriber::set_default(subscriber_for(written.clone()));
        Self {
            written,
            _recording: recording,
        }
    }

    pub fn text(&self) -> String {
        let written = self.written.0.lock().expect("record buffer").clone();
        String::from_utf8(written).expect("the record is UTF-8")
    }

    /// The lines of one level, such as `"ERROR"`: the level is the first word,
    /// or the second after the time.
    pub fn lines_at(&self, level: &str) -> Vec<String> {
        self.text()
            .lines()
            .filter(|line| line.split_whitespace().take(2).any(|word| word == level))
            .map(str::to_owned)
            .collect()
    }
}

/// While only one dispatcher exists, tracing-core decides whether a line can
/// ever be written from the dispatcher of the thread that reaches it first. A
/// parallel test thread without a record would then switch the line off for
/// the test that records it. With a second dispatcher that is never dropped,
/// it asks every dispatcher instead.
fn keep_every_record_asked() {
    static SECOND_DISPATCHER: OnceLock<Dispatch> = OnceLock::new();
    SECOND_DISPATCHER.get_or_init(|| Dispatch::new(NoSubscriber::default()));
}

#[derive(Clone, Default)]
pub struct RecordBuffer(Arc<Mutex<Vec<u8>>>);

impl io::Write for RecordBuffer {
    fn write(&mut self, line: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("record buffer")
            .extend_from_slice(line);
        Ok(line.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for RecordBuffer {
    type Writer = Self;

    fn make_writer(&'a self) -> Self {
        self.clone()
    }
}
