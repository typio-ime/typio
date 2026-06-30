# Modifier Key Consumption

When a keyboard engine handles a key press — including a modifier key like `Shift`, `Ctrl`, or `Alt` — the framework must **consume** that key. It must not forward the same key event to the client application through the virtual keyboard. Forwarding a handled modifier breaks engines that use modifiers for internal mode switching.

## The Problem

Consider Rime's `ascii_composer` switch key. When the user presses `Shift_L`, Rime's `process_key` returns *handled*: it toggles `ascii_mode` and the engine notifies the framework of a mode change. If the framework then **also** forwards the `Shift_L` press through `zwp_virtual_keyboard_v1`, two things go wrong:

1. **The client sees a bare modifier press.** The application receives a `Shift` press with no associated printable key, which is usually a no-op but can confuse toolkits that track modifier state independently.

2. **The engine's state machine diverges.** On the next key press, the compositor's modifier mask still includes `Shift`. Rime receives a keysym with `Shift` in the mask and may re-evaluate the mode, causing double-toggles, missed toggles, or stuck states.

The root cause is that modifier keys are not "handled" in the same sense as printable characters. A printable key is *handled* when it becomes part of a composition. A modifier key is *handled* when the engine used it for a state transition. In both cases, the framework must treat `handled = true` as **consume and do not forward**.

## The Fix

The Wayland frontend's key router decides whether to forward a key based on the engine's return value from `process_key`:

- `TYPIO_KEY_NOT_HANDLED` → forward through virtual keyboard
- `TYPIO_KEY_HANDLED`, `TYPIO_KEY_COMPOSING`, `TYPIO_KEY_COMMITTED` → consume

An older version of the router contained a special case: `!handled || is_modifier`. This meant modifiers were forwarded even when the engine returned `HANDLED`. Removing that special case — changing the condition to plain `!handled` — fixes the double-delivery.

The `is_modifier` flag still exists in the trace path (it affects the log reason string), but it no longer overrides the consumption decision.

## Bare Shift: Press-Consume, Release-Act Pattern

For bare Shift handling (Shift with no other keys pressed during the hold), the Rime engine uses a **press-consume, release-act** pattern:

1. **Shift press**: the engine consumes immediately (`TYPIO_KEY_HANDLED`) without forwarding to librime. It sets `shift_held` and `shift_only` flags on the session.

2. **Bare Shift release** (no other keys pressed during the hold): the engine commits the raw preedit text (e.g., typed pinyin), clears the librime composition, toggles `ascii_mode` via `set_option()`, and returns `TYPIO_KEY_COMMITTED`.

3. **Non-bare Shift release** (other keys were pressed during the hold): the engine consumes without side effects.

4. **Any non-Shift key press**: clears the `shift_only` flag, so a subsequent Shift release is treated as non-bare.

This pattern runs at the engine boundary before `api->process_key`, bypassing librime's schema-dependent `key_binder` which varies across Rime schemas and can leave composition in an inconsistent state.

## Why This Matters for All Engines

Rime is the most visible victim because `ascii_composer` binds `Shift` by default, but the same bug would affect any engine that uses modifiers:

- A Korean engine that uses `Shift` to switch between jamo sets
- A Vietnamese engine that uses `Ctrl` for tone marks
- A custom engine that uses `Alt` for symbol layers

The rule is universal: **handled is handled, regardless of key type.**

## See Also

- [Engine Contract](engine-contract.md) — the framework-side abstraction
- [Engine Operations](../reference/engine/ops.md) — `TypioKeyboardEngineOps::process_key` signature
