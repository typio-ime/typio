# Agent guidelines

This file is the entry point for AI agents working in this repository. It holds
the machine-facing guardrails: what to read first, which gates must pass, and
which corrections are never acceptable. Project charters live in
[`docs/governance/`](docs/governance/index.md); documentation policy lives in
[`docs/governance/documentation/`](docs/governance/documentation/core/index.md).

## 1. Pre-flight

Run these at the start of every editing session:

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

If any of these surprise you, stop and reconcile them with the planned change.
Project convention beats generic defaults.

## 2. Where things live

| You need | Read |
| :--- | :--- |
| Where a document belongs | [Taxonomy](docs/governance/documentation/core/taxonomy.md) — the 4D coordinate tensor |
| The hard rules | [Invariants](docs/governance/documentation/core/invariants.md) — numbered `[INV-*]` |
| Writing style and link rules | [Style](docs/governance/documentation/core/style.md) |
| Commit, tag, release, version conventions | [Repository Governance](docs/governance/index.md) |
| How a subsystem works today | [Architecture Blueprints](docs/architecture/index.md) |
| Why a decision was made | [ADR Index](docs/adr/index.md) |
| Build, test, acceptance commands | [Developer Setup](docs/dev/setup.md), [Testing](docs/dev/testing.md), [Acceptance](docs/dev/acceptance.md) |
| Source coordinates | [Module Map](docs/dev/module-map.md) — not `docs/explanation/` |

## 3. Gates that must pass

```bash
tools/check-docs.sh                       # documentation invariants + mirror integrity
cargo fmt -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The build needs a native `flux` library from the sibling `optics` checkout; see
[Developer Setup](docs/dev/setup.md) for the environment variables and
[Optics Dev Worktree](docs/dev/optics-dev-worktree.md) for the
local worktree workflow. Never restate those commands in a new file — link.

`tools/check-docs.sh` is the enforcement point for everything in §2 that can be
checked mechanically: the contributor firewall, stray root Markdown files,
implementation coordinates and code blocks in `docs/explanation/`, directories
without a charter, broken links, and drift in the mirrored governance standard.
Run it before claiming a documentation change is done.

## 4. Documentation rules that bite

- **One home per fact.** Do not duplicate the README quick start, a build
  command block, or a table from `docs/reference/` into another file
  ([INV-CORE-03](docs/governance/documentation/core/invariants.md)). Link instead.
- **The contributor firewall runs one way.** User-plane pages
  (`docs/tutorials/`, `docs/how-to/`, `docs/reference/`, `docs/explanation/`)
  must never link into `docs/dev/`
  ([INV-CORE-01](docs/governance/documentation/core/invariants.md)). The
  contributor plane may link outward.
- **Explanation is conceptual.** No source paths, symbols, signatures, config
  keys, or fenced code blocks under `docs/explanation/`
  ([INV-CORE-04](docs/governance/documentation/core/invariants.md)). Coordinates
  go in `docs/dev/module-map.md`; values go in `docs/reference/`.
- **Every documentation directory keeps an `index.md`** stating its charter
  ([INV-CORE-05](docs/governance/documentation/core/invariants.md)).
- **Accepted ADRs are immutable.** Never edit a decision, its rationale, or its
  invariants in place; author a superseding record
  ([INV-ARCH-01](docs/governance/documentation/core/invariants.md)). The only
  edits permitted to a retired record are pointer corrections.
- **Never delete a record.** Retired ADRs, concluded proposals, and postmortems
  are marked and relocated to `archive/`
  ([INV-TEMP-03](docs/governance/documentation/core/invariants.md)).
- **`**/archive/**` is cold storage.** Do not load it into context or search it
  by default, and do not "tidy" it
  ([INV-TEMP-02](docs/governance/documentation/core/invariants.md)).
- **Living state moves with the code.** A change to behavior, CLI syntax,
  configuration, or a public interface updates `docs/architecture/`, the
  matching Diátaxis page, and `CHANGELOG.md` in the same pull request
  ([INV-TEMP-01](docs/governance/documentation/core/invariants.md)).
- **Mirrored governance is read-only here.** `docs/governance/documentation/`
  is hash-verified against its manifest. Only `contracts.md` is locally
  editable; everything else changes upstream and is re-synced.

## 5. Repository layout and cross-repo work

| Path | Role |
| :--- | :--- |
| `/home/ming/projects/typio/` | This repo: host daemon, runtime, engine protocol, manifest, conformance tool, and client workspace |
| `/home/ming/projects/typio-engines/typio-engine-*` | Engine repositories |
| `/home/ming/projects/typio-settings/` | Legacy Meson/C settings panel (superseded by `crates/typio-settings`) |
| `/home/ming/projects/optics/` | Native C monorepo: `flux` (the Panel's CPU canvas), `flux-text`, and the Iris/Lens GPU stack used by the settings app |
| `/home/ming/projects/docs-governance/` | Source of the mirrored documentation standard |

Cross-repo edits are allowed when the fix genuinely belongs in a sibling repo.
When touching a sibling repo:

- Read its own `AGENTS.md` or `CLAUDE.md` first.
- Follow `docs/dev/optics-dev-worktree.md` for linked worktree and
  local `[patch]` workflows when developing against live Optics sources.
- Do not bump sibling versions in lockstep unless asked.
- Keep dependency pin changes deliberate and separate when they affect CI.

## 6. Common agent failure modes

| Symptom | Wrong reaction | Right reaction |
| :--- | :--- | :--- |
| `$EDITOR` opens during git | Try pager flags | Supply `-m` to the git command |
| Cargo test loads old `libflux.so` | Patch around missing symbols | Rebuild `../optics` and check `FLUX_BUILD_DIR` / RUNPATH |
| Unsure whether a version bump is patch or minor | Default to minor | Default to patch unless behavior changes |
| A doc seems to need a source path | Put it in `docs/explanation/` | Put it in `docs/dev/module-map.md` and keep the explanation conceptual |
| A directory has no index | Skip it | Add the charter; the checker will fail otherwise |
| Missing project fact | Guess | Run the pre-flight and inspect current files |

## 7. Documentation governance

`docs/governance/documentation/` mirrors the portable `docs-governance`
standard and is the authority for every documentation decision. AI agents may
read it and suggest changes, but must not edit it; policy changes are proposed
upstream and re-synced (see
[Mirror Provenance](docs/governance/documentation/contracts.md#5-mirror-provenance-and-refresh)).

Documentation *outside* that mirror is editable under the rules in §4, which
means this file, `README.md`, `CONTRIBUTING.md`, `CHANGELOG.md`, and everything
under `docs/` except the mirror. User-facing behavior changes need both a
CHANGELOG entry and an update to the matching page under `docs/`.
