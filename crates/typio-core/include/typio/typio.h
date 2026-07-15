/**
 * @file typio.h
 * @brief Umbrella header for hosts embedding libtypio.
 *
 * Layout reminder:
 *   - typio/abi/      — stable native engine ABI
 *   - typio/schema/   — user-facing and engine-owned config schema
 *   - typio/runtime/  — in-process API used by hosts (no stability promise)
 *
 * Engines must NOT include this host umbrella. They use `typio/abi/abi.h`
 * and, when declaring config fields, `typio/schema/config_schema.h`.
 *
 * Cross-process surfaces (UDS / D-Bus) are host concerns and are not
 * exposed here; they are implemented by typio-host (ADR-0007).
 */

#ifndef TYPIO_H
#define TYPIO_H

#include "typio/abi/abi.h"
#include "typio/runtime/instance.h"
#include "typio/runtime/registry.h"
#include "typio/runtime/voice.h"
#include "typio/schema/config_schema.h"

#endif /* TYPIO_H */
