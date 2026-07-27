use serde::{Deserialize, Serialize};

use crate::error::SinkError;
use crate::persistence::json::{self, Extra, Version};

const FILE: &str = "prefs.json";

/// How Sink's devices are labeled in other apps' device lists.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceLabelStyle {
    /// "Game"
    #[default]
    Plain,
    /// "Game (Inari)"
    Suffix,
    /// "Inari · Game"
    Prefix,
}

/// How an over-long OLED notification is animated so all of it stays readable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotifyScroll {
    /// Word-wrap the body and scroll the block vertically (like film credits).
    #[default]
    Vertical,
    /// Ticker: scroll each over-long line horizontally, right to left.
    Horizontal,
}

fn default_notify_secs() -> u64 {
    5
}

/// Something a global hotkey can do. The variant names are the wire ids the
/// Settings UI sends back, so renaming one breaks saved bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyAction {
    /// Mute/unmute the Inari mic chain.
    MicMute,
    /// Mute/unmute one chosen channel (see [`Hotkeys::channel_target`]).
    ChannelMute,
    /// Load the next saved profile, wrapping around.
    CycleProfile,
    /// Show the window, or hide it back to the tray.
    ToggleWindow,
}

impl HotkeyAction {
    /// Every action, in the order Settings lists them.
    pub const ALL: [HotkeyAction; 4] = [
        HotkeyAction::MicMute,
        HotkeyAction::ChannelMute,
        HotkeyAction::CycleProfile,
        HotkeyAction::ToggleWindow,
    ];
}

/// System-wide hotkeys, all unbound out of the box. Inari deliberately ships
/// without defaults: a fresh install that steals Ctrl+Shift+M from whatever
/// else wants it is hostile, and under Wayland the grab is likely to be
/// accepted and then never fire - so the user opts in per action and gets
/// told when the grab failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Hotkeys {
    #[serde(default)]
    pub mic_mute: Option<String>,
    #[serde(default)]
    pub channel_mute: Option<String>,
    /// Sink name the channel-mute binding acts on. None = the first channel,
    /// so the binding still does something after the target is deleted.
    #[serde(default)]
    pub channel_target: Option<String>,
    #[serde(default)]
    pub cycle_profile: Option<String>,
    #[serde(default)]
    pub toggle_window: Option<String>,
}

impl Hotkeys {
    pub fn get(&self, action: HotkeyAction) -> Option<&str> {
        let slot = match action {
            HotkeyAction::MicMute => &self.mic_mute,
            HotkeyAction::ChannelMute => &self.channel_mute,
            HotkeyAction::CycleProfile => &self.cycle_profile,
            HotkeyAction::ToggleWindow => &self.toggle_window,
        };
        slot.as_deref()
    }

    /// Bind an action, or clear it with `None`. An all-whitespace accelerator
    /// counts as clearing - the UI's "not set" and an empty text field must
    /// not persist as two different states.
    pub fn set(&mut self, action: HotkeyAction, accelerator: Option<String>) {
        let value = accelerator.filter(|a| !a.trim().is_empty());
        let slot = match action {
            HotkeyAction::MicMute => &mut self.mic_mute,
            HotkeyAction::ChannelMute => &mut self.channel_mute,
            HotkeyAction::CycleProfile => &mut self.cycle_profile,
            HotkeyAction::ToggleWindow => &mut self.toggle_window,
        };
        *slot = value;
    }

    /// The bound actions only, in [`HotkeyAction::ALL`] order.
    pub fn bound(&self) -> Vec<(HotkeyAction, &str)> {
        HotkeyAction::ALL
            .into_iter()
            .filter_map(|a| self.get(a).map(|accel| (a, accel)))
            .collect()
    }
}

/// Inari Remote: the LAN server that lets a tablet drive the mixer by touch.
///
/// Off out of the box, and the bind address is a decision the user makes
/// rather than one we guess for them: the default is loopback, which serves
/// nothing to the network until someone deliberately widens it. Opening a
/// mixer to a Wi-Fi network must be an act, not a default.
///
/// The token is deliberately *not* here. `prefs.json` is handed to the webview
/// wholesale by `get_prefs`, and the remote secret has no reason to travel
/// there; it lives in its own 0600 file (see `remote::auth`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteConfig {
    #[serde(default)]
    pub enabled: bool,
    /// The address to listen on. `127.0.0.1` reaches only this machine;
    /// `0.0.0.0` reaches everyone on the network.
    #[serde(default = "default_remote_address")]
    pub address: String,
    #[serde(default = "default_remote_port")]
    pub port: u16,
}

