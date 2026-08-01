use tauri::State;

use crate::audio::types::is_virtual_sink;
use crate::persistence::wireplumber;
use crate::state::AppState;
use crate::mixer::state::StreamKey;

pub(crate) const MAX_VOLUME: u8 = 150;

/// Move an app stream onto a channel. An empty `sink_name` unassigns the
/// stream (returns it to the system default sink).
///
/// The choice is also recorded as a persistent assignment (Phase 2): saved
/// to `$XDG_CONFIG_HOME/inari/assignments.json`, mirrored to a WirePlumber
/// conf fragment, and re-applied by the stream poll when the app restarts.
#[tauri::command]
pub fn route_app_to_channel(
    state: State<'_, AppState>,
    stream_index: u32,
    serial: Option<u64>,
    sink_name: String,
) -> Result<(), String> {
    if !sink_name.is_empty() && !is_virtual_sink(&sink_name) {
        return Err(format!("unknown channel: {sink_name}"));
    }
    let stream_index = resolve_stream(&state, stream_index, serial)?;
    state
        .backend
        .move_stream_to_sink(stream_index, &sink_name)
        .map_err(|e| e.to_string())?;

    // Resolve the stream's identity to record the assignment. The stream is
    // already moved at this point, so persistence failures are reported but
    // the live routing stands.
    let streams = state.backend.list_app_streams().map_err(|e| e.to_string())?;
    let Some(stream) = streams.iter().find(|s| s.index == stream_index) else {
        return Ok(()); // stream vanished between move and lookup
    };

    let (assignments, snapshot) = {
        let mut mixer = state.lock_mixer()?;
        if sink_name.is_empty() {
            mixer
                .assignments
                .remove(&stream.match_prop, &stream.match_value);
        } else {
            mixer
                .assignments
                .set(&stream.match_prop, &stream.match_value, &sink_name);
        }
        // The user explicitly placed this stream; don't auto-route it again.
        mixer.auto_routed.insert(StreamKey::of(stream));
        (
            mixer.assignments.clone(),
            crate::commands::profiles::build_autosave(&mixer),
        )
    };
    crate::commands::profiles::write_autosave(snapshot);

    assignments.save().map_err(|e| e.to_string())?;
    wireplumber::write(&assignments).map_err(|e| e.to_string())?;
    Ok(())
}

/// Set a channel's volume (0-150%).
#[tauri::command]
pub fn set_channel_volume(
    state: State<'_, AppState>,
    sink_name: String,
    volume: u8,
) -> Result<(), String> {
    // Only our own channels, so a compromised webview can't touch arbitrary
    // session sinks (TD-050).
    if !is_virtual_sink(&sink_name) {
        return Err(format!("unknown channel: {sink_name}"));
    }
    let volume = volume.min(MAX_VOLUME);
    state
        .backend
        .set_sink_volume(&sink_name, volume)
        .map_err(|e| e.to_string())?;

    let snapshot = {
        let mut mixer = state.lock_mixer()?;
        if let Some(channel) = mixer.channel_mut(&sink_name) {
            channel.volume_percent = volume;
        }
        crate::commands::profiles::build_autosave(&mixer)
    };
    crate::commands::profiles::write_autosave(snapshot);
    Ok(())
}

/// Mute or unmute a channel.
#[tauri::command]
pub fn toggle_channel_mute(
    state: State<'_, AppState>,
    sink_name: String,
    muted: bool,
) -> Result<(), String> {
    if !is_virtual_sink(&sink_name) {
        return Err(format!("unknown channel: {sink_name}"));
    }
    state
        .backend
        .set_sink_mute(&sink_name, muted)
        .map_err(|e| e.to_string())?;

    let snapshot = {
        let mut mixer = state.lock_mixer()?;
        if let Some(channel) = mixer.channel_mut(&sink_name) {
            channel.muted = muted;
        }
        crate::commands::profiles::build_autosave(&mixer)
    };
    crate::commands::profiles::write_autosave(snapshot);
    Ok(())
}

