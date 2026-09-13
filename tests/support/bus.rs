// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

/// A separate daemon with no service directories: host services cannot activate.
/// A watchdog kills it even if a fixture stops dispatching its GLib context.
pub struct Bus {
    pub address: String,
    directory: std::path::PathBuf,
    child: Arc<Mutex<Child>>,
    done: Arc<(Mutex<bool>, Condvar)>,
    watchdog: Option<thread::JoinHandle<()>>,
}

impl Bus {
    pub fn new() -> Self {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "mailbag-bus-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let config = directory.join("bus.conf");
        std::fs::write(&config, br#"<busconfig>
<type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy>
</busconfig>"#).unwrap();
        let mut child = Command::new("dbus-daemon")
            .args(["--nofork", "--print-address=1", "--config-file"])
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("private dbus-daemon (install dbus-daemon)");
        let stdout = child.stdout.take().unwrap();
        let child = Arc::new(Mutex::new(child));
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let watchdog = {
            let child = child.clone();
            let done = done.clone();
            thread::spawn(move || {
                let (lock, wake) = &*done;
                let (finished, _) = wake
                    .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(15), |v| !*v)
                    .unwrap();
                if !*finished {
                    let _ = child.lock().unwrap().kill();
                }
            })
        };
        let mut bus = Self {
            address: String::new(),
            directory,
            child,
            done,
            watchdog: Some(watchdog),
        };
        BufReader::new(stdout).read_line(&mut bus.address).unwrap();
        bus.address = bus.address.trim().to_owned();
        assert!(
            bus.address.starts_with("unix:"),
            "private bus did not start"
        );
        bus
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        *self.done.0.lock().unwrap() = true;
        self.done.1.notify_one();
        let mut child = self.child.lock().unwrap();
        let _ = child.kill();
        let _ = child.wait();
        drop(child);
        self.watchdog.take().unwrap().join().unwrap();
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}
