//! Inari Remote: the same UI, reached from a tablet on the LAN.
//!
//! A tablet propped next to the keyboard is a better mixer than alt-tabbing
//! out of a game, so the app serves its own frontend bundle over HTTP and
//! carries the IPC seam (`src/lib/ipc.ts`) over a WebSocket. The React code is
//! identical on both sides; only the transport differs.
//!
//! The pieces, and where the interesting decisions live:
//!
//! * [`allowlist`] - what a remote client may do. A positive list, deny by
//!   default. This is the security boundary; read its module docs first.
//! * [`auth`] - the shared token: CSPRNG, constant-time compare, never logged.
//! * [`assets`] - the frontend bundle out of Tauri's own embed, and app icons
//!   restricted to the asset-protocol scope.
//! * [`events`] - the nine backend events, fanned out with a bounded per-client
//!   queue so a tablet that walked out of Wi-Fi range cannot stall the app.
//! * `server` - axum, on a runtime this module owns.
//!
//! Everything here is off until the user turns it on, and even then it listens
//! on loopback until they say otherwise.

pub mod allowlist;
pub mod assets;
pub mod auth;
pub mod events;
mod limits;
mod server;

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use log::{info, warn};
use serde::Serialize;
use tauri::{AppHandle, Listener, Manager};

use crate::persistence::prefs::RemoteConfig;
use events::{EventFan, REMOTE_EVENTS};
use server::Running;

/// One address the listener could sit on, for the bind picker.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteInterface {
    pub address: String,
    /// What to call it in the picker - the kernel's interface name, since
    /// nothing short of asking NetworkManager knows it as "Wi-Fi".
    pub label: String,
    pub loopback: bool,
    /// `0.0.0.0`: every interface at once, including ones the user forgot
    /// about.
    pub wildcard: bool,
}

/// What the settings UI shows about the remote.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteStatus {
    /// Whether a listener is actually up - not merely what the user asked
    /// for. The two differ when a bind failed (port taken, interface gone),
    /// and showing "on" over a dead listener is how a QR that pairs nobody
    /// ends up on screen.
    pub enabled: bool,
    /// The address bound, or the one that would be used.
    pub address: String,
    pub port: u16,
    /// Base URL to type on the tablet. Carries no token, so it is not secret.
    pub url: String,
    pub clients: usize,
    pub interfaces: Vec<RemoteInterface>,
}

/// Everything a new device needs to pair, and nothing that identifies the
/// machine beyond its address on the local network.
#[derive(Debug, Clone, Serialize)]
pub struct RemotePairing {
    /// The full pairing URL. The token rides in the fragment, which browsers
    /// do not send to the server - so it stays out of access logs and out of
    /// any proxy in between.
    pub url: String,
    pub token: String,
    /// The same URL as an inline SVG. Rendered here rather than in the
    /// frontend so the token never has to be handed to an npm package.
    pub qr_svg: String,
}

/// How many tablets are attached, and to which run of the server.
///
/// The count is read from outside the remote entirely: the level and graph
/// emitters keep running while the desktop window is hidden if, and only if, a
/// client is watching. A wrong count is therefore not cosmetic - too low and
/// the tablet's meters freeze, too high and two emitter threads run forever
/// against a window nobody can see.
///
/// The generation is what keeps it right across a restart. [`RemoteState::stop`]
/// returns as soon as the shutdown signal is sent, while the old runtime is
/// still winding its connections down, and [`RemoteState::start`] re-uses this
/// same counter for the new server - so a socket closing in that window would
/// otherwise decrement a count that had just been zeroed and wrap it to
/// `usize::MAX`, which reads as "a client is connected" for the rest of the
/// process's life. A client may only move the count of the generation it
/// joined.
#[derive(Debug, Default)]
pub struct Clients {
    count: AtomicUsize,
    generation: AtomicUsize,
}

