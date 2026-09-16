# Subsystem Architecture: Input Session

- Status: Living Blueprint
- Last Updated: 2026-09-15
- Scope: core/session — the Wayland input-method session, focus/grab controller, text-input transactions, preedit coalescing, and keyboard epoch fencing
- Maintainers: Typio maintainers

---

## 1. System Overview & Boundaries

The host is a Wayland-native input method. It receives activation, text state, and keys from the compositor, routes keys through a keyboard engine, and writes committed text and preedit back on the same channel. `zwp_input_method_v2` is the **only** input-method protocol the host speaks; the engine layer knows nothing about Wayland.

Three concepts span an engagement with a focused field and must not be conflated:

| Concept | What it is | Lifetime | Owner |
| :--- | :--- | :--- | :--- |
| Protocol session | One `activate` to `deactivate` cycle on `zwp_input_method_v2` | compositor `activate` to `deactivate` | compositor |
| Session / editing state | Wayland editing facts plus the router-owned `TypioInputContext` | first `activate` to frontend teardown | daemon |
| Grab / focus resource | The keyboard grab plus its virtual-keyboard keymap | `desired.grab = Yes` to `None` | focus controller |

The subsystem boundary stops before the engine and before the renderer:

- **Protocol knowledge is isolated.** `crates/typio-host-platform/src/input_method.rs` is the only module that binds `zwp_input_method_v2`, its keyboard grab, the popup surface, and `zwp_virtual_keyboard_v1`, and the only module that implements their `Dispatch` handlers. Everything downstream consumes plain Rust facts and types.
- **The engine is an out-of-process peer.** Keys leave the daemon through the engine backend; no engine callback runs on the Wayland thread.
- **Lifecycle state is derived, not stored.** The focus path keeps raw input facts plus live resource handles and recomputes what the resources should be on every reactor step.
- **Text commits are transactions.** Compositor-facing text payloads leave through one entry point; non-text lifecycle commits use a separate, narrower one.

Editing state survives a soft pause. A normal `deactivate` keeps the grab object alive and keeps the router-owned input context, so a quick field-to-field move does not reset the engine or re-compile the keymap.

## 2. Invariants & Non-Negotiable Rules

- **`[INV-SESSION-01]` One protocol module.** Only `crates/typio-host-platform/src/input_method.rs` may construct or call `zwp_input_method_v2`, `zwp_input_method_keyboard_grab_v2`, `zwp_input_popup_surface_v2`, or `zwp_virtual_keyboard_v1` requests; every other module sees state accessors, facts, and decoded key events.
- **`[INV-SESSION-02]` One text-transaction entry point.** Every commit that carries a text payload goes through `InputMethodState::text_transaction_and_flush(commit_text, preedit)`; `commit_protocol_state()` is reserved for lifecycle state with no text payload, so a text write that bypasses the transaction path is a review-blocking defect.
- **`[INV-SESSION-03]` Serial 0 is a write barrier.** No `commit` request may be sent before the compositor has delivered a `done` event: the serial is a count of `done` events received, and `text_transaction_and_flush` additionally requires the session to be active. Staging text that the compositor would silently drop is forbidden.
- **`[INV-SESSION-04]` No stored lifecycle phase.** The focus path persists only raw input facts and live resource handles; `crates/typio-daemon/src/focus_controller.rs` exposes `reduce` and `diff` as pure functions and holds no phase field. Any code that hand-mutates a phase is a defect.
- **`[INV-SESSION-05]` Effects are idempotent and ordered.** `diff(desired, actual)` produces a minimal effect set, and `apply` runs the effects in the fixed order in `crates/typio-daemon/src/session_glue.rs`: discard composition, focus out, destroy grab, clear preedit, commit, reset key routing, create grab, focus in, reactivate. Recovery is the normal path run against changed facts, never a bespoke branch.
- **`[INV-SESSION-06]` Grab plus keymap is one resource with one readiness state.** `GrabResourceState` is `Absent` / `NeedsKeymap` / `Ready`; creating a grab clears `keymap_received_this_epoch`, and `Ready` requires a compositor keymap observed in that epoch. Keys route to the engine only while the resource is `Ready`; modifier updates may apply while `NeedsKeymap`, key presses may not.
- **`[INV-SESSION-07]` One keyboard epoch fence.** Every queued key carries the platform keyboard epoch, advanced at activation, deactivation, keymap replacement, and grab teardown. Old presses never enter a new field; old releases may only pair existing virtual-keyboard presses. Events from a destroyed grab proxy are ignored. Router press ownership is cleared at boundaries.
- **`[INV-SESSION-08]` One ordered keyboard stream and one forwarding ledger.** `KeyboardInput` preserves keys, modifier samples, and boundaries in arrival order for active and inactive sessions. `InputMethodState::forwarded_keys` alone records virtual-keyboard presses. Boundaries release every remaining forwarded key; orphan and duplicate releases are suppressed. No callback forwards modifiers ahead of queued keys.
- **`[INV-SESSION-09]` Preedit coalescing is bounded and latest-wins.** Pure preedit is staged with a 2 ms quiet deadline and a fixed 4 ms hard deadline from the first update in the burst; later updates replace the payload and renew only the quiet deadline; real commit text bypasses both deadlines and flushes immediately.
- **`[INV-SESSION-10]` Focus level and boundary observation have separate lifetimes.** `im_is_active` gives the final focus level; `im_focus_changed` remains set until `take_facts()`. `done` advances text state and serial only. Multiple `done` events cannot erase a handoff, and observing a boundary before `done` cannot replay it later.
- **`[INV-SESSION-11]` Routing follows the engine result, not the key class.** The host forwards a key to the application only when the engine returns `NOT_HANDLED`; a `HANDLED` result consumes the key even when it is Shift, Control, Alt, or Super, so an engine can implement modifier semantics without leaking duplicate events.
- **`[INV-SESSION-12]` Every staged deadline is folded into the poll timeout.** The preedit coalescer deadline and the Wayland response deadlines tracked by `wayland_pending` (commit to `done`, grab to keymap, probe to rectangle) each contribute their earliest remaining time through `PollTimeout`, so no periodic tick is needed to make progress.
- **`[INV-SESSION-13]` State transitions are testable without a display server.** `InputMethodState::new_headless()` and `InputMethodFrontend::new_headless()` must keep state accessors, composition projection, focus facts, and pending-key queues fully operational, and protocol emitters must no-op in headless mode.

