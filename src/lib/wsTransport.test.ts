import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createWsTransport, takeRemoteToken, type WsTransport } from "./wsTransport";

/**
 * A WebSocket the test drives by hand: nothing happens until `open`, `emit` or
 * `drop` is called, so every reconnect step is observable.
 */
class FakeSocket {
  static made: FakeSocket[] = [];
  readyState = 0; // CONNECTING
  sent: string[] = [];
  onopen: ((e?: unknown) => void) | null = null;
  onclose: ((e: { code: number }) => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  onerror: ((e?: unknown) => void) | null = null;

  constructor(readonly url: string) {
    FakeSocket.made.push(this);
  }

  send(data: string) {
    if (this.readyState !== 1) throw new Error("send on a socket that is not open");
    this.sent.push(data);
  }

  close(code = 1000) {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.onclose?.({ code });
  }

  // -- test drivers --
  open() {
    this.readyState = 1;
    this.onopen?.();
  }

  /** Network drop: no close frame, code 1006, as a sleeping tablet produces. */
  drop(code = 1006) {
    this.readyState = 3;
    this.onclose?.({ code });
  }

  emit(frame: unknown) {
    this.onmessage?.({ data: JSON.stringify(frame) });
  }

  frames(): Record<string, unknown>[] {
    return this.sent.map((s) => JSON.parse(s) as Record<string, unknown>);
  }
}

const latest = () => FakeSocket.made[FakeSocket.made.length - 1];

/** Longer than the first backoff step plus its jitter. */
const RETRY_WINDOW_MS = 1_000;

const createSocket = (url: string) => new FakeSocket(url) as unknown as WebSocket;

let transport: WsTransport;

/** Connected and ready, which is where most tests start. */
const connected = () => {
  transport = createWsTransport({ url: "ws://host/ws", createSocket });
  latest().open();
  return latest();
};

beforeEach(() => {
  FakeSocket.made = [];
  vi.useFakeTimers();
});

afterEach(() => {
  transport?.close();
  vi.useRealTimers();
});

describe("call", () => {
  it("matches each reply to its own call, whatever order they arrive in", async () => {
    const socket = connected();
    const first = transport.call<string>("get_prefs");
    const second = transport.call<string>("list_buses");
    await vi.advanceTimersByTimeAsync(0);

    const [a, b] = socket.frames();
    expect(a.cmd).toBe("get_prefs");
    expect(b.cmd).toBe("list_buses");
    expect(a.id).not.toBe(b.id);

    // Answered back to front - the transport must not hand over the wrong one.
    socket.emit({ t: "reply", id: b.id, ok: true, data: "buses" });
    socket.emit({ t: "reply", id: a.id, ok: true, data: "prefs" });

    await expect(first).resolves.toBe("prefs");
    await expect(second).resolves.toBe("buses");
  });

  it("rejects with the server's message on an error reply", async () => {
    const socket = connected();
    const call = transport.call("apply_update");
    await vi.advanceTimersByTimeAsync(0);
    const id = socket.frames()[0].id;

    socket.emit({ t: "reply", id, ok: false, error: "command not allowed" });

    await expect(call).rejects.toThrow("command not allowed");
  });

  it("fails fast while disconnected instead of hanging", async () => {
    const socket = connected();
    socket.drop();

    // In backoff, with no handshake to wait for.
    await expect(transport.call("get_prefs")).rejects.toThrow(/reconnecting/i);
  });

  it("rejects calls that were in flight when the socket dropped", async () => {
    const socket = connected();
    const call = transport.call("get_prefs");
    await vi.advanceTimersByTimeAsync(0);

    socket.drop();

    await expect(call).rejects.toThrow(/lost/i);
  });
});

describe("subscribe", () => {
  it("delivers payloads unwrapped and stops on unsubscribe", async () => {
    const socket = connected();
    const seen: number[] = [];
    const stop = await transport.subscribe<number>("levels", (p) => seen.push(p));

    expect(socket.frames()).toContainEqual({ t: "subscribe", event: "levels" });
    socket.emit({ t: "event", event: "levels", payload: 1 });
    expect(seen).toEqual([1]);

    stop();
    socket.emit({ t: "event", event: "levels", payload: 2 });
    expect(seen).toEqual([1]);
    expect(socket.frames()).toContainEqual({ t: "unsubscribe", event: "levels" });
  });

  it("keeps the event alive while another listener wants it", async () => {
    const socket = connected();
    const a: number[] = [];
    const b: number[] = [];
    const stopA = await transport.subscribe<number>("levels", (p) => a.push(p));
    await transport.subscribe<number>("levels", (p) => b.push(p));

    stopA();
    stopA(); // idempotent: a second call must not take the other listener down
    socket.emit({ t: "event", event: "levels", payload: 7 });

    expect(a).toEqual([]);
    expect(b).toEqual([7]);
    expect(socket.frames()).not.toContainEqual({ t: "unsubscribe", event: "levels" });
  });
});

describe("reconnect", () => {
  it("re-establishes subscriptions on the new socket and keeps delivering", async () => {
    const first = connected();
    const seen: number[] = [];
    await transport.subscribe<number>("levels", (p) => seen.push(p));

    first.drop();
    expect(transport.state()).toBe("reconnecting");

    await vi.advanceTimersByTimeAsync(RETRY_WINDOW_MS);
    const second = latest();
    expect(second).not.toBe(first);
    second.open();

    expect(transport.state()).toBe("open");
    // The call site never re-subscribed; the transport did.
    expect(second.frames()).toContainEqual({ t: "subscribe", event: "levels" });
    second.emit({ t: "event", event: "levels", payload: 3 });
    expect(seen).toEqual([3]);
  });

  it("backs off between attempts and reports the state", async () => {
    const first = connected();
    const states: string[] = [];
    transport.onStateChange((s) => states.push(s));

    first.drop();
    // Nothing immediately: a tablet that lost Wi-Fi should not spin.
    expect(FakeSocket.made).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(RETRY_WINDOW_MS);
    expect(FakeSocket.made).toHaveLength(2);

    latest().open();
    expect(states).toEqual(["reconnecting", "open"]);
  });

  it("drops a socket that stopped answering, once the server has shown it can", async () => {
    const socket = connected();

    await vi.advanceTimersByTimeAsync(15_000); // first beat
    socket.emit({ t: "pong" });
    await vi.advanceTimersByTimeAsync(15_000); // second beat, left unanswered
    expect(transport.state()).toBe("open"); // still inside the grace window

    await vi.advanceTimersByTimeAsync(5_000);
    expect(transport.state()).toBe("reconnecting");
  });

  it("leaves a server that never ponged alone", async () => {
    connected();

    // An older backend that ignores pings must not be killed every 15 s.
    await vi.advanceTimersByTimeAsync(60_000);
    expect(transport.state()).toBe("open");
  });

  it("gives up when the server rejects the token", async () => {
    const socket = connected();
    socket.drop(4001);

    await vi.advanceTimersByTimeAsync(60_000);
    expect(FakeSocket.made).toHaveLength(1); // no retry loop against a bad token
    expect(transport.state()).toBe("unauthorized");
  });
});

describe("takeRemoteToken", () => {
  const setUrl = (hash: string) => {
    window.history.replaceState(null, "", `/remote${hash}`);
  };

  beforeEach(() => {
    window.localStorage.clear();
    setUrl("");
  });

  it("takes the token out of the fragment, stores it and clears the bar", () => {
    setUrl("#token=abc123");

    expect(takeRemoteToken()).toBe("abc123");
    // Not left in history or in a screenshot of the tab.
    expect(window.location.hash).toBe("");
    // A reload after the tablet sleeps still has it.
    expect(takeRemoteToken()).toBe("abc123");
  });

  it("is null on a first visit with no token anywhere", () => {
    expect(takeRemoteToken()).toBeNull();
  });
});

describe("assetUrl", () => {
  it("points app icons at the server's icon endpoint, authenticated by cookie", () => {
    transport = createWsTransport({
      url: "ws://host/ws",
      token: "abc123",
      createSocket,
    });

    const url = new URL(transport.assetUrl("/usr/share/icons/hicolor/64x64/apps/vlc.png"), "http://host");
    expect(url.pathname).toBe("/icon");
    expect(url.searchParams.get("path")).toBe("/usr/share/icons/hicolor/64x64/apps/vlc.png");
    // Icon responses are cached for an hour, so a token in the URL would be a
    // token in the tablet's on-disk cache index. It rides the cookie instead.
    expect(url.searchParams.get("token")).toBeNull();
    expect(document.cookie).toContain("inari_remote=abc123");
  });

  it("falls back to the query when the browser refuses the cookie", () => {
    // Nothing to fall back to would mean a grid of broken icons on a tablet
    // with cookies switched off.
    const cookies = Object.getOwnPropertyDescriptor(Document.prototype, "cookie");
    Object.defineProperty(document, "cookie", {
      configurable: true,
      get: () => "",
      set: () => {},
    });
    try {
      transport = createWsTransport({ url: "ws://host/ws", token: "abc123", createSocket });
      const url = new URL(transport.assetUrl("/usr/share/pixmaps/vlc.png"), "http://host");
      expect(url.searchParams.get("token")).toBe("abc123");
    } finally {
      delete (document as unknown as Record<string, unknown>).cookie;
      if (cookies) Object.defineProperty(Document.prototype, "cookie", cookies);
    }
  });
});
