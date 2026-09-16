// SPDX-FileCopyrightText: 2026 Andrey Mitin
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo::rerun-if-changed=resources/mailbag.gresource.xml");
    println!("cargo::rerun-if-changed=resources/icons");
    let target = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build output"))
        .join("mailbag.gresource");
    let status = Command::new("glib-compile-resources")
        .arg("resources/mailbag.gresource.xml")
        .arg("--sourcedir=resources")
        .arg("--target")
        .arg(target)
        .status()
        .expect("run glib-compile-resources from the GLib development tools");
    assert!(status.success(), "could not compile application resources");
}
