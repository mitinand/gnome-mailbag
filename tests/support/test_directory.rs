// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

//! A directory of a test's own under the system's temporary directory, for a
//! store kept in a file.

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

/// A directory of its own, removed with what it holds when dropped.
pub struct TestDirectory(pub PathBuf);

impl TestDirectory {
    pub fn new() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "mailbag-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("a test directory");
        Self(path)
    }

    /// Where the application keeps its store, below this directory.
    pub fn store_path(&self) -> PathBuf {
        self.0.join("mailbag").join("mail.sqlite")
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}
