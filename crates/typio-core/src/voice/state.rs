//! Voice session state machine helpers and event dispatch.

use crate::voice::types::{TypioVoiceSessionEvent, TypioVoiceSessionEventType, VoiceState};
use std::sync::atomic::Ordering;

use crate::voice::session::VoiceSession;

impl VoiceSession {
    pub(crate) fn fire_event(&self, event: TypioVoiceSessionEvent) {
        let callback = *self.callback.lock().unwrap();
        if let Some(cb) = callback {
            cb(&event, self.callback_user_data.load(Ordering::SeqCst));
        }
        // For Result events the callback takes ownership of `text` (freed by
        // the host via free()). For all other events text is null, so the
        // free is a no-op — but skip it anyway to keep the contract clear.
        if event.type_ != TypioVoiceSessionEventType::Result || callback.is_none() {
            crate::string::typio_free_string(event.text);
        }
    }

    pub(crate) fn fire_state_change(&self, state: VoiceState) {
        let event = TypioVoiceSessionEvent {
            type_: TypioVoiceSessionEventType::StateChange,
            state,
            text: std::ptr::null_mut(),
            error: std::ptr::null(),
        };
        self.fire_event(event);
    }
}