fn default_remote_address() -> String {
    "127.0.0.1".to_string()
}

/// Unregistered, and clear of the ports a desktop is likely to already be
/// running something on.
fn default_remote_port() -> u16 {
    7684
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            address: default_remote_address(),
            port: default_remote_port(),
        }
    }
}

/// App preferences, stored at `$XDG_CONFIG_HOME/inari/prefs.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub version: Version,
    #[serde(default)]
    pub device_label_style: DeviceLabelStyle,
    /// First-run tutorial completed (false = show it on launch).
    #[serde(default)]
    pub onboarded: bool,
    /// ChatMix-style balance: the two channel sink names being balanced
    /// (None = auto: Game/Chat when present, else the first two channels).
    #[serde(default)]
    pub balance_a: Option<String>,
    #[serde(default)]
    pub balance_b: Option<String>,
    /// Show the balance slider in the title bar.
    #[serde(default = "default_true")]
    pub show_balance: bool,
    /// When autostarting on login, boot straight to the tray instead of
    /// showing the window (only meaningful with autostart enabled).
    #[serde(default)]
    pub start_minimized: bool,
    /// Seconds a mirrored desktop notification stays on the OLED before the
    /// previous screen returns.
    #[serde(default = "default_notify_secs")]
    pub notify_duration_secs: u64,
    /// How an over-long notification scrolls so all of its text can be read.
    #[serde(default)]
    pub notify_scroll: NotifyScroll,
    /// System-wide hotkeys (empty until the user assigns them).
    #[serde(default)]
    pub hotkeys: Hotkeys,
    /// The tablet remote (off until the user turns it on).
    #[serde(default)]
    pub remote: RemoteConfig,
    /// Preferences a newer Inari added: kept verbatim so downgrading and
    /// changing one setting doesn't wipe all the others.
    #[serde(default, flatten)]
    pub extra: Extra,
}

fn default_true() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            version: Version::default(),
            device_label_style: DeviceLabelStyle::default(),
            onboarded: false,
            balance_a: None,
            balance_b: None,
            show_balance: true,
            start_minimized: false,
            notify_duration_secs: default_notify_secs(),
            notify_scroll: NotifyScroll::default(),
            hotkeys: Hotkeys::default(),
            remote: RemoteConfig::default(),
            extra: Extra::new(),
        }
    }
}

impl Prefs {
    pub fn load() -> Self {
        json::load(FILE)
    }

    pub fn save(&self) -> Result<(), SinkError> {
        json::save(FILE, self)
    }

