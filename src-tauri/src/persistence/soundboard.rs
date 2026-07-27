//! The soundboard library: the clips the user can fire, and the mic-ducking
//! setting that goes with them.
//!
//! Clips are addressed by a generated id, never by their path. Firing one is
//! meant to work from a global hotkey and from the tablet remote, and a
//! command that takes a path would make "play this clip" and "read this file"
//! the same request - the lesson `commands/media.rs` spells out for cover art.
//! The path is stored here, resolved here, and never travels back out to a
//! client that could choose it.

use serde::{Deserialize, Serialize};

use crate::error::SinkError;
use crate::persistence::json::{self, Extra, Version};

const FILE: &str = "soundboard.json";

/// How far a clip's own level may be trimmed. Recorded snippets arrive at
/// wildly different levels, so a clip may be pushed above unity - the playback
/// path clamps the samples, which is the honest ceiling for a boost.
pub const MAX_CLIP_VOLUME: u8 = 200;

/// Ducking range. 0 dB is "no attenuation" (indistinguishable from off) and
/// -40 dB is already inaudible; beyond that the setting is just a mute with
/// extra steps.
pub const MIN_DUCK_DB: f32 = -40.0;
pub const MAX_DUCK_DB: f32 = 0.0;

/// One clip in the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Stable, generated, and the only thing a play request carries.
    pub id: String,
    /// What the button says.
    pub name: String,
    /// Absolute path of the source file, canonicalised when it was added.
    /// Never sent to a client and never accepted from one.
    pub path: String,
    /// Per-clip trim in percent (100 = as recorded).
    pub volume_percent: u8,
    /// Per-entry fields a newer Inari added, kept so this one's saves don't
    /// strip them back off.
    #[serde(default, flatten)]
    pub extra: Extra,
}

/// Whether - and how far - the microphone is attenuated while a clip plays.
///
/// Off by default: lowering someone's microphone is not a thing to do
/// unasked, and a soundboard is perfectly usable with the mic left open.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Duck {
    pub enabled: bool,
    /// Attenuation applied to the processed voice, in dB (negative).
    pub attenuation_db: f32,
}

impl Default for Duck {
    fn default() -> Self {
        Self {
            enabled: false,
            // A clearly audible step back that still leaves the speaker
            // present, rather than cutting them out of their own clip.
            attenuation_db: -12.0,
        }
    }
}

impl Duck {
    /// The linear factor the DSP chain multiplies the voice by. 1.0 whenever
    /// ducking is off, so "off" and "0 dB" are the same signal path.
    pub fn factor(&self) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        10f32.powf(self.attenuation_db.clamp(MIN_DUCK_DB, MAX_DUCK_DB) / 20.0)
    }
}

/// The whole library, stored at `$XDG_CONFIG_HOME/inari/soundboard.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Soundboard {
    #[serde(default)]
    pub version: Version,
    #[serde(default)]
    pub clips: Vec<Clip>,
    #[serde(default)]
    pub duck: Duck,
    #[serde(default, flatten)]
    pub extra: Extra,
}

impl Soundboard {
    pub fn load() -> Self {
        json::load(FILE)
    }

    pub fn save(&self) -> Result<(), SinkError> {
        json::save(FILE, self)
    }

    pub fn get(&self, id: &str) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    /// Append a clip. The caller has already validated and canonicalised the
    /// path; this only owns the library's shape.
    pub fn add(&mut self, id: String, name: String, path: String) -> Clip {
        let clip = Clip {
            id,
            name,
            path,
            volume_percent: 100,
            extra: Extra::new(),
        };
        self.clips.push(clip.clone());
        clip
    }

    /// Drop a clip. Unknown ids are an error rather than a silent no-op: the
    /// caller is a UI acting on a list it just read, so a miss means the two
    /// disagree and the user should hear about it.
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let before = self.clips.len();
        self.clips.retain(|c| c.id != id);
        if self.clips.len() == before {
            return Err(unknown(id));
        }
        Ok(())
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a clip needs a name".to_string());
        }
        let clip = self.clips.iter_mut().find(|c| c.id == id).ok_or_else(|| unknown(id))?;
        clip.name = name.to_string();
        Ok(())
    }

    pub fn set_volume(&mut self, id: &str, percent: u8) -> Result<(), String> {
        let clip = self.clips.iter_mut().find(|c| c.id == id).ok_or_else(|| unknown(id))?;
        clip.volume_percent = percent.min(MAX_CLIP_VOLUME);
        Ok(())
    }
}

