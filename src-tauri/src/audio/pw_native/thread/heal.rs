//! The `global_remove` handler: forget what vanished, and decide whether the
//! graph needs healing. Nodes we declared `desired` are recreated on the spot
//! when something outside Inari destroyed them (another instance dying, a
//! PipeWire restart, wpctl).

use std::cell::RefCell;
use std::rc::Rc;

use log::{error, warn};

use crate::audio::pw_native::meter::MeterHandle;

use super::links::{ensure_all_links, ensure_mic_links};
use super::state::{notify_graph, State, CORE};
use super::{create_node_object, SINK_CLASS};

enum Heal {
    Nothing,
    Relink,
    Recreate(String, String, u8),
}

pub(super) fn on_global_remove(state: &Rc<RefCell<State>>, id: u32) {
    // Meter handles own capture streams; dropping one can pump
    // the loop, and a registry event re-entering `borrow_mut`
    // here would panic across the FFI boundary (= abort, no
    // unwind). So they leave the borrow before they die.
    let mut dead_meters: Vec<MeterHandle> = Vec::new();
    let heal = {
        let mut s = state.borrow_mut();
        s.links.remove(&id);
        s.ports.remove(&id);
        let Some(node) = s.nodes.remove(&id) else {
            return;
        };
        let name = node.props.get("node.name").cloned().unwrap_or_default();
        if node.media_class == SINK_CLASS {
            dead_meters.extend(s.meters.remove(&name));
            s.adopted_sinks.remove(&name);
        }
        match s.desired.get(&name).cloned() {
            Some((label, kind)) => {
                // Drop any dangling proxy so the heal isn't
                // blocked by a corpse.
                match kind {
                    0 => {
                        s.owned_sinks.remove(&name);
                    }
                    1 => {
                        s.bus_sources.remove(&name);
                    }
                    _ => {}
                }
                // A deliberate recreate (mic rename) already has
                // a fresh proxy/node - don't double up. For the
                // mic the new proxy is set synchronously before
                // this removal event, so `mic_source.is_some()`
                // can't tell our own destroy from an external one;
                // the expected-removals counter can.
                let already_back = match kind {
                    2 => {
                        if s.mic_expected_removals > 0 {
                            s.mic_expected_removals -= 1;
                            true
                        } else {
                            // External destroy (wpctl, a session
                            // hiccup): drop the dead proxy so the
                            // recreate below isn't blocked by it.
                            s.mic_source = None;
                            false
                        }
                    }
                    _ => s.node_by_name(&name).is_some(),
                };
                if already_back {
                    Heal::Relink
                } else {
                    dead_meters.extend(s.meters.remove(&name));
                    Heal::Recreate(name, label, kind)
                }
            }
            // An output device vanished: relink so affected
            // channels fail over to the default.
            None if node.media_class == SINK_CLASS => Heal::Relink,
            None => Heal::Nothing,
        }
    };
    drop(dead_meters);
    match heal {
        Heal::Recreate(name, label, kind) => {
            warn!("{name} vanished externally - recreating");
            if let Some(core) = CORE.with(|c| c.borrow().clone()) {
                match create_node_object(&core, &name, &label, kind) {
                    Ok(proxy) => {
                        let mut s = state.borrow_mut();
                        match kind {
                            0 => {
                                s.owned_sinks.insert(name, proxy);
                            }
                            1 => {
                                s.bus_sources.insert(name, proxy);
                            }
                            _ => s.mic_source = Some(proxy),
                        }
                    }
                    Err(e) => error!("recreate {name} failed: {e}"),
                }
            }
            ensure_all_links(state);
            ensure_mic_links(state);
        }
        Heal::Relink => ensure_all_links(state),
        Heal::Nothing => {}
    }
    // Getting here means a node we tracked is gone (an app closed
    // its stream, a device was unplugged): the UI's stream and
    // device lists just went stale.
    notify_graph(state);
}