impl Clients {
    /// Begin a new generation: nobody is connected, and the clients of the
    /// previous one can no longer say otherwise. Returns the token the new
    /// server hands to each of its connections.
    fn reset(&self) -> usize {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.count.store(0, Ordering::SeqCst);
        generation
    }

    /// One client of `generation` connected. Returns the count to log.
    pub fn join(&self, generation: usize) -> usize {
        if !self.current(generation) {
            return self.count();
        }
        self.count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// ...and disconnected.
    pub fn leave(&self, generation: usize) -> usize {
        if !self.current(generation) {
            return self.count();
        }
        // Saturating rather than wrapping: even a decrement that loses a race
        // with `reset` has to land on zero. `fetch_update` returns the previous
        // value, so the new one is one less.
        self.count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                Some(n.saturating_sub(1))
            })
            .unwrap_or(0)
            .saturating_sub(1)
    }

    pub fn count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    /// Is anyone watching? The question both emitters actually ask.
    pub fn any(&self) -> bool {
        self.count() > 0
    }

    fn current(&self, generation: usize) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }
}

/// The remote's live state, managed by Tauri alongside `AppState`.
pub struct RemoteState {
    running: Mutex<Option<Running>>,
    clients: Arc<Clients>,
    fan: Arc<EventFan>,
}

impl Default for RemoteState {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteState {
    pub fn new() -> Self {
        Self {
            running: Mutex::new(None),
            clients: Arc::new(Clients::default()),
            fan: Arc::new(EventFan::new()),
        }
    }

    /// How many tablets are attached right now.
    ///
    /// The level emitter reads this: the desktop window is usually hidden
    /// exactly when the remote is in use, and the meters have to keep flowing.
    pub fn client_count(&self) -> Arc<Clients> {
        self.clients.clone()
    }

    /// Forward one backend event to the connected clients.
    pub fn push_event(&self, event: &str, payload: &str) {
        self.fan.push(event, payload);
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().is_ok_and(|r| r.is_some())
    }

    /// Bind and serve. Replaces a running server, so this doubles as the
    /// "settings changed" path.
    pub fn start(&self, app: &AppHandle, config: &RemoteConfig) -> Result<(), String> {
        self.stop();
        let ip: IpAddr = config
            .address
            .parse()
            .map_err(|_| format!("`{}` is not an IP address to listen on", config.address))?;
        let token = auth::load_or_create().map_err(|e| e.to_string())?;
        // A fresh generation before the first socket of the new server can
        // arrive, so nothing the old one is still tearing down can be counted
        // against it.
        let generation = self.clients.reset();
        let running = server::start(
            app.clone(),
            SocketAddr::new(ip, config.port),
            token,
            self.fan.clone(),
            self.clients.clone(),
            generation,
        );
        let mut slot = self.running.lock().map_err(|_| "remote state lock poisoned")?;
        *slot = Some(running?);
        Ok(())
    }

    pub fn stop(&self) {
        if let Ok(mut slot) = self.running.lock() {
            if let Some(running) = slot.take() {
                running.stop();
            }
        }
        // Clients drop with the listener, but not synchronously - the runtime
        // is still winding down when this returns. A new generation is how the
        // stragglers are kept from touching the count on their way out.
        self.clients.reset();
    }

    pub fn status(&self, config: &RemoteConfig) -> RemoteStatus {
        let bound = self
            .running
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|r| r.addr));
        // The URL is shown for a stopped server too (it is what the tablet
        // would use), so fall back to the configured address when nothing is
        // listening. A bind string we cannot parse can only mean the user has
        // not picked a valid one yet; loopback is the honest stand-in.
        let addr = bound.unwrap_or_else(|| {
            SocketAddr::new(
                config.address.parse().unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
                config.port,
            )
        });
        RemoteStatus {
            enabled: bound.is_some(),
            address: config.address.clone(),
            port: config.port,
            url: format!("http://{}", reachable_host(addr)),
            clients: self.clients.count(),
            interfaces: interfaces(),
        }
    }

    /// The URL, token and QR a new device needs.
    ///
    /// Only answers while the remote is running: a pairing code for a server
    /// that is not listening is a support ticket waiting to happen.
    pub fn pairing(&self) -> Result<RemotePairing, String> {
        let addr = self
            .running
            .lock()
            .map_err(|_| "remote state lock poisoned")?
            .as_ref()
            .map(|r| r.addr)
            .ok_or("the remote is not running")?;
        let token = auth::load_or_create().map_err(|e| e.to_string())?;
        let url = format!("http://{}/#token={}", reachable_host(addr), token.expose());
        Ok(RemotePairing {
            qr_svg: qr_svg(&url)?,
            token: token.expose().to_string(),
            url,
        })
    }
}

