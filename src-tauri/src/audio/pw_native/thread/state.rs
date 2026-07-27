//! The loop thread's shared state: the command enum the facade sends in, the
//! registry mirror every handler reads and writes, and the core handle the
//! listener closures reach for. State flows only through `Rc<RefCell<State>>`,
//! which is what lets the handlers live in separate modules at all.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::warn;
use pipewire as pw;
use pw::core::CoreRc;
use pw::metadata::{Metadata, MetadataListener};
use pw::node::{Node, NodeListener};

use crate::audio::pw_native::eq_chain::EqChainHandle;
use crate::audio::pw_native::levels::LevelStore;
use crate::audio::pw_native::meter::MeterHandle;
use crate::audio::pw_native::mic::MicStreams;
use crate::audio::pw_native::GraphNotify;
use crate::audio::types::{AppStream, EqConfig, MicConfig, OutputDevice};
use crate::error::SinkError;

pub(super) type Reply<T> = mpsc::Sender<Result<T, SinkError>>;
/// A set of live links: (output port, input port, proxy).
pub(super) type LinkSet = Vec<(u32, u32, pw::link::Link)>;

pub enum Cmd {
    CreateSink { name: String, label: String, reply: Reply<()> },
    DestroySink { name: String, reply: Reply<()> },
    ListStreams { reply: Reply<Vec<AppStream>> },
    ListOutputs { reply: Reply<Vec<OutputDevice>> },
    ResolvedOutputs { reply: Reply<HashMap<String, Option<String>>> },
    /// Live volume/mute of a node by name (None = unknown or not yet observed).
    SinkState { name: String, reply: Reply<Option<(u8, bool)>> },
    SetNodeVolumeByName { name: String, percent: u8, reply: Reply<()> },
    SetNodeMuteByName { name: String, muted: bool, reply: Reply<()> },
    SetNodeVolumeById { id: u32, percent: u8, reply: Reply<()> },
    MoveStream { id: u32, sink_name: String, reply: Reply<()> },
    /// Route a channel's monitor to an output device (None = follow default).
    SetChannelOutput { sink_name: String, output_name: Option<String>, reply: Reply<()> },
    SetChannelFailover { sink_name: String, enabled: bool, reply: Reply<()> },
    /// Create a mix bus (capturable virtual source).
    CreateBus { name: String, label: String, reply: Reply<()> },
    /// Destroy a mix bus and its links.
    DestroyBus { name: String, reply: Reply<()> },
    /// Replace the channel set feeding a bus.
    SetBusMembers { name: String, channels: Vec<String>, reply: Reply<()> },
    /// Listen to a channel/mix/mic on the default output (session scoped).
    SetMonitor { name: String, enabled: bool, reply: Reply<()> },
    /// Apply mic chain configuration (create/destroy/re-tune as needed).
    SetMicConfig { config: MicConfig, reply: Reply<()> },
    /// Apply a channel's parametric EQ (create/destroy/re-tune the insert).
    SetChannelEq { sink_name: String, config: EqConfig, reply: Reply<()> },
    /// Hardware capture devices (microphones).
    ListInputs { reply: Reply<Vec<OutputDevice>> },
    /// Current system defaults: (output sink name, input source name).
    GetDefaults { reply: Reply<(Option<String>, Option<String>)> },
    /// Set the configured system default sink (input=false) or source.
    SetDefault { input: bool, name: String, reply: Reply<()> },
}

pub(super) struct PortEntry {
    pub(super) id: u32,
    pub(super) node_id: u32,
    /// "in" (playback/sink input port) or "out" (source/monitor port).
    pub(super) direction: String,
    /// e.g. "FL", "FR", "MONO".
    pub(super) channel: Option<String>,
}

