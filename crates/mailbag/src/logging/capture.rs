// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A record kept in memory, for tests that read what Mailbag logged.

use super::{LogLevel, record_subscriber};
use std::{
    io,
    sync::{Arc, Mutex},
};
use tracing::subscriber::DefaultGuard;

/// The current thread's events at one level, laid out as the application
/// writes them. Recording stops when dropped.
pub struct CapturedRecord {
    written: RecordBuffer,
    _recording: DefaultGuard,
}

impl CapturedRecord {
    pub fn start(level: LogLevel) -> Self {
        let written = RecordBuffer::default();
        let output = written.clone();
        let recording =
            tracing::subscriber::set_default(record_subscriber(level, move || output.clone()));
        Self {
            written,
            _recording: recording,
        }
    }

    pub fn text(&self) -> String {
        let written = self.written.0.lock().expect("record buffer").clone();
        String::from_utf8(written).expect("the record is UTF-8")
    }
}

#[derive(Clone, Default)]
struct RecordBuffer(Arc<Mutex<Vec<u8>>>);

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
