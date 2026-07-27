//! Link reconciliation: which device a channel routes to, which ports get
//! wired to which, and the idempotent passes that make the live graph match.
//! Mostly pure functions over plain data, so the routing rules are unit-
//! testable without a PipeWire server.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use log::error;
use pipewire as pw;
use pw::core::CoreRc;

use crate::audio::pw_native::mic::MIC_NODE;
use crate::audio::pw_native::REQUEST_TIMEOUT;
use crate::audio::types::is_virtual_sink;

use super::state::{prune_pending, LinkSet, PortEntry, State, CORE};
use super::SINK_CLASS;

/// The node whose ports feed a channel's downstream links: the EQ insert's
/// playback stream when one is live, otherwise the channel sink itself.
/// Pure so the routing decision is unit-testable (like `resolve_target`).
pub(super) fn resolve_source(eq_playback: Option<u32>, channel_id: u32) -> u32 {
    eq_playback.unwrap_or(channel_id)
}

/// node id -> that node's ports. Built once per reconcile pass: `desired_pairs`
/// used to scan the entire port map twice per (source, target) pair, and a
/// reconcile runs that for every channel times every bus - thousands of full
/// scans per registry event on a session with 200+ ports, and registry events
/// arrive in floods at startup.
type PortIndex<'a> = HashMap<u32, Vec<&'a PortEntry>>;

fn index_ports(ports: &HashMap<u32, PortEntry>) -> PortIndex<'_> {
    let mut index: PortIndex<'_> = HashMap::new();
    for port in ports.values() {
        index.entry(port.node_id).or_default().push(port);
    }
    index
}

/// Compute monitor→input port pairs from `channel_id`'s output ports to
/// `target_id`'s input ports. Pairs by audio.channel where possible, with
/// an index-wrap fallback for mono/odd channel maps.
fn desired_pairs(ports: &PortIndex, channel_id: u32, target_id: u32) -> Vec<(u32, u32)> {
    if channel_id == target_id {
        return Vec::new();
    }
    let select = |node_id: u32, direction: &str| {
        let mut selected: Vec<&PortEntry> = ports
            .get(&node_id)
            .map(|list| {
                list.iter()
                    .copied()
                    .filter(|p| p.direction == direction)
                    .collect()
            })
            .unwrap_or_default();
        selected.sort_by_key(|p| p.id);
        selected
    };
    let monitors = select(channel_id, "out");
    let inputs = select(target_id, "in");
    if monitors.is_empty() || inputs.is_empty() {
        return Vec::new();
    }
    // Mono source into a multi-channel target: fan out to every input
    // (e.g. listening to the mic - both ears, not just FL).
    if monitors.len() == 1 && inputs.len() > 1 {
        let m = monitors[0];
        return inputs.iter().map(|p| (m.id, p.id)).collect();
    }
    monitors
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let by_channel = m.channel.as_ref().and_then(|ch| {
                inputs
                    .iter()
                    .find(|p| p.channel.as_ref() == Some(ch))
                    .copied()
            });
            let input = by_channel.or_else(|| inputs.get(i % inputs.len()).copied())?;
            Some((m.id, input.id))
        })
        .collect()
}

/// Highest-`priority.session` non-virtual sink from `(id, node.name, priority)`
/// candidates. Pure (plain tuples) so the failover choice is unit-testable,
/// and it reuses WirePlumber's own scoring so Sink's fallback matches the
/// device the OS would pick, consistently across distros.
fn pick_fallback_sink<'a>(candidates: impl Iterator<Item = (u32, &'a str, i64)>) -> Option<u32> {
    candidates
        .filter(|(_, name, _)| !is_virtual_sink(name))
        .max_by_key(|&(_, _, priority)| priority)
        .map(|(id, _, _)| id)
}

