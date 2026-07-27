/**
 * The transport the tablet remote runs on: the same `Transport` interface as
 * the Tauri bridge, spoken over a WebSocket to the origin that served the page.
 *
 * Two things shape this file more than anything else.
 *
 * First, the connection is expected to break. A tablet propped next to the
 * keyboard sleeps whenever it is ignored for a minute, and Wi-Fi roams. So a
 * drop is not an error path, it is the normal path: reconnect with backoff,
 * re-send the subscriptions the app never knew it lost, and let the UI say
 * "reconnecting" instead of quietly showing stale faders forever.
 *
 * Second, nothing here decides what may be called. The allowlist lives in Rust
 * and is a positive list; this file just carries names across. A client that
 * asks for something not on it gets an error reply, which is exactly what a
 * client that asked for a bad channel id gets.
 */
import { setTransport } from "./ipc";
import type { Args, Command, EventName, Transport, Unsubscribe } from "./ipc";

/** Where the token from the QR code is kept between visits. */
const TOKEN_KEY = "inari.remote.token";

/** Backoff bounds. Fast enough to feel instant on a screen wake, slow enough
 * not to hammer a host that is simply off. */
const RETRY_MIN_MS = 500;
const RETRY_MAX_MS = 10_000;

/** How long a call may wait on a handshake that is already in flight. Past
 * this we would rather fail than let a fader hang. */
const CONNECT_WAIT_MS = 5_000;

/** Upper bound on a single call. The heartbeat below catches most dead
 * sockets, but a call must never be able to hang forever. */
const CALL_TIMEOUT_MS = 15_000;

/** App-level heartbeat. The browser gives JS no access to WebSocket pings, and
 * a socket that died while the tablet slept can stay `OPEN` for minutes. */
const PING_INTERVAL_MS = 15_000;
const PONG_GRACE_MS = 5_000;

/** Close code the server uses for a missing or wrong token. Retrying that is
 * pointless - the token will not fix itself - so it stops the loop. */
const CLOSE_UNAUTHORIZED = 4001;

// WebSocket.CONNECTING / OPEN. Spelled out so the module does not reach for a
// global the tests replace.
const CONNECTING = 0;
const OPEN = 1;

export type ConnectionState =
  | "connecting"
  | "open"
  | "reconnecting"
  | "unauthorized"
  | "closed";

export interface WsTransport extends Transport {
  /** Current link state, for the "reconnecting" hint in the UI. */
  state(): ConnectionState;
  /** Fires on every state change; call the result to stop listening. */
  onStateChange(fn: (state: ConnectionState) => void): Unsubscribe;
  /** Stop for good: no further reconnects. */
  close(): void;
}

export interface WsTransportOptions {
  /** Defaults to the origin that served the page. Never a hardcoded host. */
  url?: string;
  /** Token from the QR code; sent on the handshake and with icon requests. */
  token?: string | null;
  /** Injection seam for tests. */
  createSocket?: (url: string) => WebSocket;
}

/** Client -> server. `t` is the frame kind; `id` correlates a reply. */
type Frame =
  | { t: "call"; id: number; cmd: Command; args: Args }
  | { t: "subscribe"; event: EventName }
  | { t: "unsubscribe"; event: EventName }
  | { t: "ping" };

/** Server -> client. Anything else on the wire is ignored, so the server can
 * add frame kinds without breaking older tablets. */
type ServerFrame =
  | { t: "reply"; id: number; ok: true; data: unknown }
  | { t: "reply"; id: number; ok: false; error: string }
  | { t: "event"; event: EventName; payload: unknown }
  | { t: "pong" };

