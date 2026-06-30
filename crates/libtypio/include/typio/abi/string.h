/**
 * @file string.h
 * @brief String utilities and the canonical libtypio deallocator family.
 *
 * Memory-ownership rule (single source of truth):
 *
 *   - Every string returned by a libtypio function is released with
 *     `typio_free_string`. This covers `typio_strdup`, `typio_strjoin`,
 *     `typio_path_join`, `typio_registry_get_active_*`,
 *     `typio_registry_get_engine_*`, `typio_instance_get_config_text`,
 *     `typio_instance_identity_load_*`, etc.
 *   - Every string-array returned by a libtypio function is released with
 *     `typio_free_string_array(list, count)`. This covers
 *     `typio_registry_list_keyboards`, `typio_registry_list_voices`, and
 *     `typio_registry_list_ordered_keyboards`.
 *   - Struct returns have a dedicated free (e.g. `typio_engine_info_free`,
 *     `typio_config_free`); the header that declares them documents it.
 *
 * Strings allocated by the C caller (e.g. via `strdup`, `malloc`) must be
 * freed with the matching C-side allocator. Never mix the two — on Windows
 * the libtypio DLL may use a different CRT than the host.
 */

#ifndef TYPIO_STRING_H
#define TYPIO_STRING_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Allocation */
char *typio_strdup(const char *str);
char *typio_strndup(const char *str, size_t n);

/**
 * @brief Release a string previously returned by libtypio.
 *
 * No-op when `str` is NULL.
 */
void typio_free_string(char *str);

/**
 * @brief Release a string-array previously returned by libtypio.
 *
 * Frees each interior string and then the outer array. No-op when @p list
 * is NULL. `count` MUST match the value libtypio wrote through its `count`
 * out-parameter.
 */
void typio_free_string_array(char **list, size_t count);

/* Composition */
char *typio_strjoin(const char *a, const char *b);
char *typio_strjoin3(const char *a, const char *b, const char *c);
char *typio_path_join(const char *base, const char *suffix);

/* Comparison */
bool typio_str_starts_with(const char *str, const char *prefix);
bool typio_str_ends_with(const char *str, const char *suffix);
bool typio_str_equals(const char *a, const char *b);
bool typio_str_equals_nocase(const char *a, const char *b);

/* Search */
const char *typio_str_find(const char *haystack, const char *needle);

/* Conversion */
int typio_str_to_int(const char *str, int default_val);
double typio_str_to_double(const char *str, double default_val);
bool typio_str_to_bool(const char *str, bool default_val);

/* UTF-8 */
size_t typio_utf8_strlen(const char *str);
const char *typio_utf8_next(const char *str);
const char *typio_utf8_prev(const char *str, const char *start);
uint32_t typio_utf8_get_char(const char *str);
size_t typio_utf8_encode(uint32_t codepoint, char *buf);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_STRING_H */