/// The real output sink to fall back to when a follow-default channel's
/// default has no live node - e.g. the device was unplugged and WirePlumber
/// hasn't reassigned the default. Without it such a channel gets no links and
/// goes silent (the field-reported "no audio on speakers when headset off").
fn fallback_sink(s: &State) -> Option<u32> {
    pick_fallback_sink(
        s.nodes
            .values()
            .filter(|n| n.media_class == SINK_CLASS)
            .map(|n| {
                (
                    n.id,
                    n.props.get("node.name").map(String::as_str).unwrap_or(""),
                    n.props
                        .get("priority.session")
                        .and_then(|p| p.parse::<i64>().ok())
                        .unwrap_or(0),
                )
            }),
    )
}

/// Which device a channel routes to. `explicit_id` is the pinned device's node
/// id when it's set *and* present; `pinned` is whether a device is pinned at
/// all; `strict` is failover-off. Follow-default and pinned-but-gone channels
/// take the default, then - only when failover is on - the best available
/// sink; in strict mode a gone device resolves to nothing (silence) rather than
/// jumping elsewhere. Pure, so the whole matrix is unit-testable.
fn resolve_target(
    explicit_id: Option<u32>,
    pinned: bool,
    strict: bool,
    default_id: Option<u32>,
    fallback: Option<u32>,
) -> Option<u32> {
    match explicit_id {
        Some(id) => Some(id),
        None if pinned && strict => None,
        None if strict => default_id,
        None => default_id.or(fallback),
    }
}

/// Create link objects for `pairs` between two nodes; returns the proxies.
fn create_links(
    core: &CoreRc,
    sink_name: &str,
    out_node: u32,
    in_node: u32,
    pairs: &[(u32, u32)],
) -> LinkSet {
    let mut created = Vec::new();
    for (monitor_port, input_port) in pairs {
        match core.create_object::<pw::link::Link>(
            "link-factory",
            &pw::properties::properties! {
                "link.output.node" => out_node.to_string(),
                "link.output.port" => monitor_port.to_string(),
                "link.input.node" => in_node.to_string(),
                "link.input.port" => input_port.to_string(),
            },
        ) {
            Ok(link) => created.push((*monitor_port, *input_port, link)),
            Err(e) => error!("link {sink_name} failed: {e}"),
        }
    }
    created
}

