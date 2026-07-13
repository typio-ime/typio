//! Relay native-library rpaths to the final application binary.
//!
//! Cargo link arguments do not propagate from dependency targets. The raw
//! bindings publish their development-tree library paths as metadata, so the
//! final binary has to re-emit them here.

fn main() {
    println!("cargo:rustc-link-arg=-Wl,--disable-new-dtags");
    for variable in ["DEP_IRIS_RPATHS", "DEP_LENS_RPATHS", "DEP_FLUX_RPATHS"] {
        println!("cargo:rerun-if-env-changed={variable}");
        if let Ok(paths) = std::env::var(variable) {
            for path in paths.split(';').filter(|path| !path.is_empty()) {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
            }
        }
    }
}
