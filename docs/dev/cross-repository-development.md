# Typio and Optics Cross-Repository Development

Use one long-lived linked Git worktree for Typio and Optics development. The
primary Typio worktree remains on `main` with the canonical remote dependency
graph. The development worktree normally remains on the long-lived local
`dev` branch and resolves the live sibling Optics sources through Cargo
`[patch]`.

## Dependency Modes

| Concern | Primary worktree | Typio development worktree |
|---------|------------------|-----------------------------|
| Rust bindings | Tagged Optics Git source | Sibling `../optics` paths through `[patch]` |
| Native libraries | `pkg-config` / system or build-release | Sibling `../optics/build-release` |
| `Cargo.lock` | Remote Git commits; tracked in Git | Local path dependencies; untracked / ignored by hook |
| Configuration | Default or `.cargo/config.example.toml` | `.cargo/config.toml` copied from `.cargo/optics-local.toml` |

## Initial Setup

Run these commands once from the repository root:

### Step 1: Create the Linked Development Worktree

```bash
git branch dev main
git worktree add ../typio-dev dev
```

### Step 2: Configure Local Optics Mode

Enter the development worktree and activate the local patch file:

```bash
cd ../typio-dev
cp .cargo/optics-local.toml .cargo/config.toml
git config core.hooksPath .githooks
```

### Step 3: Verify the Local Dependency Graph

Verify that the local worktree resolves the sibling repository:

```bash
cargo tree -i flux-sys
cargo tree -i iris
cargo tree -i lens-sys
```

These trees must show paths below the sibling `optics` checkout.

## Daily Development

Compile Optics before building a Typio consumer:

```bash
meson compile -C ../optics/build-release
cargo check -p typio-host
cargo test --workspace
```

Keep `.cargo/config.toml` in place while working in `../typio-dev`. The local
`Cargo.lock` will resolve local file paths; the repository hook will prevent
accidental commits of that local lockfile.

## Promote an Optics Release

When Optics changes are merged and tagged upstream, update the primary worktree
to the new canonical tag.

### Step 1: Prepare the Tagged Optics Release

In `../optics`:

```bash
git checkout main
git pull
git describe --tags --exact-match
```

### Step 2: Update Typio Dependencies

In `../typio` (primary worktree on `main`):

1. Update the `tag = "vX.Y.Z"` values in `Cargo.toml` under `[workspace.dependencies]`.
2. Update `OPTICS_PINNED_REF` in `.github/workflows/ci.yml`.
3. Verify the tag with the helper script:

```bash
./scripts/optics-release-ref.sh
```

### Step 3: Regenerate Canonical Lockfile

Ensure `.cargo/config.toml` does **not** contain `[patch]` in the primary
worktree, then update the lockfile:

```bash
cargo update -p flux-sys -p flux-text-sys -p iris -p iris-sys -p lens-sys
cargo check --locked --workspace
cargo test --locked --workspace
```

### Step 4: Rebase Development Worktree

After merging on `main`, fast-forward `dev` in the development worktree:

```bash
cd ../typio-dev
git fetch origin
git rebase origin/main
```

## Troubleshooting

### Pre-commit hook rejects `Cargo.lock`

```text
Local Optics mode: refusing to commit Cargo.lock.
```

This error indicates `.cargo/config.toml` contains `[patch]` entries and the
staged `Cargo.lock` reflects local checkout paths. Unstage the lockfile:

```bash
git restore --staged Cargo.lock
```

If you intended to bump canonical dependencies, perform the release in the
primary worktree without the `[patch]` table.

### Undefined symbols from `libflux.so` or `libflux_text.so`

Ensure the sibling native libraries are compiled:

```bash
meson compile -C ../optics/build-release
```

Ensure `FLUX_BUILD_DIR` points to the `build-release` directory.
