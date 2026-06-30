/**
 * @file types.h
 * @brief Common types and definitions for Typio
 */

#ifndef TYPIO_TYPES_H
#define TYPIO_TYPES_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief Export annotation for symbols a plugin or host wants visible to
 *        dlopen consumers.
 *
 * Plugins built with `-fvisibility=hidden` (recommended) use this on every
 * entry point — the `TYPIO_*_ENGINE_DEFINE` macros apply it automatically.
 */
#ifndef TYPIO_EXPORT
#  if defined(_WIN32) || defined(__CYGWIN__)
#    define TYPIO_EXPORT __declspec(dllexport)
#  elif defined(__GNUC__) || defined(__clang__)
#    define TYPIO_EXPORT __attribute__((visibility("default")))
#  else
#    define TYPIO_EXPORT
#  endif
#endif

/* Forward declarations */
typedef struct TypioInstance TypioInstance;
typedef struct TypioEngine TypioEngine;
typedef struct TypioEngineInfo TypioEngineInfo;
typedef struct TypioInputContext TypioInputContext;
typedef struct TypioEvent TypioEvent;
typedef struct TypioKeyEvent TypioKeyEvent;
typedef struct TypioConfig TypioConfig;
typedef struct TypioCandidate TypioCandidate;
typedef struct TypioPreedit TypioPreedit;
typedef struct TypioComposition TypioComposition;

/* Result codes */
typedef enum {
    TYPIO_OK = 0,
    TYPIO_ERROR = -1,
    TYPIO_ERROR_INVALID_ARGUMENT = -2,
    TYPIO_ERROR_OUT_OF_MEMORY = -3,
    TYPIO_ERROR_NOT_FOUND = -4,
    TYPIO_ERROR_ALREADY_EXISTS = -5,
    TYPIO_ERROR_NOT_INITIALIZED = -6,
    TYPIO_ERROR_ENGINE_LOAD_FAILED = -7,
    TYPIO_ERROR_ENGINE_NOT_AVAILABLE = -8,
} TypioResult;

/* Engine types */
typedef enum {
    TYPIO_ENGINE_TYPE_KEYBOARD = 0,    /* Standard keyboard input */
    TYPIO_ENGINE_TYPE_VOICE = 1,       /* Voice input */
    TYPIO_ENGINE_TYPE_HANDWRITING = 2, /* Handwriting recognition */
    TYPIO_ENGINE_TYPE_CUSTOM = 100,    /* Custom engine types start here */
} TypioEngineType;

/*
 * Engine capabilities are now declared as NULL-terminated arrays of
 * strings — see TypioEngineInfo.required_capabilities / optional_capabilities
 * in engine.h.  The host advertises a set of supported capability names and
 * rejects an engine whose required set is not a subset.
 *
 * Standard capability names (case-sensitive, snake_case):
 *
 *   "preedit"           — engine emits preedit text
 *   "candidates"        — engine emits a candidate list (host renders popup)
 *   "prediction"        — engine emits predictive candidates
 *   "voice_input"       — engine consumes audio buffers
 *   "continuous_voice"  — engine handles streaming audio
 *   "punctuation"       — engine performs auto-punctuation
 *   "learning"          — engine maintains a user dictionary
 *
 * Engines may declare additional capability strings; the host treats
 * unknown names as unsupported (and rejects the engine if any unknown
 * capability is in its required set).
 */

/* Event types */
typedef enum {
    TYPIO_EVENT_KEY_PRESS = 0,
    TYPIO_EVENT_KEY_RELEASE = 1,
    TYPIO_EVENT_FOCUS_IN = 2,
    TYPIO_EVENT_FOCUS_OUT = 3,
    TYPIO_EVENT_RESET = 4,
    TYPIO_EVENT_VOICE_START = 5,
    TYPIO_EVENT_VOICE_END = 6,
    TYPIO_EVENT_VOICE_DATA = 7,
    TYPIO_EVENT_COMMIT = 8,
    TYPIO_EVENT_CANDIDATE_SELECT = 9,
} TypioEventType;

/* Key processing result - explicitly modeling Interception, Composition, Commit */
typedef enum {
    TYPIO_KEY_NOT_HANDLED = 0, /* Not intercepted - pass to client application */
    TYPIO_KEY_HANDLED = 1,     /* Intercepted - handled internally (e.g. navigation) */
    TYPIO_KEY_COMPOSING = 2,   /* Intercepted - composition/preedit state updated */
    TYPIO_KEY_COMMITTED = 3,   /* Intercepted - text was committed */
} TypioKeyProcessResult;

/* Engine availability: the lifecycle axis answering "can the engine work at
 * all?", orthogonal to engagement (ADR-0009/0010) and mode (ADR-0011).
 * See ADR-0014. An engine with no async warm-up never leaves READY and may
 * omit the availability op entirely (NULL means READY). The host MUST NOT route
 * input to an engine that is not READY. */
typedef enum {
    TYPIO_ENGINE_UNINITIALIZED = 0, /* Created, init() not yet completed. */
    TYPIO_ENGINE_PREPARING = 1,     /* Async warm-up in progress; not routable. */
    TYPIO_ENGINE_READY = 2,         /* Can process input; routable. */
    TYPIO_ENGINE_FAILED = 3,        /* Warm-up failed; not routable; host falls back. */
} TypioEngineAvailability;