/// Mint a new token, invalidating every paired device.
///
/// The server is restarted around it so the sockets holding the old token are
/// dropped rather than living on until they happen to reconnect.
pub fn regenerate_token(
    app: &AppHandle,
    state: &RemoteState,
    config: &RemoteConfig,
) -> Result<(), String> {
    auth::regenerate().map_err(|e| e.to_string())?;
    if state.is_running() {
        state.start(app, config)?;
    }
    Ok(())
}

/// Bridge the backend's events into the fan-out.
///
/// Tauri delivers the payload to a Rust listener already JSON-encoded, which
/// is the same encoding the WebSocket wants - so this is a splice, not a
/// re-serialization, and it costs nothing while no client is attached.
pub fn bridge_events(app: &AppHandle) {
    for &event in REMOTE_EVENTS {
        let handle = app.clone();
        app.listen_any(event, move |received| {
            handle
                .state::<RemoteState>()
                .push_event(event, received.payload());
        });
    }
}

/// Start the remote if the user left it enabled. Called once at boot.
pub fn apply_saved(app: &AppHandle, config: &RemoteConfig) {
    if !config.enabled {
        return;
    }
    match app.state::<RemoteState>().start(app, config) {
        // The address is worth writing down; the token never is.
        Ok(()) => info!("remote: enabled on {}:{}", config.address, config.port),
        // Not fatal and not silent: the app runs on, and the settings page
        // shows the remote as off because nothing is listening.
        Err(e) => warn!("remote: could not start: {e}"),
    }
}

/// The addresses this machine could listen on, safest first.
///
/// Offering the real list is the difference between "type an IP" and a choice
/// the user can reason about - and the wildcard is last, with its own flag, so
/// the widest option is never the one they land on by accident.
///
/// IPv4 only. A tablet is reached by typing or scanning a URL, and a
/// link-local IPv6 address with a `%wlan0` zone id in it is not a URL anyone
/// can act on.
fn interfaces() -> Vec<RemoteInterface> {
    let mut found = vec![RemoteInterface {
        address: "127.0.0.1".to_string(),
        label: "Loopback".to_string(),
        loopback: true,
        wildcard: false,
    }];
    if let Ok(addrs) = if_addrs::get_if_addrs() {
        for addr in addrs {
            let std::net::IpAddr::V4(v4) = addr.ip() else {
                continue;
            };
            if addr.is_loopback() {
                continue;
            }
            found.push(RemoteInterface {
                address: v4.to_string(),
                label: addr.name.clone(),
                loopback: false,
                wildcard: false,
            });
        }
    }
    found.push(RemoteInterface {
        address: "0.0.0.0".to_string(),
        label: "All networks".to_string(),
        loopback: false,
        wildcard: true,
    });
    found
}

/// The host:port a device on the network should aim at.
///
/// A wildcard bind (`0.0.0.0`) is not something a tablet can dial, so it is
/// replaced with this machine's address on the LAN.
fn reachable_host(addr: SocketAddr) -> String {
    let ip = if addr.ip().is_unspecified() {
        lan_address().unwrap_or_else(|| addr.ip())
    } else {
        addr.ip()
    };
    match ip {
        IpAddr::V6(v6) => format!("[{v6}]:{}", addr.port()),
        IpAddr::V4(v4) => format!("{v4}:{}", addr.port()),
    }
}