pub(super) struct NodeEntry {
    pub(super) id: u32,
    pub(super) serial: Option<u64>,
    pub(super) media_class: String,
    pub(super) props: HashMap<String, String>,
    pub(super) proxy: Node,
    pub(super) _listener: NodeListener,
    pub(super) volume_percent: u8,
    pub(super) channels: usize,
    pub(super) muted: bool,
    /// True once a Props param event has actually been seen for this node.
    /// Until then `volume_percent`/`muted` are placeholders, not readings.
    pub(super) props_seen: bool,
    /// True while the node is in the Running state (actively streaming).
    pub(super) active: bool,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) nodes: HashMap<u32, NodeEntry>,
    /// link global id -> (output node id, input node id)
    pub(super) links: HashMap<u32, (u32, u32)>,
    pub(super) metadata: Option<Metadata>,
    pub(super) _metadata_listener: Option<MetadataListener>,
    pub(super) default_sink_name: Option<String>,
    pub(super) default_source_name: Option<String>,
    /// Virtual sinks we created: name -> created-object proxy (kept alive;
    /// destroyed explicitly on teardown).
    pub(super) owned_sinks: HashMap<String, Node>,
    /// Sinks that existed before us (e.g. leftover pactl modules): name -> global id.
    pub(super) adopted_sinks: HashMap<String, u32>,
    /// Nodes that must stay alive: name -> (label, kind 0=sink 1=bus 2=mic).
    /// If one vanishes without us destroying it (another instance dying,
    /// a PipeWire restart, wpctl) it gets recreated on the spot.
    pub(super) desired: HashMap<String, (String, u8)>,
    /// Create requests waiting for the sink's global to appear, with the
    /// instant the request was made. The global may never come (the factory
    /// failed server-side); the timestamp lets us drop the dead waiters
    /// instead of holding them for the process lifetime.
    pub(super) pending_creates: HashMap<String, (Instant, Vec<Reply<()>>)>,
    /// Live meter capture streams per virtual sink name.
    pub(super) meters: HashMap<String, MeterHandle>,
    /// All known ports, for monitor→output linking.
    pub(super) ports: HashMap<u32, PortEntry>,
    /// Channel sink name -> chosen output node.name (None = follow default).
    pub(super) channel_outputs: HashMap<String, Option<String>>,

    /// Channel sink name -> live loopback links.
    pub(super) channel_links: HashMap<String, LinkSet>,
    /// Channel sink name -> the device node id it currently routes to (after
    /// explicit/default/fallback resolution). Lets the UI show what "System
    /// default" actually resolves to, and makes failover visible.
    pub(super) channel_targets: HashMap<String, u32>,
    /// Channels with auto-failover turned off: they route only to their chosen
    /// device (or the exact default) and stay silent when it's gone. Absence
    /// (the default) means failover is on.
    pub(super) channel_strict: std::collections::HashSet<String>,
    /// Phase 3 mic chain.
    pub(super) mic_config: MicConfig,
    /// Proxy for the sink_mic virtual source (kept alive while enabled).
    pub(super) mic_source: Option<Node>,
    /// Mic-node removals we caused ourselves (a rename recreates the node), so
    /// the heal path can tell them from an external destroy and only recreate
    /// for the latter.
    pub(super) mic_expected_removals: u32,
    pub(super) mic_streams: Option<MicStreams>,
    pub(super) levels: Option<Arc<LevelStore>>,
    /// Bumped on every structural graph change so the UI can refetch on an
    /// event instead of polling. Optional only because `State::default()` is
    /// used by the unit tests.
    pub(super) graph: Option<Arc<GraphNotify>>,
    /// Mix buses we own: node name -> proxy.
    pub(super) bus_sources: HashMap<String, Node>,
    /// Bus node name -> member channel sink names.
    pub(super) bus_members: HashMap<String, std::collections::HashSet<String>>,
    /// (bus, channel) -> live links feeding the bus.
    pub(super) bus_links: HashMap<(String, String), LinkSet>,
    /// Nodes monitored on the default output, and their live links.
    pub(super) monitored: std::collections::HashSet<String>,
    pub(super) monitor_links: HashMap<String, LinkSet>,
    /// Links from the mic playback stream into the virtual mic.
    pub(super) mic_links: LinkSet,
    /// Per-channel EQ configs (source of truth for chain (re)creation -
    /// kept even while disabled so re-enabling restores the bands).
    pub(super) eq_configs: HashMap<String, EqConfig>,
    /// Live EQ inserts by channel sink name. Presence here *is* "EQ is
    /// enabled and live"; the channel's outgoing links re-source from the
    /// insert's playback node.
    pub(super) eq_streams: HashMap<String, EqChainHandle>,
    /// EQ playback node id -> node ids it is allowed to feed. Rebuilt
    /// wholesale by every `ensure_all_links` pass; the link police destroys
    /// anything else (WirePlumber routes playback streams to the default
    /// sink - same leak the mic police exists for).
    pub(super) eq_desired_targets: HashMap<u32, std::collections::HashSet<u32>>,
}

impl State {
    /// Live node id of the mic playback stream. Resolved lazily - the id
    /// is only valid once the server has created the stream's node.
    pub(super) fn mic_playback_node(&self) -> Option<u32> {
        self.mic_streams
            .as_ref()
            .map(|m| m.playback_node_id())
            .filter(|id| *id != u32::MAX)
    }

    /// Live node id of a channel's EQ playback stream, if the insert is up.
    pub(super) fn eq_playback_node(&self, sink_name: &str) -> Option<u32> {
        self.eq_streams
            .get(sink_name)
            .map(|h| h.playback_node_id())
            .filter(|id| *id != u32::MAX)
    }
}

