use log::error;
use tauri::State;

use crate::audio::backend::AudioBackend;
use crate::commands::routing::MAX_VOLUME;
use crate::persistence::buses::{is_bus_name, BusDef};
use crate::state::AppState;

/// Re-apply a mix's persisted volume/mute to its node. Bus nodes are born at
/// unity/unmuted, so this restores the saved level after create/rename/load.
/// Best-effort: a routing failure shouldn't abort bringing the mix up.
pub(crate) fn apply_bus_level(backend: &dyn AudioBackend, def: &BusDef) {
    if def.volume_percent != 100 {
        let _ = backend.set_sink_volume(&def.name, def.volume_percent);
    }
    if def.muted {
        let _ = backend.set_sink_mute(&def.name, true);
    }
}

/// The user's mixes (buses) with their member channels.
#[tauri::command]
pub fn list_buses(state: State<'_, AppState>) -> Result<Vec<BusDef>, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.buses.buses.clone())
}

/// Create a new mix. Recorders see it under `label`. New mixes carry
/// every channel (auto-include) until the user unchecks some.
#[tauri::command]
pub fn add_bus(state: State<'_, AppState>, label: String) -> Result<(), String> {
    let (def, defs, prefs, all) = {
        let mut mixer = state.lock_mixer()?;
        let def = mixer.buses.add(&label).map_err(|e| e.to_string())?;
        (
            def,
            mixer.buses.clone(),
            mixer.prefs.clone(),
            channel_names(&mixer),
        )
    };
    if let Err(e) = state.backend.create_bus(&def.name, &prefs.decorate(&def.label)) {
        let mut mixer = state.lock_mixer()?;
        let _ = mixer.buses.remove(&def.name);
        return Err(e.to_string());
    }
    if let Err(e) = state
        .backend
        .set_bus_members(&def.name, &def.effective_members(&all))
    {
        error!("members for new mix {} failed: {e}", def.name);
    }
    defs.save().map_err(|e| e.to_string())?;
    let snapshot = {
        let mixer = state.lock_mixer()?;
        crate::commands::profiles::build_autosave(&mixer)
    };
    crate::commands::profiles::write_autosave(snapshot);
    Ok(())
}

/// Rename a mix. The node is recreated so recorders immediately see the
/// new name (the node name stays stable, so OBS configs keep working -
/// capture re-attaches automatically).
#[tauri::command]
pub fn rename_bus(state: State<'_, AppState>, name: String, label: String) -> Result<(), String> {
    let (def, defs, prefs, all) = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses.rename(&name, &label).map_err(|e| e.to_string())?;
        let def = mixer
            .buses
            .get(&name)
            .cloned()
            .ok_or_else(|| "unknown mix".to_string())?;
        (
            def,
            mixer.buses.clone(),
            mixer.prefs.clone(),
            channel_names(&mixer),
        )
    };

    state.backend.destroy_bus(&name).map_err(|e| e.to_string())?;
    state
        .backend
        .create_bus(&def.name, &prefs.decorate(&def.label))
        .map_err(|e| e.to_string())?;
    state
        .backend
        .set_bus_members(&def.name, &def.effective_members(&all))
        .map_err(|e| e.to_string())?;
    // The node was recreated fresh; restore this mix's saved level.
    apply_bus_level(state.backend.as_ref(), &def);

    defs.save().map_err(|e| e.to_string())?;
    let snapshot = {
        let mixer = state.lock_mixer()?;
        crate::commands::profiles::build_autosave(&mixer)
    };
    crate::commands::profiles::write_autosave(snapshot);
    Ok(())
}

/// Delete a mix.
#[tauri::command]
pub fn remove_bus(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state.backend.destroy_bus(&name).map_err(|e| e.to_string())?;
    let (defs, snapshot) = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses.remove(&name).map_err(|e| e.to_string())?;
        (
            mixer.buses.clone(),
            crate::commands::profiles::build_autosave(&mixer),
        )
    };
    crate::commands::profiles::write_autosave(snapshot);
    defs.save().map_err(|e| e.to_string())
}

/// What gets persisted for a mix given what the user checked. Auto-include
/// ("exclude") mixes store the complement, so channels created later join by
/// themselves; manual mixes store the selection verbatim.
fn stored_members(all: &[String], checked: &[String], exclude: bool) -> Vec<String> {
    if exclude {
        all.iter().filter(|c| !checked.contains(c)).cloned().collect()
    } else {
        checked.to_vec()
    }
}

