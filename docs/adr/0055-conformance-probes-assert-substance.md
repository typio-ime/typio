# ADR-0055: Conformance probes assert substance, not presence

- **Status**: Accepted
- **Date**: 2026-10-29
- **Scope**: engine
- **Deciders**: Typio maintainers
- **Related RFC**: Not used — this repository does not operate an RFC workflow

## Context

`typio-engine-check` is the black-box gate for engine processes. It starts the
manifest's command in an isolated process, performs the Engine Protocol handshake,
exercises runtime requests, and reports pass/warn/fail. Its output is trusted: CI
gates on it, and a green run is taken as evidence that an engine works.

The voice-inference check does not support that trust. It is:

```rust
match session.request(&Request::ProcessAudio(Vec::new())) {
    Ok(reply) => {
        if let Some(error) = reply_error(&reply) {
            // fail
        } else if reply.records.iter().any(|r| matches!(r, ReplyRecord::Text(_))) {
            // PASS
        } else {
            // WARN: "empty-audio probe returned no TEXT record"
        }
    }
    ...
}
```

Two defects are structural, not incidental:

1. **The probe sends no audio.** `Vec::new()` is an empty payload. A recogniser is
   entitled to return an empty transcript for zero samples, so the probe cannot
   distinguish a working engine from one that returns an empty string for every
   input.
2. **The assertion is on the record's *type*, not its *content*.** `Text("")` and
   `Text("hello world")` both satisfy `matches!(.., ReplyRecord::Text(_))`. The
   pattern `Text(_)` is the whole defect: it accepts the exact shape produced by an
   unimplemented engine.

Measured consequence: `typio-engine-sherpa` returns `Availability::Ready` and
`Text("")` for 1.5 s of audio at peak amplitude 0.60, and `typio-engine-check`
reports **4 passed, 0 warnings, 0 failed** on the behavior dimension and **9
passed** on protocol. The gate is green over a non-functional engine.

## Decision

A conformance probe must exercise the engine with an input that a working
implementation cannot trivially satisfy, and must assert on the substance of the
reply rather than on the presence of a record type.

For voice inference this means:

1. **The check is tiered, and the tier is reported.** Absent a model, an engine is
   legitimately not inferring, so the check must say so rather than pass silently:
   - **Required by declaration.** An engine whose manifest declares the
     `voice_input` capability is asserting it can transcribe. The probe then sends
     real non-silent audio and requires a non-empty transcript. Silence, an empty
     `Text`, or a bare `Ok` is a **failure**, not a warning.
   - **Not declared.** An engine that does not claim `voice_input` is not held to
     transcription. The probe sends the same audio and requires a well-formed reply
     that is either non-empty text or an explicit error. A silent empty success is
     still a **failure**, because it is indistinguishable from a lie.
2. **The probe audio is synthetic and deterministic.** A generated waveform with a
   known peak amplitude is used instead of a checked-in fixture: it cannot go stale,
   needs no binary asset, and its only job is to prove the engine consumed non-empty
   input. The assertion is on *non-emptiness and error-freedom*, not on a particular
   transcription string, because no engine-independent expected string exists for
   synthetic audio.
3. **`Text("")` is never a pass.** An empty transcript paired with `Ok` is an
   explicit failure. An engine with nothing to say must say so with an error.
4. **A skipped capability check is visible.** When a check cannot run, it reports
   that it was skipped and why. It never reports `pass` for work it did not do.

## Rationale

**Why not compare against a known-good transcript.** Doing so would require shipping
a speech fixture and pinning every supported model's exact output, which couples the
gate to model revisions and to tokenizer normalisation. That is a large, brittle
obligation for a check whose real question is "does the engine consume audio and
produce text at all?" The cheap, durable question is the right one: non-empty input
in, non-empty text out, or an explicit error.

**Why `Text("")` must fail even when nothing is declared.** The dangerous failure
mode is not a missing feature, it is a *present-looking* feature. An engine that
reports Ready and returns empty text will be wired into a user's push-to-talk key and
appear broken in the field, with no diagnostic. Making the empty success a hard
failure forces an engine to pick a story: transcribe, or refuse.

**Why the tiering.** Holding a genuinely not-installed model to a hard failure would
make the gate environment-dependent and encourage `--ignore` flags, which is how
gates die. Tiering on the engine's own declaration keeps the assertion honest — an
engine is judged against what it promised — while still banning the silent empty
success everywhere.

## Rejected Alternatives

- **Keep the presence check and add a separate "real inference" test.** Rejected: two
  checks where one is known to be vacuous invites the vacuous one to be cited. The
  single probe becomes substantive instead.
- **Require an exact transcript from a bundled WAV fixture.** Rejected: brittle
  across model revisions and tokenizer settings, and it makes the gate depend on a
  binary asset that can be silently regenerated wrong.
- **Treat a missing model as a warning so CI stays green.** Rejected: it recreates the
  original defect in a different branch. An engine that has not loaded a model must
  not answer `Ready`.
- **Let each engine repository self-certify transcription.** Rejected: this is exactly
  the arrangement that produced two shipped stubs. The gate lives in the host repo
  precisely so it is not owned by the code under test.
- **Add a `--allow-stub` escape hatch.** Rejected: a flag that turns the only
  meaningful voice assertion off will be set once and never removed. If an engine is
  not ready, its manifest should not claim `voice_input`.

## Consequences

- `typio-engine-sherpa` must transcribe or fail; the stub shape is now a gate failure.
  Verified against the rebuilt engine.
- `typio-engine-whisper` currently fails the strengthened gate. That is the correct
  outcome: it is a stub, and the gate now says so. It must be implemented or have
  `voice_input` removed from its manifest.
- Engine authors get an actionable message ("produced an empty transcript for
  non-empty audio") instead of a silent pass.
- The probe carries a generated-audio helper. It asserts error-freedom and
  non-emptiness, so it does not encode any model's expected output.