/// One wording for the one thing that can go wrong with an id, so a stale UI
/// gets the same answer from every entry point.
pub fn unknown(id: &str) -> String {
    format!("unknown clip: {id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> Soundboard {
        let mut s = Soundboard::default();
        s.add("a1".into(), "Airhorn".into(), "/clips/airhorn.wav".into());
        s.add("b2".into(), "Sad trombone".into(), "/clips/sad.flac".into());
        s
    }

    #[test]
    fn add_remove_and_rename_move_only_the_named_clip() {
        let mut s = library();
        assert_eq!(s.clips.len(), 2);
        assert_eq!(s.get("a1").map(|c| c.name.as_str()), Some("Airhorn"));

        s.rename("a1", "  Air horn  ").expect("renames");
        assert_eq!(s.get("a1").map(|c| c.name.as_str()), Some("Air horn"));
        assert_eq!(s.get("b2").map(|c| c.name.as_str()), Some("Sad trombone"));

        s.remove("a1").expect("removes");
        assert!(s.get("a1").is_none());
        assert_eq!(s.clips.len(), 1, "the other clip is untouched");
    }

    #[test]
    fn a_stale_id_is_an_error_rather_than_a_silent_no_op() {
        let mut s = library();
        assert!(s.remove("gone").is_err());
        assert!(s.rename("gone", "x").is_err());
        assert!(s.set_volume("gone", 50).is_err());
        // An empty name would leave an unclickable blank button behind.
        assert!(s.rename("a1", "   ").is_err());
        assert_eq!(s.clips.len(), 2);
    }

    #[test]
    fn clip_volume_is_capped_at_the_boost_ceiling() {
        let mut s = library();
        s.set_volume("a1", 255).expect("sets");
        assert_eq!(s.get("a1").expect("clip").volume_percent, MAX_CLIP_VOLUME);
        s.set_volume("a1", 40).expect("sets");
        assert_eq!(s.get("a1").expect("clip").volume_percent, 40);
    }

    #[test]
    fn ducking_is_off_until_asked_for() {
        // Nobody's microphone drops on its own.
        let d = Duck::default();
        assert!(!d.enabled);
        assert_eq!(d.factor(), 1.0);
    }

    #[test]
    fn the_duck_factor_matches_the_decibels() {
        let d = Duck { enabled: true, attenuation_db: -6.0 };
        assert!((d.factor() - 0.5011872).abs() < 1e-4, "{}", d.factor());
        let d = Duck { enabled: true, attenuation_db: -20.0 };
        assert!((d.factor() - 0.1).abs() < 1e-5, "{}", d.factor());
        // Out-of-range settings are clamped, not honoured: a positive dB
        // value would *raise* the mic while a clip plays.
        let d = Duck { enabled: true, attenuation_db: 12.0 };
        assert_eq!(d.factor(), 1.0);
        let d = Duck { enabled: true, attenuation_db: -300.0 };
        assert!(d.factor() < 0.02);
    }

    #[test]
    fn a_file_from_a_newer_inari_round_trips_without_losing_fields() {
        let raw = r#"{"version":9,"clips":[
            {"id":"a1","name":"Airhorn","path":"/clips/a.wav","volume_percent":80,"color":"red"}
        ],"duck":{"enabled":true,"attenuation_db":-9.0},"pages":["default"]}"#;
        let s = json::parse_or_default::<Soundboard>("soundboard.json", raw);
        assert_eq!(s.get("a1").expect("clip").volume_percent, 80);
        assert!(s.duck.enabled);

        let back: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&s).expect("serializes")).expect("value");
        assert_eq!(back["version"], serde_json::json!(9));
        assert_eq!(back["pages"], serde_json::json!(["default"]));
        assert_eq!(back["clips"][0]["color"], serde_json::json!("red"));
    }

    #[test]
    fn a_corrupt_file_leaves_an_empty_board_instead_of_taking_the_app_down() {
        assert_eq!(
            json::parse_or_default::<Soundboard>("soundboard.json", "{ truncated"),
            Soundboard::default()
        );
        assert_eq!(
            json::parse_or_default::<Soundboard>("soundboard.json", ""),
            Soundboard::default()
        );
        // A half-written file must not turn ducking on by accident either.
        assert!(!json::parse_or_default::<Soundboard>("soundboard.json", "[]").duck.enabled);
    }

    #[test]
    fn the_library_survives_a_round_trip_through_the_config_file() {
        // Same atomic write and same parse path the app uses, just at a
        // temp location so the test cannot touch a real config.
        let dir = std::env::temp_dir().join(format!(
            "sink-soundboard-{}-{}",
            std::process::id(),
            super::super::unix_now()
        ));
        let path = dir.join(FILE);
        let mut s = library();
        s.set_volume("b2", 60).expect("sets");
        s.duck = Duck { enabled: true, attenuation_db: -15.0 };

        json::save_at(&path, &s).expect("saves");
        let raw = std::fs::read_to_string(&path).expect("reads back");
        assert_eq!(json::parse_or_default::<Soundboard>(FILE, &raw), s);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
