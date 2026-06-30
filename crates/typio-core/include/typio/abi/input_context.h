/**
 * @file input_context.h
 * @brief Input context for managing input state per client
 */

#ifndef TYPIO_INPUT_CONTEXT_H
#define TYPIO_INPUT_CONTEXT_H

#include "typio/abi/types.h"

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief Preedit text formatting
 */
typedef enum {
    TYPIO_PREEDIT_NONE = 0,
    TYPIO_PREEDIT_UNDERLINE = (1 << 0),
    TYPIO_PREEDIT_HIGHLIGHT = (1 << 1),
    TYPIO_PREEDIT_BOLD = (1 << 2),
    TYPIO_PREEDIT_ITALIC = (1 << 3),
} TypioPreeditFormat;

/**
 * @brief Preedit segment
 */
typedef struct TypioPreeditSegment {
    const char *text;           /* Segment text */
    uint32_t format;            /* Format flags */
} TypioPreeditSegment;

/**
 * @brief Preedit text structure
 */
struct TypioPreedit {
    TypioPreeditSegment *segments;  /* Array of segments */
    size_t segment_count;           /* Number of segments */
    int cursor_pos;                 /* Cursor position in characters */
};

/**
 * @brief Single candidate entry
 */
struct TypioCandidate {
    const char *text;           /* Candidate text */
    const char *comment;        /* Optional comment/annotation */
    const char *label;          /* Optional label (e.g., "1", "a") */
};

/**
 * @brief Host-managed candidate selection flags (ADR-0012).
 *
 * Engine declares which selection operations the host should intercept.
 * Zero means engine-managed (host does not intercept any selection keys).
 */
typedef enum {
    TYPIO_HOST_SEL_NONE       = 0,
    TYPIO_HOST_SEL_NAVIGATE   = (1 << 0),  /* Up/Down/Left/Right */
    TYPIO_HOST_SEL_COMMIT     = (1 << 1),  /* Space */
    TYPIO_HOST_SEL_INDEX_PICK = (1 << 2),  /* 0–9 */
    TYPIO_HOST_SEL_COMMIT_RAW = (1 << 3),  /* Enter / KP_Enter — commit preedit as-is */
    TYPIO_HOST_SEL_ALL        = 0xF,
} TypioHostManagedSelection;

/**
 * @brief Atomic composition snapshot: preedit + candidates as one value.
 *
 * ADR-0006. The engine emits the whole in-flight composition in one
 * transactional call (`typio_input_context_set_composition`); the context is
 * never half-updated. An empty composition (segment_count == 0 &&
 * candidate_count == 0) is the Idle state. All pointers are borrowed and valid
 * only for the call/callback duration; the receiver copies what it retains.
 * `struct_size` carries the author's `sizeof` for append-only ABI evolution.
 * Offsets (`cursor_pos`, `selected`) count Unicode scalar values, not bytes.
 */
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
    uint64_t content_signature;     /* stable; excludes `selected` */
    uint64_t revision;              /* monotonic per context */
    uint32_t host_managed_selection; /* TypioHostManagedSelection flags */
};

/**
 * @brief Input context capabilities/hints
 */
typedef enum {
    TYPIO_CTX_CAP_PREEDIT = (1 << 0),       /* Client supports preedit */
    TYPIO_CTX_CAP_SURROUNDING = (1 << 1),   /* Client provides surrounding text */
    TYPIO_CTX_CAP_PASSWORD = (1 << 2),      /* Password input mode */
    TYPIO_CTX_CAP_MULTILINE = (1 << 3),     /* Multiline text input */
} TypioContextCapability;

/* Input context lifecycle */
TypioInputContext *typio_input_context_new(TypioInstance *instance);
void typio_input_context_free(TypioInputContext *ctx);

/* Focus management */
void typio_input_context_focus_in(TypioInputContext *ctx);
void typio_input_context_focus_out(TypioInputContext *ctx);
bool typio_input_context_is_focused(TypioInputContext *ctx);

/* Reset state */
void typio_input_context_reset(TypioInputContext *ctx);

/* Event processing */
bool typio_input_context_process_key(TypioInputContext *ctx,
                                      const TypioKeyEvent *event);

/* Set the active keyboard engine's mode for this context.
 * `mode_id` is a previously reported TypioKeyboardEngineMode::id. Returns
 * TYPIO_ERROR_NOT_FOUND when no active keyboard, no set_active_mode, or rejected. */
TypioResult typio_input_context_set_active_mode(TypioInputContext *ctx,
                                                 const char *mode_id);

/* Commit text to client (one-shot event; clears the composition) */
void typio_input_context_commit(TypioInputContext *ctx, const char *text);

/* Composition (preedit + candidates) — set atomically, ADR-0006.
 * An empty composition is the Idle state; `clear` is the convenience for it. */
void typio_input_context_set_composition(TypioInputContext *ctx,
                                         const TypioComposition *composition);
void typio_input_context_clear(TypioInputContext *ctx);

/* Read projection of the stored preedit. Candidates are only delivered via
 * the composition callback (see `TypioComposition` above). */
const TypioPreedit *typio_input_context_get_preedit(TypioInputContext *ctx);

/* Host-managed candidate selection (ADR-0012).
 * Dispatches commit_candidate to the active keyboard engine via the input
 * context. Returns TYPIO_ERROR_NOT_SUPPORTED if the engine does not implement
 * commit_candidate or no engine is active. */
TypioResult typio_input_context_commit_candidate(TypioInputContext *ctx,
                                                  int candidate_index);

/* Surrounding text */
void typio_input_context_set_surrounding(TypioInputContext *ctx,
                                          const char *text,
                                          int cursor_pos,
                                          int anchor_pos);
bool typio_input_context_get_surrounding(TypioInputContext *ctx,
                                          const char **text,
                                          int *cursor_pos,
                                          int *anchor_pos);
/* Request the client delete `before` UTF-8 bytes preceding the cursor and
 * `after` bytes following it (Wayland text-input v3 delete_surrounding_text). */
void typio_input_context_delete_surrounding(TypioInputContext *ctx,
                                             uint32_t before, uint32_t after);

/* Context capabilities */
void typio_input_context_set_capabilities(TypioInputContext *ctx, uint32_t caps);
uint32_t typio_input_context_get_capabilities(TypioInputContext *ctx);

/* Callbacks */
void typio_input_context_set_commit_callback(TypioInputContext *ctx,
                                              TypioCommitCallback callback,
                                              void *user_data);
void typio_input_context_set_composition_callback(TypioInputContext *ctx,
                                                  TypioCompositionCallback callback,
                                                  void *user_data);
void typio_input_context_set_delete_surrounding_callback(TypioInputContext *ctx,
                                                  TypioDeleteSurroundingCallback callback,
                                                  void *user_data);

/* User data */
void typio_input_context_set_user_data(TypioInputContext *ctx, void *data);
void *typio_input_context_get_user_data(TypioInputContext *ctx);

/* Engine-specific property storage */
void typio_input_context_set_property(TypioInputContext *ctx,
                                       const char *key, void *value,
                                       void (*free_func)(void *));
void *typio_input_context_get_property(TypioInputContext *ctx, const char *key);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_INPUT_CONTEXT_H */
