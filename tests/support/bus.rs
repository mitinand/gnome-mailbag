// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

/// A private D-Bus daemon that cannot launch desktop services. Tests install
/// only their fake GOA service. A timeout kills the daemon even if the test hangs.
pub struct TestBus {
    pub address: String,
    directory: std::path::PathBuf,
    daemon: Arc<Mutex<Child>>,
    shutdown_requested: Arc<(Mutex<bool>, Condvar)>,
    watchdog: Option<thread::JoinHandle<()>>,
}

impl TestBus {
    pub fn new() -> Self {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "mailbag-bus-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let config = directory.join("bus.conf");
        std::fs::write(&config, r#"<busconfig>
<type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy>
</busconfig>"#).unwrap();
        let mut daemon = Command::new("dbus-daemon")
            .args(["--nofork", "--print-address=1", "--config-file"])
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("private dbus-daemon (install dbus-daemon)");
        let stdout = daemon.stdout.take().unwrap();
        let daemon = Arc::new(Mutex::new(daemon));
        let shutdown_requested = Arc::new((Mutex::new(false), Condvar::new()));
        let watchdog = {
            let daemon = daemon.clone();
            let shutdown_requested = shutdown_requested.clone();
            thread::spawn(move || {
                let (lock, wake) = &*shutdown_requested;
                let (finished, _) = wake
                    .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(15), |v| !*v)
                    .unwrap();
                if !*finished {
                    let _ = daemon.lock().unwrap().kill();
                }
            })
        };
        let mut bus = Self {
            address: String::new(),
            directory,
            daemon,
            shutdown_requested,
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
impl Drop for TestBus {
    fn drop(&mut self) {
        *self.shutdown_requested.0.lock().unwrap() = true;
        self.shutdown_requested.1.notify_one();
        let mut daemon = self.daemon.lock().unwrap();
        let _ = daemon.kill();
        let _ = daemon.wait();
        drop(daemon);
        self.watchdog.take().unwrap().join().unwrap();
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}
