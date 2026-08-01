use log::warn;
use serde::{Deserialize, Serialize};

use crate::error::SinkError;
use crate::persistence::json;
use crate::persistence::json::{Extra, Version};

const FILE: &str = "channels.json";

/// Sink node names reserved by Sink itself (not user channels).
pub const RESERVED_SINK_NAMES: [&str; 2] = ["sink_mic", "sink_stream"];
/// Upper bound on user channels (level-meter slots are budgeted for this).
pub const MAX_CHANNELS: usize = 10;

/// One user-defined mixer channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelDef {
    /// PipeWire sink node name, e.g. "sink_game". Stable once created.
    pub name: String,
    /// Display label, e.g. "Game". Renameable.
    pub label: String,
    /// Material Symbol name for the strip icon (None = legacy default).
    #[serde(default)]
    pub icon: Option<String>,
    /// Whether the channel feeds the Stream Mix source (default: yes).
    #[serde(default = "default_true")]
    pub stream_mix: bool,
    /// Per-channel fields a newer Inari added, kept so this one's saves don't
    /// strip them back off.
    #[serde(default, flatten)]
    pub extra: Extra,
}

fn default_true() -> bool {
    true
}

/// The user's channel set, stored as JSON at
/// `$XDG_CONFIG_HOME/inari/channels.json`. Defaults to the classic four.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channels {
    #[serde(default)]
    pub version: Version,
    pub channels: Vec<ChannelDef>,
    #[serde(default, flatten)]
    pub extra: Extra,
}

impl Default for Channels {
    fn default() -> Self {
        let def = |name: &str, label: &str, icon: &str| ChannelDef {
            name: name.to_string(),
            label: label.to_string(),
            icon: Some(icon.to_string()),
            stream_mix: true,
            extra: Extra::new(),
        };
        Self {
            version: Version::default(),
            extra: Extra::new(),
            channels: vec![
                def("sink_game", "Game", "sports_esports"),
                def("sink_chat", "Chat", "forum"),
                def("sink_music", "Music", "music_note"),
                def("sink_system", "System", "desktop_windows"),
            ],
        }
    }
}

fn slugify(label: &str) -> String {
    let slug: String = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if slug.is_empty() {
        "channel".to_string()
    } else {
        slug
    }
}

impl Channels {
    pub fn load() -> Self {
        // A missing file is first run (silent default); a present-but-broken
        // one is a torn or hand-edited write the shared loader logs rather
        // than honours - it hands back the default, which sanitizes to the
        // classic four.
        let Some(raw) = json::read(FILE) else {
            return Self::default();
        };
        let channels = json::parse_or_default::<Self>(FILE, &raw).sanitized();
        if channels.channels.is_empty() {
            warn!("channels.json held no valid channels; using defaults");
            return Self::default();
        }
        channels
    }

    /// Sanitize the channel set. Entries that break the invariants `add()`
    /// guarantees (a reserved name, a missing `sink_` prefix, or a duplicate)
    /// are dropped: a hand-edited or foreign-tool file would otherwise collide
    /// with the mic/stream service nodes, or make `is_virtual_sink` reject a
    /// channel and abort `init_virtual_devices`. The set is capped at
    /// `MAX_CHANNELS` so it can't exhaust the level-meter slots.
    fn sanitized(self) -> Self {
        let mut seen = std::collections::HashSet::new();
        let mut channels = Vec::new();
        for def in self.channels {
            let name = def.name.as_str();
            let valid = name.starts_with("sink_")
                && !RESERVED_SINK_NAMES.contains(&name)
                && seen.insert(def.name.clone());
            if valid && channels.len() < MAX_CHANNELS {
                channels.push(def);
            } else {
                warn!("dropping invalid channel '{}' from channels.json", def.name);
            }
        }
        Self {
            version: self.version,
            extra: self.extra.clone(),
            channels,
        }
    }

    pub fn save(&self) -> Result<(), SinkError> {
        json::save(FILE, self)
    }

    pub fn get(&self, name: &str) -> Option<&ChannelDef> {
        self.channels.iter().find(|c| c.name == name)
    }

