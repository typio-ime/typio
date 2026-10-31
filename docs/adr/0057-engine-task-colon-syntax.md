---
id: ADR-0057
title: "Engine-scoped task invocation uses target:action syntax"
status: accepted
date: 2026-10-31
superseded_by: null
scope: control
negative_knowledge: true
---

# ADR-0057: Engine-scoped task invocation uses target:action syntax

- **Status**: Accepted
- **Date**: 2026-10-31
- **Scope**: control
- **Deciders**: Typio maintainers
- **Supersedes**: `typioctl engine do` and `typioctl engine setup` from CLI control ADR-0004 (held in [`archive/cli-control/0004-resource-verb-cli-schema-on-tip.md`](archive/cli-control/0004-resource-verb-cli-schema-on-tip.md))
- **Related RFC**: Not used — this repository does not operate an RFC workflow

## Context

`typioctl` exposes imperative engine actions through the daemon's TIP `engine.invoke` RPC (`{ name, command }`). These actions — such as Rime's `deploy` and `sync`, or speech recognition model acquisition via `setup` — represent private behavioral workflows defined by individual engines rather than generic daemon lifecycle verbs.

Previously, `typioctl` provided two separate syntaxes to invoke them:

1. `typioctl engine do <name> <command>`: Inherited from archive ADR-0004. The verb `do` was chosen as a terse placeholder, but it is semantically vague and conveys no architectural intent. Furthermore, spanning four positional tokens (`typioctl` `engine` `do` `<name>` `<command>`) makes common operations unnecessarily verbose.
2. `typioctl engine setup [name]`: A specialized convenience command added later to facilitate model provisioning. Elevating `setup` into a top-level subcommand under `engine` eroded the conceptual boundary between host-owned engine lifecycle management (`list`, `show`, `use`, `props`, `actions`) and engine-owned private workflows.

These legacy forms created ambiguity over what is a host lifecycle verb versus what is an engine-owned internal task.

## Decision

1. **Adopt target:action task syntax.** Engine-specific tasks are dispatched using a colon-delimited target token:
   ```text
   typioctl engine <engine>:<action>
   ```
   Examples:
   ```text
   typioctl engine rime:deploy
   typioctl engine sherpa-onnx:setup
   ```
2. **Strict validation.** The parser requires exactly one colon separator, non-empty engine and action names, and rejects trailing arguments. Malformed invocations report the expected syntax explicitly.
3. **Clean break from legacy commands.** Both `typioctl engine do` and `typioctl engine setup` are removed outright without deprecation aliases. The `engine` subcommand tree now cleanly differentiates host-defined verbs (`list`, `show`, `use`, `next`, `props`, `actions`, `get`, `set`, `load`, `unload`, `reload`) from engine-owned tasks (`<engine>:<action>`).

## Rationale

- **Intent and scoping.** The `<engine>:<action>` syntax (widely established by build and container tooling like Gradle, npm scripts, and Docker Compose) explicitly establishes that `<action>` is a task namespaced within `<engine>`.
- **Elimination of ambiguous verbs.** Retiring `do` removes an overly broad verb that lacked technical precision.
- **Uniformity without host pollution.** Any engine can introduce private operational commands without altering the host's CLI subcommand tree or requiring ad-hoc host-level syntax concessions.
- **Brevity.** Consolidating the target and command into a single token reduces invocation length while preserving the `engine` resource namespace.

## Rejected Alternatives

- **Retain `typioctl engine do <name> <command>`.** Rejected: `do` is semantically diluted, provides no context regarding execution boundaries, and adds an unnecessary intermediate token.
- **Introduce a top-level `typioctl do <engine> <command>` or `typioctl invoke`.** Rejected: flattening the invocation breaks the `resource + verb` taxonomy established by ADR-0004 and obscures that the target belongs to the `engine` domain.
- **Dot-separated notation (`typioctl engine <engine>.<action>`).** Rejected: in the Typio configuration system, dot-separated identifiers are reserved for configuration keys (`engines.sherpa-onnx.model`). Using dots for task invocations invites syntactic confusion between reading/writing configuration properties and triggering imperative engine actions.
- **Keep `typioctl engine setup [name]` as a host-level shortcut.** Rejected: `setup` is a task owned by specific engines (like `sherpa-onnx`), not a universal property of every input engine. Special-casing it in the host client compromises engine autonomy and creates dead commands for engines that do not define a `setup` procedure.

## Consequences

- Invariant: `typioctl engine <engine>:<action>` is the sole CLI syntax for invoking engine tasks through `engine.invoke`.
- Host lifecycle verbs under `typioctl engine` remain strictly generic; engine-specific behaviors do not claim top-level subcommand names.
- All documentation, troubleshooting guides, and client manuals are updated to the target:action syntax.
