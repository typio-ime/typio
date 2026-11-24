//! Cargo xtask helpers for typio.
//!
//! Run with `cargo xtask <command>`.
//!
//! Commands:
//!   install          Install binaries, systemd user service, icons, and
//!                    example configs.
//!   install --dry-run
//!   uninstall        Remove installed files.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "xtask", about = "Typio build helper tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install typio system-wide or to a custom prefix.
    Install {
        /// Install prefix (default: /usr/local).
        #[arg(long, default_value = "/usr/local")]
        prefix: PathBuf,
        /// Stage files under DESTDIR while preserving the runtime prefix.
        #[arg(long)]
        destdir: Option<PathBuf>,
        /// Do not write anything; just print what would happen.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove installed typio files from a prefix.
    Uninstall {
        /// Install prefix (default: /usr/local).
        #[arg(long, default_value = "/usr/local")]
        prefix: PathBuf,
        /// Optional staging root matching `install --destdir`.
        #[arg(long)]
        destdir: Option<PathBuf>,
        /// Do not remove anything; just print what would happen.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Install {
            prefix,
            destdir,
            dry_run,
        } => install(&prefix, destdir.as_deref(), dry_run),
        Command::Uninstall {
            prefix,
            destdir,
            dry_run,
        } => uninstall(&prefix, destdir.as_deref(), dry_run),
    }
}

struct InstallPlan {
    copies: Vec<(PathBuf, PathBuf)>, // (source, dest)
    dirs: Vec<PathBuf>,
    service_dst: PathBuf,
}

fn plan_install(prefix: &Path) -> Result<InstallPlan> {
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "release".to_string());
    let target_dir = project_root().join("target").join(profile);
    plan_install_with_roots(prefix, &project_root(), &target_dir)
}

fn plan_install_with_roots(
    prefix: &Path,
    project_root: &Path,
    target_dir: &Path,
) -> Result<InstallPlan> {
    let bindir = prefix.join("bin");
    let libdir = prefix.join("lib");
    let datadir = prefix.join("share");
    let systemd_user_dir = libdir.join("systemd/user");
    let icons_dst = datadir.join("icons");
    let data_dst = datadir.join("typio");
    let applications_dst = datadir.join("applications");
    let metainfo_dst = datadir.join("metainfo");

    let mut copies = Vec::new();
    let dirs = vec![
        bindir.clone(),
        systemd_user_dir.clone(),
        icons_dst.clone(),
        data_dst.clone(),
        applications_dst.clone(),
        metainfo_dst.clone(),
    ];

    // Binaries.
    copies.push((target_dir.join("typio"), bindir.join("typio")));
    copies.push((target_dir.join("typioctl"), bindir.join("typioctl")));
    copies.push((
        target_dir.join("typio-settings"),
        bindir.join("typio-settings"),
    ));

    // Graphical settings launcher and AppStream metadata.
    copies.push((
        project_root.join("data/applications/io.typio.Settings.desktop"),
        applications_dst.join("io.typio.Settings.desktop"),
    ));
    copies.push((
        project_root.join("data/metainfo/io.typio.Settings.metainfo.xml"),
        metainfo_dst.join("io.typio.Settings.metainfo.xml"),
    ));

    // Icons. Source is the icon-theme *root* (`data/icons/`), so the walk
    // yields `hicolor/scalable/apps/foo.svg` as the relative path. That lands
    // the files at `<prefix>/share/icons/hicolor/scalable/apps/foo.svg` — the
    // layout the freedesktop icon-theme spec requires for Gtk/Qt to resolve
    // the "hicolor" default theme.
    let icons_src = project_root.join("data/icons");
    if icons_src.is_dir() {
        for entry in WalkDir::new(&icons_src).min_depth(1) {
            let entry = entry?;
            let src = entry.path();
            if src.is_file() {
                let rel = src.strip_prefix(&icons_src)?;
                let dst = icons_dst.join(rel);
                copies.push((src.to_path_buf(), dst));
            }
        }
    }

    // Example configs.
    for name in ["core.toml.example", "platform.toml.example"] {
        let src = project_root.join("data").join(name);
        if src.is_file() {
            copies.push((src, data_dst.join(name)));
        }
    }

    let service_dst = systemd_user_dir.join("typio.service");
    Ok(InstallPlan {
        copies,
        dirs,
        service_dst,
    })
}

