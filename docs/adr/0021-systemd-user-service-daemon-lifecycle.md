---
id: ADR-0021
title: "systemd user service for daemon lifecycle"
status: accepted
date: 2026-06-03
scope: loop
---

# ADR-0021: systemd user service for daemon lifecycle

- **Status**: Accepted
- **Date**: 2026-06-03
- **Deciders**: typio-linux maintainers

## Context

`typio` is a session daemon, not a foreground desktop application. It has no
primary window, should normally have one instance per user session, and must
remain available while the graphical session is active. Operational diagnosis
also depends on a stable place to inspect logs and process state.

Desktop entries are a good launch surface for foreground applications, but a
desktop entry that directly executes a long-running daemon leaves process
ownership to the compositor or session launcher. That makes stderr capture
environment-dependent, weakens duplicate-start control, and provides no
standard restart policy or service status.

## Decision

Install and document `typio.service` as the only packaged startup surface for
the daemon. Remove packaged `.desktop` launcher and XDG autostart entries.

The service runs the daemon as a foreground process under the systemd user
manager. Standard output and standard error go to the user journal, and
packagers enable the unit for graphical sessions.

## Alternatives considered

- **Keep direct `.desktop` launch and autostart**: Rejected because logs land in
  compositor/session-specific streams, process supervision is absent, duplicate
  starts are harder to control, and troubleshooting cannot rely on one service
  name.
- **Keep `.desktop` entries that call `systemctl --user start typio.service`**:
  Rejected because it leaves a redundant launch surface that still depends on
  desktop autostart behavior. The daemon should be session infrastructure
  managed by the user service manager, not an application launcher item.
- **Write Typio-managed log files for desktop launch compatibility**: Rejected
  because input-method logs may contain sensitive input context, and a custom
  file logger would need rotation, retention, permissions, and cleanup policy
  already provided by the journal.

## Consequences

- Positive: `systemctl --user status typio.service` reports process state.
- Positive: `journalctl --user -u typio.service` is the stable log surface for
  installed builds.
- Positive: restart policy and duplicate-start behavior are owned by the user
  service manager.
- Trade-off: environments without systemd user services need an explicit
  downstream integration instead of the upstream packaged startup files.
- Negative (accepted): Typio no longer appears as a launcher application from
  upstream-installed desktop entries.
