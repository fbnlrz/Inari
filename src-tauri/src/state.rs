use std::sync::{Arc, Mutex};

use log::warn;

use crate::audio::backend::AudioBackend;
use crate::headset::HeadsetManager;
use crate::keyboard::KeyboardManager;
use crate::mixer::state::MixerState;
use crate::mouse::MouseManager;

/// Application state managed by Tauri and shared across commands and the tray.
pub struct AppState {
    pub backend: Arc<dyn AudioBackend>,
    /// True when the native PipeWire backend is driving (vs pactl fallback).
    pub backend_native: bool,
    pub mixer: Mutex<MixerState>,
    /// Arctis Nova Pro Wireless base station (HID control + OLED).
    pub headset: Arc<HeadsetManager>,
    /// SteelSeries mouse (Aerox 9 Wireless).
    pub mouse: Arc<MouseManager>,
    /// SteelSeries Apex keyboard (per-key RGB, OLED).
    pub keyboard: Arc<KeyboardManager>,
}

impl AppState {
    /// Lock the mixer state, mapping poisoning to a command-friendly error.
    /// All command handlers go through this instead of hand-rolled map_errs.
    pub fn lock_mixer(&self) -> Result<std::sync::MutexGuard<'_, MixerState>, String> {
        self.mixer
            .lock()
            .map_err(|_| "mixer state lock poisoned".to_string())
    }

    /// Pull each channel strip's volume and mute from the sink itself.
    ///
    /// `mixer.channels` is a cache, and three different things read it: the
    /// window (`get_virtual_devices`), the tray's mute rows, and `inari
    /// status`. It used to be filled once during `init_virtual_devices` and
    /// never refreshed, so anything that changed a level outside Inari —
    /// `pavucontrol`, `wpctl`, WirePlumber restoring a remembered level when a
    /// device reappears — left all three reporting a number the sink had
    /// stopped using. Refreshing in only one of the three readers would just
    /// move the problem: whether the tray told the truth would depend on
    /// whether the window happened to be open.
    ///
    /// `sink_state` is an in-memory read of what the loop thread already
    /// mirrors, so this is a channel round trip and no server traffic. A
    /// backend that cannot answer leaves the cached value alone, as at init.
    pub fn refresh_channel_state(&self) {
        let names: Vec<String> = match self.mixer.lock() {
            Ok(mixer) => mixer.channels.iter().map(|c| c.name.clone()).collect(),
            Err(_) => return,
        };
        // Read outside the lock: holding it across a backend call would stall
        // every other command.
        let live: Vec<(String, Option<(u8, bool)>)> = names
            .into_iter()
            .map(|name| {
                let state = self.backend.sink_state(&name).unwrap_or(None);
                (name, state)
            })
            .collect();
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.adopt_live_channel_state(|name| {
                live.iter().find(|(n, _)| n == name).and_then(|(_, s)| *s)
            });
        }
    }

    pub fn new(backend: Arc<dyn AudioBackend>, backend_native: bool) -> Self {
        // Saved assignments are loaded eagerly so auto-routing can enforce
        // them as soon as the sinks exist.
        let channel_defs = crate::persistence::channels::Channels::load();
        let buses = crate::persistence::buses::Buses::load(&channel_defs);
        let active_profile = crate::persistence::active::load();
        // Cache the active profile's trigger once so autosave never has to
        // re-read the profile file to preserve it.
        let active_trigger = active_profile
            .as_deref()
            .and_then(|name| crate::persistence::profiles::load(name).ok())
            .and_then(|p| p.trigger_device);
        let now = crate::persistence::unix_now();
        let mut mixer = MixerState {
            assignments: crate::persistence::assignments::Assignments::load(),
            aliases: crate::persistence::aliases::Aliases::load(),
            outputs: crate::persistence::outputs::ChannelOutputs::load(),
            eq: crate::persistence::eq::ChannelEq::load(),
            mic: crate::persistence::mic::load(),
            channel_defs,
            buses,
            seen: crate::persistence::seen::SeenApps::load(),
            active_profile,
            active_trigger,
            prefs: crate::persistence::prefs::Prefs::load(),
            seen_saved_at: now,
            ..MixerState::default()
        };
        if mixer.prune_stale_apps(now) {
            if let Err(e) = mixer.seen.save() {
                warn!("pruning app history failed: {e}");
            }
        }
        Self {
            backend,
            backend_native,
            mixer: Mutex::new(mixer),
            headset: HeadsetManager::new(),
            mouse: MouseManager::new(),
            keyboard: KeyboardManager::new(),
        }
    }

    /// Best-effort teardown of all virtual sinks. Collects error messages
    /// instead of aborting on the first failure so a single bad unload
    /// doesn't leave the remaining sinks behind.
    pub fn teardown_virtual_sinks(&self) -> Vec<String> {
        let names: Vec<String> = self
            .mixer
            .lock()
            .map(|m| m.channel_defs.channels.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        let mut errors = Vec::new();
        for name in names {
            if let Err(e) = self.backend.destroy_virtual_sink(&name) {
                errors.push(format!("{name}: {e}"));
            }
        }
        if let Ok(mut mixer) = self.mixer.lock() {
            // Persist freshest last-seen timestamps on the way out (the
            // poll only writes on structural changes).
            let _ = mixer.seen.save();
            mixer.reset();
        }
        errors
    }
}
