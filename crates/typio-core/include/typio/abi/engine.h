/**
 * @file engine.h
 * @brief Input engine interface for Typio
 *
 * This file defines the engine interface that all input engines must implement.
 * Engines are loaded as plugins and provide input method functionality.
 *
 * Design principles:
 *   1. Type separation — keyboard and voice engines are distinct C types.
 *      The framework never mixes them in the same slot.
 *   2. Common base — TypioEngine holds lifecycle fields shared by both
 *      modalities.  TypioKeyboardEngine and TypioVoiceEngine embed it as
 *      their first member so pointer conversion between the specific type
 *      and the common base is always safe (offset zero).
 *   3. Explicit contracts — every callback in base_ops is mandatory.
 *      Engines that do not need a particular behaviour provide a no-op.
 */

#ifndef TYPIO_ENGINE_H
#define TYPIO_ENGINE_H

#include "typio/abi/types.h"
#include "typio/abi/version.h"

struct TypioConfigField; /* fwd-decl; full def in typio/schema/config_schema.h */

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief Engine metadata structure.
 *
 * Each engine plugin allocates this statically and returns it from
 * `typio_engine_get_info()`. Compatibility is established out-of-band via the
 * exported `typio_engine_abi_version()` (see typio/abi/version.h), so this
 * struct no longer carries a `struct_size` witness.
 */
struct TypioEngineInfo {
    const char *name;                          /* Unique engine identifier */
    const char *display_name;                  /* Human-readable name */
    const char *description;                   /* Engine description */
    const char *author;                        /* Engine author */
    const char *icon;                          /* Icon name or path */
    const char *language;                      /* Primary language code (e.g., "zh_CN") */
    TypioEngineType type;                      /* Engine type */

    /*
     * Capability negotiation.  Both arrays are NULL-terminated; either may
     * itself be NULL (treated as empty).  See typio/abi/types.h for the
     * standard capability names.
     *
     * required_capabilities — engine refuses to operate without them.
     *   Host rejects the engine at load time if any string is not in its
     *   supported set.
     * optional_capabilities — engine adapts when missing.  Host logs an
     *   info-level message but loads the engine.
     */
    const char *const *required_capabilities;
    const char *const *optional_capabilities;
};

/* -------------------------------------------------------------------------- */
/* Forward declarations                                                       */
/* -------------------------------------------------------------------------- */

typedef struct TypioKeyboardEngine TypioKeyboardEngine;
typedef struct TypioVoiceEngine TypioVoiceEngine;

/* -------------------------------------------------------------------------- */
/* Engine operations                                                          */
/* -------------------------------------------------------------------------- */

/**
 * @brief Base operations — mandatory for every engine.
 *
 * All callbacks must be provided.  If an engine does not need a particular
 * behaviour it should supply a no-op implementation (e.g. a function that
 * simply returns TYPIO_OK or does nothing).
 *
 * The @c engine parameter is a pointer to the common base (TypioEngine).
 * Because TypioKeyboardEngine and TypioVoiceEngine both embed TypioEngine as
 * their first member, a callback receiving TypioEngine* can be safely cast to
 * the specific type when the engine needs to access its modality-specific ops.
 */
