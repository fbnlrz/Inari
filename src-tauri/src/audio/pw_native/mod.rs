//! Native PipeWire backend (Phase 2): replaces pactl subprocess calls with
//! pipewire-rs. All PipeWire objects live on a dedicated loop thread (see
//! `thread.rs`); this facade sends commands over a pipewire channel and
//! blocks on an mpsc reply with a timeout.
//!
//! Extras over the pactl backend: real per-sink level metering (`levels`).

mod dsp;
mod eq;
mod eq_chain;
pub mod levels;
pub mod meter;
mod mic;
mod pods;
mod ring;
mod thread;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use pipewire as pw;

use crate::audio::backend::AudioBackend;
use crate::audio::types::{AppStream, OutputDevice};
use crate::error::SinkError;
use levels::LevelStore;
use thread::Cmd;

pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

/// How long the graph must hold still before a change is announced. Long
/// enough to swallow the dozens of globals a session start, a profile switch
/// or a device hotplug produce back to back, short enough that the UI still
/// reacts faster than the 2s poll it replaces.
pub const GRAPH_QUIET: Duration = Duration::from_millis(150);
/// Ceiling on that wait: a graph that keeps moving (an app opening streams in
/// a loop) must not starve the UI of updates forever.
pub const GRAPH_MAX_WAIT: Duration = Duration::from_secs(1);

/// Revision counter for structural PipeWire graph changes: nodes appearing,
/// vanishing or being renamed, and default-device switches. The loop thread
/// bumps it, the emitter thread blocks on it - so an idle session costs no
/// wakeups at all, which is the whole point of replacing the poll.
pub struct GraphNotify {
    revision: Mutex<u64>,
    changed: Condvar,
}

impl Default for GraphNotify {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphNotify {
    pub fn new() -> Self {
        Self {
            revision: Mutex::new(0),
            changed: Condvar::new(),
        }
    }

    /// Called from the loop thread. Never blocks on anything but the tiny
    /// revision mutex - it runs inside PipeWire callbacks, where anything
    /// that could pump the loop would re-enter them.
    pub fn bump(&self) {
        if let Ok(mut r) = self.revision.lock() {
            *r = r.wrapping_add(1);
        }
        self.changed.notify_all();
    }

    /// Block until the revision moves away from `since` or `timeout` elapses,
    /// then report the current revision.
    pub fn wait_changed(&self, since: u64, timeout: Duration) -> u64 {
        let Ok(guard) = self.revision.lock() else {
            return since;
        };
        match self
            .changed
            .wait_timeout_while(guard, timeout, |r| *r == since)
        {
            Ok((guard, _)) => *guard,
            Err(_) => since,
        }
    }
}

/// Turns a stream of revision bumps into at most one UI event per burst:
/// emit once the graph has been quiet for `quiet`, but never hold an update
/// back longer than `max_wait`. Kept free of PipeWire and Tauri so the timing
/// rules are unit-testable.
pub struct GraphCoalescer {
    quiet: Duration,
    max_wait: Duration,
    /// Last revision handed to `poll`.
    seen: u64,
    /// Last revision an emit was reported for.
    emitted: u64,
    /// When the pending burst started and when it last moved.
    burst_start: Option<Instant>,
    last_change: Option<Instant>,
}

impl GraphCoalescer {
    pub fn new(quiet: Duration, max_wait: Duration) -> Self {
        Self {
            quiet,
            max_wait,
            seen: 0,
            emitted: 0,
            burst_start: None,
            last_change: None,
        }
    }

    /// True while a change is waiting for its quiet window - the caller uses
    /// it to pick a short tick over an idle block.
    pub fn pending(&self) -> bool {
        self.burst_start.is_some()
    }

