use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::persistence::buses::MAX_BUSES;
use crate::persistence::channels::MAX_CHANNELS;

/// Maximum concurrent meters (channels + buses + mic, with headroom).
///
/// This has to cover what the UI actually lets the user build, or the failure
/// lands somewhere far away: the channel sinks and bus sinks claim their slots
/// first, and it was the microphone chain — built last — that fell over with
/// "meter budget exhausted" once they had used them all up.
pub const MAX_METERS: usize = MAX_CHANNELS + MAX_BUSES + 4;

const _: () = assert!(
    MAX_METERS >= MAX_CHANNELS + MAX_BUSES + 2,
    "a full set of channels, every bus, the master and the mic must all fit"
);

/// How many independent consumers can drain the meters.
///
/// Today: the UI emitter, the keyboard's audio-reactive lighting, and the
/// headset OLED's VU mode.
pub const MAX_READERS: usize = 4;

/// Lock-free per-meter peak store with a dynamic name→slot registry
/// (channels are user-defined since the dynamic-channels work). Peaks are
/// written by realtime meter/DSP callbacks and drained by the level
/// emitter; values are f32 amplitudes bit-cast into AtomicU32.
pub struct LevelStore {
    /// One peak array per reader.
    ///
    /// Draining is destructive — it is a `swap(0)` — so a single array meant
    /// whichever consumer ticked first took the peak and the others saw
    /// whatever had accumulated since. With the keyboard's lighting draining
    /// every 33 ms, the OLED every 40 ms and the window every 100 ms, the
    /// meters in the window were showing the leftovers of the other two.
    peaks: [[[AtomicU32; 2]; MAX_METERS]; MAX_READERS],
    readers: AtomicUsize,
    slots: Mutex<SlotRegistry>,
}

#[derive(Default)]
struct SlotRegistry {
    by_name: HashMap<String, usize>,
    free: Vec<usize>,
    readers: HashMap<String, usize>,
}

impl LevelStore {
    pub fn new() -> Self {
        Self {
            peaks: Default::default(),
            readers: AtomicUsize::new(0),
            slots: Mutex::new(SlotRegistry::default()),
        }
    }

    /// A drain handle for one consumer, stable for a given name.
    ///
    /// Cheap enough to call per tick — it takes the registry lock, which the
    /// realtime `raise` path never touches. Past `MAX_READERS` it hands back
    /// reader 0, so a new consumer degrades to sharing rather than silently
    /// reading nothing.
    pub fn reader(&self, name: &str) -> usize {
        let Ok(mut registry) = self.slots.lock() else {
            return 0;
        };
        if let Some(id) = registry.readers.get(name) {
            return *id;
        }
        let id = registry.readers.len();
        if id >= MAX_READERS {
            return 0;
        }
        registry.readers.insert(name.to_string(), id);
        self.readers.store(id + 1, Ordering::Release);
        id
    }

    /// Slot for `name`, registering it on first use. None when the meter
    /// budget is exhausted.
    pub fn slot_for(&self, name: &str) -> Option<usize> {
        let mut registry = self.slots.lock().ok()?;
        if let Some(slot) = registry.by_name.get(name) {
            return Some(*slot);
        }
        let slot = registry
            .free
            .pop()
            .or_else(|| {
                let next = registry.by_name.len() + registry.free.len();
                (next < MAX_METERS).then_some(next)
            })?;
        registry.by_name.insert(name.to_string(), slot);
        Some(slot)
    }

    /// Free a name's slot for reuse (channel deleted).
    pub fn release(&self, name: &str) {
        if let Ok(mut registry) = self.slots.lock() {
            if let Some(slot) = registry.by_name.remove(name) {
                for reader in &self.peaks {
                    reader[slot][0].store(0, Ordering::Relaxed);
                    reader[slot][1].store(0, Ordering::Relaxed);
                }
                registry.free.push(slot);
            }
        }
    }

    /// Snapshot of registered meter names and their slots.
    pub fn names(&self) -> Vec<(String, usize)> {
        self.slots
            .lock()
            .map(|r| r.by_name.iter().map(|(n, s)| (n.clone(), *s)).collect())
            .unwrap_or_default()
    }

    /// Raise the stored peak for a meter channel, for every reader.
    ///
    /// Writes all reader slots, registered or not. Taking only the registered
    /// ones would make the result depend on whether a consumer happened to
    /// tick before the audio started, and losing the first peaks of a stream
    /// is exactly the kind of bug that only shows up on someone else's
    /// machine. Still lock-free and still fit for the realtime path: a
    /// handful of relaxed compare-exchanges on cache lines this thread just
    /// touched.
    pub fn raise(&self, slot: usize, channel: usize, amplitude: f32) {
        let channel = channel.min(1);
        let new = amplitude.to_bits();
        for reader in self.peaks.iter() {
            let Some(cell) = reader.get(slot).map(|p| &p[channel]) else {
                return;
            };
            let mut current = cell.load(Ordering::Relaxed);
            while f32::from_bits(current) < amplitude {
                match cell.compare_exchange_weak(
                    current,
                    new,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => current = actual,
                }
            }
        }
    }

