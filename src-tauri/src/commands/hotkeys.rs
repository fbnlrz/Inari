//! Global (system-wide) hotkeys.
//!
//! Two deliberate choices live here. Nothing is bound out of the box - see
//! [`crate::persistence::prefs::Hotkeys`] - and a failed grab is reported
//! instead of swallowed: on Wayland the X11 grab the underlying crate uses
//! either fails outright or succeeds against XWayland and then never sees a
//! key pressed in a native Wayland client. A binding that looks active but is
//! dead is worse than no binding, so the registration error is kept per action
//! and shown in Settings.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::MutexGuard;

use log::{info, warn};
use serde::Serialize;
use tauri::{Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::persistence::prefs::{HotkeyAction, Hotkeys};
use crate::state::AppState;

/// Per-action registration errors from the last [`apply`]. Not persisted:
/// whether the OS grants a grab is a property of the session, not of the
/// user's configuration.
#[derive(Default)]
pub struct HotkeyErrors(Mutex<HashMap<HotkeyAction, String>>);

impl HotkeyErrors {
    fn lock(&self) -> Result<MutexGuard<'_, HashMap<HotkeyAction, String>>, String> {
        self.0
            .lock()
            .map_err(|_| "hotkey state lock poisoned".to_string())
    }
}

/// One row of the Settings hotkey list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HotkeyBinding {
    pub action: HotkeyAction,
    /// None = unbound.
    pub accelerator: Option<String>,
    /// Sink the channel-mute row acts on; None on every other row and when
    /// the target follows "the first channel".
    pub channel: Option<String>,
    /// True only when a binding exists *and* the OS accepted the grab.
    pub registered: bool,
    /// Why the grab failed, when it did.
    pub error: Option<String>,
}

/// The Settings list, derived from what is stored plus what registration
/// actually did. Pure so the "bound but dead" case is testable.
fn snapshot(hotkeys: &Hotkeys, errors: &HashMap<HotkeyAction, String>) -> Vec<HotkeyBinding> {
    HotkeyAction::ALL
        .into_iter()
        .map(|action| {
            let accelerator = hotkeys.get(action).map(str::to_string);
            let error = errors.get(&action).cloned();
            HotkeyBinding {
                registered: accelerator.is_some() && error.is_none(),
                channel: match action {
                    HotkeyAction::ChannelMute => hotkeys.channel_target.clone(),
                    _ => None,
                },
                accelerator,
                error,
                action,
            }
        })
        .collect()
}

/// (Re)register every bound hotkey, recording what the OS said.
///
/// Must not run on the main thread before the event loop is up: the plugin
/// hops to the main thread to talk to the hotkey manager and would deadlock
/// waiting for a loop that has not started. `setup` therefore calls this from
/// a worker thread, and commands are already off the main thread.
pub fn apply(app: &tauri::AppHandle) {
    let hotkeys = match app.state::<AppState>().lock_mixer() {
        Ok(mixer) => mixer.prefs.hotkeys.clone(),
        Err(e) => {
            warn!("hotkeys not applied: {e}");
            return;
        }
    };
    let shortcuts = app.global_shortcut();
    // Wholesale rebuild: cheaper to reason about than diffing, and there are
    // at most four of these.
    if let Err(e) = shortcuts.unregister_all() {
        warn!("clearing previous hotkeys failed: {e}");
    }
    let mut errors = HashMap::new();
    for (action, accelerator) in hotkeys.bound() {
        let bound = action;
        let result = shortcuts.on_shortcut(accelerator, move |app, _shortcut, event| {
            // Both edges arrive; acting on the release too would fire twice.
            if event.state() == ShortcutState::Pressed {
                trigger(app, bound);
            }
        });
        match result {
            Ok(()) => info!("hotkey {accelerator} bound to {action:?}"),
            Err(e) => {
                warn!("hotkey {accelerator} ({action:?}) could not be registered: {e}");
                errors.insert(action, e.to_string());
            }
        }
    }
    if let Ok(mut slot) = app.state::<HotkeyErrors>().lock() {
        *slot = errors;
    }
}

/// Run an action, then tell the UI and the tray so both stop lying.
fn trigger(app: &tauri::AppHandle, action: HotkeyAction) {
    if let Err(e) = run(app, action) {
        warn!("hotkey {action:?} failed: {e}");
        return;
    }
    crate::after_external_change(app);
}

