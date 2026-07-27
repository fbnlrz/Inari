//! The settings UI's view of Inari Remote: turn it on, choose where it
//! listens, see who is connected, pair a device, and cut every paired device
//! loose again.
//!
//! These are desktop-only commands. They are not on the remote's own allowlist
//! and must never be: a tablet that could rebind the server or read the token
//! back out would be able to hand itself to the next person on the Wi-Fi.

use tauri::State;

use crate::persistence::prefs::RemoteConfig;
use crate::remote::{RemotePairing, RemoteState, RemoteStatus};
use crate::state::AppState;

/// The stored remote settings.
fn config(state: &State<'_, AppState>) -> Result<RemoteConfig, String> {
    Ok(state.lock_mixer()?.prefs.remote.clone())
}

/// Write `config` back to `prefs.json`.
///
/// The whole prefs file is saved, not just this section: the mixer holds the
/// authoritative copy and everything else in it has to survive the write.
fn store(state: &State<'_, AppState>, config: &RemoteConfig) -> Result<(), String> {
    let prefs = {
        let mut mixer = state.lock_mixer()?;
        mixer.prefs.remote = config.clone();
        mixer.prefs.clone()
    };
    prefs.save().map_err(|e| e.to_string())
}

/// Apply `config`: start, restart or stop the listener to match it, then save.
///
/// The order matters. A bind that fails (port taken, interface gone) must not
/// leave "enabled" written to disk, or the next launch would try the same
/// broken setup and the switch would read as on over a dead listener.
fn apply(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    remote: &RemoteState,
    mut config: RemoteConfig,
) -> Result<RemoteStatus, String> {
    if config.enabled {
        if let Err(e) = remote.start(app, &config) {
            config.enabled = false;
            store(state, &config)?;
            return Err(e);
        }
    } else {
        remote.stop();
    }
    store(state, &config)?;
    Ok(remote.status(&config))
}

/// Whether the remote is listening, where, and how many devices are attached.
#[tauri::command]
pub fn get_remote_status(
    state: State<'_, AppState>,
    remote: State<'_, RemoteState>,
) -> Result<RemoteStatus, String> {
    Ok(remote.status(&config(&state)?))
}

/// Turn the remote on or off, leaving the address it listens on alone.
#[tauri::command]
pub fn set_remote_enabled(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    remote: State<'_, RemoteState>,
    enabled: bool,
) -> Result<RemoteStatus, String> {
    let config = RemoteConfig {
        enabled,
        ..config(&state)?
    };
    apply(&app, &state, &remote, config)
}

/// Move the listener. Takes effect immediately when it is running, and is
/// remembered either way.
#[tauri::command]
pub fn set_remote_bind(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    remote: State<'_, RemoteState>,
    address: String,
    port: u16,
) -> Result<RemoteStatus, String> {
    // Rejected here rather than at bind time so a typo cannot be written to
    // prefs and then fail silently on every launch.
    address
        .parse::<std::net::IpAddr>()
        .map_err(|_| format!("`{address}` is not an address this machine can listen on"))?;
    let config = RemoteConfig {
        address,
        port,
        ..config(&state)?
    };
    apply(&app, &state, &remote, config)
}

/// The URL, token and QR code a new device needs.
#[tauri::command]
pub fn get_remote_pairing(remote: State<'_, RemoteState>) -> Result<RemotePairing, String> {
    remote.pairing()
}

/// Mint a fresh token. Every device paired with the old one has to be paired
/// again - which is the point when a tablet is lost or lent out.
#[tauri::command]
pub fn regenerate_remote_token(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    remote: State<'_, RemoteState>,
) -> Result<RemotePairing, String> {
    let config = config(&state)?;
    crate::remote::regenerate_token(&app, &remote, &config)?;
    remote.pairing()
}
