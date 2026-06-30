use crate::c_api::registry::TypioRegistry;
use crate::config::Config;
use crate::input_context::TypioInputContext;
use crate::types::*;
use std::ffi::c_void;

pub(crate) struct RegistryPtr(pub *mut TypioRegistry);
impl Drop for RegistryPtr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            crate::c_api::registry::typio_registry_free(self.0);
        }
    }
}
#[allow(dead_code)]
impl RegistryPtr {
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
    pub fn as_ptr(&self) -> *mut TypioRegistry {
        self.0
    }
}

pub(crate) struct ConfigPtr(pub *mut Config);
impl Drop for ConfigPtr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            crate::config::typio_config_free(self.0);
        }
    }
}
#[allow(dead_code)]
impl ConfigPtr {
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
    pub fn as_ptr(&self) -> *mut Config {
        self.0
    }
}

pub(crate) struct InputContextPtr(pub *mut TypioInputContext);
impl Drop for InputContextPtr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            crate::input_context::typio_input_context_free(self.0);
        }
    }
}
#[allow(dead_code)]
impl InputContextPtr {
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
    pub fn as_ptr(&self) -> *mut TypioInputContext {
        self.0
    }
}

pub(crate) struct VoiceSessionPtr(pub *mut TypioVoiceSession);
impl Drop for VoiceSessionPtr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { crate::instance::typio_voice_session_free(self.0) };
        }
    }
}
#[allow(dead_code)]
impl VoiceSessionPtr {
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
    pub fn as_ptr(&self) -> *mut TypioVoiceSession {
        self.0
    }
}

pub(crate) struct InstanceLastMode(pub TypioKeyboardEngineMode);
impl Drop for InstanceLastMode {
    fn drop(&mut self) {
        crate::string::typio_free_string(self.0.id as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.0.label as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.0.display_label as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.0.icon_name as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.0.profile_id as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.0.profile_label as *mut std::ffi::c_char);
        crate::string::typio_free_string(self.0.description as *mut std::ffi::c_char);
    }
}

pub(crate) struct InstanceCallbacks {
    pub engine_changed: Option<TypioEngineChangedCallback>,
    pub engine_changed_user_data: *mut c_void,
    pub voice_engine_changed: Option<TypioVoiceEngineChangedCallback>,
    pub voice_engine_changed_user_data: *mut c_void,
    pub status_icon_changed: Option<TypioStatusIconChangedCallback>,
    pub status_icon_changed_user_data: *mut c_void,
    pub mode_changed: Option<TypioKeyboardModeChangedCallback>,
    pub mode_changed_user_data: *mut c_void,
    pub availability_changed: Option<TypioEngineAvailabilityChangedCallback>,
    pub availability_changed_user_data: *mut c_void,
    pub languages_changed: Option<TypioLanguagesChangedCallback>,
    pub languages_changed_user_data: *mut c_void,
}

impl Default for InstanceCallbacks {
    fn default() -> Self {
        Self {
            engine_changed: None,
            engine_changed_user_data: std::ptr::null_mut(),
            voice_engine_changed: None,
            voice_engine_changed_user_data: std::ptr::null_mut(),
            status_icon_changed: None,
            status_icon_changed_user_data: std::ptr::null_mut(),
            mode_changed: None,
            mode_changed_user_data: std::ptr::null_mut(),
            availability_changed: None,
            availability_changed_user_data: std::ptr::null_mut(),
            languages_changed: None,
            languages_changed_user_data: std::ptr::null_mut(),
        }
    }
}
