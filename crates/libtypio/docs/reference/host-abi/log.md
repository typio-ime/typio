# Logging API Reference

libtypio never decides where logs go. It only produces structured log events and forwards them to a host-provided callback. If no callback is installed, records are silently retained in an internal ring buffer.

## Lifecycle

```c
bool typio_logger_init(void);
void typio_logger_shutdown(void);
```

| Function | Notes |
|----------|-------|
| `typio_logger_init` | Idempotent; safe to call multiple times. Must be called before any log output is captured. |
| `typio_logger_shutdown` | Clears the callback, resets the level to `TYPIO_LOG_INFO`, and empties the ring buffer. The global logger remains registered but becomes a no-op. |

## Configuration

```c
void typio_logger_set_callback(TypioLogCallback callback, void *user_data);
void typio_logger_set_level(TypioLogLevel level);
TypioLogLevel typio_logger_get_level(void);
void typio_logger_set_recent_capacity(size_t capacity);
```

| Function | Default | Notes |
|----------|---------|-------|
| `typio_logger_set_callback` | `NULL` | `NULL` disables host-side output; records are only kept in the ring buffer |
| `typio_logger_set_level` | `TYPIO_LOG_INFO` | Filter threshold; records below this level are dropped |
| `typio_logger_set_recent_capacity` | 256 | Size of the internal ring buffer |

## Diagnostics

```c
bool typio_logger_dump_recent(const char *path);
```

Dumps the ring buffer to a file. Creates parent directories if necessary. Returns `true` on success.

## Log levels

```c
typedef enum {
    TYPIO_LOG_TRACE = 0,
    TYPIO_LOG_DEBUG = 1,
    TYPIO_LOG_INFO = 2,
    TYPIO_LOG_WARNING = 3,
    TYPIO_LOG_ERROR = 4,
} TypioLogLevel;
```

## Structured event

```c
typedef struct {
    TypioLogLevel level;
    const char *message;
    const char *domain;
    const char *file;
    uint32_t line;
    uint64_t timestamp_ms;
} TypioLogEvent;

typedef void (*TypioLogCallback)(const TypioLogEvent *event, void *user_data);
```

| Field | Meaning |
|-------|---------|
| `level` | Severity |
| `message` | Formatted log message |
| `domain` | Module path (e.g. `typio::voice::session`) |
| `file` | Source file name |
| `line` | Source line number |
| `timestamp_ms` | Milliseconds since the Unix epoch |

## Engine/plugin logging

C engines should use the convenience macros rather than calling `typio_log_emit` directly:

```c
void typio_log_emit(TypioLogLevel level, const char *message);

#define typio_log_trace(...)   /* TYPIO_LOG_TRACE */
#define typio_log_debug(...)   /* TYPIO_LOG_DEBUG */
#define typio_log_info(...)    /* TYPIO_LOG_INFO */
#define typio_log_warning(...) /* TYPIO_LOG_WARNING */
#define typio_log_error(...)   /* TYPIO_LOG_ERROR */
```
