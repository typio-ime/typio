## ADR-0012: Host-managed candidate selection

- **Status**: Superseded by ADR-0013
- **Date**: 2026-06-02
- **Deciders**: Project maintainers
- **Supersedes**: None
- **Relates**: [ADR-0006](0006-composition-state-and-commit-event.md)

## Context

Every keyboard engine currently implements its own candidate selection UX inside
`process_key`. The basic engine handles arrows, 1-9, space, enter, escape.
Rime delegates to librime which has its own deeply integrated selection model.

This has three problems:

1. **UX inconsistency.** Each engine may implement different selection semantics
   (different number key ranges, different behaviour for space/enter, different
   navigation wrapping). Users cannot build transferable muscle memory.

2. **Duplicated effort.** Every new engine must re-implement the same picker
   logic: candidate navigation, selection by index, commit, cancel. This is
   boilerplate that belongs in the framework, not in each engine.

3. **No framework-level UX evolution.** Changing candidate interaction (e.g.
   adding Tab to cycle, changing number key mapping, adding fuzzy search)
   requires updating every engine independently.

The framework already has partial host-side candidate infrastructure:
`candidate_guard` intercepts arrow keys when candidates are visible,
`TypioWlSession` stores selection state, and the panel renders candidates with
highlighting. But the guard only *reserves* keys — it doesn't act on them.

### What we need

- A way for engines to **opt in** to host-managed selection on a per-composition
  basis.
- A clear **UX contract** defining which keys the host intercepts and what
  actions they perform.
- A **callback** for the host to tell the engine which candidate was selected.
- Engines with deeply embedded selection (Rime/librime) continue to handle
  selection internally — no migration required.

## Decision

### 1. Add `host_managed_selection` flags to `TypioComposition`

```c
typedef enum {
    TYPIO_HOST_SEL_NONE       = 0,
    TYPIO_HOST_SEL_NAVIGATE   = (1 << 0),  /* Up/Down/Left/Right */
    TYPIO_HOST_SEL_COMMIT     = (1 << 1),  /* Enter / Space */
    TYPIO_HOST_SEL_INDEX_PICK = (1 << 2),  /* 1–9 */
    TYPIO_HOST_SEL_ALL        = 0x7,
} TypioHostManagedSelection;

struct TypioComposition {
    size_t struct_size;
    /* preedit */
    const TypioPreeditSegment *segments;
    size_t segment_count;
    int cursor_pos;
    /* candidates */
    const TypioCandidate *candidates;
    size_t candidate_count;
    int page;
    int page_size;
    int total;
    int selected;
    bool has_prev;
    bool has_next;
    uint64_t content_signature;
    uint64_t revision;
    uint32_t host_managed_selection; /* TypioHostManagedSelection flags */
};
```

The `host_managed_selection` field is a bit-mask of `TypioHostManagedSelection`.
When a bit is set, the host intercepts the corresponding keys (see §3 below).
When `TYPIO_HOST_SEL_NONE` (the default, zero-initialised), the host does not
intercept any selection keys — the engine handles everything through
`process_key`. Engines may opt in to individual capabilities rather than
accepting the entire host-managed UX contract.

### 2. Add `commit_candidate` to `TypioKeyboardEngineOps`

```c
typedef struct TypioKeyboardEngineOps {
    TypioKeyProcessResult (*process_key)(TypioKeyboardEngine *, TypioInputContext *, const void *);
    const TypioKeyboardEngineMode *(*list_modes)(TypioKeyboardEngine *, size_t *);
    const TypioKeyboardEngineMode *(*get_active_mode)(TypioKeyboardEngine *, TypioInputContext *);
    TypioResult (*set_active_mode)(TypioKeyboardEngine *, TypioInputContext *, const char *);
    TypioResult (*commit_candidate)(TypioKeyboardEngine *, TypioInputContext *, int candidate_index);
} TypioKeyboardEngineOps;
```

The host calls `commit_candidate(engine, ctx, index)` when the user selects a
candidate via a number key, space, or enter. The engine:

1. Retrieves the candidate text for the given index.
2. Commits it via `typio_input_context_commit`.
3. Clears the composition via `typio_input_context_clear`.
4. Returns `TypioOk` on success.

Engines that do not use host-managed selection set this to `NULL`.

### 3. UX contract — host-intercepted keys

When `host_managed_selection` has one or more flags set and candidates are
visible (`candidate_count > 0`), the host intercepts the corresponding keys
**before** they reach `process_key`:

| Flag | Key | Action |
|------|-----|--------|
| `TYPIO_HOST_SEL_NAVIGATE` | `Up` / `Left` | Decrement selected (min 0), update visual |
| `TYPIO_HOST_SEL_NAVIGATE` | `Down` / `Right` | Increment selected (max `candidate_count - 1`), update visual |
| `TYPIO_HOST_SEL_COMMIT` | `Space` | Commit currently selected candidate via `commit_candidate` |
| `TYPIO_HOST_SEL_COMMIT` | `Enter` / `KP_Enter` | Commit currently selected candidate via `commit_candidate` |
| `TYPIO_HOST_SEL_INDEX_PICK` | `1` – `9` | Commit candidate at index 0–8 via `commit_candidate` |
| — | `Escape` | **Forwarded** to `process_key` — engine handles cleanup |

All other keys — including `Escape`, `BackSpace`, printable input, and modifiers
— are forwarded to `process_key` as usual. The engine uses these to update its
internal buffer and re-publish the composition with updated candidates.

Engines whose preedit input domain overlaps with selection keys (e.g. a compose
picker that accepts digits or space as part of the trigger sequence) should
omit the conflicting flags rather than disabling host-managed selection
entirely. For example, a symbol-search engine might set only
`TYPIO_HOST_SEL_NAVIGATE` so that digits and space remain available as search
input while the host still handles arrow-key navigation.

### 4. Selected index management

The host maintains the selected index locally in the session. Rules:

- **New composition or changed candidates** (`content_signature` differs): the
  host reads the engine-provided `selected` field as the initial value, then
  takes over management.
- **Navigation**: the host updates `last_candidate_selected` and re-renders the
  panel without engine involvement.
- **Commit**: the host passes its locally-maintained index to
  `commit_candidate`.
- **Engine updates composition** (e.g. after BackSpace): the host detects the
  new `content_signature`, resets to the engine's `selected` field, and
  continues managing from there.

### 5. Pagination (deferred)

For the initial implementation, the engine provides all candidates in one
composition (`page = 0`, `page_size = candidate_count`, `total =
candidate_count`). The host renders up to a configurable page size and handles
scrolling within the candidate list.

Server-side pagination (engine provides candidates page-by-page) is a future
extension. When needed, the host would call back to the engine to request a
different page, and the `TypioComposition` already has `page`, `page_size`,
`total`, `has_prev`, `has_next` fields to support this.

### 6. Default: engine-managed (no change)

If `host_managed_selection` is `TYPIO_HOST_SEL_NONE` — the zero-initialised
default — the host does not intercept any keys for candidate navigation. The
engine handles everything through `process_key`. This is the existing behaviour
and is the correct choice for engines where candidate selection is deeply
embedded in the processing pipeline (e.g. Rime/librime).

## Alternatives considered

- **Always host-managed.** Rejected: librime's `RimeProcessKey` simultaneously
  produces candidates and handles selection internally. These two operations
  cannot be separated without a major rewrite of the rime engine's architecture.

- **Synthetic key events through `process_key`.** Rejected: ambiguous — when the
  host sends '1' through `process_key`, the engine cannot distinguish "user
  pressed 1 to filter" from "user pressed 1 to select candidate 1". A separate
  callback eliminates this ambiguity.

- **Per-engine configuration (engine metadata flag).** Rejected: a per-composition
  flag is more flexible. An engine could use host-managed selection for a browse
  mode and engine-managed for a direct-input mode. The composition is the right
  granularity.

- **Host calls `typio_input_context_commit` directly, bypassing engine.**
  Rejected: the engine may need to perform side effects on commit (updating
  frequency data, logging, state transitions). The `commit_candidate` callback
  gives the engine control over the commit lifecycle.

## Consequences

- Positive: new engines get a consistent, framework-provided picker UX for free.
  Engine authors focus on producing candidates, not implementing navigation.

- Positive: UX changes (e.g. adding Tab cycling, changing number key mapping,
  adding pointer/touch selection) only need to be made in one place — the host.

- Positive: the basic engine's picker logic simplifies. The `ComposePicker`
  state machine no longer needs to handle arrows or enter/space when those
  flags are set, but it retains control over digits if it sets only
  `TYPIO_HOST_SEL_NAVIGATE | TYPIO_HOST_SEL_COMMIT`.

- Trade-off: `commit_candidate` is a new engine op. Engines that use
  host-managed selection must implement it. Engines that don't set it to `NULL`.
  The `struct_size` guard in `TypioKeyboardEngineOps` protects against older
  engines that don't have this field.

- Negative (accepted): the host and engine must agree on index semantics — the
  index refers to the position in the `candidates` array of the composition
  snapshot that was current when the user triggered selection. If the engine
  updates the composition between keypress and commit, the index may refer to a
  different candidate. The host mitigates this by committing synchronously
  within the key event handler before any async composition updates.
