//! The soundboard: a library of short clips the user can fire, heard by the
//! chat and by themselves at the same time.
//!
//! **At most one clip plays at a time.** Pressing a button while its own clip
//! is running stops it; pressing a different one takes over. That decision has
//! to be made here rather than in the UI: "stop, then play" as two requests is
//! a race the moment the presses come from the tablet over Wi-Fi - the stop of
//! the first clip can arrive after the start of the second and kill it on the
//! spot. [`SoundboardManager::toggle`] makes the whole decision in one place,
//! where the state is known.
//!
//! Two more things this module is careful about:
//!
//! - **A clip is fired by id, never by path.** The library owns the paths; a
//!   play request carries only an id, so the same command is safe whether it
//!   comes from the window, a global hotkey or the tablet remote. See
//!   `persistence/soundboard.rs`.
//! - **Ducking is momentary, and continuous.** While a clip plays the
//!   microphone may be attenuated, but the user's own gain setting is never
//!   written to. Across a takeover the attenuation is *left alone* - one clip
//!   ending and the next starting must not let the mic jump back up and drop
//!   again, which is plainly audible.
//!
//! The audio itself is published by the PipeWire backend (see
//! `audio/pw_native/clip.rs` for why a clip goes straight into `sink_mic`
//! rather than through the mic's DSP chain).

pub mod decode;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use log::warn;
use serde::Serialize;

use crate::audio::backend::AudioBackend;
use crate::audio::types::{ClipPcm, ClipTargets};
use crate::persistence::soundboard::{self, Clip, Duck, Soundboard as Library, MAX_CLIP_VOLUME};

/// Grace period between a clip's last sample and tearing its stream down.
/// The stream feeds silence in the meantime; cutting it at the exact sample
/// would clip the tail that is still sitting in the graph's buffers.
const REAP_GRACE: Duration = Duration::from_millis(300);

/// One clip as the UI sees it. The path is deliberately absent: a client that
/// never learns a path cannot ask for one back.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipInfo {
    pub id: String,
    pub name: String,
    pub volume_percent: u8,
    /// How this clip decodes - `native` needs nothing, `ffmpeg` needs the
    /// system tool.
    pub format: decode::ClipFormat,
    /// The file behind it is gone (deleted, unmounted, renamed). The clip
    /// stays in the library - the user may well be about to plug the drive
    /// back in - but the UI can show it as unavailable instead of offering a
    /// button that only produces an error.
    pub missing: bool,
    /// False when playing it would fail right now: the file is gone, or it
    /// needs an ffmpeg this machine does not have.
    pub playable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SoundboardStatus {
    /// How many clips are playing right now (0 or 1 today).
    pub playing: u32,
    /// Which clips those are, so the UI can light up the button that is
    /// running. A list even though playback is exclusive: if an overlap mode
    /// ever arrives, it fills up instead of breaking the contract again.
    pub playing_ids: Vec<String>,
    /// Whether compressed formats (mp3, ogg, m4a, …) can be played at all.
    pub ffmpeg: bool,
    /// The ducking setting, so the UI shows one truth.
    pub duck: Duck,
    /// False on the pactl fallback, where clips cannot be published at all.
    pub available: bool,
}

/// What a press of a button means right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Press {
    /// This very clip is playing: the press stops it.
    Stop,
    /// Nothing, or something else, is playing: the press starts this one.
    Start,
}

/// What has to happen to the microphone as a result of a bookkeeping change.
/// Returned rather than done, so the rules are decided in one testable place
/// and the backend call happens outside the lock.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DuckAction {
    /// Leave the mic exactly as it is. This is what a takeover returns while
    /// ducking is already applied - releasing and re-attenuating around the
    /// swap is what a listener would hear as a jump.
    Leave,
    /// Attenuate by this linear factor.
    Apply(f32),
    /// Back to the user's own level.
    Release,
}