## 3. Component Architecture & Data Flow

### 3.1 Components

| Component | File | Responsibility |
| :--- | :--- | :--- |
| Platform frontend | `crates/typio-host-platform/src/input_method.rs` | Global binding, `Dispatch` handlers, `InputMethodState` / `InputMethodFrontend`, serial tracking, text transactions, grab + vk keymap bridge, `SessionState`, `CompositionState`, `PanelCoordinator` ownership, headless construction |
| Pure focus controller | `crates/typio-daemon/src/focus_controller.rs` | `GrabWant`, `DesiredState`, `GrabResourceState`, `ActualState`, `EffectSet`, `reduce`, `diff` |
| Effectful session glue | `crates/typio-daemon/src/session_glue.rs` | `observe`, `apply`, `ApplyTarget`, `FocusDriver::tick`, `FocusTransition` |
| Key policy | `crates/typio-daemon/src/keyboard_policy.rs` | Effective modifier mask and repeat-modifier policy |
| Keyboard router | `crates/typio-daemon/src/keyboard/router.rs` | Pure routing decision, host-managed selection, engine dispatch, text staging and flush |
| Preedit coalescer | `crates/typio-daemon/src/keyboard/preedit_coalescer.rs` | Latest-wins staging with quiet and hard deadlines |
| Host-managed selection | `crates/typio-daemon/src/candidate_guard.rs` | Which navigation/commit keys the host intercepts, given the engine's declared flags |
| Input driver | `crates/typio-daemon/src/app/input_driver.rs` | Ordered key batch, engine output, virtual-keyboard forwarding, repeat |
| Reactor | `crates/typio-daemon/src/app/event_loop.rs`, `app/reactor.rs` | Wayland dispatch ordering, deadline reduction, focus tick placement |

### 3.2 Per-tick pipeline

```text
facts   = take_facts()                    platform event handlers recorded them
desired = reduce(facts, prev)             pure: grab want + focus_in/out/reactivate edges
actual  = observe(frontend state, router) live: IC focus + grab resource state
effects = diff(desired, actual)           pure: minimal, idempotent
apply(effects)                            effectful, fixed order
```

`reduce` rules, in first-match-wins order:

| Condition | `desired.grab` |
| :--- | :--- |
| `!connection_alive`, `suspend_gap_detected`, or no engine | `None` (hard teardown) |
| `im_is_active` | `Yes` |
| inactive with a boundary or a previously wanted grab | `SoftPause` |
| otherwise | `None` |

