/**
 * @file voice.h
 * @brief Voice-engine ABI types shared between engines and the host.
 *
 * Native voice workers implement `TypioVoiceEngineOps` internally; see
 * `typio/abi/engine.h`. This header defines only the types that need to be
 * visible to engines for that contract: the audio-source vtable used by the
 * host to feed PCM samples, the voice state enum reported back to the host,
 * and the session-event payloads delivered through the host's callback.
 *
 * The session orchestration itself (`TypioVoiceSession`, lifecycle, feed/
 * dispatch) is a host-only surface and lives in `typio/runtime/voice.h`.
 */

#ifndef TYPIO_ABI_VOICE_H
#define TYPIO_ABI_VOICE_H

#include "typio/abi/types.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ── Audio source abstraction (provided by the host) ───────────────────── */

typedef struct TypioAudioSource TypioAudioSource;

typedef struct {
    bool (*start)(TypioAudioSource *source);
    void (*stop)(TypioAudioSource *source);
    void (*free)(TypioAudioSource *source);
    int  (*get_fd)(TypioAudioSource *source);
    void (*dispatch)(TypioAudioSource *source);
} TypioAudioSourceOps;

struct TypioAudioSource {
    const TypioAudioSourceOps *ops;
};

/* ── Voice session events ──────────────────────────────────────────────── */

typedef enum {
    TYPIO_VOICE_STATE_IDLE = 0,
    TYPIO_VOICE_STATE_LOADING,
    TYPIO_VOICE_STATE_RECORDING,
    TYPIO_VOICE_STATE_PROCESSING,
} TypioVoiceState;

typedef enum {
    TYPIO_VOICE_EVENT_STATE_CHANGE,
    TYPIO_VOICE_EVENT_RESULT,
    TYPIO_VOICE_EVENT_ERROR,
} TypioVoiceSessionEventType;

typedef struct {
    TypioVoiceSessionEventType type;
    TypioVoiceState     state;
    char               *text;   /**< RESULT: heap-allocated, caller frees with typio_free_string */
    const char         *error;  /**< ERROR: borrowed, do not free */
} TypioVoiceSessionEvent;

typedef void (*TypioVoiceSessionEventCallback)(const TypioVoiceSessionEvent *event,
                                                void *user_data);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_ABI_VOICE_H */
