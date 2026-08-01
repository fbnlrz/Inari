use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::audio::types::VirtualSink;
use crate::error::SinkError;
use crate::persistence::assignments::Assignments;
use crate::persistence::json::{Extra, Version};

/// A named snapshot of the mixer: channel volumes/mutes, the app→channel
/// assignment set, and per-channel output choices. Stored as JSON in
/// `$XDG_CONFIG_HOME/inari/profiles/<name>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub version: Version,
    pub name: String,
    pub channels: Vec<VirtualSink>,
    pub assignments: Assignments,
    /// Added in Phase 4; default keeps older profile files loadable.
    #[serde(default)]
    pub outputs: crate::persistence::outputs::ChannelOutputs,
    /// Per-channel parametric EQ; default keeps older profile files loadable.
    #[serde(default)]
    pub eq: crate::persistence::eq::ChannelEq,
    /// Phase 5: output device (node.name) whose appearance auto-loads this
    /// profile - Sonar-style hardware profile switching.
    #[serde(default)]
    pub trigger_device: Option<String>,
    /// User-defined mixes (record buses) with their member channels.
    #[serde(default)]
    pub buses: crate::persistence::buses::Buses,
    /// Whatever a newer Inari put here. Every other store carries one of
    /// these; a profile written by a newer version and then autosaved by this
    /// one used to come back with the new version's work quietly deleted.
    #[serde(default, flatten)]
    pub extra: Extra,
}

/// Listing entry: name plus trigger metadata for the UI/auto-switcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub name: String,
    pub trigger_device: Option<String>,
}

fn profiles_dir() -> Result<PathBuf, SinkError> {
    let dir = dirs::config_dir()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
    Ok(dir.join("inari").join("profiles"))
}

/// Profile names become file names: restrict to a safe charset so a name
/// can never traverse out of the profiles directory.
pub fn sanitize_name(name: &str) -> Result<String, SinkError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(SinkError::Config(
            "profile name must be 1-64 characters".into(),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_')
    {
        return Err(SinkError::Config(
            "profile name may only contain letters, digits, spaces, '-' and '_'".into(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Does a profile file exist under this name?
///
/// Deliberately a filesystem question, not a parse. Asking `load(..).is_ok()`
/// answers "is there a *readable* profile", so a file that had become
/// unparseable — hand-edited, half-restored from a backup, or written by a
/// newer version with a renamed field — read as absent and got silently
/// overwritten. That profile is visible in the list the whole time, because
/// `list` takes the filename and shrugs off a parse failure.
pub fn exists(name: &str) -> bool {
    profile_path(name).map(|p| p.exists()).unwrap_or(false)
}

fn profile_path(name: &str) -> Result<PathBuf, SinkError> {
    Ok(profiles_dir()?.join(format!("{}.json", sanitize_name(name)?)))
}

pub fn list() -> Result<Vec<ProfileInfo>, SinkError> {
    let dir = profiles_dir()?;
    let mut infos = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(infos),
        Err(e) => return Err(e.into()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let trigger_device = fs::read_to_string(&path)
                    .ok()
                    .and_then(|raw| serde_json::from_str::<Profile>(&raw).ok())
                    .and_then(|p| p.trigger_device);
                infos.push(ProfileInfo {
                    name: stem.to_string(),
                    trigger_device,
                });
            }
        }
    }
    infos.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(infos)
}

/// Set or clear the trigger device on an existing profile file.
pub fn set_trigger(name: &str, trigger_device: Option<String>) -> Result<(), SinkError> {
    let mut profile = load(name)?;
    profile.trigger_device = trigger_device;
    save(&profile)
}

pub fn save(profile: &Profile) -> Result<(), SinkError> {
    crate::persistence::json::save_at(&profile_path(&profile.name)?, profile)
}

pub fn load(name: &str) -> Result<Profile, SinkError> {
    let path = profile_path(name)?;
    let raw = fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            SinkError::Config(format!("no such profile: {name}"))
        } else {
            e.into()
        }
    })?;
    serde_json::from_str(&raw).map_err(|e| SinkError::Config(format!("malformed profile {name}: {e}")))
}

pub fn delete(name: &str) -> Result<(), SinkError> {
    let path = profile_path(name)?;
    fs::remove_file(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            SinkError::Config(format!("no such profile: {name}"))
        } else {
            e.into()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_accepts_reasonable_names() {
        assert_eq!(sanitize_name("Gaming").expect("valid"), "Gaming");
        assert_eq!(sanitize_name("  Work_2 -late ").expect("valid"), "Work_2 -late");
    }

    #[test]
    fn sanitize_rejects_traversal_and_garbage() {
        assert!(sanitize_name("../etc/passwd").is_err());
        assert!(sanitize_name("a/b").is_err());
        assert!(sanitize_name("").is_err());
        assert!(sanitize_name("   ").is_err());
        assert!(sanitize_name(&"x".repeat(65)).is_err());
        assert!(sanitize_name("nul\0byte").is_err());
    }
}
