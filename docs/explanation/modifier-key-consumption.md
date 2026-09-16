# Modifier-key consumption

Modifier keys follow the same engine result contract as all other keys. The
host forwards a key to the application only when the active engine declines it
with a not-handled result (`NOT_HANDLED`); any other result consumes the key,
even when the key is Shift, Control, Alt, or Super.

The protocol's modifier mask is a stable logical input field, not a Rust or C
struct layout. The host derives it from the keyboard state and preserves the raw
keycode, the resolved keysym, the base keysym, the Unicode scalar, the
press/release state, the timestamp, and the repeat marker in the same key
description, so the engine never depends on host-internal state.

This rule lets an engine implement mode switches or language-specific modifier
semantics without leaking duplicate events to the application. Engines should
consume a bare modifier only when it has an intentional semantic effect.

## Divergence: pass-through is consumed today

The engine result contract defines a third outcome, pass-through
(`PASS_THROUGH`), whose stated intent is to keep the current composition alive
while still forwarding the key to the application. The host does not implement
that distinction: its key path reports consumption for every result other than
the not-handled result, so a pass-through result is swallowed in exactly the
same way as a handled one and the application never receives the key.

This page previously described the stated intent as if it were the behaviour.
The code is authoritative: treat pass-through as equivalent to handled for every
key, modifiers included, until the host path separates the two. The
[Input Session Blueprint](../architecture/input-session.md) records the same
divergence from the session side, and the
[Engine Contract](engine-contract.md) states the routing-result contract the
host implements.
