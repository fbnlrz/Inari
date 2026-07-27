import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";

// The stores talk to the Rust backend through src/lib/ipc; mock that boundary.
const call = vi.fn();
vi.mock("./lib/ipc", () => ({
  call: (...args: unknown[]) => call(...args),
  subscribe: () => Promise.resolve(() => {}),
}));

// This file is the browser half: the rail is built at module scope, so the
// answer has to be settled before App is imported.
vi.mock("./lib/platform", () => ({ isTauri: false }));

import App from "./App";

/** Answers the reads the mixer performs on mount with an empty-but-valid board. */
const backend = () => {
  call.mockImplementation((cmd: string) => {
    switch (cmd) {
      case "get_backend_info":
        return Promise.resolve({ native: true, engine_alive: true });
      case "get_prefs":
        return Promise.resolve({
          onboarded: true,
          balance_a: null,
          balance_b: null,
          show_balance: false,
        });
      case "get_active_profile":
        return Promise.resolve(null);
      case "get_virtual_devices":
      case "get_app_streams":
      case "get_output_devices":
      case "get_input_devices":
      case "get_seen_apps":
      case "list_buses":
      case "list_profiles":
        return Promise.resolve([]);
      default:
        return Promise.resolve(null);
    }
  });
};

const mount = async () => {
  const view = render(<App />);
  await act(async () => {});
  return view;
};

beforeEach(() => {
  call.mockReset();
  backend();
});

describe("the nav rail over the remote", () => {
  it("has no Mouse entry - every mouse command is off the allowlist", async () => {
    await mount();

    expect(screen.queryByRole("button", { name: "Mouse" })).not.toBeInTheDocument();
  });

  it("still offers the screens a tablet can drive", async () => {
    await mount();

    for (const label of ["Mixer", "Apps", "Mic", "Headset", "OLED", "Settings"]) {
      expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
    }
  });

  it("never asks the backend about a mouse it cannot reach", async () => {
    await mount();

    expect(call.mock.calls.map(([cmd]) => cmd as string)).not.toContain("get_mouse_status");
  });
});
