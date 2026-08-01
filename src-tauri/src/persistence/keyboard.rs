//! Keyboard settings, stored as JSON at `$XDG_CONFIG_HOME/inari/keyboard.json`.
//!
//! The keyboard keeps its own lighting in onboard memory, so nothing here has
//! to be pushed to survive a reboot — but Inari's *host-side* effects (a wave,
//! the audio-reactive mode, a per-key painting) only exist while Inari runs,
//! and those must come back the same after a restart. Hence a store.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::SinkError;
use crate::keyboard::effects::Lighting;
use crate::keyboard::keys::Physical;
use crate::keyboard::oled_modes::KbMode;
use crate::keyboard::protocol::OledWire;
use crate::persistence::json::{self, Extra, Version};

const FILE: &str = "keyboard.json";

/// Everything the user configured for the keyboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyboardConfig {
    #[serde(default)]
    pub version: Version,
    /// Master switch. Off means Inari never writes to the keyboard, and the
    /// board keeps doing whatever SteelSeries GG or its onboard profile says.
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub lighting: Lighting,
    /// Which physical layout to draw. Cannot be read from the device — the
    /// keyboard reports a region byte, not a layout — so it is the user's call.
    #[serde(default)]
    pub physical: Physical,
    /// What to put on the OLED. `None` leaves the keyboard's own screen alone.
    #[serde(default)]
    pub oled_mode: Option<KbMode>,
    /// Lines for [`KbMode::Text`].
    #[serde(default)]
    pub oled_text: Vec<String>,
    /// Pin the OLED transport instead of using the model's default. For a
    /// board that takes a frame and shows noise, this is the fix.
    #[serde(default)]
    pub oled_wire: Option<OledWire>,
    /// Blank the panel after this many idle minutes; 0 keeps it lit.
    ///
    /// The keyboard has a screensaver of its own, but it guards its own screen
    /// and cannot know Inari is driving the panel — while Inari is pushing
    /// frames the firmware's timer never fires. So the screensaver has to live
    /// on this side too, or a clock would sit burnt into the OLED overnight.
    #[serde(default = "default_screensaver")]
    pub oled_screensaver_minutes: u16,
    /// Global actuation in tenths of a millimetre (1 = 0.1 mm, 40 = 4.0 mm).
    /// Verified on hardware: writing 40 to a key makes it trigger visibly
    /// later. `None` leaves whatever the keyboard's own profile holds.
    #[serde(default)]
    pub actuation: Option<u8>,
    /// Per-key actuation overrides, same units.
    #[serde(default)]
    pub per_key_actuation: HashMap<u8, u8>,
    /// Rapid Trigger sensitivity applied to every key; 0 turns it off.
    #[serde(default)]
    pub rapid_trigger: u8,
    /// Protection Mode: dampens the keys around the one you meant to press.
    #[serde(default)]
    pub protection_mode: bool,
    /// Rapid Tap / SOCD master switch.
    #[serde(default)]
    pub rapid_tap: bool,
    /// Seconds of inactivity before the keyboard dims its own lighting;
    /// 0 never dims. This is the firmware's timer — the one that actually
    /// controls the keyboard, unlike Inari's own OLED blanking.
    #[serde(default = "default_idle_secs")]
    pub idle_timeout_secs: u32,
    /// Brightness once dimmed, 0..10.
    #[serde(default)]
    pub idle_brightness: u8,
    /// Minutes before the board sleeps; 0 never sleeps.
    #[serde(default = "default_sleep_minutes")]
    pub sleep_minutes: u32,
    /// Trade lighting for battery life on the wireless models.
    #[serde(default)]
    pub high_efficiency: bool,
    /// Unknown keys from a newer Inari, preserved across a load/save.
    #[serde(default, flatten)]
    pub extra: Extra,
}

fn yes() -> bool {
    true
}

/// Ten minutes, which is roughly what the keyboard's own screensaver does.
fn default_screensaver() -> u16 {
    10
}

/// What the measured board shipped with: 60 s to dim, 5 min to sleep. Using
/// the device's own defaults means a fresh install changes nothing until the
/// user asks it to.
fn default_idle_secs() -> u32 {
    60
}