    /// Feed the current revision; true means "emit now".
    pub fn poll(&mut self, revision: u64, now: Instant) -> bool {
        if revision != self.seen {
            self.seen = revision;
            self.last_change = Some(now);
            self.burst_start.get_or_insert(now);
        }
        let (Some(start), Some(last)) = (self.burst_start, self.last_change) else {
            return false;
        };
        if now.duration_since(last) < self.quiet && now.duration_since(start) < self.max_wait {
            return false;
        }
        self.emitted = self.seen;
        self.burst_start = None;
        self.last_change = None;
        true
    }
}

pub struct PipeWireBackend {
    sender: Mutex<pw::channel::Sender<Cmd>>,
    /// Set while the loop thread is running; cleared by the thread on every
    /// exit path. Without it a dead loop is only visible as a 3s timeout per
    /// call - the app looks hung instead of broken.
    alive: Arc<AtomicBool>,
    /// Live per-sink peak levels, fed by the meter capture streams.
    pub levels: Arc<LevelStore>,
    /// Bumped by the loop thread whenever the node graph changes; drives the
    /// `graph-changed` event the UI refetches on.
    pub graph: Arc<GraphNotify>,
}

impl PipeWireBackend {
    pub fn new() -> Result<Self, SinkError> {
        let levels = Arc::new(LevelStore::new());
        let graph = Arc::new(GraphNotify::new());
        let (sender, receiver) = pw::channel::channel();
        let (init_tx, init_rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(false));

        let thread_levels = levels.clone();
        let thread_graph = graph.clone();
        let thread_alive = alive.clone();
        std::thread::Builder::new()
            .name("pipewire-loop".into())
            .spawn(move || thread::run(receiver, init_tx, thread_levels, thread_graph, thread_alive))
            .map_err(|e| SinkError::Config(format!("spawn pipewire thread: {e}")))?;

        match init_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                sender: Mutex::new(sender),
                alive,
                levels,
                graph,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(SinkError::Config(
                "pipewire loop did not come up within 5s".into(),
            )),
        }
    }

    /// False once the loop thread has left. The state it owned (sinks, links,
    /// EQ chains) died with it and is not reconstructible from here, so we
    /// report the fact instead of restarting the loop behind the user's back.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn request<T>(
        &self,
        build: impl FnOnce(mpsc::Sender<Result<T, SinkError>>) -> Cmd,
    ) -> Result<T, SinkError> {
        // Checked before sending: a send into a dead loop's channel still
        // succeeds (the queue outlives the thread), so without this every
        // call would burn the full REQUEST_TIMEOUT.
        if !self.is_alive() {
            return Err(SinkError::EngineStopped);
        }
        let (tx, rx) = mpsc::channel();
        {
            let sender = self
                .sender
                .lock()
                .map_err(|_| SinkError::Config("pipewire sender lock poisoned".into()))?;
            sender.send(build(tx)).map_err(|_| SinkError::EngineStopped)?;
        }
        // The loop can also die *during* a request; say so rather than
        // blaming a timeout the user can do nothing about.
        rx.recv_timeout(REQUEST_TIMEOUT).map_err(|_| {
            if self.is_alive() {
                SinkError::Config("pipewire request timed out".into())
            } else {
                SinkError::EngineStopped
            }
        })?
    }
}

impl AudioBackend for PipeWireBackend {
    fn is_engine_alive(&self) -> bool {
        self.is_alive()
    }

    fn create_virtual_sink(&self, name: &str, label: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        let label = label.to_string();
        self.request(|reply| Cmd::CreateSink { name, label, reply })
    }

    fn destroy_virtual_sink(&self, name: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        self.request(|reply| Cmd::DestroySink { name, reply })
    }

    fn list_app_streams(&self) -> Result<Vec<AppStream>, SinkError> {
        self.request(|reply| Cmd::ListStreams { reply })
    }

    fn list_output_devices(&self) -> Result<Vec<OutputDevice>, SinkError> {
        self.request(|reply| Cmd::ListOutputs { reply })
    }

    fn resolved_channel_outputs(
        &self,
    ) -> Result<std::collections::HashMap<String, Option<String>>, SinkError> {
        self.request(|reply| Cmd::ResolvedOutputs { reply })
    }

