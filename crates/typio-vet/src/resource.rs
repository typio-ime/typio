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

    // Validate every matched SVG and its placement.
    let mut svg_ok = true;
    let mut placement_warn: Option<String> = None;
    for path in &matched {
        match fs::read(path) {
            Ok(bytes) => {
                if let Err(e) = validate_svg(&bytes) {
                    svg_ok = false;
                    out.push(CheckResult::fail(
                        RES,
                        "svg_wellformed",
                        format!("{}: {e}", rel(pkg, path)),
                    ));
                }
            }
            Err(e) => {
                svg_ok = false;
                out.push(CheckResult::fail(
                    RES,
                    "svg_wellformed",
                    format!("{}: {e}", rel(pkg, path)),
                ));
            }
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
    if svg_ok {
        out.push(CheckResult::pass(RES, "svg_wellformed"));
    }
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

/// Dependency-free sanity check that a byte slice is a plausible, well-formed
/// SVG: valid UTF-8, an `<svg>` root with sized geometry, and balanced tags.
/// This is a vet-level smoke test, not a full XML validator.
pub fn validate_svg(bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "not valid UTF-8".to_string())?;

    let svg_start = text.find("<svg").ok_or("no <svg> root element")?;
    // Geometry: a viewBox or an explicit width/height on the root tag.
    let root_tag_end = text[svg_start..]
        .find('>')
        .map(|i| svg_start + i)
        .unwrap_or(text.len());
    let root = &text[svg_start..root_tag_end];
    let has_geometry =
        root.contains("viewBox") || (root.contains("width") && root.contains("height"));
    if !has_geometry {
        return Err("<svg> has neither viewBox nor width/height".to_string());
    }

    balanced_tags(text)?;
    Ok(())
}

/// Verify element tags nest and close correctly. Skips comments, CDATA,
/// processing instructions, doctype, and quoted attribute values so that path
/// data (`d="...>..."`) does not confuse the scanner.
fn balanced_tags(text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut stack: Vec<String> = Vec::new();

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        // Skip comments / CDATA / doctype.
        if text[i..].starts_with("<!--") {
            i = text[i..]
                .find("-->")
                .map(|j| i + j + 3)
                .ok_or("unterminated comment")?;
            continue;
        }
        if text[i..].starts_with("<![CDATA[") {
            i = text[i..]
                .find("]]>")
                .map(|j| i + j + 3)
                .ok_or("unterminated CDATA")?;
            continue;
        }
        if text[i..].starts_with("<!") || text[i..].starts_with("<?") {
            i = text[i..]
                .find('>')
                .map(|j| i + j + 1)
                .ok_or("unterminated declaration")?;
            continue;
        }

        // Find the matching '>' for this tag, ignoring quoted regions.
        let mut j = i + 1;
        let mut quote: Option<u8> = None;
        while j < bytes.len() {
            let c = bytes[j];
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == b'"' || c == b'\'' => quote = Some(c),
                None if c == b'>' => break,
                None => {}
            }
            j += 1;
        }
        if j >= bytes.len() {
            return Err("unterminated tag".to_string());
        }

        let inner = &text[i + 1..j];
        i = j + 1;

        if let Some(close) = inner.strip_prefix('/') {
            let name = tag_name(close);
            match stack.pop() {
                Some(open) if open == name => {}
                Some(open) => return Err(format!("closing </{name}> does not match <{open}>")),
                None => return Err(format!("stray closing tag </{name}>")),
            }
        } else if !inner.ends_with('/') {
            stack.push(tag_name(inner));
        }
        // self-closing (`.../>`) pushes nothing.
    }

    if stack.is_empty() {
        Ok(())
    } else {
        Err(format!("unclosed tag <{}>", stack.last().unwrap()))
    }
}

fn tag_name(s: &str) -> String {
    s.trim()
        .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or("")
        .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_minimal_symbolic_svg() {
        let svg = br#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M1 1 L15 15"/></svg>"#;
        assert!(validate_svg(svg).is_ok());
    }

    #[test]
    fn rejects_missing_geometry() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0"/></svg>"#;
        assert!(validate_svg(svg).is_err());
    }

    #[test]
    fn rejects_unbalanced_tags() {
        let svg = br#"<svg viewBox="0 0 16 16"><g><path d="M0 0"/></svg>"#;
        assert!(validate_svg(svg).is_err());
    }

    #[test]
    fn quoted_gt_does_not_break_scanner() {
        let svg = br#"<svg viewBox="0 0 16 16"><path d="M0 0 C 4>2 thing"/></svg>"#;
        assert!(validate_svg(svg).is_ok());
    }

    #[test]
    fn rejects_non_svg() {
        assert!(validate_svg(b"not an svg at all").is_err());
    }
}
