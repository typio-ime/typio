// Shared by the Typio build scripts. Included with `include!` because it is
// ordinary build-script code, not a crate module.
//
// Cargo does not propagate a dependency build script's `rustc-link-arg` to a
// downstream final artifact, and the `DEP_*_RPATHS` metadata published by the
// Optics `-sys` crates is visible only to direct dependents. Since Typio
// reaches the native stack through the `optics` facade, neither route survives:
// inspect the same Meson build-tree environment variables the `-sys` build
// scripts consume, and re-emit rpaths only when an Optics library is linked
// dynamically. When Optics static archives (`lib*.a` / `*.lib`) are present
// and `OPTICS_DYNAMIC` is unset, the `-sys` crates link Optics statically and
// binaries require no build-tree rpath.

/// Build-tree environment variables inspected by the Optics `-sys` crates.
const OPTICS_BUILD_DIR_VARS: &[&str] = &[
    "OPTICS_BUILD_DIR",
    "FLUX_BUILD_DIR",
    "GLYPH_BUILD_DIR",
    "LENS_BUILD_DIR",
    "IRIS_BUILD_DIR",
    "APERTURE_BUILD_DIR",
    "RETICLE_BUILD_DIR",
];

/// Per-library build-dir override variables in precedence order.
const OPTICS_LIBRARY_BUILD_VARS: &[(&str, &[&str])] = &[
    ("flux", &["FLUX_BUILD_DIR", "OPTICS_BUILD_DIR"]),
    ("glyph", &["GLYPH_BUILD_DIR", "OPTICS_BUILD_DIR"]),
    ("lens", &["LENS_BUILD_DIR", "OPTICS_BUILD_DIR"]),
    ("iris", &["IRIS_BUILD_DIR", "OPTICS_BUILD_DIR"]),
    ("aperture", &["APERTURE_BUILD_DIR", "OPTICS_BUILD_DIR"]),
    (
        "reticle",
        &[
            "RETICLE_BUILD_DIR",
            "OPTICS_BUILD_DIR",
            "LENS_BUILD_DIR",
            "APERTURE_BUILD_DIR",
        ],
    ),
];

/// Return the distinct Optics Meson build directories named by the environment.
///
/// Unlike [`optics_native_rpaths`], this returns the build trees regardless of
/// whether Optics is linked statically or dynamically, so release-profile
/// checks still inspect Meson's build options on static builds.
#[allow(dead_code)]
fn optics_build_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for variable in OPTICS_BUILD_DIR_VARS {
        let Some(value) = std::env::var_os(variable) else {
            continue;
        };
        let path = std::path::PathBuf::from(value);
        if path.is_dir() && seen.insert(path.clone()) {
            dirs.push(path);
        }
    }

    dirs
}

fn optics_target_os() -> String {
    std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "linux".to_string())
}

fn optics_has_static_archive(dir: &std::path::Path, library: &str, target_os: &str) -> bool {
    if target_os == "windows" {
        dir.join(format!("{library}.lib")).is_file()
            || dir.join(format!("lib{library}.a")).is_file()
    } else {
        dir.join(format!("lib{library}.a")).is_file()
    }
}

fn optics_has_shared_library(dir: &std::path::Path, library: &str, target_os: &str) -> bool {
    let name = match target_os {
        "windows" => format!("{library}.dll"),
        "macos" => format!("lib{library}.dylib"),
        _ => format!("lib{library}.so"),
    };
    dir.join(name).exists()
}

/// Return the distinct native-library directories that require a runtime rpath.
///
/// Mirrors the Optics `-sys` crates' linkage selection: static archives win
/// whenever present unless `OPTICS_DYNAMIC` is set, so statically linked
/// binaries receive an empty rpath list. Installed-prefix builds leave the
/// variables unset and also return an empty list.
fn optics_native_rpaths() -> Vec<String> {
    let target_os = optics_target_os();
    let force_dynamic = std::env::var_os("OPTICS_DYNAMIC").is_some();
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for &(library, variables) in OPTICS_LIBRARY_BUILD_VARS {
        let Some(build_dir) = variables.iter().find_map(std::env::var_os) else {
            continue;
        };
        let build_dir = std::path::PathBuf::from(build_dir);
        let directory = build_dir.join("libs").join(library);
        let candidate = if directory.is_dir() {
            directory
        } else if build_dir.is_dir() {
            build_dir
        } else {
            continue;
        };

        let has_static = optics_has_static_archive(&candidate, library, &target_os);
        let has_shared = optics_has_shared_library(&candidate, library, &target_os);
        let use_static = !force_dynamic && has_static;
        if use_static || !has_shared {
            continue;
        }

        let path = candidate.to_string_lossy().into_owned();
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }

    paths
}

/// Emit the rpaths as linker arguments for the calling build script.
#[allow(dead_code)]
fn emit_optics_rpaths() {
    for directory in optics_native_rpaths() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{directory}");
    }
}

/// Emit rerun guards for every variable that selects a native library or
/// linkage mode.
fn rerun_on_optics_paths() {
    for variable in OPTICS_BUILD_DIR_VARS {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    println!("cargo:rerun-if-env-changed=OPTICS_STATIC");
    println!("cargo:rerun-if-env-changed=OPTICS_DYNAMIC");
}