/// What is playing, and which run of playback it belongs to.
///
/// The generation exists because a takeover (or a stop) and a clip's own end
/// timer race: without it, the timer of a clip that was superseded would tear
/// down bookkeeping that by then belongs to the *newer* clip and release
/// ducking while that one is still playing.
#[derive(Debug, Default, PartialEq)]
struct Active {
    /// playback id -> the library clip it is playing.
    running: std::collections::HashMap<u64, String>,
    generation: u64,
    /// Whether the mic is currently attenuated by us. Tracked rather than
    /// inferred from `running`, because a takeover empties and refills that
    /// map while the attenuation has to stay put.
    ducked: bool,
}

impl Active {
    fn is_playing(&self, clip_id: &str) -> bool {
        self.running.values().any(|id| id == clip_id)
    }

    /// What pressing `clip_id` means. Read under the same lock as the change
    /// it leads to, so two presses cannot both decide "start".
    fn press(&self, clip_id: &str) -> Press {
        if self.is_playing(clip_id) {
            Press::Stop
        } else {
            Press::Start
        }
    }

    /// The clips playing right now, for the status.
    fn playing_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.running.values().cloned().collect();
        // Stable order: a status poll must not reshuffle the UI.
        ids.sort_unstable();
        ids
    }

    /// This clip takes over: whatever ran before is abandoned (the caller
    /// stops those streams) and this one becomes the only one. Returns its
    /// generation and what to do with the mic - `Leave` whenever the previous
    /// clip already had it ducked, which is what keeps the attenuation
    /// continuous across the swap.
    fn take_over(&mut self, playback: u64, clip_id: String, duck: Duck) -> (u64, DuckAction) {
        self.generation = self.generation.wrapping_add(1);
        self.running.clear();
        self.running.insert(playback, clip_id);
        (self.generation, self.aim_duck(duck.enabled, duck.factor()))
    }

    /// Register a clip ending. Stale timers (a generation that has moved on)
    /// change nothing at all.
    fn finish(&mut self, playback: u64, generation: u64) -> DuckAction {
        if generation != self.generation {
            return DuckAction::Leave;
        }
        self.running.remove(&playback);
        let still_playing = !self.running.is_empty();
        self.aim_duck(still_playing && self.ducked, 1.0)
    }

    /// Stop everything.
    fn clear(&mut self) -> DuckAction {
        self.generation = self.generation.wrapping_add(1);
        self.running.clear();
        self.aim_duck(false, 1.0)
    }

    /// Move the ducking state to `wanted`, reporting only a real change - the
    /// whole point of `Leave` is that a takeover writes nothing.
    fn aim_duck(&mut self, wanted: bool, factor: f32) -> DuckAction {
        if wanted == self.ducked {
            return DuckAction::Leave;
        }
        self.ducked = wanted;
        if wanted {
            DuckAction::Apply(factor)
        } else {
            DuckAction::Release
        }
    }
}

pub struct SoundboardManager {
    library: Mutex<Library>,
    active: Mutex<Active>,
    /// Held for the whole "stop what is running, start this" sequence. Two
    /// presses arriving together would otherwise interleave their stop and
    /// their start - the second clip started, then the first one's stop
    /// sweeping it away - which is exactly the race the UI cannot fix from
    /// its side either.
    switch: Mutex<()>,
    /// Playback ids. Unrelated to clip ids: the same clip fired twice is two
    /// playbacks, each with its own stream to stop.
    next_playback: std::sync::atomic::AtomicU64,
}