fn install(prefix: &Path, destdir: Option<&Path>, dry_run: bool) -> Result<()> {
    let plan = plan_install(prefix)?;
    install_with_plan(prefix, destdir, dry_run, &project_root(), &plan)
}

fn install_with_plan(
    prefix: &Path,
    destdir: Option<&Path>,
    dry_run: bool,
    project_root: &Path,
    plan: &InstallPlan,
) -> Result<()> {
    // Render service file content separately because it is not a direct copy.
    let service_template = project_root.join("data/typio.service.in");
    let service_text = fs::read_to_string(&service_template)
        .with_context(|| format!("reading service template {}", service_template.display()))?;
    let host_dir = prefix.join("bin").to_string_lossy().into_owned();
    let service_rendered = service_text.replace("@TYPIO_HOST_DIR@", &host_dir);

    // Fail fast before mutating the destination tree if any source file is missing.
    if !dry_run {
        let missing: Vec<&Path> = plan
            .copies
            .iter()
            .map(|(src, _)| src.as_path())
            .filter(|src| !src.is_file())
            .collect();
        if !missing.is_empty() {
            let list = missing
                .iter()
                .map(|p| format!("  - {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n");
            anyhow::bail!(
                "missing build artifacts or data files:\n{list}\n\
                 Build the workspace first (e.g. `cargo build --release --workspace`)."
            );
        }
    }

    // Create directories.
    for dir in &plan.dirs {
        let staged = staged_path(destdir, dir);
        if dry_run {
            println!("mkdir -p {}", staged.display());
        } else {
            fs::create_dir_all(&staged)
                .with_context(|| format!("creating dir {}", staged.display()))?;
        }
    }

    // Copy files.
    for (src, dst) in &plan.copies {
        let staged = staged_path(destdir, dst);
        if dry_run {
            println!("cp {} {}", src.display(), staged.display());
        } else {
            if let Some(parent) = staged.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating dir {}", parent.display()))?;
            }
            fs::copy(src, &staged)
                .with_context(|| format!("copying {} to {}", src.display(), staged.display()))?;
        }
    }

    let staged_service_dst = staged_path(destdir, &plan.service_dst);
    if dry_run {
        println!(
            "write {} -> {}",
            service_template.display(),
            staged_service_dst.display()
        );
    } else {
        if let Some(parent) = staged_service_dst.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating dir {}", parent.display()))?;
        }
        fs::write(&staged_service_dst, service_rendered)
            .with_context(|| format!("writing {}", staged_service_dst.display()))?;
    }

    if dry_run {
        println!("Dry run complete. No files were written.");
    } else {
        match destdir {
            Some(root) => println!(
                "Staged typio under {} with runtime prefix {}",
                root.display(),
                prefix.display()
            ),
            None => println!("Installed typio to {}", prefix.display()),
        }
    }
    Ok(())
}

fn staged_path(destdir: Option<&Path>, path: &Path) -> PathBuf {
    match destdir {
        Some(root) => {
            let rel = path.strip_prefix("/").unwrap_or(path);
            root.join(rel)
        }
        None => path.to_path_buf(),
    }
}

fn uninstall(prefix: &Path, destdir: Option<&Path>, dry_run: bool) -> Result<()> {
    let plan = plan_install(prefix)?;
    uninstall_with_plan(prefix, destdir, dry_run, &plan)
}

