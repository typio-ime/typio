//! Build script for typio-host-platform.
//!
//! Owns platform-native prerequisites and rpath propagation for the Wayland /
//! Flux integration crate itself, including its test binaries.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let project_root = manifest_dir.parent().unwrap().parent().unwrap();
    let optics_dir = project_root.join("optics");

    if optics_dir.exists() {
        let build_dir = optics_dir.join("build");
        if !build_dir.exists() {
            let status = Command::new("meson")
                .arg("setup")
                .arg(&build_dir)
                .current_dir(&optics_dir)
                .status()
                .expect("Failed to run meson setup");
            if !status.success() {
                panic!("meson setup failed");
            }
        }

        let status = Command::new("meson")
            .arg("compile")
            .arg("-C")
            .arg(&build_dir)
            .current_dir(&optics_dir)
            .status()
            .expect("Failed to run meson compile");
        if !status.success() {
            panic!("meson compile failed");
        }
    }

    // flux-sys publishes the Meson build-tree rpaths it used via `links`
    // metadata. Re-emit them here so platform crate tests load the same
    // libraries bindgen saw, not a stale or missing system install.
    if let Ok(rpaths) = env::var("DEP_FLUX_RPATHS") {
        for dir in rpaths.split(';').filter(|s| !s.is_empty()) {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        }
    }

    if let Ok(rpaths) = env::var("DEP_FLUX_TEXT_RPATHS") {
        for dir in rpaths.split(';').filter(|s| !s.is_empty()) {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DEP_FLUX_RPATHS");
    println!("cargo:rerun-if-env-changed=DEP_FLUX_TEXT_RPATHS");
}
