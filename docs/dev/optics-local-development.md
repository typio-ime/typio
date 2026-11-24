# Local Optics Development

This guide defines how to develop and integrate Typio against a local sibling
**Optics** checkout. To keep canonical builds isolated from local overrides,
the workflow uses a linked Git worktree: the primary worktree (`main`) stays
canonical and statically linked for production and CI, while the linked
worktree (`dev`) patches the sibling Optics facade and links shared libraries
for fast iteration.

## Architecture and Invariants

| Concern | Primary worktree (`main`) | Development worktree (`dev`) |
|---|---|---|
| Directory | `../typio/` | `../typio-dev/` |
| Branch | `main` | `dev` |
| `Cargo.toml` | Identical across both branches (`optics` facade with `features = ["static"]`) | Same |
| Rust bindings | Tagged `optics` facade from Git | Sibling `../optics/crates/optics` via `[patch]` |
| `Cargo.lock` | Canonical, tagged, committed | Worktree-local, never committed |
| Cargo config | `.cargo/config.toml` absent | Copied from [.cargo/optics-local.toml](../../.cargo/optics-local.toml) |
| Native linkage | Static `liboptics_*.a` archives (default `static` feature) | Shared `../optics/build/` via `[env] OPTICS_DYNAMIC = "1"` + `-rpath` |
| Target cache | Primary `target/` | Worktree-local `target/` |

### Core Invariants

1. **Identical Manifests, Worktree-Local Overrides**: `main` and `dev` never
   diverge on `Cargo.toml` link settings. `Cargo.toml` enables
   `features = ["static"]` on the `optics` facade so canonical and production
   builds embed Optics archives by default; only `typio-dev`'s untracked
   `.cargo/config.toml` overrides the mode with `OPTICS_DYNAMIC = "1"`.
2. **Optics Upstream First**: `main` must depend solely on an immutable,
   publicly tagged Optics release — never on local or uncommitted Optics state.
3. **Always-Buildable Main**: Every commit on `main` must build independently
   with `cargo check --locked --workspace`. Feature code, the Optics tag bump
   in `Cargo.toml`, and the canonical `Cargo.lock` land together in one atomic
   squash commit.
4. **Target Directory Isolation**: Keep separate `target/` directories per
   worktree; do not share `CARGO_TARGET_DIR` across canonical and patched trees.

## Workspace Setup

Place the repositories under a common parent directory:

```text
projects/
├── typio/       # Primary worktree on main (canonical, static Optics)
├── typio-dev/   # Linked worktree on dev (local patch, dynamic Optics)
└── optics/      # Sibling Optics checkout
```

1. **Create the linked worktree** from `projects/typio/`:
   ```bash
   git worktree add -b dev ../typio-dev main
   ```
2. **Enable local Optics mode and hooks** inside `projects/typio-dev/`:
   ```bash
   cd ../typio-dev
   cp .cargo/optics-local.toml .cargo/config.toml
   git config core.hooksPath .githooks
   ```
   [.cargo/optics-local.toml](../../.cargo/optics-local.toml) patches `optics`
   to `../optics/crates/optics` (whose path deps resolve the rest of
   `../optics/crates/*`) and sets `OPTICS_BUILD_DIR`, `OPTICS_SOURCE_DIR`, and
   `OPTICS_DYNAMIC = "1"` so local builds link `liboptics_*.so` from
   `../optics/build` via `-Wl,-rpath`.
3. **Build Optics and resolve the local graph**:
   ```bash
   meson setup ../optics/build ../optics -Dtests=false --buildtype=debugoptimized  # first time only
   meson compile -C ../optics/build
   cargo check -p typio-daemon
   cargo tree -i optics   # confirm path points to ../optics/crates/optics
   ```

## Daily Development (`typio-dev`)

Run all day-to-day commands inside `projects/typio-dev/`:

```bash
# Recompile Optics after C or shader edits (zero Rust relink when headers are unchanged):
meson compile -C ../optics/build

# Build and test Typio against the shared libraries in ../optics/build:
cargo check -p typio-daemon
cargo test --workspace

# Verify the production static closure inside dev on demand:
OPTICS_DYNAMIC=0 cargo build -p typio-daemon --bin typio
```

Do not run concurrent Meson/Ninja builds against `../optics/build` from
multiple terminals.

Commit on `dev` normally (`git add . && git commit ...`). Never bypass the
hook with `--no-verify` — `.githooks/pre-commit` automatically unstages the
path-patched `Cargo.lock` and `.cargo/config.toml` so local overrides never
pollute commits, rebases, or squash merges.

## Feature-Level Atomic Promotion to `main`

`dev` holds fine-grained WIP commits built against local Optics; `main` holds
curated, individually buildable atomic commits. Never fast-forward `main` onto
`dev`.

### 1. Publish the Optics Tag First

In `projects/optics/`:

```bash
cd ../optics
git checkout main && git pull --ff-only
git tag -v vX.Y.Z || git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
meson compile -C build
```

### 2. Squash, Re-pin, Verify, and Commit on `main`

In `projects/typio/` (primary worktree, no `.cargo/config.toml`):

```bash
cd ../typio
git pull --ff-only

# Squash dev changes into the staging area (do not commit yet):
git merge --squash dev

# Update the `optics` tag in Cargo.toml to vX.Y.Z, then regenerate Cargo.lock:
cargo update -p optics

# Confirm remote Git resolution (no local ../optics paths):
cargo tree -i optics
cargo tree -i optics-ui-sys

# Full locked static verification (omit OPTICS_* if matching Optics is installed system-wide):
OPTICS_SOURCE_DIR=../optics OPTICS_BUILD_DIR=../optics/build cargo check --locked --workspace
OPTICS_SOURCE_DIR=../optics OPTICS_BUILD_DIR=../optics/build cargo test --locked --workspace
tools/check-docs.sh

# Single atomic promotion commit:
git add -A
git commit -m "feat(<scope>): <summary>

Adopt Optics vX.Y.Z."
git push origin main
```

### 3. Reset `dev` onto `main`

Because `dev`'s changes now live inside the squash commit on `main`, reset
rather than rebase (`typio-dev/.cargo/config.toml` stays untouched):

```bash
cd ../typio-dev
git reset --hard main
cargo check -p typio-daemon   # regenerates the local patched Cargo.lock
```

If `dev` is tracked on a remote, update it with `git push --force-with-lease origin dev`.

## Git Hook Guard and Maintenance

`.githooks/pre-commit` switches behavior based on whether `.cargo/config.toml`
contains `[patch."https://github.com/ming2k/optics"]`:

- **Local Optics mode (`typio-dev/`)**: unstages `Cargo.lock` and
  `.cargo/config.toml` automatically, and rejects release-shaped version bumps
  in `Cargo.toml`.
- **Canonical mode (`typio/`)**: validates `cargo metadata --locked` whenever
  `Cargo.toml` is staged.

### Recovering Canonical State

If a worktree's `Cargo.lock` is accidentally polluted by local paths:

```bash
rm -f .cargo/config.toml
git restore Cargo.lock
cargo check --locked --workspace
```

## See Also

- [Developer Setup](setup.md) — prerequisites, build commands, and engine loading.
- [How to Package for Distribution](../how-to/package-for-distribution.md) — staging, install manifest, and runtime dependencies.
