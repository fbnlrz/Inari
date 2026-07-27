//! Registry event handling: everything that reacts to a global appearing -
//! nodes, ports, links, the default metadata - plus the take-over work a new
//! node of ours triggers (adoption, metering, EQ insert, mic streams).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use log::{error, warn};
use pipewire as pw;
use pw::core::CoreRc;
use pw::metadata::Metadata;
use pw::node::Node;
use pw::registry::{GlobalObject, RegistryRc};
use pw::spa::utils::dict::DictRef;
use pw::types::ObjectType;

use crate::audio::pw_native::eq_chain::EqChainHandle;
use crate::audio::pw_native::meter::MeterHandle;
use crate::audio::pw_native::mic::{MicStreams, MIC_NODE};
use crate::audio::pw_native::pods;
use crate::audio::types::is_virtual_sink;
use crate::persistence::buses::is_bus_name;

use super::links::{ensure_all_links, ensure_clip_links, ensure_mic_links};
use super::state::{notify_graph, NodeEntry, PortEntry, State, CORE};
use super::{
    INTERNAL_PREFIX, SINK_CLASS, SOURCE_CLASS, STREAM_CLASS, VIRTUAL_SOURCE_CLASS,
};

pub(super) fn on_global(
    state: &Rc<RefCell<State>>,
    registry: &RegistryRc,
    core: &CoreRc,
    global: &GlobalObject<&DictRef>,
) {
    match global.type_ {
        ObjectType::Node => on_node(state, registry, core, global),
        ObjectType::Port => {
            let Some(props) = global.props else { return };
            let Some(node_id) = props.get("node.id").and_then(|v| v.parse().ok()) else {
                return;
            };
            let entry = PortEntry {
                id: global.id,
                node_id,
                direction: props.get("port.direction").unwrap_or_default().to_string(),
                channel: props.get("audio.channel").map(str::to_string),
            };
            state.borrow_mut().ports.insert(global.id, entry);
            // Channel, mic and clip wiring all depend on ports of untracked
            // stream nodes (EQ/mic/clip playback streams), so reconcile on
            // every port event - all three are idempotent no-ops until both
            // ends exist. A clip's ports arrive here a few milliseconds after
            // it was started, and this is where it gets connected.
            ensure_all_links(state);
            ensure_mic_links(state);
            ensure_clip_links(state);
        }
        ObjectType::Link => {
            let Some(props) = global.props else { return };
            let out = props.get("link.output.node").and_then(|v| v.parse().ok());
            let inp = props.get("link.input.node").and_then(|v| v.parse().ok());
            if let (Some(out), Some(inp)) = (out, inp) {
                let police = {
                    let mut s = state.borrow_mut();
                    s.links.insert(global.id, (out, inp));
                    // Police the mic playback stream: if anything (e.g. a
                    // session-manager fallback) links it somewhere other
                    // than the virtual mic, destroy that link - mic audio
                    // must never leak into the speakers.
                    let mic_stray = match (s.mic_playback_node(), s.node_by_name(MIC_NODE)) {
                        (Some(playback), mic) if out == playback => {
                            mic.map(|n| n.id) != Some(inp)
                        }
                        _ => false,
                    };
                    // Same policing for EQ playback streams: only the links
                    // the loop planned (device/buses/monitor) may exist. An
                    // EQ node with no plan yet (chain just built, first
                    // reconcile pending) allows nothing - our own links are
                    // always created after the plan is recorded.
                    let eq_stray = s
                        .eq_streams
                        .values()
                        .any(|h| h.playback_node_id() == out)
                        && !s
                            .eq_desired_targets
                            .get(&out)
                            .is_some_and(|allowed| allowed.contains(&inp));
                    mic_stray || eq_stray
                };
                if police {
                    let _ = registry.destroy_global(global.id);
                }
            }
        }
        ObjectType::Metadata => {
            let Some(props) = global.props else { return };
            if props.get("metadata.name") != Some("default") {
                return;
            }
            let Ok(metadata) = registry.bind::<Metadata, _>(global) else {
                return;
            };
            let state_m = state.clone();
            let listener = metadata
                .add_listener_local()
                .property(move |_subject, key, _type, value| {
                    // values are JSON like {"name":"alsa_output...."}
                    let parse_name = |v: Option<&str>| {
                        v.and_then(|v| {
                            serde_json::from_str::<serde_json::Value>(v)
                                .ok()?
                                .get("name")?
                                .as_str()
                                .map(str::to_string)
                        })
                    };
                    if key == Some("default.audio.sink") {
                        let name = parse_name(value);
                        let changed = {
                            let mut s = state_m.borrow_mut();
                            let changed = s.default_sink_name != name;
                            s.default_sink_name = name;
                            changed
                        };
                        // Channels following the default must relink
                        // (Sonar-style automatic device failover).
                        if changed {
                            ensure_all_links(&state_m);
                            // Relinking moved every follow-default channel:
                            // the UI's resolved outputs are stale.
                            notify_graph(&state_m);
                        }
                    } else if key == Some("default.audio.source") {
                        let name = parse_name(value);
                        let (changed, rebuild) = {
                            let mut s = state_m.borrow_mut();
                            let changed = s.default_source_name != name;
                            s.default_source_name = name;
                            // A follow-default mic chain is pinned to the
                            // resolved device (dont-reconnect), so it
                            // tracks default changes by rebuilding.
                            let rebuild = changed
                                && s.mic_config.enabled
                                && s.mic_config.input_device.is_none()
                                && s.mic_streams.is_some();
                            (changed, rebuild)
                        };
                        if changed {
                            notify_graph(&state_m);
                        }
                        if rebuild {
                            // Tear the old chain down outside the borrow -
                            // stream drops can pump the loop (see below).
                            let old = state_m.borrow_mut().mic_streams.take();
                            drop(old);
                            build_mic_streams(&state_m);
                        }
                    }
                    0
                })
                .register();
            let mut s = state.borrow_mut();
            s.metadata = Some(metadata);
            s._metadata_listener = Some(listener);
        }
        _ => {}
    }
}