The final level determines the target, so activate/deactivate in either order
converges correctly. A boundary observed while both previous and final targets
are `Yes` produces `reactivate`; it retains the grab and composition while
resetting gesture ownership and refreshing the panel anchor.

### 3.3 Protocol facts and the `done` boundary

Activation and deactivation update the active level, latch `im_focus_changed`,
advance the keyboard epoch, and append an ordered keyboard boundary. Editing
context remains buffered until `done`.

`done` advances `serial`, marks initialization complete, and commits the buffered
editing state. It never clears or reintroduces a focus edge. `take_facts()` alone
consumes the observed edge. This replaces the per-`done` activation classifier
from ADR-0018; see [ADR-0052](../adr/0052-ordered-keyboard-ownership.md).

### 3.4 Grab resource and keys

```text
ABSENT  --create_grab-->  NEEDS_KEYMAP  --keymap_received_this_epoch-->  READY
   ^                                                                            |
   +-------------------- destroy_grab (hard teardown / focus loss) --------------+
```

`observe` reads the live resource: `keyboard_grab_present()` plus `keymap_received_this_epoch` decide between `Absent`, `NeedsKeymap`, and `Ready`. The keymap itself is delivered by the compositor on the grab, compiled with xkbcommon, and mirrored to `zwp_virtual_keyboard_v1` before forwarding is healthy.

Key flow for one reactor step:

1. The focus controller reconciles resources against final focus facts.
2. `drive_pending_keys` drains `KeyboardInput` in order. Platform
   `prepare_input` forwards modifier samples, releases keys at boundaries,
   drops obsolete presses, and pairs releases without consulting an engine.
3. Inactive keys use that same transport ledger. Current active keys route
   through host selection and the engine; only owned presses can send a
   release into the router. An unowned modifier release still clears its
   physical baseline without completing a shortcut.
4. Engine output is drained after each key. Commit text flushes before the
   next key; pure preedit remains coalesced.
5. Forwarded keys use application-native repeat. The host timer handles only
   consumed keys allowed to repeat by XKB. Each expiration requires an active
   session, router ownership, a physically held key, and compatible modifiers.
   A transient blocking-modifier change cancels the chain even if it changes
   back before the next expiration. An unrelated release does not stop it.

### 3.5 Text transactions

```text
InputMethodState::text_transaction_and_flush(commit_text, preedit)
  requires initialized && active
  commit_string(text)            if commit_text is Some
  set_preedit_string(text, cur)  if preedit is Some
  commit(serial)                 one request, one serial
```

Staging lives in the router: `pending_commit_flush` for commit text (flushed in order before the next key) and `PreeditCoalescer` for composition-only preedit. A key that produces both commit text and a replacement preedit is sent as one transaction, which is the protocol's natural atomic form and avoids two same-serial commits. Compositor `done` is a state boundary and a serial advance, not a text-commit acknowledgement; preedit is never held waiting for it.

### 3.6 Where the derived-state rule is and is not honoured today

Honoured:

- `focus_controller.rs` holds no lifecycle phase. `observe` returns a snapshot, not a second source of truth.
- Recovery paths for suspend, reconnect, and internal drift all run the same `diff` to `apply` pipeline.

Not fully expressed by the current code:

- `session_glue::observe` hard-codes `connection_alive: true`; connection death is handled in the event loop (which exits on a disconnected Wayland socket) rather than by an observed axis feeding `reduce`.
- The boundary bit is an unconsumed event fact, cleared on observation. It is independent of text-state batches (ADR-0052).
- The status surface still declares a `lifecycle_phase` string field in `RuntimeState` (`crates/typio-daemon/src/service.rs`), which `daemon.status` would expose if a runtime-state provider were wired; the daemon's own backend returns no snapshot (`crates/typio-daemon/src/ipc_bus.rs`), and no focus-path code writes a phase into it.
- `docs/explanation/modifier-key-consumption.md` states that the host forwards a key for `NOT_HANDLED` **or** `PASS_THROUGH`, while `TypioInputContext::process_key` reports consumption for any result other than `NOT_HANDLED` (`crates/typio-runtime/src/input_context.rs`). Under the code, a `PASS_THROUGH` result today is treated as consumed.