    fn set_sink_volume(&self, sink_name: &str, volume_percent: u8) -> Result<(), SinkError> {
        let name = sink_name.to_string();
        self.request(|reply| Cmd::SetNodeVolumeByName {
            name,
            percent: volume_percent,
            reply,
        })
    }

    fn set_sink_mute(&self, sink_name: &str, muted: bool) -> Result<(), SinkError> {
        let name = sink_name.to_string();
        self.request(|reply| Cmd::SetNodeMuteByName { name, muted, reply })
    }

    fn sink_state(&self, sink_name: &str) -> Result<Option<(u8, bool)>, SinkError> {
        // No extra round trip to the server: the loop thread already mirrors
        // every node's volume/mute from its Props param events.
        let name = sink_name.to_string();
        self.request(|reply| Cmd::SinkState { name, reply })
    }

    fn move_stream_to_sink(&self, stream_index: u32, sink_name: &str) -> Result<(), SinkError> {
        let sink_name = sink_name.to_string();
        self.request(|reply| Cmd::MoveStream {
            id: stream_index,
            sink_name,
            reply,
        })
    }

    fn set_app_volume(&self, stream_index: u32, volume_percent: u8) -> Result<(), SinkError> {
        self.request(|reply| Cmd::SetNodeVolumeById {
            id: stream_index,
            percent: volume_percent,
            reply,
        })
    }

    fn set_channel_output(
        &self,
        sink_name: &str,
        output_name: Option<&str>,
    ) -> Result<(), SinkError> {
        let sink_name = sink_name.to_string();
        let output_name = output_name.map(str::to_string);
        self.request(|reply| Cmd::SetChannelOutput {
            sink_name,
            output_name,
            reply,
        })
    }

    fn set_channel_failover(&self, sink_name: &str, enabled: bool) -> Result<(), SinkError> {
        let sink_name = sink_name.to_string();
        self.request(|reply| Cmd::SetChannelFailover {
            sink_name,
            enabled,
            reply,
        })
    }

    fn create_bus(&self, name: &str, label: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        let label = label.to_string();
        self.request(|reply| Cmd::CreateBus { name, label, reply })
    }

    fn destroy_bus(&self, name: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        self.request(|reply| Cmd::DestroyBus { name, reply })
    }

    fn set_bus_members(&self, name: &str, channels: &[String]) -> Result<(), SinkError> {
        let name = name.to_string();
        let channels = channels.to_vec();
        self.request(|reply| Cmd::SetBusMembers { name, channels, reply })
    }

    fn set_monitor(&self, name: &str, enabled: bool) -> Result<(), SinkError> {
        let name = name.to_string();
        self.request(|reply| Cmd::SetMonitor { name, enabled, reply })
    }

    fn list_input_devices(&self) -> Result<Vec<crate::audio::types::OutputDevice>, SinkError> {
        self.request(|reply| Cmd::ListInputs { reply })
    }

    fn set_mic_config(&self, config: &crate::audio::types::MicConfig) -> Result<(), SinkError> {
        let config = config.clone();
        self.request(|reply| Cmd::SetMicConfig { config, reply })
    }

    fn set_channel_eq(
        &self,
        sink_name: &str,
        config: &crate::audio::types::EqConfig,
    ) -> Result<(), SinkError> {
        let sink_name = sink_name.to_string();
        let config = config.clone();
        self.request(|reply| Cmd::SetChannelEq { sink_name, config, reply })
    }

    fn get_default_devices(&self) -> Result<(Option<String>, Option<String>), SinkError> {
        self.request(|reply| Cmd::GetDefaults { reply })
    }

    fn set_default_output(&self, name: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        self.request(|reply| Cmd::SetDefault { input: false, name, reply })
    }

