//! The command handler: everything the `PipeWireBackend` facade asks for,
//! executed on the loop thread. One arm per `Cmd`, each ending in a reply.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use pipewire as pw;
use pw::node::Node;
use pw::registry::RegistryRc;

use crate::audio::pw_native::eq_chain::EqChainHandle;
use crate::audio::pw_native::mic::{MicStreams, MIC_NODE};
use crate::audio::types::{is_virtual_sink, AppStream, OutputDevice};
use crate::error::SinkError;
use crate::persistence::buses::is_bus_name;

use super::links::ensure_all_links;
use super::registry::{adopt_sink, build_mic_streams};
use super::state::{Cmd, State, CORE};
use super::{
    create_node_object, set_props, SINK_CLASS, SOURCE_CLASS, STREAM_CLASS, VIRTUAL_SOURCE_CLASS,
};

pub(super) fn handle_cmd(state: &Rc<RefCell<State>>, registry: &RegistryRc, cmd: Cmd) {
    match cmd {
        Cmd::CreateSink { name, label, reply } => {
            // Validated before anything else: both branches below end with
            // the node being ours - and `DestroySink` destroys an adopted
            // global outright, so a foreign name must never get that far.
            if !is_virtual_sink(&name) {
                let _ = reply.send(Err(SinkError::UnknownSink(name)));
                return;
            }
            let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                let _ = reply.send(Err(SinkError::Config(
                    "sink creation requires a live core".into(),
                )));
                return;
            };
            let existing = state.borrow().node_by_name(&name).map(|n| n.id);
            if let Some(id) = existing {
                // Already exists (leftover from a previous run, or a pactl
                // module). The registry handler saw it before it was one of
                // ours and left it alone, so take it over here.
                state.borrow_mut().desired.insert(name.clone(), (label, 0));
                adopt_sink(state, &core, &name, id);
                ensure_all_links(state);
                let _ = reply.send(Ok(()));
                return;
            }
            match core.create_object::<Node>(
                "adapter",
                &pw::properties::properties! {
                    "factory.name" => "support.null-audio-sink",
                    "node.name" => name.as_str(),
                    "node.description" => label.as_str(),
                    "media.class" => SINK_CLASS,
                    "audio.position" => "[ FL FR ]",
                    "monitor.channel-volumes" => "true",
                },
            ) {
                // The created proxy must be kept alive until teardown. The
                // reply fires when the global appears in the registry.
                Ok(proxy) => {
                    let mut s = state.borrow_mut();
                    s.owned_sinks.insert(name.clone(), proxy);
                    s.desired.insert(name.clone(), (label, 0));
                    s.pending_creates
                        .entry(name)
                        .or_insert_with(|| (Instant::now(), Vec::new()))
                        .1
                        .push(reply);
                }
                Err(e) => {
                    let _ = reply.send(Err(SinkError::Config(format!("create sink: {e}"))));
                }
            }
        }
        Cmd::DestroySink { name, reply } => {
            // The EQ insert goes first, so its capture target doesn't vanish
            // under it mid-teardown - but both it and the meter own capture
            // streams whose drop pumps the loop, so they leave the borrow
            // before they die (a registry event re-entering `borrow_mut`
            // panics inside an FFI callback, which aborts the process).
            let doomed = {
                let mut s = state.borrow_mut();
                (s.eq_streams.remove(&name), s.meters.remove(&name))
            };
            drop(doomed);
            let mut s = state.borrow_mut();
            s.desired.remove(&name);
            s.eq_configs.remove(&name);
            s.channel_links.remove(&name);
            s.bus_links.retain(|(_, ch), _| ch != &name);
            s.channel_outputs.remove(&name);
            if let Some(levels) = &s.levels {
                levels.release(&name);
            }
            if let Some(proxy) = s.owned_sinks.remove(&name) {
                match CORE.with(|c| c.borrow().clone()) {
                    Some(core) => {
                        let _ = core.destroy_object(proxy);
                        let _ = reply.send(Ok(()));
                    }
                    None => {
                        let _ = reply.send(Err(SinkError::Config("core is gone".into())));
                    }
                }
            } else if let Some(id) = s.adopted_sinks.remove(&name) {
                let _ = registry.destroy_global(id);
                let _ = reply.send(Ok(()));
            } else {
                // Nothing to destroy - idempotent success.
                let _ = reply.send(Ok(()));
            }
        }
        Cmd::ListStreams { reply } => {
            let s = state.borrow();
            let streams = s
                .nodes
                .values()
                .filter(|n| n.media_class == STREAM_CLASS)
                .map(|n| {
                    let (app_name, match_prop, match_value) =
                        crate::audio::types::resolve_identity(|key| n.props.get(key).cloned());
                    AppStream {
                        index: n.id,
                        app_name,
                        match_prop,
                        match_value,
                        alias: None,
                        icon_name: n.props.get("application.icon-name").cloned(),
                        icon_path: None,
                        pid: n
                            .props
                            .get("application.process.id")
                            .and_then(|v| v.parse().ok()),
                        assigned_sink: s
                            .sink_of_stream(n.id)
                            .and_then(|sink| sink.props.get("node.name"))
                            .filter(|name| is_virtual_sink(name))
                            .cloned(),
                        volume_percent: n.volume_percent,
                        muted: n.muted,
                        active: n.active,
                    }
                })
                .collect();
            let _ = reply.send(Ok(streams));
        }
        Cmd::ListOutputs { reply } => {
            let s = state.borrow();
            let outputs = s
                .nodes
                .values()
                .filter(|n| {
                    n.media_class == SINK_CLASS
                        && !n
                            .props
                            .get("node.name")
                            .is_some_and(|name| is_virtual_sink(name))
                })
                .map(|n| OutputDevice {
                    index: n.id,
                    name: n.props.get("node.name").cloned().unwrap_or_default(),
                    description: n
                        .props
                        .get("node.description")
                        .or_else(|| n.props.get("node.nick"))
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect();
            let _ = reply.send(Ok(outputs));
        }
        Cmd::ResolvedOutputs { reply } => {
            let s = state.borrow();
            let resolved = s
                .owned_sinks
                .keys()
                .chain(s.adopted_sinks.keys())
                .map(|name| {
                    let device = s
                        .channel_targets
                        .get(name)
                        .and_then(|id| s.nodes.get(id))
                        .and_then(|n| n.props.get("node.name").cloned());
                    (name.clone(), device)
                })
                .collect();
            let _ = reply.send(Ok(resolved));
        }
        Cmd::SinkState { name, reply } => {
            let s = state.borrow();
            let _ = reply.send(Ok(s.observed_state(&name)));
        }
        Cmd::SetNodeVolumeByName { name, percent, reply } => {
            let s = state.borrow();
            let _ = reply.send(set_props(s.node_by_name(&name), Some(percent), None));
        }
        Cmd::SetNodeMuteByName { name, muted, reply } => {
            let s = state.borrow();
            let _ = reply.send(set_props(s.node_by_name(&name), None, Some(muted)));
        }
        Cmd::SetNodeVolumeById { id, percent, reply } => {
            let s = state.borrow();
            let _ = reply.send(set_props(s.nodes.get(&id), Some(percent), None));
        }
        Cmd::CreateBus { name, label, reply } => {
            let mut s = state.borrow_mut();
            if s.bus_sources.contains_key(&name) || s.node_by_name(&name).is_some() {
                s.desired.insert(name, (label, 1)); // adopted - keep alive
                let _ = reply.send(Ok(()));
                return;
            }
            let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                let _ = reply.send(Err(SinkError::Config("core is gone".into())));
                return;
            };
            match create_node_object(&core, &name, &label, 1) {
                Ok(proxy) => {
                    s.desired.insert(name.clone(), (label, 1));
                    s.bus_sources.insert(name, proxy);
                    let _ = reply.send(Ok(()));
                }
                Err(e) => {
                    let _ = reply.send(Err(SinkError::Config(format!("create bus: {e}"))));
                }
            }
        }
        Cmd::DestroyBus { name, reply } => {
            // Capture stream - dropped outside the borrow (see DestroySink).
            let doomed = state.borrow_mut().meters.remove(&name);
            drop(doomed);
            let mut s = state.borrow_mut();
            s.desired.remove(&name);
            s.bus_members.remove(&name);
            s.bus_links.retain(|(bus, _), _| bus != &name);
            if let Some(levels) = &s.levels {
                levels.release(&name);
            }
            if let Some(proxy) = s.bus_sources.remove(&name) {
                if let Some(core) = CORE.with(|c| c.borrow().clone()) {
                    let _ = core.destroy_object(proxy);
                }
            }
            let _ = reply.send(Ok(()));
        }
        Cmd::SetBusMembers { name, channels, reply } => {
            state
                .borrow_mut()
                .bus_members
                .insert(name, channels.into_iter().collect());
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetMonitor { name, enabled, reply } => {
            {
                let mut s = state.borrow_mut();
                if enabled {
                    s.monitored.insert(name);
                } else {
                    s.monitored.remove(&name);
                    s.monitor_links.remove(&name);
                }
            }
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetChannelEq { sink_name, config, reply } => {
            if !is_virtual_sink(&sink_name) {
                let _ = reply.send(Err(SinkError::UnknownSink(sink_name)));
                return;
            }
            let (needs_create, needs_destroy) = {
                let mut s = state.borrow_mut();
                s.eq_configs.insert(sink_name.clone(), config.clone());
                // Live re-tune: band edits reach the RT thread through the
                // params atomics, no relink and no audible gap.
                if let Some(handle) = s.eq_streams.get(&sink_name) {
                    handle.params.apply(&config);
                }
                let live = s.eq_streams.contains_key(&sink_name);
                (config.enabled && !live, !config.enabled && live)
            };
            if needs_destroy {
                // Two streams plus listeners; dropped outside the borrow
                // (see DestroySink).
                let doomed = state.borrow_mut().eq_streams.remove(&sink_name);
                drop(doomed);
            } else if needs_create {
                let sink_id = state
                    .borrow()
                    .node_by_name(&sink_name)
                    .map(|n| n.id);
                if let Some(sink_id) = sink_id {
                    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                        let _ = reply.send(Err(SinkError::Config(
                            "eq chain requires a live core".into(),
                        )));
                        return;
                    };
                    match EqChainHandle::new(&core, &sink_name, sink_id, &config) {
                        Ok(handle) => {
                            let old =
                                state.borrow_mut().eq_streams.insert(sink_name.clone(), handle);
                            drop(old);
                        }
                        Err(e) => {
                            let _ = reply.send(Err(e));
                            return;
                        }
                    }
                }
                // Sink not live yet (e.g. mid-profile-load): the on_node
                // hook builds the chain from eq_configs when it appears.
            }
            // Re-source the channel's links from/to the insert.
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetMicConfig { config, reply } => {
            // The mic chain's streams are dropped after the borrow is
            // released - see DestroySink for why that matters.
            let mut dead_mic: Option<MicStreams> = None;
            let (needs_create, needs_destroy, needs_rebuild, source_exists, orphaned) = {
                let mut s = state.borrow_mut();
                let prev = s.mic_config.clone();
                s.mic_config = config.clone();

                // Live-tunable params apply without a rebuild.
                if let Some(streams) = &s.mic_streams {
                    streams.params.apply(&config);
                }

                // Renaming the published mic recreates the node so other
                // apps see the new description immediately.
                let needs_recreate = config.enabled
                    && s.mic_source.is_some()
                    && prev.output_label != config.output_label;
                let mut orphaned: Vec<u32> = Vec::new();
                if needs_recreate {
                    // Remember who was capturing the mic (Discord, OBS …) -
                    // destroying the node drops them onto the fallback
                    // source, and they'd silently stay there.
                    if let Some(mic) = s.node_by_name(MIC_NODE) {
                        let mic_id = mic.id;
                        orphaned = s
                            .links
                            .values()
                            .filter(|(out, _)| *out == mic_id)
                            .map(|(_, input)| *input)
                            // Tracked nodes here are devices (monitor
                            // targets) - foreign capture streams aren't in
                            // the mirror.
                            .filter(|input| !s.nodes.contains_key(input))
                            .collect();
                    }
                    dead_mic = s.mic_streams.take();
                    s.mic_links.clear();
                    if let Some(proxy) = s.mic_source.take() {
                        // Our own destroy - the heal path should expect this
                        // removal rather than treat it as external and race a
                        // second recreate.
                        s.mic_expected_removals += 1;
                        if let Some(core) = CORE.with(|c| c.borrow().clone()) {
                            let _ = core.destroy_object(proxy);
                        }
                    }
                }

                let needs_create = config.enabled && s.mic_source.is_none();
                let needs_destroy = !config.enabled && s.mic_source.is_some();
                let needs_rebuild = config.enabled
                    && s.mic_streams.is_some()
                    && prev.input_device != config.input_device;
                let source_exists = s.node_by_name(MIC_NODE).is_some();
                (needs_create, needs_destroy, needs_rebuild, source_exists, orphaned)
            };
            drop(dead_mic);

            if needs_destroy {
                let (streams, proxy) = {
                    let mut s = state.borrow_mut();
                    s.desired.remove(MIC_NODE);
                    s.mic_links.clear();
                    (s.mic_streams.take(), s.mic_source.take())
                };
                drop(streams);
                if let Some(proxy) = proxy {
                    if let Some(core) = CORE.with(|c| c.borrow().clone()) {
                        let _ = core.destroy_object(proxy);
                    }
                }
                let _ = reply.send(Ok(()));
                return;
            }

            if needs_create {
                let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                    let _ = reply.send(Err(SinkError::Config("core is gone".into())));
                    return;
                };
                match core.create_object::<Node>(
                    "adapter",
                    &pw::properties::properties! {
                        "factory.name" => "support.null-audio-sink",
                        "node.name" => MIC_NODE,
                        "node.description" => config.output_label.as_str(),
                        "media.class" => VIRTUAL_SOURCE_CLASS,
                        "audio.position" => "[ MONO ]",
                    },
                ) {
                    Ok(proxy) => {
                        let mut s = state.borrow_mut();
                        s.mic_source = Some(proxy);
                        s.desired
                            .insert(MIC_NODE.to_string(), (config.output_label.clone(), 2));
                        // Re-point streams that were capturing the old node
                        // (target.object by name survives the recreation -
                        // the session manager re-attaches them when the new
                        // global appears). Type stays None deliberately:
                        // that's what `pw-metadata <id> target.object <name>`
                        // sets, and WirePlumber matches the value against
                        // serials first, node names second, regardless of
                        // the annotation. Spa:Id (used for serial-based
                        // moves elsewhere) would be wrong for a name.
                        if let Some(meta) = &s.metadata {
                            for id in &orphaned {
                                meta.set_property(*id, "target.object", None, Some(MIC_NODE));
                            }
                        }
                        // Streams attach when the global appears (on_node).
                    }
                    Err(e) => {
                        let _ =
                            reply.send(Err(SinkError::Config(format!("create mic source: {e}"))));
                        return;
                    }
                }
            } else if needs_rebuild {
                let old = state.borrow_mut().mic_streams.take();
                drop(old);
                if source_exists {
                    build_mic_streams(state);
                }
            } else if config.enabled && source_exists {
                // Source exists but streams may be missing (earlier failure
                // or config re-applied at startup) - attach if needed.
                let missing = state.borrow().mic_streams.is_none();
                if missing {
                    build_mic_streams(state);
                }
            }
            let _ = reply.send(Ok(()));
        }
        Cmd::ListInputs { reply } => {
            let s = state.borrow();
            let inputs = s
                .nodes
                .values()
                .filter(|n| {
                    let name = n.props.get("node.name").map(String::as_str);
                    n.media_class == SOURCE_CLASS
                        || (n.media_class == VIRTUAL_SOURCE_CLASS
                            && name != Some(MIC_NODE)
                            && !name.is_some_and(is_bus_name))
                })
                .map(|n| OutputDevice {
                    index: n.id,
                    name: n.props.get("node.name").cloned().unwrap_or_default(),
                    description: n
                        .props
                        .get("node.description")
                        .or_else(|| n.props.get("node.nick"))
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect();
            let _ = reply.send(Ok(inputs));
        }
        Cmd::GetDefaults { reply } => {
            let s = state.borrow();
            let _ = reply.send(Ok((
                s.default_sink_name.clone(),
                s.default_source_name.clone(),
            )));
        }
        Cmd::SetDefault { input, name, reply } => {
            let s = state.borrow();
            let Some(metadata) = s.metadata.as_ref() else {
                let _ = reply.send(Err(SinkError::Config(
                    "no default metadata object (is WirePlumber running?)".into(),
                )));
                return;
            };
            // The same mechanism wpctl uses: WirePlumber watches the
            // configured keys and applies + persists the choice.
            let key = if input {
                "default.configured.audio.source"
            } else {
                "default.configured.audio.sink"
            };
            // Build the Spa:String:JSON value with serde so backslashes,
            // quotes and control chars are all escaped - a hand-rolled
            // format! that only escaped `"` let a name ending in `\` break
            // out of the quoted string and inject metadata keys (TD-018).
            let value = serde_json::json!({ "name": name }).to_string();
            metadata.set_property(0, key, Some("Spa:String:JSON"), Some(&value));
            let _ = reply.send(Ok(()));
        }
        Cmd::SetChannelOutput { sink_name, output_name, reply } => {
            if !is_virtual_sink(&sink_name) {
                let _ = reply.send(Err(SinkError::UnknownSink(sink_name)));
                return;
            }
            state
                .borrow_mut()
                .channel_outputs
                .insert(sink_name, output_name);
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetChannelFailover { sink_name, enabled, reply } => {
            if !is_virtual_sink(&sink_name) {
                let _ = reply.send(Err(SinkError::UnknownSink(sink_name)));
                return;
            }
            {
                let mut s = state.borrow_mut();
                if enabled {
                    s.channel_strict.remove(&sink_name);
                } else {
                    s.channel_strict.insert(sink_name);
                }
            }
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::MoveStream { id, sink_name, reply } => {
            let s = state.borrow();
            let Some(metadata) = s.metadata.as_ref() else {
                let _ = reply.send(Err(SinkError::Config(
                    "no default metadata object (is WirePlumber running?)".into(),
                )));
                return;
            };
            // Empty sink name = back to the default device.
            let target = if sink_name.is_empty() {
                s.default_sink_name.clone()
            } else {
                Some(sink_name.clone())
            };
            let serial = target
                .as_deref()
                .and_then(|name| s.node_by_name(name))
                .and_then(|n| n.serial);
            match serial {
                Some(serial) => {
                    metadata.set_property(
                        id,
                        "target.object",
                        Some("Spa:Id"),
                        Some(&serial.to_string()),
                    );
                    // Clear any stale low-level target left by other tools.
                    metadata.set_property(id, "target.node", None, None);
                    let _ = reply.send(Ok(()));
                }
                None => {
                    let _ = reply.send(Err(SinkError::UnknownSink(
                        target.unwrap_or_else(|| "<default>".into()),
                    )));
                }
            }
        }
    }
}
