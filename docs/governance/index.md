# Repository Governance

This directory holds Typio's own governance: the charters, gates, and release
process that apply to *this* repository only. Portable documentation policy —
the rules any repository could adopt — lives in the mirrored standard at
[docs/governance/documentation/](documentation/core/index.md) and must never be
edited here.

---

## What governs what

| Plane | Lives in | Authority |
| :--- | :--- | :--- |
| Documentation policy | [documentation/](documentation/core/index.md) | Mirrored `docs-governance` standard, hash-verified; changes come from upstream |
| Repository charters | This page and [contracts.md](documentation/contracts.md) | Typio maintainers |
| Contributor procedures | [docs/dev/](../dev/index.md) | Typio maintainers |
| Interface stability tiers | [Interface Stability](../reference/stability.md) | Typio maintainers |

Typio-specific rules deliberately stay out of the upstream standard. The
standard's intake filter (`spec/core/workflow.md`, Part 3) admits only rules
that apply to at least 90% of repositories; Rust formatting, the sibling
`optics` build, and the release process below are Tier 3 and belong here.

---

## Documentation gates

Every pull request that touches documentation passes three gates. They are
checks, not advice — the first two are mechanical.

### Gate A: Structure

New content routes through the [4D coordinate tensor](documentation/core/taxonomy.md)
(temperature x lifecycle x audience x cognitive mode). No arbitrary Markdown
files at the repository root ([INV-CORE-02](documentation/core/invariants.md)).
No user-facing page links into [docs/dev/](../dev/index.md)
([INV-CORE-01](documentation/core/invariants.md)).

### Gate B: Invariants

- Conceptual pages under [docs/explanation/](../explanation/index.md) carry no
  source paths, symbols, or code blocks ([INV-CORE-04](documentation/core/invariants.md)).
  Implementation coordinates live in [docs/dev/module-map.md](../dev/module-map.md).
- Every documentation directory keeps an `index.md` charter
  ([INV-CORE-05](documentation/core/invariants.md)).
- `Accepted` ADRs are immutable: change a decision by superseding it, never by
  editing it ([INV-ARCH-01](documentation/core/invariants.md)).
- New ADRs clear the [3-question significance test](documentation/profiles/architecture/adr.md);
  trivial decisions stay in the pull request description.
- Living state — [docs/architecture/](../architecture/index.md), the Diátaxis
  quadrants, and `README.md` — is updated in the same pull request as the code
  change it describes ([INV-TEMP-01](documentation/core/invariants.md)).

### Gate C: Mechanical verification

```bash
tools/check-docs.sh
```

The script verifies the mirror against its own manifest (the standard's
`verify.sh` behavior, vendored so CI needs no network) and then enforces the
invariants above, including relative-link integrity. Non-zero exit fails the
pull request. Archive tiers are exempt from formatting churn, not from link
integrity.

---

## Release Process

A release is two commits plus one annotated tag, in this order.

1. **Land substantive commits first.** Changelog entries accumulate under
   `## [Unreleased]` in [CHANGELOG.md](../../CHANGELOG.md).
2. **Release commit.** Bump `version` in
   [crates/typio-daemon/Cargo.toml](../../crates/typio-daemon/Cargo.toml),
   update `Cargo.lock`, and move the `## [Unreleased]` block to
   `## [X.Y.Z] - YYYY-MM-DD`. The commit subject is `release: vX.Y.Z`.
3. **Annotated tag.** `git tag -a vX.Y.Z -m "release: vX.Y.Z"`.
4. **Push `main` and the tag.**

The daemon package is the single version source. Do not add a second one, and
do not tag with `git tag vX.Y.Z` or with an empty message: every release tag in
this repository is annotated, and an editor must never open mid-release.

### Version bumps

| Bump | Applies to |
| :--- | :--- |
| **Patch** | Bug fixes and internal improvements with no user-visible behavior change |
| **Minor** | A user-visible feature or behavior change |
| **Major** | An incompatible change |

When the choice is unclear, it is a patch. Patch releases do not add a new empty
`## [Unreleased]` placeholder; the next feature commit recreates it.

### Changelog format

Keep a Changelog order: `### Added`, `### Changed`, `### Deprecated`,
`### Removed`, `### Fixed`, `### Security`. Every bullet starts with a bold
lead phrase, names the affected surface, and ends with an ADR reference when a
decision record exists. Dates use `YYYY-MM-DD`.

---

## Commit and tag conventions

Subjects use conventional-commit prefixes — `feat:`, `fix:`, `fix(scope):`,
`test:`, `test(scope):`, `docs:`, `build:`, `ci:`, and `release: vX.Y.Z` — kept
under about 70 characters, lowercase, with no trailing period. Bodies wrap near
72 columns and explain *why*, referencing ADRs by ID where one applies.
Attribution trailers are not added.

---

## Changing governance

- **Portable policy**: propose it upstream in `docs-governance`, then refresh
  the mirror (see [Mirror Provenance](documentation/contracts.md#5-mirror-provenance-and-refresh)).
  Do not edit mirrored files here.
- **Repository charters**: change this directory by pull request, with the
  rationale in the description.
- **Everything else**: if a rule applies to one subsystem only, it belongs in
  that subsystem's [architecture blueprint](../architecture/index.md) or in
  code comments, not in governance.

## See also

- [Repository Contracts](documentation/contracts.md) — active profiles and path bindings
- [Workflow and review gates](documentation/core/workflow.md) — the code-to-documentation trigger matrix
- [ADR Index](../adr/index.md) — immutable architectural decisions
- [Architecture Blueprints](../architecture/index.md) — how the system works today
- [Developer Documentation](../dev/index.md) — setup, module map, testing, acceptance
