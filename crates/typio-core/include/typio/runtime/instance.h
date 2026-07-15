/**
 * @file runtime/instance.h
 * @brief Host-only TypioInstance lifecycle and orchestration.
 *
 * This header is for hosts that embed libtypio. Engines must not include
 * it; engine-facing operations live in `typio/abi/instance.h` and are pulled
 * in transitively.
 */

#ifndef TYPIO_RUNTIME_INSTANCE_H
#define TYPIO_RUNTIME_INSTANCE_H

#include "typio/abi/instance.h"
#include "typio/abi/types.h"
#include "typio/runtime/registry.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ── Instance lifecycle ───────────────────────────────────────────────── */

typedef struct TypioInstanceConfig {
    const char *config_dir;
    const char *data_dir;
    const char *state_dir;
} TypioInstanceConfig;

/* Logging is configured out-of-band via `typio_logger_*` (`typio/abi/log.h`).
 * Call `typio_logger_init()` and `typio_logger_set_callback()` before
 * `typio_instance_init()`. */

TypioInstance *typio_instance_new(void);
TypioInstance *typio_instance_new_with_config(const TypioInstanceConfig *config);
void typio_instance_free(TypioInstance *instance);

TypioResult typio_instance_init(TypioInstance *instance);
void typio_instance_shutdown(TypioInstance *instance);

/* ── Engine registry access ───────────────────────────────────────────── */

TypioRegistry *typio_instance_get_registry(TypioInstance *instance);

/* ── Input context lifecycle (host creates/destroys; engines observe) ── */

TypioInputContext *typio_instance_create_context(TypioInstance *instance);
void typio_instance_destroy_context(TypioInstance *instance,
                                     TypioInputContext *ctx);
void typio_instance_set_focused_context(TypioInstance *instance,
                                        TypioInputContext *ctx);

/* ── Host-side observer callbacks ─────────────────────────────────────── */

void typio_instance_set_engine_changed_callback(TypioInstance *instance,
                                                 TypioEngineChangedCallback callback,
                                                 void *user_data);
void typio_instance_set_voice_engine_changed_callback(TypioInstance *instance,
                                                      TypioVoiceEngineChangedCallback callback,
                                                      void *user_data);
void typio_instance_set_status_icon_changed_callback(TypioInstance *instance,
                                                      TypioStatusIconChangedCallback callback,
                                                      void *user_data);
void typio_instance_set_keyboard_mode_changed_callback(TypioInstance *instance,
                                                TypioKeyboardModeChangedCallback callback,
                                                void *user_data);
void typio_instance_set_engine_availability_changed_callback(TypioInstance *instance,
                                                TypioEngineAvailabilityChangedCallback callback,
                                                void *user_data);
/* Dynamic engine capabilities (ADR-0034): fired whenever an engine updates
 * its declared languages at runtime via `typio_registry_set_engine_languages`.
 * The host rebuilds the language menu and validates the active language. */
void typio_instance_set_languages_changed_callback(TypioInstance *instance,
                                                   TypioLanguagesChangedCallback callback,
                                                   void *user_data);

/* ── Config-text surfaces (control panel / IPC) ───────────────────────── */

char *typio_instance_get_config_text(TypioInstance *instance);
TypioResult typio_instance_set_config_text(TypioInstance *instance,
                                            const char *content);

/* ── Internal runtime notifications (called by core, not engines) ─────── */

void typio_instance_notify_engine_changed(TypioInstance *instance,
                                          const TypioEngineInfo *engine);
void typio_instance_notify_voice_engine_changed(TypioInstance *instance,
                                                const TypioEngineInfo *engine);

/* ── Voice session — created by the host, owned by the instance ───────── */

struct TypioVoiceSession;
struct TypioVoiceSession *typio_instance_get_voice_session(TypioInstance *instance);
void typio_instance_set_voice_session(TypioInstance *instance,
                                      struct TypioVoiceSession *session);

/* ── Per-application identity (host-side persistence) ─────────────────── */

bool typio_instance_identity_preferences_enabled(TypioInstance *instance);
char *typio_instance_identity_load_engine(TypioInstance *instance,
                                          const char *provider_name,
                                          const char *app_id);
void typio_instance_identity_store_engine(TypioInstance *instance,
                                          const char *provider_name,
                                          const char *app_id,
                                          const char *engine_name);
bool typio_instance_identity_load_mode(TypioInstance *instance,
                                       const char *provider_name,
                                       const char *app_id,
                                       char **out_engine,
                                       char **out_mode_id);
void typio_instance_identity_store_mode(TypioInstance *instance,
                                        const char *provider_name,
                                        const char *app_id,
                                        const char *mode_engine,
                                        const char *mode_id);
void typio_instance_identity_clear_mode(TypioInstance *instance,
                                        const char *provider_name,
                                        const char *app_id,
                                        const char *current_engine);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_RUNTIME_INSTANCE_H */
