//! The HTTP + WebSocket server behind Inari Remote.
//!
//! One axum app on a runtime the remote owns, on its own thread - Tauri's
//! `async_runtime` keeps serving the commands that already use it. One
//! WebSocket endpoint carries both directions:
//!
//! ```text
//! client -> server   {"id": 7, "cmd": "set_channel_volume", "args": {...}}
//!                    {"t": "ping"}
//! server -> client   {"t": "reply", "id": 7, "ok": true,  "data": null}
//!                    {"t": "reply", "id": 7, "ok": false, "err": "unknown channel"}
//!                    {"t": "event", "event": "levels", "payload": {...}}
//!                    {"t": "pong"}
//! ```
//!
//! Replies carry the id the client chose, echoed back unchanged. The `t` tag
//! is what lets a client tell a reply from a push without inspecting which
//! keys are present.
//!
//! Subscription frames are accepted and ignored: every connected client gets
//! every one of the nine events. A client that filters locally costs nothing
//! here, and per-client subscription state would be one more thing to rebuild
//! after each of the reconnects a sleeping tablet generates.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use log::{debug, info, warn};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::{broadcast, mpsc, oneshot, Semaphore};

use super::allowlist::{self, Args};
use super::assets::{self, IconReject};
use super::auth::Token;
use super::events::EventFan;
use super::limits::{Gated, Peer};
use super::Clients;

/// Commands executing at once, across every connected client. Enough that a UI
/// which refetches its whole state on connect is not serialized behind itself,
/// low enough that a flood cannot exhaust the blocking pool the audio commands
/// share - and global rather than per client, because eight tablets each
/// allowed eight would be sixty-four handlers contending for the mixer lock.
const MAX_IN_FLIGHT: usize = 8;

/// Requests accepted from one client but not yet answered.
///
/// [`MAX_IN_FLIGHT`] bounds what *executes*; without this nothing bounds what
/// is *admitted*, and a client that writes faster than the mixer can answer -
/// each `get_virtual_devices` shells out to `pactl` - grows one task and one
/// frame of memory per request until the process is killed. A real client
/// never has more than a couple of dozen outstanding, so passing this is not a
/// burst, it is a flood.
const MAX_QUEUED: usize = 64;

/// Close code for a client whose token is missing or wrong, matching
/// `CLOSE_UNAUTHORIZED` in `src/lib/wsTransport.ts`. It exists so a tablet that
/// was cut off by "Regenerate token" can stop retrying and say so, instead of
/// reconnecting into a 401 every ten seconds forever.
const CLOSE_UNAUTHORIZED: u16 = 4001;

/// How often a refused connection is worth a line in the log file.
///
/// A refusal needs no credential, so logging one per attempt hands a stranger
/// on the network control of how large the log file gets. One line with a
/// count carries the same signal at a bounded cost.
const REFUSAL_LOG_INTERVAL: Duration = Duration::from_secs(60);

/// Replies buffered per client before the dispatcher waits for the socket.
const REPLY_QUEUE: usize = 32;

/// How often a burst of remote writes is folded into one refresh of the
/// desktop UI. A slider drag is hundreds of writes a second; the desktop must
/// learn about them, but not once per frame.
const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

/// What every request handler needs. Cheap to clone - it is all `Arc` inside.
struct Ctx {
    app: AppHandle,
    /// The connection secret. Immutable: regenerating restarts the server, so
    /// the sockets holding the old token are dropped rather than grandfathered.
    token: Token,
    fan: Arc<EventFan>,
    clients: Arc<Clients>,
    /// Which run of the server this is, so a client winding down after a
    /// restart cannot move the new run's count.
    generation: usize,
    /// Handlers running right now, across every client.
    in_flight: Semaphore,
    refusals: Refusals,
    refresh: Refresh,
}

/// Rate limit for the "bad token" log line, and the count it suppressed.
struct Refusals(Mutex<(Option<Instant>, u64)>);

impl Default for Refusals {
    fn default() -> Self {
        Self(Mutex::new((None, 0)))
    }
}

impl Refusals {
    /// Count one refusal. Answers with how many have gone unreported when it
    /// is time for a line, and `None` while the interval is still running.
    fn note(&self) -> Option<u64> {
        let mut state = self.0.lock().ok()?;
        let (last, suppressed) = &mut *state;
        *suppressed += 1;
        if last.is_some_and(|at| at.elapsed() < REFUSAL_LOG_INTERVAL) {
            return None;
        }
        *last = Some(Instant::now());
        Some(std::mem::take(suppressed))
    }
}