fn uninstall_with_plan(
    prefix: &Path,
    destdir: Option<&Path>,
    dry_run: bool,
    plan: &InstallPlan,
) -> Result<()> {
    let icons_root = staged_path(destdir, &prefix.join("share/icons"));
    let data_dst = staged_path(destdir, &prefix.join("share/typio"));

    // Remove installed files (never remove_dir_all on shared directories like
    // share/icons/hicolor, which belong to all applications on the system).
    for (_, dst) in &plan.copies {
        let staged = staged_path(destdir, dst);
        if staged.exists() {
            if dry_run {
                println!("rm {}", staged.display());
            } else {
                fs::remove_file(&staged)
                    .with_context(|| format!("removing {}", staged.display()))?;
                if staged.starts_with(&icons_root) {
                    prune_empty_parents(staged.parent(), &icons_root);
                }
            }
        }
    }

    // Remove the rendered systemd user unit.
    let staged_service = staged_path(destdir, &plan.service_dst);
    if staged_service.exists() {
        if dry_run {
            println!("rm {}", staged_service.display());
        } else {
            fs::remove_file(&staged_service)
                .with_context(|| format!("removing {}", staged_service.display()))?;
        }
    }

    // Prune typio-owned data directory if now empty.
    if data_dst.is_dir() && !dry_run {
        let _ = fs::remove_dir(&data_dst);
    }

    if dry_run {
        println!("Dry run complete. No files were removed.");
    } else {
        println!("Uninstalled typio from {}", prefix.display());
    }
    Ok(())
}

/// Walk upward from `dir` removing empty directories, stopping before `stop_at`.
fn prune_empty_parents(mut dir: Option<&Path>, stop_at: &Path) {
    while let Some(current) = dir {
        if current == stop_at || !current.starts_with(stop_at) {
            break;
        }
        if fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}

fn project_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDirGuard(PathBuf);

    impl TempDirGuard {
        fn new(label: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "typio-xtask-test-{}-{}-{id}",
                std::process::id(),
                label
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn install_fails_fast_when_binaries_are_missing() {
        let fake_target = TempDirGuard::new("missing-bin-target");
        let staging = TempDirGuard::new("missing-bin-staging");
        let prefix = Path::new("/usr/local");
        let plan = plan_install_with_roots(prefix, &project_root(), &fake_target.0).unwrap();

        let err =
            install_with_plan(prefix, Some(&staging.0), false, &project_root(), &plan).unwrap_err();
        assert!(
            err.to_string().contains("missing build artifacts"),
            "unexpected error: {err}"
        );
        // Destination tree must remain untouched.
        assert!(!staging.0.join("usr/local/bin").exists());
    }

    #[test]
    fn install_and_uninstall_preserve_foreign_icons_and_remove_service() {
        let fake_target = TempDirGuard::new("roundtrip-target");
        let staging = TempDirGuard::new("roundtrip-staging");
        for bin in ["typio", "typioctl", "typio-settings"] {
            fs::write(fake_target.0.join(bin), b"stub-binary").unwrap();
        }

        let prefix = Path::new("/usr");
        let plan = plan_install_with_roots(prefix, &project_root(), &fake_target.0).unwrap();

        // Pre-populate a foreign application's icon inside the shared hicolor tree.
        let foreign_icon = staging
            .0
            .join("usr/share/icons/hicolor/scalable/apps/other-app.svg");
        fs::create_dir_all(foreign_icon.parent().unwrap()).unwrap();
        fs::write(&foreign_icon, b"<svg/>").unwrap();

        install_with_plan(prefix, Some(&staging.0), false, &project_root(), &plan).unwrap();

        let service_file = staging.0.join("usr/lib/systemd/user/typio.service");
        assert!(service_file.is_file(), "typio.service should be installed");
        let service_content = fs::read_to_string(&service_file).unwrap();
        assert!(
            service_content.contains("ExecStart=/usr/bin/typio"),
            "service should render runtime prefix"
        );
        assert!(staging.0.join("usr/bin/typio").is_file());
        assert!(staging.0.join("usr/bin/typioctl").is_file());
        assert!(staging.0.join("usr/bin/typio-settings").is_file());

        uninstall_with_plan(prefix, Some(&staging.0), false, &plan).unwrap();

        assert!(!service_file.exists(), "typio.service must be removed");
        assert!(!staging.0.join("usr/bin/typio").exists());
        assert!(!staging.0.join("usr/bin/typioctl").exists());
        assert!(!staging.0.join("usr/bin/typio-settings").exists());
        assert!(
            foreign_icon.is_file(),
            "uninstall must NEVER delete foreign icons in share/icons/hicolor"
        );
    }
}