/// Reconcile loopback links for every virtual channel:
/// - monitor → chosen output device (or the system default when unset /
///   the chosen device is gone - automatic failover)
/// - monitor → Stream Mix source (Phase 5, for OBS capture)
///
/// Idempotent - existing correct links are left untouched.
pub(super) fn ensure_all_links(state: &Rc<RefCell<State>>) {
    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
        return;
    };
    let mut s = state.borrow_mut();
    // Reborrow so the port index below can coexist with the mutations that
    // follow: they touch disjoint fields, which `RefMut`'s deref would hide.
    let s = &mut *s;
    // One name→id snapshot per reconcile instead of a linear node scan per
    // lookup (this runs on every relevant registry event).
    let node_ids: HashMap<String, u32> = s
        .nodes
        .values()
        .filter_map(|n| n.props.get("node.name").map(|name| (name.clone(), n.id)))
        .collect();
    // Live bus nodes: (bus name, node id).
    let bus_ids: Vec<(String, u32)> = s
        .bus_members
        .keys()
        .filter_map(|bus| node_ids.get(bus).map(|id| (bus.clone(), *id)))
        .collect();

    // Live channel set: every virtual sink we created or adopted. A set, not
    // a list - it is also the membership test for the retain below.
    let channel_names: HashSet<String> = s
        .owned_sinks
        .keys()
        .chain(s.adopted_sinks.keys())
        .cloned()
        .collect();

    // Where follow-default channels go when their default has no live node
    // (unplugged, WirePlumber slow/unwilling to reassign): the best available
    // real sink, so audio fails over instead of dropping to silence.
    let fallback = fallback_sink(s);
    // Forget resolved targets for channels that no longer exist.
    s.channel_targets.retain(|name, _| channel_names.contains(name));
    // Waiters whose sink never showed up: the caller timed out long ago, so
    // holding their reply senders only leaks (see `prune_pending`).
    prune_pending(&mut s.pending_creates, Instant::now(), REQUEST_TIMEOUT);
    let port_index = index_ports(&s.ports);

    // The link plan for every live EQ insert, rebuilt from scratch each
    // pass - the link police destroys anything an EQ playback node feeds
    // that isn't in here.
    let mut eq_targets: HashMap<u32, std::collections::HashSet<u32>> = HashMap::new();

    for sink_name in &channel_names {
        let sink_name = sink_name.as_str();
        let channel_id = match node_ids.get(sink_name) {
            Some(id) => *id,
            None => continue,
        };
        // With a live EQ insert, every outgoing link (device, buses,
        // monitor) re-sources from its playback node - one coherent source,
        // so all listeners hear the same (EQ'd, equally delayed) audio.
        let source_id = resolve_source(s.eq_playback_node(sink_name), channel_id);

        // ---- output device links ----
        let explicit = s.channel_outputs.get(sink_name).cloned().flatten();
        let pinned = explicit.is_some();
        let explicit_id = explicit.as_deref().and_then(|name| node_ids.get(name).copied());
        let strict = s.channel_strict.contains(sink_name);
        let default_id = s
            .default_sink_name
            .as_ref()
            .and_then(|name| node_ids.get(name))
            .copied();
        let target_id = resolve_target(explicit_id, pinned, strict, default_id, fallback);
        // Record where this channel resolves to (even when the link set is
        // unchanged) so the UI reflects the live target, including failover.
        match target_id {
            Some(t) => {
                s.channel_targets.insert(sink_name.to_string(), t);
            }
            None => {
                s.channel_targets.remove(sink_name);
            }
        }
        if let (Some(t), true) = (target_id, source_id != channel_id) {
            eq_targets.entry(source_id).or_default().insert(t);
        }
        let pairs = target_id
            .map(|t| desired_pairs(&port_index, source_id, t))
            .unwrap_or_default();
        let current: Vec<(u32, u32)> = s
            .channel_links
            .get(sink_name)
            .map(|links| links.iter().map(|(o, i, _)| (*o, *i)).collect())
            .unwrap_or_default();
        if current != pairs {
            s.channel_links.remove(sink_name);
            if let Some(in_node) = pairs
                .first()
                .and_then(|(_, input)| s.ports.get(input).map(|p| p.node_id))
            {
                let created = create_links(&core, sink_name, source_id, in_node, &pairs);
                if !created.is_empty() {
                    s.channel_links.insert(sink_name.to_string(), created);
                }
            }
        }

        // ---- mix bus links (one set per bus, membership-gated) ----
        for (bus_name, bus_id) in &bus_ids {
            let included = s
                .bus_members
                .get(bus_name)
                .is_some_and(|members| members.contains(sink_name));
            if included && source_id != channel_id {
                eq_targets.entry(source_id).or_default().insert(*bus_id);
            }
            let pairs = if included {
                desired_pairs(&port_index, source_id, *bus_id)
            } else {
                Vec::new()
            };
            let key = (bus_name.clone(), sink_name.to_string());
            let current: Vec<(u32, u32)> = s
                .bus_links
                .get(&key)
                .map(|links| links.iter().map(|(o, i, _)| (*o, *i)).collect())
                .unwrap_or_default();
            if current != pairs {
                s.bus_links.remove(&key);
                if !pairs.is_empty() {
                    let created = create_links(&core, sink_name, source_id, *bus_id, &pairs);
                    if !created.is_empty() {
                        s.bus_links.insert(key, created);
                    }
                }
            }
        }
    }

    // ---- monitor links (listen on the default output, session scoped) ----
    let default_id = s
        .default_sink_name
        .as_ref()
        .and_then(|name| node_ids.get(name))
        .copied();
    let monitored: Vec<String> = s.monitored.iter().cloned().collect();
    for name in monitored {
        // Monitoring an EQ'd channel listens to the insert's output - the
        // same audio its device/buses hear.
        let node_id = node_ids
            .get(&name)
            .copied()
            .map(|id| resolve_source(s.eq_playback_node(&name), id));
        if let (Some(node), Some(default)) = (node_id, default_id) {
            if node_ids.get(&name).copied() != Some(node) {
                eq_targets.entry(node).or_default().insert(default);
            }
        }
        let mut pairs = match (node_id, default_id) {
            (Some(node), Some(default)) => desired_pairs(&port_index, node, default),
            _ => Vec::new(),
        };
        // A channel already playing to the default output needs no extra
        // links (and duplicates would fail) - monitoring is a no-op there.
        if let Some(existing) = s.channel_links.get(&name) {
            let existing_pairs: Vec<(u32, u32)> =
                existing.iter().map(|(o, i, _)| (*o, *i)).collect();
            if existing_pairs == pairs {
                pairs = Vec::new();
            }
        }
        let current: Vec<(u32, u32)> = s
            .monitor_links
            .get(&name)
            .map(|links| links.iter().map(|(o, i, _)| (*o, *i)).collect())
            .unwrap_or_default();
        if current != pairs {
            s.monitor_links.remove(&name);
            if !pairs.is_empty() {
                if let (Some(node), Some(default)) = (node_id, default_id) {
                    let created = create_links(&core, &name, node, default, &pairs);
                    if !created.is_empty() {
                        s.monitor_links.insert(name, created);
                    }
                }
            }
        }
    }

    // Publish the EQ link plan for the police (see on_global's Link arm).
    s.eq_desired_targets = eq_targets;
}

