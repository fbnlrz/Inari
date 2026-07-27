use std::fs;
use std::path::PathBuf;

use log::warn;

use crate::audio::types::MicConfig;
use crate::error::SinkError;

/// Mic chain configuration, stored as JSON at
/// `$XDG_CONFIG_HOME/inari/mic.json`.
pub fn config_path() -> Result<PathBuf, SinkError> {
    let dir = dirs::config_dir()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
    Ok(dir.join("inari").join("mic.json"))
}

pub fn load() -> MicConfig {
    let Ok(path) = config_path() else {
        return MicConfig::default();
    };
    let mut config: MicConfig = match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            warn!("ignoring malformed {}: {e}", path.display());
            MicConfig::default()
        }),
        Err(_) => MicConfig::default(),
    };
    // Same sanitization the IPC setter applies (TD-050): a hand-edited or torn
    // mic.json otherwise pushes inf/NaN into the chain at init.
    config.clamp_ranges();
    config
}

pub fn save(config: &MicConfig) -> Result<(), SinkError> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        crate::persistence::ensure_private_dir(parent)?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| SinkError::Config(format!("serialize mic config: {e}")))?;
    super::write_atomic(&path, &json)?;
    Ok(())
}
