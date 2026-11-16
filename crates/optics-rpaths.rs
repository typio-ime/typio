// Shared by the Typio build scripts. Included with `include!` because it is
// ordinary build-script code, not a crate module.
//
// Cargo does not propagate a dependency build script's `rustc-link-arg` to a
// downstream final artifact, and the `DEP_*_RPATHS` metadata published by the
// Optics `-sys` crates is visible only to direct dependents. Since Typio now
// reaches the native stack through the `optics` facade, neither route survives:
// derive the Meson build-tree library directories from the same environment
// variables the `-sys` build scripts consume, and let each Typio build script
// re-emit them for its own binaries and test harnesses.

/// Return the distinct native-library directories selected by the Optics
/// build-tree environment variables.
///
/// Installed-prefix builds leave the variables unset (or point at a prefix
/// without `libs/`), so the returned list is empty and the loader uses the
/// normal system library path.
fn optics_native_rpaths() -> Vec<String> {
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for variable in [
        "FLUX_BUILD_DIR",
        "GLYPH_BUILD_DIR",
        "LENS_BUILD_DIR",
        "IRIS_BUILD_DIR",
        "APERTURE_BUILD_DIR",
    ] {
        let Some(build_dir) = std::env::var_os(variable) else {
            continue;
        };
        let libs = std::path::PathBuf::from(build_dir).join("libs");
        for library in ["flux", "glyph", "lens", "iris", "aperture", "reticle"] {
            let directory = libs.join(library);
            if !directory.is_dir() {
                continue;
            }
            let path = directory.to_string_lossy().into_owned();
            if seen.insert(path.clone()) {
                paths.push(path);
            }
        }
    }

    paths
}

/// Emit the rpaths as linker arguments for the calling build script.
fn emit_optics_rpaths() {
    for directory in optics_native_rpaths() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{directory}");
    }
}

/// Emit rerun guards for every variable that selects a native library.
fn rerun_on_optics_paths() {
    for variable in [
        "FLUX_BUILD_DIR",
        "GLYPH_BUILD_DIR",
        "LENS_BUILD_DIR",
        "IRIS_BUILD_DIR",
        "APERTURE_BUILD_DIR",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
}
