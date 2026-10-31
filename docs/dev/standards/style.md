# Documentation & Markdown Style Guide

This document establishes the formatting, hyperlink, heading, and prose standards across Typio documentation.

---

## 1. Heading Hierarchy

1. **Exactly One H1**: Every Markdown document must contain exactly one top-level `# Heading` at the beginning of the file.
2. **No Heading Skips**: Heading levels must increment sequentially without skipping (e.g. `## H2` followed by `### H3`; jumping directly from `## H2` to `#### H4` is forbidden).
3. **Title Case**: Headings use Title Case for document titles and sentence case or title case consistently within sections.

---

## 2. Hyperlink Conventions

1. **Descriptive Anchor Text**: Never use generic link text like "here", "click here", "link", or "this". The anchor text must explicitly describe the target document or concept.
2. **Relative Linking**: Internal links must use relative paths (`../architecture/panel-rendering.md`) and resolve to explicit files, not bare directories.
3. **Anchor Verification**: Links with fragment identifiers (`#section-slug`) must match an existing heading slug in the target document.
4. **One Home Per Fact**: Avoid duplicating explanations or configuration tables across files. Link to the canonical reference source instead.

---

## 3. Code Blocks and Code Spans

1. **Fenced Blocks**: Fenced code blocks must specify an explicit language identifier (`bash`, `rust`, `toml`, `text`, `c`).
2. **Inline Spans**: Inline symbols, types, function names, file paths, and config keys must be enclosed in backticks (`typio-daemon`, `wl_surface`, `core.toml`).

---

## 4. Prose and Voice

1. **Clear and Concise**: Prefer direct, active-voice technical prose over academic abstraction.
2. **Mechanisms Over Exhortations**: Explain concrete technical guarantees, invariants, and failure modes rather than vague admonishments.
3. **Truth and Accuracy**: Keep documentation synchronized with the current implementation. When behavior changes, update the architecture docs and Diátaxis guides in the same commit.
