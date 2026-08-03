//! System-tray helpers.
//!
//! Free functions that translate between the live typio-core registry and
//! the Rust-side [`Tray`] / [`RegistrySnapshot`] surfaces. Split out of
//! `app/mod.rs` so the daemon's main loop file doesn't carry the registry
//! lookup boilerplate.

use crate::ipc_bus::TypioRegistryView;
use crate::runtime::SharedInstance;
use crate::service::SvcError;
use crate::state_controller::StateController;
#[cfg(feature = "systray")]
use crate::tray_menu::{EngineDesc, RegistrySnapshot};
#[cfg(feature = "systray")]
use crate::tray_sni::{MenuAction, Tray, TrayAction};

use super::{DaemonEvent, DaemonEventSender};

/// Wire tray callbacks to typed daemon events. Registry mutations happen only
/// when the main loop applies the resulting action.
#[cfg(feature = "systray")]
pub(super) fn install_tray_action_handler(tray: &Tray, event_tx: DaemonEventSender) {
    tray.set_action_handler(move |action| {
        let event = match action {
            TrayAction::Menu(MenuAction::Restart) => DaemonEvent::Restart,
            TrayAction::Menu(MenuAction::Quit) => DaemonEvent::Shutdown,
            action => DaemonEvent::TrayAction(action),
        };
        let _ = event_tx.send(event);
    });
}

/// Apply a tray action on the main loop. Returns whether registry-derived
/// surfaces need refreshing.
#[cfg(feature = "systray")]
pub(super) fn apply_tray_action(
    instance: Option<&SharedInstance>,
    action: TrayAction,
    _event_tx: &DaemonEventSender,
) -> bool {
    let Some(instance) = instance else {
        return false;
    };
    match action {
        TrayAction::Menu(MenuAction::Language(index)) => {
            language_at_index(instance, index as usize)
                .is_some_and(|tag| set_active_language(instance, &tag).is_ok())
        }
        TrayAction::Menu(MenuAction::EngineInLanguage {
            lang_idx,
            engine_idx,
        }) => {
            let tag = language_at_index(instance, lang_idx as usize);
            let name = keyboard_at_index(instance, engine_idx as usize);
            tag.zip(name)
                .is_some_and(|(tag, name)| set_language_keyboard(instance, &tag, &name).is_ok())
        }
        TrayAction::Menu(MenuAction::OrphanEngine(index)) => {
            orphan_keyboard_at_index(instance, index as usize)
                .is_some_and(|name| set_active_keyboard(instance, &name).is_ok())
        }
        TrayAction::Menu(MenuAction::Voice(index)) => voice_at_index(instance, index as usize)
            .is_some_and(|name| set_active_voice(instance, &name).is_ok()),
        _ => false,
    }
}

/// Push the current controller state (active engine name, status icon /
/// badge, full menu snapshot) into the tray.
#[cfg(feature = "systray")]
pub(super) fn update_tray_from_controller(
    tray: &Tray,
    controller: &StateController<TypioRegistryView>,
    instance: &SharedInstance,
) {
    tray.update_engine(controller.active_engine_name(), controller.engine_active());
    if controller.status_icon_is_badge() {
        tray.set_badge(controller.status_badge_text());
    } else {
        tray.set_icon(Some(controller.status_icon()));
    }
    if let Some(snapshot) = build_tray_snapshot(instance) {
        tray.set_menu_snapshot(snapshot);
    }
}

/// Build the menu snapshot from the live registry: known languages,
/// per-language keyboards, voice engines, and the active selections.
#[cfg(feature = "systray")]
pub(super) fn build_tray_snapshot(instance: &SharedInstance) -> Option<RegistrySnapshot> {
    let inst = instance.borrow();
    let reg = inst.registry_rust()?;
    let languages = reg.known_languages();
    let mut keyboards = Vec::new();
    for name in reg.list_keyboards() {
        let info = reg.engine_info(name)?;
        keyboards.push(EngineDesc {
            name: name.to_string(),
            display_name: Some(info.display_name.clone()),
            languages: info
                .effective_languages()
                .iter()
                .map(|s| s.to_string())
                .collect(),
        });
    }
    let mut voices = Vec::new();
    for name in reg.list_voices() {
        let info = reg.engine_info(name)?;
        voices.push(EngineDesc {
            name: name.to_string(),
            display_name: Some(info.display_name.clone()),
            languages: Vec::new(),
        });
    }
    Some(RegistrySnapshot {
        languages,
        active_language: reg.active_language().map(str::to_string),
        keyboards,
        voices,
        active_voice: reg.active_voice_name().map(str::to_string),
    })
}

