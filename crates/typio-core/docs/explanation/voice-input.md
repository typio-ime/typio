# Voice input

Voice input keeps capture, inference, and text delivery in separate ownership
domains:

```mermaid
flowchart LR
    K[push-to-talk policy] --> C[host PipeWire capture]
    C -->|owned audio buffer| S[voice session]
    S -->|ProcessAudio on fd 3| E[voice engine]
    E -->|TEXT reply| S
    S -->|owned VoiceEvent| H[host event loop]
    H -->|commit text| W[Wayland client]
```

The host captures bounded 16 kHz mono little-endian `f32` samples. A cloned
process handle keeps an in-flight worker transport alive without raw pointers
or unsafe thread ownership. Completion is returned through an owned eventfd
queue and applied on the main event loop.

The voice engine owns model loading, readiness, inference, and transcription.
It reports `AVAILABILITY` and accepts `ProcessAudio`; silence may legally
produce no `TEXT`. Switching or unloading registry slots does not invalidate a
job that already owns its handle, and shutdown joins or completes in-flight
work before runtime state is dropped.
