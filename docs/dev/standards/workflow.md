# Engineering Workflow & Verification Gates

This document defines the verification gates, pre-flight checks, promotion protocols, and release procedures for Typio.

---

## 1. Pre-Flight Verification

Before authoring code or committing changes, run the baseline pre-flight checks:

```bash
# Recent commit message style; match it.
git --no-pager log --oneline -15

# Tag type the project uses.
git cat-file -t "$(git describe --abbrev=0)"

# Current host version source.
grep -n '^version =' crates/typio-daemon/Cargo.toml | head -1

# Pending CHANGELOG entries.
sed -n '/^## \[Unreleased\]/,/^## \[/p' CHANGELOG.md
```

---

## 2. Mandatory Verification Gates

Every pull request and promotion must pass all four local verification gates:

```bash
# 1. Documentation invariants and relative link integrity
tools/check-docs.sh

# 2. Rust formatting
cargo fmt -- --check

# 3. Compiler warnings and Clippy lints
cargo clippy --workspace --all-targets -- -D warnings

# 4. Workspace unit and integration tests
cargo test --workspace
```

---

## 3. Cross-Repository Optics Workflow

Typio depends on the companion C engine **Optics** for UI and rendering:

- **Primary Worktree (`main`)**: Uses canonical Optics Git dependencies pinned to release tags in `Cargo.toml`. Every commit on `main` must build independently with `cargo check --locked --workspace`.
- **Development Worktree (`dev`)**: Uses local path overrides from `.cargo/optics-local.toml` copied to `.cargo/config.toml`. Uncommitted lockfiles remain strictly isolated to the worktree via `.githooks/pre-commit`.

Detailed worktree setup and promotion protocols are documented in [`docs/dev/optics-dev-worktree.md`](../optics-dev-worktree.md).

---

## 4. Code-Doc Synchronization

A change to runtime behavior, configuration schemas, CLI flags, or public interfaces must update:
1. The matching architectural blueprint in `docs/architecture/`
2. The matching Diátaxis guide in `docs/` (`how-to/`, `reference/`, or `explanation/`)
3. `CHANGELOG.md` under `## [Unreleased]`

in the exact same pull request.

---

## 5. Bisectability & Release Hygiene

1. **Atomic Commits**: Every commit promoted to `main` must be individually compileable and testable.
2. **Pinned Upstream Tags**: Never commit references to unreleased, local-only, or detached branches in companion repositories. Upstream releases precede downstream promotions.