/* Key modifiers */
typedef enum {
    TYPIO_MOD_NONE = 0,
    TYPIO_MOD_SHIFT = (1 << 0),
    TYPIO_MOD_CTRL = (1 << 1),
    TYPIO_MOD_ALT = (1 << 2),
    TYPIO_MOD_SUPER = (1 << 3),
    TYPIO_MOD_CAPSLOCK = (1 << 4),
    TYPIO_MOD_NUMLOCK = (1 << 5),
} TypioModifier;

/* Announcement salience: should this keyboard state *auto-reveal* on an
 * incidental focus (the host's on-focus indicator)? This governs ONLY the
 * unprompted reveal. A deliberate, user-initiated change — engine switch,
 * mode change — always earns confirmation feedback regardless of salience.
 *
 * The engine classifies *meaning* — only it knows if a mode is surprising.
 * The host owns *when* to reveal (recency, focus churn, candidate UI, config).
 *
 * Contract: the engine sets a *ceiling* on the auto-reveal. The host may only
 * lower it (suppress), never raise it. */
typedef enum {
    TYPIO_STATUS_SALIENCE_QUIET = 0,   /* Behaves like the user's home keyboard:
                                           never announce unprompted. */
    TYPIO_STATUS_SALIENCE_NOTABLE = 1, /* Could surprise if typed into blind
                                           (native-script composing): worth a
                                           brief, suppressible announcement. */
} TypioStatusSalience;

/**
 * @brief A named, user-facing engine mode (ADR-0011).
 *
 * Mode is a first-class concept: engines declare their modes and notify the
 * framework when the active mode changes.
 *
 * Engines produce this via TypioKeyboardEngineOps.list_modes /
 * get_active_mode. The host observes via
 * typio_instance_set_keyboard_mode_changed_callback.
 *
 * Identity for change detection is @c id only. All other fields are
 * presentation / metadata that ride alongside.
 */
typedef struct TypioKeyboardEngineMode {
    const char             *id;             /* "native", "ascii", "browse" */
    const char             *label;          /* "Native", "ASCII", "Browse" */
    const char             *display_label;  /* Short indicator badge: "中", "A", "Browse" */
    const char             *icon_name;      /* freedesktop icon name */

    /* Profile — engine-defined active profile (e.g. Rime schema). */
    const char             *profile_id;     /* "luna_pinyin", "wubi86" */
    const char             *profile_label;  /* "朙月拼音", "五笔86" */
    const char             *description;    /* Optional detailed description */

    TypioStatusSalience    salience;        /* Announcement salience ceiling */
} TypioKeyboardEngineMode;

/* Callback types */
typedef void (*TypioCommitCallback)(TypioInputContext *ctx, const char *text, void *user_data);
typedef void (*TypioCompositionCallback)(TypioInputContext *ctx, const TypioComposition *composition, void *user_data);
/* Delete text around the cursor: `before` UTF-8 bytes preceding it and `after`
 * UTF-8 bytes following it (Wayland text-input v3 delete_surrounding_text). */
typedef void (*TypioDeleteSurroundingCallback)(TypioInputContext *ctx, uint32_t before, uint32_t after, void *user_data);
typedef void (*TypioEngineChangedCallback)(TypioInstance *instance, const TypioEngineInfo *engine, void *user_data);
typedef void (*TypioVoiceEngineChangedCallback)(TypioInstance *instance, const TypioEngineInfo *engine, void *user_data);
typedef void (*TypioStatusIconChangedCallback)(TypioInstance *instance, const char *icon_name, void *user_data);
typedef void (*TypioKeyboardModeChangedCallback)(TypioInstance *instance, const TypioKeyboardEngineMode *mode, void *user_data);
/* Active engine availability changed (ADR-0014). `reason` is an optional,
 * borrowed, human-readable string (may be NULL), valid only for the call. */
typedef void (*TypioEngineAvailabilityChangedCallback)(TypioInstance *instance, TypioEngineAvailability state, const char *reason, void *user_data);

/* An engine declared a new set of supported languages (ADR-0034: dynamic
 * engine capabilities). `engine_name` is borrowed and valid only for the
 * call; NULL means "the global language set may have changed, re-query".
 * The host should re-fetch `typio_registry_list_languages` and refresh any
 * derived surfaces (menu, persisted-active-language validity). */
typedef void (*TypioLanguagesChangedCallback)(TypioInstance *instance,
                                              const char *engine_name,
                                              void *user_data);

/* Log levels */
typedef enum {
    TYPIO_LOG_TRACE = 0,
    TYPIO_LOG_DEBUG = 1,
    TYPIO_LOG_INFO = 2,
    TYPIO_LOG_WARNING = 3,
    TYPIO_LOG_ERROR = 4,
} TypioLogLevel;

typedef struct {
    TypioLogLevel level;
    const char *message;
    const char *domain;
    const char *file;
    uint32_t line;
    uint64_t timestamp_ms;
} TypioLogEvent;

typedef void (*TypioLogCallback)(const TypioLogEvent *event, void *user_data);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_TYPES_H */