/// Coalescing flags for "something changed, tell the desktop side".
#[derive(Default)]
struct Refresh {
    /// A remote client wrote something.
    ui: AtomicBool,
    /// ...and it was something the tray menu displays.
    tray: AtomicBool,
}

/// A running server. Dropping it, or calling [`Running::stop`], shuts the
/// listener down and lets the runtime thread finish.
pub struct Running {
    pub addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
}

impl Running {
    pub fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Bind and start serving, or fail with the reason the UI should show (the
/// usual one being a port already in use).
///
/// Blocks only until the listener is bound: the caller is a Tauri command that
/// has to answer "did it come up?", and it must not wait for the server to
/// stop.
pub fn start(
    app: AppHandle,
    addr: SocketAddr,
    token: Token,
    fan: Arc<EventFan>,
    clients: Arc<Clients>,
    generation: usize,
) -> Result<Running, String> {
    let (bound_tx, bound_rx) = std::sync::mpsc::channel::<Result<SocketAddr, String>>();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let ctx = Arc::new(Ctx {
        app,
        token,
        fan,
        clients,
        generation,
        in_flight: Semaphore::new(MAX_IN_FLIGHT),
        refusals: Refusals::default(),
        refresh: Refresh::default(),
    });

    std::thread::Builder::new()
        .name("inari-remote".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(e) => {
                    let _ = bound_tx.send(Err(format!("cannot start the remote runtime: {e}")));
                    return;
                }
            };
            runtime.block_on(serve(ctx, addr, bound_tx, shutdown_rx));
        })
        .map_err(|e| format!("cannot start the remote thread: {e}"))?;

    // The thread always answers: it either sends the bound address or the
    // reason it could not bind, and a send failure means it died first.
    let addr = bound_rx
        .recv()
        .map_err(|_| "the remote server thread stopped before it came up".to_string())??;
    Ok(Running {
        addr,
        shutdown: Some(shutdown_tx),
    })
}

async fn serve(
    ctx: Arc<Ctx>,
    addr: SocketAddr,
    bound: std::sync::mpsc::Sender<Result<SocketAddr, String>>,
    shutdown: oneshot::Receiver<()>,
) {
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            let _ = bound.send(Err(format!("cannot listen on {addr}: {e}")));
            return;
        }
    };
    let local = listener.local_addr().unwrap_or(addr);
    if bound.send(Ok(local)).is_err() {
        return;
    }
    info!("remote: listening on {local}");

    tokio::spawn(refresh_loop(ctx.clone()));

    let router = router(ctx);
    // Not the bare listener: `Gated` is what keeps an unauthenticated peer
    // from spending the app's file descriptors, and everything in this file
    // runs far too late to help with that.
    let served = axum::serve(
        Gated::new(listener),
        router.into_make_service_with_connect_info::<Peer>(),
    )
    .with_graceful_shutdown(async {
        // Either the stop signal, or the handle being dropped.
        let _ = shutdown.await;
    })
    .await;
    if let Err(e) = served {
        warn!("remote: server stopped: {e}");
    } else {
        info!("remote: stopped");
    }
}

fn router(ctx: Arc<Ctx>) -> Router {
    Router::new()
        .route("/ws", get(ws_route))
        .route("/icon", get(icon_route))
        .route("/", get(asset_route))
        .route("/{*path}", get(asset_route))
        .with_state(ctx)
}

/// Fold a burst of remote writes into one refresh of the desktop side.
async fn refresh_loop(ctx: Arc<Ctx>) {
    let mut ticker = tokio::time::interval(REFRESH_INTERVAL);
    loop {
        ticker.tick().await;
        // The tray flag is checked first so a mute always ends up rebuilding
        // the menu, never just emitting the event.
        if ctx.refresh.tray.swap(false, Ordering::Relaxed) {
            ctx.refresh.ui.store(false, Ordering::Relaxed);
            crate::after_external_change(&ctx.app);
        } else if ctx.refresh.ui.swap(false, Ordering::Relaxed) {
            // Volumes and EQ do not show in the tray, so this is the cheap
            // half of `after_external_change`: tell the window to re-read.
            let _ = ctx.app.emit("state-changed", ());
        }
    }
}

