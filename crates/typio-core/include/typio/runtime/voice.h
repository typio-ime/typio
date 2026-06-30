/**
 * @file runtime/voice.h
 * @brief Host-only voice session API.
 *
 * Core owns the voice session lifecycle, state machine, and dispatch into
 * the active voice engine. Hosts (typiod-wayland, etc.) inject a platform-
 * specific audio source and register an event callback to receive results
 * and state changes.
 *
 * Engines must NOT include this header; the engine-side voice ABI lives in
 * `typio/abi/voice.h`.
 */

#ifndef TYPIO_RUNTIME_VOICE_H
#define TYPIO_RUNTIME_VOICE_H

#include "typio/abi/types.h"
#include "typio/abi/voice.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ── Opaque session handle ──────────────────────────────────────────────── */

typedef struct TypioVoiceSession TypioVoiceSession;

/* ── Lifecycle ─────────────────────────────────────────────────────────── */

TypioVoiceSession *typio_voice_session_new(TypioInstance *instance);
void typio_voice_session_free(TypioVoiceSession *session);

/* ── Audio source injection (must be set before start) ─────────────────── */

void typio_voice_session_set_audio_source(TypioVoiceSession *session,
                                          TypioAudioSource *source);

/** Push audio samples into the session (called by the audio source callback). */
void typio_voice_session_feed_audio(TypioVoiceSession *session,
                                    const float *samples, size_t count);

/* ── Event callback ────────────────────────────────────────────────────── */

void typio_voice_session_set_callback(TypioVoiceSession *session,
                                      TypioVoiceSessionEventCallback callback,
                                      void *user_data);

/* ── Control ───────────────────────────────────────────────────────────── */

bool typio_voice_session_start(TypioVoiceSession *session);
void typio_voice_session_stop(TypioVoiceSession *session);
bool typio_voice_session_is_available(const TypioVoiceSession *session);
const char *typio_voice_session_get_unavail_reason(const TypioVoiceSession *session);

/* ── Event-loop integration (fd-based dispatch) ────────────────────────── */

int  typio_voice_session_get_fd(TypioVoiceSession *session);
void typio_voice_session_dispatch(TypioVoiceSession *session);

/* ── Engine reload ─────────────────────────────────────────────────────── */

void typio_voice_session_reload_engine(TypioVoiceSession *session);

/* ── Utility ───────────────────────────────────────────────────────────── */

void typio_voice_filter_tags_inplace(char *text);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_RUNTIME_VOICE_H */