    /// Read and reset one reader's peak for a meter channel.
    pub fn drain(&self, reader: usize, slot: usize, channel: usize) -> f32 {
        self.peaks
            .get(reader)
            .and_then(|r| r.get(slot))
            .map(|p| f32::from_bits(p[channel.min(1)].swap(0, Ordering::Relaxed)))
            .unwrap_or(0.0)
    }
}

impl Default for LevelStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raise_keeps_maximum_and_drain_resets() {
        let store = LevelStore::new();
        let r = store.reader("ui");
        let slot = store.slot_for("sink_game").expect("slot");
        store.raise(slot, 0, 0.5);
        store.raise(slot, 0, 0.3); // lower - ignored
        assert!((store.drain(r, slot, 0) - 0.5).abs() < f32::EPSILON);
        assert_eq!(store.drain(r, slot, 0), 0.0); // drained
    }

    #[test]
    fn one_consumer_draining_does_not_take_the_peak_from_the_others() {
        // The keyboard's lighting drains every 33 ms, the OLED every 40, the
        // window every 100. On a shared array whoever ticked first took the
        // peak and the rest saw the leftovers.
        let store = LevelStore::new();
        let (ui, keyboard, oled) = (
            store.reader("ui"),
            store.reader("keyboard"),
            store.reader("oled"),
        );
        assert_ne!(ui, keyboard);
        assert_ne!(keyboard, oled);

        let slot = store.slot_for("sink_game").expect("slot");
        store.raise(slot, 0, 0.75);

        assert!((store.drain(keyboard, slot, 0) - 0.75).abs() < f32::EPSILON);
        assert!((store.drain(oled, slot, 0) - 0.75).abs() < f32::EPSILON);
        assert!((store.drain(ui, slot, 0) - 0.75).abs() < f32::EPSILON);
        // ...and each one really did reset its own.
        assert_eq!(store.drain(ui, slot, 0), 0.0);
    }

    #[test]
    fn a_reader_is_stable_per_name_and_degrades_to_sharing_when_exhausted() {
        let store = LevelStore::new();
        assert_eq!(store.reader("ui"), store.reader("ui"));
        for i in 1..MAX_READERS {
            assert_eq!(store.reader(&format!("r{i}")), i);
        }
        // Past the budget: sharing reader 0 loses peaks, but reading nothing
        // at all would look like silence, which is worse.
        assert_eq!(store.reader("one-too-many"), 0);
    }

    #[test]
    fn a_peak_raised_before_anyone_registers_is_still_there() {
        // Audio can start before a consumer first ticks; if raise() only wrote
        // to registered readers, those peaks would vanish and the meter would
        // start late for reasons nothing on screen could explain.
        let store = LevelStore::new();
        let slot = store.slot_for("sink_game").expect("slot");
        store.raise(slot, 0, 0.6);
        let late = store.reader("arrived-afterwards");
        assert!((store.drain(late, slot, 0) - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn the_meter_budget_covers_a_full_ui_configuration() {
        // Channels and buses claim their slots before the mic chain is built,
        // so a budget that only just fits them made the microphone the thing
        // that failed.
        let store = LevelStore::new();
        for i in 0..MAX_CHANNELS {
            assert!(store.slot_for(&format!("sink_ch{i}")).is_some());
        }
        for i in 0..MAX_BUSES {
            assert!(store.slot_for(&format!("sink_bus{i}")).is_some());
        }
        assert!(store.slot_for("sink_stream").is_some(), "the master mix");
        assert!(store.slot_for("sink_mic").is_some(), "and the microphone");
    }

    #[test]
    fn slots_are_stable_and_reusable() {
        let store = LevelStore::new();
        let _ = store.reader("ui");
        let a = store.slot_for("sink_game").expect("slot");
        assert_eq!(store.slot_for("sink_game"), Some(a), "stable per name");
        let b = store.slot_for("sink_chat").expect("slot");
        assert_ne!(a, b);
        store.release("sink_game");
        let c = store.slot_for("sink_voice").expect("slot");
        assert_eq!(c, a, "freed slot is reused");
    }

    #[test]
    fn budget_is_enforced() {
        let store = LevelStore::new();
        for i in 0..MAX_METERS {
            assert!(store.slot_for(&format!("m{i}")).is_some());
        }
        assert!(store.slot_for("overflow").is_none());
    }
}
