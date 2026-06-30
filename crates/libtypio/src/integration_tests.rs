//! End-to-end integration tests for the out-of-process engine subsystem.

#[cfg(test)]
mod tests {
    use crate::c_api::registry::*;
    use crate::types::*;
    use std::ffi::{CStr, CString};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::ptr;

    struct TestWorker {
        path: CString,
        capture_path: Option<PathBuf>,
    }

    impl TestWorker {
        fn new(capture_key: bool) -> Self {
            Self::build(capture_key, None, None)
        }

        /// A worker that appends an `ACTIVE_MODE` line (id `native`, badge "中",
        /// salience Notable) to every `process-key` reply — the shape a real
        /// keyboard engine uses to report an internal mode flip.
        fn mode_reporting() -> Self {
            Self::build(false, Some(Self::NATIVE_MODE_TAIL), None)
        }

        /// A worker that reports *different* modes on the two host paths: the
        /// incidental focus path (id `ascii`, badge "A", Quiet) and the
        /// deliberate key path (id `native`, badge "中", Notable). Lets a test
        /// prove the announce split — focus must not fire the mode-changed
        /// callback, a keystroke must.
        fn mode_reporting_split() -> Self {
            Self::build(
                false,
                Some(Self::NATIVE_MODE_TAIL),
                Some(Self::ASCII_MODE_TAIL),
            )
        }

        /// `ACTIVE_MODE` field tail (hex id, label, badge, …, is_active,
        /// salience) for the two modes the tests exercise.
        /// id=native, label=Native, badge=中, is_active=1, salience=1 (Notable).
        const NATIVE_MODE_TAIL: &'static str =
            "6e6174697665\\t4e6174697665\\te4b8ad\\t\\t\\t\\t\\t1\\t1";
        /// id=ascii, label=ascii, badge=A, is_active=1, salience=0 (Quiet).
        const ASCII_MODE_TAIL: &'static str = "6173636969\\t6173636969\\t41\\t\\t\\t\\t\\t1\\t0";

        /// Build a stub worker. `key_mode`/`focus_mode` are `ACTIVE_MODE` field
        /// tails appended to the `process-key` / `focus-in` replies; `None`
        /// leaves that path mode-silent.
        fn build(capture_key: bool, key_mode: Option<&str>, focus_mode: Option<&str>) -> Self {
            let stem = format!(
                "typio-libtypio-integration-{}-{}",
                std::process::id(),
                std::thread::current().name().unwrap_or("unnamed")
            );
            let script_path = std::env::temp_dir().join(format!("{stem}.py"));
            let capture_path =
                capture_key.then(|| std::env::temp_dir().join(format!("{stem}.key")));
            let capture_path_literal = capture_path
                .as_ref()
                .map(|path| format!("{:?}", path.to_string_lossy()))
                .unwrap_or_else(|| "None".to_string());
            let key_mode_literal = key_mode
                .map(|tail| format!("{:?}", tail.replace("\\t", "\t")))
                .unwrap_or_else(|| "None".to_string());
            let focus_mode_literal = focus_mode
                .map(|tail| format!("{:?}", tail.replace("\\t", "\t")))
                .unwrap_or_else(|| "None".to_string());
            let script = r#"#!/usr/bin/env python3
import os
import struct
import sys

MAGIC = 0x54594550
MAJOR = 1
MINOR = 0
ENGINE_HELLO = 1
HOST_HELLO = 2
REQUEST = 3
RESPONSE = 4
ERROR = 6

name = sys.argv[1]
engine_type = sys.argv[2]
capture_path = @CAPTURE_PATH@
key_mode = @KEY_MODE@
focus_mode = @FOCUS_MODE@
fd = int(os.environ.get("TYPIO_ENGINE_FD", "3"))

def read_exact(n):
    data = b""
    while len(data) < n:
        chunk = os.read(fd, n - len(data))
        if not chunk:
            raise SystemExit(0)
        data += chunk
    return data

def read_frame():
    header = read_exact(28)
    magic, major, minor, msg_type, flags, request_id, payload_len = struct.unpack("!IHHIIQI", header)
    if magic != MAGIC or major != MAJOR:
        raise SystemExit(2)
    return msg_type, request_id, read_exact(payload_len)

def write_frame(msg_type, request_id, payload):
    os.write(fd, struct.pack("!IHHIIQI", MAGIC, MAJOR, MINOR, msg_type, 0, request_id, len(payload)) + payload)

write_frame(ENGINE_HELLO, 0, f"protocol\t1.0\nengine\t{name}\ntype\t{engine_type}".encode())
msg_type, request_id, payload = read_frame()
if msg_type != HOST_HELLO:
    write_frame(ERROR, request_id, b"expected host hello")
    raise SystemExit(2)

while True:
    msg_type, request_id, payload = read_frame()
    if msg_type != REQUEST:
        write_frame(ERROR, request_id, b"expected request")
        continue
    line = payload.decode()
    if line == "shutdown":
        raise SystemExit(0)
    if line == "availability":
        response = "AVAILABILITY\tREADY\n"
    elif line.startswith("process-key"):
        if capture_path is not None:
            with open(capture_path, "w", encoding="utf-8") as f:
                f.write(line + "\n")
        response = "RESULT\tHANDLED\n"
        if key_mode is not None:
            response += "ACTIVE_MODE\t" + key_mode + "\n"
    elif line.startswith("process-audio"):
        response = "TEXT\t6f6b\n"
    elif line == "list-modes":
        response = "MODE\t636f6d706f7365\t436f6d706f7365\t416263\t\t\t\t1\n"
    elif line.startswith("get-active-mode"):
        response = "ACTIVE_MODE\t636f6d706f7365\t436f6d706f7365\t416263\t\t\t\t1\n"
    elif line.startswith("focus-in") and focus_mode is not None:
        response = "ACTIVE_MODE\t" + focus_mode + "\n"
    else:
        response = "OK\n"
    write_frame(RESPONSE, request_id, response.encode())
"#
            .replace("@CAPTURE_PATH@", &capture_path_literal)
            .replace("@KEY_MODE@", &key_mode_literal)
            .replace("@FOCUS_MODE@", &focus_mode_literal);
            fs::write(&script_path, script).unwrap();
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
            Self {
                path: CString::new(script_path.to_string_lossy().as_bytes()).unwrap(),
                capture_path,
            }
        }