impl SoundboardManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            library: Mutex::new(Library::load()),
            active: Mutex::new(Active::default()),
            switch: Mutex::new(()),
            next_playback: std::sync::atomic::AtomicU64::new(1),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Library>, String> {
        self.library
            .lock()
            .map_err(|_| "soundboard library lock poisoned".to_string())
    }

    /// Mutate the library and persist it in one step, so no caller can change
    /// it and forget to save.
    fn edit<T>(&self, f: impl FnOnce(&mut Library) -> Result<T, String>) -> Result<T, String> {
        let mut library = self.lock()?;
        let out = f(&mut library)?;
        library.save().map_err(|e| e.to_string())?;
        Ok(out)
    }

    /// The library as the UI shows it, with each clip's current playability
    /// resolved (the file may have vanished since it was added).
    pub fn clips(&self) -> Result<Vec<ClipInfo>, String> {
        let ffmpeg = decode::ffmpeg_available();
        Ok(self.lock()?.clips.iter().map(|c| info(c, ffmpeg)).collect())
    }

    /// Add a clip from a path. This is the one entry point that takes a path,
    /// and it is a desktop action behind a file dialog - hence its absence
    /// from the remote allowlist.
    pub fn add(&self, path: &str) -> Result<ClipInfo, String> {
        let resolved = decode::resolve(path)?;
        if decode::classify(&resolved).is_none() {
            return Err(format!(
                "{} is not an audio format Inari can play",
                resolved.display()
            ));
        }
        // Stored as text, so the JSON round-trip cannot mangle it. A path that
        // is not UTF-8 would come back as something else, pointing at a
        // different file - refuse it instead.
        let stored = resolved
            .to_str()
            .ok_or_else(|| "that path is not valid UTF-8".to_string())?
            .to_string();
        let name = resolved
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "Clip".to_string());
        let id = new_id();
        let ffmpeg = decode::ffmpeg_available();
        let clip = self.edit(|library| Ok(library.add(id, name, stored)))?;
        Ok(info(&clip, ffmpeg))
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        self.edit(|library| library.remove(id))
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<(), String> {
        self.edit(|library| library.rename(id, name))
    }

    pub fn set_volume(&self, id: &str, percent: u8) -> Result<(), String> {
        self.edit(|library| library.set_volume(id, percent.min(MAX_CLIP_VOLUME)))
    }

    pub fn duck(&self) -> Result<Duck, String> {
        Ok(self.lock()?.duck)
    }

    /// Change the ducking setting. It takes effect on the next clip; a duck
    /// that is already applied is not re-levelled mid-clip, which would be a
    /// gain change in the middle of what the chat is hearing.
    pub fn set_duck(&self, enabled: bool, attenuation_db: f32) -> Result<Duck, String> {
        if !attenuation_db.is_finite() {
            return Err("that attenuation is not a number".to_string());
        }
        let duck = Duck {
            enabled,
            attenuation_db: attenuation_db
                .clamp(soundboard::MIN_DUCK_DB, soundboard::MAX_DUCK_DB),
        };
        self.edit(|library| {
            library.duck = duck;
            Ok(duck)
        })
    }

    pub fn status(&self, backend: &dyn AudioBackend) -> Result<SoundboardStatus, String> {
        let playing_ids = self
            .active
            .lock()
            .map(|a| a.playing_ids())
            .map_err(|_| "soundboard state lock poisoned".to_string())?;
        Ok(SoundboardStatus {
            playing: playing_ids.len() as u32,
            playing_ids,
            ffmpeg: decode::ffmpeg_available(),
            duck: self.duck()?,
            available: backend.play_clip_supported(),
        })
    }

    /// Press a button: stop this clip if it is the one playing, otherwise
    /// start it (taking over from whatever was). Returns whether the clip is
    /// playing now, which is what a button needs to redraw itself.
    ///
    /// The decision is made here, under one lock, because the alternative -
    /// the UI asking "what is playing", then sending stop or play - is a race
    /// over the network, and it is the *press* that is atomic, not the two
    /// requests it would decompose into.
    pub fn toggle(
        self: &Arc<Self>,
        backend: &Arc<dyn AudioBackend>,
        id: &str,
        targets: ClipTargets,
    ) -> Result<bool, String> {
        // Serialised against other presses for the whole sequence: the state
        // decision and the backend calls it implies belong together.
        let _switch = self
            .switch
            .lock()
            .map_err(|_| "soundboard switch lock poisoned".to_string())?;
        // An unknown id is an error even when the answer would be "stop":
        // a stale button must say so rather than quietly do nothing.
        let clip = self.clip(id)?;
        let press = self
            .active
            .lock()
            .map(|a| a.press(id))
            .map_err(|_| "soundboard state lock poisoned".to_string())?;
        match press {
            Press::Stop => {
                self.stop_everything(backend.as_ref())?;
                Ok(false)
            }
            Press::Start => {
                self.start(backend, &clip, targets)?;
                Ok(true)
            }
        }
    }

    /// Fire a clip, taking over from whatever was playing. Decoding happens
    /// here, on the calling thread, so a broken file is an error the user sees
    /// rather than silence.
    ///
    /// Pressing the same button again does *not* restart the clip - use
    /// [`Self::toggle`] for that, which is what the buttons are wired to.
    pub fn play(
        self: &Arc<Self>,
        backend: &Arc<dyn AudioBackend>,
        id: &str,
        targets: ClipTargets,
    ) -> Result<(), String> {
        let _switch = self
            .switch
            .lock()
            .map_err(|_| "soundboard switch lock poisoned".to_string())?;
        let clip = self.clip(id)?;
        self.start(backend, &clip, targets)
    }

    fn clip(&self, id: &str) -> Result<Clip, String> {
        self.lock()?
            .get(id)
            .cloned()
            .ok_or_else(|| soundboard::unknown(id))
    }

    /// The start half of a press. The caller holds `switch`.
    fn start(
        self: &Arc<Self>,
        backend: &Arc<dyn AudioBackend>,
        clip: &Clip,
        targets: ClipTargets,
    ) -> Result<(), String> {
        // Decoded before anything is torn down: a clip whose file has gone
        // missing must leave the one that is playing alone.
        let pcm = decode::decode(std::path::Path::new(&clip.path))
            .map_err(|e| format!("{}: {e}", clip.name))?;
        let duck = self.lock()?.duck;

        let playback = self
            .next_playback
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let duration = pcm.duration();
        let request = ClipPcm {
            id: playback,
            samples: Arc::new(pcm.samples),
            rate: pcm.rate,
            channels: pcm.channels,
            gain: f32::from(clip.volume_percent) / 100.0,
            targets,
        };

        // Bookkeeping first, then the mic, then the audio. Ducking goes in
        // before the clip does - the ramp and the stream's own startup are
        // both a few dozen milliseconds - and across a takeover this reports
        // `Leave`, so the attenuation simply continues.
        let (generation, duck_action) = {
            let mut active = self
                .active
                .lock()
                .map_err(|_| "soundboard state lock poisoned".to_string())?;
            active.take_over(playback, clip.id.clone(), duck)
        };
        self.apply_duck(backend.as_ref(), duck_action);

        // Exclusive: whatever was playing goes now. Ordered before the new
        // stream so a takeover cannot stop the clip it just started.
        if let Err(e) = backend.stop_all_clips() {
            warn!("stopping the previous clip failed: {e}");
        }
        if let Err(e) = backend.play_clip(request) {
            // Undo the bookkeeping, or nothing would ever clear it and the
            // mic would stay ducked for good.
            self.finish(backend.as_ref(), playback, generation);
            return Err(e.to_string());
        }

        // The clip's own end: it is a finite buffer, so its length is known
        // exactly and no polling is needed.
        let manager = Arc::clone(self);
        let backend = Arc::clone(backend);
        std::thread::spawn(move || {
            std::thread::sleep(duration + REAP_GRACE);
            if let Err(e) = backend.stop_clip(playback) {
                warn!("reaping a finished clip failed: {e}");
            }
            manager.finish(&*backend, playback, generation);
        });
        Ok(())
    }

    /// One clip ended. Ducking is released only when nothing is left - and a
    /// timer from a run that has since been taken over changes nothing at all.
    fn finish(&self, backend: &dyn AudioBackend, playback: u64, generation: u64) {
        let action = self
            .active
            .lock()
            .map(|mut a| a.finish(playback, generation))
            .unwrap_or(DuckAction::Leave);
        self.apply_duck(backend, action);
    }

    /// Stop everything now, and let the microphone back up.
    pub fn stop_all(&self, backend: &dyn AudioBackend) -> Result<(), String> {
        let _switch = self
            .switch
            .lock()
            .map_err(|_| "soundboard switch lock poisoned".to_string())?;
        self.stop_everything(backend)
    }

    /// The stop half of a press, and the whole of `stop_all`. The caller holds
    /// `switch`.
    fn stop_everything(&self, backend: &dyn AudioBackend) -> Result<(), String> {
        let action = self
            .active
            .lock()
            .map(|mut a| a.clear())
            .map_err(|_| "soundboard state lock poisoned".to_string())?;
        let stopped = backend.stop_all_clips();
        // Released even if the stop failed: leaving the mic attenuated with
        // nothing left to release it is the worse of the two outcomes.
        self.apply_duck(backend, action);
        stopped.map_err(|e| e.to_string())
    }

    /// Carry out what the bookkeeping decided. `Leave` is the common case on
    /// a takeover and must stay a no-op: writing the same attenuation again
    /// would be harmless, but writing 1.0 in between is what people hear.
    fn apply_duck(&self, backend: &dyn AudioBackend, action: DuckAction) {
        let factor = match action {
            DuckAction::Leave => return,
            DuckAction::Apply(factor) => factor,
            DuckAction::Release => 1.0,
        };
        if let Err(e) = backend.set_mic_duck(factor) {
            warn!("mic ducking failed: {e}");
        }
    }
}

