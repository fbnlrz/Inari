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

const channel = (name: string, volume = 100): VirtualSink => ({
  name,
  label: name.replace("sink_", ""),
  icon: null,
  volume_percent: volume,
  muted: false,
  stream_mix: true,
});

const initialState = useMixerStore.getState();

/** name -> volume after the (optimistic, synchronous) apply. */
const volumes = () =>
  Object.fromEntries(useMixerStore.getState().channels.map((c) => [c.name, c.volume_percent]));

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

describe("applyChatMix channel resolution", () => {
  it("uses the saved balance picks", () => {
    useMixerStore.setState({
      channels: [channel("sink_game"), channel("sink_chat"), channel("sink_media")],
      balanceA: "sink_media",
      balanceB: "sink_chat",
    });

    useMixerStore.getState().applyChatMix(30, 80);

    expect(volumes()).toEqual({ sink_game: 100, sink_media: 30, sink_chat: 80 });
  });

  it("falls back to Game/Chat when nothing is saved", () => {
    useMixerStore.setState({
      channels: [channel("sink_media"), channel("sink_chat"), channel("sink_game")],
    });

    useMixerStore.getState().applyChatMix(30, 80);

    // Order in the strip list is irrelevant - the well-known names win.
    expect(volumes()).toEqual({ sink_media: 100, sink_game: 30, sink_chat: 80 });
  });

  it("falls back to Game/Chat when a saved pick no longer exists", () => {
    useMixerStore.setState({
      channels: [channel("sink_game"), channel("sink_chat")],
      balanceA: "sink_deleted",
      balanceB: null,
    });

    useMixerStore.getState().applyChatMix(10, 90);

    expect(volumes()).toEqual({ sink_game: 10, sink_chat: 90 });
  });

  it("falls back to the first two channels on a renamed layout", () => {
    useMixerStore.setState({
      channels: [channel("sink_main"), channel("sink_voice"), channel("sink_music")],
    });

    useMixerStore.getState().applyChatMix(20, 70);

    expect(volumes()).toEqual({ sink_main: 20, sink_voice: 70, sink_music: 100 });
  });

  it("mixes a saved pick with a well-known fallback", () => {
    useMixerStore.setState({
      channels: [channel("sink_media"), channel("sink_chat")],
      balanceA: "sink_media",
      balanceB: null,
    });

    useMixerStore.getState().applyChatMix(40, 60);

    expect(volumes()).toEqual({ sink_media: 40, sink_chat: 60 });
  });

  it("skips the second-channel fallback that would pick the first one twice", () => {
    useMixerStore.setState({
      channels: [channel("sink_game"), channel("sink_media")],
      balanceB: "sink_game", // same channel the A side resolves to
    });

    useMixerStore.getState().applyChatMix(40, 60);

    // Ducking one channel against itself is meaningless - leave both alone.
    expect(volumes()).toEqual({ sink_game: 100, sink_media: 100 });
  });

  it("does nothing with fewer than two channels", () => {
    useMixerStore.setState({ channels: [channel("sink_game")] });

    useMixerStore.getState().applyChatMix(40, 60);

    expect(volumes()).toEqual({ sink_game: 100 });
    vi.advanceTimersByTime(200);
    expect(call).not.toHaveBeenCalled();
  });

  it("does nothing with no channels at all", () => {
    useMixerStore.getState().applyChatMix(40, 60);

    vi.advanceTimersByTime(200);
    expect(call).not.toHaveBeenCalled();
  });
});

describe("applyChatMix values", () => {
  beforeEach(() => {
    useMixerStore.setState({ channels: [channel("sink_game"), channel("sink_chat")] });
  });

  it("clamps and rounds into 0-100", () => {
    useMixerStore.getState().applyChatMix(-20, 140.6);

    expect(volumes()).toEqual({ sink_game: 0, sink_chat: 100 });
  });

  it("rounds fractional headset positions", () => {
    useMixerStore.getState().applyChatMix(33.4, 66.5);

    expect(volumes()).toEqual({ sink_game: 33, sink_chat: 67 });
  });

  it("reaches the backend through the normal debounced volume path", () => {
    useMixerStore.getState().applyChatMix(25, 75);

    expect(call).not.toHaveBeenCalled(); // still inside the debounce window
    vi.advanceTimersByTime(100);
    expect(call).toHaveBeenCalledWith("set_channel_volume", {
      sinkName: "sink_game",
      volume: 25,
    });
    expect(call).toHaveBeenCalledWith("set_channel_volume", {
      sinkName: "sink_chat",
      volume: 75,
    });
  });
});
