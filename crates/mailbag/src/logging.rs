// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! The record Mailbag writes to the standard error stream when started with
//! `--log-level`: its level, the time on each line, its first and last line,
//! and the names of provider types (specs/003-logging). Without the option
//! nothing here is installed.

#[cfg(test)]
pub mod capture;
#[cfg(test)]
mod tests;

use adw::{glib, gtk};
use goa_adapter::AccountProvider;
use std::{fmt, io};
use tracing_subscriber::fmt::{MakeWriter, format::Writer, time::FormatTime};

/// The level chosen with `--log-level`. Each level includes those before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warning,
    Info,
    Debug,
}

impl LogLevel {
    fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    fn most_detailed_event(self) -> tracing::Level {
        match self {
            Self::Error => tracing::Level::ERROR,
            Self::Warning => tracing::Level::WARN,
            Self::Info => tracing::Level::INFO,
            Self::Debug => tracing::Level::DEBUG,
        }
    }
}

/// Reads the value of `--log-level`, or returns the message that names the
/// accepted levels.
pub fn parse_log_level(value: &str) -> Result<LogLevel, String> {
    [
        LogLevel::Error,
        LogLevel::Warning,
        LogLevel::Info,
        LogLevel::Debug,
    ]
    .into_iter()
    .find(|level| level.name() == value)
    .ok_or_else(|| format!("Unknown log level \"{value}\". Use error, warning, info or debug."))
}

/// Writes the first line, then records the rest of the run on `output`, the
/// standard error stream in the application. GTK must be initialized.
pub fn start_logging<W>(level: LogLevel, output: W)
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let library_versions = format!(
        "GTK {}.{}.{}, libadwaita {}.{}.{}",
        gtk::major_version(),
        gtk::minor_version(),
        gtk::micro_version(),
        adw::major_version(),
        adw::minor_version(),
        adw::micro_version(),
    );
    // The first line bypasses the level filter: a record is unusable without it.
    write_first_line(level, &library_versions, &mut output.make_writer());
    tracing::subscriber::set_global_default(record_subscriber(level, output))
        .expect("logging is started once, before any event");
}

/// Writes the record's last line when Mailbag quits.
pub fn finish_logging() {
    tracing::info!("Mailbag is quitting");
}

/// Writes the record's events to `output`: the level filter and the library's
/// one-line formatter.
fn record_subscriber<W>(level: LogLevel, output: W) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_max_level(level.most_detailed_event())
        .with_timer(LocalTime)
        .with_ansi(false)
        // By default a failed write is reported with `eprintln!`, which panics
        // when the standard error stream itself cannot be written.
        .log_internal_errors(false)
        .with_writer(output)
        .finish()
}

/// Versions, build kind and level, laid out like every other line
/// (specs/003-logging/contracts/record.md "First line"). A failed write is ignored.
fn write_first_line(level: LogLevel, library_versions: &str, output: &mut impl io::Write) {
    let mut line = String::new();
    let _ = LocalTime.format_time(&mut Writer::new(&mut line));
    line.push_str(&format!(
        "  INFO mailbag: Mailbag {}, {}, {library_versions}, level {}\n",
        env!("CARGO_PKG_VERSION"),
        build_kind(),
        level.name(),
    ));
    let _ = output.write_all(line.as_bytes());
}

/// "Flatpak build" with the runtime in the sandbox, otherwise "native build"
/// with the operating system's name.
fn build_kind() -> String {
    let flatpak_info = glib::KeyFile::new();
    if flatpak_info
        .load_from_file("/.flatpak-info", glib::KeyFileFlags::NONE)
        .is_ok()
    {
        let runtime = flatpak_info.string("Application", "runtime");
        format!(
            "Flatpak build, {}",
            runtime.as_deref().unwrap_or("unknown runtime")
        )
    } else {
        let system = glib::os_info("PRETTY_NAME");
        format!(
            "native build, {}",
            system.as_deref().unwrap_or("unknown operating system")
        )
    }
}

/// The time of a line: local, with milliseconds and the UTC offset, such as
/// `2026-09-21T14:03:15.102+03:00`.
struct LocalTime;

impl FormatTime for LocalTime {
    fn format_time(&self, output: &mut Writer<'_>) -> fmt::Result {
        let now = glib::DateTime::now_local().map_err(|_| fmt::Error)?;
        let seconds = now.format("%Y-%m-%dT%H:%M:%S").map_err(|_| fmt::Error)?;
        let offset = now.format("%:z").map_err(|_| fmt::Error)?;
        write!(output, "{seconds}.{:03}{offset}", now.microsecond() / 1000)
    }
}

/// The provider type the record writes for an account.
pub fn provider_type(provider: AccountProvider) -> &'static str {
    match provider {
        AccountProvider::ImapSmtp => "imap",
        AccountProvider::Google => "google",
        AccountProvider::Microsoft365 => "microsoft365",
        AccountProvider::Other => "other",
    }
}