        fn argv(&self, info: &TestEngineInfo) -> [*const i8; 4] {
            [
                self.path.as_ptr(),
                info.name.as_ptr(),
                c"keyboard".as_ptr(),
                ptr::null(),
            ]
        }
    }

    struct TestEngineInfo {
        name: CString,
        _display_name: CString,
        _description: CString,
        _author: CString,
        _language: CString,
        _icon: Option<CString>,
        info: TypioEngineInfo,
    }

    impl TestEngineInfo {
        fn keyboard(name: &str, icon: Option<&str>) -> Self {
            let name = CString::new(name).unwrap();
            let display_name = CString::new(format!("{} Display", name.to_str().unwrap())).unwrap();
            let description = CString::new("Process test engine").unwrap();
            let author = CString::new("Test").unwrap();
            let language = CString::new("und").unwrap();
            let icon = icon.map(|s| CString::new(s).unwrap());
            let info = TypioEngineInfo {
                name: name.as_ptr(),
                display_name: display_name.as_ptr(),
                description: description.as_ptr(),
                author: author.as_ptr(),
                icon: icon.as_ref().map(|s| s.as_ptr()).unwrap_or(ptr::null()),
                language: language.as_ptr(),
                type_: TypioEngineType::TypioEngineTypeKeyboard,
                required_capabilities: ptr::null(),
                optional_capabilities: ptr::null(),
            };
            Self {
                name,
                _display_name: display_name,
                _description: description,
                _author: author,
                _language: language,
                _icon: icon,
                info,
            }
        }
    }

    fn register_process(reg: *mut TypioRegistry, info: &TestEngineInfo, worker: &TestWorker) {
        let argv = worker.argv(info);
        assert_eq!(
            typio_registry_register_engine_process(reg, &info.info, argv.as_ptr()),
            TypioResult::TypioOk
        );
    }

    #[test]
    fn e2e_registry_register_process_and_activate() {
        let reg = typio_registry_new(ptr::null_mut());
        assert!(!reg.is_null());
        let worker = TestWorker::new(false);
        let info = TestEngineInfo::keyboard("handling", None);
        register_process(reg, &info, &worker);

        assert_eq!(
            typio_registry_set_active_keyboard(reg, info.name.as_ptr()),
            TypioResult::TypioOk
        );

        let active = typio_registry_get_active_keyboard(reg);
        assert!(!active.is_null());
        assert_eq!(
            unsafe { CStr::from_ptr(active) }.to_str().unwrap(),
            "handling"
        );
        crate::string::typio_free_string(active);
        typio_registry_free(reg);
    }