/// A clip's id: random, because it must not be derived from the file name.
/// An id that spelled out a path would put one back into every play request,
/// which is the thing this design exists to avoid.
fn new_id() -> String {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        // The OS CSPRNG is not something that fails in practice; if it does,
        // a counter still gives a unique id within this library.
        let n = crate::persistence::unix_now();
        bytes.copy_from_slice(&n.to_le_bytes());
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A stored clip plus what this machine can currently do with it.
fn info(clip: &Clip, ffmpeg: bool) -> ClipInfo {
    let path = std::path::Path::new(&clip.path);
    let format = decode::classify(path).unwrap_or(decode::ClipFormat::Ffmpeg);
    let missing = !path.exists();
    ClipInfo {
        id: clip.id.clone(),
        name: clip.name.clone(),
        volume_percent: clip.volume_percent,
        format,
        missing,
        playable: !missing && (format == decode::ClipFormat::Native || ffmpeg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFF: Duck = Duck { enabled: false, attenuation_db: -12.0 };
    const ON: Duck = Duck { enabled: true, attenuation_db: -12.0 };

    #[test]
    fn pressing_the_playing_clip_again_stops_it() {
        // The whole point of the toggle: the second press is a stop, not a
        // second copy of the same clip.
        let mut active = Active::default();
        assert_eq!(active.press("airhorn"), Press::Start);
        active.take_over(1, "airhorn".into(), OFF);
        assert_eq!(active.press("airhorn"), Press::Stop);

        active.clear();
        assert_eq!(active.press("airhorn"), Press::Start, "and it can start again");
        assert!(active.playing_ids().is_empty());
    }

    #[test]
    fn another_clip_takes_over_and_leaves_exactly_one_playing() {
        let mut active = Active::default();
        active.take_over(1, "airhorn".into(), OFF);
        // A different button while the first is running: that one starts…
        assert_eq!(active.press("trombone"), Press::Start);
        active.take_over(2, "trombone".into(), OFF);
        // …and it is the only thing playing.
        assert_eq!(active.playing_ids(), vec!["trombone".to_string()]);
        assert_eq!(active.press("airhorn"), Press::Start, "the first one is gone");
        assert_eq!(active.press("trombone"), Press::Stop);
    }

    #[test]
    fn ducking_stays_down_across_a_takeover() {
        // One clip ends and the next begins within a few milliseconds. The
        // mic must not pop back up in between - that is audible.
        let mut active = Active::default();
        let (_, first) = active.take_over(1, "airhorn".into(), ON);
        assert_eq!(first, DuckAction::Apply(ON.factor()));

        let (gen2, swap) = active.take_over(2, "trombone".into(), ON);
        assert_eq!(swap, DuckAction::Leave, "the mic is left where it is");

        // Only when the second clip really ends does it come back up.
        assert_eq!(active.finish(2, gen2), DuckAction::Release);
        assert!(!active.ducked);
    }

    #[test]
    fn a_superseded_clips_timer_cannot_unduck_the_one_that_replaced_it() {
        // The first clip's reap timer fires while the second is playing.
        let mut active = Active::default();
        let (gen1, _) = active.take_over(1, "airhorn".into(), ON);
        let (gen2, _) = active.take_over(2, "trombone".into(), ON);

        assert_eq!(active.finish(1, gen1), DuckAction::Leave, "stale timer");
        assert!(active.ducked, "the mic stays down for the playing clip");
        assert_eq!(active.playing_ids(), vec!["trombone".to_string()]);
        assert_eq!(active.finish(2, gen2), DuckAction::Release);
    }

    #[test]
    fn stopping_an_idle_board_does_not_touch_the_microphone() {
        let mut active = Active::default();
        assert_eq!(active.clear(), DuckAction::Leave, "nothing to release");
        // …and stopping an un-ducked clip does not write to the mic either.
        active.take_over(1, "airhorn".into(), OFF);
        assert_eq!(active.clear(), DuckAction::Leave);
    }

    #[test]
    fn a_double_reap_is_harmless() {
        let mut active = Active::default();
        let (gen, _) = active.take_over(1, "airhorn".into(), ON);
        assert_eq!(active.finish(1, gen), DuckAction::Release);
        // The same timer firing twice (or a hand-stop plus its timer) must
        // not release ducking a second time, over a clip that has since
        // started.
        assert_eq!(active.finish(1, gen), DuckAction::Leave);
        assert!(active.playing_ids().is_empty());
    }

    #[test]
    fn turning_ducking_off_between_clips_lets_the_mic_back_up() {
        // The setting changed while a clip was running; the next takeover is
        // where it takes effect.
        let mut active = Active::default();
        active.take_over(1, "airhorn".into(), ON);
        let (_, action) = active.take_over(2, "trombone".into(), OFF);
        assert_eq!(action, DuckAction::Release);
        assert!(!active.ducked);
    }

    #[test]
    fn the_status_names_what_is_playing() {
        // The UI highlights a button by id, so the id has to be in the status.
        let mut active = Active::default();
        assert!(active.playing_ids().is_empty());
        active.take_over(1, "airhorn".into(), OFF);
        assert_eq!(active.playing_ids(), vec!["airhorn".to_string()]);
    }

    #[test]
    fn ids_are_unique_and_carry_nothing_from_the_file() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_clip_whose_file_vanished_is_listed_but_not_playable() {
        let clip = Clip {
            id: "a1".into(),
            name: "Airhorn".into(),
            path: "/nonexistent/airhorn.wav".into(),
            volume_percent: 100,
            extra: Default::default(),
        };
        let shown = info(&clip, true);
        assert!(shown.missing);
        assert!(!shown.playable, "the button must not pretend it works");
        // …and it is still in the list: the drive may come back.
        assert_eq!(shown.name, "Airhorn");
    }

    #[test]
    fn without_ffmpeg_only_the_compressed_clips_go_dark() {
        // Both files exist, so the difference between them is the format and
        // nothing else - which is exactly what the status is meant to explain.
        let dir = std::env::temp_dir().join(format!(
            "sink-soundboard-info-{}-{}",
            std::process::id(),
            crate::persistence::unix_now()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let wav = dir.join("horn.wav");
        let mp3 = dir.join("horn.mp3");
        std::fs::write(&wav, b"").expect("wav");
        std::fs::write(&mp3, b"").expect("mp3");

        let clip = |path: &std::path::Path| Clip {
            id: "a1".into(),
            name: "Horn".into(),
            path: path.to_string_lossy().to_string(),
            volume_percent: 100,
            extra: Default::default(),
        };

        // WAV plays either way - that is the requirement.
        assert!(info(&clip(&wav), false).playable);
        assert!(info(&clip(&wav), true).playable);
        assert_eq!(info(&clip(&wav), false).format, decode::ClipFormat::Native);

        // MP3 only with ffmpeg, and it says so through `format` rather than
        // failing at the moment the user presses the button.
        assert!(!info(&clip(&mp3), false).playable);
        assert!(info(&clip(&mp3), true).playable);
        assert_eq!(info(&clip(&mp3), true).format, decode::ClipFormat::Ffmpeg);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
