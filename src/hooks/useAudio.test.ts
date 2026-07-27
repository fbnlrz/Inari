import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";

// Same boundary mock as src/store/mixer.test.ts: the store talks to Rust
// through src/lib/ipc, and this hook additionally subscribes to backend events.
const call = vi.fn();

type Handler = (payload: never) => void;
const unsubscribe = vi.fn();
const subscribe = vi.fn<(event: string, handler: Handler) => Promise<typeof unsubscribe>>(() =>
  Promise.resolve(unsubscribe),
);
vi.mock("../lib/ipc", () => ({
  call: (...args: unknown[]) => call(...args),
  subscribe: (event: string, handler: Handler) => subscribe(event, handler),
}));

import { useAudio } from "./useAudio";
import { useMixerStore } from "../store/mixer";
import type { OutputDevice, ProfileInfo } from "../types";

const device = (name: string): OutputDevice => ({ index: 0, name, description: name });
const profile = (name: string, trigger: string | null = null): ProfileInfo => ({
  name,
  trigger_device: trigger,
});

const initialState = useMixerStore.getState();

// jsdom drives document.hidden off visibilityState, which tests can't set;
// override the getter so the tray-hidden path is reachable.
let hidden = false;
Object.defineProperty(document, "hidden", {
  configurable: true,
  get: () => hidden,
});
const setHidden = (value: boolean) => {
  hidden = value;
  document.dispatchEvent(new Event("visibilitychange"));
};

/** Store actions the hook calls, replaced by spies. */
const spyActions = () => ({
  initialize: vi.fn(async () => {}),
  fetchAppStreams: vi.fn(async () => {}),
  fetchOutputs: vi.fn(async () => {}),
  fetchSeenApps: vi.fn(async () => {}),
  loadProfile: vi.fn<(name: string) => Promise<void>>(async () => {}),
});
let actions: ReturnType<typeof spyActions>;

/** How many times one mixer refresh's worth of fetches has run. */
const pollCount = () => actions.fetchAppStreams.mock.calls.length;
const seenCount = () => actions.fetchSeenApps.mock.calls.length;

/** The handler registered for a backend event, by event name. */
const handlerFor = (event: string) => {
  const entry = subscribe.mock.calls.find((c) => c[0] === event);
  if (!entry) throw new Error(`no listener for ${event}`);
  return entry[1];
};
const fireGraphChanged = () =>
  act(async () => handlerFor("graph-changed")(undefined as never));

beforeEach(() => {
  vi.useFakeTimers();
  hidden = false;
  call.mockReset();
  call.mockResolvedValue(undefined);
  subscribe.mockClear();
  unsubscribe.mockClear();
  useMixerStore.setState(initialState, true);
  actions = spyActions();
  // Set before render: the poll effect keys on these identities.
  useMixerStore.setState(actions);
});

afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
});

