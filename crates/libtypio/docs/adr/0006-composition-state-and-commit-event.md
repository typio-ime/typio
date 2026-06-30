# ADR-0006: Composition as State, Commit as Event — the Engine↔Framework Contract

- **Status**: Accepted
- **Date**: 2026-05-28
- **Deciders**: Project maintainers

## Context

The engine↔framework contract has two outputs that look superficially similar but differ in ontology:

- **Preedit and candidates** describe the *in-flight composition*. They are re-readable state: at any moment, there is one current preedit and one current candidate list. They change together (a new keystroke updates both), and the framework renders them together.
- **Commit text** describes a *one-shot event*: finalized text that has been emitted into the client and is no longer the framework's concern.

An earlier design exposed three independent channels (set_preedit, set_candidates, commit) with three callbacks. That factoring was wrong:

- Preedit and candidates were the same concept split into two state channels that could disagree and had a hidden render order. An engine emitting `set_preedit` without `set_candidates` silently never rendered, because rendering ran inside the candidate callback.
- Putting commit into the same shape as composition collapses two ontologies (state vs event) into one, which is then forced to add ad-hoc rules to express things like "this `process_key` committed text *and* left a residual composition" (Rime commits a completed phrase while keeping trailing pinyin in preedit).

## Decision

The engine↔framework contract is **two channels** with distinct semantics:

- **Composition (state).** Preedit + candidates fuse into one transactional value, emitted by one call: `typio_input_context_set_composition(ctx, &comp)`. The context is never half-updated. An *empty* composition (`segment_count == 0 && candidate_count == 0`) is the `Idle` state; there is no separate "clear" call.
- **Commit (event).** Stays a one-shot ordered event: `typio_input_context_commit(ctx, text)`. Commit is **not** a variant of the composition struct, because a single `process_key` can both commit finalized text *and* leave a residual composition. A terminal `COMMIT` variant cannot represent "committed X, now composing Y"; two channels can.

```c
typedef struct TypioComposition {
    size_t struct_size;          /* sizeof at author's compile time; append-only ABI */

    /* preedit */
    const TypioPreeditSegment *segments;
    size_t segment_count;
    int cursor_pos;              /* Unicode scalar values, not bytes/graphemes */

    /* candidates */
    const TypioCandidate *candidates;
    size_t candidate_count;
    int page, page_size, total, selected;
    bool has_prev, has_next;
    uint64_t content_signature;  /* stable; excludes `selected` (selection-only delta) */
    uint64_t revision;           /* monotonic per ctx; cheap "anything changed?" key */
} TypioComposition;

void typio_input_context_set_composition(TypioInputContext *ctx,
                                         const TypioComposition *comp);
void typio_input_context_commit(TypioInputContext *ctx, const char *text);
```

### Ordering contract

Within one `process_key` turn the engine may emit any sequence of composition updates and commits. The framework preserves commit order *relative to* composition updates, but may coalesce consecutive composition updates (last wins) and renders once per loop iteration. So "commit then compose" becomes `commit(text)` followed by a composition update; "compose then commit" clears the composition after the commit. Both fall out of the same rule with no special case.

### Return code

`process_key` returns one of four values:

```c
TYPIO_KEY_NOT_HANDLED   /* pass through */
TYPIO_KEY_HANDLED       /* consumed, no composition change */
TYPIO_KEY_COMPOSING     /* composition updated */
TYPIO_KEY_COMMITTED     /* text committed */
```

The return code is informational; the framework relies on the composition and commit emissions, not the code, to drive rendering.

### ABI longevity rules

Engines are runtime-loaded `.so`s authoring composition across a version-skewed boundary, so these are load-bearing:

- **Append-only structs with a leading `size_t struct_size`.** Readers honor only the fields the writer's size covers; new fields are appended, never reordered or resized.
- **Uniform borrowed-pointer ownership.** Every pointer in `TypioComposition` (and the commit string) is borrowed and valid **only for the call/callback duration**; the receiver copies what it must retain. One rule for all fields kills the recurring use-after-free patch class.
- **Pinned encoding contract.** All strings are UTF-8, NUL-terminated, never `NULL` for required fields (use `""`). `cursor_pos`, `selected`, and offsets count Unicode scalar values — not bytes, not grapheme clusters.
- **Transactional emit only.** No incremental mutation API; partial states are unrepresentable.

## Alternatives considered

- **Keep three callbacks, just document the ordering.** Rejected: a required ordering between "independent" callbacks is a latent bug surface; the atomic update makes the ordering unrepresentable.
- **Fold commit into the composition value as a terminal variant.** Rejected: cannot represent "committed X, now composing Y" without contortions.
- **Incremental mutation API (set_preedit_text, set_candidate_at, …).** Rejected: lets the context be half-updated; recreates the disagreement bug the redesign solved.

## Consequences

- Positive: composition can no longer half-render; the engine→framework contract is one atomic value matching the documented state machine.
- Positive: the new structs carry a `struct_size` guard and an append-only rule, so future composition fields are additive — no further break.
- Positive: hosts coalesce composition updates once per loop iteration and diff against last-sent before any protocol write or repaint, without engines needing to be "smart" about change detection.
- Trade-off: engines that previously interleaved `set_preedit` / `set_candidates` must build a full composition value per emit. In practice this is a Rime-style "pull current state and push" function — simpler, not harder.

## Related

- [Composition State Machine](../explanation/composition-state-machine.md) — the abstract state model
- [Engine Contract](../explanation/engine-contract.md) — engine↔framework boundary in depth
- [ADR-0003: Plugin engine ABI — dual-category slots](0003-plugin-engine-abi-dual-category.md) — the surrounding engine ABI