fn on_node(
    state: &Rc<RefCell<State>>,
    registry: &RegistryRc,
    core: &CoreRc,
    global: &GlobalObject<&DictRef>,
) {
    let Some(dict) = global.props else { return };
    let media_class = dict.get("media.class").unwrap_or_default().to_string();
    if media_class != STREAM_CLASS
        && media_class != SINK_CLASS
        && media_class != SOURCE_CLASS
        && media_class != VIRTUAL_SOURCE_CLASS
    {
        return;
    }
    let props: HashMap<String, String> = dict
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let node_name = props.get("node.name").cloned().unwrap_or_default();
    // Never track our own internal helper streams (meters, mic chain).
    if node_name.starts_with(INTERNAL_PREFIX) {
        return;
    }

    let Ok(proxy) = registry.bind::<Node, _>(global) else {
        return;
    };

    // Track volume/mute through Props param events, and the running state
    // through info events (drives the app-list activity indicator).
    let state_p = state.clone();
    let state_i = state.clone();
    let node_id = global.id;
    let listener = proxy
        .add_listener_local()
        .info(move |info| {
            let running = matches!(info.state(), pw::node::NodeState::Running);
            let changed = {
                let mut s = state_i.borrow_mut();
                let Some(entry) = s.nodes.get_mut(&node_id) else {
                    return;
                };
                let mut changed = entry.active != running;
                entry.active = running;
                // Registry globals only carry an abbreviated prop set; the
                // info event has the full dict (e.g. application.process.
                // binary, needed to name Discord's "WEBRTC VoiceEngine").
                if let Some(props) = info.props() {
                    for (k, v) in props.iter() {
                        let previous = entry.props.insert(k.to_string(), v.to_string());
                        changed |= previous.as_deref() != Some(v);
                    }
                }
                changed
            };
            // Both halves are visible in the UI: the activity dot follows the
            // running state, and the display name only settles once the full
            // prop dict lands here. Nothing to say when neither moved.
            if changed {
                notify_graph(&state_i);
            }
        })
        .param(move |_seq, id, _index, _next, param| {
            if id != pw::spa::param::ParamType::Props {
                return;
            }
            let Some(pod) = param else { return };
            let parsed = pods::parse_props(pod);
            let mut s = state_p.borrow_mut();
            if let Some(entry) = s.nodes.get_mut(&node_id) {
                if let Some(linear) = parsed.volume_linear {
                    entry.volume_percent = pods::linear_to_percent(linear);
                    // From here on the fields are readings, not placeholders -
                    // `observed_state` may report them.
                    entry.props_seen = true;
                }
                if let Some(channels) = parsed.channels {
                    entry.channels = channels;
                }
                if let Some(muted) = parsed.muted {
                    entry.muted = muted;
                    entry.props_seen = true;
                }
            }
        })
        .register();
    proxy.subscribe_params(&[pw::spa::param::ParamType::Props]);

    let entry = NodeEntry {
        id: global.id,
        serial: props.get("object.serial").and_then(|v| v.parse().ok()),
        media_class: media_class.clone(),
        props,
        proxy,
        _listener: listener,
        volume_percent: 100,
        channels: 2,
        muted: false,
        props_seen: false,
        active: false,
    };

    let adopt = {
        let mut s = state.borrow_mut();
        s.nodes.insert(global.id, entry);
        media_class == SINK_CLASS && should_adopt_sink(&node_name, s.expected_sinks())
    };
    // A node we track appeared: an app started streaming, a device was
    // plugged in, or one of our own sinks came up. Announced before the
    // linking below - the coalescing window outlives that work by far, so
    // the UI still refetches after the graph has settled.
    notify_graph(state);

    if media_class == SINK_CLASS {
        // A sink we asked for came up: resolve pending create requests, then
        // take it over (teardown bookkeeping, meter, EQ insert). Foreign
        // sinks - including ones that merely carry the `sink_` prefix - only
        // matter as possible link targets.
        let waiters = state.borrow_mut().pending_creates.remove(&node_name);
        for reply in waiters.into_iter().flat_map(|(_, replies)| replies) {
            let _ = reply.send(Ok(()));
        }
        if adopt {
            adopt_sink(state, core, &node_name, global.id);
        }
        // A new hardware sink may also be the (returning) target of a channel.
        ensure_all_links(state);
        return;
    }

    // The virtual mic source came up: attach the DSP streams.
    if media_class == VIRTUAL_SOURCE_CLASS && node_name == MIC_NODE {
        build_mic_streams(state);
        return;
    }

    // A mix bus came up: meter it (direct source capture) and link members.
    if media_class == VIRTUAL_SOURCE_CLASS && is_bus_name(&node_name) {
        attach_meter(state, core, &node_name, global.id, false);
        ensure_all_links(state);
    }
}