    /// Decorate a device label per the chosen style (applied at node
    /// creation; stored labels stay raw).
    pub fn decorate(&self, label: &str) -> String {
        match self.device_label_style {
            DeviceLabelStyle::Plain => label.to_string(),
            DeviceLabelStyle::Suffix => format!("{label} (Inari)"),
            DeviceLabelStyle::Prefix => format!("Inari · {label}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `load` does to a file body, minus the filesystem: malformed
    /// input degrades to defaults rather than blocking launch.
    fn parse(raw: &str) -> Prefs {
        json::parse_or_default(FILE, raw)
    }

    #[test]
    fn decorate_styles() {
        let mut p = Prefs::default();
        assert_eq!(p.decorate("Game"), "Game");
        p.device_label_style = DeviceLabelStyle::Suffix;
        assert_eq!(p.decorate("Game"), "Game (Inari)");
        p.device_label_style = DeviceLabelStyle::Prefix;
        assert_eq!(p.decorate("Game"), "Inari · Game");
    }

    #[test]
    fn malformed_prefs_degrade_to_defaults() {
        // Corrupt / partially-written files must never panic or block
        // launch - they fall back to defaults.
        assert_eq!(parse(""), Prefs::default());
        assert_eq!(parse("{not json"), Prefs::default());
        assert_eq!(parse("[]"), Prefs::default());
        assert_eq!(
            parse(r#"{"device_label_style":"bogus_style"}"#),
            Prefs::default()
        );
        // Unknown fields are tolerated; known fields still apply.
        let p = parse(r#"{"device_label_style":"suffix","future_field":1}"#);
        assert_eq!(p.device_label_style, DeviceLabelStyle::Suffix);
    }

    #[test]
    fn newer_prefs_round_trip_instead_of_being_dropped() {
        // The downgrade case: this version loads a newer prefs file, the
        // user toggles one setting, and the autosave must keep the rest.
        let mut p = parse(r#"{"version":9,"onboarded":true,"theme":"tokyo-night"}"#);
        assert!(p.onboarded);
        p.show_balance = false;

        let back: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&p).expect("serializes")).expect("value");
        assert_eq!(back["theme"], serde_json::json!("tokyo-night"));
        assert_eq!(back["version"], serde_json::json!(9), "the file's version stands");
        assert_eq!(back["show_balance"], serde_json::json!(false));
    }

    #[test]
    fn hotkeys_ship_unbound() {
        // Nothing is grabbed until the user asks for it - neither on a fresh
        // install nor when loading a prefs file written before hotkeys existed.
        assert_eq!(Prefs::default().hotkeys, Hotkeys::default());
        assert!(Prefs::default().hotkeys.bound().is_empty());
        assert!(parse(r#"{"onboarded":true}"#).hotkeys.bound().is_empty());
    }

    #[test]
    fn hotkey_bindings_round_trip_through_the_file() {
        let mut p = Prefs::default();
        p.hotkeys.set(HotkeyAction::MicMute, Some("Ctrl+Shift+M".into()));
        p.hotkeys.set(HotkeyAction::ChannelMute, Some("Ctrl+Shift+G".into()));
        p.hotkeys.channel_target = Some("sink_game".into());

        let back = parse(&serde_json::to_string(&p).expect("serializes"));
        assert_eq!(back.hotkeys.get(HotkeyAction::MicMute), Some("Ctrl+Shift+M"));
        assert_eq!(back.hotkeys.get(HotkeyAction::ChannelMute), Some("Ctrl+Shift+G"));
        assert_eq!(back.hotkeys.channel_target.as_deref(), Some("sink_game"));
        assert_eq!(back.hotkeys.get(HotkeyAction::CycleProfile), None);
        assert_eq!(back.hotkeys, p.hotkeys);
        assert_eq!(
            back.hotkeys.bound(),
            vec![
                (HotkeyAction::MicMute, "Ctrl+Shift+M"),
                (HotkeyAction::ChannelMute, "Ctrl+Shift+G"),
            ],
        );
    }

    #[test]
    fn clearing_a_binding_leaves_no_ghost() {
        // The UI clears by sending None or an emptied field; both must land as
        // "unbound", or a blank accelerator would be re-registered on restart.
        let mut h = Hotkeys::default();
        h.set(HotkeyAction::ToggleWindow, Some("Super+I".into()));
        h.set(HotkeyAction::ToggleWindow, None);
        assert_eq!(h.get(HotkeyAction::ToggleWindow), None);
        h.set(HotkeyAction::ToggleWindow, Some("   ".into()));
        assert_eq!(h.get(HotkeyAction::ToggleWindow), None);
        assert_eq!(h, Hotkeys::default());
    }

    #[test]
    fn the_remote_is_off_and_loopback_only_until_someone_says_otherwise() {
        // Both halves matter: a prefs file written before the remote existed
        // must not come back as "enabled", and it must not come back bound to
        // every interface either.
        for prefs in [Prefs::default(), parse(r#"{"onboarded":true}"#)] {
            assert!(!prefs.remote.enabled);
            assert_eq!(prefs.remote.address, "127.0.0.1");
        }
        // A file that only says "enabled" still gets the safe bind address.
        let p = parse(r#"{"remote":{"enabled":true}}"#);
        assert!(p.remote.enabled);
        assert_eq!(p.remote.address, "127.0.0.1");
        assert_eq!(p.remote.port, RemoteConfig::default().port);
    }

    #[test]
    fn the_remote_settings_round_trip_through_the_file() {
        let p = Prefs {
            remote: RemoteConfig {
                enabled: true,
                address: "0.0.0.0".into(),
                port: 9000,
            },
            ..Prefs::default()
        };
        assert_eq!(parse(&serde_json::to_string(&p).expect("serializes")).remote, p.remote);
    }

    #[test]
    fn version_defaults_for_files_written_before_it_existed() {
        assert_eq!(parse(r#"{"onboarded":true}"#).version, Version::default());
        // Fresh prefs carry the same stamp, so Default and disk agree.
        assert_eq!(Prefs::default().version, Version::default());
    }
}