/// Announce a structural graph change (node added/removed/renamed, default
/// device switched). Only a counter bump and a condvar notify - safe to call
/// from inside a PipeWire callback, where anything that pumps the loop would
/// re-enter it. The emitter thread does the coalescing.
pub(super) fn notify_graph(state: &Rc<RefCell<State>>) {
    if let Some(graph) = state.borrow().graph.as_ref() {
        graph.bump();
    }
}

impl State {
    /// Names of the virtual sinks we expect to see in the registry: created
    /// by us this run, waiting on a create, or declared as a channel we keep
    /// alive (kind 0). Drives the adoption decision - see `should_adopt_sink`.
    pub(super) fn expected_sinks(&self) -> impl Iterator<Item = &str> + '_ {
        self.owned_sinks
            .keys()
            .chain(self.pending_creates.keys())
            .chain(
                self.desired
                    .iter()
                    .filter(|(_, (_, kind))| *kind == 0)
                    .map(|(name, _)| name),
            )
            .map(String::as_str)
    }

    pub(super) fn node_by_name(&self, name: &str) -> Option<&NodeEntry> {
        self.nodes
            .values()
            .find(|n| n.props.get("node.name").map(String::as_str) == Some(name))
    }

    /// A node's volume/mute as observed on the wire, or `None` while we have
    /// no reading for it. A freshly bound `NodeEntry` carries placeholder
    /// defaults until its first Props event lands, and passing those off as
    /// the sink's state would be exactly the illusion the read path exists to
    /// end - an unknown node and an unobserved one are equally unknown.
    pub(super) fn observed_state(&self, name: &str) -> Option<(u8, bool)> {
        self.node_by_name(name)
            .filter(|n| n.props_seen)
            .map(|n| (n.volume_percent, n.muted))
    }

    /// The sink a stream is currently connected to, resolved through links.
    pub(super) fn sink_of_stream(&self, stream_id: u32) -> Option<&NodeEntry> {
        self.links
            .values()
            .find(|(out, _)| *out == stream_id)
            .and_then(|(_, input)| self.nodes.get(input))
    }
}

// The CoreRc is needed by the command handler (object creation/destruction);
// this thread owns all PipeWire objects, so a thread-local is the simplest
// way to share it across the listener closures.
thread_local! {
    pub(super) static CORE: RefCell<Option<CoreRc>> = const { RefCell::new(None) };
}

/// Drop create requests whose sink global never appeared. The reply is only
/// sent when the registry announces the node, so a server-side factory
/// failure would park the waiters in the map for the process lifetime.
/// Anything older than the caller's own timeout is dead weight: whoever was
/// waiting has long since given up. Generic over the payload so the rule is
/// unit-testable without PipeWire.
pub(super) fn prune_pending<T>(
    pending: &mut HashMap<String, (Instant, T)>,
    now: Instant,
    max_age: Duration,
) {
    pending.retain(|name, (queued, _)| {
        let keep = now.duration_since(*queued) < max_age;
        if !keep {
            warn!("create of {name} never completed - dropping the request");
        }
        keep
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::pw_native::mic::MIC_NODE;
    use crate::audio::pw_native::REQUEST_TIMEOUT;

    #[test]
    fn expected_sinks_covers_created_pending_and_desired_channels() {
        let mut s = State::default();
        s.pending_creates
            .insert("sink_pending".into(), (Instant::now(), Vec::new()));
        s.desired.insert("sink_kept".into(), ("Kept".into(), 0));
        // Buses and the virtual mic are not sinks and must not be adopted
        // through this path.
        s.desired.insert("bus_stream".into(), ("Stream".into(), 1));
        s.desired.insert(MIC_NODE.into(), ("Mic".into(), 2));
        let expected: Vec<&str> = s.expected_sinks().collect();
        assert!(expected.contains(&"sink_pending"));
        assert!(expected.contains(&"sink_kept"));
        assert!(!expected.contains(&"bus_stream"));
        assert!(!expected.contains(&MIC_NODE));
    }

    /// A node we have never seen has no state to report - the caller must get
    /// `None` and fall back, not a fabricated 100%. (The observed branch needs
    /// a bound PipeWire node, which can't be built without a live server.)
    #[test]
    fn observed_state_of_an_unknown_node_is_unknown() {
        let s = State::default();
        assert_eq!(s.observed_state("sink_music"), None);
    }

    #[test]
    fn prune_pending_drops_only_the_waiters_past_their_timeout() {
        let now = Instant::now();
        let mut pending: HashMap<String, (Instant, u8)> = HashMap::new();
        pending.insert("fresh".into(), (now - Duration::from_millis(500), 1));
        pending.insert("stale".into(), (now - Duration::from_secs(30), 2));
        prune_pending(&mut pending, now, REQUEST_TIMEOUT);
        assert!(pending.contains_key("fresh"));
        assert!(!pending.contains_key("stale"), "caller timed out long ago");
    }
}
