use std::collections::HashSet;

use crate::audio::types::VirtualSink;
use crate::persistence::aliases::Aliases;
use crate::persistence::assignments::Assignments;
use crate::persistence::channels::Channels;

/// In-memory mixer state: the source of truth for channel volume/mute as
/// set through the UI, plus the persistent app→channel assignments.
#[derive(Debug, Default)]
pub struct MixerState {
    pub channels: Vec<VirtualSink>,
    /// User-defined channel set (persisted to disk).
    pub channel_defs: Channels,
    /// True once `init_virtual_devices` has created the sinks.
    pub initialized: bool,
    /// Saved app→channel assignments (persisted to disk + WirePlumber conf).
    pub assignments: Assignments,
    /// User-chosen display names for discovered apps (persisted to disk).
    pub aliases: Aliases,
    /// Per-channel output device choices (persisted to disk).
    pub outputs: crate::persistence::outputs::ChannelOutputs,
    /// Per-channel parametric EQ configs (persisted to disk).
    pub eq: crate::persistence::eq::ChannelEq,
    /// Mic chain configuration (persisted to disk).
    pub mic: crate::audio::types::MicConfig,
    /// Every app identity ever observed (history + ignore list).
    pub seen: crate::persistence::seen::SeenApps,
    /// Unix seconds of the last `seen` write. The poll only saves on
    /// structural changes, so this drives a slow flush that bounds how stale
    /// on-disk `last_seen` timestamps can get if Inari dies without a clean
    /// quit - the age-based prune trusts them.
    pub seen_saved_at: u64,
    /// Profile changes autosave into this profile (live-bound, not a
    /// snapshot). None = unmanaged state.
    pub active_profile: Option<String>,
    /// Cached trigger device of `active_profile`, so autosave preserves it
    /// without re-reading the profile file on every mutation. Kept in step
    /// whenever the active profile or its trigger changes.
    pub active_trigger: Option<String>,
    /// User-defined mixes (record buses), persisted to disk.
    pub buses: crate::persistence::buses::Buses,
    /// App preferences (device naming etc.), persisted to disk.
    pub prefs: crate::persistence::prefs::Prefs,
    /// Stream indices already considered for auto-routing this session.
    /// Each stream is enforced once, on first sight, so a user moving a
    /// stream elsewhere (here or in pavucontrol) isn't fought every poll.
    pub auto_routed: HashSet<u32>,
}

impl MixerState {
    /// Populate the channel strips from the user's channel definitions.
    ///
    /// The 100%/unmuted here is a placeholder, not a claim about the audio:
    /// `adopt_live_channel_state` replaces it with what the sinks report, and
    /// it only survives for channels the backend can't tell us about.
    pub fn init_defaults(&mut self) {
        self.channels = self
            .channel_defs
            .channels
            .iter()
            .map(|def| VirtualSink {
                name: def.name.clone(),
                label: def.label.clone(),
                icon: def.icon.clone(),
                volume_percent: 100,
                muted: false,
                stream_mix: def.stream_mix,
            })
            .collect();
        self.initialized = true;
    }

    /// Replace the strips' volume/mute with what the sinks are actually doing.
    ///
    /// `read` answers `None` when the backend can't say - unknown sink, no
    /// reading yet, or a fallback backend that doesn't report it - and those
    /// strips keep the `init_defaults` placeholder as a deliberate fallback.
    ///
    /// Read rather than written because the session manager (WirePlumber)
    /// restores a remembered volume per node name when a sink appears and wins
    /// any race against a write from us. A channel restored to 0% used to show
    /// 100% in the UI and be silent with no visible reason; the strips follow
    /// the sinks now, so nothing on screen is a value the backend never gave.
    pub fn adopt_live_channel_state(&mut self, read: impl Fn(&str) -> Option<(u8, bool)>) {
        for channel in &mut self.channels {
            if let Some((volume_percent, muted)) = read(&channel.name) {
                channel.volume_percent = volume_percent;
                channel.muted = muted;
            }
        }
    }

    pub fn channel_mut(&mut self, sink_name: &str) -> Option<&mut VirtualSink> {
        self.channels.iter_mut().find(|c| c.name == sink_name)
    }