fn default_sleep_minutes() -> u32 {
    5
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            version: Version::default(),
            enabled: true,
            lighting: Lighting::default(),
            physical: Physical::default(),
            oled_mode: None,
            oled_text: Vec::new(),
            oled_wire: None,
            oled_screensaver_minutes: default_screensaver(),
            actuation: None,
            per_key_actuation: HashMap::new(),
            rapid_trigger: 0,
            protection_mode: false,
            rapid_tap: false,
            idle_timeout_secs: default_idle_secs(),
            idle_brightness: 3,
            sleep_minutes: default_sleep_minutes(),
            high_efficiency: false,
            extra: Extra::new(),
        }
    }
}

impl KeyboardConfig {
    /// Fold out-of-range values from a hand-edited file, so a bad number
    /// cannot reach the device or make the UI render nonsense.
    pub fn clamp_ranges(&mut self) {
        self.lighting.brightness = self.lighting.brightness.min(100);
        self.lighting.speed = self.lighting.speed.clamp(1, 100);
        self.actuation = self.actuation.map(|a| a.clamp(1, 40));
        self.per_key_actuation
            .retain(|_, tenths| (1..=40).contains(tenths));
        self.idle_brightness = self.idle_brightness.min(10);
        // A day of either is well past "did you mean off?".
        self.idle_timeout_secs = self.idle_timeout_secs.min(86_400);
        self.sleep_minutes = self.sleep_minutes.min(1440);
        // Three lines is all the 128x40 panel fits.
        self.oled_text.truncate(3);
        // A day is well past "did you mean off?".
        self.oled_screensaver_minutes = self.oled_screensaver_minutes.min(1440);
    }
}

pub fn load() -> KeyboardConfig {
    let mut config: KeyboardConfig = json::load(FILE);
    config.clamp_ranges();
    config
}

pub fn save(config: &KeyboardConfig) -> Result<(), SinkError> {
    json::save(FILE, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::effects::EffectKind;

    #[test]
    fn a_fresh_config_is_enabled_with_a_static_colour() {
        let config = KeyboardConfig::default();
        assert!(config.enabled);
        assert_eq!(config.lighting.kind, EffectKind::Static);
        assert_eq!(config.physical, Physical::Iso);
        assert_eq!(config.oled_mode, None, "the panel stays the keyboard's");
        assert_eq!(config.oled_screensaver_minutes, 10);
    }

    #[test]
    fn clamping_folds_values_a_hand_edited_file_could_carry() {
        let mut config = KeyboardConfig {
            actuation: Some(200),
            oled_text: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            ..KeyboardConfig::default()
        };
        config.lighting.brightness = 250;
        config.lighting.speed = 0;
        config.per_key_actuation.insert(0x04, 99);
        config.per_key_actuation.insert(0x05, 12);
        config.clamp_ranges();

        assert_eq!(config.lighting.brightness, 100);
        assert_eq!(config.lighting.speed, 1);
        assert_eq!(config.actuation, Some(40));
        assert_eq!(config.oled_text.len(), 3);
        assert_eq!(config.oled_screensaver_minutes, 10);
        assert_eq!(config.per_key_actuation.get(&0x04), None, "99 is off-scale");
        assert_eq!(config.per_key_actuation.get(&0x05), Some(&12));
    }

    #[test]
    fn a_stored_config_round_trips_through_json() {
        let mut config = KeyboardConfig::default();
        config.lighting.kind = EffectKind::PerKey;
        config.lighting.per_key.insert(0x29, [1, 2, 3]);
        config.oled_mode = Some(KbMode::Clock);
        config.oled_wire = Some(OledWire::Chunked {
            cmd: 0x0c,
            page: false,
        });
        let raw = serde_json::to_string(&config).unwrap();
        let back: KeyboardConfig = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn an_empty_or_broken_file_degrades_to_the_default() {
        let fresh: KeyboardConfig = json::parse_or_default(FILE, "{}");
        assert!(
            fresh.enabled,
            "a missing flag must not disable the keyboard"
        );
        let broken: KeyboardConfig = json::parse_or_default(FILE, "not json");
        assert_eq!(broken, KeyboardConfig::default());
    }
}