    pub fn set_icon(&mut self, name: &str, icon: Option<String>) -> Result<(), SinkError> {
        let def = self
            .channels
            .iter_mut()
            .find(|c| c.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        def.icon = icon;
        Ok(())
    }

    /// Add a channel for `label`, generating a unique reserved-safe sink
    /// name. Returns the new definition.
    pub fn add(&mut self, label: &str, icon: Option<String>) -> Result<ChannelDef, SinkError> {
        let label = label.trim();
        if label.is_empty() || label.len() > 24 {
            return Err(SinkError::Config("channel label must be 1-24 characters".into()));
        }
        if self.channels.len() >= MAX_CHANNELS {
            return Err(SinkError::Config(format!(
                "at most {MAX_CHANNELS} channels are supported"
            )));
        }
        let base = format!("sink_{}", slugify(label));
        let mut name = base.clone();
        let mut counter = 2;
        while self.get(&name).is_some() || RESERVED_SINK_NAMES.contains(&name.as_str()) {
            name = format!("{base}_{counter}");
            counter += 1;
        }
        let def = ChannelDef {
            name,
            label: label.to_string(),
            icon,
            stream_mix: true,
            extra: Extra::new(),
        };
        self.channels.push(def.clone());
        Ok(def)
    }

    pub fn rename(&mut self, name: &str, label: &str) -> Result<(), SinkError> {
        let label = label.trim();
        if label.is_empty() || label.len() > 24 {
            return Err(SinkError::Config("channel label must be 1-24 characters".into()));
        }
        let def = self
            .channels
            .iter_mut()
            .find(|c| c.name == name)
            .ok_or_else(|| SinkError::UnknownSink(name.to_string()))?;
        def.label = label.to_string();
        Ok(())
    }

    /// Reorder the channel set. `order` must contain exactly the current
    /// sink names (it's a permutation, not an edit).
    pub fn reorder(&mut self, order: &[String]) -> Result<(), SinkError> {
        if order.len() != self.channels.len()
            || !order.iter().all(|n| self.get(n).is_some())
        {
            return Err(SinkError::Config(
                "reorder must list every existing channel exactly once".into(),
            ));
        }
        self.channels.sort_by_key(|c| {
            order.iter().position(|n| n == &c.name).unwrap_or(usize::MAX)
        });
        Ok(())
    }

    /// Can this channel be removed? Answers without changing anything.
    ///
    /// Split out from [`Self::remove`] so the caller can validate before it
    /// starts touching the audio graph, and only commit once the parts that
    /// can fail have succeeded.
    pub fn can_remove(&self, name: &str) -> Result<(), SinkError> {
        if self.channels.len() <= 1 {
            return Err(SinkError::Config("at least one channel is required".into()));
        }
        if !self.channels.iter().any(|c| c.name == name) {
            return Err(SinkError::UnknownSink(name.to_string()));
        }
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<(), SinkError> {
        self.can_remove(name)?;
        self.channels.retain(|c| c.name != name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_from_a_newer_version_survives_a_load_and_save() {
        // The doctrine in json.rs: every store carries a version and a
        // catch-all, so an older Inari that loads a newer file and then
        // autosaves does not delete what it did not understand. Eight stores
        // followed it; channels.json and the profiles did not, and they are
        // the ones holding the channel set itself.
        let json = r##"{
            "version": 7,
            "channels": [
                {"name":"sink_game","label":"Game","icon":"sports_esports",
                 "stream_mix":true,"colour":"#ff5200"}
            ],
            "layout": "vertical"
        }"##;
        let loaded: Channels = serde_json::from_str(json).expect("loads");
        assert_eq!(loaded.version.0, 7, "a newer version is kept, not relabelled");
        assert_eq!(
            loaded.extra.get("layout").and_then(|v| v.as_str()),
            Some("vertical")
        );
        assert_eq!(
            loaded.channels[0].extra.get("colour").and_then(|v| v.as_str()),
            Some("#ff5200")
        );

        let written = serde_json::to_value(&loaded).expect("serialises");
        assert_eq!(written["layout"], "vertical");
        assert_eq!(written["version"], 7);
        assert_eq!(written["channels"][0]["colour"], "#ff5200");
    }

    #[test]
    fn defaults_to_classic_four() {
        let c = Channels::default();
        assert_eq!(c.channels.len(), 4);
        assert_eq!(c.channels[0].name, "sink_game");
    }

    #[test]
    fn add_generates_unique_safe_names() {
        let mut c = Channels::default();
        let d = c.add("Voice Chat!", Some("mic".into())).expect("adds");
        assert_eq!(d.name, "sink_voice_chat");
        assert_eq!(d.icon.as_deref(), Some("mic"));
        let d2 = c.add("Voice Chat", None).expect("adds duplicate label");
        assert_eq!(d2.name, "sink_voice_chat_2");
        // Reserved collision: label "mic" must not produce sink_mic.
        let d3 = c.add("Mic", None).expect("adds");
        assert_eq!(d3.name, "sink_mic_2");
    }

    #[test]
    fn pathological_labels_hit_the_slug_fallback() {
        let mut c = Channels::default();
        // All-special-char labels slugify to empty → "channel" fallback.
        let d = c.add("!!!", None).expect("adds");
        assert_eq!(d.name, "sink_channel");
        let d2 = c.add("___", None).expect("adds second pathological label");
        assert_eq!(d2.name, "sink_channel_2");
        // Whitespace-only labels are rejected outright.
        assert!(c.add("   ", None).is_err());
    }

    #[test]
    fn reorder_is_a_strict_permutation() {
        let mut c = Channels::default();
        c.reorder(&[
            "sink_music".into(),
            "sink_game".into(),
            "sink_system".into(),
            "sink_chat".into(),
        ])
        .expect("reorders");
        let names: Vec<&str> = c.channels.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["sink_music", "sink_game", "sink_system", "sink_chat"]);
        // Wrong length and unknown names are rejected.
        assert!(c.reorder(&["sink_game".into()]).is_err());
        assert!(c
            .reorder(&[
                "sink_music".into(),
                "sink_game".into(),
                "sink_system".into(),
                "sink_nope".into(),
            ])
            .is_err());
    }

    #[test]
    fn remove_keeps_at_least_one() {
        let mut c = Channels::default();
        c.remove("sink_game").expect("removes");
        c.remove("sink_chat").expect("removes");
        c.remove("sink_music").expect("removes");
        assert!(c.remove("sink_system").is_err(), "last channel must stay");
    }

    #[test]
    fn rename_updates_label_only() {
        let mut c = Channels::default();
        c.rename("sink_game", "Gaems").expect("renames");
        assert_eq!(c.get("sink_game").expect("exists").label, "Gaems");
        assert!(c.rename("sink_nope", "X").is_err());
    }

    /// What `load` does to a file body, minus the filesystem.
    fn parse(raw: &str) -> Channels {
        json::parse_or_default::<Channels>(FILE, raw).sanitized()
    }

    #[test]
    fn parse_keeps_valid_and_fills_serde_defaults() {
        // Old-shape entries (pre-Phase-4: no icon / stream_mix) must still
        // load, with the serde defaults applied - an upgrade keeps user data.
        let raw = r#"{"channels":[
            {"name":"sink_game","label":"Game"},
            {"name":"sink_music","label":"Music","icon":"music_note","stream_mix":false}
        ]}"#;
        let c = parse(raw);
        assert_eq!(c.channels.len(), 2);
        assert_eq!(c.channels[0].icon, None);
        assert!(c.channels[0].stream_mix, "missing stream_mix defaults true");
        assert!(!c.channels[1].stream_mix);
    }