    #[test]
    fn e2e_registry_list_and_unload_process() {
        let reg = typio_registry_new(ptr::null_mut());
        assert!(!reg.is_null());
        let worker = TestWorker::new(false);
        let info = TestEngineInfo::keyboard("handling", None);
        register_process(reg, &info, &worker);

        let mut count: usize = 0;
        let list = typio_registry_list_keyboards(reg, &mut count);
        assert_eq!(count, 1);
        assert_eq!(
            unsafe { CStr::from_ptr(*list) }.to_str().unwrap(),
            "handling"
        );
        crate::string::typio_free_string_array(list, count);

        assert_eq!(
            typio_registry_unload(reg, info.name.as_ptr()),
            TypioResult::TypioOk
        );
        let list = typio_registry_list_keyboards(reg, &mut count);
        assert!(list.is_null() || count == 0);
        crate::string::typio_free_string_array(list, count);
        typio_registry_free(reg);
    }

    #[test]
    fn e2e_registry_language_switching() {
        let reg = typio_registry_new(ptr::null_mut());
        assert!(!reg.is_null());
        let worker = TestWorker::new(false);
        let rime = TestEngineInfo::keyboard("rime", None);
        let anthy = TestEngineInfo::keyboard("anthy", None);
        register_process(reg, &rime, &worker);
        register_process(reg, &anthy, &worker);

        // No declared languages yet: cycling reports NotFound so hosts can
        // fall back to engine cycling (ADR-0018).
        assert_eq!(
            typio_registry_next_language(reg),
            TypioResult::TypioErrorNotFound
        );

        let zh = [
            CString::new("zh-Hans").unwrap(),
            CString::new("zh-Hant").unwrap(),
        ];
        let zh_ptrs = [zh[0].as_ptr(), zh[1].as_ptr(), ptr::null()];
        assert_eq!(
            typio_registry_set_engine_languages(reg, rime.name.as_ptr(), zh_ptrs.as_ptr()),
            TypioResult::TypioOk
        );
        let ja = [CString::new("ja").unwrap()];
        let ja_ptrs = [ja[0].as_ptr(), ptr::null()];
        assert_eq!(
            typio_registry_set_engine_languages(reg, anthy.name.as_ptr(), ja_ptrs.as_ptr()),
            TypioResult::TypioOk
        );

        // Enabled cycle falls back to declared languages (no config here).
        let mut count: usize = 0;
        let langs = typio_registry_list_languages(reg, &mut count);
        assert_eq!(count, 3); // zh-Hans, zh-Hant, ja
        crate::string::typio_free_string_array(langs, count);

        // Cycle: lands on the first declared language and activates rime.
        assert_eq!(typio_registry_next_language(reg), TypioResult::TypioOk);
        let active_lang = typio_registry_get_active_language(reg);
        assert_eq!(
            unsafe { CStr::from_ptr(active_lang) }.to_str().unwrap(),
            "zh-Hans"
        );
        crate::string::typio_free_string(active_lang);
        let active_kb = typio_registry_get_active_keyboard(reg);
        assert_eq!(
            unsafe { CStr::from_ptr(active_kb) }.to_str().unwrap(),
            "rime"
        );
        crate::string::typio_free_string(active_kb);

        // Direct activation by tag retargets the keyboard slot.
        let ja_tag = CString::new("ja").unwrap();
        assert_eq!(
            typio_registry_set_active_language(reg, ja_tag.as_ptr()),
            TypioResult::TypioOk
        );
        let active_kb = typio_registry_get_active_keyboard(reg);
        assert_eq!(
            unsafe { CStr::from_ptr(active_kb) }.to_str().unwrap(),
            "anthy"
        );
        crate::string::typio_free_string(active_kb);

        // Layout-only language: no engine declares it, keyboard slot empties.
        let darija = CString::new("ar-MA").unwrap();
        assert_eq!(
            typio_registry_set_active_language(reg, darija.as_ptr()),
            TypioResult::TypioOk
        );
        let active_kb = typio_registry_get_active_keyboard(reg);
        assert!(active_kb.is_null());

        typio_registry_free(reg);
    }

