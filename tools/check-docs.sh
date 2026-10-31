#!/bin/bash
# check-docs.sh — verify documentation invariants, Audience Firewall, and link integrity.
#
# Repository compliance:
#   - Root Markdown file whitelist (INV-CORE-02)
#   - Audience Firewall: user-facing docs never link into docs/dev/ (INV-CORE-01)
#   - Conceptual explanation purity: no code blocks or source paths in docs/explanation/ (INV-CORE-04)
#   - Directory charters: every docs directory declares an index.md or README.md (INV-CORE-05)
#   - ADR registration in docs/adr/index.md (INV-ARCH-REG)
#   - Relative link and anchor integrity (INV-LINK-02, INV-LINK-03)
#
# Exit 0 = compliant; exit 1 = violations (errors). Warnings do not fail.
# Dependencies: bash + python3 standard library only.
#
# Usage: tools/check-docs.sh [repo-path]
set -u

REPO=${1:-"$(cd "$(dirname "$0")/.." && pwd)"}
REPO=$(cd "$REPO" && pwd)

python3 - "$REPO" <<'PY'
import json, os, re, sys

root = sys.argv[1]
errors, warnings = [], []


def rel(p):
    return os.path.relpath(p, root).replace(os.sep, "/")


def err(path, line, inv, msg):
    errors.append((path, line, inv, msg))


def warn(path, line, inv, msg):
    warnings.append((path, line, inv, msg))


def read(path):
    with open(path, "r", encoding="utf-8") as fh:
        return fh.read()


# ---------------------------------------------------------------- compliance
USER_PLANES = ("docs/tutorials/", "docs/how-to/", "docs/reference/", "docs/explanation/")
ROOT_EXCEPTIONS = {"README.md", "CHANGELOG.md", "CONTRIBUTING.md", "AGENTS.md",
                   "SECURITY.md", "LICENSE"}

md_files = []
for dirpath, dirnames, filenames in os.walk(root):
    dirnames[:] = [d for d in dirnames
                   if not d.startswith(".") and d not in ("target", "node_modules")]
    for name in filenames:
        if not name.endswith(".md"):
            continue
        path = os.path.join(dirpath, name)
        md_files.append(rel(path))

# INV-CORE-02: no arbitrary Markdown files at the repository root.
for path in md_files:
    if "/" not in path and path not in ROOT_EXCEPTIONS:
        err(path, 0, "INV-CORE-02", "stray Markdown file at repository root")