describe("refresh lifecycle", () => {
  /** The native backend has reported in, so `graph-changed` events exist. */
  const nativeBackend = () => useMixerStore.setState({ backendNative: true });

  it("initializes and refreshes immediately", () => {
    renderHook(() => useAudio());

    expect(actions.initialize).toHaveBeenCalledTimes(1);
    // A returning window must not show stale data, so start() refreshes now.
    expect(pollCount()).toBe(1);
    expect(actions.fetchOutputs).toHaveBeenCalledTimes(1);
    expect(seenCount()).toBe(1);
  });

  it("keeps the 2s poll while the backend kind is still unknown", () => {
    renderHook(() => useAudio());

    // backendNative is null until get_backend_info answers; never be slower
    // than the old behaviour in that window.
    act(() => void vi.advanceTimersByTime(2000));
    expect(pollCount()).toBe(2);
    act(() => void vi.advanceTimersByTime(4000));
    expect(pollCount()).toBe(4);
  });

  it("keeps the 2s poll on the pactl fallback, which emits no graph events", () => {
    useMixerStore.setState({ backendNative: false });
    renderHook(() => useAudio());

    act(() => void vi.advanceTimersByTime(2000));
    expect(pollCount()).toBe(2);
  });

  it("drops to the 15s backstop once the native backend reports in", () => {
    nativeBackend();
    renderHook(() => useAudio());
    expect(pollCount()).toBe(1);

    // Events are the primary path now; the timer is only insurance.
    act(() => void vi.advanceTimersByTime(14_000));
    expect(pollCount()).toBe(1);
    act(() => void vi.advanceTimersByTime(1000));
    expect(pollCount()).toBe(2);
  });

  it("switches to the backstop when the backend kind arrives after mount", () => {
    renderHook(() => useAudio());
    act(() => void vi.advanceTimersByTime(2000));
    expect(pollCount()).toBe(2);

    act(() => nativeBackend());

    expect(pollCount()).toBe(3); // the restarted timer refreshes once
    act(() => void vi.advanceTimersByTime(2000));
    expect(pollCount()).toBe(3); // the 2s poll is gone
    act(() => void vi.advanceTimersByTime(13_000));
    expect(pollCount()).toBe(4);
  });

  it("refreshes app history on its own slow timer, off the mixer cadence", () => {
    nativeBackend();
    renderHook(() => useAudio());
    expect(seenCount()).toBe(1);

    act(() => void vi.advanceTimersByTime(30_000));
    expect(pollCount()).toBe(3); // mixer ticked twice
    expect(seenCount()).toBe(1); // history did not

    act(() => void vi.advanceTimersByTime(30_000));
    expect(seenCount()).toBe(2);
  });

  it("does not refresh while the window starts hidden in the tray", () => {
    hidden = true;
    renderHook(() => useAudio());

    // initialize still runs - the sinks must exist even if the UI is hidden.
    expect(actions.initialize).toHaveBeenCalledTimes(1);
    expect(pollCount()).toBe(0);

    act(() => void vi.advanceTimersByTime(10_000));
    expect(pollCount()).toBe(0);
  });

  it("stops on hide and resumes with an immediate refresh on show", () => {
    renderHook(() => useAudio());
    expect(pollCount()).toBe(1);

    act(() => setHidden(true));
    act(() => void vi.advanceTimersByTime(10_000));
    expect(pollCount()).toBe(1); // interval cleared, nothing ticked

    act(() => setHidden(false));
    expect(pollCount()).toBe(2); // refreshed at once, not one tick later
    expect(seenCount()).toBe(2);
    act(() => void vi.advanceTimersByTime(2000));
    expect(pollCount()).toBe(3);
  });

  it("ignores a redundant show event instead of stacking intervals", () => {
    renderHook(() => useAudio());

    act(() => setHidden(false)); // already visible
    expect(pollCount()).toBe(1); // start() is a no-op while a timer exists

    act(() => void vi.advanceTimersByTime(2000));
    expect(pollCount()).toBe(2); // one interval, not two
  });

  it("clears the timers and the visibility listener on unmount", () => {
    const remove = vi.spyOn(document, "removeEventListener");
    const { unmount } = renderHook(() => useAudio());

    unmount();
    act(() => void vi.advanceTimersByTime(60_000));

    expect(pollCount()).toBe(1); // no ticks after unmount
    expect(remove).toHaveBeenCalledWith("visibilitychange", expect.any(Function));
    remove.mockRestore();
  });
});

describe("graph-changed subscription", () => {
  beforeEach(() => useMixerStore.setState({ backendNative: true }));

  it("refetches streams and outputs on the event, without waiting for the backstop", async () => {
    renderHook(() => useAudio());
    expect(pollCount()).toBe(1);

    await fireGraphChanged();

    expect(pollCount()).toBe(2);
    expect(actions.fetchOutputs).toHaveBeenCalledTimes(2);
  });

  it("reads app history back only after the stream fetch that writes it", async () => {
    renderHook(() => useAudio());
    const order: string[] = [];
    actions.fetchAppStreams.mockImplementation(async () => void order.push("streams"));
    actions.fetchSeenApps.mockImplementation(async () => void order.push("seen"));

    await fireGraphChanged();

    expect(order).toEqual(["streams", "seen"]);
  });

  it("ignores events while hidden in the tray", async () => {
    renderHook(() => useAudio());
    act(() => setHidden(true));
    expect(pollCount()).toBe(1);

    await fireGraphChanged();

    expect(pollCount()).toBe(1); // the show handler refreshes on return
    act(() => setHidden(false));
    expect(pollCount()).toBe(2);
  });

  it("still refreshes on the backstop after an event", async () => {
    renderHook(() => useAudio());
    await fireGraphChanged();
    expect(pollCount()).toBe(2);

    act(() => void vi.advanceTimersByTime(15_000));

    expect(pollCount()).toBe(3);
  });
});

