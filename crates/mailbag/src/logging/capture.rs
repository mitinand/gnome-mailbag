// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A record kept in memory, for tests that read what Mailbag logged.

use super::{LogLevel, record_subscriber};
use crate::test_record::CapturedRecord;

/// Records the current thread's events at one level, laid out as the
/// application writes them.
pub fn start_record(level: LogLevel) -> CapturedRecord {
    CapturedRecord::start_with(|output| record_subscriber(level, output))
}