/// Link the mic playback stream's output ports into the virtual mic.
/// Called whenever ports appear; idempotent.
pub(super) fn ensure_mic_links(state: &Rc<RefCell<State>>) {
    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
        return;
    };
    let mut s = state.borrow_mut();
    let (Some(playback_id), Some(mic_node)) = (
        s.mic_playback_node(),
        s.node_by_name(MIC_NODE).map(|n| n.id),
    ) else {
        return;
    };
    let pairs = desired_pairs(&index_ports(&s.ports), playback_id, mic_node);
    let current: Vec<(u32, u32)> = s.mic_links.iter().map(|(o, i, _)| (*o, *i)).collect();
    if current == pairs || pairs.is_empty() {
        return;
    }
    s.mic_links.clear();
    s.mic_links = create_links(&core, "mic", playback_id, mic_node, &pairs);
}

/// Which nodes one soundboard clip has to reach: the virtual mic when the
/// chat is meant to hear it, the user's own output when they are. Pure so the
/// mapping from "only me / only the chat / both" to actual link targets is
/// testable without a graph.
fn clip_targets(
    to_mic: bool,
    to_output: bool,
    mic_id: Option<u32>,
    output_id: Option<u32>,
) -> Vec<u32> {
    let mut targets = Vec::new();
    if to_mic {
        targets.extend(mic_id);
    }
    // Never twice into the same node: with the virtual mic somehow selected
    // as the output device, two identical link sets would fight.
    if to_output {
        targets.extend(output_id.filter(|id| !targets.contains(id)));
    }
    targets
}

