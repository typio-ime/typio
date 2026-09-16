# Optics Development Worktree Workflow

This guide establishes the standard workflow for developing Typio alongside
its companion C rendering and material engine, **Optics**. It uses a dual
Git worktree architecture to combine zero-friction local iteration on the
development branch with strict, reproducible builds on `main`.

## Architecture and Invariants

Cross-repository development splits responsibilities across two linked Git
worktrees sharing a single repository storage:

| Concern | Primary worktree (`main`) | Development worktree (`dev`) |
|---|---|---|
| Directory path | `../typio/` | `../typio-dev/` |
| Active branch | `main` | `dev` |
| Rust bindings | Tagged Optics Git source | Sibling `../optics` via `[patch]` |
| `Cargo.lock` | Canonical, tagged, committed | Worktree-local, never committed |
| Cargo config | `.cargo/config.toml` absent | Copied from `.cargo/optics-local.toml` |
| Native libraries | System-installed `/usr/lib/lib*.so` | Sibling `../optics/build-release/` via `-rpath` |
| Target cache | Primary `target/` | Worktree-local `target/` |

### The Foundational Principle: Truth on Main vs Local Illusion

A feature compiling and passing tests in `typio-dev` is often a **local
illusion**: it succeeds only because Cargo and the runtime secretly link
against the privileged, uncommitted state in your sibling `../optics/build-release`
directory.

The foundational principle of this dual-worktree architecture is:
> **`main` represents external reality. It must genuinely build and run in
> the wild for users, CI, and package managers—without depending on any
> developer's private machine state.**

Every rule in this workflow (Optics Upstream First, canonical manifest
updates, and locked lockfile validation) exists solely to prevent local
development privilege from masquerading as a functional release on `main`.

### Core Invariants

1. **Local Patch Containment**: `.cargo/config.toml` and path-resolved
   `Cargo.lock` belong strictly to the `typio-dev` worktree. The tracked
   pre-commit hook automatically excludes them from regular commits.
2. **Optics Upstream First**: Any feature merged into `main` that relies on
   new or modified Optics APIs must depend solely on a tagged, public
   Optics commit. `main` must never reference uncommitted or local-only
   Optics revisions.
3. **Always-Buildable Main**: Every commit on `main` must be individually
   buildable with `cargo check --locked --workspace`. Canonical lockfiles
   are updated alongside feature promotions, never left to guess at release.

Separate target directories are required. Do not configure a shared
`CARGO_TARGET_DIR` across worktrees, as mixing incremental compilation
artifacts between canonical Git dependencies and local path overrides leads
to cache invalidation churn.

## Workspace Setup

### Directory Topology

Place all repositories under a common parent directory:

```text
projects/
├── typio/       # Primary worktree on main (canonical)
├── typio-dev/   # Linked development worktree on dev (local patch)
└── optics/      # Sibling checkout of the Optics C engine
```

### 1. Create the Linked Worktree

Run from the primary `typio` repository (`../typio/`, branch `main`):

```bash
# In projects/typio/ (branch main):
git worktree add -b dev ../typio-dev main
```

### 2. Activate Local Optics Mode

Enter the development worktree, copy the local patch configuration, and
install the repository-owned Git hooks:

```bash
# In projects/typio-dev/ (branch dev):
cd ../typio-dev
cp .cargo/optics-local.toml .cargo/config.toml
git config core.hooksPath .githooks
```

Verify that the sibling Optics repository and Meson build exist:

```bash
# In projects/typio-dev/ (branch dev):
test -f ../optics/meson.build
meson compile -C ../optics/build-release
```

#### Understanding `.cargo/optics-local.toml`

The repository tracks `.cargo/optics-local.toml` as the reviewed template for
local development. Because `.cargo/config.toml` is ignored by Git to keep
worktree state isolated, copying this template activates source-replacement
without dirtying version control:

```toml
[patch."https://github.com/ming2k/optics"]
flux = { path = "../optics/bindings/flux-rs/crates/flux" }
flux-sys = { path = "../optics/bindings/flux-rs/crates/flux-sys" }
flux-text-sys = { path = "../optics/bindings/flux-rs/crates/flux-text-sys" }
lens = { path = "../optics/bindings/lens-rs/crates/lens" }
lens-sys = { path = "../optics/bindings/lens-rs/crates/lens-sys" }
iris = { path = "../optics/bindings/iris-rs/crates/iris" }
iris-sys = { path = "../optics/bindings/iris-rs/crates/iris-sys" }
```

This configuration achieves two things:

