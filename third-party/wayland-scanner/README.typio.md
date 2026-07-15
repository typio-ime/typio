# Typio vendoring note

This directory contains the crates.io source for `wayland-scanner 0.31.10`,
published from upstream commit `a3d7927d87799b2955bf491b51c7c2a3a82da661`.
Its Typio changes are the `quick-xml` dependency update from 0.39 to 0.41 and
the corresponding `xml_content()` to `xml10_content()` API rename. Both match
upstream commit `d07c4f91f28b42e5a485823ffd9d8d5a210b1053`.

The dependency-only backport addresses RUSTSEC-2026-0194 and
RUSTSEC-2026-0195 while preserving compatibility with the released Wayland
0.31 crates. Remove this copy and the workspace `[patch.crates-io]` entry once
crates.io publishes a compatible `wayland-scanner` release containing the
same fix.
