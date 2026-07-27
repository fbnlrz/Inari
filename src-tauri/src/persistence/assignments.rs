use serde::{Deserialize, Serialize};

use crate::error::SinkError;
use crate::persistence::json::{self, Extra, Version};

const FILE: &str = "assignments.json";

/// One persistent routing assignment: streams whose PipeWire property
/// `match_prop` equals `match_value` belong on `sink_name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    /// Property to match, e.g. "application.name".
    pub match_prop: String,
    /// Property value, e.g. "spotify".
    pub match_value: String,
    /// Target virtual sink, e.g. "sink_music".
    pub sink_name: String,
    /// Per-entry fields a newer Inari added, kept so this one's autosaves
    /// don't strip them back off.
    #[serde(default, flatten)]
    pub extra: Extra,
}

/// The set of saved app→channel assignments, stored as JSON at
/// `$XDG_CONFIG_HOME/inari/assignments.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Assignments {
    #[serde(default)]
    pub version: Version,
    pub assignments: Vec<Assignment>,
    #[serde(default, flatten)]
    pub extra: Extra,
}

impl Assignments {
    /// Load from disk; a missing or unreadable file yields the empty set
    /// (first run, or the user deleted their config).
    pub fn load() -> Self {
        json::load(FILE)
    }

    pub fn save(&self) -> Result<(), SinkError> {
        json::save(FILE, self)
    }

    /// Insert or update the assignment for (`match_prop`, `match_value`).
    pub fn set(&mut self, match_prop: &str, match_value: &str, sink_name: &str) {
        match self
            .assignments
            .iter_mut()
            .find(|a| a.match_prop == match_prop && a.match_value == match_value)
        {
            Some(existing) => existing.sink_name = sink_name.to_string(),
            None => self.assignments.push(Assignment {
                match_prop: match_prop.to_string(),
                match_value: match_value.to_string(),
                sink_name: sink_name.to_string(),
                extra: Extra::new(),
            }),
        }
    }

    pub fn remove(&mut self, match_prop: &str, match_value: &str) {
        self.assignments
            .retain(|a| !(a.match_prop == match_prop && a.match_value == match_value));
    }

    pub fn sink_for(&self, match_prop: &str, match_value: &str) -> Option<&str> {
        self.assignments
            .iter()
            .find(|a| a.match_prop == match_prop && a.match_value == match_value)
            .map(|a| a.sink_name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_upserts_and_remove_deletes() {
        let mut a = Assignments::default();
        a.set("application.name", "spotify", "sink_music");
        a.set("application.name", "spotify", "sink_game");
        assert_eq!(a.assignments.len(), 1);
        assert_eq!(a.sink_for("application.name", "spotify"), Some("sink_game"));

        a.remove("application.name", "spotify");
        assert!(a.sink_for("application.name", "spotify").is_none());
        assert!(a.assignments.is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let mut a = Assignments::default();
        a.set("node.name", "audio-src", "sink_system");
        let json = serde_json::to_string(&a).expect("serializes");
        let back: Assignments = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back.assignments, a.assignments);
    }

    #[test]
    fn newer_file_round_trips_without_losing_fields() {
        let raw = r#"{"version":9,"assignments":[
            {"match_prop":"node.name","match_value":"x","sink_name":"sink_game","priority":3}
        ],"rules":{"strict":true}}"#;
        let a = json::parse_or_default::<Assignments>("assignments.json", raw);
        assert_eq!(a.sink_for("node.name", "x"), Some("sink_game"));

        let back: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&a).expect("serializes")).expect("value");
        assert_eq!(back["version"], serde_json::json!(9));
        assert_eq!(back["rules"]["strict"], serde_json::json!(true));
        assert_eq!(back["assignments"][0]["priority"], serde_json::json!(3));
    }

    #[test]
    fn corrupt_file_degrades_to_the_empty_set() {
        assert_eq!(
            json::parse_or_default::<Assignments>("assignments.json", "{ truncated"),
            Assignments::default()
        );
    }
}