1. **Cargo Source Replacement**: It instructs Cargo to intercept every
   dependency on the remote `ming2k/optics` repository and redirect it to
   the local sibling directory `../optics/`.
2. **Native Discovery and Precedence**: Loading `*-sys` crates locally
   causes their `build.rs` scripts to run from disk. These scripts detect
   `../optics/build-release/meson-uninstalled/`, prepend it to `PKG_CONFIG_PATH`,
   and inject `-Wl,-rpath` so that uninstalled local C libraries take
   absolute precedence over any system-installed versions.

### 3. Resolve the Local Dependency Graph

Run an initial check in `typio-dev` to populate the local worktree
lockfile with path dependencies:

```bash
# In projects/typio-dev/ (branch dev):
cargo check -p typio-daemon
```

Verify that Cargo resolves the sibling paths instead of Git tags:

```bash
# In projects/typio-dev/ (branch dev):
cargo tree -i flux-sys
cargo tree -i iris
```

Both trees must display paths pointing into `../optics/bindings/`.

## Daily Development in Local Mode

> **Active Worktree**: All operations in this section execute inside the
> development worktree (`../typio-dev/`, branch `dev`), interacting with
> the sibling directory `../optics/`.
>
> Do not run these daily development commands in the primary `typio/`
> (`main`) worktree.

### Compiling and Testing

When modifying Optics and Typio simultaneously:

1. Recompile the native C libraries whenever Optics C sources change:
   ```bash
   # From either directory, build the shared Meson tree:
   meson compile -C ../optics/build-release
   ```
2. Build and test Typio in the development worktree:
   ```bash
   # In projects/typio-dev/ (branch dev):
   cargo check -p typio-daemon
   cargo test --workspace
   ```

Do not run concurrent Meson or Ninja builds against `../optics/build-release` from
multiple terminals, as the build output directory is a shared write location.

### Committing Typio Changes

Stage and commit changes on `dev` as usual:

```bash
# In projects/typio-dev/ (branch dev):
git add .
git commit -m "fix(host): preserve focus across surface transitions"
```

The tracked `.githooks/pre-commit` hook automatically unstages:
- `Cargo.lock` (mutated by the local patch); and
- `.cargo/config.toml` (if accidentally staged).

The commit proceeds with only your clean Typio source changes. Never bypass this
guard with `--no-verify`.

## Feature-Level Merge to Main

Promote changes from `dev` to `main` at **feature-level boundaries** rather
than accumulating a monolithic, unreviewable multi-month release dump.

### Relationship Between `dev` and `main` Commit Histories

The commit history on `dev` and `main` is **deliberately distinct**:

- **`dev` history**: Contains rapid, exploratory, and fine-grained iteration
  commits created during co-development with local Optics. These commits were
  built against the local path-patched lockfile, so they are **not**
  individually buildable in canonical mode.
- **`main` history**: Contains curated, atomic feature commits that are
  guaranteed to build independently with `cargo check --locked --workspace`,
  pinned to canonical remote Git tags.

Because intermediate `dev` commits cannot build canonically, they must never
be merged onto `main` verbatim. Promotion is always a **squash merge**: the
entire feature becomes one atomic commit on `main`, and `dev` is then reset
onto the promoted `main`. The fine-grained history survives through dated
archive tags (Step 5), not through `main`'s graph.

Do not treat this process as a bidirectional "sync". It is a strict
**one-way promotion and merge** of completed feature milestones from `dev`
into `main`.

Follow this protocol whenever a completed feature on `dev` depends on new or
updated Optics functionality:

### Step 1: Upstream Optics First

Land and tag the required changes in `../optics` before making Typio
canonical:

```bash
# In projects/optics/ (Optics repository, branch main):
cd ../optics
git checkout main
git pull
# Ensure the release tag exists and the build passes:
git tag -v vX.Y.Z || git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
meson compile -C build-release
```

### Step 2: Squash Merge the Feature into Main Worktree

Switch to the primary `typio/` worktree (`branch main`). Because `main`
deliberately lacks `.cargo/config.toml`, it operates natively in canonical
mode without disabling or toggling any local configurations.

Squash merge the feature so `main` receives exactly one atomic, buildable
commit:

```bash
# In projects/typio/ (primary worktree, branch main):
cd ../typio
git switch main
git pull --ff-only
git merge --squash dev
```

### Step 3: Update Manifests and Canonical Lockfile on Main

If the merged feature requires new Optics APIs, update every Optics dependency
in `projects/typio/Cargo.toml` to the new tag `vX.Y.Z`:

```bash
# In projects/typio/ (primary worktree, branch main):
./scripts/optics-release-ref.sh
```

Regenerate the canonical `Cargo.lock` directly in the primary worktree.
Because there are no local path overrides, Cargo connects to
GitHub and pins the authoritative remote Git SHA:

```bash
# In projects/typio/ (primary worktree, branch main):
cargo update -p flux-sys -p flux-text-sys -p iris -p iris-sys -p lens-sys
```

Confirm that `cargo tree -i flux-sys` and `cargo tree -i iris` report the tagged
Git source instead of local filesystem paths.

### Step 4: Validate and Commit Canonical State on Main

Verify that the canonical tree compiles cleanly under `--locked`:

```bash
# In projects/typio/ (primary worktree, branch main):
cargo check --locked --workspace
cargo test --locked --workspace
tools/check-docs.sh
```

Commit the canonical promotion directly on `main` and push:

```bash
# In projects/typio/ (primary worktree, branch main):
git add -A
git commit -m "feat(host): promote milestone from dev"
git push origin main
```

### Step 5: Archive the Dev History, Reset Dev, and Continue

Notice that **`typio-dev` never touched or disabled its `.cargo/config.toml`**.
Your local development environment remained active throughout the merge.

Before moving the `dev` pointer, preserve the fine-grained iteration history
with a dated archive tag:

```bash
# In projects/typio-dev/ (development worktree, branch dev):
git tag archive/dev-YYYYMMDD-<feature-slug> dev
git push origin archive/dev-YYYYMMDD-<feature-slug>   # optional
```

Then reset `dev` onto the freshly promoted `main`. Do **not** rebase here:
the dev content already lives inside the squash commit, so rebasing yields
empty commits and conflicts—`reset --hard main` is the only correct
operation:

```bash
# In projects/typio-dev/ (development worktree, branch dev):
git reset --hard main
```

`Cargo.lock` now holds the canonical git-tag state. Simply run any Cargo
command to rewrite it into the local path-patched form:

```bash
# In projects/typio-dev/ (development worktree, branch dev):
cargo check -p typio-daemon
cargo tree -i flux-sys   # confirm ../optics/bindings/ paths reappear
```

If `dev` was previously pushed, force-update the remote branch after the
reset:

```bash
# In projects/typio-dev/ (development worktree, branch dev):
git push --force-with-lease origin dev
```

## Automated Git Hook Guards (.githooks/pre-commit)

To minimize cognitive overhead and prevent human error, repository constraints
are codified into tracked Git hooks in `.githooks/`. Setting
`git config core.hooksPath .githooks` activates the repository-owned
guardrails across linked worktrees. The hook implementation is tracked
directly in `.githooks/pre-commit`.

### Dual-Mode Guard Behavior

The tracked `.githooks/pre-commit` hook dynamically detects the active
worktree mode and enforces corresponding constraints:

#### 1. In Local Optics Mode (`typio-dev/`)

- **Automatic Path Unstaging**: During regular feature commits, the hook
  silently unstages `Cargo.lock` (mutated by the local patch) and
  `.cargo/config.toml` (if force-staged):
  ```text
  Local Optics mode: excluded Cargo.lock from this commit.
  ```
- **Refusing Release-Shaped Commits**: If you attempt to commit a version bump
  (`+version = "..."` in `Cargo.toml`), the hook aborts the commit:
  ```text
  Local Optics mode: refusing a release-shaped commit.
  ```

#### 2. In Canonical Mode (`typio/`, `main`)

- **Lockfile Integrity Validation**: Whenever `Cargo.toml` is staged on `main`,
  the hook validates that the canonical lockfile is in sync with the remote Git
  tags before committing.

### Recovering Canonical State

If a worktree's lockfile is accidentally polluted by path entries, restore it
immediately:

```bash
# In projects/typio/ (main) or projects/typio-dev/ (dev):
rm -f .cargo/config.toml
git restore Cargo.lock
cargo check --locked --workspace
```

## Sibling Project Alignment

The same dual-worktree pattern, `.cargo/optics-local.toml` template, and Git
hook guards apply uniformly across companion repositories in the desktop stack:

- `tessera-dev`: Desktop panel and applet framework.
- `arca-dev`: Desktop file manager and chooser.
- `sigil-dev`: Secret service daemon and PAM provider.
- `aphrodite-dev`: Companion workspace.

All applications maintain clean canonical branches on `main` and use
worktree-local patch files during daily development.