pub(super) fn language_at_index(instance: &SharedInstance, idx: usize) -> Option<String> {
    let inst = instance.borrow();
    let reg = inst.registry_rust()?;
    reg.known_languages().get(idx).cloned()
}

pub(super) fn keyboard_at_index(instance: &SharedInstance, idx: usize) -> Option<String> {
    let inst = instance.borrow();
    let reg = inst.registry_rust()?;
    reg.list_keyboards().get(idx).map(|n| n.to_string())
}

pub(super) fn orphan_keyboard_at_index(instance: &SharedInstance, idx: usize) -> Option<String> {
    let inst = instance.borrow();
    let reg = inst.registry_rust()?;
    let known: std::collections::HashSet<String> = reg.known_languages().into_iter().collect();
    let orphans: Vec<String> = reg
        .list_keyboards()
        .into_iter()
        .filter(|name| {
            reg.engine_info(name)
                .map(|info| {
                    info.effective_languages()
                        .iter()
                        .all(|l| !known.contains(l))
                })
                .unwrap_or(true)
        })
        .map(str::to_string)
        .collect();
    orphans.get(idx).cloned()
}

pub(super) fn voice_at_index(instance: &SharedInstance, idx: usize) -> Option<String> {
    let inst = instance.borrow();
    let reg = inst.registry_rust()?;
    reg.list_voices().get(idx).map(|n| n.to_string())
}

pub(super) fn set_active_language(instance: &SharedInstance, tag: &str) -> Result<(), SvcError> {
    instance
        .borrow_mut()
        .activate_language(tag)
        .map_err(|_| SvcError)
}

pub(super) fn set_language_keyboard(
    instance: &SharedInstance,
    tag: &str,
    name: &str,
) -> Result<(), SvcError> {
    match instance.borrow_mut().activate_language_keyboard(tag, name) {
        Ok(()) => {
            tracing::debug!(target: "typio.tray", language = %tag, keyboard = %name, "active language and keyboard changed atomically");
            Ok(())
        }
        _ => {
            tracing::warn!(target: "typio.tray", language = %tag, keyboard = %name, "set_language_keyboard failed");
            Err(SvcError)
        }
    }
}

pub(super) fn set_active_keyboard(instance: &SharedInstance, name: &str) -> Result<(), SvcError> {
    let result = instance
        .borrow_mut()
        .registry_rust_mut()
        .ok_or(SvcError)?
        .activate_keyboard(name);
    match result {
        Ok(()) => {
            tracing::debug!(target: "typio.tray", keyboard = %name, "active keyboard changed");
            Ok(())
        }
        _ => {
            tracing::warn!(target: "typio.tray", keyboard = %name, "set_active_keyboard failed");
            Err(SvcError)
        }
    }
}

/// Cycle to the next registered keyboard engine, called when the user
/// presses the Ctrl+Shift engine-switch chord. Wraps from last back to
/// first; if only one keyboard is registered, the call is a no-op.
pub(super) fn cycle_active_keyboard(instance: &SharedInstance) {
    if let Some(mut registry) = instance.borrow_mut().registry_rust_mut() {
        let _ = registry.switch_keyboard(typio::core::registry::SwitchDirection::Next);
    }
}

/// Cycle to the next enabled language, called when the user presses the
/// Ctrl+Shift switch chord. The language cycle reuses the engine last used
/// for the target language (typio-core's per-language memory). When fewer than
/// two languages are enabled/declared there is nothing to cycle, so this
/// falls back to [`cycle_active_keyboard`] — keeping the chord useful in
/// single-language, multi-engine setups.
pub(super) fn cycle_active_language(instance: &SharedInstance) {
    let result = {
        instance
            .borrow_mut()
            .cycle_language(typio::core::registry::SwitchDirection::Next)
    };
    match result {
        Ok(()) => {
            tracing::debug!(target: "typio.tray", "active language cycled");
        }
        _ => {
            // No cycleable language (none or single): cycle engines instead.
            cycle_active_keyboard(instance);
        }
    }
}

pub(super) fn set_active_voice(instance: &SharedInstance, name: &str) -> Result<(), SvcError> {
    instance
        .borrow_mut()
        .registry_rust_mut()
        .ok_or(SvcError)?
        .activate_voice(name)
        .map_err(|_| SvcError)
}
