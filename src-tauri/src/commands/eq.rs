use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use tauri::State;

use crate::audio::eq_import::parse_autoeq;
use crate::audio::presets::{bundled_presets, EqPreset, PRESET_SCHEMA};
use crate::audio::types::EqConfig;
use crate::persistence::eq_presets;
use crate::state::AppState;

/// All channels' EQ configs in one round-trip (channels without an entry
/// have never been configured - the frontend treats them as default).
#[tauri::command]
pub fn get_channel_eq_configs(
    state: State<'_, AppState>,
) -> Result<HashMap<String, EqConfig>, String> {
    Ok(state.lock_mixer()?.eq.configs.clone())
}

/// Apply and persist one channel's parametric EQ.
#[tauri::command]
pub fn set_channel_eq(
    state: State<'_, AppState>,
    sink_name: String,
    mut config: EqConfig,
) -> Result<(), String> {
    config.clamp_ranges();
    state
        .backend
        .set_channel_eq(&sink_name, &config)
        .map_err(|e| e.to_string())?;
    let (eq, snapshot) = {
        let mut mixer = state.lock_mixer()?;
        mixer.eq.set(&sink_name, config);
        (
            mixer.eq.clone(),
            crate::commands::profiles::build_autosave(&mixer),
        )
    };
    crate::commands::profiles::write_autosave(snapshot);
    eq.save().map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct EqPresetEntry {
    /// "bundled" (ships in the binary) or "user" (local library).
    pub source: String,
    pub preset: EqPreset,
}

/// Bundled presets first, then the user's library, each sorted by name.
#[tauri::command]
pub fn list_eq_presets() -> Result<Vec<EqPresetEntry>, String> {
    let mut entries: Vec<EqPresetEntry> = bundled_presets()
        .into_iter()
        .map(|preset| EqPresetEntry {
            source: "bundled".into(),
            preset,
        })
        .collect();
    entries.extend(
        eq_presets::list()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|preset| EqPresetEntry {
                source: "user".into(),
                preset,
            }),
    );
    Ok(entries)
}

/// Save the given config into the user's local preset library.
#[tauri::command]
pub fn save_user_eq_preset(name: String, mut config: EqConfig) -> Result<(), String> {
    config.clamp_ranges();
    let preset = EqPreset {
        schema: PRESET_SCHEMA,
        name,
        author: None,
        description: None,
        preamp_db: config.preamp_db,
        bands: config.bands,
    };
    eq_presets::save(&preset).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_user_eq_preset(name: String) -> Result<(), String> {
    eq_presets::delete(&name).map_err(|e| e.to_string())
}

/// A channel's EQ as shareable preset JSON (pretty-printed, schema 1).
#[tauri::command]
pub fn export_channel_eq(state: State<'_, AppState>, sink_name: String) -> Result<String, String> {
    let (config, label) = {
        let mixer = state.lock_mixer()?;
        let label = mixer
            .channels
            .iter()
            .find(|c| c.name == sink_name)
            .map(|c| c.label.clone())
            .unwrap_or_else(|| sink_name.clone());
        (mixer.eq.get(&sink_name), label)
    };
    let preset = EqPreset {
        schema: PRESET_SCHEMA,
        name: label,
        author: None,
        description: None,
        preamp_db: config.preamp_db,
        bands: config.bands,
    };
    serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())
}