LINK = re.compile(r"!?\[([^\]]*)\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
FENCE = re.compile(r"^(\s*)(`{3,}|~{3,})\s*(\S*)")


def slugify(heading):
    """Approximate a GitHub heading slug: lowercase, drop punctuation, keep
    word characters (including `_`), and join words with single hyphens."""
    text = heading.strip().lower()
    text = re.sub(r"[^\w\s-]", "", text)
    text = text.replace(" ", "-")
    return re.sub(r"-{2,}", "-", text).strip("-")


def headings(path):
    out = set()
    inside = False
    for line in read(path).splitlines():
        if FENCE.match(line):
            inside = not inside
            continue
        if inside:
            continue
        m = re.match(r"^(#{1,6})\s+(.*)$", line)
        if m:
            out.add(slugify(m.group(2)))
    return out


for path in sorted(md_files):
    in_user_plane = any(path.startswith(p) for p in USER_PLANES)
    in_explanation = path.startswith("docs/explanation/")
    lines = read(path).splitlines()
    target_headings = None
    inside_fence = False
    fence_lang = None
    h1_count = 0
    previous_level = 0

    for num, line in enumerate(lines, 1):
        fence = FENCE.match(line)
        if fence:
            if not inside_fence:
                inside_fence, fence_lang = True, fence.group(3)
                # style.md §4: fenced blocks declare a language identifier.
                if not fence_lang:
                    err(path, num, "INV-STYLE-01",
                        "fenced code block without a language identifier")
                # INV-CORE-04: explanation is conceptual, not code.
                if in_explanation:
                    err(path, num, "INV-CORE-04",
                        "fenced code block in docs/explanation/ (move coordinates "
                        "to docs/reference/ or docs/dev/)")
            else:
                inside_fence = False
            continue
        if inside_fence:
            continue

        # style.md §2: one H1 per file, and heading levels never skip.
        heading = re.match(r"^(#{1,6})\s+\S", line)
        if heading:
            level = len(heading.group(1))
            if level == 1:
                h1_count += 1
            elif previous_level and level > previous_level + 1:
                err(path, num, "INV-STYLE-04",
                    f"heading level jumps from H{previous_level} to H{level}")
            if level > 1:
                previous_level = level

        for label, target in LINK.findall(line):
            if label.strip().lower() in ("here", "link", "this", "click here", "read more"):
                err(path, num, "INV-STYLE-02",
                    f'generic link label "{label}" does not name its destination')
            if re.match(r"^[a-z][a-z0-9+.-]*:", target) or target.startswith("#"):
                continue
            file_part, _, anchor = target.partition("#")
            if not file_part:
                continue
            resolved = os.path.normpath(os.path.join(os.path.dirname(path), file_part))
            # INV-CORE-01: user-facing planes never link across the firewall.
            if in_user_plane and (resolved == "docs/dev" or resolved.startswith("docs/dev/")):
                err(path, num, "INV-CORE-01",
                    f"user-facing document links into the contributor plane: {target}")
            if resolved.startswith(".."):
                warn(path, num, "INV-LINK-01",
                     f"link escapes the repository: {target}")
                continue
            if os.path.isdir(os.path.join(root, resolved)):
                err(path, num, "INV-STYLE-03",
                    f"link targets a directory, not an explicit file: {target}")
                continue
            if not os.path.exists(os.path.join(root, resolved)):
                err(path, num, "INV-LINK-02", f"broken relative link: {target}")
                continue
            if anchor and resolved.endswith(".md"):
                if target_headings is None:
                    target_headings = {}
                if resolved not in target_headings:
                    target_headings[resolved] = headings(os.path.join(root, resolved))
                if anchor.lower() not in target_headings[resolved]:
                    err(path, num, "INV-LINK-03",
                        f'link anchor "#{anchor}" matches no heading in {resolved}')

        # INV-CORE-04: explanation must not carry implementation coordinates.
        if in_explanation and not line.lstrip().startswith("|"):
            for match in re.finditer(r"(?<![\w/])crates/[\w./-]+|(?<![\w/])src/[\w./-]+", line):
                err(path, num, "INV-CORE-04",
                    f"implementation coordinate in docs/explanation/: {match.group(0)}")

    if h1_count == 0:
        err(path, 0, "INV-STYLE-04", "no top-level heading")
    elif h1_count > 1:
        err(path, 0, "INV-STYLE-04", f"{h1_count} top-level headings; expected exactly one")

# INV-CORE-05: every documentation directory declares its charter and index.
for dirpath, dirnames, filenames in os.walk(os.path.join(root, "docs")):
    dirnames[:] = [d for d in dirnames if d not in (".git",)]
    names = set(filenames)
    if not any(n.endswith(".md") for n in names):
        continue
    directory = rel(dirpath)
    if "index.md" not in names and "README.md" not in names:
        err(directory, 0, "INV-CORE-05",
            "documentation directory without index.md charter/registry")

# INV-ARCH-REG: every record is registered in the ADR index, so that a reader
# can filter decisions without loading them.
adr_dir = os.path.join(root, "docs", "adr")
adr_index = os.path.join(adr_dir, "index.md")
if os.path.isdir(adr_dir) and os.path.exists(adr_index):
    index_text = read(adr_index)
    for name in sorted(os.listdir(adr_dir)):
        if not re.match(r"^\d{4}-.*\.md$", name):
            continue
        if f"({name})" not in index_text:
            err("docs/adr/index.md", 0, "INV-ARCH-REG",
                f"record {name} has no registry row in the ADR index")

# ---------------------------------------------------------------- report
for path, line, inv, msg in sorted(warnings):
    where = f"{path}:{line}" if line else path
    print(f"WARN  {where}: [{inv}] {msg}")
for path, line, inv, msg in sorted(errors):
    where = f"{path}:{line}" if line else path
    print(f"ERROR {where}: [{inv}] {msg}")

print(f"\ncheck-docs: {len(errors)} error(s), {len(warnings)} warning(s) "
      f"[invariants and link integrity]; scanned {len(md_files)} Markdown file(s)")
sys.exit(1 if errors else 0)
PY
