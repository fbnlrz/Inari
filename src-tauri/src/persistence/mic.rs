use crate::audio::types::MicConfig;
use crate::error::SinkError;
use crate::persistence::json;

/// Mic chain configuration, stored as JSON at
/// `$XDG_CONFIG_HOME/inari/mic.json`.
const FILE: &str = "mic.json";

/// The only store without a schema version or an unknown-field bag: its
/// shape is `audio::types::MicConfig`, which lives with the IPC types.
pub fn load() -> MicConfig {
    let mut config: MicConfig = json::load(FILE);
    // Same sanitization the IPC setter applies (TD-050): a hand-edited or torn
    // mic.json otherwise pushes inf/NaN into the chain at init.
    config.clamp_ranges();
    config
}

pub fn save(config: &MicConfig) -> Result<(), SinkError> {
    json::save(FILE, config)
}
