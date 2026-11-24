//! Build script for typio-host-platform.
//!
//! Emits rpath link arguments for the Optics module family so panel and text-rendering
//! test harnesses resolve the same shared libraries bindgen saw.
//!
//! Owns platform-native prerequisites and rpath propagation for the Wayland /
//! Optics integration crate itself, including its test binaries.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

include!("../optics-rpaths.rs");

fn main() {
    let build_dirs = optics_build_dirs();
    enforce_optimized_gfx_for_release(&build_dirs);

    // The platform crate reaches Optics `gfx` and `text` through the `optics` facade,
    // so the `-sys` crates' `links` metadata is no longer visible here. Emit
    // the build-tree rpaths when Optics is linked dynamically so test harnesses
    // load the libraries bindgen saw rather than a stale system install.
    emit_optics_rpaths();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../optics-rpaths.rs");
    rerun_on_optics_paths();
}

/// Reject a Cargo release binary backed by an unoptimized Meson Optics `gfx` build.
///
/// The Rust profile does not affect native libraries selected through
/// `OPTICS_BUILD_DIR` or `OPTICS_GFX_BUILD_DIR`. Walk upward from the configured
/// Meson build directories (regardless of static vs. dynamic linkage); when a
/// tree carries Meson introspection data, treat it as the authoritative
/// optimization setting. Installed libraries have no Meson metadata nearby and
/// remain the packager's responsibility.
fn enforce_optimized_gfx_for_release(build_dirs: &[PathBuf]) {
    if env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }

    let mut checked = HashSet::new();
    for dir in build_dirs {
        let Some(options_path) = find_meson_build_options(dir) else {
            continue;
        };
        if !checked.insert(options_path.clone()) {
            continue;
        }
        println!("cargo:rerun-if-changed={}", options_path.display());

        let contents = fs::read_to_string(&options_path).unwrap_or_else(|error| {
            panic!(
                "failed to read Optics gfx Meson build options at {}: {error}",
                options_path.display()
            )
        });
        let options: serde_json::Value = serde_json::from_str(&contents).unwrap_or_else(|error| {
            panic!(
                "failed to parse Optics gfx Meson build options at {}: {error}",
                options_path.display()
            )
        });
        let optimization = meson_string_option(&options, "optimization").unwrap_or_else(|| {
            panic!(
                "Optics gfx Meson build options at {} do not report optimization",
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
                "Cargo release cannot link the unoptimized Optics gfx build at {} \
                 (Meson buildtype={buildtype}, optimization=0). Configure a \
                 release tree and point OPTICS_BUILD_DIR (or OPTICS_GFX_BUILD_DIR) at it, for example: \
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
