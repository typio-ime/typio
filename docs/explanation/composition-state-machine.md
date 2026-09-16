# Composition state machine

Every input context owns one current composition and an ordered queue of output
events. The empty composition is the idle state.

| State | Trigger | Next state |
|---|---|---|
| Idle | the engine publishes a non-empty composition | Composing |
| Composing | the engine publishes a composition update | Composing |
| Composing | the engine publishes a clear, or replaces the composition with an empty one | Idle |
| Composing | a commit followed by a non-empty composition | Composing |
| Composing | a commit followed by a clear | Idle |

A composition is an atomic snapshot containing preedit segments, cursor,
candidates, page metadata, selection, paging flags, and host-managed selection
flags. Replacing the whole value prevents preedit and candidate state from
observing different revisions.

A commit is not state. It is a one-shot ordered event because a key may commit
final text and also begin or retain another composition. The runtime therefore
queues commits and composition changes in reply order; the host drains them and
applies the corresponding Wayland requests during its normal flush phase.

Focus changes do not destroy the input context. A focus-out hides visible state
at the host boundary, while the engine may retain per-context language state
under the stable numeric context id. A reset explicitly abandons the current
composition. Context destruction finally releases runtime state.

Candidate clicks arrive as a commit-candidate request carrying the context id
and the chosen index, and are never turned into synthetic keyboard input.
Selection-only keyboard navigation can be host-managed when the composition
flags allow it; otherwise it is sent to the engine so language-specific policy
remains authoritative.
