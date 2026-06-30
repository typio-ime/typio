/*
 * typio-engine-hello — minimal example keyboard engine for Typio.
 *
 * Intercepts the letter 'a' and commits "hello".  Everything else passes
 * through to the focused application unchanged.
 *
 * Use this as a starting point: rename, change capabilities, fill in the
 * vtable callbacks.  See typio/abi/engine.h for the full ABI surface.
 */

#include "typio/abi/abi.h"

#include <stddef.h>
#include <stdlib.h>

/* -- engine state --------------------------------------------------------- */

typedef struct HelloState {
    int commit_count;
} HelloState;

/*
 * Icon resources:
 *   - Engine brand icon:    TypioEngineInfo.icon (static, also in the manifest)
 *   - Mode icon:            TypioKeyboardEngineMode.icon_name (via get_active_mode)
 *   - Runtime status icon:  typio_instance_notify_status_icon()
 *
 * The bundled symbolic SVG is installed into the hicolor icon theme
 * (<datadir>/icons/hicolor/...) so hosts resolve the names above through
 * the freedesktop icon lookup.
 */

/* -- base operations ------------------------------------------------------ */

static TypioResult hello_init(TypioEngine *engine, TypioInstance *instance) {
    (void)instance;
    HelloState *state = calloc(1, sizeof(*state));
    if (!state) {
        return TYPIO_ERROR_OUT_OF_MEMORY;
    }
    typio_engine_set_user_data(engine, state);
    return TYPIO_OK;
}

static void hello_destroy(TypioEngine *engine) {
    HelloState *state = typio_engine_get_user_data(engine);
    free(state);
    typio_engine_set_user_data(engine, NULL);
}

static void hello_deactivate(TypioEngine *engine) {
    (void)engine;
}

static void hello_focus_in(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine; (void)ctx;
}

static void hello_focus_out(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine; (void)ctx;
}

static void hello_reset(TypioEngine *engine, TypioInputContext *ctx) {
    (void)engine; (void)ctx;
}

static TypioResult hello_reload_config(TypioEngine *engine) {
    (void)engine;
    return TYPIO_OK;
}

static void hello_on_config_change(TypioEngine *engine,
                                   const char *key,
                                   const char *value) {
    (void)engine; (void)key; (void)value;
}

/* -- keyboard operations -------------------------------------------------- */

static TypioKeyProcessResult hello_process_key(TypioKeyboardEngine *engine,
                                                TypioInputContext *ctx,
                                                const TypioKeyEvent *event) {
    (void)engine;
    if (event->type != TYPIO_EVENT_KEY_PRESS) {
        return TYPIO_KEY_NOT_HANDLED;
    }
    if (event->keysym != 'a') {
        return TYPIO_KEY_NOT_HANDLED;
    }
    typio_input_context_commit(ctx, "hello");
    return TYPIO_KEY_COMMITTED;
}

static const TypioKeyboardEngineMode hello_mode = {
    .id = "hello-latin",           /* mode id (engine-defined) */
    .label = "Hello",              /* human mode name */
    .display_label = "Hello",      /* short indicator badge */
    .icon_name = "hello",          /* mode-level icon; same rules as engine icon */
    .salience = TYPIO_STATUS_SALIENCE_QUIET,
};

static const TypioKeyboardEngineMode *hello_list_modes(TypioKeyboardEngine *engine,
                                                       size_t *count) {
    (void)engine;
    if (count) {
        *count = 1;
    }
    return &hello_mode;
}

static const TypioKeyboardEngineMode *hello_get_active_mode(TypioKeyboardEngine *engine,
                                                            TypioInputContext *ctx) {
    (void)engine; (void)ctx;
    return &hello_mode;
}

/* -- vtables -------------------------------------------------------------- */

static const TypioEngineBaseOps hello_base_ops = {
    .init = hello_init,
    .destroy = hello_destroy,
    .deactivate = hello_deactivate,
    .focus_in = hello_focus_in,
    .focus_out = hello_focus_out,
    .reset = hello_reset,
    .reload_config = hello_reload_config,
    .on_config_change = hello_on_config_change,
};

static const TypioKeyboardEngineOps hello_keyboard_ops = {
    .process_key = hello_process_key,
    .list_modes = hello_list_modes,
    .get_active_mode = hello_get_active_mode,
    .set_active_mode = NULL,
    .commit_candidate = NULL,
};

/* -- engine metadata ------------------------------------------------------ */

static const TypioEngineInfo hello_engine_info = {
    .name = "hello",
    .display_name = "Hello",
    .description = "Minimal example engine — types 'hello' when you press 'a'.",
    .author = "Typio",
    .icon = "hello",                 /* freedesktop icon name; bundled SVG
                                        * installed into the hicolor theme */
    .language = "und",
    .type = TYPIO_ENGINE_TYPE_KEYBOARD,
    .required_capabilities = NULL,  /* no required capabilities */
    .optional_capabilities = NULL,  /* no optional capabilities */
};

/* -- factory exports ----------------------------------------------------- */
/*
 * TYPIO_KEYBOARD_ENGINE_DEFINE emits:
 *   typio_engine_get_info()          — worker_main reads metadata from this
 *   typio_keyboard_engine_create()   — worker_main instantiates through this
 */

static TypioKeyboardEngine *hello_engine_create(void) {
    return typio_keyboard_engine_new(&hello_engine_info,
                                     &hello_base_ops,
                                     &hello_keyboard_ops);
}

TYPIO_KEYBOARD_ENGINE_DEFINE(hello_engine_info, hello_engine_create)