/// Whether a sink global is one of ours to adopt. Pure so the rule is
/// unit-testable: the `sink_` prefix is a *namespace* check, not proof of
/// ownership. A foreign node that happens to carry it (and to collide with a
/// channel name) must be left alone - an adopted sink is one `DestroySink`
/// away from `registry.destroy_global`, i.e. from us killing someone else's
/// node. `expected` is the set of sinks we created, have a create in flight
/// for, or were told to keep alive; that still covers adoption of sinks that
/// outlived a previous run, which is what the prefix test was reaching for.
fn should_adopt_sink<'a>(name: &str, expected: impl IntoIterator<Item = &'a str>) -> bool {
    is_virtual_sink(name) && expected.into_iter().any(|e| e == name)
}

/// Attach a level meter to a live node, unless it already has one.
///
/// The stream is built *outside* the state borrow: `Stream::connect` can pump
/// the loop, which re-enters the registry callbacks - and a `borrow_mut`
/// panic inside an FFI callback aborts the process instead of unwinding.
fn attach_meter(
    state: &Rc<RefCell<State>>,
    core: &CoreRc,
    name: &str,
    id: u32,
    capture_sink: bool,
) {
    let (missing, levels) = {
        let s = state.borrow();
        (!s.meters.contains_key(name), s.levels.clone())
    };
    let (true, Some(levels)) = (missing, levels) else {
        return;
    };
    match MeterHandle::new(core, name, id, levels, capture_sink) {
        Ok(meter) => {
            // A meter that appeared meanwhile is replaced, and the old one
            // dropped once the borrow is gone (see above).
            let old = state.borrow_mut().meters.insert(name.to_string(), meter);
            drop(old);
        }
        Err(e) => warn!("meter for {name} failed: {e}"),
    }
}

