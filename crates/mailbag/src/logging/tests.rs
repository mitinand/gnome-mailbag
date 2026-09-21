// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use super::capture::start_record;
use super::*;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Whether a line begins with local time with milliseconds and the UTC offset,
/// such as `2026-09-21T14:03:15.102+03:00`.
fn starts_with_local_time(line: &str) -> bool {
    let Some((time, _)) = line.split_once(' ') else {
        return false;
    };
    let Some((_, milliseconds_and_offset)) = time.split_once('.') else {
        return false;
    };
    glib::DateTime::from_iso8601(time, None).is_ok()
        && milliseconds_and_offset.len() == "102+03:00".len()
        && milliseconds_and_offset[..3]
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        && matches!(milliseconds_and_offset.as_bytes()[3], b'+' | b'-')
}

/// An output whose every write fails, as on a full disk.
struct UnwritableStream(Arc<AtomicUsize>);

impl io::Write for UnwritableStream {
    fn write(&mut self, _line: &[u8]) -> io::Result<usize> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(io::ErrorKind::StorageFull.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_four_levels_are_accepted_by_name() {
    assert_eq!(parse_log_level("error"), Ok(LogLevel::Error));
    assert_eq!(parse_log_level("warning"), Ok(LogLevel::Warning));
    assert_eq!(parse_log_level("info"), Ok(LogLevel::Info));
    assert_eq!(parse_log_level("debug"), Ok(LogLevel::Debug));
}

#[test]
fn an_unknown_level_is_refused_with_the_accepted_ones() {
    assert_eq!(
        parse_log_level("debg"),
        Err("Unknown log level \"debg\". Use error, warning, info or debug.".to_owned())
    );
    for unknown_level in ["", "trace", "Debug", "warn"] {
        assert!(parse_log_level(unknown_level).is_err(), "{unknown_level:?}");
    }
}

#[test]
fn a_level_includes_the_levels_before_it() {
    let record = start_record(LogLevel::Warning);
    tracing::error!("load failed");
    tracing::warn!("list incomplete");
    tracing::info!("signed in");
    tracing::debug!("part decoded");
    let text = record.text();
    let events: Vec<&str> = text.lines().collect();
    assert_eq!(events.len(), 2, "{text}");
    assert!(events[0].contains(" ERROR ") && events[0].contains("load failed"));
    assert!(events[1].contains(" WARN ") && events[1].contains("list incomplete"));
}

#[test]
fn values_from_mail_and_servers_stay_on_one_line() {
    let record = start_record(LogLevel::Debug);
    // Online Accounts accepts identifiers it did not generate, holding any text.
    let account = "corporate\n2026-09-21T14:03:15.102+03:00 ERROR forged \"line\" \0end";
    let folder = "INBOX\n2026-09-21T14:03:15.102+03:00 ERROR forged \"line\" \0end";
    let server_text = "NO [ALERT] first\r\nsecond \"quoted\" \0";
    tracing::error_span!("load", account).in_scope(|| {
        tracing::debug!(folder, server_text, "server replied");
    });
    let text = record.text();
    assert_eq!(text.lines().count(), 1, "one line per event: {text}");
    assert!(!text.contains(['\0', '\r']), "{text:?}");
    let line = text.lines().next().expect("the event's line");
    for escaped in [
        r#"account="corporate\n2026-09-21T14:03:15.102+03:00 ERROR forged \"line\" \0end""#,
        r#"folder="INBOX\n2026-09-21T14:03:15.102+03:00 ERROR forged \"line\" \0end""#,
        r#"server_text="NO [ALERT] first\r\nsecond \"quoted\" \0""#,
    ] {
        assert!(line.contains(escaped), "{escaped} is missing from {line}");
    }
}

#[test]
fn a_line_names_its_time_load_message_and_place() {
    let record = start_record(LogLevel::Debug);
    tracing::error_span!("load", account = "account_1726920000_0").in_scope(|| {
        tracing::debug_span!("message", uid = 4711).in_scope(|| {
            tracing::debug!(section = "1", "part decoded");
        });
    });
    let text = record.text();
    assert!(!text.contains('\x1b'), "colors are off: {text:?}");
    let line = text.lines().next().expect("the event's line");
    assert!(starts_with_local_time(line), "{line}");
    for expected in [
        " DEBUG ",
        r#"account="account_1726920000_0""#,
        "uid=4711",
        "mailbag::logging::tests:",
        "part decoded",
        r#"section="1""#,
    ] {
        assert!(line.contains(expected), "{expected} is missing from {line}");
    }
}

#[test]
fn lines_that_cannot_be_written_are_dropped_and_work_continues() {
    let write_attempts = Arc::new(AtomicUsize::new(0));
    let mut first_line_output = UnwritableStream(write_attempts.clone());
    write_first_line(
        LogLevel::Debug,
        "GTK 4.20.1, libadwaita 1.8.0",
        &mut first_line_output,
    );
    let output_attempts = write_attempts.clone();
    let record = record_subscriber(LogLevel::Debug, move || {
        UnwritableStream(output_attempts.clone())
    });
    let load_result = tracing::subscriber::with_default(record, || {
        tracing::error_span!("load", account = "account_1726920000_0").in_scope(|| {
            tracing::error!(cause = "TimedOut", "load failed");
            tracing::debug!("text loaded");
            "the load's result"
        })
    });
    assert_eq!(load_result, "the load's result");
    assert_eq!(
        write_attempts.load(Ordering::Relaxed),
        3,
        "the first line and both events were each written once and dropped"
    );
}

#[test]
fn the_first_line_gives_versions_and_level_whatever_the_level() {
    let mut written = Vec::new();
    write_first_line(
        LogLevel::Error,
        "GTK 4.20.1, libadwaita 1.8.0",
        &mut written,
    );
    let first_line = String::from_utf8(written).expect("UTF-8");
    assert!(starts_with_local_time(&first_line), "{first_line}");
    let application = format!("  INFO mailbag: Mailbag {}, ", env!("CARGO_PKG_VERSION"));
    assert!(first_line.contains(&application), "{first_line}");
    assert!(
        first_line.contains(", native build, ") || first_line.contains(", Flatpak build, "),
        "{first_line}"
    );
    assert!(
        first_line.ends_with(", GTK 4.20.1, libadwaita 1.8.0, level error\n"),
        "{first_line}"
    );
    assert_eq!(first_line.lines().count(), 1, "{first_line}");
}

#[test]
fn quitting_is_recorded_at_info() {
    let record = start_record(LogLevel::Info);
    finish_logging();
    let text = record.text();
    let last_line = text.lines().last().expect("the quit line");
    assert!(
        last_line.contains(" INFO mailbag::logging: Mailbag is quitting"),
        "{text}"
    );
}

#[test]
fn without_the_option_no_subscriber_exists() {
    // No test installs a subscriber for the whole process, so a thread that
    // starts no record sees what Mailbag has without --log-level.
    tracing::dispatcher::get_default(|current| {
        assert!(current.is::<tracing::subscriber::NoSubscriber>());
    });
    assert!(!tracing::enabled!(tracing::Level::ERROR));
}
