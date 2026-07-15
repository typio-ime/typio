/**
 * @file abi/abi.h
 * @brief Umbrella header for native engine workers — the complete engine ABI.
 *
 * Native engine implementations include this single header to access the
 * stable engine ABI. It pulls in only the typio/abi headers — never host-only
 * runtime, ipc, or schema headers — so an engine that includes this and
 * nothing else from typio/ is guaranteed to stay within the engine contract.
 *
 * Out-of-tree workers consume the installed headers and link libtypio for the
 * worker-local instance and input-context implementation.
 */

#ifndef TYPIO_ABI_ABI_H
#define TYPIO_ABI_ABI_H

#include "typio/abi/config.h"
#include "typio/abi/engine.h"
#include "typio/abi/event.h"
#include "typio/abi/input_context.h"
#include "typio/abi/instance.h"
#include "typio/abi/log.h"
#include "typio/abi/shortcut.h"
#include "typio/abi/string.h"
#include "typio/abi/types.h"
#include "typio/abi/version.h"
#include "typio/abi/voice.h"

#endif /* TYPIO_ABI_ABI_H */
