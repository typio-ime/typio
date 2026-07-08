// Build script for libtypio.
//
// Engine plugin directory resolution is the host's responsibility, not
// core's — core does not bake in any engine path.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set");

    // Restrict the cdylib's exported symbols to the public `typio_*` /
    // `TYPIO_*` namespace via a version script. Without this every Rust
    // `#[unsafe(no_mangle)]` helper and a sprinkling of runtime symbols ship as
    // part of libtypio.so, polluting the ABI surface.
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "macos" && target_os != "windows" {
        let version_script = format!("{}/libtypio.map", manifest);
        println!("cargo:rerun-if-changed=libtypio.map");
        println!(
            "cargo:rustc-cdylib-link-arg=-Wl,--version-script,{}",
            version_script
        );
    }

    // Emit pkg-config files so C consumers (typio, engines) can
    // discover this library without hard-coding paths.
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let out_path = PathBuf::from(&out_dir);

    // OUT_DIR is deep inside target/<profile>/build/...; write .pc files
    // directly into target/<profile>/ so they are easy to find.
    let _profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let mut target_profile = out_path.clone();
    for _ in 0..3 {
        if let Some(parent) = target_profile.parent() {
            target_profile = parent.to_path_buf();
        }
    }

    let prefix = manifest.clone();
    let libdir = target_profile.to_string_lossy().to_string();

    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());

    let engine_abi_pc = format!(
        "prefix={prefix}\n\
         includedir=${{prefix}}/include\n\
         libdir={libdir}\n\n\
         Name: typio-engine-abi\n\
         Description: Typio engine plugin ABI (headers + core library to link against)\n\
         Version: {version}\n\
         Libs: -L${{libdir}} -ltypio\n\
         Cflags: -I${{includedir}}\n",
        prefix = prefix,
        libdir = libdir,
        version = version,
    );

    let libtypio_pc = format!(
        "prefix={prefix}\n\
         includedir=${{prefix}}/include\n\
         libdir={libdir}\n\n\
         Name: libtypio\n\
         Description: Typio input method framework — core library and full C ABI\n\
         Version: {version}\n\
         Requires: typio-engine-abi\n\
         Libs: -L${{libdir}} -ltypio\n\
         Cflags: -I${{includedir}}\n",
        prefix = prefix,
        libdir = libdir,
        version = version,
    );

    let engine_abi_path = target_profile.join("typio-engine-abi.pc");
    let libtypio_path = target_profile.join("libtypio.pc");

    fs::write(&engine_abi_path, engine_abi_pc).expect("failed to write typio-engine-abi.pc");
    fs::write(&libtypio_path, libtypio_pc).expect("failed to write libtypio.pc");

    eprintln!("Generated {}", engine_abi_path.display());
    eprintln!("Generated {}", libtypio_path.display());
}