    #[test]
    fn e2e_registry_engine_metadata_getters() {
        let reg = typio_registry_new(ptr::null_mut());
        assert!(!reg.is_null());
        let worker = TestWorker::new(false);
        let info = TestEngineInfo::keyboard("handling", None);
        register_process(reg, &info, &worker);

        let display_name = typio_registry_get_engine_display_name(reg, info.name.as_ptr());
        assert!(!display_name.is_null());
        assert_eq!(
            unsafe { CStr::from_ptr(display_name) }.to_str().unwrap(),
            "handling Display"
        );
        crate::string::typio_free_string(display_name);

        let icon = typio_registry_get_engine_icon(reg, info.name.as_ptr());
        assert!(icon.is_null());

        let description = typio_registry_get_engine_description(reg, info.name.as_ptr());
        assert!(!description.is_null());
        assert_eq!(
            unsafe { CStr::from_ptr(description) }.to_str().unwrap(),
            "Process test engine"
        );
        crate::string::typio_free_string(description);

        let bad_icon = TestEngineInfo::keyboard("bad-icon", Some("http://evil.example/icon.png"));
        register_process(reg, &bad_icon, &worker);
        let bad_icon_ptr = typio_registry_get_engine_icon(reg, bad_icon.name.as_ptr());
        assert!(bad_icon_ptr.is_null());

        typio_registry_free(reg);
    }

    #[test]
    fn e2e_process_key_preserves_all_event_fields_over_process() {
        let inst = crate::instance::typio_instance_new();
        assert!(!inst.is_null());
        crate::instance::typio_instance_init(inst);

        let reg = crate::instance::typio_instance_get_registry(inst);
        assert!(!reg.is_null());
        let worker = TestWorker::new(true);
        let info = TestEngineInfo::keyboard("recorder", None);
        register_process(reg, &info, &worker);
        assert_eq!(
            typio_registry_set_active_keyboard(reg, info.name.as_ptr()),
            TypioResult::TypioOk
        );

        let ctx = crate::instance::typio_instance_create_context(inst);
        assert!(!ctx.is_null());
        let modifiers = TypioModifier::TypioModShift as u32 | TypioModifier::TypioModCtrl as u32;
        let event = TypioKeyEvent {
            struct_size: std::mem::size_of::<TypioKeyEvent>(),
            type_: TypioEventType::TypioEventKeyPress,
            keycode: 38,
            keysym: 0x61,
            modifiers,
            unicode: 0x41,
            time: 123_456_789,
            is_repeat: true,
            base_keysym: 0x61,
        };

        assert!(crate::input_context::typio_input_context_process_key(
            ctx, &event
        ));

        let capture = fs::read_to_string(worker.capture_path.as_ref().unwrap()).unwrap();
        let fields: Vec<&str> = capture.trim_end().split('\t').collect();
        assert_eq!(fields[0], "process-key");
        assert_eq!(fields[2], "press");
        assert_eq!(fields[3], "38");
        assert_eq!(fields[4], "97");
        assert_eq!(fields[5], modifiers.to_string());
        assert_eq!(fields[6], "65");
        assert_eq!(fields[7], "123456789");
        assert_eq!(fields[8], "1");
        assert_eq!(fields[9], "97");

        crate::instance::typio_instance_free(inst);
    }

    struct ModeCapture {
        count: u32,
        display: Option<String>,
        salience: TypioStatusSalience,
    }

    extern "C" fn capture_mode(
        _instance: *mut typio_abi::TypioInstance,
        mode: *const TypioKeyboardEngineMode,
        user_data: *mut std::os::raw::c_void,
    ) {
        let cap = unsafe { &mut *(user_data as *mut ModeCapture) };
        let m = unsafe { &*mode };
        cap.count += 1;
        cap.display = (!m.display_label.is_null()).then(|| {
            unsafe { CStr::from_ptr(m.display_label) }
                .to_string_lossy()
                .into_owned()
        });
        cap.salience = m.salience;
    }

