# Modifier-key consumption

Modifier keys follow the same engine result contract as all other keys. The
host forwards a key to the client only when the active engine returns
`NOT_HANDLED` or `PASS_THROUGH`; `HANDLED` consumes it even when the key is
Shift, Control, Alt, or Super.

The protocol modifier mask is a stable logical input field, not a Rust or C
struct layout. The host derives it from XKB state and preserves the raw
keycode, resolved keysym, base keysym, Unicode scalar, press/release state,
timestamp, and repeat marker in the same `KeyEvent`.

This rule lets an engine implement mode switches or language-specific modifier
semantics without leaking duplicate events to the application. Engines should
consume a bare modifier only when it has an intentional semantic effect.
