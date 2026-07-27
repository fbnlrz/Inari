import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";

// The store talks to the Rust backend through src/lib/ipc; mock that boundary.
const call = vi.fn();
vi.mock("../../lib/ipc", () => ({
  call: (...args: unknown[]) => call(...args),
  subscribe: () => Promise.resolve(() => {}),
}));

// One definition of "am I in the desktop window", flipped per test. A getter
// rather than a value, because the components read it at render time and both
// shells have to be exercised from the same module graph.
let desktop = true;
vi.mock("../../lib/platform", () => ({
  get isTauri() {
    return desktop;
  },
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: () => Promise.resolve("1.0.9"),
}));

import { SettingsScreen } from "./SettingsScreen";
import { useRemote } from "../../store/remote";
import { useHotkeys } from "../../store/hotkeys";

/**
 * Answers every command the screen reads on mount. Matched by name, so a
 * command that should no longer be sent from the browser shows up as an
 * "unexpected command" rejection rather than as a quietly passing test.
 */
const backend = () => {
  call.mockImplementation((cmd: string) => {
    switch (cmd) {
      case "get_backend_info":
        return Promise.resolve({ native: true, engine_alive: true });
      case "get_autostart":
        return Promise.resolve(true);
      case "get_default_devices":
        return Promise.resolve({ output: null, input: null });
      case "get_prefs":
        return Promise.resolve({ device_label_style: "plain", start_minimized: false });
      case "get_hotkeys":
        return Promise.resolve([]);
      case "get_remote_status":
        return Promise.resolve({
          enabled: false,
          address: "127.0.0.1",
          port: 7433,
          url: "http://127.0.0.1:7433",
          clients: 0,
          interfaces: [],
        });
      default:
        return Promise.reject(new Error(`unexpected command ${cmd}`));
    }
  });
};

const remoteState = useRemote.getState();
const hotkeyState = useHotkeys.getState();

/** Renders and lets the mount fetches settle, so assertions see real state. */
const mount = async () => {
  const view = render(<SettingsScreen />);
  await act(async () => {});
  return view;
};

/** The commands the allowlist keeps off the remote, and their rows. */
const DESKTOP_ONLY = [
  "Device naming",
  "Default output",
  "Default input",
  "Start at login",
  "Hotkeys",
  "Remote",
  "Updates",
  "Logs",
  "Reset Inari",
];

beforeEach(() => {
  call.mockReset();
  backend();
  useRemote.setState(remoteState, true);
  useHotkeys.setState(hotkeyState, true);
});

afterEach(() => {
  desktop = true;
});

describe("SettingsScreen in the desktop window", () => {
  it("offers the settings that belong to this machine", async () => {
    await mount();
    for (const row of DESKTOP_ONLY) {
      expect(screen.getByText(row)).toBeInTheDocument();
    }
  });
});

describe("SettingsScreen over the remote", () => {
  beforeEach(() => {
    desktop = false;
  });

  it("omits every section whose command the remote cannot send", async () => {
    await mount();
    for (const row of DESKTOP_ONLY) {
      expect(screen.queryByText(row)).not.toBeInTheDocument();
    }
  });

  it("keeps the settings a tablet is allowed to change", async () => {
    await mount();
    // Absence above only means something if the screen actually rendered.
    expect(screen.getByText("Theme")).toBeInTheDocument();
    expect(screen.getByText("Balance slider")).toBeInTheDocument();
    expect(screen.getByText("Audio engine")).toBeInTheDocument();
  });

  it("does not even ask for the state behind the hidden rows", async () => {
    await mount();
    const asked = call.mock.calls.map(([cmd]) => cmd as string);
    for (const cmd of ["get_autostart", "get_default_devices", "get_hotkeys", "get_remote_status"]) {
      expect(asked).not.toContain(cmd);
    }
  });
});
