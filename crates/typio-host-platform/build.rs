//! Build script for typio-host-platform.
//!
//! Owns platform-native prerequisites and rpath propagation for the Wayland /
//! Flux integration crate itself, including its test binaries.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
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

    let flux_rpaths = env::var("DEP_FLUX_RPATHS").unwrap_or_default();
    let flux_text_rpaths = env::var("DEP_FLUX_TEXT_RPATHS").unwrap_or_default();
    enforce_optimized_flux_for_release(&[&flux_rpaths, &flux_text_rpaths]);

    // flux-sys publishes the Meson build-tree rpaths it used via `links`
    // metadata. Re-emit them here so platform crate tests load the same
    // libraries bindgen saw, not a stale or missing system install.
    for dir in flux_rpaths.split(';').filter(|s| !s.is_empty()) {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    }

    for dir in flux_text_rpaths.split(';').filter(|s| !s.is_empty()) {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DEP_FLUX_RPATHS");
    println!("cargo:rerun-if-env-changed=DEP_FLUX_TEXT_RPATHS");
}

/// Reject a Cargo release binary backed by a Meson `-O0` Flux build.
///
/// The Rust profile does not affect native libraries selected through
/// `FLUX_BUILD_DIR`. Walk upward from the rpaths published by `flux-sys` and
/// `flux-text-sys`; when they belong to a Meson build tree, use its
/// introspection data as the authoritative optimization setting. Installed
/// libraries have no Meson metadata nearby and remain the packager's
/// responsibility.
fn enforce_optimized_flux_for_release(rpath_sets: &[&str]) {
    if env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }

    let mut checked = HashSet::new();
    for rpath in rpath_sets
        .iter()
        .flat_map(|paths| paths.split(';'))
        .filter(|path| !path.is_empty())
    {
        let Some(options_path) = find_meson_build_options(Path::new(rpath)) else {
            continue;
        };
        if !checked.insert(options_path.clone()) {
            continue;
        }
        println!("cargo:rerun-if-changed={}", options_path.display());

        let contents = fs::read_to_string(&options_path).unwrap_or_else(|error| {
            panic!(
                "failed to read Flux Meson build options at {}: {error}",
                options_path.display()
            )
        });
        let options: serde_json::Value = serde_json::from_str(&contents).unwrap_or_else(|error| {
            panic!(
                "failed to parse Flux Meson build options at {}: {error}",
                options_path.display()
            )
        });
        let optimization = meson_string_option(&options, "optimization").unwrap_or_else(|| {
            panic!(
                "Flux Meson build options at {} do not report optimization",
                options_path.display()
            )
        });
        if optimization == "0" {
            let buildtype = meson_string_option(&options, "buildtype").unwrap_or("unknown");
            let build_dir = options_path
                .parent()
                .and_then(Path::parent)
                .unwrap_or(&options_path);
            panic!(
                "Cargo release cannot link the unoptimized Flux build at {} \
                 (Meson buildtype={buildtype}, optimization=0). Configure a \
                 release tree and point FLUX_BUILD_DIR at it, for example: \
                 meson setup ../optics/build-release ../optics -Dtext=true \
                 --buildtype=release",
                build_dir.display()
            );
        }
    }
}

fn find_meson_build_options(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .map(|ancestor| ancestor.join("meson-info/intro-buildoptions.json"))
        .find(|candidate| candidate.is_file())
}

fn meson_string_option<'a>(options: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    options.as_array()?.iter().find_map(|option| {
        if option.get("name")?.as_str()? != name {
            return None;
        }
        option.get("value")?.as_str()
    })
}
