//! Fan-out of the backend's events to every connected remote client.
//!
//! The desktop UI gets these through Tauri's event system; a tablet gets the
//! same nine events as WebSocket frames. `levels` arrives at 10 Hz, which is
//! the whole design constraint here: a tablet that has walked out of Wi-Fi
//! range, or one whose browser has been backgrounded, must not be able to slow
//! the emitter thread down - that thread also serves the desktop meters and,
//! behind it, the audio poll.
//!
//! So the queue is bounded per client and overflow drops *that client's*
//! oldest frames rather than blocking the sender. Meters are strictly
//! latest-value data: a dropped frame is invisible, a stalled one is not.

use std::sync::Arc;

use tokio::sync::broadcast;

/// Frames buffered per client before the slow one starts losing the oldest.
/// At the 10 Hz `levels` cadence this is ~6 s of grace, which covers a Wi-Fi
/// roam or a phone waking up, and no more.
const QUEUE: usize = 64;

/// The events a remote client can receive. Exactly the set the frontend's
/// `EventName` union names - anything else the backend emits internally is not
/// part of the remote contract.
pub const REMOTE_EVENTS: &[&str] = &[
    "levels",
    "graph-changed",
    "profile-changed",
    "state-changed",
    "headset-status",
    "headset-presence",
    "headset-chatmix",
    "mouse-status",
    "mouse-presence",
];

/// One event, already serialized into the frame that goes on the wire.
///
/// Serializing once for all clients matters at 10 Hz with several tablets
/// attached; `Arc` makes the fan-out a refcount bump.
#[derive(Debug, Clone)]
pub struct EventFrame(Arc<str>);

impl EventFrame {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The broadcast side of the fan-out.
#[derive(Debug)]
pub struct EventFan {
    tx: broadcast::Sender<EventFrame>,
}

impl Default for EventFan {
    fn default() -> Self {
        Self::new()
    }
}

impl EventFan {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(QUEUE).0,
        }
    }

    /// Queue one event for every attached client.
    ///
    /// `payload` is the already-JSON-encoded payload Tauri hands a Rust
    /// listener, so it is spliced in rather than re-encoded. Never blocks and
    /// never fails: with nobody listening there is nothing to do.
    pub fn push(&self, event: &str, payload: &str) {
        if self.tx.receiver_count() == 0 {
            return;
        }
        // Both halves are already valid JSON - `event` comes from
        // `REMOTE_EVENTS`, `payload` from Tauri's own serializer.
        let frame = format!(r#"{{"t":"event","event":"{event}","payload":{payload}}}"#);
        let _ = self.tx.send(EventFrame(frame.into()));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventFrame> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast::error::TryRecvError;

    #[test]
    fn a_stalled_client_loses_frames_instead_of_stalling_the_emitter() {
        let fan = EventFan::new();
        let mut client = fan.subscribe();

        // The tablet has stopped reading (screen off, Wi-Fi roam) while the
        // level emitter keeps pushing. Every push has to return immediately.
        for i in 0..QUEUE + 10 {
            fan.push("levels", &format!(r#"{{"n":{i}}}"#));
        }

        // The client is told how much it missed, then resumes at the newest
        // frames - not at the stale ones it slept through.
        assert_eq!(client.try_recv().unwrap_err(), TryRecvError::Lagged(10));
        let resumed = client.try_recv().expect("a frame after the lag notice");
        assert!(
            resumed.as_str().contains(r#""n":10"#),
            "resumed at {}, expected the oldest surviving frame",
            resumed.as_str()
        );
    }

    #[test]
    fn pushing_with_nobody_attached_is_free() {
        // The emitter pushes unconditionally; with no remote client there must
        // be no allocation and no error to handle - and a client that attaches
        // afterwards starts at "now", not at whatever it missed.
        let fan = EventFan::new();
        fan.push("levels", r#"{"sink_game":[0.1,0.2]}"#);
        let mut client = fan.subscribe();
        assert_eq!(client.try_recv().unwrap_err(), TryRecvError::Empty);
    }

    #[test]
    fn a_frame_carries_the_event_name_and_the_payload_verbatim() {
        let fan = EventFan::new();
        let mut client = fan.subscribe();
        fan.push("headset-chatmix", "[64,64]");
        assert_eq!(
            client.try_recv().expect("frame").as_str(),
            r#"{"t":"event","event":"headset-chatmix","payload":[64,64]}"#
        );
    }

    #[test]
    fn every_frontend_event_is_forwarded() {
        // The nine names are the contract with `src/lib/ipc.ts`'s EventName
        // union; a backend event missing here silently never reaches a tablet.
        assert_eq!(REMOTE_EVENTS.len(), 9);
        for event in ["levels", "graph-changed", "state-changed", "headset-status"] {
            assert!(REMOTE_EVENTS.contains(&event));
        }
    }
}