    /// Forget history entries the user never acted on and hasn't seen in a
    /// week, so the "not running" list stays about apps they actually use.
    /// Returns true when the history changed and should be saved.
    pub fn prune_stale_apps(&mut self, now: u64) -> bool {
        // Disjoint field borrows: `prune` needs `seen` mutably while the
        // intent test reads the other two.
        let Self {
            seen,
            assignments,
            aliases,
            ..
        } = self;
        seen.prune(
            now,
            crate::persistence::seen::MAX_SEEN_AGE_SECS,
            |prop, value| {
                assignments.sink_for(prop, value).is_some() || aliases.get(prop, value).is_some()
            },
        )
    }

    pub fn reset(&mut self) {
        self.channels.clear();
        self.initialized = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_defaults_creates_four_channels() {
        let mut state = MixerState::default();
        state.init_defaults();
        assert_eq!(state.channels.len(), 4);
        assert!(state.initialized);
        assert_eq!(state.channels[0].name, "sink_game");
        assert_eq!(state.channels[0].label, "Game");
        assert!(state.channels.iter().all(|c| c.volume_percent == 100 && !c.muted));
    }

    #[test]
    fn prune_stale_apps_exempts_assigned_and_aliased() {
        const DAY: u64 = 24 * 60 * 60;
        let now = 100 * DAY;
        let old = now - 30 * DAY;
        let mut state = MixerState::default();
        for value in ["plain", "assigned", "aliased"] {
            state.seen.upsert("application.name", value, value, None, old);
        }
        state
            .assignments
            .set("application.name", "assigned", "sink_game");
        state.aliases.set("application.name", "aliased", "My App");

        assert!(state.prune_stale_apps(now));
        assert!(state.seen.get("application.name", "plain").is_none());
        assert!(state.seen.get("application.name", "assigned").is_some());
        assert!(state.seen.get("application.name", "aliased").is_some());
    }

    /// The regression this exists for: a channel WirePlumber restored to 0%
    /// and muted used to be shown at 100%, unmuted - silent with no visible
    /// reason. The strips must carry what the sink reports.
    #[test]
    fn strips_take_volume_and_mute_from_the_sinks() {
        let mut state = MixerState::default();
        state.init_defaults();
        state.adopt_live_channel_state(|name| match name {
            "sink_music" => Some((0, true)),
            "sink_game" => Some((42, false)),
            _ => None,
        });
        let music = state.channels.iter().find(|c| c.name == "sink_music").expect("music");
        assert_eq!(music.volume_percent, 0);
        assert!(music.muted);
        let game = state.channels.iter().find(|c| c.name == "sink_game").expect("game");
        assert_eq!(game.volume_percent, 42);
        assert!(!game.muted);
    }

    /// Nothing on screen may be a number the backend never gave us. When the
    /// read fails (unknown sink, no reading yet, pactl fallback), the strip
    /// keeps the documented 100%/unmuted fallback and nothing is invented.
    #[test]
    fn unreadable_channels_keep_the_default_instead_of_a_guess() {
        let mut state = MixerState::default();
        state.init_defaults();
        // Pretend a previous adoption left a real reading on one strip, so the
        // test also proves an unreadable channel isn't reset by a later pass.
        state.adopt_live_channel_state(|name| (name == "sink_chat").then_some((30, true)));
        state.adopt_live_channel_state(|_| None);

        let chat = state.channels.iter().find(|c| c.name == "sink_chat").expect("chat");
        assert_eq!((chat.volume_percent, chat.muted), (30, true));
        for channel in state.channels.iter().filter(|c| c.name != "sink_chat") {
            assert_eq!(
                (channel.volume_percent, channel.muted),
                (100, false),
                "{} invented a value",
                channel.name
            );
        }
    }

    #[test]
    fn channel_mut_finds_by_name() {
        let mut state = MixerState::default();
        state.init_defaults();
        let chat = state.channel_mut("sink_chat").expect("chat channel exists");
        chat.volume_percent = 85;
        assert_eq!(state.channels[1].volume_percent, 85);
        assert!(state.channel_mut("sink_nope").is_none());
    }
}
