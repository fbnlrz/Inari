use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

use crate::error::SinkError;
use crate::persistence::json::{self, Extra, Version};

const FILE: &str = "outputs.json";

/// Per-channel output device choices (Phase 4), stored as JSON at
/// `$XDG_CONFIG_HOME/inari/outputs.json`. `None` = follow the system default
/// output (with automatic failover, Sonar-style).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChannelOutputs {
    #[serde(default)]
    pub version: Version,
    pub outputs: HashMap<String, Option<String>>,
    /// Channels with auto-failover turned off: they route only to their chosen
    /// device (or the exact system default) and stay silent when it's gone,
    /// rather than falling back to another sink - so e.g. a headset-pinned
    /// channel never surprises you by jumping to the speakers. Absence (the
    /// default) means failover is on. `serde(default)` keeps older configs,
    /// written before this field, loading cleanly.
    #[serde(default)]
    pub no_failover: HashSet<String>,
    #[serde(default, flatten)]
    pub extra: Extra,
}

impl ChannelOutputs {
    pub fn load() -> Self {
        json::load(FILE)
    }

    pub fn save(&self) -> Result<(), SinkError> {
        json::save(FILE, self)
    }

    pub fn set(&mut self, sink_name: &str, output: Option<String>) {
        self.outputs.insert(sink_name.to_string(), output);
    }

    pub fn get(&self, sink_name: &str) -> Option<&str> {
        self.outputs.get(sink_name)?.as_deref()
    }

    /// Whether this channel fails over to another device when its chosen
    /// device (or the default) is gone. On unless explicitly turned off.
    pub fn failover(&self, sink_name: &str) -> bool {
        !self.no_failover.contains(sink_name)
    }

    pub fn set_failover(&mut self, sink_name: &str, enabled: bool) {
        if enabled {
            self.no_failover.remove(sink_name);
        } else {
            self.no_failover.insert(sink_name.to_string());
        }
    }

    /// Drop all state for a removed channel.
    pub fn remove(&mut self, sink_name: &str) {
        self.outputs.remove(sink_name);
        self.no_failover.remove(sink_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_with_follow_default_entries() {
        let mut o = ChannelOutputs::default();
        o.set("sink_game", Some("alsa_output.usb-Headset".into()));
        o.set("sink_music", None);
        let json = serde_json::to_string(&o).expect("serializes");
        let back: ChannelOutputs = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, o);
        assert_eq!(back.get("sink_game"), Some("alsa_output.usb-Headset"));
        assert_eq!(back.get("sink_music"), None);
    }

    #[test]
    fn failover_defaults_on_and_roundtrips() {
        let mut o = ChannelOutputs::default();
        assert!(o.failover("sink_game"), "failover on by default");
        o.set_failover("sink_game", false);
        assert!(!o.failover("sink_game"));
        let back: ChannelOutputs =
            serde_json::from_str(&serde_json::to_string(&o).unwrap()).unwrap();
        assert_eq!(back, o);
        assert!(!back.failover("sink_game"));
        // Turning it back on clears the entry rather than storing `true`.
        o.set_failover("sink_game", true);
        assert!(o.no_failover.is_empty());
    }

    #[test]
    fn old_config_without_no_failover_field_loads() {
        // Configs written before the failover flag have no `no_failover` key.
        let legacy = r#"{"outputs":{"sink_game":"dev","sink_music":null}}"#;
        let o: ChannelOutputs = serde_json::from_str(legacy).expect("legacy loads");
        assert!(o.failover("sink_game"), "missing field means failover on");
        assert_eq!(o.get("sink_game"), Some("dev"));
    }

    #[test]
    fn newer_file_round_trips_without_losing_fields() {
        let raw = r#"{"version":9,"outputs":{"sink_game":"dev"},"latency":{"sink_game":12}}"#;
        let o = json::parse_or_default::<ChannelOutputs>("outputs.json", raw);
        assert_eq!(o.get("sink_game"), Some("dev"));
        assert_eq!(o.version, Version(9));
        let back: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&o).expect("serializes")).expect("value");
        assert_eq!(back["latency"]["sink_game"], serde_json::json!(12));
    }

    #[test]
    fn corrupt_file_degrades_to_default() {
        assert_eq!(
            json::parse_or_default::<ChannelOutputs>("outputs.json", "{ truncated"),
            ChannelOutputs::default()
        );
    }

    #[test]
    fn remove_drops_output_and_failover() {
        let mut o = ChannelOutputs::default();
        o.set("sink_game", Some("dev".into()));
        o.set_failover("sink_game", false);
        o.remove("sink_game");
        assert_eq!(o.get("sink_game"), None);
        assert!(o.failover("sink_game"));
    }
}
