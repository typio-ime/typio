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
    emit_optics_rpaths();
    rerun_on_optics_paths();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../optics-rpaths.rs");
}
