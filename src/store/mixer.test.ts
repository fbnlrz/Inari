import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The store talks to the Rust backend through src/lib/ipc; mock that boundary.
const call = vi.fn();
const subscribe = vi.fn<(event: string, handler: (payload: never) => void) => Promise<() => void>>(
  () => Promise.resolve(() => {}),
);
vi.mock("../lib/ipc", () => ({
  call: (...args: unknown[]) => call(...args),
  subscribe: (event: string, handler: (payload: never) => void) => subscribe(event, handler),
}));

import { useMixerStore } from "./mixer";
import type { VirtualSink } from "../types";
import { defaultEqConfig } from "../types";

const channel = (name: string, volume = 100): VirtualSink => ({
  name,
  label: name.replace("sink_", ""),
  icon: null,
  volume_percent: volume,
  muted: false,
  stream_mix: true,
});

const initialState = useMixerStore.getState();

beforeEach(() => {
  vi.useFakeTimers();
  call.mockReset();
  call.mockResolvedValue(undefined);
  useMixerStore.setState(initialState, true);
});

afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
});

describe("setChannelVolume", () => {
  it("updates the UI immediately and debounces the backend call", async () => {
    useMixerStore.setState({ channels: [channel("sink_game")] });
    const store = useMixerStore.getState();

    await store.setChannelVolume("sink_game", 40);
    await store.setChannelVolume("sink_game", 55);

    // Optimistic: the strip moved on the second call already…
    expect(useMixerStore.getState().channels[0].volume_percent).toBe(55);
    // …but the backend hasn't been hit yet (drag in progress).
    expect(call).not.toHaveBeenCalled();

    vi.advanceTimersByTime(100);
    // Only the final value of the drag reaches the backend.
    expect(call).toHaveBeenCalledTimes(1);
    expect(call).toHaveBeenCalledWith("set_channel_volume", {
      sinkName: "sink_game",
      volume: 55,
    });
  });

  it("keeps per-channel debounce keys separate", async () => {
    useMixerStore.setState({ channels: [channel("sink_game"), channel("sink_chat")] });
    const store = useMixerStore.getState();

    await store.setChannelVolume("sink_game", 10);
    await store.setChannelVolume("sink_chat", 20);
    vi.advanceTimersByTime(100);

    expect(call).toHaveBeenCalledTimes(2);
  });
});

describe("toggleMonitor", () => {
  it("flips optimistically and calls the backend", async () => {
    const store = useMixerStore.getState();
    await store.toggleMonitor("sink_game");

    expect(useMixerStore.getState().monitors["sink_game"]).toBe(true);
    expect(call).toHaveBeenCalledWith("set_monitor", {
      sinkName: "sink_game",
      enabled: true,
    });

    await useMixerStore.getState().toggleMonitor("sink_game");
    expect(useMixerStore.getState().monitors["sink_game"]).toBe(false);
  });

  it("reverts the optimistic flip when the backend rejects", async () => {
    call.mockRejectedValueOnce("monitoring requires the native PipeWire backend");
    const store = useMixerStore.getState();

    await store.toggleMonitor("sink_game");

    const s = useMixerStore.getState();
    expect(s.monitors["sink_game"]).toBe(false);
    expect(s.error).toContain("native PipeWire");
  });
});

describe("setLevels", () => {
  it("stores per-sink peaks", () => {
    useMixerStore.getState().setLevels({ sink_game: [0.5, 0.4] });
    expect(useMixerStore.getState().levels["sink_game"]).toEqual([0.5, 0.4]);
  });
});

describe("setChannelEq", () => {
  it("applies optimistically and debounces per channel", async () => {
    const store = useMixerStore.getState();
    const config = {
      ...defaultEqConfig(),
      enabled: true,
    };

    await store.setChannelEq("sink_game", { ...config, preamp_db: -2 });
    await store.setChannelEq("sink_game", { ...config, preamp_db: -5 });
    await store.setChannelEq("sink_chat", config);

    // Optimistic: both channels reflect their latest config immediately…
    expect(useMixerStore.getState().eqConfigs["sink_game"].preamp_db).toBe(-5);
    expect(useMixerStore.getState().eqConfigs["sink_chat"].enabled).toBe(true);
    // …but nothing has hit the backend yet (drag in progress).
    expect(call).not.toHaveBeenCalled();

    vi.advanceTimersByTime(100);
    // One call per channel: sink_game's two edits collapsed into the last.
    expect(call).toHaveBeenCalledTimes(2);
    expect(call).toHaveBeenCalledWith("set_channel_eq", {
      sinkName: "sink_game",
      config: { ...config, preamp_db: -5 },
    });
    expect(call).toHaveBeenCalledWith("set_channel_eq", {
      sinkName: "sink_chat",
      config,
    });
  });
});