/// Link every playing clip into its targets. Called when a clip starts and
/// again whenever ports appear - a stream has no ports for the first few
/// milliseconds of its life, so the first pass is usually the no-op.
///
/// Idempotent, like the other reconcile passes: a clip whose links already
/// match is left alone.
pub(super) fn ensure_clip_links(state: &Rc<RefCell<State>>) {
    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
        return;
    };
    let mut s = state.borrow_mut();
    let s = &mut *s;
    if s.clips.is_empty() {
        return;
    }
    let mic_id = s.node_by_name(MIC_NODE).map(|n| n.id);
    // The user's side goes wherever their audio goes: the default output, or
    // the best real sink when the default has no live node - the same
    // resolution a follow-default channel gets, so a clip and the game it
    // interrupts come out of the same device.
    let output_id = s
        .default_sink_name
        .clone()
        .and_then(|name| s.node_by_name(&name).map(|n| n.id))
        .or_else(|| fallback_sink(s));
    // Collected first: the loop below mutates `clip_links` while it walks the
    // clips, which cannot borrow `clips` at the same time.
    let plan: Vec<(u64, u32, Vec<u32>)> = s
        .clips
        .iter()
        .filter_map(|(id, clip)| {
            let node = clip.node_id();
            // The server has not created this stream's node yet.
            if node == u32::MAX {
                return None;
            }
            Some((
                *id,
                node,
                clip_targets(clip.targets.to_mic, clip.targets.to_output, mic_id, output_id),
            ))
        })
        .collect();
    let port_index = index_ports(&s.ports);
    for (id, node, targets) in plan {
        let wanted = targets.len();
        let mut linked = 0;
        for target in targets {
            let pairs = desired_pairs(&port_index, node, target);
            if pairs.is_empty() {
                continue;
            }
            let key = (id, target);
            let current: Vec<(u32, u32)> = s
                .clip_links
                .get(&key)
                .map(|links| links.iter().map(|(o, i, _)| (*o, *i)).collect())
                .unwrap_or_default();
            if current == pairs {
                linked += 1;
                continue;
            }
            s.clip_links.remove(&key);
            let created = create_links(&core, "clip", node, target, &pairs);
            if !created.is_empty() {
                s.clip_links.insert(key, created);
                linked += 1;
            }
        }
        // Only now does the clip get to run, and only once every destination
        // it was asked for is wired up: a stream that is scheduled before its
        // links exist plays into nothing, and by the time it is connected the
        // clip is already over (see the INACTIVE note in `clip.rs`).
        if wanted > 0 && linked == wanted {
            if let Some(clip) = s.clips.get(&id) {
                clip.start();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(id: u32, node_id: u32, dir: &str, channel: Option<&str>) -> PortEntry {
        PortEntry {
            id,
            node_id,
            direction: dir.to_string(),
            channel: channel.map(str::to_string),
        }
    }

    #[test]
    fn resolve_source_prefers_live_eq_playback() {
        assert_eq!(resolve_source(Some(77), 10), 77);
    }

    #[test]
    fn resolve_source_falls_back_to_channel() {
        assert_eq!(resolve_source(None, 10), 10);
    }

    #[test]
    fn desired_pairs_matches_stereo_by_channel_not_index() {
        let mut s = State::default();
        // Monitor FL/FR on node 10, inputs FL/FR on node 20 with ids ordered
        // so a naive index pairing would cross the channels.
        s.ports.insert(1, port(1, 10, "out", Some("FL")));
        s.ports.insert(2, port(2, 10, "out", Some("FR")));
        s.ports.insert(3, port(3, 20, "in", Some("FR")));
        s.ports.insert(4, port(4, 20, "in", Some("FL")));
        let mut pairs = desired_pairs(&index_ports(&s.ports), 10, 20);
        pairs.sort_unstable();
        assert_eq!(pairs, vec![(1, 4), (2, 3)]);
    }

    #[test]
    fn desired_pairs_fans_mono_source_to_every_input() {
        let mut s = State::default();
        s.ports.insert(1, port(1, 10, "out", Some("MONO")));
        s.ports.insert(2, port(2, 20, "in", Some("FL")));
        s.ports.insert(3, port(3, 20, "in", Some("FR")));
        let mut pairs = desired_pairs(&index_ports(&s.ports), 10, 20);
        pairs.sort_unstable();
        assert_eq!(pairs, vec![(1, 2), (1, 3)]);
    }

    #[test]
    fn desired_pairs_empty_for_self_or_missing_ports() {
        let mut s = State::default();
        s.ports.insert(1, port(1, 10, "out", Some("FL")));
        let index = index_ports(&s.ports);
        assert!(desired_pairs(&index, 10, 10).is_empty(), "same node");
        assert!(desired_pairs(&index, 10, 20).is_empty(), "target has no inputs");
    }

    #[test]
    fn index_ports_groups_by_node_and_keeps_every_port() {
        let mut s = State::default();
        s.ports.insert(1, port(1, 10, "out", Some("FL")));
        s.ports.insert(2, port(2, 10, "in", Some("FL")));
        s.ports.insert(3, port(3, 20, "in", Some("FR")));
        let index = index_ports(&s.ports);
        assert_eq!(index.len(), 2);
        assert_eq!(index[&10].len(), 2, "both directions stay in the index");
        assert_eq!(index[&20].len(), 1);
        assert!(!index.contains_key(&30), "unknown node has no entry");
    }

    #[test]
    fn fallback_picks_highest_priority_real_sink() {
        let candidates = [
            (1u32, "sink_game", 10_000i64), // virtual - never a fallback
            (2, "alsa_output.hdmi", 500),
            (3, "alsa_output.analog", 900),
            (4, "alsa_output.usb", 700),
        ];
        assert_eq!(pick_fallback_sink(candidates.into_iter()), Some(3));
    }

    #[test]
    fn resolve_target_covers_the_failover_matrix() {
        // Pinned and present -> that device, failover on or off.
        assert_eq!(resolve_target(Some(7), true, false, Some(1), Some(2)), Some(7));
        assert_eq!(resolve_target(Some(7), true, true, Some(1), Some(2)), Some(7));
        // Follow-default, failover on -> default, else the fallback sink.
        assert_eq!(resolve_target(None, false, false, Some(1), Some(2)), Some(1));
        assert_eq!(resolve_target(None, false, false, None, Some(2)), Some(2));
        // Follow-default, failover off -> default only; silent when it's gone.
        assert_eq!(resolve_target(None, false, true, Some(1), Some(2)), Some(1));
        assert_eq!(resolve_target(None, false, true, None, Some(2)), None);
        // Pinned but gone, failover on -> default then fallback.
        assert_eq!(resolve_target(None, true, false, Some(1), Some(2)), Some(1));
        assert_eq!(resolve_target(None, true, false, None, Some(2)), Some(2));
        // Pinned but gone, failover off -> silence, never another device.
        assert_eq!(resolve_target(None, true, true, Some(1), Some(2)), None);
    }

    #[test]
    fn clip_targets_follow_the_chosen_destinations() {
        const MIC: u32 = 7;
        const OUT: u32 = 9;
        // Both (the default): the chat hears it and so does the user.
        assert_eq!(clip_targets(true, true, Some(MIC), Some(OUT)), vec![MIC, OUT]);
        // Only the chat - the clip is for them, not for the speakers.
        assert_eq!(clip_targets(true, false, Some(MIC), Some(OUT)), vec![MIC]);
        // Only me: an audition that must not reach the call.
        assert_eq!(clip_targets(false, true, Some(MIC), Some(OUT)), vec![OUT]);
        assert!(clip_targets(false, false, Some(MIC), Some(OUT)).is_empty());
    }

    #[test]
    fn a_missing_destination_drops_out_instead_of_taking_the_other_with_it() {
        // No mic chain running: the user still hears their own clip.
        assert_eq!(clip_targets(true, true, None, Some(9)), vec![9]);
        // No output device at all: the chat still gets it.
        assert_eq!(clip_targets(true, true, Some(7), None), vec![7]);
        assert!(clip_targets(true, true, None, None).is_empty());
    }

    #[test]
    fn a_clip_is_never_linked_into_one_node_twice() {
        // Two link sets into the same node would be a doubled clip.
        assert_eq!(clip_targets(true, true, Some(7), Some(7)), vec![7]);
    }

    #[test]
    fn fallback_is_none_when_only_virtual_channel_sinks_exist() {
        // Channel sinks are virtual and must never be a fallback target.
        // (sink_mic/sink_stream are sources, excluded by media_class before
        // reaching here - so they're not exercised at this layer.)
        let candidates = [(1u32, "sink_game", 0i64), (2, "sink_chat", 0)];
        assert_eq!(pick_fallback_sink(candidates.into_iter()), None);
    }
}
