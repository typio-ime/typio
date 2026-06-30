# Event Reference

Types in `typio/abi/event.h`. Hosts construct `TypioKeyEvent` values and pass
them to [`typio_input_context_process_key`](input-context.md#key-processing);
engines receive them through the keyboard ops vtable
([Engine ▸ Operations](../engine/ops.md)).

## `TypioEventType`

```c
typedef enum {
    TYPIO_EVENT_KEY_PRESS        = 0,
    TYPIO_EVENT_KEY_RELEASE      = 1,
    TYPIO_EVENT_FOCUS_IN         = 2,
    TYPIO_EVENT_FOCUS_OUT        = 3,
    TYPIO_EVENT_RESET            = 4,
    TYPIO_EVENT_VOICE_START      = 5,
    TYPIO_EVENT_VOICE_END        = 6,
    TYPIO_EVENT_VOICE_DATA       = 7,
    TYPIO_EVENT_COMMIT           = 8,
    TYPIO_EVENT_CANDIDATE_SELECT = 9,
} TypioEventType;
```

| Value | Carried by | Producer → Consumer |
|-------|------------|---------------------|
| `KEY_PRESS`, `KEY_RELEASE` | `TypioKeyEvent` | Host → engine |
| `FOCUS_IN`, `FOCUS_OUT` | — (delivered through input-context ops) | Host → engine |
| `RESET` | — | Host → engine |
| `VOICE_START`, `VOICE_END`, `VOICE_DATA` | `TypioVoiceEvent` | Host → voice engine |
| `COMMIT` | — (delivered through commit callback) | Engine → host |
| `CANDIDATE_SELECT` | — | Host → engine |

## `TypioKeyEvent`

```c
struct TypioKeyEvent {
    size_t         struct_size;
    TypioEventType type;
    uint32_t       keycode;
    uint32_t       keysym;
    uint32_t       modifiers;
    uint32_t       unicode;
    uint64_t       time;
    bool           is_repeat;
};
```

| Field | Type | Meaning |
|-------|------|---------|
| `struct_size` | `size_t` | MUST equal `sizeof(TypioKeyEvent)` at host build time. Read by the framework to detect old-header hosts after additive field growth. |
| `type` | `TypioEventType` | `TYPIO_EVENT_KEY_PRESS` or `TYPIO_EVENT_KEY_RELEASE` |
| `keycode` | `uint32_t` | Platform raw keycode (evdev on Linux Wayland) |
| `keysym` | `uint32_t` | XKB-compatible key symbol; match against `TYPIO_KEY_*` constants below |
| `modifiers` | `uint32_t` | Bitmask of `TypioModifier` values |
| `unicode` | `uint32_t` | Unicode codepoint produced by this key, or `0` if not applicable |
| `time` | `uint64_t` | Event timestamp in milliseconds since the Unix epoch |
| `is_repeat` | `bool` | `true` if the key event is a key repeat |

### Ownership

| Allocation | Free with | When to use |
|------------|-----------|-------------|
| Stack-allocated by host | — (RAII) | Normal path. Host fills the struct and passes by pointer to `typio_input_context_process_key`. |
| `typio_key_event_new(...)` | `typio_key_event_free(event)` | Test code or async queueing where the event must outlive the call frame. |

In both cases the host MUST set `struct_size = sizeof(TypioKeyEvent)`.

### Construction

```c
TypioKeyEvent *typio_key_event_new(TypioEventType type,
                                   uint32_t keycode,
                                   uint32_t keysym,
                                   uint32_t modifiers);
void typio_key_event_free(TypioKeyEvent *event);
```

### Predicates

```c
bool     typio_key_event_is_press(const TypioKeyEvent *event);
bool     typio_key_event_is_release(const TypioKeyEvent *event);
bool     typio_key_event_has_modifier(const TypioKeyEvent *event, TypioModifier mod);
bool     typio_key_event_is_modifier_only(const TypioKeyEvent *event);
uint32_t typio_key_event_get_unicode(const TypioKeyEvent *event);
```

| Function | Returns |
|----------|---------|
| `is_press` | `type == TYPIO_EVENT_KEY_PRESS` |
| `is_release` | `type == TYPIO_EVENT_KEY_RELEASE` |
| `has_modifier` | `(event->modifiers & mod) != 0` |
| `is_modifier_only` | `true` if `keysym` is itself a modifier (`Shift_L`, `Control_L`, etc.) |
| `get_unicode` | `event->unicode` (convenience accessor) |

### Key-class predicates

```c
bool typio_key_event_is_backspace(const TypioKeyEvent *event);
bool typio_key_event_is_enter    (const TypioKeyEvent *event);
bool typio_key_event_is_escape   (const TypioKeyEvent *event);
bool typio_key_event_is_space    (const TypioKeyEvent *event);
bool typio_key_event_is_tab      (const TypioKeyEvent *event);
bool typio_key_event_is_arrow    (const TypioKeyEvent *event);
bool typio_key_event_is_page     (const TypioKeyEvent *event);
```

| Function | Matches `keysym` |
|----------|------------------|
| `is_backspace` | `TYPIO_KEY_BackSpace` |
| `is_enter` | `TYPIO_KEY_Return`, `TYPIO_KEY_KP_Enter` |
| `is_escape` | `TYPIO_KEY_Escape` |
| `is_space` | `TYPIO_KEY_space` |
| `is_tab` | `TYPIO_KEY_Tab` |
| `is_arrow` | `TYPIO_KEY_Left`, `Right`, `Up`, `Down` |
| `is_page` | `TYPIO_KEY_Page_Up`, `TYPIO_KEY_Page_Down` |

## `TypioModifier`

```c
typedef enum {
    TYPIO_MOD_NONE     = 0,
    TYPIO_MOD_SHIFT    = (1 << 0),
    TYPIO_MOD_CTRL     = (1 << 1),
    TYPIO_MOD_ALT      = (1 << 2),
    TYPIO_MOD_SUPER    = (1 << 3),
    TYPIO_MOD_CAPSLOCK = (1 << 4),
    TYPIO_MOD_NUMLOCK  = (1 << 5),
} TypioModifier;
```

Combine with bitwise OR; test with `typio_key_event_has_modifier`.

## `TYPIO_KEY_*` constants

XKB-compatible keysym values for direct comparison with `TypioKeyEvent.keysym`.

### Editing

| Constant | Value |
|----------|-------|
| `TYPIO_KEY_BackSpace` | `0xff08` |
| `TYPIO_KEY_Tab` | `0xff09` |
| `TYPIO_KEY_Return` | `0xff0d` |
| `TYPIO_KEY_KP_Enter` | `0xff8d` |
| `TYPIO_KEY_Escape` | `0xff1b` |
| `TYPIO_KEY_Delete` | `0xffff` |
| `TYPIO_KEY_space` | `0x0020` |

### Navigation

| Constant | Value |
|----------|-------|
| `TYPIO_KEY_Home` | `0xff50` |
| `TYPIO_KEY_Left` | `0xff51` |
| `TYPIO_KEY_Up` | `0xff52` |
| `TYPIO_KEY_Right` | `0xff53` |
| `TYPIO_KEY_Down` | `0xff54` |
| `TYPIO_KEY_Page_Up` | `0xff55` |
| `TYPIO_KEY_Page_Down` | `0xff56` |
| `TYPIO_KEY_End` | `0xff57` |

### Modifier keysyms

| Constant | Value |
|----------|-------|
| `TYPIO_KEY_Shift_L` | `0xffe1` |
| `TYPIO_KEY_Shift_R` | `0xffe2` |
| `TYPIO_KEY_Control_L` | `0xffe3` |
| `TYPIO_KEY_Control_R` | `0xffe4` |
| `TYPIO_KEY_Alt_L` | `0xffe9` |
| `TYPIO_KEY_Alt_R` | `0xffea` |
| `TYPIO_KEY_Super_L` | `0xffeb` |
| `TYPIO_KEY_Super_R` | `0xffec` |

### Function keys

| Constant | Value |
|----------|-------|
| `TYPIO_KEY_F1` … `TYPIO_KEY_F12` | `0xffbe` … `0xffc9` (contiguous) |

## `TypioVoiceEvent`

```c
typedef struct TypioVoiceEvent {
    TypioEventType type;
    const void    *audio_data;
    size_t         audio_size;
    int            sample_rate;
    int            channels;
    int            bits_per_sample;
} TypioVoiceEvent;
```

| Field | Type | Meaning |
|-------|------|---------|
| `type` | `TypioEventType` | `TYPIO_EVENT_VOICE_START`, `_END`, or `_DATA` |
| `audio_data` | `const void *` | PCM sample buffer; non-owning, valid for the call |
| `audio_size` | `size_t` | Size of `audio_data` in **bytes** |
| `sample_rate` | `int` | e.g. `16000` |
| `channels` | `int` | Usually `1` |
| `bits_per_sample` | `int` | Usually `16` |

### Construction

```c
TypioVoiceEvent *typio_voice_event_new(TypioEventType type);
void             typio_voice_event_free(TypioVoiceEvent *event);
void             typio_voice_event_set_data(TypioVoiceEvent *event,
                                            const void *data,
                                            size_t size,
                                            int sample_rate,
                                            int channels,
                                            int bits_per_sample);
```

`typio_voice_event_set_data` stores `data` by reference — the caller retains
ownership and must keep the buffer alive for the duration of the call into
the voice engine.

## `TypioEvent`

```c
struct TypioEvent {
    TypioEventType type;
    uint64_t       time;
    union {
        TypioKeyEvent   key;
        TypioVoiceEvent voice;
    } data;
};
```

Generic envelope used by surfaces that multiplex multiple event types. Inspect
`type` before reading `data`.

## See also

- [Input Context](input-context.md) — `typio_input_context_process_key`
- [Engine ▸ Operations](../engine/ops.md) — `TypioKeyboardEngineOps::process_key`, `TypioVoiceEngineOps::process_audio`
