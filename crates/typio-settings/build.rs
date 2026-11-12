//! Build script for typio-settings.
//!
//! The settings application reaches Iris and Lens through the `optics` facade,
//! so it does not see the `-sys` crates' `links` metadata. Re-emit the native
//! library rpaths explicitly so local and CI binaries load the sibling Optics
//! build tree rather than a stale system install.

include!("../optics-rpaths.rs");

fn main() {
    // The settings binary keeps DT_RPATH rather than DT_RUNPATH so the
    // development tree wins over an environment path or a stale system install.
    println!("cargo:rustc-link-arg=-Wl,--disable-new-dtags");
    emit_optics_rpaths();
    rerun_on_optics_paths();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../optics-rpaths.rs");
}