/// This machine's address on the network it would route out of.
///
/// Connecting a UDP socket sends nothing - it only asks the kernel which
/// interface a packet would leave through, and therefore which address a
/// device on that network can reach us at. The target is in TEST-NET-1, which
/// is never routed anywhere, so there is no host being contacted even in
/// principle.
fn lan_address() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    socket.local_addr().ok().map(|addr| addr.ip())
}

fn qr_svg(url: &str) -> Result<String, String> {
    use qrcode::render::svg;
    use qrcode::QrCode;

    let code = QrCode::new(url.as_bytes()).map_err(|e| format!("cannot encode the QR: {e}"))?;
    let rendered = code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        // The quiet zone is not decoration: scanners need the margin to find
        // the code at all.
        .quiet_zone(true)
        .build();
    // The renderer prefixes a standalone-document XML declaration. This one is
    // going into a page that already exists, where that declaration is not
    // markup - so hand back the bare `<svg>` element.
    Ok(match rendered.split_once("?>") {
        Some((_, svg)) => svg.to_string(),
        None => rendered,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn a_wildcard_bind_is_not_what_a_tablet_dials() {
        // 0.0.0.0 is a listen address, not a destination - the pairing URL has
        // to name an address that exists on the network.
        let host = reachable_host(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 7684));
        assert!(host.ends_with(":7684"));
        assert!(
            !host.starts_with("0.0.0.0"),
            "a machine with no route home is the only case, and it is not this one"
        );
    }

    #[test]
    fn an_explicit_bind_is_used_as_given() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 40)), 7684);
        assert_eq!(reachable_host(addr), "192.168.1.40:7684");
    }

    #[test]
    fn the_bind_picker_offers_loopback_first_and_the_wildcard_last() {
        // Order is the safety story: the first option reaches nothing, the
        // last one reaches everything, and the machine's real addresses sit
        // between them. It must hold on a build machine with no network too.
        let found = interfaces();
        assert!(found.first().expect("loopback").loopback);
        assert_eq!(found.first().expect("loopback").address, "127.0.0.1");
        let last = found.last().expect("wildcard");
        assert!(last.wildcard);
        assert_eq!(last.address, "0.0.0.0");
        assert_eq!(
            found.iter().filter(|i| i.wildcard).count(),
            1,
            "exactly one option opens every interface"
        );
        assert!(
            found.iter().skip(1).all(|i| !i.loopback),
            "loopback is offered once, not once per alias"
        );
    }

    #[test]
    fn a_client_of_a_stopped_server_cannot_move_the_new_count() {
        let clients = Clients::default();
        let first = clients.reset();
        assert_eq!(clients.join(first), 1);

        // The user toggled the remote off and on again. The old socket is
        // still winding down and reports its own teardown afterwards.
        let second = clients.reset();
        assert_eq!(clients.count(), 0);
        assert_eq!(clients.leave(first), 0, "a straggler is not this server's");
        assert!(!clients.any(), "and it cannot wrap the count either");

        assert_eq!(clients.join(second), 1);
        assert_eq!(clients.leave(second), 0);
        assert!(!clients.any());
    }

    #[test]
    fn the_count_never_goes_below_zero() {
        // Belt to the generation's braces: whatever the ordering, the emitters
        // must never see usize::MAX and run forever against a hidden window.
        let clients = Clients::default();
        let generation = clients.reset();
        assert_eq!(clients.leave(generation), 0);
        assert_eq!(clients.count(), 0);
    }

    #[test]
    fn the_pairing_qr_is_a_standalone_svg() {
        let svg = qr_svg("http://192.168.1.40:7684/#token=deadbeef").expect("renders");
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        // Droppable straight into the DOM: no external references to fetch,
        // which the remote page's CSP would block anyway.
        assert!(!svg.contains("<image"));
        assert!(!svg.contains("http://192.168.1.40"), "the URL is encoded, not printed");
    }
}