typedef struct TypioEngineBaseOps {
    /**
     * @brief Initialise the engine instance.
     *
     * Allocate engine-specific state (usually via typio_engine_set_user_data)
     * and read configuration.  Called once before the engine becomes active.
     */
    TypioResult (*init)(TypioEngine *engine, TypioInstance *instance);

    /**
     * @brief Tear down the engine instance.
     *
     * Free all resources allocated by init().  Called once when the engine
     * is unloaded or the application exits.
     */
    void (*destroy)(TypioEngine *engine);

    /**
     * @brief The engine is no longer the active engine.
     *
     * Called when the user switches to a different engine.  Engines that
     * hold large in-memory resources (e.g. voice models) should free them
     * here to avoid memory bloat.  The engine remains registered and may
     * be reactivated later, in which case init() or focus_in() should
     * lazily reload the resource.
     */
    void (*deactivate)(TypioEngine *engine);

    /**
     * @brief The input context has gained focus.
     *
     * Engines should restore any visible UI state (preedit, candidates) that
     * was hidden by a previous focus_out.  Engines that do not have per-
     * context state may implement this as a no-op.
     */
    void (*focus_in)(TypioEngine *engine, TypioInputContext *ctx);

    /**
     * @brief The input context has lost focus.
     *
     * Engines should clear visible composition UI but preserve session state
     * (e.g. ascii_mode) so it survives focus churn.
     */
    void (*focus_out)(TypioEngine *engine, TypioInputContext *ctx);

    /**
     * @brief Reset engine state for the given context.
     *
     * Called on explicit reset (e.g. user pressed Escape).  Engines should
     * cancel any active composition and restore the default mode.
     */
    void (*reset)(TypioEngine *engine, TypioInputContext *ctx);

    /**
     * @brief Hot-reload engine-specific configuration.
     *
     * Called when the user edits core.toml or issues a reload command.
     * Engines that do not support runtime config changes may return TYPIO_OK.
     */
    TypioResult (*reload_config)(TypioEngine *engine);

    /**
     * @brief A configuration key the engine owns changed (ADR-0008).
     *
     * Invoked by the host after the value is committed to the unified config
     * tree. Engines react with live side effects (e.g. librime re-selecting a
     * schema). Receives only keys under the engine's own `engines.<name>.*`
     * namespace. Engines that need only persistence (no live reaction)
     * leave this NULL.
     *
     * `value` is the new string value; for non-string properties the engine
     * parses according to the schema field it registered. The string is
     * borrowed and only valid for the duration of the call.
     */
    void (*on_config_change)(TypioEngine *engine,
                             const char *key,
                             const char *value);

    /**
     * @brief Report the engine's current availability (ADR-0014).
     *
     * Answers "can the engine process input right now?"; the lifecycle axis
     * orthogonal to engagement and mode. Optional: if NULL the engine is
     * assumed @c TYPIO_ENGINE_READY at all times, which is correct for engines
     * with no asynchronous warm-up.
     *
     * This is the *pull* form, sampled on demand (e.g. the voice capture loop
     * checks it each chunk). Engines that warm up asynchronously should also
     * *push* transitions via typio_instance_notify_engine_availability so the
     * host reacts the instant they become ready, without polling.
     */
    TypioEngineAvailability (*availability)(TypioEngine *engine);
} TypioEngineBaseOps;

/**
 * @brief Keyboard-engine extension operations.
 *
 * Only keyboard engines populate this vtable.  The framework verifies at
 * registration time that process_key is non-NULL; the mode ops are
 * optional (engines with a single implicit mode may omit them).
 */
typedef struct TypioKeyboardEngineOps {
    /**
     * @brief Process a single key event.
     *
     * The engine inspects the key and either consumes it (returning HANDLED,
     * COMPOSING, or COMMITTED) or passes it through (NOT_HANDLED).
     */
    TypioKeyProcessResult (*process_key)(TypioKeyboardEngine *engine,
                                         TypioInputContext *ctx,
                                         const TypioKeyEvent *event);

    /**
     * @brief List all modes the engine supports.
     *
     * Returns a static array of TypioKeyboardEngineMode descriptors.
     * The framework uses this to know available modes, display them in UI,
     * and determine cycling order.  Returns NULL with *count = 0 for
     * engines with no mode concept.
     *
     * Optional.
     */
    const TypioKeyboardEngineMode *(*list_modes)(TypioKeyboardEngine *engine,
                                                  size_t *count);

    /**
     * @brief Return the currently active mode.
     *
     * The framework calls this on focus-in and mode change to determine
     * UI state (indicator, tray, panel). The host routes all keys to the
     * active engine regardless of mode; the engine decides via process_key
     * whether to consume or pass through each key.
     *
     * Optional.
     */
    const TypioKeyboardEngineMode *(*get_active_mode)(TypioKeyboardEngine *engine,
                                                       TypioInputContext *ctx);

    /**
     * @brief Set the active mode by @c mode_id.
     *
     * The host requests a mode switch by id.  The engine may accept or
     * reject.  Called when the standard mode trigger fires or when the user
     * selects a mode from UI.  Passing NULL as mode_id means "cycle to next
     * mode".
     *
     * Optional.
     */
    TypioResult (*set_active_mode)(TypioKeyboardEngine *engine,
                                     TypioInputContext *ctx,
                                     const char *mode_id);

    /**
     * @brief Commit a candidate selected by the host (ADR-0013).
     *
     * Called when host-managed selection is active and the user selects a
     * candidate (via digit key 0–9, space, or enter).  The engine retrieves the
     * candidate text at @p candidate_index and commits it via
     * typio_input_context_commit.  The commit call atomically clears preedit
     * and candidates, so the engine does not need to call
     * typio_input_context_clear separately.
     *
     * Optional.  Engines that do not use host-managed selection leave this
     * NULL.
     */
    TypioResult (*commit_candidate)(TypioKeyboardEngine *engine,
                                      TypioInputContext *ctx,
                                      int candidate_index);

} TypioKeyboardEngineOps;

