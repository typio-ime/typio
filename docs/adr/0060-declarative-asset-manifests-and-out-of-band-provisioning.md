---
id: ADR-0060
title: "Declarative asset manifests and out-of-band provisioning for recognition engines"
status: accepted
date: 2026-11-01
superseded_by: null
scope: architecture
negative_knowledge: true
---

# ADR-0060: Declarative asset manifests and out-of-band provisioning for recognition engines

- **Status**: Accepted
- **Date**: 2026-11-01
- **Scope**: architecture
- **Deciders**: Typio maintainers
- **Related RFC**: Not used — this repository does not operate an RFC workflow

## Context

Offline speech recognition, handwriting, and machine-learning input engines require substantial external model assets (e.g. 239 MB ONNX compute graphs, vocabulary tokenizers, language models). 

Historically, asset acquisition alternated between manual user scripts and embedding download routines inside the engine worker process via the Engine Protocol (`setup` command). Both approaches exhibit structural architectural flaws:

1. **Security and Sandboxing Incompatibility**: Engine worker processes handle sensitive user keystrokes (passwords, identity credentials) and raw microphone streams. Granting engine processes network access (`curl`, sockets) destroys the zero-trust security perimeter and prevents strict sandboxing (e.g. Landlock, seccomp, network namespace unsharing).
2. **Protocol Impedance Mismatch**: The Engine Protocol is an ultra-low-latency IPC channel. Host command dispatch operates under strict timeouts (5 seconds). Downloading multi-hundred-megabyte archives over variable networks inside the engine worker process leads to timeout aborts, detached zombie processes, and lifecycle races during desktop logout or input-method switching.
3. **Black-box User Experience**: In-process background workers cannot stream interactive download progress, bandwidth rates, or time estimates to the user's terminal TTY or the settings GUI.
4. **Distribution and Packaging Conflicts**: Embedding download targets into engine logic assumes mutable user home directories (`~/.local/share`), conflicting with system-wide Linux distribution packages (e.g. `/usr/share/typio/sherpa-onnx/`) that install models offline.

## Decision

Adopt a strict separation between **Zero-Network Offline Engine Runtimes**, **Declarative Asset Manifests**, and **Out-of-Band Foreground Provisioners**:

1. **Engine runtimes are strictly offline and zero-network.**
   Engine worker processes perform local inference only. They contain no HTTP clients, curl subprocesses, or extraction logic. An engine discovers models across multi-tier XDG-compliant search directories in order of precedence:
   - Custom host override (`<data_dir>/<engine>/`)
   - User data home (`$XDG_DATA_HOME/typio/<engine>/` or `~/.local/share/typio/<engine>/`)
   - System local share (`/usr/local/share/typio/<engine>/`)
   - System vendor share (`/usr/share/typio/<engine>/`)
   If a model is absent, the engine honestly reports `Availability::Failed` with an informative message.

2. **Asset metadata is declared statically in manifests.**
   Engine manifests declare model dependencies under `[assets]` tables (upstream archive URL, checksum, target subdirectory, required files), decoupling resource definitions from runtime code.

3. **Asset provisioning is executed out-of-band by foreground tooling.**
   `typioctl engine <engine>:setup` is handled by the foreground CLI tool rather than dispatched to the worker process. The CLI runs with full TTY privileges, providing native streaming progress bars, SHA-256 integrity verification, and atomic file extraction into the user's XDG data directory.

4. **Hot-reload synchronization.**
   Upon successful installation, the provisioning tool issues a lightweight `engine.reload` signal to the running `typio` daemon via TIP, prompting the engine to rescan its search directories and transition immediately to `Availability::Ready` without restarting the session.

## Rationale

- **Zero-trust input isolation:** Disabling network capabilities in engine processes ensures keystrokes and microphone streams cannot be exfiltrated, satisfying Linux desktop isolation requirements.
- **Distro and packaging friendliness:** By scanning `/usr/share`, Linux distributions can package speech models as standard RPM/Deb/Arch packages that function out-of-the-box in air-gapped environments without triggering downloads.
- **Superior user experience:** Foreground CLI tools naturally handle terminal resize, interactive progress bars, and informative error diagnostics during network interruptions.
- **Daemon minimalism:** Keeps `typio-daemon` lean and focused on Wayland event loops, avoiding bloat from network libraries, archive decompression, and proxy configuration.

## Rejected Alternatives

- **In-engine background download thread:** Rejected: violates process sandboxing, leaks network privileges to sensitive input workers, and hides progress behind an opaque status enum.
- **Daemon-hosted asset download manager:** Rejected: bloats `typio-daemon` with HTTP/TLS stacks, decompression libraries, and retry policies that do not belong in an input-method event loop.
- **Manual-only model installation:** Rejected: creates prohibitive friction for new users and results in broken configurations due to typos in directory names.

## Consequences

- Invariant: Engine worker binaries must not perform network I/O or invoke external download utilities.
- Engine manifests declare asset specifications statically.
- `typioctl engine <engine>:setup` provides rich, interactive provisioning and triggers seamless runtime reloads.
- Packagers and users can place speech models in any valid XDG directory.
