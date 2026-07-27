use log::warn;
use serde::Serialize;
use tauri::{Manager, State};

use crate::persistence::autostart;
use crate::persistence::prefs::{DeviceLabelStyle, Prefs};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct BackendInfo {
    /// True = native PipeWire backend; false = pactl subprocess fallback.
    pub native: bool,
    /// False when the backend's engine has stopped (the native PipeWire loop
    /// thread left). Every audio request fails from here on and nothing but a
    /// restart brings it back, so this is a distinct state from `native:
    /// false` - the fallback works, a stopped engine does not.
    pub engine_alive: bool,
}

#[tauri::command]
pub fn get_backend_info(state: State<'_, AppState>) -> BackendInfo {
    BackendInfo {
        native: state.backend_native,
        engine_alive: state.backend.is_engine_alive(),
    }
}

#[tauri::command]
pub fn get_autostart() -> bool {
    autostart::is_enabled()
}

/// Enable/disable the systemd user unit for autostart on login.
#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<bool, String> {
    let result = if enabled {
        autostart::enable()
    } else {
        autostart::disable()
    };
    result.map_err(|e| e.to_string())?;
    Ok(autostart::is_enabled())
}

#[tauri::command]
pub fn get_prefs(state: State<'_, AppState>) -> Result<Prefs, String> {
    Ok(state.lock_mixer()?.prefs.clone())
}

/// Set the device naming style. Existing nodes keep their labels until
/// they are recreated (restart or rename).
#[tauri::command]
pub fn set_device_label_style(
    state: State<'_, AppState>,
    style: DeviceLabelStyle,
) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.device_label_style = style;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Toggle "start minimized" (boot to tray when autostarting). Rewrites
/// the systemd unit when autostart is already enabled so the flag tracks
/// the preference.
#[tauri::command]
pub fn set_start_minimized(state: State<'_, AppState>, minimized: bool) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.start_minimized = minimized;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())?;
    if autostart::is_enabled() {
        autostart::enable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Show or hide the title-bar balance slider.
#[tauri::command]
pub fn set_balance_visible(state: State<'_, AppState>, visible: bool) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.show_balance = visible;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Pick the two channels the balance slider blends.
#[tauri::command]
pub fn set_balance_channels(
    state: State<'_, AppState>,
    a: Option<String>,
    b: Option<String>,
) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.balance_a = a;
        mixer.prefs.balance_b = b;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Mark the first-run tutorial as completed (never shown again, until a
/// factory reset).
#[tauri::command]
pub fn set_onboarded(state: State<'_, AppState>) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.onboarded = true;
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Factory reset: tear down our audio nodes, wipe every saved file, undo
/// autostart, and relaunch as if freshly installed.
#[tauri::command]
pub fn reset_app(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // Best-effort teardown - the relaunch recreates everything anyway.
    for err in state.teardown_virtual_sinks() {
        warn!("reset teardown: {err}");
    }
    let _ = autostart::disable();
    crate::persistence::wipe_all().map_err(|e| e.to_string())?;
    app.restart()
}

#[derive(Debug, Clone, Serialize)]
pub struct DefaultDevices {
    pub output: Option<String>,
    pub input: Option<String>,
}

/// Current system default output/input device node names.
#[tauri::command]
pub fn get_default_devices(state: State<'_, AppState>) -> Result<DefaultDevices, String> {
    let (output, input) = state
        .backend
        .get_default_devices()
        .map_err(|e| e.to_string())?;
    Ok(DefaultDevices { output, input })
}

/// Set the system default output device.
#[tauri::command]
pub fn set_default_output(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .backend
        .set_default_output(&name)
        .map_err(|e| e.to_string())
}

/// Set the system default input device.
#[tauri::command]
pub fn set_default_input(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .backend
        .set_default_input(&name)
        .map_err(|e| e.to_string())
}

/// Open the log directory in the user's file manager. Started from the app
/// menu or the systemd autostart unit there is no terminal to read, so this
/// is the only way a reporter gets at the log file.
#[tauri::command]
pub fn open_log_dir(app: tauri::AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    // The log plugin creates the directory lazily on its first write; xdg-open
    // on a path that isn't there yet just fails.
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("xdg-open")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("xdg-open: {e}"))
}