/// Writes that change something the tray menu displays.
fn touches_tray(cmd: &str) -> bool {
    matches!(
        cmd,
        "toggle_channel_mute" | "set_mic_config" | "load_profile"
    )
}

fn unauthorized() -> Response {
    // No hint about what was wrong with the token, and nothing about it in the
    // body - a paired client knows what it sent.
    (StatusCode::UNAUTHORIZED, "unauthorized").into_response()
}

/// Is the token in this request's query string the right one?
///
/// The token travels as a query parameter because a browser cannot set headers
/// on a WebSocket handshake. It is never logged, here or anywhere else.
fn authorized(token: &Token, query: &HashMap<String, String>) -> bool {
    query
        .get("token")
        .is_some_and(|presented| token.matches(presented))
}

async fn ws_route(
    upgrade: WebSocketUpgrade,
    Query(query): Query<HashMap<String, String>>,
    ConnectInfo(peer): ConnectInfo<Peer>,
    State(ctx): State<Arc<Ctx>>,
) -> Response {
    let peer = peer.0;
    if !authorized(&ctx.token, &query) {
        if let Some(refused) = ctx.refusals.note() {
            warn!(
                "remote: refused {refused} connection(s) with a bad token, latest from {}",
                peer.ip()
            );
        }
        // Upgraded and closed immediately rather than answered with a 401. The
        // browser reports a failed handshake as close code 1006 - the same
        // code a host that is simply off produces - so a tablet cut loose by
        // "Regenerate token" could never tell "re-pair me" from "keep
        // retrying", and retried until someone noticed. A close code can say
        // which, and costs one frame.
        return upgrade.on_upgrade(refuse);
    }
    upgrade.on_upgrade(move |socket| client(socket, ctx, peer))
}

/// Tell a client with the wrong token that retrying will not help, and hang up.
async fn refuse(mut socket: WebSocket) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: CLOSE_UNAUTHORIZED,
            // No hint about what was wrong with the token - a paired client
            // knows what it sent.
            reason: "unauthorized".into(),
        })))
        .await;
}