/**
 * @brief Voice-engine extension operations.
 *
 * Only voice engines populate this vtable.  The framework verifies at
 * registration time that process_audio is non-NULL.
 */
typedef struct TypioVoiceEngineOps {
    /**
     * @brief Run speech-to-text inference on a buffer of audio samples.
     *
     * @param engine    The voice engine instance.
     * @param samples   PCM float32 mono 16kHz audio data.
     * @param n_samples Number of samples.
     * @return Heap-allocated result text (caller frees), or NULL on failure.
     */
    char *(*process_audio)(TypioVoiceEngine *engine,
                           const float *samples, size_t n_samples);
} TypioVoiceEngineOps;

/* -------------------------------------------------------------------------- */
/* Engine command surface (ADR-0008)                                          */
/* -------------------------------------------------------------------------- */
/*
 * Engines expose imperative actions ("deploy", "reload-dict", …) through the
 * command surface so hosts, the CLI, and the control panel can invoke them
 * without per-engine knowledge.
 *
 * Engine-owned *properties* (e.g. Rime's "schema") are NOT on this surface
 * (ADR-0008). They live in the unified config schema layer:
 * engines register their property fields at load time via
 * `typio_config_schema_register_many` and react to changes via the
 * `on_config_change` callback on `TypioEngineBaseOps`. Values are read
 * from / written to the unified config tree (`typio_config_get_*` /
 * `typio_config_set_*`).
 */

/**
 * @brief A named action an engine can perform on request.
 */
typedef struct {
    const char *id;              /* Stable identifier, e.g. "deploy" */
    const char *label;           /* Human-readable label */
} TypioEngineCommand;

/**
 * @brief Optional vtable for engines that expose commands.
 *
 * Engines without invokable actions leave TypioEngine::surface NULL.
 * Array returns are engine-owned and transient (valid until the next
 * surface call); the caller must serialize/copy before the next call.
 */
typedef struct TypioEngineSurfaceOps {
    /** Return a flat array of commands and write its length to out_count. */
    const TypioEngineCommand *(*list_commands)(TypioEngine *engine, size_t *out_count);
    /** Invoke a command by id. */
    TypioResult (*invoke_command)(TypioEngine *engine, const char *id);
} TypioEngineSurfaceOps;

/* -------------------------------------------------------------------------- */
/* Common engine base (shared lifecycle fields)                               */
/* -------------------------------------------------------------------------- */

/**
 * @brief Common engine instance structure
 *
 * This structure holds lifecycle fields shared by every engine regardless of
 * input modality.  It is embedded as the first member of both
 * TypioKeyboardEngine and TypioVoiceEngine, so a pointer to either specific
 * type can be safely cast to TypioEngine* and back.
 */
struct TypioEngine {
    const TypioEngineInfo *info;        /* Engine metadata */
    const TypioEngineBaseOps *base_ops; /* Mandatory base operations */
    TypioInstance *instance;            /* Parent instance */
    void *user_data;                    /* Engine-specific data */
    bool active;                        /* Whether engine is currently active */
    bool initialized;                   /* Whether init has been called */
    char *config_path;                  /* Path to engine configuration file */
    const TypioEngineSurfaceOps *surface; /* Optional surface vtable (may be NULL) */
};

/**
 * @brief Keyboard engine instance structure
 *
 * Embeds TypioEngine as its first member.  A TypioKeyboardEngine* can be
 * safely cast to TypioEngine* (and vice-versa) because the base sits at
 * offset zero.
 */
struct TypioKeyboardEngine {
    struct TypioEngine base;                /* Common lifecycle fields */
    const TypioKeyboardEngineOps *keyboard; /* Keyboard-specific operations */
};