/// Replace the channel set a mix carries. `channels` is what the user
/// sees checked; for auto-include mixes the complement (the unchecked
/// set) is what gets stored, so future channels keep flowing in.
#[tauri::command]
pub fn set_bus_members(
    state: State<'_, AppState>,
    name: String,
    channels: Vec<String>,
) -> Result<(), String> {
    // Validate against the definition set first, so a rejected request
    // (master mix, unknown name) never reaches the backend - otherwise
    // backend membership and the persisted definition could diverge.
    let stored = {
        let mixer = state.lock_mixer()?;
        if crate::persistence::buses::is_master(&name) {
            return Err("the master mix always carries every channel".to_string());
        }
        let Some(def) = mixer.buses.get(&name) else {
            return Err("unknown mix".to_string());
        };
        let exclude = def.exclude;
        stored_members(&channel_names(&mixer), &channels, exclude)
    };
    state
        .backend
        .set_bus_members(&name, &channels)
        .map_err(|e| e.to_string())?;
    let (defs, snapshot) = {
        let mut mixer = state.lock_mixer()?;
        mixer
            .buses
            .set_members(&name, stored)
            .map_err(|e| e.to_string())?;
        (
            mixer.buses.clone(),
            crate::commands::profiles::build_autosave(&mixer),
        )
    };
    crate::commands::profiles::write_autosave(snapshot);
    defs.save().map_err(|e| e.to_string())
}

/// Switch a mix between manual selection and auto-include mode. The
/// carried set is preserved; only what happens to future channels changes.
#[tauri::command]
pub fn set_bus_exclude(
    state: State<'_, AppState>,
    name: String,
    exclude: bool,
) -> Result<(), String> {
    let (defs, snapshot) = {
        let mut mixer = state.lock_mixer()?;
        let all = channel_names(&mixer);
        mixer
            .buses
            .set_exclude(&name, exclude, &all)
            .map_err(|e| e.to_string())?;
        (
            mixer.buses.clone(),
            crate::commands::profiles::build_autosave(&mixer),
        )
    };
    crate::commands::profiles::write_autosave(snapshot);
    defs.save().map_err(|e| e.to_string())
}

/// Set a mix's playback level (0-150%) - what recorders hear. Unlike
/// `set_channel_volume`, this accepts mix nodes (including the master mix,
/// whose reserved name `set_channel_volume` rejects) and persists the level.
#[tauri::command]
pub fn set_bus_volume(state: State<'_, AppState>, name: String, volume: u8) -> Result<(), String> {
    if !is_bus_name(&name) {
        return Err(format!("unknown mix: {name}"));
    }
    let volume = volume.min(MAX_VOLUME);
    state
        .backend
        .set_sink_volume(&name, volume)
        .map_err(|e| e.to_string())?;
    let (defs, snapshot) = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses.set_volume(&name, volume).map_err(|e| e.to_string())?;
        (
            mixer.buses.clone(),
            crate::commands::profiles::build_autosave(&mixer),
        )
    };
    crate::commands::profiles::write_autosave(snapshot);
    defs.save().map_err(|e| e.to_string())
}

/// Mute or unmute a mix for recorders. Persisted, and accepts the master mix.
#[tauri::command]
pub fn set_bus_mute(state: State<'_, AppState>, name: String, muted: bool) -> Result<(), String> {
    if !is_bus_name(&name) {
        return Err(format!("unknown mix: {name}"));
    }
    state
        .backend
        .set_sink_mute(&name, muted)
        .map_err(|e| e.to_string())?;
    let (defs, snapshot) = {
        let mut mixer = state.lock_mixer()?;
        mixer.buses.set_muted(&name, muted).map_err(|e| e.to_string())?;
        (
            mixer.buses.clone(),
            crate::commands::profiles::build_autosave(&mixer),
        )
    };
    crate::commands::profiles::write_autosave(snapshot);
    defs.save().map_err(|e| e.to_string())
}

/// The current channel sink names (the "all channels" set for mixes).
pub(crate) fn channel_names(mixer: &crate::mixer::state::MixerState) -> Vec<String> {
    mixer.channels.iter().map(|c| c.name.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn manual_mixes_store_exactly_what_was_checked() {
        let all = names(&["sink_game", "sink_music", "sink_chat"]);
        assert_eq!(
            stored_members(&all, &names(&["sink_game"]), false),
            names(&["sink_game"])
        );
    }

    #[test]
    fn auto_include_mixes_store_the_complement_so_new_channels_join() {
        let all = names(&["sink_game", "sink_music", "sink_chat"]);
        let stored = stored_members(&all, &names(&["sink_game", "sink_chat"]), true);
        // What is persisted is what the user *unchecked*, so a channel added
        // later isn't in the set and `effective_members` carries it anyway.
        assert_eq!(stored, names(&["sink_music"]));
        assert!(!stored.contains(&"sink_browser".to_string()));
    }

    #[test]
    fn unchecking_every_channel_on_an_auto_include_mix_excludes_them_all() {
        let all = names(&["sink_game", "sink_music"]);
        assert_eq!(stored_members(&all, &[], true), all);
    }
}