/// One connected tablet, for as long as it stays connected.
async fn client(mut socket: WebSocket, ctx: Arc<Ctx>, peer: SocketAddr) {
    let mut events = ctx.fan.subscribe();
    let (replies_tx, mut replies) = mpsc::channel::<String>(REPLY_QUEUE);
    let queued = Arc::new(Semaphore::new(MAX_QUEUED));
    let count = ctx.clients.join(ctx.generation);
    info!("remote: {} connected ({count} client(s))", peer.ip());

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                match message {
                    Message::Text(text) => {
                        if !spawn_command(&ctx, text.to_string(), &replies_tx, &queued) {
                            // Past MAX_QUEUED unanswered requests the client is
                            // not waiting for its replies, so there is nothing
                            // to answer it with. Hanging up bounds the damage;
                            // growing a task per frame does not.
                            warn!(
                                "remote: {} outran its replies by {MAX_QUEUED} requests, closing",
                                peer.ip()
                            );
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    // Binary frames are not part of the protocol; ping/pong is
                    // answered by axum itself.
                    _ => {}
                }
            }
            reply = replies.recv() => {
                let Some(reply) = reply else { break };
                if socket.send(Message::Text(reply.into())).await.is_err() {
                    break;
                }
            }
            event = events.recv() => match event {
                Ok(frame) => {
                    if socket.send(Message::Text(frame.as_str().into())).await.is_err() {
                        break;
                    }
                }
                // The whole point of the bounded queue: this client fell
                // behind and lost frames, and the emitter never noticed.
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    debug!("remote: {} fell behind, dropped {missed} event(s)", peer.ip());
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }

    let count = ctx.clients.leave(ctx.generation);
    info!("remote: {} disconnected ({count} client(s))", peer.ip());
}

/// Parse one request frame and run it, off the socket loop so a slow command
/// (an OLED clip is ~140 rendered frames) does not stall the event stream.
///
/// Returns whether the request was admitted. The queue slot is taken *here*,
/// on the socket loop, and released when the spawned task finishes: taking it
/// inside the task would bound how many run at once but not how many exist.
fn spawn_command(
    ctx: &Arc<Ctx>,
    text: String,
    replies: &mpsc::Sender<String>,
    queued: &Arc<Semaphore>,
) -> bool {
    let Ok(slot) = queued.clone().try_acquire_owned() else {
        return false;
    };
    let ctx = ctx.clone();
    let replies = replies.clone();
    tokio::spawn(async move {
        let _slot = slot;
        // Closed only when the connection is going away.
        let Ok(_permit) = ctx.in_flight.acquire().await else {
            return;
        };
        let reply = match Frame::parse(&text) {
            Ok(Frame::Call(request)) => run(&ctx, request).await,
            Ok(Frame::Pong) => PONG.to_string(),
            // Nothing to do and nothing to say: the client is already getting
            // every event.
            Ok(Frame::Ignored) => return,
            Err(e) => reply_err(&Value::Null, &e),
        };
        let _ = replies.send(reply).await;
    });
    true
}

/// The client's answer to a heartbeat. The browser gives JS no access to
/// WebSocket-level pings, so a sleeping tablet needs an application-level one
/// to tell a live socket from a half-open one.
const PONG: &str = r#"{"t":"pong"}"#;

/// One frame from a client.
enum Frame {
    Call(Request),
    /// A heartbeat to answer.
    Pong,
    /// A frame with nothing to do (subscribe / unsubscribe).
    Ignored,
}

/// A request frame. `id` is whatever the client used to correlate its call and
/// is echoed back untouched.
struct Request {
    id: Value,
    cmd: String,
    args: Args,
}

impl Frame {
    fn parse(text: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(text).map_err(|e| format!("malformed request: {e}"))?;
        let object = value.as_object().ok_or("a request must be an object")?;
        match object.get("t").and_then(Value::as_str) {
            Some("ping") => return Ok(Self::Pong),
            Some("subscribe") | Some("unsubscribe") => return Ok(Self::Ignored),
            _ => {}
        }
        let cmd = object
            .get("cmd")
            .and_then(Value::as_str)
            .ok_or("a request needs a `cmd`")?
            .to_string();
        let args = match object.get("args") {
            None | Some(Value::Null) => Args::default(),
            Some(Value::Object(map)) => Args::new(map.clone()),
            Some(_) => return Err("`args` must be an object".to_string()),
        };
        Ok(Self::Call(Request {
            id: object.get("id").cloned().unwrap_or(Value::Null),
            cmd,
            args,
        }))
    }
}

async fn run(ctx: &Ctx, request: Request) -> String {
    match allowlist::dispatch(&ctx.app, &request.cmd, request.args).await {
        Ok(data) => {
            if !allowlist::is_read(&request.cmd) {
                ctx.refresh.ui.store(true, Ordering::Relaxed);
                if touches_tray(&request.cmd) {
                    ctx.refresh.tray.store(true, Ordering::Relaxed);
                }
            }
            reply_ok(&request.id, data)
        }
        Err(e) => reply_err(&request.id, &e),
    }
}

fn reply_ok(id: &Value, data: Value) -> String {
    serde_json::json!({ "t": "reply", "id": id, "ok": true, "data": data }).to_string()
}

fn reply_err(id: &Value, err: &str) -> String {
    // Both spellings of the same field: the wire contract calls it `err`, the
    // TypeScript client reads `error`. Collapse to one once both sides agree
    // which - they are always identical.
    serde_json::json!({ "t": "reply", "id": id, "ok": false, "err": err, "error": err }).to_string()
}

/// Name of the cookie the remote page sets so that icon URLs carry no secret.
/// Kept in step with `ICON_COOKIE` in `src/lib/wsTransport.ts`.
const TOKEN_COOKIE: &str = "inari_remote";

/// One cookie's value out of the request headers, percent-decoded.
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|line| line.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| key.trim() == name)
        .and_then(|(_, value)| {
            percent_encoding::percent_decode_str(value.trim())
                .decode_utf8()
                .ok()
                .map(|decoded| decoded.into_owned())
        })
}

