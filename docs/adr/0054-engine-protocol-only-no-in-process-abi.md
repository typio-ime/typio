# ADR-0054: Recognition engines are protocol processes with no in-process ABI

- **Status**: Accepted
- **Date**: 2026-10-29
- **Scope**: engine
- **Deciders**: Typio maintainers
- **Supersedes**: the in-process C ABI engine contract (retired with `typio-core`; see ADR-0035)
- **Related RFC**: Not used — this repository does not operate an RFC workflow

## Context

Typio's runtime boundary is the Engine Protocol: an engine is a child process that
speaks typed frames over a socketpair, announced by a `typio-engine-*.toml`
manifest. That decision is not in question here and is already the only path the
host implements.

What is in question is that at least one shipped engine never completed the
migration. `typio-engine-sherpa` still carries two mutually incompatible
implementations of itself:

| Implementation | Files | State |
| :--- | :--- | :--- |
| Real inference against the retired C ABI | `src/sherpa.c`, `src/worker_main.c` | Cannot build: `meson` demands `typio-engine-abi`, a pkg-config module that no longer exists anywhere in the stack |
| Engine-Protocol frontend | `src/main.rs` | Builds, but `process_audio` unconditionally returns the empty string |

The consequence is not merely a missing feature, it is a **silently green
conformance gate**. `typio-engine-check` probes voice inference with
`Request::ProcessAudio(Vec::new())` — an empty payload — and accepts any reply that
contains a `TEXT` record at all, without inspecting whether the text is non-empty.
A stub that returns `Text("")` satisfies every assertion. Measured against the
shipped binary, the engine reports `Availability::Ready`, accepts 1.5 s of audio at
peak amplitude 0.60, and answers `Text("")`. No test fails.

`typio-engine-whisper` carries a byte-for-byte copy of the same stub, so this is a
systemic shape, not a one-off.

## Decision

An engine repository ships **exactly one** implementation of itself, and that
implementation is an Engine Protocol process. There is no in-process ABI path, no
fallback worker, and no second entry point.

Concretely:

1. **Delete the legacy ABI implementation outright.** `src/sherpa.c`,
   `src/worker_main.c`, and the `meson.build` that assembles them are removed. The
   real inference logic they contain is *ported*, not preserved in place: the
   useful part is a model-directory layout recogniser and a recognizer-config
   constructor, both of which are reimplemented against the sherpa-onnx C API from
   the Rust process.
2. **The process performs inference.** `process_audio` decodes 16 kHz mono f32
   little-endian PCM, runs sherpa-onnx offline recognition, and returns the
   transcript. A model that is absent or fails to load yields an explicit error,
   never an empty success.
3. **Build via Cargo only.** The engine is `cargo build --release`. sherpa-onnx is
   discovered through `pkg-config` and bound by a local `sherpa-onnx-sys` crate
   that runs `bindgen` against the shipped header. There is no `meson`, no
   `typio-engine-abi`, no `libcurl`/`libcrypto` link dependency for the recogniser.
4. **Vendor nothing.** The sys crate generates a **minimal** binding surface — the
   handful of types and functions the recogniser uses — instead of binding the
   entire 4700-line header. A pinned `SHERPA_ONNX_REF` and a bound declaration list
   make an upstream ABI drift a compile error at a known place.
5. **Fail loudly, always.** Absent model, unsupported model layout, undecodable
   audio, and a recognizer that returns NULL are all reported as
   `ReplyRecord::Error`. "Ready but produces nothing" must be unrepresentable in
   practice, not merely unlikely.

## Rationale

**Why port instead of repair.** The C worker cannot build: `typio-engine-abi` is
gone from the tree and from `pkg-config`, and the host has no `dlopen` path to load
an in-process engine. Restoring the ABI to make the worker compile would mean
reintroducing the boundary the protocol decision deliberately removed, so that one
engine could keep its old shape. Porting is less work *and* leaves one boundary
instead of two.

**Why a local sys crate rather than a registry crate.** No network is available in
the target build environment, so `sherpa-onnx-sys` cannot be fetched. More
importantly, a registry crate would bind the whole header and silently change shape
under us. A local crate that binds only what is called, against the header that
`pkg-config` actually found, turns an upstream rename into a compile error in a
four-line declaration block instead of an undefined-symbol at runtime.

**Why the conformance probe had to be fixed in the host repo.** An engine cannot be
trusted to police itself. The probe that is supposed to catch this class of defect
asserted presence, not substance, so it is the probe that must be strengthened. See
ADR-0055.

## Rejected Alternatives

- **Rebuild `typio-engine-abi` and ship the C worker.** Rejected: it resurrects a
  deleted boundary to avoid porting ~200 lines of configuration code, and it leaves
  the host with two engine-loading designs to keep working. The protocol boundary is
  the asset; the ABI is the debt.
- **Leave the stub and document it as "model not installed".** Rejected on evidence:
  the model *is* installed (SenseVoice int8, 239 MB, plus `tokens.txt`), sherpa-onnx
  1.13.5 is present, and the engine still returns `Text("")`. Labeling that
  "unavailable" would be a false statement about the system.
- **Shell out to a `sherpa-onnx` CLI binary in `process_audio`.** Rejected: it adds
  a per-utterance process spawn to the latency path, needs a second binary deployed
  beside the engine, and reproduces the same "is it really there?" question one
  layer further out. The C API is in-process and already a declared dependency.
- **Vendor the 4700-line header and bind it wholesale.** Rejected: the binding would
  be unreadable, would encode model families the engine never supports, and would
  break on every upstream struct addition. The recogniser needs five functions and
  four structs.
- **Keep `sherpa.c`'s curl-based model downloader.** Rejected: model acquisition is
  an install-time concern with its own integrity requirements (SHA-256, bounded
  redirects, atomic extraction). It does not belong on the inference path, and the
  engine must not silently fetch 239 MB while a user holds a key.

## Consequences

- `typio-engine-sherpa` produces real transcripts or a real error. The
  `Availability::Ready` / `Text("")` combination is no longer reachable.
- The repository builds with Cargo alone. CI drops the meson install-layout jobs and
  the `typio-core` build, and gains a fact: the engine must transcribe a known
  utterance.
- The sys crate's declaration list is the contract. Bumping `SHERPA_ONNX_REF` starts
  by re-checking that list against the new header.
- `typio-engine-whisper` is out of scope for this record and remains stubbed. It must
  be fixed or withdrawn; shipping a second stub that the same probe cannot detect is
  the exact failure this record exists to end. Tracked as follow-up work.