Known limit, accepted rather than patched: `observe` reads resource *presence*, not liveness, so a resource that is dead but still present (canonically a grab whose compositor-side routing stopped while the client proxy survives) projects as healthy and produces no diff. Such conditions need an external fact source — the resume detector, or socket death surfaced by the loop — not a change inside `observe` or `diff`.

### 3.7 Retired focus machinery — do not reintroduce

| Retired | Why it must not return |
| :--- | :--- |
| Boolean reactivation predicates (`should_defer_activate`, `should_cleanup_on_done`, `should_commit_reactivation`) plus a `pending_reactivation` flag and a separate `handle_reactivation()` path | One was already dead on arrival after an incomplete refactor; the active-to-active case is handled once, at the batch boundary, through recorded facts ([ADR-0018](../adr/0018-focus-transition-classification.md)) |
| Per-boundary recovery paths (one each for suspend, reconcile, reconnect) | Scrub-and-rebuild logic drifts apart; the idempotent diff makes them one path ([ADR-0003](../adr/0003-session-controller-reduce-diff.md)) |
| Storing lifecycle phase as a cache, with observed axes as a passenger | A stored projection of lifecycle reintroduces exactly the drift a reconciler exists to chase ([ADR-0003](../adr/0003-session-controller-reduce-diff.md)) |
| Per-key generation arrays and synthetic-release markers in both router and platform | One queue epoch and one virtual-keyboard ledger cover active and inactive paths (ADR-0052). |
| Holding preedit until the compositor's next `done` | `done` does not acknowledge a text transaction and can leave preedit waiting indefinitely ([ADR-0043](../adr/0043-bounded-preedit-coalescing.md)) |
| Requiring a live Wayland display to test state transitions | Headless construction is what keeps these transitions covered in CI ([ADR-0047](../adr/0047-headless-platform-state-decoupling.md)) |

## 4. Historical Lineage & Founding ADRs

- Compaction status: **no record has been compacted yet.** Each record below remains an active entry in [the ADR index](../adr/index.md); none has been tombstoned, moved to `docs/adr/archive/`, or superseded by this blueprint.

| Record | What it established |
| :--- | :--- |
| [ADR-0002: Adopt `zwp_input_method_v2` as the Host Protocol](../adr/0002-wayland-input-method-v2.md) | The protocol choice, defensive serial handling, protocol isolation in one module, and the virtual-keyboard pairing |
| [ADR-0003: Session Controller — Derived State, Idempotent Diff](../adr/0003-session-controller-reduce-diff.md) | No stored lifecycle phase; facts plus live handles; idempotent effects; grab plus vk keymap as one resource with one readiness state; one generation fence |
| [ADR-0018: Focus-transition classification and re-activation](../adr/0018-focus-transition-classification.md) | The `activate_seen` fact, `ACTIVATE` / `DEACTIVATE` / `REACTIVATE` / no-op classification, and grab-preserving reactivation with a Panel re-anchor |
| [ADR-0042: Stage text-input transactions at key-batch boundaries](../adr/0042-text-input-transaction-staging.md) | The single transaction entry point, commit-text ordering before the next key, and preedit coalescing within a pending-key drain |
| [ADR-0043: Bounded Preedit Coalescing Across Reactor Steps](../adr/0043-bounded-preedit-coalescing.md) | The 2 ms quiet window and 4 ms hard deadline, immediate commit-text flushes, clearing on focus/reset boundaries, and deadline participation in the poll timeout |
| [ADR-0047: Headless Platform State Decoupling](../adr/0047-headless-platform-state-decoupling.md) | Transport proxies behind an option, headless constructors, no-op protocol emitters, and unconditionally running state tests |

| [ADR-0052: Ordered Keyboard Events and Press Ownership](../adr/0052-ordered-keyboard-ownership.md) | Supersedes ADR-0018 focus-fact lifetime and extends ADR-0003 epoch fencing; preserves derived resource reconciliation. |

## See Also

- [Input-Method Session](../explanation/input-method-session.md) — the three session lifetimes and the build-up chain
- [Focus Controller](../explanation/focus-controller.md) — the reduce/diff model and its blind spot
- [Wayland Input Method Protocol](../explanation/wayland-input-method.md) — protocol handlers and the serial chokepoint
- [Modifier-key consumption](../explanation/modifier-key-consumption.md) — the engine-result contract for modifiers
- [Composition state machine](../explanation/composition-state-machine.md) — composition as state, commit as event
- [Daemon Lifecycle](daemon-lifecycle.md) — where the reactor step and its deadlines come from