/// `GET /icon?path=<absolute path>`, authenticated by the token cookie.
///
/// Behind the token like everything else: it hands back file contents, and
/// "only icon directories" is a narrowing of the damage, not a substitute for
/// authentication.
///
/// The credential is a cookie rather than a query parameter because an `<img>`
/// cannot carry a header and this response *is* cached - a token in the URL
/// would be written into the tablet's on-disk cache index and into anything
/// that ever logs a URL. The query form is still accepted for a browser that
/// refuses to store the cookie at all, where the alternative is every icon
/// coming out blank.
async fn icon_route(
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    State(ctx): State<Arc<Ctx>>,
) -> Response {
    let by_cookie = cookie(&headers, TOKEN_COOKIE).is_some_and(|t| ctx.token.matches(&t));
    if !by_cookie && !authorized(&ctx.token, &query) {
        return unauthorized();
    }
    let Some(path) = query.get("path") else {
        return (StatusCode::BAD_REQUEST, "missing `path`").into_response();
    };
    // Reading a file is blocking work; the icon grid asks for many at once.
    let path = path.clone();
    let read = tokio::task::spawn_blocking(move || assets::read_icon(&path)).await;
    match read {
        Ok(Ok((bytes, content_type))) => (
            [
                (header::CONTENT_TYPE, content_type.to_string()),
                // Icon files are effectively immutable for a session, and a
                // tablet re-fetching every icon on every reconnect is the
                // slowest thing the remote does.
                (header::CACHE_CONTROL, "private, max-age=3600".to_string()),
                // The response depends on the cookie, so a cache must not
                // reuse it across one.
                (header::VARY, "Cookie".to_string()),
                // Icons are a user-writable directory away (~/.local/share/
                // icons) and SVG is on the list, so this endpoint can be made
                // to return a document. Navigated to directly, an SVG runs its
                // inline script on our origin - the static route is protected
                // against exactly that and this one was not. `sandbox` puts
                // any such document in an opaque origin where there is no
                // token to read, and `nosniff` keeps a mislabelled file from
                // becoming HTML.
                (
                    header::CONTENT_SECURITY_POLICY,
                    "default-src 'none'; sandbox".to_string(),
                ),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            ],
            bytes,
        )
            .into_response(),
        // One answer for "not an icon", "outside the scope" and "not there":
        // the client has no business learning which.
        Ok(Err(IconReject::Malformed)) => (StatusCode::BAD_REQUEST, "bad path").into_response(),
        Ok(Err(_)) => (StatusCode::NOT_FOUND, "no such icon").into_response(),
        Err(e) => {
            warn!("remote: icon read did not complete: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The frontend bundle, straight out of the binary Tauri already embedded it
/// in. No token: this is the same static app the desktop window runs, it holds
/// no data, and the client needs it before it can prove anything.
async fn asset_route(uri: axum::http::Uri, State(ctx): State<Arc<Ctx>>) -> Response {
    let Some(key) = assets::asset_key(uri.path()) else {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    };
    let resolver = ctx.app.asset_resolver();
    // A path with no extension is a client-side route, not a missing file -
    // hand it the app and let the router sort it out.
    let asset = resolver.get(key.clone()).or_else(|| {
        if std::path::Path::new(&key).extension().is_none() {
            resolver.get("index.html".to_string())
        } else {
            None
        }
    });
    let Some(asset) = asset else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let content_type = if asset.mime_type.is_empty() {
        assets::content_type(std::path::Path::new(&key)).to_string()
    } else {
        asset.mime_type.clone()
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            // Deliberately *not* the CSP from tauri.conf.json: that one allows
            // `connect-src ipc:` only, which would block the very WebSocket
            // this page exists to open. This is the same policy adapted to the
            // transport - same-origin scripts, and connections back to us.
            //
            // `connect-src` names no scheme: under CSP3 `'self'` already
            // covers a same-origin `ws://`, whereas the bare `ws: wss:` this
            // used to carry allowed a WebSocket to *any* host - which is an
            // exfiltration channel for the token in `localStorage` the moment
            // anything gets a script onto this origin.
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                 img-src 'self' data:; font-src 'self'; connect-src 'self'; \
                 object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
                    .to_string(),
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        asset.bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(text: &str) -> Request {
        match Frame::parse(text).expect("valid frame") {
            Frame::Call(request) => request,
            _ => panic!("expected a command frame"),
        }
    }

    #[test]
    fn a_request_frame_is_read_the_way_the_ipc_seam_writes_it() {
        let request = call(
            r#"{"t":"call","id":7,"cmd":"set_channel_volume","args":{"sinkName":"sink_game","volume":50}}"#,
        );
        assert_eq!(request.id, serde_json::json!(7));
        assert_eq!(request.cmd, "set_channel_volume");
    }

    #[test]
    fn a_command_with_no_arguments_needs_no_args_key() {
        assert_eq!(call(r#"{"id":1,"cmd":"list_profiles"}"#).cmd, "list_profiles");
        assert_eq!(
            call(r#"{"id":1,"cmd":"list_profiles","args":null}"#).cmd,
            "list_profiles"
        );
    }

    #[test]
    fn heartbeats_and_subscriptions_are_not_commands() {
        assert!(matches!(Frame::parse(r#"{"t":"ping"}"#), Ok(Frame::Pong)));
        assert!(matches!(
            Frame::parse(r#"{"t":"subscribe","event":"levels"}"#),
            Ok(Frame::Ignored)
        ));
        assert!(matches!(
            Frame::parse(r#"{"t":"unsubscribe","event":"levels"}"#),
            Ok(Frame::Ignored)
        ));
    }

    #[test]
    fn a_malformed_frame_is_an_error_reply_not_a_dropped_connection() {
        for frame in [
            "",
            "not json",
            "[]",
            r#"{"id":1}"#,
            r#"{"cmd":42}"#,
            r#"{"cmd":"list_profiles","args":[]}"#,
        ] {
            assert!(Frame::parse(frame).is_err(), "{frame} should not parse");
        }
    }

    #[test]
    fn replies_carry_the_id_the_client_chose() {
        // Ids are echoed untouched: a client is free to use strings, and
        // correlating on anything we invented would break it.
        let ok: Value = serde_json::from_str(&reply_ok(&serde_json::json!("a"), Value::Null))
            .expect("valid json");
        assert_eq!(ok["t"], serde_json::json!("reply"));
        assert_eq!(ok["id"], serde_json::json!("a"));
        assert_eq!(ok["ok"], serde_json::json!(true));
        assert!(ok["data"].is_null());

        let err: Value =
            serde_json::from_str(&reply_err(&serde_json::json!(3), "nope")).expect("valid json");
        assert_eq!(err["t"], serde_json::json!("reply"));
        assert_eq!(err["id"], serde_json::json!(3));
        assert_eq!(err["ok"], serde_json::json!(false));
        assert_eq!(err["err"], serde_json::json!("nope"));
        assert_eq!(err["error"], err["err"]);
        assert!(err.get("data").is_none(), "a failure carries no data");
    }

    #[test]
    fn a_connection_without_the_token_is_refused() {
        let token = Token::generate().expect("system randomness");
        let query = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        assert!(!authorized(&token, &query(&[])), "no token at all");
        assert!(!authorized(&token, &query(&[("token", "")])));
        assert!(!authorized(&token, &query(&[("token", "guess")])));
        assert!(
            !authorized(&token, &query(&[("token", &token.expose()[..8])])),
            "a prefix of the real token is not the token"
        );
        assert!(
            !authorized(&token, &query(&[("Token", token.expose())])),
            "the parameter name is exact"
        );
        assert!(authorized(&token, &query(&[("token", token.expose())])));
    }

    #[test]
    fn the_icon_credential_is_read_out_of_the_cookie_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "theme=dark; inari_remote=a1b2c3; other=x".parse().expect("valid header"),
        );
        assert_eq!(cookie(&headers, TOKEN_COOKIE).as_deref(), Some("a1b2c3"));
        assert_eq!(cookie(&headers, "missing"), None);
        // The name is exact, so a cookie whose name merely ends in ours is not
        // ours - it would be a way to present a token nobody set.
        headers.insert(
            header::COOKIE,
            "not_inari_remote=sneaky".parse().expect("valid header"),
        );
        assert_eq!(cookie(&headers, TOKEN_COOKIE), None);
        assert_eq!(cookie(&HeaderMap::new(), TOKEN_COOKIE), None);
    }

    #[test]
    fn refusals_are_counted_but_logged_at_most_once_an_interval() {
        // Refusing costs no credential, so one line per attempt would hand a
        // stranger on the network control of the log file's size.
        let refusals = Refusals::default();
        assert_eq!(refusals.note(), Some(1), "the first one is always worth a line");
        for _ in 0..999 {
            assert_eq!(refusals.note(), None, "and the rest are only counted");
        }
        // Pretend the interval has run out; the count that was suppressed is
        // what the next line carries.
        if let Ok(mut state) = refusals.0.lock() {
            state.0 = None;
        }
        assert_eq!(refusals.note(), Some(1000));
    }

    #[test]
    fn only_the_tray_relevant_writes_rebuild_the_menu() {
        assert!(touches_tray("toggle_channel_mute"));
        assert!(touches_tray("load_profile"));
        assert!(!touches_tray("set_channel_volume"), "a slider drag must not rebuild it");
        assert!(!touches_tray("get_virtual_devices"));
    }
}
