//! Packaged-resource checks: the assets that must ship alongside the native
//! engine artifact.
//!
//! Today this is the freedesktop icon contract. `TypioEngineInfo.icon` reports
//! an *icon name* (not a path); the engine package is expected to provide a
//! matching SVG under `data/icons/hicolor/{scalable,symbolic}/apps/`. The host
//! resolves the name against the icon theme at runtime, so a mismatch here is
//! invisible until a user sees a blank tray icon.

use std::ffi::{c_char, CStr};
use std::fs;
use std::path::{Path, PathBuf};

use typio_abi::TypioEngineInfo;

use crate::check::{CheckCategory, CheckResult};

const RES: CheckCategory = CheckCategory::Resource;

/// Locate the package root for a native engine artifact, looking for the
/// `data/icons` (source layout) or `icons/hicolor` (bundled-next-to-artifact
/// layout) tree by walking up from the artifact's directory.
pub fn discover_package(artifact_path: &Path) -> Option<PathBuf> {
    let start = artifact_path.parent()?;
    let mut dir = Some(start);
    for _ in 0..8 {
        let d = dir?;
        if d.join("data/icons/hicolor").is_dir() || d.join("icons/hicolor").is_dir() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// Run resource checks for an engine given its info and (optionally) the
/// package root. With no package root, file-level checks are skipped with a
/// warning rather than failing.
///
/// # Safety
/// `info` must be a valid pointer (or null) returned by the engine.
pub unsafe fn resource_checks(
    info: *const TypioEngineInfo,
    pkg: Option<&Path>,
) -> Vec<CheckResult> {
    let icon_name = if info.is_null() {
        None
    } else {
        cstr((*info).icon)
    };
    check_icon(icon_name.as_deref(), pkg)
}

fn check_icon(icon_name: Option<&str>, pkg: Option<&Path>) -> Vec<CheckResult> {
    let mut out = Vec::new();

    let name = match icon_name {
        Some(n) if !n.is_empty() => n,
        _ => {
            out.push(CheckResult::warn(
                RES,
                "icon_name",
                "no icon declared in TypioEngineInfo",
            ));
            return out;
        }
    };

    // icon_name must be a freedesktop *name*, not a path or a filename.
    if name.contains('/') || name.contains('\\') {
        out.push(CheckResult::fail(
            RES,
            "icon_name",
            format!("icon '{name}' looks like a path; it must be an icon name"),
        ));
    } else if Path::new(name).extension().is_some() {
        out.push(CheckResult::fail(
            RES,
            "icon_name",
            format!("icon '{name}' carries a file extension; report a bare name"),
        ));
    } else {
        out.push(CheckResult::pass(RES, "icon_name"));
    }

    let pkg = match pkg {
        Some(p) => p,
        None => {
            out.push(CheckResult::warn(
                RES,
                "icon_asset",
                "package root not found; skipped SVG asset checks",
            ));
            return out;
        }
    };

    // Collect candidate SVGs from both the source and bundled layouts.
    let mut svgs = Vec::new();
    for base in ["data/icons/hicolor", "icons/hicolor"] {
        collect_svgs(&pkg.join(base), &mut svgs);
    }

    // An asset matches if its file stem is `<name>` or `<name>-symbolic`.
    let symbolic = format!("{name}-symbolic");
    let matched: Vec<&PathBuf> = svgs
        .iter()
        .filter(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|stem| stem == name || stem == symbolic)
                .unwrap_or(false)
        })
        .collect();

    if matched.is_empty() {
        out.push(CheckResult::fail(
            RES,
            "icon_asset",
            format!(
                "no SVG named '{name}' or '{symbolic}' under {}",
                pkg.join("data/icons/hicolor").display()
            ),
        ));
        return out;
    }
    out.push(CheckResult::pass(RES, "icon_asset"));

    // Confirm every matched asset is readable (a missing/unreadable file is
    // a real packaging bug). SVG well-formedness is left to the compositor's
    // own renderer — vet does not reimplement an XML parser.
    let mut placement_warn: Option<String> = None;
    for path in &matched {
        if let Err(e) = fs::read(path) {
            out.push(CheckResult::fail(
                RES,
                "icon_asset",
                format!("{}: {e}", rel(pkg, path)),
            ));
        }

        // A `-symbolic` asset should live under a `symbolic/` or `scalable/`
        // theme dir; anything else is a freedesktop convention slip.
        let path_str = path.to_string_lossy();
        let is_symbolic_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.ends_with("-symbolic"))
            .unwrap_or(false);
        if is_symbolic_name && !(path_str.contains("/symbolic/") || path_str.contains("/scalable/"))
        {
            placement_warn = Some(format!(
                "{} is a symbolic icon outside symbolic/ or scalable/",
                rel(pkg, path)
            ));
        }
    }
    out.push(CheckResult::pass(RES, "icon_asset"));
    if let Some(msg) = placement_warn {
        out.push(CheckResult::warn(RES, "icon_placement", msg));
    }

    out
}

fn collect_svgs(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_svgs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("svg") {
            out.push(path);
        }
    }
}

fn cstr(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .ok()
        .map(|s| s.to_owned())
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
