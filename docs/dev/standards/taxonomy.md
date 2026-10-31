# Documentation Taxonomy and Audience Firewall

This document defines the information architecture, quadrant boundaries, and audience separation policies for Typio documentation.

---

## 1. Diátaxis 4-Quadrant Framework

Documentation is strictly organized into four orthogonal quadrants based on user intent and cognitive mode:

```text
                     PRACTICAL                     THEORETICAL
           ┌─────────────────────────────┬─────────────────────────────┐
           │                             │                             │
ACQUISITION│          Tutorials          │         Explanation         │
  (Learning)│    `docs/tutorials/`        │     `docs/explanation/`     │
           │                             │                             │
           ├─────────────────────────────┼─────────────────────────────┤
           │                             │                             │
APPLICATION│        How-To Guides        │          Reference          │
   (Doing) │      `docs/how-to/`         │      `docs/reference/`      │
           │                             │                             │
           └─────────────────────────────┴─────────────────────────────┘
```

1. **Tutorials (`docs/tutorials/`)**: Guided, hands-on learning experiences for beginners. Linear, reproducible, guaranteed to work.
2. **How-To Guides (`docs/how-to/`)**: Goal-oriented recipes solving specific problems (e.g., packaging, debugging candidate lag, writing an engine).
3. **Reference (`docs/reference/`)**: Strict, authoritative technical information (CLI syntax, config options, IPC JSON schemas, stability policy).
4. **Explanation (`docs/explanation/`)**: High-level conceptual understanding, architectural motivations, and systems thinking.

---

## 2. The Audience Firewall (`[INV-CORE-01]`)

Typio maintains a strict one-way visibility boundary between public user documentation and internal contributor engineering guides:

```text
┌────────────────────────────────────────────────────────┐
│ Public User Planes                                     │
│ `docs/tutorials/`, `docs/how-to/`,                     │
│ `docs/reference/`, `docs/explanation/`                 │
└───────────────────────────┬────────────────────────────┘
                            │
               ▲            │
         NO    │            │ YES
       LINKS   │            │ Outward links permitted
               │            ▼
┌──────────────┴─────────────────────────────────────────┐
│ Contributor Internal Plane                             │
│ `docs/dev/**`                                          │
└────────────────────────────────────────────────────────┘
```

- **Rule**: Public user-facing documents must **NEVER** link into `docs/dev/**`.
- **Rationale**: External users, packagers, and engine authors should never encounter private internal developer workflows, local worktree tricks, or maintainer tooling.
- **Enforcement**: Mechanically verified by `tools/check-docs.sh`.

---

## 3. Conceptual Purity of Explanation (`[INV-CORE-04]`)

Documents under `docs/explanation/` provide conceptual context and architectural models:

- Fenced code blocks (` ``` `) are strictly prohibited.
- Implementation coordinates (e.g., `crates/typio-daemon/src/...`) are prohibited. Source locations belong in [`docs/dev/module-map.md`](../module-map.md); schema values belong in [`docs/reference/`](../../reference/index.md).

---

## 4. Root File Whitelist (`[INV-CORE-02]`)

To prevent repository root clutter, Markdown files at the repository root are strictly limited to:

- `README.md`
- `CHANGELOG.md`
- `CONTRIBUTING.md`
- `AGENTS.md`
- `SECURITY.md`
- `LICENSE`

All other documentation belongs in the appropriate subdirectories under `docs/`.

---

## 5. Directory Charters (`[INV-CORE-05]`)

Every directory under `docs/` must contain an `index.md` or `README.md` stating its purpose, scope, and index of contents.
