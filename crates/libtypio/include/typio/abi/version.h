/**
 * @file version.h
 * @brief Engine ABI version — the compatibility witness between a native
 *        engine implementation and the engine runtime.
 *
 * Every engine plugin exports `typio_engine_abi_version()` (emitted
 * automatically by the `TYPIO_*_ENGINE_DEFINE` macros in engine.h). A direct
 * worker links it into the executable; a compatibility loader may resolve it
 * with `dlsym` inside the worker process. The runtime rejects the engine when
 * the major version differs from its own, or when the engine's minor exceeds
 * the runtime's.
 * This replaces the older `TypioEngineInfo.struct_size` field — see
 * `docs/dev/abi-stability.md`.
 *
 * Versioning policy (pre-1.0):
 *   - MAJOR bumps on any incompatible layout/semantic change. Plugins built
 *     against a different major are always rejected.
 *   - MINOR bumps on backward-compatible additions. A runtime accepts engines
 *     whose minor is <= its own (the runtime understands everything the engine
 *     relies on); it rejects plugins built against a newer minor.
 */

#ifndef TYPIO_ABI_VERSION_H
#define TYPIO_ABI_VERSION_H

#include "typio/abi/types.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Current engine ABI major version (incompatible changes). */
#define TYPIO_ENGINE_ABI_MAJOR 0u
/** Current engine ABI minor version (backward-compatible additions). */
#define TYPIO_ENGINE_ABI_MINOR 2u

/**
 * @brief ABI version reported by a plugin through `typio_engine_abi_version`.
 */
typedef struct TypioAbiVersion {
    uint32_t major;
    uint32_t minor;
} TypioAbiVersion;

/**
 * @brief Type of the `typio_engine_abi_version` entry point every plugin
 *        exports. The returned pointer must remain valid for the lifetime of
 *        the loaded plugin (engines return a pointer to a static value).
 */
typedef const TypioAbiVersion *(*TypioEngineAbiVersionFunc)(void);

/**
 * @brief Test whether an engine's reported ABI version is compatible with this
 *        runtime build.
 *
 * Engine runtimes call this after resolving `typio_engine_abi_version` and
 * before registering the engine. The `typio-vet` tool uses it too.
 *
 * @param plugin Version reported by the plugin, or NULL.
 * @return true when @p plugin is non-NULL, `plugin->major` equals
 *         `TYPIO_ENGINE_ABI_MAJOR`, and `plugin->minor` is <=
 *         `TYPIO_ENGINE_ABI_MINOR`; false otherwise.
 */
bool typio_engine_abi_check(const TypioAbiVersion *plugin);

#ifdef __cplusplus
}
#endif

#endif /* TYPIO_ABI_VERSION_H */
