# Input Context Reference

`TypioInputContext` carries per-client input state: focus, key processing,
composition (preedit + candidates), surrounding text, and engine-private
property storage.

Header: `typio/abi/input_context.h`. Shared types in [types.md](types.md).

## Lifecycle

```c
TypioInputContext *typio_input_context_new(TypioInstance *instance);
void               typio_input_context_free(TypioInputContext *ctx);
```

Hosts almost always create contexts through the instance helpers
(`typio_instance_create_context` / `_destroy_context` — see
[instance.md](instance.md#input-contexts)) so the instance can track focus
correctly. The bare `_new` / `_free` pair is exposed for test code and
specialised hosts.

## Focus

```c
void typio_input_context_focus_in (TypioInputContext *ctx);
void typio_input_context_focus_out(TypioInputContext *ctx);
bool typio_input_context_is_focused(TypioInputContext *ctx);
void typio_input_context_reset    (TypioInputContext *ctx);
```

| Function | Effect |
|----------|--------|
| `focus_in` | Mark `ctx` as the focused context. Triggers the active engine's `focus_in` op. |
| `focus_out` | Clear focus. Triggers the active engine's `focus_out` op; the engine MUST preserve session state. |
| `is_focused` | `true` if `ctx` currently holds focus on its instance |
| `reset` | Cancel composition, restore default mode. Triggers the engine's `reset` op. |

## Key processing

```c
bool typio_input_context_process_key(TypioInputContext *ctx,
                                     const TypioKeyEvent *event);
```

| Aspect | Detail |
|--------|--------|
| Return | `true` if the key was intercepted; `false` if it should be forwarded to the client application |
| Precondition | Host MUST initialise `event->struct_size = sizeof(TypioKeyEvent)` |
| Underlying | The engine returns `TypioKeyProcessResult`; the host-facing `bool` collapses every "intercepted" variant to `true` |

### `TypioKeyProcessResult`

Used by engines internally; exposed here for completeness of the contract.

```c
typedef enum {
    TYPIO_KEY_NOT_HANDLED = 0,
    TYPIO_KEY_HANDLED     = 1,
    TYPIO_KEY_COMPOSING   = 2,
    TYPIO_KEY_COMMITTED   = 3,
} TypioKeyProcessResult;
```

| Value | Meaning | `process_key` returns |
|-------|---------|------------------------|
| `NOT_HANDLED` | Engine does not consume the key | `false` |
| `HANDLED` | Engine consumed it (e.g. internal navigation), no composition change | `true` |
| `COMPOSING` | Engine consumed it and updated composition state | `true` |
| `COMMITTED` | Engine consumed it and committed text via `typio_input_context_commit` | `true` |

## Composition

Composition is one **transactional** value ([ADR-0006](../../adr/0006-composition-state-and-commit-event.md)). Engines emit the entire
in-flight composition via `set_composition`; the framework delivers it to the
registered `TypioCompositionCallback`.

```c
void typio_input_context_set_composition(TypioInputContext *ctx,
                                         const TypioComposition *composition);
void typio_input_context_clear          (TypioInputContext *ctx);
const TypioPreedit *typio_input_context_get_preedit(TypioInputContext *ctx);
```

| Function | Notes |
|----------|-------|
| `set_composition` | Engine writes the whole snapshot. Empty composition (`segment_count == 0 && candidate_count == 0`) is the Idle state. |
| `clear` | Convenience for emitting the empty composition |
| `get_preedit` | Host read-projection of the stored preedit. Candidates are delivered only via the composition callback. Borrowed pointer; valid until the next composition mutation. |

### `TypioPreeditFormat`

```c
typedef enum {
    TYPIO_PREEDIT_NONE      = 0,
    TYPIO_PREEDIT_UNDERLINE = (1 << 0),
    TYPIO_PREEDIT_HIGHLIGHT = (1 << 1),
    TYPIO_PREEDIT_BOLD      = (1 << 2),
    TYPIO_PREEDIT_ITALIC    = (1 << 3),
} TypioPreeditFormat;
```

Bitmask. Combine with bitwise OR in `TypioPreeditSegment.format`.

### `TypioPreeditSegment`

```c
typedef struct TypioPreeditSegment {
    const char *text;    /* UTF-8 segment text, borrowed */
    uint32_t    format;  /* TypioPreeditFormat bitmask */
} TypioPreeditSegment;
```

### `TypioPreedit`

```c
struct TypioPreedit {
    TypioPreeditSegment *segments;
    size_t               segment_count;
    int                  cursor_pos;   /* in Unicode scalar values */
};
```

### `TypioCandidate`

```c
struct TypioCandidate {
    const char *text;     /* UTF-8 candidate text */
    const char *comment;  /* Optional comment/annotation, may be NULL */
    const char *label;    /* Optional label (e.g. "1", "a"), may be NULL */
};
```

### `TypioComposition`

```c
struct TypioComposition {
    size_t                       struct_size;

    /* preedit */
    const TypioPreeditSegment   *segments;
    size_t                       segment_count;
    int                          cursor_pos;

    /* candidates */
    const TypioCandidate        *candidates;
    size_t                       candidate_count;
    int                          page;
    int                          page_size;
    int                          total;
    int                          selected;
    bool                         has_prev;
    bool                         has_next;
    uint64_t                     content_signature;
    uint64_t                     revision;
};
```

| Field | Meaning |
|-------|---------|
| `struct_size` | `sizeof(TypioComposition)` at engine build time. Append-only ABI evolution. |
| `segments`, `segment_count` | Preedit segment array; borrowed for the call only |
| `cursor_pos` | Cursor position in **Unicode scalar values**, not bytes |
| `candidates`, `candidate_count` | Candidate array for the current page |
| `page`, `page_size`, `total` | Pagination over the full candidate set |
| `selected` | Selected candidate index in **Unicode scalar values** (highlighted in the popup) |
| `has_prev`, `has_next` | Pagination affordances |
| `content_signature` | Stable hash of the content; **excludes** `selected`. Hosts use it for popup re-rendering. |
| `revision` | Monotonic per-context counter; bumped on every `set_composition` call |

All pointer fields are borrowed for the duration of the
`set_composition` call (and the callback dispatch that follows). The
receiver MUST copy anything it retains.

## Commit

```c
void typio_input_context_commit(TypioInputContext *ctx, const char *text);
```

One-shot event: fires the commit callback with a copy of `text`, then clears
the composition. `text` is borrowed only for the call.

## Surrounding text

```c
void typio_input_context_set_surrounding(TypioInputContext *ctx,
                                         const char *text,
                                         int cursor_pos,
                                         int anchor_pos);
bool typio_input_context_get_surrounding(TypioInputContext *ctx,
                                         const char **text,
                                         int *cursor_pos,
                                         int *anchor_pos);
void typio_input_context_delete_surrounding(TypioInputContext *ctx,
                                            int offset, int length);
```

| Function | Behaviour |
|----------|-----------|
| `set_surrounding` | Host writes the surrounding text snapshot. `text` is copied. Offsets in Unicode scalar values. |
| `get_surrounding` | Returns `true` if surrounding text is available. `*text` is borrowed from `ctx`; valid until the next `set_surrounding`. |
| `delete_surrounding` | Engine requests deletion of `length` scalar values starting at `offset` relative to the cursor. Host applies it to the client. |

## Capabilities

```c
typedef enum {
    TYPIO_CTX_CAP_PREEDIT     = (1 << 0),  /* Client supports preedit */
    TYPIO_CTX_CAP_SURROUNDING = (1 << 1),  /* Client provides surrounding text */
    TYPIO_CTX_CAP_PASSWORD    = (1 << 2),  /* Password input mode */
    TYPIO_CTX_CAP_MULTILINE   = (1 << 3),  /* Multiline text input */
} TypioContextCapability;

void     typio_input_context_set_capabilities(TypioInputContext *ctx, uint32_t caps);
uint32_t typio_input_context_get_capabilities(TypioInputContext *ctx);
```

Bitmask. Engines query `get_capabilities` to adapt behaviour (e.g. suppress
preedit when `PREEDIT` is clear, refuse learning when `PASSWORD` is set).

## Callbacks

```c
void typio_input_context_set_commit_callback     (TypioInputContext *ctx,
                                                  TypioCommitCallback callback,
                                                  void *user_data);
void typio_input_context_set_composition_callback(TypioInputContext *ctx,
                                                  TypioCompositionCallback callback,
                                                  void *user_data);
```

Callback signatures and lifetime rules are in [types.md ▸ Callback typedefs](types.md#callback-typedefs).

## User data

```c
void  typio_input_context_set_user_data(TypioInputContext *ctx, void *data);
void *typio_input_context_get_user_data(TypioInputContext *ctx);
```

Opaque host pointer. The framework never dereferences it.

## Engine-side property storage

```c
void  typio_input_context_set_property(TypioInputContext *ctx,
                                       const char *key, void *value,
                                       void (*free_func)(void *));
void *typio_input_context_get_property(TypioInputContext *ctx, const char *key);
```

| Aspect | Detail |
|--------|--------|
| Purpose | Engine-private per-context state (e.g. a Rime session handle) |
| Key namespacing | Each engine uses its own prefix (e.g. `"rime.session"`) to avoid clashes |
| `free_func` | Invoked on context destruction or when the key is overwritten. May be NULL to disable cleanup. |

## See also

- [Event](event.md) — `TypioKeyEvent`, `TYPIO_KEY_*` constants, modifier helpers
- [Shared types](types.md) — callback typedefs, opaque handles, `TypioResult`
- [Instance ▸ Input contexts](instance.md#input-contexts) — host-side context creation
- [Engine ▸ Operations](../engine/ops.md) — `TypioKeyboardEngineOps::process_key`, `TypioEngineBaseOps::focus_in/out/reset`
- [Composition state machine](../../explanation/composition-state-machine.md) — design rationale
