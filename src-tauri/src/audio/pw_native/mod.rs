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
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pipewire as pw;

use crate::audio::backend::AudioBackend;
use crate::audio::types::{AppStream, OutputDevice};
use crate::error::SinkError;
use levels::LevelStore;
use thread::Cmd;

pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

pub struct PipeWireBackend {
    sender: Mutex<pw::channel::Sender<Cmd>>,
    /// Set while the loop thread is running; cleared by the thread on every
    /// exit path. Without it a dead loop is only visible as a 3s timeout per
    /// call - the app looks hung instead of broken.
    alive: Arc<AtomicBool>,
    /// Live per-sink peak levels, fed by the meter capture streams.
    pub levels: Arc<LevelStore>,
}

impl PipeWireBackend {
    pub fn new() -> Result<Self, SinkError> {
        let levels = Arc::new(LevelStore::new());
        let (sender, receiver) = pw::channel::channel();
        let (init_tx, init_rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(false));

        let thread_levels = levels.clone();
        let thread_alive = alive.clone();
        std::thread::Builder::new()
            .name("pipewire-loop".into())
            .spawn(move || thread::run(receiver, init_tx, thread_levels, thread_alive))
            .map_err(|e| SinkError::Config(format!("spawn pipewire thread: {e}")))?;

        match init_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                sender: Mutex::new(sender),
                alive,
                levels,
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
}