fn run(app: &tauri::AppHandle, action: HotkeyAction) -> Result<(), String> {
    let state = app.state::<AppState>();
    match action {
        HotkeyAction::MicMute => {
            let muted = !state.lock_mixer()?.mic.muted;
            super::mic::set_mic_muted(&state, muted)
        }
        HotkeyAction::ChannelMute => {
            let target = {
                let mixer = state.lock_mixer()?;
                // Falling back to the first channel keeps the binding useful
                // after its target was renamed away or deleted.
                let target = mixer
                    .prefs
                    .hotkeys
                    .channel_target
                    .clone()
                    .filter(|t| mixer.channels.iter().any(|c| &c.name == t))
                    .or_else(|| mixer.channels.first().map(|c| c.name.clone()));
                target.map(|name| {
                    let muted = mixer
                        .channels
                        .iter()
                        .find(|c| c.name == name)
                        .is_some_and(|c| c.muted);
                    (name, muted)
                })
            };
            let Some((name, muted)) = target else {
                return Err("no channel to mute".into());
            };
            super::routing::toggle_channel_mute(app.state(), name, !muted)
        }
        HotkeyAction::CycleProfile => {
            let profiles = crate::persistence::profiles::list().map_err(|e| e.to_string())?;
            if profiles.is_empty() {
                return Err("no saved profiles".into());
            }
            let active = state.lock_mixer()?.active_profile.clone();
            let at = active
                .and_then(|a| profiles.iter().position(|p| p.name == a))
                .map_or(0, |i| (i + 1) % profiles.len());
            let name = profiles[at].name.clone();
            super::profiles::load_profile(app.clone(), app.state(), name.clone())?;
            let _ = app.emit("profile-changed", name);
            Ok(())
        }
        HotkeyAction::ToggleWindow => {
            let Some(window) = app.get_webview_window("main") else {
                return Err("no window".into());
            };
            if window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false) {
                window.hide().map_err(|e| e.to_string())
            } else {
                let _ = window.unminimize();
                window.show().map_err(|e| e.to_string())?;
                window.set_focus().map_err(|e| e.to_string())
            }
        }
    }
}

#[tauri::command]
pub fn get_hotkeys(
    state: State<'_, AppState>,
    errors: State<'_, HotkeyErrors>,
) -> Result<Vec<HotkeyBinding>, String> {
    let hotkeys = state.lock_mixer()?.prefs.hotkeys.clone();
    let errors = errors.lock()?;
    Ok(snapshot(&hotkeys, &errors))
}

/// Bind an action to `accelerator`, or clear it with None/"". Returns the
/// refreshed list so the caller sees straight away whether the grab took.
#[tauri::command]
pub fn set_hotkey(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    errors: State<'_, HotkeyErrors>,
    action: HotkeyAction,
    accelerator: Option<String>,
) -> Result<Vec<HotkeyBinding>, String> {
    let accelerator = accelerator.filter(|a| !a.trim().is_empty());
    // Reject unparsable accelerators here rather than persisting a binding
    // that can never register.
    if let Some(accel) = &accelerator {
        accel
            .parse::<Shortcut>()
            .map_err(|e| format!("{accel} is not a valid shortcut: {e}"))?;
    }
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.hotkeys.set(action, accelerator);
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())?;
    apply(&app);
    let errors = errors.lock()?;
    Ok(snapshot(&prefs.hotkeys, &errors))
}

/// Pick the channel the channel-mute hotkey acts on (None = first channel).
#[tauri::command]
pub fn set_hotkey_channel(
    state: State<'_, AppState>,
    errors: State<'_, HotkeyErrors>,
    sink_name: Option<String>,
) -> Result<Vec<HotkeyBinding>, String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.hotkeys.channel_target = sink_name.filter(|s| !s.is_empty());
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())?;
    let errors = errors.lock()?;
    Ok(snapshot(&prefs.hotkeys, &errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_bound_means_nothing_registered_and_no_error() {
        let rows = snapshot(&Hotkeys::default(), &HashMap::new());
        assert_eq!(rows.len(), HotkeyAction::ALL.len());
        assert!(rows.iter().all(|r| r.accelerator.is_none()));
        assert!(rows.iter().all(|r| !r.registered && r.error.is_none()));
    }

    #[test]
    fn a_failed_grab_is_bound_but_not_registered() {
        // The Wayland case: the accelerator stays saved (the user may move to
        // an X11 session), but the UI must not present it as working.
        let mut hotkeys = Hotkeys::default();
        hotkeys.set(HotkeyAction::MicMute, Some("Ctrl+Shift+M".into()));
        hotkeys.set(HotkeyAction::ToggleWindow, Some("Super+I".into()));
        let errors = HashMap::from([(HotkeyAction::MicMute, "HotKey already registered".into())]);

        let rows = snapshot(&hotkeys, &errors);
        let mic = rows.iter().find(|r| r.action == HotkeyAction::MicMute).expect("row");
        assert_eq!(mic.accelerator.as_deref(), Some("Ctrl+Shift+M"));
        assert!(!mic.registered);
        assert_eq!(mic.error.as_deref(), Some("HotKey already registered"));

        let window = rows
            .iter()
            .find(|r| r.action == HotkeyAction::ToggleWindow)
            .expect("row");
        assert!(window.registered && window.error.is_none());
    }

    #[test]
    fn only_the_channel_row_carries_a_target() {
        let hotkeys = Hotkeys {
            channel_target: Some("sink_chat".into()),
            ..Hotkeys::default()
        };
        let rows = snapshot(&hotkeys, &HashMap::new());
        for row in rows {
            match row.action {
                HotkeyAction::ChannelMute => assert_eq!(row.channel.as_deref(), Some("sink_chat")),
                _ => assert_eq!(row.channel, None),
            }
        }
    }
}
