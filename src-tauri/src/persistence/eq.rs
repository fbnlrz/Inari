use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::audio::types::EqConfig;
use crate::error::SinkError;
use crate::persistence::json::{self, Extra, Version};

const FILE: &str = "eq.json";

/// Per-channel parametric EQ configs, stored as JSON at
/// `$XDG_CONFIG_HOME/inari/eq.json`. A missing entry means "never touched" -
/// the default (disabled, flat) config.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelEq {
    #[serde(default)]
    pub version: Version,
    /// `serde(default)` keeps pre-EQ profile files loading cleanly.
    #[serde(default)]
    pub configs: HashMap<String, EqConfig>,
    #[serde(default, flatten)]
    pub extra: Extra,
}

impl ChannelEq {
    pub fn load() -> Self {
        let mut eq: Self = json::load(FILE);
        // Same sanitization the IPC setter applies (TD-050): a hand-edited or
        // torn eq.json otherwise pushes inf/NaN straight through the biquad
        // cascade onto the output device at init.
        for config in eq.configs.values_mut() {
            config.clamp_ranges();
        }
        eq
    }

    pub fn save(&self) -> Result<(), SinkError> {
        json::save(FILE, self)
    }

    /// A channel's EQ, defaulting to disabled/flat when never configured.
    pub fn get(&self, sink_name: &str) -> EqConfig {
        self.configs.get(sink_name).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, sink_name: &str, config: EqConfig) {
        self.configs.insert(sink_name.to_string(), config);
    }

    /// Drop all state for a removed channel.
    pub fn remove(&mut self, sink_name: &str) {
        self.configs.remove(sink_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::types::{default_eq_bands, EqBandKind};

    #[test]
    fn roundtrips_configured_channels() {
        let mut eq = ChannelEq::default();
        let mut config = EqConfig {
            enabled: true,
            preamp_db: -3.0,
            ..EqConfig::default()
        };
        config.bands[0].gain_db = 4.5;
        eq.set("sink_game", config.clone());
        let json = serde_json::to_string(&eq).expect("serializes");
        let back: ChannelEq = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, eq);
        assert_eq!(back.get("sink_game"), config);
    }

    #[test]
    fn unconfigured_channel_gets_default() {
        let eq = ChannelEq::default();
        let config = eq.get("sink_chat");
        assert!(!config.enabled);
        assert_eq!(config.bands, default_eq_bands());
        assert_eq!(config.bands[0].kind, EqBandKind::LowShelf);
    }

    #[test]
    fn legacy_file_without_configs_field_loads() {
        // A pre-EQ profile (or an empty file body) has no `configs` key.
        let eq: ChannelEq = serde_json::from_str("{}").expect("legacy loads");
        assert_eq!(eq, ChannelEq::default());
    }

    #[test]
    fn newer_file_round_trips_without_losing_fields() {
        // The EQ store carries real user work; a downgrade autosaving it
        // must not drop what it could not read.
        let raw = r#"{"version":9,"configs":{},"crossfeed":{"amount":0.4}}"#;
        let eq = json::parse_or_default::<ChannelEq>("eq.json", raw);
        assert_eq!(eq.version, Version(9));
        let back: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&eq).expect("serializes")).expect("value");
        assert_eq!(back["crossfeed"]["amount"], serde_json::json!(0.4));
    }

    #[test]
    fn corrupt_file_degrades_to_default() {
        assert_eq!(
            json::parse_or_default::<ChannelEq>("eq.json", "{ truncated"),
            ChannelEq::default()
        );
    }

    #[test]
    fn remove_drops_config() {
        let mut eq = ChannelEq::default();
        eq.set("sink_game", EqConfig::default());
        eq.remove("sink_game");
        assert!(eq.configs.is_empty());
    }
}