/// Monitoring is scoped to our own nodes: a channel, a mix bus, or the mic
/// (TD-050) - not any arbitrary session sink.
fn monitor_target_known(mixer: &crate::mixer::state::MixerState, sink_name: &str) -> bool {
    sink_name == "sink_mic"
        || mixer.channel_defs.channels.iter().any(|c| c.name == sink_name)
        || mixer.buses.buses.iter().any(|b| b.name == sink_name)
}

/// Listen to a channel/mix/mic on the default output (session scoped -
/// not persisted, cleared on restart).
#[tauri::command]
pub fn set_monitor(
    state: State<'_, AppState>,
    sink_name: String,
    enabled: bool,
) -> Result<(), String> {
    {
        let mixer = state.lock_mixer()?;
        if !monitor_target_known(&mixer, &sink_name) {
            return Err(format!("unknown monitor target: {sink_name}"));
        }
    }
    state
        .backend
        .set_monitor(&sink_name, enabled)
        .map_err(|e| e.to_string())
}

/// Set or clear a persistent display name for an app, keyed by its stream
/// identity. An empty `alias` reverts to the discovered name.
#[tauri::command]
pub fn rename_app(
    state: State<'_, AppState>,
    match_prop: String,
    match_value: String,
    alias: String,
) -> Result<(), String> {
    let aliases = {
        let mut mixer = state.lock_mixer()?;
        mixer.aliases.set(&match_prop, &match_value, &alias);
        mixer.aliases.clone()
    };
    aliases.save().map_err(|e| e.to_string())
}

/// Turn the stream the UI was looking at into the id it has *now*.
///
/// PipeWire recycles global ids hard, and both of these commands can arrive
/// well after the UI read the list — `set_app_volume` is debounced by 90 ms,
/// and browsers create and destroy a stream per media element. Holding the id
/// across that gap means a write can land on whatever inherited the number.
/// The serial never repeats, so it is what the UI passes and what gets
/// resolved here, immediately before acting. A stream that has genuinely gone
/// is an error rather than a write to a stranger.
fn resolve_stream(
    state: &AppState,
    stream_index: u32,
    serial: Option<u64>,
) -> Result<u32, String> {
    let Some(serial) = serial else {
        // The pactl backend has no serials; nothing better is available.
        return Ok(stream_index);
    };
    let streams = state.backend.list_app_streams().map_err(|e| e.to_string())?;
    streams
        .iter()
        .find(|s| s.serial == Some(serial))
        .map(|s| s.index)
        .ok_or_else(|| "that stream has ended".to_string())
}

/// Set the volume of a single app stream (0-150%).
#[tauri::command]
pub fn set_app_volume(
    state: State<'_, AppState>,
    stream_index: u32,
    serial: Option<u64>,
    volume: u8,
) -> Result<(), String> {
    let index = resolve_stream(&state, stream_index, serial)?;
    state
        .backend
        .set_app_volume(index, volume.min(MAX_VOLUME))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::state::MixerState;

    #[test]
    fn volume_and_mute_only_reach_our_own_channels() {
        // The gate `set_channel_volume` / `toggle_channel_mute` apply, so a
        // compromised webview can't drive arbitrary session sinks (TD-050).
        assert!(is_virtual_sink("sink_game"));
        assert!(!is_virtual_sink("sink_mic"), "the mic chain, not a channel");
        assert!(!is_virtual_sink("sink_stream"), "a mix bus, not a channel");
        assert!(!is_virtual_sink("alsa_output.usb-SteelSeries-00.analog-stereo"));
        assert!(!is_virtual_sink("ink_game"), "the prefix is required");
        assert!(!is_virtual_sink(""));
    }

    #[test]
    fn monitoring_accepts_channels_mixes_and_the_mic_only() {
        let mixer = MixerState::default();
        assert!(monitor_target_known(&mixer, "sink_game"), "a channel");
        assert!(monitor_target_known(&mixer, "sink_mic"));
        assert!(monitor_target_known(&mixer, "sink_stream"), "the master mix");
        assert!(!monitor_target_known(&mixer, "alsa_output.pci-0000_00_1f.3"));
        assert!(!monitor_target_known(&mixer, "sink_game_evil"));
        assert!(!monitor_target_known(&mixer, ""));
    }
}
