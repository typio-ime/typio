/**
 * @file config.h
 * @brief Configuration management for Typio
 */

#ifndef TYPIO_CONFIG_H
#define TYPIO_CONFIG_H

#include "typio/abi/types.h"

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief Configuration value type — used by the schema layer.
 *
 * `TypioConfig` stores arbitrary structured data internally; the public API
 * is the set of typed getters below (`typio_config_get_string` /
 * `_int` / `_bool` / `_float`), which abstract over the storage type.
 */
typedef enum {
    TYPIO_CONFIG_STRING = 0,
    TYPIO_CONFIG_INT = 1,
    TYPIO_CONFIG_BOOL = 2,
    TYPIO_CONFIG_FLOAT = 3,
    TYPIO_CONFIG_ARRAY = 4,
    TYPIO_CONFIG_OBJECT = 5,
} TypioConfigType;

/* Config lifecycle */
TypioConfig *typio_config_new(void);
TypioConfig *typio_config_load_file(const char *path);
TypioConfig *typio_config_load_string(const char *content);
void typio_config_free(TypioConfig *config);

/* Config I/O */
TypioResult typio_config_save_file(const TypioConfig *config, const char *path);
char *typio_config_to_string(const TypioConfig *config);

/* Value getters (typed). All borrow strings from the live config; the
 * pointer is valid until the next mutation or `typio_config_free`. */
const char *typio_config_get_string(const TypioConfig *config, const char *key,
                                     const char *default_val);
int typio_config_get_int(const TypioConfig *config, const char *key,
                          int default_val);
bool typio_config_get_bool(const TypioConfig *config, const char *key,
                            bool default_val);
double typio_config_get_float(const TypioConfig *config, const char *key,
                               double default_val);

/* Value setters */
TypioResult typio_config_set_string(TypioConfig *config, const char *key,
                                     const char *value);
TypioResult typio_config_set_int(TypioConfig *config, const char *key,
                                  int value);
TypioResult typio_config_set_bool(TypioConfig *config, const char *key,
                                   bool value);
TypioResult typio_config_set_float(TypioConfig *config, const char *key,
                                    double value);
TypioResult typio_config_set_string_array(TypioConfig *config, const char *key,
                                          const char *const *values, size_t count);

/* Nested config */
TypioConfig *typio_config_get_section(const TypioConfig *config,
                                       const char *section);
TypioResult typio_config_set_section(TypioConfig *config, const char *section,
                                      TypioConfig *sub_config);

/* Array access (typed read-only views) */
size_t typio_config_get_array_size(const TypioConfig *config, const char *key);
const char *typio_config_get_array_string(const TypioConfig *config,
                                           const char *key, size_t index);
int typio_config_get_array_int(const TypioConfig *config,
                                const char *key, size_t index);

/* Key enumeration */
size_t typio_config_key_count(const TypioConfig *config);
/**
 * @brief Get the name of the key at `index`.
 *
 * @return Newly allocated NUL-terminated string (caller frees with
 *         `typio_free_string`), or NULL if `index` is out of range.
 */
char *typio_config_key_at(const TypioConfig *config, size_t index);
bool typio_config_has_key(const TypioConfig *config, const char *key);
/** Return true when a key came from user config rather than schema defaults. */
bool typio_config_is_user_value(const TypioConfig *config, const char *key);

/* Remove key */
TypioResult typio_config_remove(TypioConfig *config, const char *key);

/* Merge configs */
TypioResult typio_config_merge(TypioConfig *dest, const TypioConfig *src);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_CONFIG_H */
