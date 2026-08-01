/**
 * Races between an optimistic write and a read that overtakes it.
 *
 * All three of these were found the same way: something other than the control
 * you are touching changes the state, the store refetches, and the refetch —
 * or the write — carries a value from before your change. They are grouped in
 * one file because they are one mistake made in three places, and a fourth
 * would look exactly like them.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const call = vi.fn();
vi.mock("../lib/ipc", () => ({
  call: (...args: unknown[]) => call(...args),
  subscribe: () => Promise.resolve(() => {}),
}));

import { useMixerStore } from "./mixer";
import { IDLE, useMedia, type MediaStatus } from "./media";
import type { MicConfig, VirtualSink } from "../types";

const channel = (name: string, volume = 100): VirtualSink => ({
  name,
  label: name.replace("sink_", ""),
  icon: null,
  volume_percent: volume,
  muted: false,
  stream_mix: true,
});

const mic = (over: Partial<MicConfig> = {}): MicConfig =>
  ({
    enabled: true,
    input_device: null,
    output_label: "Inari Mic",
    gain_percent: 100,
    gate_enabled: false,
    comp_enabled: false,
    limiter_enabled: false,
    muted: false,
    gate_threshold_db: -40,
    comp_threshold_db: -18,
    comp_ratio: 3,
    limiter_ceiling_db: -1,
    ...over,
  }) as MicConfig;

const mixerInitial = useMixerStore.getState();
const mediaInitial = useMedia.getState();

describe("optimistic writes versus reads that overtake them", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    call.mockReset();
    call.mockResolvedValue(undefined);
    useMixerStore.setState(mixerInitial, true);
    useMedia.setState(mediaInitial, true);
  });
  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it("a refetch during the volume debounce does not hand the fader its old position back", async () => {
    useMixerStore.setState({ channels: [channel("sink_chat", 80)] });
    const store = useMixerStore.getState();

    await store.setChannelVolume("sink_chat", 30);
    expect(useMixerStore.getState().channels[0].volume_percent).toBe(30);

    // Anything at all triggers state-changed — a hotkey, the tray, the CLI,
    // a tablet — and the backend still holds 80 until the debounce fires.
    call.mockImplementation((cmd: string) =>
      cmd === "get_virtual_devices"
        ? Promise.resolve([channel("sink_chat", 80)])
        : Promise.resolve(undefined),
    );
    await store.fetchChannels();

    expect(useMixerStore.getState().channels[0].volume_percent).toBe(30);

    await vi.advanceTimersByTimeAsync(100);
    expect(call).toHaveBeenCalledWith("set_channel_volume", {
      sinkName: "sink_chat",
      volume: 30,
    });
    // And once the write has landed the backend is authoritative again.
    call.mockImplementation((cmd: string) =>
      cmd === "get_virtual_devices"
        ? Promise.resolve([channel("sink_chat", 55)])
        : Promise.resolve(undefined),
    );
    await useMixerStore.getState().fetchChannels();
    expect(useMixerStore.getState().channels[0].volume_percent).toBe(55);
  });

  it("a debounced mic write does not undo a mute that happened while it waited", async () => {
    useMixerStore.setState({ micConfig: mic() });
    const store = useMixerStore.getState();

    await store.setMicConfig({ gain_percent: 130 });

    // The mic-mute hotkey fires; the backend mutes and the store refetches.
    call.mockImplementation((cmd: string) => {
      if (cmd === "get_mic_config") return Promise.resolve(mic({ gain_percent: 130, muted: true }));
      if (cmd === "get_input_devices") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    await store.fetchMic();
    expect(useMixerStore.getState().micConfig?.muted).toBe(true);

    await vi.advanceTimersByTimeAsync(100);

    const write = call.mock.calls.find((c) => c[0] === "set_mic_config");
    expect(write).toBeDefined();
    const sent = (write?.[1] as { config: MicConfig }).config;
    // The whole struct goes out, so it has to carry both changes.
    expect(sent.gain_percent).toBe(130);
    expect(sent.muted).toBe(true);
    expect(useMixerStore.getState().micConfig?.muted).toBe(true);
  });

  it("a media poll already in flight does not undo an optimistic pause or seek", async () => {
    const playing: MediaStatus = {
      ...IDLE,
      player: "spotify",
      title: "Track",
      playing: true,
      position_us: 30_000_000,
      length_us: 200_000_000,
    };
    useMedia.setState({ status: playing, statusAt: Date.now(), loaded: true });

    let release: (v: unknown) => void = () => {};
    const slow = new Promise((r) => (release = r));
    call.mockImplementation((cmd: string) =>
      cmd === "media_status" ? slow.then(() => playing) : Promise.resolve(undefined),
    );
    const inFlight = useMedia.getState().poll();

    await useMedia.getState().playPause();
    expect(useMedia.getState().status.playing).toBe(false);

    release(undefined);
    await inFlight;

    expect(useMedia.getState().status.playing).toBe(false);
  });
});
