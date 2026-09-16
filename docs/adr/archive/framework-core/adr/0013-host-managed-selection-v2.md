# ADR-0013: Host-managed candidate selection — amended flags

- **Status**: Accepted
- **Date**: 2026-06-02
- **Deciders**: Project maintainers
- **Supersedes**: [ADR-0012](0012-host-managed-candidate-selection.md)
- **Relates**: [ADR-0006](0006-composition-state-and-commit-event.md)

## Context

ADR-0012 introduced host-managed candidate selection with three flag bits
(`NAVIGATE`, `COMMIT`, `INDEX_PICK`). Two gaps have emerged since acceptance:

1. **`COMMIT` overloaded Space and Enter.** Engines need Space to commit the
   selected candidate but Enter to commit the *raw* preedit buffer (e.g. the
   user typed `a` and wants the literal `a`, not `á`). A single `COMMIT` flag
   cannot express both semantics.

2. **`INDEX_PICK` mapped `1`–`9` to index 0–8.** Key `0` was excluded. Most
   IME UIs label the 10th candidate with `0`, so users expect `0` to pick
   index 9.

Additionally, the original `TYPIO_HOST_SEL_ALL = 0x7` was calculated for three
flags and must be `0xF` to cover the fourth flag.

## Decision

### 1. Split `COMMIT` into `COMMIT` (Space) and `COMMIT_RAW` (Enter)

```c
typedef enum {
    TYPIO_HOST_SEL_NONE       = 0,
    TYPIO_HOST_SEL_NAVIGATE   = (1 << 0),  /* Up/Down/Left/Right */
    TYPIO_HOST_SEL_COMMIT     = (1 << 1),  /* Space */
    TYPIO_HOST_SEL_INDEX_PICK = (1 << 2),  /* 0–9 */
    TYPIO_HOST_SEL_COMMIT_RAW = (1 << 3),  /* Enter / KP_Enter */
    TYPIO_HOST_SEL_ALL        = 0xF,
} TypioHostManagedSelection;
```

- `TYPIO_HOST_SEL_COMMIT` now covers **Space only**. The host commits the
  currently selected candidate via `commit_candidate(ctx, selected_index)`.
- `TYPIO_HOST_SEL_COMMIT_RAW` (new) covers **Enter / KP_Enter**. The host
  commits the raw preedit buffer text via `typio_input_context_commit`,
  bypassing `commit_candidate`.

Engines that want both Space and Enter to commit the selected candidate
(i.e. no raw-commit concept) should set only `COMMIT`.

### 2. Extend `INDEX_PICK` to include `0`

`TYPIO_HOST_SEL_INDEX_PICK` now covers digit keys `0`–`9`:

| Key | Candidate index |
|-----|-----------------|
| `1` | 0 |
| `2` | 1 |
| `3` | 2 |
| `4` | 3 |
| `5` | 4 |
| `6` | 5 |
| `7` | 6 |
| `8` | 7 |
| `9` | 8 |
| `0` | 9 |

### 3. UX contract — host-intercepted keys (amended)

| Flag | Key | Action |
|------|-----|--------|
| `NAVIGATE` | `Up` / `Left` | Decrement selected (min 0), update visual |
| `NAVIGATE` | `Down` / `Right` | Increment selected (max `candidate_count - 1`), update visual |
| `COMMIT` | `Space` | Commit currently selected candidate via `commit_candidate` |
| `COMMIT_RAW` | `Enter` / `KP_Enter` | Commit raw preedit buffer via `typio_input_context_commit` |
| `INDEX_PICK` | `1` – `9` | Commit candidate at index 0–8 via `commit_candidate` |
| `INDEX_PICK` | `0` | Commit candidate at index 9 via `commit_candidate` |
| — | `Escape` | **Forwarded** to `process_key` — engine handles cleanup |

### 4. `commit_candidate` contract (clarified)

The host calls `commit_candidate(engine, ctx, index)` when the user selects a
candidate. The engine:

1. Retrieves the candidate text for the given index.
2. Commits it via `typio_input_context_commit`.

The engine does **not** need to call `typio_input_context_clear` before
committing — `typio_input_context_commit` already clears preedit and candidates
atomically.

## Alternatives considered

- **Keep `COMMIT` for both Space and Enter.** Rejected: engines cannot
  distinguish "commit selected" from "commit raw preedit" without separate
  flags.

- **Map `0` to index 0 instead of index 9.** Rejected: index 0 is already
  covered by key `1`. Key `0` at the right end of the number row maps
  naturally to the 10th position, consistent with most IME implementations.

## Consequences

- Positive: engines can now opt into raw-commit semantics independently of
  space-commit, enabling compose pickers and IMEs where Enter commits literal
  input.

- Positive: the full digit row (0–9) is available for candidate selection,
  matching user expectations from other IME frameworks.

- Trade-off: existing implementations that set `TYPIO_HOST_SEL_COMMIT` and
  expected Enter to commit the selected candidate must now also set
  `TYPIO_HOST_SEL_COMMIT_RAW` if they want that behaviour.