/// The directories the preset file commands may touch: the user's home, plus
/// the XDG user dirs, which can be relocated onto another volume.
fn allowed_roots() -> Vec<PathBuf> {
    [
        dirs::home_dir(),
        dirs::desktop_dir(),
        dirs::document_dir(),
        dirs::download_dir(),
        dirs::config_dir(),
        dirs::data_dir(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Gate for the two preset file commands. The frontend only ever passes a path
/// the user picked in a native dialog, but the IPC boundary doesn't enforce
/// that - a compromised webview could hand us `/etc/shadow` or `~/.ssh/config`
/// just as easily. So: absolute, no `..` (checked lexically, since an export
/// target need not exist yet and so can't be canonicalized), and under one of
/// [`allowed_roots`].
fn user_path(path: &str) -> Result<&Path, String> {
    let candidate = Path::new(path);
    let outside = || format!("refusing a path outside your own directories: {path}");
    if !candidate.is_absolute() || candidate.components().any(|c| c == Component::ParentDir) {
        return Err(outside());
    }
    if !allowed_roots().iter().any(|root| candidate.starts_with(root)) {
        return Err(outside());
    }
    Ok(candidate)
}

/// Write a channel's EQ preset JSON to `path` (picked via the native save
/// dialog; the file I/O stays in Rust, and [`user_path`] enforces what the
/// dialog only implies - IPC could pass any path at all).
#[tauri::command]
pub fn export_channel_eq_to_file(
    state: State<'_, AppState>,
    sink_name: String,
    path: String,
) -> Result<(), String> {
    let target = user_path(&path)?;
    let json = export_channel_eq(state, sink_name)?;
    std::fs::write(target, json).map_err(|e| format!("write {path}: {e}"))
}

/// Parse pasted preset text: our JSON schema or an AutoEq result block.
/// Returns a disabled config for preview-then-apply in the modal.
#[tauri::command]
pub fn import_eq_config(text: String) -> Result<EqConfig, String> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        let preset: EqPreset = serde_json::from_str(trimmed).map_err(|e| e.to_string())?;
        if preset.schema != PRESET_SCHEMA {
            return Err(format!("unsupported preset schema {}", preset.schema));
        }
        if preset.bands.is_empty() {
            return Err("preset has no bands".into());
        }
        let mut config = preset.to_config();
        config.enabled = false;
        Ok(config)
    } else {
        parse_autoeq(trimmed).map_err(|e| e.to_string())
    }
}

/// Read + parse a preset file picked via the native open dialog. Same
/// [`user_path`] gate as the export side: without it this command reads any
/// file the app can, and hands its first line back to the webview in the parse
/// error.
#[tauri::command]
pub fn import_eq_file(path: String) -> Result<EqConfig, String> {
    let source = user_path(&path)?;
    let text = std::fs::read_to_string(source).map_err(|e| format!("read {path}: {e}"))?;
    import_eq_config(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::types::{EqBand, EqBandKind};

    fn home() -> PathBuf {
        dirs::home_dir().expect("the test host has a home directory")
    }

    fn band(freq_hz: f32) -> EqBand {
        EqBand {
            kind: EqBandKind::Peaking,
            freq_hz,
            gain_db: 3.0,
            q: 1.0,
        }
    }

    /// Preset JSON built through the real type, so the tests can't drift from
    /// the wire format.
    fn preset_text(schema: u32, bands: Vec<EqBand>) -> String {
        serde_json::to_string(&EqPreset {
            schema,
            name: "T".into(),
            author: None,
            description: None,
            preamp_db: 0.0,
            bands,
        })
        .expect("serializes")
    }

    // --- the path gate ----------------------------------------------------

    #[test]
    fn user_path_accepts_a_file_in_the_home_directory() {
        assert!(user_path(&home().join("preset.json").to_string_lossy()).is_ok());
    }

    #[test]
    fn user_path_refuses_relative_and_traversing_paths() {
        assert!(user_path("preset.json").is_err(), "relative");
        assert!(user_path("~/preset.json").is_err(), "an unexpanded tilde");
        let escape = home().join("../../etc/shadow");
        assert!(user_path(&escape.to_string_lossy()).is_err(), "..");
    }

    #[test]
    fn user_path_refuses_paths_outside_the_users_own_directories() {
        assert!(user_path("/etc/shadow").is_err());
        assert!(user_path("/proc/self/environ").is_err());
        assert!(user_path("/").is_err());
    }

    #[test]
    fn user_path_matches_components_not_string_prefixes() {
        // "/home/bob-evil" shares a string prefix with "/home/bob" but is a
        // different directory, and Path::starts_with must be what decides.
        let sibling = format!("{}-evil/preset.json", home().to_string_lossy());
        assert!(user_path(&sibling).is_err());
    }

    #[test]
    fn import_eq_file_refuses_before_it_reads() {
        // The gate has to fire first: otherwise the file's contents come back
        // to the webview inside the parse error.
        let err = import_eq_file("/etc/hostname".into()).expect_err("outside home");
        assert!(err.starts_with("refusing a path"), "{err}");
    }

    // --- preset parsing ---------------------------------------------------

    #[test]
    fn import_eq_config_previews_disabled_and_clamped() {
        let config = import_eq_config(preset_text(PRESET_SCHEMA, vec![band(99_000.0)]))
            .expect("imports");
        assert!(!config.enabled, "the modal previews before applying");
        assert_eq!(config.bands[0].freq_hz, 20_000.0);
    }

    #[test]
    fn import_eq_config_rejects_unusable_preset_json() {
        assert!(import_eq_config("{not json".into()).is_err());
        assert!(
            import_eq_config(preset_text(PRESET_SCHEMA + 1, vec![band(1000.0)])).is_err(),
            "future schema"
        );
        assert!(
            import_eq_config(preset_text(PRESET_SCHEMA, Vec::new())).is_err(),
            "no bands"
        );
    }

    #[test]
    fn import_eq_config_falls_through_to_autoeq_for_non_json() {
        let config = import_eq_config(
            "Preamp: -6.0 dB\nFilter 1: ON PK Fc 105 Hz Gain -2.4 dB Q 0.70\n".into(),
        )
        .expect("parses");
        assert_eq!(config.preamp_db, -6.0);
        assert!(!config.enabled);
        // Leading whitespace must not push pasted JSON onto the AutoEq branch.
        let padded = format!("\n  {}", preset_text(PRESET_SCHEMA, vec![band(1000.0)]));
        assert!(import_eq_config(padded).is_ok());
    }

    #[test]
    fn import_eq_config_rejects_text_that_is_neither_format() {
        assert!(import_eq_config("hello".into()).is_err());
        assert!(import_eq_config(String::new()).is_err());
    }
}
