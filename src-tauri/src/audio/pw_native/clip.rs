//! Soundboard clip playback: one decoded buffer, one short-lived stream.
//!
//! Topology:  clip buffer ──▶ playback stream ──▶ sink_mic (the chat)
//!                                            └─▶ default output (the user)
//!
//! **The clip does not go through the mic chain.** Feeding it into the
//! microphone input would put it through the DSP chain in `mic.rs`, where the
//! noise gate chops it up the moment it gets quiet and the compressor squashes
//! whatever survives. It is published straight into `sink_mic` instead, next to
//! the already-processed voice - PipeWire mixes several inputs into one node by
//! itself, which is exactly what a soundboard wants.
//!
//! The stream carries `node.autoconnect=false` and connects without
//! AUTOCONNECT for the same reason the mic and EQ playback streams do:
//! WirePlumber routes playback streams to the default *sink*, and a stream
//! meant for a virtual source would never reach it (that is measurable -
//! `pw-play --target sink_mic` lands on the speakers). The loop thread creates
//! both sets of links itself.

use std::sync::Arc;

use pipewire as pw;
use pw::spa;
use spa::pod::Pod;

use crate::audio::types::{ClipPcm, ClipTargets};
use crate::error::SinkError;

/// node.name prefix of clip streams. Under `INTERNAL_PREFIX`, so a firing clip
/// never shows up as an app in the mixer's stream list.
pub const CLIP_PREFIX: &str = "sink-internal-clip-";

struct ClipCtx {
    samples: Arc<Vec<f32>>,
    /// Read cursor in samples (not frames).
    pos: usize,
    gain: f32,
    channels: usize,
}

pub struct ClipStream {
    stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<ClipCtx>,
    pub targets: ClipTargets,
    /// Whether `start` has already been called. `Cell` because everything
    /// here lives on the loop thread.
    started: std::cell::Cell<bool>,
}

impl ClipStream {
    /// Node id of the stream - the loop links its output ports to the virtual
    /// mic and/or the output device. `u32::MAX` until the server has created
    /// the node (callers filter it, like `mic_playback_node`).
    pub fn node_id(&self) -> u32 {
        self.stream.node_id()
    }

    /// Let the clip run. Called once its links are up (see the INACTIVE note
    /// in `new`); calling it again is a no-op the loop relies on, because the
    /// link pass runs on every port event.
    pub fn start(&self) {
        if self.started.replace(true) {
            return;
        }
        if let Err(e) = self.stream.set_active(true) {
            log::warn!("clip did not start: {e}");
        }
    }
}

/// Format pod for a clip: the file's own rate, its channel count, and named
/// channel positions so the loop can pair ports by `audio.channel` instead of
/// by index. PipeWire resamples a 44.1 kHz clip into a 48 kHz graph itself.
fn clip_format(rate: u32, channels: u16) -> Result<Vec<u8>, SinkError> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(rate);
    info.set_channels(u32::from(channels));
    let mut position = [0u32; spa::param::audio::MAX_CHANNELS];
    if channels == 1 {
        position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
    } else {
        position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
        position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    }
    info.set_position(position);
    let object = spa::pod::Object {
        type_: spa::sys::SPA_TYPE_OBJECT_Format,
        id: spa::sys::SPA_PARAM_EnumFormat,
        properties: info.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|(c, _)| c.into_inner())
    .map_err(|e| SinkError::Config(format!("clip format pod: {e:?}")))
}

impl ClipStream {
    pub fn new(core: &pw::core::CoreRc, clip: &ClipPcm) -> Result<Self, SinkError> {
        let err = |stage: &str, e: pw::Error| SinkError::Config(format!("clip {stage}: {e}"));
        let channels = usize::from(clip.channels.clamp(1, 2));
        let name = format!("{CLIP_PREFIX}{}", clip.id);

        let stream = pw::stream::StreamRc::new(
            core.clone(),
            &name,
            pw::properties::properties! {
                "media.type" => "Audio",
                "media.category" => "Playback",
                // Not "Music": this is a one-shot effect, and a session
                // manager that dips music for notifications should treat it
                // as one rather than as another player to duck against.
                "media.role" => "Notification",
                "node.name" => name.as_str(),
                "node.autoconnect" => "false",
                "node.dont-reconnect" => "true",
            },
        )
        .map_err(|e| err("stream", e))?;

        let listener = stream
            .add_local_listener_with_user_data(ClipCtx {
                samples: clip.samples.clone(),
                pos: 0,
                gain: clip.gain,
                channels,
            })
            .process(move |stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                // Fill only what the graph asked for this cycle - handing it
                // the whole mmap'd buffer runs the clip several times faster
                // than real time (the mic playback stream learned this the
                // hard way, see mic.rs).
                let requested = buffer.requested() as usize;
                let stride = 4 * ctx.channels;
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else { return };
                let max_frames = data.data().map(|d| d.len()).unwrap_or(0) / stride;
                let frames = if requested > 0 {
                    requested.min(max_frames)
                } else {
                    max_frames.min(1024)
                };
                if frames == 0 {
                    return;
                }
                {
                    // Unreachable given the guard above, but an unwrap on a
                    // data thread aborts the whole process - just bail.
                    let Some(bytes) = data.data() else { return };
                    for i in 0..frames * ctx.channels {
                        // Past the end of the clip the stream keeps emitting
                        // silence until it is reaped: a stream that stops
                        // producing would leave the graph waiting on it.
                        let sample = ctx
                            .samples
                            .get(ctx.pos + i)
                            .map_or(0.0, |s| (s * ctx.gain).clamp(-1.0, 1.0));
                        let off = i * 4;
                        bytes[off..off + 4].copy_from_slice(&sample.to_ne_bytes());
                    }
                    ctx.pos += frames * ctx.channels;
                }
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = stride as i32;
                *chunk.size_mut() = (frames * stride) as u32;
            })
            .register()
            .map_err(|e| err("listener", e))?;

        let format = clip_format(clip.rate, clip.channels)?;
        let mut params = [Pod::from_bytes(&format)
            .ok_or_else(|| SinkError::Config("clip format pod invalid".into()))?];
        stream
            .connect(
                spa::utils::Direction::Output,
                None,
                // No AUTOCONNECT: the loop creates the links (see the module
                // doc - the session manager cannot route this one).
                //
                // INACTIVE is not a detail: an unlinked stream is scheduled by
                // the dummy driver, which pulls as fast as it likes. Measured,
                // that drained a one-second clip in about 50 ms - the whole
                // thing was gone before the links existed and only the last
                // few milliseconds ever reached the chat. So the stream stays
                // parked until it is wired up, and `start` releases it.
                pw::stream::StreamFlags::INACTIVE
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut params,
            )
            .map_err(|e| err("connect", e))?;

        Ok(Self {
            stream,
            _listener: listener,
            targets: clip.targets,
            started: std::cell::Cell::new(false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_streams_are_internal_and_named_per_playback() {
        // The prefix is what keeps a firing clip out of the app list, and
        // the id is what keeps a clip taking over from another one from
        // colliding with the node it replaces.
        assert!(CLIP_PREFIX.starts_with(crate::audio::pw_native::thread::INTERNAL_PREFIX));
        assert_ne!(format!("{CLIP_PREFIX}1"), format!("{CLIP_PREFIX}2"));
    }
}
