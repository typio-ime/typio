/**
 * @file typio.h
 * @brief Umbrella header for hosts embedding libtypio.
 *
 * Layout reminder:
 *   - typio/abi/      — stable plugin ABI (engines link to these)
 *   - typio/schema/   — user-facing config schema
 *   - typio/runtime/  — in-process API used by hosts (no stability promise)
 *
 * Engines must NOT include this header; they pull only from typio/abi/.
 * The single include they need is `typio/abi/abi.h`.
 *
 * Cross-process surfaces (UDS / D-Bus) are host concerns and are not
 * exposed here; see the typio-wayland repository (ADR-0007).
 */

#ifndef TYPIO_H
#define TYPIO_H

#include "typio/abi/abi.h"
#include "typio/runtime/instance.h"
#include "typio/runtime/registry.h"
#include "typio/runtime/voice.h"
#include "typio/schema/config_schema.h"

#endif /* TYPIO_H */
