//! The PipeWire main-loop thread. All PipeWire objects live here (they are
//! not Send); the `PipeWireBackend` facade talks to this thread through a
//! pipewire channel, and each command carries an mpsc reply sender.
//!
//! The handlers are split by concern - registry events, healing, commands,
//! link reconciliation - over the one piece of shared state they all reach
//! through, `Rc<RefCell<State>>`.

mod commands;
mod heal;
mod links;
mod registry;
mod state;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use log::warn;
use pipewire as pw;
use pw::core::CoreRc;
use pw::node::Node;

use crate::audio::pw_native::levels::LevelStore;
use crate::audio::pw_native::pods;
use crate::error::SinkError;

use state::{NodeEntry, State, CORE};

use super::GraphNotify;

pub use state::Cmd;

const STREAM_CLASS: &str = "Stream/Output/Audio";
const SINK_CLASS: &str = "Audio/Sink";
const SOURCE_CLASS: &str = "Audio/Source";
const VIRTUAL_SOURCE_CLASS: &str = "Audio/Source/Virtual";
/// node.name prefix of all our internal helper streams (meters, mic chain) -
/// excluded from stream listings and node tracking.
pub const INTERNAL_PREFIX: &str = "sink-internal-";
/// node.name prefix of our meter capture streams.
pub const METER_PREFIX: &str = "sink-internal-meter-";

/// Clears the alive flag when the loop thread leaves - on a clean shutdown,
/// on an error return, and on a panic that unwinds out of `setup_and_run`.
/// A flag left set on a dead thread would put every later request back on
/// the 3s timeout path, which is exactly what it exists to avoid.
struct AliveGuard(Arc<AtomicBool>);

impl Drop for AliveGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Entry point: runs the PipeWire loop until the channel closes.
/// `init_tx` reports startup success/failure exactly once; `alive` stays
/// set for exactly as long as this thread can serve commands.
pub fn run(
    receiver: pw::channel::Receiver<Cmd>,
    init_tx: mpsc::Sender<Result<(), SinkError>>,
    levels: Arc<LevelStore>,
    graph: Arc<GraphNotify>,
    alive: Arc<AtomicBool>,
) {
    let _guard = AliveGuard(alive.clone());
    if let Err(e) = setup_and_run(receiver, &init_tx, levels, graph, &alive) {
        let _ = init_tx.send(Err(e));
    }
}

fn setup_and_run(
    receiver: pw::channel::Receiver<Cmd>,
    init_tx: &mpsc::Sender<Result<(), SinkError>>,
    levels: Arc<LevelStore>,
    graph: Arc<GraphNotify>,
    alive: &AtomicBool,
) -> Result<(), SinkError> {
    pw::init();
    let err = |stage: &str, e: pw::Error| SinkError::Config(format!("pipewire {stage}: {e}"));

    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|e| err("mainloop", e))?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|e| err("context", e))?;
    let core = context.connect_rc(None).map_err(|e| err("connect", e))?;
    let registry = core.get_registry_rc().map_err(|e| err("registry", e))?;

    CORE.with(|c| *c.borrow_mut() = Some(core.clone()));

    let state = Rc::new(RefCell::new(State {
        levels: Some(levels.clone()),
        graph: Some(graph),
        ..State::default()
    }));

    // ---- registry listeners ----
    let state_g = state.clone();
    let registry_g = registry.clone();
    let core_g = core.clone();
    let _reg_listener = registry
        .add_listener_local()
        .global(move |global| {
            registry::on_global(&state_g, &registry_g, &core_g, global);
        })
        .global_remove({
            let state = state.clone();
            move |id| heal::on_global_remove(&state, id)
        })
        .register();

    // ---- command channel ----
    let state_c = state.clone();
    let registry_c = registry.clone();
    let _recv = receiver.attach(mainloop.loop_(), move |cmd| {
        commands::handle_cmd(&state_c, &registry_c, cmd);
    });

    // Set before the owner is unblocked: it may issue commands the instant
    // `PipeWireBackend::new` returns, and those must not fail the liveness
    // check they are about to pass through.
    alive.store(true, Ordering::SeqCst);
    init_tx
        .send(Ok(()))
        .map_err(|_| SinkError::Config("backend owner vanished during init".into()))?;

    mainloop.run();
    warn!("pipewire loop exited - audio control is gone until Inari restarts");
    Ok(())
}

fn node_needs_monitor_volumes(kind: u8) -> bool {
    kind == 0 || kind == 1
}

/// The three virtual node shapes we own (kind 0=channel sink, 1=mix bus,
/// 2=virtual mic). The heal path mirrors the create handlers with this.
fn create_node_object(
    core: &CoreRc,
    name: &str,
    label: &str,
    kind: u8,
) -> Result<Node, pw::Error> {
    let class = if kind == 0 { SINK_CLASS } else { VIRTUAL_SOURCE_CLASS };
    let position = if kind == 2 { "[ MONO ]" } else { "[ FL FR ]" };
    let mut props = pw::properties::properties! {
        "factory.name" => "support.null-audio-sink",
        "node.name" => name,
        "node.description" => label,
        "media.class" => class,
        "audio.position" => position,
    };
    if node_needs_monitor_volumes(kind) {
        props.insert("monitor.channel-volumes", "true");
    }
    core.create_object::<Node>("adapter", &props)
}

fn set_props(
    entry: Option<&NodeEntry>,
    volume_percent: Option<u8>,
    mute: Option<bool>,
) -> Result<(), SinkError> {
    let Some(entry) = entry else {
        return Err(SinkError::UnknownSink("node not found".into()));
    };
    let volume = volume_percent.map(|p| (pods::percent_to_linear(p), entry.channels));
    let bytes = pods::props_pod_bytes(volume, mute)?;
    let pod = pw::spa::pod::Pod::from_bytes(&bytes)
        .ok_or_else(|| SinkError::Config("constructed an invalid pod".into()))?;
    entry
        .proxy
        .set_param(pw::spa::param::ParamType::Props, 0, pod);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_get_monitor_volumes_like_channels() {
        assert!(node_needs_monitor_volumes(0));
        assert!(node_needs_monitor_volumes(1));
        assert!(!node_needs_monitor_volumes(2));
    }
}
