import { beforeEach, describe, expect, it, vi } from "vitest";

// The store talks to the Rust backend through Tauri IPC; mock the boundary.
const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

import { useMixerStore } from "./mixer";
import type { BusDef, VirtualSink } from "../types";
import { busMembers } from "../types";

const channel = (name: string): VirtualSink => ({
  name,
  label: name.replace("sink_", ""),
  icon: null,
  volume_percent: 100,
  muted: false,
  stream_mix: true,
});

const bus = (patch: Partial<BusDef> = {}): BusDef => ({
  name: "sink_bus1",
  label: "Bus 1",
  channels: [],
  exclude: false,
  volume_percent: 100,
  muted: false,
  ...patch,
});

const ALL = ["sink_game", "sink_chat", "sink_media", "sink_aux"];

const initialState = useMixerStore.getState();

/** The bus as the store holds it right now. */
const stored = () => useMixerStore.getState().buses[0];

beforeEach(() => {
  invoke.mockReset();
  // list_buses would otherwise resolve undefined and wipe the state the
  // assertions are about; echo the store back so the re-sync is a no-op.
  invoke.mockImplementation(async (cmd: string) =>
    cmd === "list_buses" ? useMixerStore.getState().buses : undefined,
  );
  useMixerStore.setState(initialState, true);
  useMixerStore.setState({ channels: ALL.map(channel) });
});

describe("setBusMembers", () => {
  it("stores the carried set verbatim in manual mode", async () => {
    useMixerStore.setState({ buses: [bus({ channels: ["sink_game"] })] });

    await useMixerStore.getState().setBusMembers("sink_bus1", ["sink_game", "sink_chat"]);

    expect(stored().channels).toEqual(["sink_game", "sink_chat"]);
    expect(busMembers(stored(), ALL)).toEqual(["sink_game", "sink_chat"]);
  });

  it("stores the complement in auto-include mode", async () => {
    useMixerStore.setState({ buses: [bus({ exclude: true, channels: [] })] });

    // Caller always passes the carried set, whatever the mode.
    await useMixerStore.getState().setBusMembers("sink_bus1", ["sink_game", "sink_chat"]);

    expect(stored().channels).toEqual(["sink_media", "sink_aux"]);
    expect(busMembers(stored(), ALL)).toEqual(["sink_game", "sink_chat"]);
  });

  it("sends the carried set to the backend, never the stored complement", async () => {
    useMixerStore.setState({ buses: [bus({ exclude: true, channels: [] })] });

    await useMixerStore.getState().setBusMembers("sink_bus1", ["sink_game"]);

    expect(invoke).toHaveBeenCalledWith("set_bus_members", {
      name: "sink_bus1",
      channels: ["sink_game"],
    });
  });

  it("empties an auto-include mix by excluding everything", async () => {
    useMixerStore.setState({ buses: [bus({ exclude: true, channels: ["sink_aux"] })] });

    await useMixerStore.getState().setBusMembers("sink_bus1", []);

    expect(stored().channels).toEqual(ALL);
    expect(busMembers(stored(), ALL)).toEqual([]);
  });

  it("leaves other mixes alone", async () => {
    const other = bus({ name: "sink_bus2", channels: ["sink_aux"] });
    useMixerStore.setState({ buses: [bus({ channels: [] }), other] });

    await useMixerStore.getState().setBusMembers("sink_bus1", ["sink_game"]);

    expect(useMixerStore.getState().buses[1]).toEqual(other);
  });

  it("re-syncs from the backend so the stored complement can't drift", async () => {
    useMixerStore.setState({ buses: [bus({ exclude: true })] });

    await useMixerStore.getState().setBusMembers("sink_bus1", ["sink_game"]);

    expect(invoke).toHaveBeenCalledWith("list_buses");
  });

  it("re-syncs from the backend when the write fails", async () => {
    useMixerStore.setState({ buses: [bus()] });
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "set_bus_members") throw "no such mix";
      return cmd === "list_buses" ? [bus({ channels: ["sink_game"] })] : undefined;
    });

    await useMixerStore.getState().setBusMembers("sink_bus1", ["sink_chat"]);

    expect(useMixerStore.getState().error).toContain("no such mix");
    expect(stored().channels).toEqual(["sink_game"]); // backend truth wins
  });
});

describe("setBusExclude", () => {
  it("keeps the carried set when switching manual -> auto-include", async () => {
    useMixerStore.setState({
      buses: [bus({ exclude: false, channels: ["sink_game", "sink_chat"] })],
    });

    await useMixerStore.getState().setBusExclude("sink_bus1", true);

    expect(stored().exclude).toBe(true);
    expect(stored().channels).toEqual(["sink_media", "sink_aux"]); // now the excluded set
    expect(busMembers(stored(), ALL)).toEqual(["sink_game", "sink_chat"]);
  });

  it("keeps the carried set when switching auto-include -> manual", async () => {
    useMixerStore.setState({
      buses: [bus({ exclude: true, channels: ["sink_media", "sink_aux"] })],
    });

    await useMixerStore.getState().setBusExclude("sink_bus1", false);

    expect(stored().exclude).toBe(false);
    expect(stored().channels).toEqual(["sink_game", "sink_chat"]);
    expect(busMembers(stored(), ALL)).toEqual(["sink_game", "sink_chat"]);
  });

  it("round-trips membership through both flips", async () => {
    useMixerStore.setState({ buses: [bus({ channels: ["sink_chat"] })] });
    const store = useMixerStore.getState();

    await store.setBusExclude("sink_bus1", true);
    await store.setBusExclude("sink_bus1", false);

    expect(stored().channels).toEqual(["sink_chat"]);
  });

  it("is a no-op on the stored set when the mode is unchanged", async () => {
    useMixerStore.setState({ buses: [bus({ exclude: true, channels: ["sink_aux"] })] });

    await useMixerStore.getState().setBusExclude("sink_bus1", true);

    // Converting again would turn the excluded set into its own complement.
    expect(stored().channels).toEqual(["sink_aux"]);
    expect(busMembers(stored(), ALL)).toEqual(["sink_game", "sink_chat", "sink_media"]);
  });

  it("hands an auto-include mix every new channel for free", async () => {
    useMixerStore.setState({ buses: [bus({ channels: ["sink_game"] })] });

    await useMixerStore.getState().setBusExclude("sink_bus1", true);

    // A channel added after the flip is not in the excluded set, so it joins.
    expect(busMembers(stored(), [...ALL, "sink_new"])).toEqual(["sink_game", "sink_new"]);
  });

  it("re-syncs from the backend when the write fails", async () => {
    useMixerStore.setState({ buses: [bus({ channels: ["sink_game"] })] });
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "set_bus_exclude") throw "backend down";
      return cmd === "list_buses" ? [bus({ channels: ["sink_game"] })] : undefined;
    });

    await useMixerStore.getState().setBusExclude("sink_bus1", true);

    expect(useMixerStore.getState().error).toContain("backend down");
    expect(stored().exclude).toBe(false);
  });
});
