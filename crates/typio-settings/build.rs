//! Build script for typio-settings.
//!
//! The settings application reaches `optics::app`, `optics::window`, and
//! `optics::ui` through the `optics` facade, so it does not see the `-sys`
//! crates' `links` metadata.
//! Re-emit the native library rpaths when Optics is linked dynamically so local
//! and CI binaries load the sibling Optics build tree rather than a stale
//! system install. Statically linked binaries emit no rpath tags.

include!("../optics-rpaths.rs");

fn main() {
    let rpaths = optics_native_rpaths();
    if !rpaths.is_empty() {
        // Dynamically linked settings binaries keep DT_RPATH rather than
        // DT_RUNPATH so the development tree wins over an environment path or
        // a stale system install.
        println!("cargo:rustc-link-arg=-Wl,--disable-new-dtags");
        for directory in rpaths {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{directory}");
        }
    }
    rerun_on_optics_paths();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../optics-rpaths.rs");
}