    /// A `process-key` whose reply carries `ACTIVE_MODE` fires the deliberate
    /// mode-changed callback once, with the reported badge and salience, and
    /// de-duplicates identical follow-up reports.
    #[test]
    fn e2e_process_key_active_mode_drives_host_callback() {
        let inst = crate::instance::typio_instance_new();
        assert!(!inst.is_null());
        crate::instance::typio_instance_init(inst);

        let mut capture = ModeCapture {
            count: 0,
            display: None,
            salience: TypioStatusSalience::TypioStatusSalienceQuiet,
        };
        crate::instance::typio_instance_set_keyboard_mode_changed_callback(
            inst,
            capture_mode,
            &mut capture as *mut ModeCapture as *mut std::os::raw::c_void,
        );

        let reg = crate::instance::typio_instance_get_registry(inst);
        let worker = TestWorker::mode_reporting();
        let info = TestEngineInfo::keyboard("moder", None);
        register_process(reg, &info, &worker);
        assert_eq!(
            typio_registry_set_active_keyboard(reg, info.name.as_ptr()),
            TypioResult::TypioOk
        );

        let ctx = crate::instance::typio_instance_create_context(inst);
        let event = TypioKeyEvent {
            struct_size: std::mem::size_of::<TypioKeyEvent>(),
            type_: TypioEventType::TypioEventKeyPress,
            keycode: 38,
            keysym: 0x61,
            modifiers: 0,
            unicode: 0x61,
            time: 1,
            is_repeat: false,
            base_keysym: 0x61,
        };

        crate::input_context::typio_input_context_process_key(ctx, &event);
        assert_eq!(capture.count, 1, "first mode report must fire the callback");
        assert_eq!(capture.display.as_deref(), Some("中"));
        assert_eq!(
            capture.salience,
            TypioStatusSalience::TypioStatusSalienceNotable
        );

        // Same mode reported again → de-duplicated, callback not re-fired.
        crate::input_context::typio_input_context_process_key(ctx, &event);
        assert_eq!(capture.count, 1, "identical mode must not re-fire");

        crate::instance::typio_instance_free(inst);
    }

    /// Read the cached active-mode id, or `None` when unset.
    fn last_mode_id(inst: *mut crate::instance::TypioInstance) -> Option<String> {
        let mode = crate::instance::typio_instance_get_last_keyboard_mode(inst);
        if mode.is_null() {
            return None;
        }
        let id = unsafe { (*mode).id };
        (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned())
    }

    /// The two host paths must diverge: an *incidental* mode report (focus-in)
    /// refreshes the cached mode without firing the deliberate mode-changed
    /// callback, while a *deliberate* one (process-key) both updates the cache
    /// and fires the callback. This is the contract the indicator's salience
    /// gate and the tray both depend on.
    #[test]
    fn e2e_focus_is_incidental_keystroke_is_deliberate() {
        let inst = crate::instance::typio_instance_new();
        assert!(!inst.is_null());
        crate::instance::typio_instance_init(inst);

        let mut capture = ModeCapture {
            count: 0,
            display: None,
            salience: TypioStatusSalience::TypioStatusSalienceQuiet,
        };
        crate::instance::typio_instance_set_keyboard_mode_changed_callback(
            inst,
            capture_mode,
            &mut capture as *mut ModeCapture as *mut std::os::raw::c_void,
        );

        let reg = crate::instance::typio_instance_get_registry(inst);
        let worker = TestWorker::mode_reporting_split();
        let info = TestEngineInfo::keyboard("splitter", None);
        register_process(reg, &info, &worker);
        assert_eq!(
            typio_registry_set_active_keyboard(reg, info.name.as_ptr()),
            TypioResult::TypioOk
        );

        let ctx = crate::instance::typio_instance_create_context(inst);

        // Incidental: focus-in reports the `ascii` mode. The cache updates but
        // the deliberate callback stays silent — the host's focus path owns the
        // salience decision and must not be pre-empted.
        crate::input_context::typio_input_context_focus_in(ctx);
        assert_eq!(
            capture.count, 0,
            "focus-in must not fire the deliberate callback"
        );
        assert_eq!(last_mode_id(inst).as_deref(), Some("ascii"));

        // Deliberate: a keystroke flips to `native`. The callback fires once
        // with the new badge/salience, and the cache follows.
        let event = TypioKeyEvent {
            struct_size: std::mem::size_of::<TypioKeyEvent>(),
            type_: TypioEventType::TypioEventKeyPress,
            keycode: 38,
            keysym: 0x61,
            modifiers: 0,
            unicode: 0x61,
            time: 1,
            is_repeat: false,
            base_keysym: 0x61,
        };
        crate::input_context::typio_input_context_process_key(ctx, &event);
        assert_eq!(
            capture.count, 1,
            "keystroke mode flip must fire the callback"
        );
        assert_eq!(capture.display.as_deref(), Some("中"));
        assert_eq!(
            capture.salience,
            TypioStatusSalience::TypioStatusSalienceNotable
        );
        assert_eq!(last_mode_id(inst).as_deref(), Some("native"));

        crate::instance::typio_instance_free(inst);
    }
}