    fn set_default_input(&self, name: &str) -> Result<(), SinkError> {
        let name = name.to_string();
        self.request(|reply| Cmd::SetDefault { input: true, name, reply })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::backend::AudioBackend;
    use std::time::Instant;

    /// A backend whose loop thread is gone: nothing was ever spawned, so the
    /// alive flag is clear and the receiving end of the channel is dropped -
    /// exactly the state a crashed loop leaves behind.
    fn stopped_backend() -> PipeWireBackend {
        let (sender, _receiver) = pw::channel::channel::<Cmd>();
        PipeWireBackend {
            sender: Mutex::new(sender),
            alive: Arc::new(AtomicBool::new(false)),
            levels: Arc::new(LevelStore::new()),
            graph: Arc::new(GraphNotify::new()),
        }
    }

    #[test]
    fn requests_fail_fast_once_the_loop_is_gone() {
        let backend = stopped_backend();
        assert!(!backend.is_engine_alive());
        let started = Instant::now();
        let err = backend.list_output_devices().unwrap_err();
        assert!(matches!(err, SinkError::EngineStopped), "got {err}");
        // The whole point of the flag: no REQUEST_TIMEOUT wait per call.
        assert!(started.elapsed() < REQUEST_TIMEOUT, "waited on a dead loop");
    }

    #[test]
    fn a_live_backend_reports_itself_alive() {
        let backend = stopped_backend();
        backend.alive.store(true, Ordering::SeqCst);
        assert!(backend.is_engine_alive());
    }

    const QUIET: Duration = Duration::from_millis(150);
    const MAX_WAIT: Duration = Duration::from_secs(1);

    fn coalescer() -> GraphCoalescer {
        GraphCoalescer::new(QUIET, MAX_WAIT)
    }

    #[test]
    fn an_idle_graph_never_emits() {
        let mut c = coalescer();
        let t0 = Instant::now();
        assert!(!c.poll(0, t0));
        assert!(!c.poll(0, t0 + Duration::from_secs(10)));
        assert!(!c.pending());
    }

    #[test]
    fn a_startup_burst_becomes_one_event() {
        let mut c = coalescer();
        let t0 = Instant::now();
        // 40 globals in 40ms: the session coming up.
        for i in 1..=40u64 {
            assert!(!c.poll(i, t0 + Duration::from_millis(i)));
        }
        assert!(c.pending(), "still inside the quiet window");
        // Nothing more happens - one event, once it has held still.
        assert!(!c.poll(40, t0 + Duration::from_millis(100)));
        assert!(c.poll(40, t0 + Duration::from_millis(250)));
        assert!(!c.pending());
        assert!(!c.poll(40, t0 + Duration::from_secs(5)), "no repeats");
    }

    #[test]
    fn a_never_quiet_graph_still_emits_at_the_ceiling() {
        let mut c = coalescer();
        let t0 = Instant::now();
        // A change every 50ms: the quiet window alone would never fire.
        let mut emits = 0;
        for i in 1..=60u64 {
            if c.poll(i, t0 + Duration::from_millis(i * 50)) {
                emits += 1;
            }
        }
        // 3s of churn under a 1s ceiling: two updates, not sixty.
        assert_eq!(emits, 2);
    }

    #[test]
    fn a_change_after_an_emit_starts_a_fresh_window() {
        let mut c = coalescer();
        let t0 = Instant::now();
        c.poll(1, t0);
        assert!(c.poll(1, t0 + QUIET));

        c.poll(2, t0 + Duration::from_secs(1));
        assert!(!c.poll(2, t0 + Duration::from_secs(1)), "quiet not yet met");
        assert!(c.poll(2, t0 + Duration::from_secs(1) + QUIET));
    }

    #[test]
    fn wait_changed_returns_at_once_when_the_revision_already_moved() {
        let notify = GraphNotify::new();
        notify.bump();
        // The caller's `since` is stale, so this must not block for the
        // timeout - a bump that lands between two waits is not lost.
        let started = Instant::now();
        assert_eq!(notify.wait_changed(0, Duration::from_secs(30)), 1);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn wait_changed_wakes_on_a_bump_from_another_thread() {
        let notify = Arc::new(GraphNotify::new());
        let bumper = notify.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            bumper.bump();
        });
        assert_eq!(notify.wait_changed(0, Duration::from_secs(30)), 1);
    }
}
