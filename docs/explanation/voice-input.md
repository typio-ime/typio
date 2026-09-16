# Voice input

Voice input keeps capture, inference, and text delivery in separate ownership
domains:

1. A push-to-talk policy decides when recording starts and stops.
2. The host captures audio through PipeWire and hands the voice session an owned
   audio buffer.
3. The voice session sends the audio to the voice engine over the private engine
   channel.
4. The engine replies with recognized text — or with nothing at all — and the
   session turns the outcome into an owned event.
5. The host event loop applies that event and commits the text to the focused
   Wayland client.

The host captures bounded 16 kHz mono little-endian float samples. A cloned
process handle keeps an in-flight worker transport alive without raw pointers
or unsafe thread ownership. Completion is returned through an owned event
queue and applied on the main event loop.

The voice engine owns model loading, readiness, inference, and transcription.
It reports its availability state and accepts audio requests; silence may
legally produce no text. Switching or unloading registry slots does not
invalidate a job that already owns its handle, and shutdown joins or completes
in-flight work before runtime state is dropped.