describe("event subscriptions", () => {
  it("subscribes to every event and unsubscribes on unmount", async () => {
    const { unmount } = renderHook(() => useAudio());

    expect(subscribe.mock.calls.map((c) => c[0])).toEqual([
      "graph-changed",
      "levels",
      "profile-changed",
    ]);

    unmount();
    // The cleanups await the subscribe() promise before calling the unsubscribe fn.
    await act(async () => {});
    expect(unsubscribe).toHaveBeenCalledTimes(3);
  });

  it("routes level events into the store", () => {
    renderHook(() => useAudio());

    act(() => handlerFor("levels")({ sink_game: [0.5, 0.25] } as never));

    expect(useMixerStore.getState().levels["sink_game"]).toEqual([0.5, 0.25]);
  });
});

describe("hardware profile auto-switch", () => {
  const render = () => renderHook(() => useAudio());
  const setDevices = (names: string[]) =>
    act(() => useMixerStore.setState({ outputDevices: names.map(device) }));

  it("learns the first non-empty device set without switching profiles", () => {
    useMixerStore.setState({ profiles: [profile("Headset", "hw_headset")] });
    render();

    // Plugged in before the app started: not a hotplug, must not steal the
    // user's current profile.
    setDevices(["hw_headset"]);

    expect(actions.loadProfile).not.toHaveBeenCalled();
  });

  it("loads the bound profile when a new device appears", () => {
    useMixerStore.setState({ profiles: [profile("Headset", "hw_headset")] });
    render();
    setDevices(["hw_speakers"]); // first sample: learn only

    setDevices(["hw_speakers", "hw_headset"]);

    expect(actions.loadProfile).toHaveBeenCalledWith("Headset");
  });

  it("ignores a new device with no profile bound to it", () => {
    useMixerStore.setState({ profiles: [profile("Headset", "hw_headset"), profile("Other")] });
    render();
    setDevices(["hw_speakers"]);

    setDevices(["hw_speakers", "hw_hdmi"]);

    expect(actions.loadProfile).not.toHaveBeenCalled();
  });

  it("does not re-switch while a known device stays plugged in", () => {
    useMixerStore.setState({ profiles: [profile("Headset", "hw_headset")] });
    render();
    setDevices(["hw_speakers"]);
    setDevices(["hw_speakers", "hw_headset"]);
    expect(actions.loadProfile).toHaveBeenCalledTimes(1);

    setDevices(["hw_headset"]); // speakers unplugged, headset unchanged

    expect(actions.loadProfile).toHaveBeenCalledTimes(1);
  });

  it("switches again after the bound device is unplugged and re-plugged", () => {
    useMixerStore.setState({ profiles: [profile("Headset", "hw_headset")] });
    render();
    setDevices(["hw_speakers"]);
    setDevices(["hw_speakers", "hw_headset"]);
    setDevices(["hw_speakers"]);

    setDevices(["hw_speakers", "hw_headset"]);

    expect(actions.loadProfile).toHaveBeenCalledTimes(2);
  });

  it("treats an empty device list as no sample at all", () => {
    useMixerStore.setState({ profiles: [profile("Headset", "hw_headset")] });
    render();

    // Backend not up yet -> [] -> the ref stays null, so the first real
    // sample is still only learned, never acted on.
    setDevices([]);
    setDevices(["hw_headset"]);

    expect(actions.loadProfile).not.toHaveBeenCalled();
  });
});