/// Take over a live virtual sink of ours: remember it for teardown unless we
/// hold its proxy already, meter it, and build its EQ insert if one is
/// configured. Called both when the global appears and when `CreateSink`
/// finds the node already there (leftovers from a previous run).
pub(super) fn adopt_sink(state: &Rc<RefCell<State>>, core: &CoreRc, name: &str, id: u32) {
    let eq_config = {
        let mut s = state.borrow_mut();
        if !s.owned_sinks.contains_key(name) {
            s.adopted_sinks.insert(name.to_string(), id);
        }
        // An enabled EQ config with no live insert: build it against the
        // fresh sink id. Covers both startup (config loaded before the sink
        // exists) and the heal path (sink recreated after an external
        // destroy) with the same hook - like the meter below.
        if s.eq_streams.contains_key(name) {
            None
        } else {
            s.eq_configs.get(name).filter(|c| c.enabled).cloned()
        }
    };
    attach_meter(state, core, name, id, true);
    if let Some(config) = eq_config {
        // Two streams plus listeners - constructed outside the borrow for
        // the same reason as the meter.
        match EqChainHandle::new(core, name, id, &config) {
            Ok(handle) => {
                let old = state.borrow_mut().eq_streams.insert(name.to_string(), handle);
                drop(old);
            }
            Err(e) => error!("eq chain for {name} failed: {e}"),
        }
    }
}

/// (Re)build the mic capture/DSP/playback streams. The loop links the
/// playback stream to the virtual source by name, so no id is needed.
pub(super) fn build_mic_streams(state: &Rc<RefCell<State>>) {
    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
        return;
    };
    let (config, mic_target, levels) = {
        let s = state.borrow();
        if !s.mic_config.enabled {
            return;
        }
        // Resolve "follow default" to the actual hardware source at build
        // time - the capture must be pinned (and must never point at our own
        // virtual mic, or the chain would eat its own output).
        let mic_target = s.mic_config.input_device.clone().or_else(|| {
            s.default_source_name
                .clone()
                .filter(|name| name != MIC_NODE)
        });
        let Some(levels) = s.levels.clone() else { return };
        (s.mic_config.clone(), mic_target, levels)
    };
    // Built (and any predecessor dropped) outside the borrow: stream
    // construction and teardown pump the loop, and a registry event
    // re-entering `borrow_mut` would panic across FFI = abort.
    match MicStreams::new(&core, &config, mic_target.as_deref(), levels) {
        Ok(streams) => {
            let old = {
                let mut s = state.borrow_mut();
                s.mic_links.clear();
                // A fresh chain starts un-ducked. If a clip is playing right
                // now (a device change mid-clip), that would pop the mic back
                // to full level under it - so re-apply what the soundboard
                // last asked for.
                streams.params.set_duck(s.mic_duck.0);
                s.mic_streams.replace(streams)
            };
            drop(old);
        }
        Err(e) => error!("mic chain failed: {e}"),
    }
    ensure_mic_links(state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adoption_needs_ownership_not_a_matching_prefix() {
        // The whole point: a foreign node called `sink_*` - even one that
        // collides with a channel name we don't (yet) expect - is not ours.
        assert!(should_adopt_sink("sink_game", ["sink_game", "sink_chat"]));
        assert!(!should_adopt_sink("sink_game", ["sink_chat"]));
        assert!(!should_adopt_sink("sink_game", []));
        // Namespace check still applies on top of the expectation.
        assert!(!should_adopt_sink("sink_mic", ["sink_mic"]), "reserved");
        assert!(!should_adopt_sink("alsa_output.pci", ["alsa_output.pci"]));
    }
}
