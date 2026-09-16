# Acceptance: User Journeys and Scenario Matrix

This document defines the end-to-end journeys a user must be able to complete,
and the scenarios verified before a release. Every journey here drives Typio
through its real user surfaces — the `typio` daemon binary, the `typioctl` and
`typio-settings` clients, the TIP control socket, and the Wayland session — from
a cold start. No journey may depend on a private test harness, a debug-only
backdoor, or a hand-edited state file ([INV-VAL-01](../governance/documentation/core/invariants.md)).

Mechanical, automatically reproducible verification lives in
[Testing](testing.md) instead. This document is about outcomes a user can see.

---

## 1. Core User Journeys

Each journey starts from the **cold-start precondition**: a fresh Wayland
session, no Typio process running, no user configuration directory, and at least
one engine package available to install.

### Journey 1: Cold start to first typed character

1. **Initial state**: cold-start precondition. Nothing of Typio is running.
2. **Action**: build or install Typio, then start the daemon as a user service:

   ```bash
   systemctl --user enable --now typio.service
   ```

3. **Verification**: the service starts, and `typioctl daemon status` connects
   and reports a daemon version, a control-protocol version, and uptime.
4. **Action**: install an engine package, then select it:

   ```bash
   typioctl engine list
   typioctl engine use <engine-name>
   ```

5. **Verification**: `engine list` marks the selected engine as active.
6. **Action**: click into a text field in any application and type.
7. **Outcome**: text appears in the application. When the engine produces
   candidates, the Panel appears next to the caret; the tray indicator shows the
   active language. No keystroke is lost while the Panel is visible.

The step-by-step version of this journey is the
[Your First Typio Session](../tutorials/getting-started.md) tutorial.

### Journey 2: Switching what types for you

1. **Initial state**: Journey 1 completed; at least two engines installed.
2. **Action**: press the language-switch shortcut (default `Ctrl+Shift`), then
   run `typioctl language next` from a terminal.
3. **Verification**: both paths change the active language in the same
   direction, the tray indicator updates, and `typioctl language list` agrees
   with what is on screen.
4. **Action**: type again.
5. **Outcome**: candidates come from the engine that serves the newly active
   language. The previously active engine's composition is not carried over.

### Journey 3: Reconfiguring a running daemon

1. **Initial state**: Journey 1 completed.
2. **Action**: read a key, change it, then verify the change:

   ```bash
   typioctl config get <key>
   typioctl config set <key> <value>
   typioctl config get <key>
   ```

3. **Verification**: the second read returns the value just written, and the
   change takes effect in the running daemon without a restart.
4. **Action**: `typioctl config unset <key>`.
5. **Verification**: the key returns to its documented default.
6. **Outcome**: configuration changes round-trip through the control surface
   alone; no file editing is required for keys the daemon owns.

### Journey 4: Configuring graphically

1. **Initial state**: Journey 1 completed; a graphical session is available.
2. **Action**: launch `typio-settings`.
3. **Verification**: the window lists the engines and languages the running
   daemon reports, rather than a cached or hand-parsed configuration.
4. **Action**: change a setting, then type in another window.
5. **Outcome**: the change is visible in behaviour, and the daemon is still the
   same process (the client reconfigures it over the control surface instead of
   restarting it).

### Journey 5: An engine stops responding

1. **Initial state**: Journey 1 completed with an interactive engine.
2. **Action**: kill the engine's worker process while typing.
3. **Verification**: the daemon stays responsive — the keyboard grab is not
   held, and keys that the engine would not have handled still reach the
   application. The daemon reports the engine's degraded state through
   `typioctl engine list` or the status surface.
4. **Outcome**: typing continues to work; re-activating the engine re-spawns the
   worker and restores candidates.

### Journey 6: The compositor restarts

1. **Initial state**: Journey 1 completed.
2. **Action**: restart the Wayland compositor (or log out and back in) with the
   daemon running.
3. **Verification**: the daemon either re-establishes its input-method session
   or exits cleanly and is restarted by the user service.
4. **Outcome**: after restarting, Journey 1's step 6 works again with no manual
   state repair and no stale preedit left in any application.

---

## 2. Acceptance Scenario Matrix