    #[test]
    fn parse_drops_reserved_unprefixed_and_duplicate_names() {
        let raw = r#"{"channels":[
            {"name":"sink_game","label":"Game"},
            {"name":"sink_game","label":"Dup"},
            {"name":"sink_mic","label":"Reserved"},
            {"name":"nope","label":"NoPrefix"},
            {"name":"sink_ok","label":"Fine"}
        ]}"#;
        let names: Vec<String> = parse(raw).channels.into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["sink_game", "sink_ok"]);
    }

    #[test]
    fn parse_caps_at_max_channels() {
        let mut items = Vec::new();
        for i in 0..(MAX_CHANNELS + 5) {
            items.push(format!(r#"{{"name":"sink_c{i}","label":"C{i}"}}"#));
        }
        let raw = format!(r#"{{"channels":[{}]}}"#, items.join(","));
        assert_eq!(parse(&raw).channels.len(), MAX_CHANNELS);
    }

    #[test]
    fn corrupt_or_empty_text_degrades_to_defaults() {
        // The shared loader logs and hands back the default set, which
        // sanitizes to itself - so a torn file lands on the classic four.
        assert_eq!(parse("{ truncated"), Channels::default());
        assert_eq!(parse(""), Channels::default());
    }

    #[test]
    fn all_invalid_yields_empty_set() {
        // load() turns this into defaults; sanitization itself reports the
        // empty set so load can log why it fell back.
        let raw = r#"{"channels":[{"name":"sink_mic","label":"x"},{"name":"bad","label":"y"}]}"#;
        assert!(parse(raw).channels.is_empty());
    }
}
