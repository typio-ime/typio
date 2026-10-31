---
id: ADR-0059
title: "Voice engine idle memory eviction and configurable working set reclamation"
status: accepted
date: 2026-11-01
superseded_by: null
scope: engine
negative_knowledge: true
---

# ADR-0059: Voice engine idle memory eviction and configurable working set reclamation

- **Status**: Accepted
- **Date**: 2026-11-01
- **Scope**: engine
- **Deciders**: Typio maintainers
- **Related RFC**: Not used — this repository does not operate an RFC workflow

## Context

Offline neural speech recognition engines (such as `typio-engine-sherpa` executing SenseVoice-Small int8 via sherpa-onnx) require significant memory footprints: ~239 MB on disk and ~350 MB+ resident set size (RSS) in memory for model weights, ONNX Runtime compute graphs, and tensor arenas.

Voice input in desktop input methods exhibits high burstiness: users dictate multiple sentences during an active editing or chat session, punctuated by long idle intervals (hours or days).

Under the legacy architecture:
1. The voice model loaded lazily on the first `ProcessAudio` request and remained pinned in memory indefinitely.
2. The daemon's idle worker reaper (`reap_idle_workers`) explicitly skipped the active voice slot (`if is_current_voice { continue; }`) to avoid cold respawn storms on switch-back.
3. As a result, an engine that processed even one audio request remained resident for the lifetime of the session, consuming ~350 MB of physical memory even when completely unused.

Conversely, eagerly tearing down the model immediately after each transcription is catastrophic: reloading 239 MB of ONNX weights takes 0.5 to 2.0 seconds, introducing prohibitive latency on conversational voice input.

## Decision

1. **Engine-owned idle working set reclamation.** Heavyweight voice engines manage their own model memory lifecycle within the engine process boundary.
2. **Configurable idle timeout with 30-minute default.** Introduce the standardized configuration property:
   ```toml
   [engines.<name>]
   idle_timeout_seconds = 1800  # Default: 1800s (30 minutes)
   ```
   - A positive integer specifies the idle duration after the last audio transcription before the engine drops its resident recognizer (`SherpaOnnxDestroyOfflineRecognizer`), reclaiming ~350 MB RSS down to the ~10 MB process baseline.
   - Setting `idle_timeout_seconds = 0` explicitly disables eviction, guaranteeing permanent model residency.
3. **Deadline polling in the engine event loop.** The engine protocol channel (fd 3) is polled with a timeout derived from `idle_timeout_seconds - elapsed`. When no audio or protocol frames arrive before the deadline, the engine logs the eviction and unloads the model cleanly.
4. **Transparent on-demand reload.** Eviction is fully transparent to the host:
   - The engine process remains running and listening for requests.
   - `Request::Availability` continues reporting `Availability::Ready` as long as model artifacts remain intact on disk.
   - The subsequent `Request::ProcessAudio` automatically re-executes `ensure_loaded()`, reconstructing the recognizer without host-side coordination.
5. **Host-side slot resilience.** In `typio-runtime`, `snapshot_active_voice` and availability checks ensure the engine backend process is instantiated if uninstantiated, preventing stale or terminated workers from failing subsequent voice capture requests.

## Rationale

- **Optimal memory vs latency balance.** Retaining the model for 30 minutes absorbs natural speech bursts with zero latency while guaranteeing that systems left running for hours or days do not waste hundreds of megabytes on idle neural networks.
- **Clean separation of concerns.** Memory reclamation is internal to the engine. The host does not need to know the model framework (ONNX, GGML, LibTorch) or micromanage weights lifecycle; it simply routes audio over the Engine Protocol.
- **Zero process churn.** By keeping the worker process alive and only dropping the model weights, the engine avoids the overhead of process respawning, socketpair recreation, and protocol handshakes.

## Invariants & Behavioral Boundaries

- `[INV-VOICE-EVICT-01] Engine-Owned Model Lifecycle`: Heavyweight model memory eviction must be executed within the engine process boundary rather than through external process termination, keeping IPC channels and status probing responsive.
- `[INV-VOICE-EVICT-02] Default 30-Minute Window`: The default idle eviction threshold across voice engines is 1800 seconds (30 minutes). Setting `idle_timeout_seconds = 0` guarantees permanent residency.
- `[INV-VOICE-EVICT-03] Transparent Reload`: An engine with an evicted model must continue reporting `Availability::Ready` when on-disk artifacts are valid, and must reload transparently on the next `ProcessAudio`.

## Rejected Alternatives & Negative Knowledge

- **Unload immediately after every transcription (Eager eviction).** Rejected because ONNX model loading and graph optimization impose a 0.5–2.0s penalty per phrase. For push-to-talk dictation where users speak in rapid succession, this renders voice input unusable.
- **Terminate the engine process via host-side `reap_idle_workers`.** Rejected because killing the process severs the socketpair, drops engine schema and availability caches, and introduces a heavyweight fork+exec+handshake sequence on the host main loop during the critical voice capture path.
- **Hardcoded non-configurable timeout.** Rejected because memory constraints and usage patterns differ radically across hardware: high-throughput dictation on a 64 GB workstation benefits from permanent residency (`idle_timeout_seconds = 0`), whereas handhelds and small SBCs benefit from aggressive timeouts.

## Consequences

- **Positive**: Memory usage of idle voice engines drops from ~350 MB to ~10 MB after 30 minutes of inactivity; speech bursts experience zero cold-start delay; user has full control over the eviction threshold.
- **Negative (accepted)**: The first transcription after an idle eviction incurs model reload latency (~0.5–1.5s), identical to the first-run experience.
