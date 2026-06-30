/**
 * @file log.h
 * @brief Structured logging API (public ABI).
 *
 * libtypio never decides where logs go.  It only produces structured log
 * events and forwards them to a host-provided callback.  If no callback is
 * installed, records are silently dropped (but retained in a ring buffer for
 * crash dumps).
 *
 * Hosts call `typio_logger_init()` once at startup, then install a callback
 * with `typio_logger_set_callback()`.  C engines emit records through the
 * `typio_log_debug` / `typio_log_info` / `typio_log_warning` /
 * `typio_log_error` convenience macros.
 */

#ifndef TYPIO_LOG_H
#define TYPIO_LOG_H

#include "typio/abi/types.h"
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Lifecycle ──────────────────────────────────────────────────────────── */

bool typio_logger_init(void);
void typio_logger_shutdown(void);

/* ── Configuration ──────────────────────────────────────────────────────── */

void typio_logger_set_callback(TypioLogCallback callback, void *user_data);
void typio_logger_set_level(TypioLogLevel level);
TypioLogLevel typio_logger_get_level(void);
void typio_logger_set_recent_capacity(size_t capacity);

/* ── Diagnostics ────────────────────────────────────────────────────────── */

bool typio_logger_dump_recent(const char *path);

/* ── Engine/plugin logging ──────────────────────────────────────────────── */

/* Implemented in Rust; C code should use the inline wrappers below. */
void typio_log_emit(TypioLogLevel level, const char *message);

#if defined(__GNUC__) || defined(__clang__)
__attribute__((format(printf, 2, 3)))
#endif
static inline void typio_logf(TypioLogLevel level, const char *format, ...) {
    va_list args;
    char buf[1024];
    va_start(args, format);
    vsnprintf(buf, sizeof(buf), format, args);
    va_end(args);
    typio_log_emit(level, buf);
}

#define typio_log_trace(...)   typio_logf(TYPIO_LOG_TRACE, __VA_ARGS__)
#define typio_log_debug(...)   typio_logf(TYPIO_LOG_DEBUG, __VA_ARGS__)
#define typio_log_info(...)    typio_logf(TYPIO_LOG_INFO, __VA_ARGS__)
#define typio_log_warning(...) typio_logf(TYPIO_LOG_WARNING, __VA_ARGS__)
#define typio_log_error(...)   typio_logf(TYPIO_LOG_ERROR, __VA_ARGS__)

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_LOG_H */
