//! Soundboard commands: the library, and firing clips into the chat.
//!
//! **At most one clip plays at a time**, and `soundboard_toggle` is what a
//! button is wired to: pressing the running clip stops it, pressing another
//! takes over. The decision belongs here rather than in the UI - see that
//! command's doc.
//!
//! **A clip is played by id.** `soundboard_add_clip` is the only command here
//! that takes a filesystem path, it is a desktop action behind a file dialog,
//! and it is the only one kept off the remote allowlist for that reason. Every
//! other command works on ids the client learned from `soundboard_clips`, so a
//! hotkey or a tablet can fire a clip without a file name ever travelling in -
//! the same rule cover art follows in `commands/media.rs`.
//!
//! Paths never travel *out* either: `ClipInfo` carries a name and an id, not a
//! location on this machine.

use tauri::State;

use crate::audio::types::ClipTargets;
use crate::persistence::soundboard::Duck;
use crate::soundboard::{decode, ClipInfo, SoundboardStatus};
use crate::state::AppState;

/// Where a clip should be heard. A string rather than two booleans because
/// this is what a UI has: three buttons, one of which is the default.
#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayTargets {
    /// The chat and the user - what a soundboard normally means.
    #[default]
    Both,
    /// Only the chat: the user is not interested in hearing it again.
    Chat,
    /// Only the user: auditioning a clip without firing it into the call.
    Me,
}

impl From<PlayTargets> for ClipTargets {
    fn from(t: PlayTargets) -> Self {
        match t {
            PlayTargets::Both => ClipTargets { to_mic: true, to_output: true },
            PlayTargets::Chat => ClipTargets { to_mic: true, to_output: false },
            PlayTargets::Me => ClipTargets { to_mic: false, to_output: true },
        }
    }
}

/// Every clip in the library, with what this machine can currently do with it.
#[tauri::command]
pub fn soundboard_clips(state: State<'_, AppState>) -> Result<Vec<ClipInfo>, String> {
    state.soundboard.clips()
}

/// Add a clip from a path (desktop only - see the module doc). Returns the new
/// entry so the caller does not have to re-read the whole library.
#[tauri::command]
pub fn soundboard_add_clip(state: State<'_, AppState>, path: String) -> Result<ClipInfo, String> {
    state.soundboard.add(&path)
}

/// The file extensions a clip may have, for the file dialog. Static, but read
/// from the decoder so the picker and the decoder cannot drift apart.
#[tauri::command]
pub fn soundboard_formats() -> Vec<&'static str> {
    decode::supported_extensions()
}

#[tauri::command]
pub fn soundboard_remove_clip(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.soundboard.remove(&id)
}

#[tauri::command]
pub fn soundboard_rename_clip(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), String> {
    state.soundboard.rename(&id, &name)
}

/// Per-clip trim in percent (100 = as recorded, 200 = the ceiling). Recorded
/// snippets arrive at wildly different levels, so this is per clip rather than
/// one level for the board.
#[tauri::command]
pub fn soundboard_set_clip_volume(
    state: State<'_, AppState>,
    id: String,
    volume_percent: u8,
) -> Result<(), String> {
    state.soundboard.set_volume(&id, volume_percent)
}

/// What a soundboard button does: stop this clip if it is the one playing,
/// otherwise start it - taking over from whatever was running, because at most
/// one clip plays at a time. Returns `true` when the clip is playing now.
///
/// This is the command the buttons should use. The equivalent from the UI's
/// side would be "read the status, then send stop or play", and over the
/// tablet's WebSocket those two requests race: the stop can land after the
/// next start and kill it. The press is the atomic thing, so the press is the
/// command.
///
/// `targets` is `"both"` (default), `"chat"` or `"me"`.
#[tauri::command]
pub fn soundboard_toggle(
    state: State<'_, AppState>,
    id: String,
    targets: Option<PlayTargets>,
) -> Result<bool, String> {
    state
        .soundboard
        .toggle(&state.backend, &id, targets.unwrap_or_default().into())
}

/// Start a clip unconditionally, taking over from whatever was playing.
/// `targets` is `"both"` (default), `"chat"` or `"me"`.
///
/// Pressing the same clip twice through this command restarts it from the
/// top; a button that should stop on the second press wants
/// [`soundboard_toggle`].
#[tauri::command]
pub fn soundboard_play(
    state: State<'_, AppState>,
    id: String,
    targets: Option<PlayTargets>,
) -> Result<(), String> {
    state
        .soundboard
        .play(&state.backend, &id, targets.unwrap_or_default().into())
}

/// Stop every clip at once, and release ducking.
#[tauri::command]
pub fn soundboard_stop_all(state: State<'_, AppState>) -> Result<(), String> {
    state.soundboard.stop_all(state.backend.as_ref())
}

/// The mic-ducking setting on its own (the status carries it too).
#[tauri::command]
pub fn soundboard_duck(state: State<'_, AppState>) -> Result<Duck, String> {
    state.soundboard.duck()
}

/// Turn ducking on or off and set how far the microphone drops while a clip
/// plays. Off by default; the attenuation is clamped to -40..0 dB.
#[tauri::command]
pub fn soundboard_set_duck(
    state: State<'_, AppState>,
    enabled: bool,
    attenuation_db: f32,
) -> Result<Duck, String> {
    state.soundboard.set_duck(enabled, attenuation_db)
}

/// What is playing (by clip id, so a button can light up), can compressed
/// formats be played at all, what is ducking set to, and is the soundboard
/// available on this backend.
#[tauri::command]
pub fn soundboard_status(state: State<'_, AppState>) -> Result<SoundboardStatus, String> {
    state.soundboard.status(state.backend.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_choices_map_to_the_destinations_they_name() {
        let both: ClipTargets = PlayTargets::Both.into();
        assert!(both.to_mic && both.to_output);

        let chat: ClipTargets = PlayTargets::Chat.into();
        assert!(chat.to_mic, "the chat has to hear it");
        assert!(!chat.to_output, "and the user asked not to");

        let me: ClipTargets = PlayTargets::Me.into();
        assert!(!me.to_mic, "an audition must never reach the call");
        assert!(me.to_output);
    }

    #[test]
    fn omitting_the_target_plays_it_everywhere() {
        // What `soundboard_play` does with a missing argument.
        let default: ClipTargets = PlayTargets::default().into();
        assert_eq!(default, ClipTargets::default());
    }

    #[test]
    fn the_targets_arrive_as_the_lowercase_words_a_ui_sends() {
        for (raw, expected) in [
            ("\"both\"", ClipTargets { to_mic: true, to_output: true }),
            ("\"chat\"", ClipTargets { to_mic: true, to_output: false }),
            ("\"me\"", ClipTargets { to_mic: false, to_output: true }),
        ] {
            let parsed: PlayTargets = serde_json::from_str(raw).expect(raw);
            assert_eq!(ClipTargets::from(parsed), expected, "{raw}");
        }
        // Anything else is a rejection, not a guess at what was meant.
        assert!(serde_json::from_str::<PlayTargets>("\"speakers\"").is_err());
    }
}
