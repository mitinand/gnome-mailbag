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
        Self::start(false)
    }

    pub fn with_activation() -> Self {
        Self::start(true)
    }

    pub fn set_activation_mode(&self, mode: &str) {
        self.stop_activated_service();
        std::fs::write(self.directory.join("mode"), mode).unwrap();
        std::fs::write(self.directory.join("run"), "").unwrap();
    }

    fn stop_activated_service(&self) {
        let _ = std::fs::remove_file(self.directory.join("run"));
        let Ok(pid) = std::fs::read_to_string(self.directory.join("service.pid")) else {
            return;
        };
        let pid: u32 = pid.trim().parse().expect("fixture process ID");
        let process_path = std::path::PathBuf::from(format!("/proc/{pid}"));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while process_path.exists() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if process_path.exists() {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
        }
        let _ = std::fs::remove_file(self.directory.join("service.pid"));
    }

    fn start(activation_enabled: bool) -> Self {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "mailbag-bus-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let config = directory.join("bus.conf");
        let service_directory_xml = if activation_enabled {
            let service_directory = directory.join("services");
            std::fs::create_dir(&service_directory).unwrap();
            let activation_script = directory.join("activate.sh");
            let quote_shell_path = |path: &std::path::Path| {
                format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
            };
            std::fs::write(&activation_script, format!("#!/bin/sh\necho \"$$\" > {}\nexport MAILBAG_ACTIVATION_DIRECTORY={}\nexec {} --exact test_goa::activated_service_process --ignored > {} 2>&1\n",
                quote_shell_path(&directory.join("service.pid")), quote_shell_path(&directory), quote_shell_path(&std::env::current_exe().unwrap()), quote_shell_path(&directory.join("activation.log")))).unwrap();
            std::fs::write(
                service_directory.join("org.gnome.OnlineAccounts.service"),
                format!(
                    "[D-BUS Service]\nName=org.gnome.OnlineAccounts\nExec=/bin/sh {}\n",
                    activation_script.display()
                ),
            )
            .unwrap();
            std::fs::write(directory.join("mode"), "hang").unwrap();
            std::fs::write(directory.join("run"), "").unwrap();
            format!("<servicedir>{}</servicedir>", service_directory.display())
        } else {
            String::new()
        };
        std::fs::write(&config, format!(r#"<busconfig>
<type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
{service_directory_xml}
<policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy>
</busconfig>"#)).unwrap();
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
        self.stop_activated_service();
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
