import { beforeEach, describe, expect, it, vi } from "vitest";

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

const channel = (name: string): VirtualSink => ({
  name,
  label: name.replace("sink_", ""),
  icon: null,
  volume_percent: 100,
  muted: false,
  stream_mix: true,
});

const initialState = useMixerStore.getState();

/**
 * Minimal backend fake: remove_channel really shrinks the channel list, so
 * the re-fetches after each removal return what the loop should be leaving
 * behind rather than resurrecting the seeded layout.
 */
let backendChannels: VirtualSink[] = [];
const fakeBackend = async (cmd: string, args: Record<string, unknown> = {}) => {
  switch (cmd) {
    case "remove_channel":
      backendChannels = backendChannels.filter((c) => c.name !== args.sinkName);
      return undefined;
    case "get_virtual_devices":
      return backendChannels;
    case "get_app_streams":
    case "get_output_devices":
    case "list_buses":
      return [];
    case "get_channel_outputs":
    case "get_resolved_outputs":
    case "get_channel_failover":
    case "get_channel_eq_configs":
      return {};
    default:
      return undefined;
  }
};

const seed = (names: string[]) => {
  backendChannels = names.map(channel);
  useMixerStore.setState({ channels: backendChannels.slice(), showOnboarding: true });
};

/** Commands actually sent, in order. */
const commands = () => call.mock.calls.map((c) => c[0] as string);

beforeEach(() => {
  call.mockReset();
  call.mockImplementation(fakeBackend);
  useMixerStore.setState(initialState, true);
});

describe("finishOnboarding(blank)", () => {
  it("collapses the seeded layout to a single Main channel", async () => {
    seed(["sink_game", "sink_chat", "sink_media", "sink_aux"]);

    await useMixerStore.getState().finishOnboarding(true);

    // Destructive by design: everything but the first strip is deleted.
    expect(commands().filter((c) => c === "remove_channel")).toHaveLength(3);
    expect(
      call.mock.calls
        .filter((c) => c[0] === "remove_channel")
        .map((c) => (c[1] as { sinkName: string }).sinkName),
    ).toEqual(["sink_chat", "sink_media", "sink_aux"]);
    expect(backendChannels.map((c) => c.name)).toEqual(["sink_game"]);
  });

  it("renames and re-icons the survivor", async () => {
    seed(["sink_game", "sink_chat"]);

    await useMixerStore.getState().finishOnboarding(true);

    expect(call).toHaveBeenCalledWith("rename_channel", {
      sinkName: "sink_game",
      label: "Main",
    });
    expect(call).toHaveBeenCalledWith("set_channel_icon", {
      sinkName: "sink_game",
      icon: "graphic_eq",
    });
  });

  it("marks the app onboarded before touching any channel", async () => {
    seed(["sink_game", "sink_chat"]);

    await useMixerStore.getState().finishOnboarding(true);

    expect(commands()[0]).toBe("set_onboarded");
    expect(useMixerStore.getState().showOnboarding).toBe(false);
  });

  it("keeps a single-channel layout and only relabels it", async () => {
    seed(["sink_game"]);

    await useMixerStore.getState().finishOnboarding(true);

    expect(commands()).not.toContain("remove_channel");
    expect(call).toHaveBeenCalledWith("rename_channel", {
      sinkName: "sink_game",
      label: "Main",
    });
  });

  it("does not rename anything when there are no channels", async () => {
    seed([]);

    await useMixerStore.getState().finishOnboarding(true);

    expect(commands()).toEqual(["set_onboarded"]);
  });

  it("keeps the seeded channels when the user declines the blank start", async () => {
    seed(["sink_game", "sink_chat", "sink_media"]);

    await useMixerStore.getState().finishOnboarding(false);

    expect(commands()).toEqual(["set_onboarded"]);
    expect(backendChannels).toHaveLength(3);
  });

  it("deletes nothing in replay mode", async () => {
    seed(["sink_game", "sink_chat"]);
    useMixerStore.setState({ onboardingReplay: true });

    // Replaying the tour is view-only; a stray blank=true must not wipe the
    // user's live layout.
    await useMixerStore.getState().finishOnboarding(true);

    expect(call).not.toHaveBeenCalled();
    expect(useMixerStore.getState().showOnboarding).toBe(false);
    expect(useMixerStore.getState().onboardingReplay).toBe(false);
    expect(backendChannels).toHaveLength(2);
  });

  it("closes the modal and reports the failure when the backend refuses", async () => {
    seed(["sink_game", "sink_chat"]);
    call.mockImplementation(async (cmd: string, args: Record<string, unknown>) => {
      if (cmd === "set_onboarded") throw "prefs are read-only";
      return fakeBackend(cmd, args);
    });

    await useMixerStore.getState().finishOnboarding(true);

    expect(useMixerStore.getState().error).toContain("read-only");
    expect(useMixerStore.getState().showOnboarding).toBe(false);
    expect(backendChannels).toHaveLength(2); // bailed before the deletions
  });
});