| Scenario ID | Category | Initial condition | Action / trigger | Expected observable outcome |
| :--- | :--- | :--- | :--- | :--- |
| `SCEN-01` | Happy path | Cold start | Complete Journey 1 | Text appears; Panel follows the caret; tray shows the active language |
| `SCEN-02` | Happy path | Journey 1 complete | Press `Ctrl+Shift` repeatedly | Language cycles and wraps; tray and `typioctl language list` agree |
| `SCEN-03` | Edge case | No engine package installed | Complete Journey 1 step 6 | Typing passes through unchanged; `typioctl engine list` reports no engine; no Panel appears |
| `SCEN-04` | Edge case | Engine declares no languages | Press `Ctrl+Shift` | Selection cycles to the next engine instead of failing |
| `SCEN-05` | Edge case | Daemon not running | Run `typioctl daemon status` | The client reports a connection failure with a non-zero exit status; it does not hang |
| `SCEN-06` | Edge case | No text field focused | Type with the daemon running | Keys reach the session normally; no Panel is shown without a focused input context |
| `SCEN-07` | Edge case | Tab or window switch mid-composition | Switch focus while a preedit is visible | No stale preedit remains in the abandoned field; the next field starts clean |
| `SCEN-08` | Error recovery | Interactive engine killed | Journey 5 | Keyboard stays responsive; keys pass through; re-activation restores candidates |
| `SCEN-09` | Error recovery | Engine that crashes during initialisation | Activate it | The daemon retries with backoff instead of retrying once per keystroke; periodic activation attempts fail fast and visibly |
| `SCEN-10` | Error recovery | Compositor restart | Journey 6 | Session is re-established or the service restarts the daemon; typing works again afterwards |
| `SCEN-11` | Error recovery | Corrupt configuration file | Start the daemon | The daemon starts with the previous or default configuration and reports the problem; it does not refuse to start |
| `SCEN-12` | Security | Another local user's process | Attempt to connect to the control socket | The connection is refused; the socket is not world-writable |
| `SCEN-13` | Security | Engine directory contains a manifest that does not declare the engine protocol | Start the daemon | The manifest is ignored, and `typioctl engine list` does not show it |
| `SCEN-14` | Security | Engine disabled by configuration | Type with that engine active | The engine is not driven and receives no keystrokes |
| `SCEN-15` | Constraint | Large candidate page (many pages of candidates) | Page through all candidates while typing | Paging stays responsive; memory and buffer use stay bounded rather than growing with the widest page seen |
| `SCEN-16` | Constraint | Long session with continuous typing | Type for a sustained period | Resident memory and wakeups stay bounded; no degradation in candidate latency |

---

### Shortcut Focus Handoff

From a fresh daemon start, open two Chrome tabs containing editable fields.
Focus a field in the second tab, press Ctrl+W, and release both keys after the
first tab becomes active. Repeat with W released before Ctrl, Ctrl released
before W, and rapid tab closure. Wait several seconds each time: the remaining
field must not receive repeated letters. Type normally afterward to verify
that Ctrl is not stuck.

Also move between an editable field and non-editable browser chrome while a
key is held. Releases must stop repetition on either side of that transition.
Verify that holding Backspace inside engine composition repeats normally, and
that holding a plain letter in direct input uses the browser's normal repeat.
Record compositor, browser, engine, and tested binary revision with the result.

## 3. Verification Checklist

Run before tagging a release:

- [ ] Every journey in Section 1 completes from a cold start on a real Wayland
      session, on the release commit, with the released binaries.
- [ ] Every scenario in Section 2 was exercised, with the observed outcome
      recorded in the release pull request.
- [ ] Both TIP clients were exercised: `typioctl` on the command line and
      `typio-settings` in the graphical session.
- [ ] No journey required a private test backdoor, an environment variable that
      is not documented in [Developer Setup](setup.md), or a hand-edited file
      outside the documented configuration paths.
- [ ] The mechanical suites in [Testing](testing.md) pass on the same commit.

## See also

- [Testing](testing.md) — automated suites and their environment matrix
- [Optics Dev Worktree](optics-dev-worktree.md) — building against a live Optics worktree
- [Interface Stability](../reference/stability.md) — what a release is allowed to change
