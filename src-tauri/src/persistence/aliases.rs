use serde::{Deserialize, Serialize};

use crate::error::SinkError;
use crate::persistence::json::{self, Extra, Version};

const FILE: &str = "aliases.json";

/// A user-chosen display name for a discovered app, keyed by the same
/// stream identity used for routing assignments (e.g. apps like Spotify
/// that only expose a generic `media.name = "audio-src"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AliasEntry {
    pub match_prop: String,
    pub match_value: String,
    pub alias: String,
    /// Per-entry fields a newer Inari added, kept so this one's autosaves
    /// don't strip them back off.
    #[serde(default, flatten)]
    pub extra: Extra,
}

/// All saved aliases, stored as JSON at `$XDG_CONFIG_HOME/inari/aliases.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Aliases {
    #[serde(default)]
    pub version: Version,
    pub aliases: Vec<AliasEntry>,
    #[serde(default, flatten)]
    pub extra: Extra,
}

impl Aliases {
    pub fn load() -> Self {
        json::load(FILE)
    }

    pub fn save(&self) -> Result<(), SinkError> {
        json::save(FILE, self)
    }

    /// Set the alias for an identity; an empty alias removes it.
    pub fn set(&mut self, match_prop: &str, match_value: &str, alias: &str) {
        let alias = alias.trim();
        if alias.is_empty() {
            self.aliases
                .retain(|a| !(a.match_prop == match_prop && a.match_value == match_value));
            return;
        }
        match self
            .aliases
            .iter_mut()
            .find(|a| a.match_prop == match_prop && a.match_value == match_value)
        {
            Some(existing) => existing.alias = alias.to_string(),
            None => self.aliases.push(AliasEntry {
                match_prop: match_prop.to_string(),
                match_value: match_value.to_string(),
                alias: alias.to_string(),
                extra: Extra::new(),
            }),
        }
    }

    pub fn get(&self, match_prop: &str, match_value: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|a| a.match_prop == match_prop && a.match_value == match_value)
            .map(|a| a.alias.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_and_empty_removes() {
        let mut a = Aliases::default();
        a.set("media.name", "audio-src", "Spotify");
        assert_eq!(a.get("media.name", "audio-src"), Some("Spotify"));

        a.set("media.name", "audio-src", "Spotify Premium");
        assert_eq!(a.aliases.len(), 1);
        assert_eq!(a.get("media.name", "audio-src"), Some("Spotify Premium"));

        a.set("media.name", "audio-src", "   ");
        assert!(a.get("media.name", "audio-src").is_none());
        assert!(a.aliases.is_empty());
    }

    #[test]
    fn newer_file_round_trips_without_losing_fields() {
        // What a downgrade does: load a file from a newer Inari, then
        // autosave it. Both the top-level and the per-entry additions have
        // to come back out unchanged.
        let raw = r#"{"version":9,"aliases":[
            {"match_prop":"media.name","match_value":"audio-src","alias":"Spotify","color":"magenta"}
        ],"groups":["music"]}"#;
        let a = json::parse_or_default::<Aliases>("aliases.json", raw);
        assert_eq!(a.get("media.name", "audio-src"), Some("Spotify"));

        let back: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&a).expect("serializes")).expect("value");
        assert_eq!(back["version"], serde_json::json!(9));
        assert_eq!(back["groups"], serde_json::json!(["music"]));
        assert_eq!(back["aliases"][0]["color"], serde_json::json!("magenta"));
    }

    #[test]
    fn corrupt_file_degrades_to_the_empty_set() {
        assert_eq!(
            json::parse_or_default::<Aliases>("aliases.json", "{ truncated"),
            Aliases::default()
        );
        // A file without the version key is stamped as the current schema.
        let a = json::parse_or_default::<Aliases>("aliases.json", r#"{"aliases":[]}"#);
        assert_eq!(a.version, Version::default());
    }
}