interface Pending {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const DISCONNECTED = "Not connected to Inari - reconnecting";

/**
 * Reads the token the QR code put in the URL fragment, remembers it, and takes
 * it back out of the address bar.
 *
 * The fragment is the only place a token can ride without ever reaching a
 * server log, but it survives in history and in any screenshot of the tab, so
 * it gets stripped the moment it has been read. Later visits (the tablet's
 * bookmark, a reload after sleep) use the stored copy.
 */
export function takeRemoteToken(): string | null {
  const fragment = window.location.hash.replace(/^#/, "");
  const fromUrl = new URLSearchParams(fragment).get("token");
  if (fromUrl) {
    try {
      window.localStorage.setItem(TOKEN_KEY, fromUrl);
    } catch {
      // Private browsing denies storage; the token still works for this visit.
    }
    window.history.replaceState(
      null,
      "",
      window.location.pathname + window.location.search,
    );
    return fromUrl;
  }
  try {
    return window.localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

/** `ws(s)://<this host>/ws`, with the token as a query parameter so the server
 * can reject an unknown client at the handshake rather than after upgrade. */
function defaultUrl(token: string | null | undefined): string {
  const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  const query = token ? `?token=${encodeURIComponent(token)}` : "";
  return `${scheme}//${window.location.host}/ws${query}`;
}

/** Cookie the icon endpoint authenticates with. Matches `TOKEN_COOKIE` in
 * src-tauri/src/remote/server.rs. */
const ICON_COOKIE = "inari_remote";

/**
 * Hand the token to the icon endpoint as a cookie, and say whether it stuck.
 *
 * An `<img>` cannot carry a header, so the credential has to travel on its
 * own - but icon responses are cached for an hour, and a token in the URL is a
 * token written into the tablet's on-disk cache index and into anything that
 * ever logs a URL. A cookie is sent automatically and appears in neither.
 *
 * The read-back is the point of the return value: a browser set to refuse
 * cookies fails silently here, and the caller falls back to the query form
 * rather than showing a grid of broken icons.
 */
function setIconCookie(token: string): boolean {
  try {
    document.cookie = `${ICON_COOKIE}=${encodeURIComponent(token)}; Path=/; SameSite=Strict`;
    return document.cookie.includes(`${ICON_COOKIE}=`);
  } catch {
    return false;
  }
}

/** Browser-loadable URL for a host-side icon file. Carries the token only when
 * the cookie could not be set. */
function iconUrl(path: string, token: string | null | undefined): string {
  const params = new URLSearchParams({ path });
  if (token) params.set("token", token);
  return `/icon?${params.toString()}`;
}

let installed: WsTransport | null = null;

/**
 * The transport the remote client is running on, or null in the desktop app.
 * Lets any component ask for the link state without threading the instance
 * down from `main.tsx`.
 */
export function remoteTransport(): WsTransport | null {
  return installed;
}

/**
 * Take the token out of the URL, connect, and make this the app's transport.
 * Called once from `main.tsx` when the app is not running inside the webview.
 */
export function installRemoteTransport(options: WsTransportOptions = {}): WsTransport {
  const transport = createWsTransport({ token: takeRemoteToken(), ...options });
  installed = transport;
  setTransport(transport);
  return transport;
}

export function createWsTransport(options: WsTransportOptions = {}): WsTransport {
  const token = options.token ?? null;
  // Set before anything renders, so no icon request can go out ahead of the
  // credential it needs.
  const cookieHolds = token !== null && setIconCookie(token);
  const url = options.url ?? defaultUrl(token);
  const createSocket = options.createSocket ?? ((u: string) => new WebSocket(u));

  let socket: WebSocket | null = null;
  let state: ConnectionState = "closed";
  let stopped = false;
  let attempt = 0;
  let nextId = 1;

  const pending = new Map<number, Pending>();
  /** One entry per live subscription, so the same function may subscribe twice
   * and each unsubscribe only cancels its own. */
  const listeners = new Map<EventName, Set<(payload: unknown) => void>>();
  const watchers = new Set<(state: ConnectionState) => void>();
  let openWaiters: Pending[] = [];

  let retryTimer: ReturnType<typeof setTimeout> | null = null;
  let pingTimer: ReturnType<typeof setInterval> | null = null;
  let pongTimer: ReturnType<typeof setTimeout> | null = null;
  /** Only enforce the pong deadline once the server has proven it answers
   * pings - an older backend would otherwise be killed every 15 s. */
  let pongSeen = false;

  function setState(next: ConnectionState) {
    if (state === next) return;
    state = next;
    for (const watcher of [...watchers]) watcher(next);
  }

  function send(frame: Frame): boolean {
    if (!socket || socket.readyState !== OPEN) return false;
    socket.send(JSON.stringify(frame));
    return true;
  }

  function connect() {
    if (stopped || (socket && socket.readyState <= OPEN)) return;
    setState(attempt === 0 ? "connecting" : "reconnecting");
    const ws = createSocket(url);
    socket = ws;
    ws.onopen = () => {
      attempt = 0;
      setState("open");
      // The server keeps no per-client state across a drop, so the app's live
      // subscriptions are re-declared here rather than by any call site.
      for (const event of listeners.keys()) send({ t: "subscribe", event });
      const waiters = openWaiters;
      openWaiters = [];
      for (const waiter of waiters) {
        clearTimeout(waiter.timer);
        waiter.resolve(ws);
      }
      startHeartbeat();
    };
    ws.onmessage = (e: MessageEvent) => receive(e.data);
    ws.onclose = (e: CloseEvent) => {
      if (ws !== socket) return; // a socket we already replaced
      socket = null;
      onClosed(e.code);
    };
    // No `onerror`: it is always followed by `close`, which already handles it.
  }

  function onClosed(code: number) {
    stopHeartbeat();
    fail(new Error("Connection to Inari lost"));
    if (stopped) {
      setState("closed");
      return;
    }
    if (code === CLOSE_UNAUTHORIZED) {
      stopped = true;
      setState("unauthorized");
      return;
    }
    setState("reconnecting");
    scheduleRetry();
  }

  /** Everything waiting on the socket gives up together. */
  function fail(reason: Error) {
    for (const p of pending.values()) {
      clearTimeout(p.timer);
      p.reject(reason);
    }
    pending.clear();
    const waiters = openWaiters;
    openWaiters = [];
    for (const waiter of waiters) {
      clearTimeout(waiter.timer);
      waiter.reject(reason);
    }
  }

  function scheduleRetry() {
    if (retryTimer !== null) return;
    // Exponential with jitter: a household of clients waking together should
    // not retry in lockstep.
    const base = Math.min(RETRY_MAX_MS, RETRY_MIN_MS * 2 ** attempt);
    attempt += 1;
    const delay = base * (0.75 + Math.random() * 0.5);
    retryTimer = setTimeout(() => {
      retryTimer = null;
      connect();
    }, delay);
  }

  /** A screen wake or a regained network is better evidence than any timer. */
  function retryNow() {
    if (stopped) return;
    if (state === "open") {
      // A socket that survived a sleep may be open in name only; probe it
      // rather than wait for the next scheduled beat.
      ping();
      return;
    }
    if (retryTimer !== null) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
    attempt = 0;
    connect();
  }

  function ping() {
    if (!send({ t: "ping" })) return;
    if (pongSeen && pongTimer === null) {
      pongTimer = setTimeout(() => {
        pongTimer = null;
        // Unanswered: the socket is half-open. Closing it routes through the
        // normal drop path, backoff and all.
        socket?.close();
      }, PONG_GRACE_MS);
    }
  }

  function startHeartbeat() {
    stopHeartbeat();
    pingTimer = setInterval(ping, PING_INTERVAL_MS);
  }

  function stopHeartbeat() {
    if (pingTimer !== null) clearInterval(pingTimer);
    if (pongTimer !== null) clearTimeout(pongTimer);
    pingTimer = null;
    pongTimer = null;
  }

  function receive(data: unknown) {
    if (typeof data !== "string") return;
    let frame: ServerFrame;
    try {
      frame = JSON.parse(data) as ServerFrame;
    } catch {
      return; // garbage on the wire is not worth killing a session over
    }
    if (frame.t === "pong") {
      pongSeen = true;
      if (pongTimer !== null) {
        clearTimeout(pongTimer);
        pongTimer = null;
      }
      return;
    }
    if (frame.t === "reply") {
      const p = pending.get(frame.id);
      if (!p) return; // already timed out, or a reply to a dead session
      pending.delete(frame.id);
      clearTimeout(p.timer);
      if (frame.ok) p.resolve(frame.data);
      else p.reject(new Error(frame.error));
      return;
    }
    if (frame.t === "event") {
      const set = listeners.get(frame.event);
      if (!set) return;
      // Copy: a callback may unsubscribe itself while we iterate.
      for (const fn of [...set]) fn(frame.payload);
    }
  }

  /**
   * The open socket, or a rejection. A handshake already in flight is worth
   * waiting out - that is the tablet coming back - but once we are down to
   * backoff there is nothing to wait for, and a call that hangs is worse than
   * one that says why it failed.
   */
  function ready(): Promise<WebSocket> {
    if (socket && socket.readyState === OPEN) return Promise.resolve(socket);
    if (socket && socket.readyState === CONNECTING) {
      return new Promise<WebSocket>((resolve, reject) => {
        const waiter: Pending = {
          resolve: resolve as (value: unknown) => void,
          reject,
          timer: setTimeout(() => {
            openWaiters = openWaiters.filter((w) => w !== waiter);
            reject(new Error(DISCONNECTED));
          }, CONNECT_WAIT_MS),
        };
        openWaiters.push(waiter);
      });
    }
    return Promise.reject(new Error(DISCONNECTED));
  }

  async function call<T>(cmd: Command, args: Args = {}): Promise<T> {
    const ws = await ready();
    const id = nextId++;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`Inari did not answer "${cmd}"`));
      }, CALL_TIMEOUT_MS);
      pending.set(id, { resolve: resolve as (value: unknown) => void, reject, timer });
      try {
        ws.send(JSON.stringify({ t: "call", id, cmd, args } satisfies Frame));
      } catch (e) {
        pending.delete(id);
        clearTimeout(timer);
        reject(e instanceof Error ? e : new Error(String(e)));
      }
    });
  }

  function subscribe<T>(
    event: EventName,
    onPayload: (payload: T) => void,
  ): Promise<Unsubscribe> {
    const handler = (payload: unknown) => onPayload(payload as T);
    let set = listeners.get(event);
    if (!set) {
      set = new Set();
      listeners.set(event, set);
      // While down this is a no-op; `onopen` re-sends every live event.
      send({ t: "subscribe", event });
    }
    set.add(handler);

    let live = true;
    return Promise.resolve(() => {
      if (!live) return; // unsubscribing twice must not cancel someone else
      live = false;
      const current = listeners.get(event);
      if (!current) return;
      current.delete(handler);
      if (current.size === 0) {
        listeners.delete(event);
        send({ t: "unsubscribe", event });
      }
    });
  }

  const onWake = () => {
    if (document.visibilityState === "visible") retryNow();
  };
  window.addEventListener("online", retryNow);
  document.addEventListener("visibilitychange", onWake);

  connect();

  return {
    call,
    subscribe,
    assetUrl: (path: string) => iconUrl(path, cookieHolds ? null : token),
    state: () => state,
    onStateChange(fn) {
      watchers.add(fn);
      return () => {
        watchers.delete(fn);
      };
    },
    close() {
      stopped = true;
      if (retryTimer !== null) clearTimeout(retryTimer);
      retryTimer = null;
      stopHeartbeat();
      window.removeEventListener("online", retryNow);
      document.removeEventListener("visibilitychange", onWake);
      fail(new Error(DISCONNECTED));
      socket?.close();
      socket = null;
      setState("closed");
    },
  };
}
