//! Marker for the active profile (plain text file). The active profile is
//! live-bound: mixer changes autosave into it, and the marker survives
//! restarts so the UI shows the right name immediately.

use std::fs;
use std::path::PathBuf;

use crate::error::SinkError;

fn marker_path() -> Result<PathBuf, SinkError> {
    let dir = dirs::config_dir()
        .ok_or_else(|| SinkError::Config("cannot resolve the user config directory".into()))?;
    Ok(dir.join("inari").join("active_profile"))
}

pub fn load() -> Option<String> {
    let path = marker_path().ok()?;
    let name = fs::read_to_string(path).ok()?.trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub fn save(name: Option<&str>) -> Result<(), SinkError> {
    let path = marker_path()?;
    match name {
        Some(name) => {
            if let Some(parent) = path.parent() {
                crate::persistence::ensure_private_dir(parent)?;
            }
            super::write_atomic(&path, name)?;
        }
        None => {
            if path.exists() {
                fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}