/**
 * @brief Voice engine instance structure
 *
 * Embeds TypioEngine as its first member.  A TypioVoiceEngine* can be
 * safely cast to TypioEngine* (and vice-versa) because the base sits at
 * offset zero.
 */
struct TypioVoiceEngine {
    struct TypioEngine base;            /* Common lifecycle fields */
    const TypioVoiceEngineOps *voice;   /* Voice-specific operations */
};

/* -------------------------------------------------------------------------- */
/* Entry points                                                               */
/* -------------------------------------------------------------------------- */

/**
 * @brief Keyboard engine factory function type
 *
 * Each keyboard engine library must export a function named
 * "typio_keyboard_engine_create" with this signature.
 */
typedef TypioKeyboardEngine *(*TypioKeyboardEngineFactory)(void);

/**
 * @brief Voice engine factory function type
 *
 * Each voice engine library must export a function named
 * "typio_voice_engine_create" with this signature.
 */
typedef TypioVoiceEngine *(*TypioVoiceEngineFactory)(void);

/**
 * @brief Engine info function type
 *
 * All engine libraries export a function with this signature named
 * "typio_engine_get_info" to return engine metadata.
 */
typedef const TypioEngineInfo *(*TypioEngineInfoFunc)(void);

/**
 * @brief Optional engine entry point exposing the engine's config schema.
 *
 * Engines that declare `engines.<name>.*` configuration fields should export
 * a function named "typio_engine_get_config_schema" with this signature.
 * The host loader calls it immediately after `typio_engine_get_info` (i.e.
 * before instantiation) and forwards the result to
 * `typio_config_schema_register_many`, so the schema is visible to UI and
 * defaulting layers without the engine being active.
 *
 * The returned array must remain valid for as long as the plugin is loaded;
 * libtypio takes its own deep copy on registration. Engines may return NULL
 * with `*out_count == 0` to indicate no engine-owned config.
 */
typedef const struct TypioConfigField *(*TypioEngineConfigSchemaFunc)(size_t *out_count);

/*
 * Engine-entry-point macros.
 *
 * Each macro expands to three exported symbols with explicit C linkage and
 * default visibility regardless of the plugin's `-fvisibility` setting:
 *
 *   - `typio_engine_abi_version` (the host's compatibility witness)
 *   - `typio_engine_get_info`
 *   - `typio_keyboard_engine_create` (or `typio_voice_engine_create`)
 *
 * Recommended build flags for engines:
 *
 *     -fvisibility=hidden -fvisibility-inlines-hidden
 */

#ifdef __cplusplus
#  define TYPIO__EXTERN_C extern "C"
#else
#  define TYPIO__EXTERN_C
#endif

/* Emits typio_engine_abi_version() reporting the ABI the plugin was built
 * against. Shared by both engine-kind macros. */
#define TYPIO__ENGINE_ABI_VERSION_DEFINE \
    TYPIO__EXTERN_C TYPIO_EXPORT const TypioAbiVersion *typio_engine_abi_version(void) { \
        static const TypioAbiVersion version = { \
            TYPIO_ENGINE_ABI_MAJOR, TYPIO_ENGINE_ABI_MINOR \
        }; \
        return &version; \
    }

#define TYPIO_KEYBOARD_ENGINE_DEFINE(info_var, create_func) \
    TYPIO__ENGINE_ABI_VERSION_DEFINE \
    TYPIO__EXTERN_C TYPIO_EXPORT const TypioEngineInfo *typio_engine_get_info(void) { \
        return &info_var; \
    } \
    TYPIO__EXTERN_C TYPIO_EXPORT TypioKeyboardEngine *typio_keyboard_engine_create(void) { \
        return create_func(); \
    }

#define TYPIO_VOICE_ENGINE_DEFINE(info_var, create_func) \
    TYPIO__ENGINE_ABI_VERSION_DEFINE \
    TYPIO__EXTERN_C TYPIO_EXPORT const TypioEngineInfo *typio_engine_get_info(void) { \
        return &info_var; \
    } \
    TYPIO__EXTERN_C TYPIO_EXPORT TypioVoiceEngine *typio_voice_engine_create(void) { \
        return create_func(); \
    }

/* libtypio no longer provides in-process engine lifecycle/helper functions.
 * Engine workers allocate their own TypioKeyboardEngine / TypioVoiceEngine
 * values and communicate with the host through the process protocol. */

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_ENGINE_H */
