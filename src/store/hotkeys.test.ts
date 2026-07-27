import { beforeEach, describe, expect, it, vi } from "vitest";

// The store talks to the Rust backend through Tauri IPC; mock the boundary.
const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

import {
  formatAccelerator,
  toAccelerator,
  useHotkeys,
  type HotkeyBinding,
} from "./hotkeys";

const press = (code: string, mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

const binding = (over: Partial<HotkeyBinding> = {}): HotkeyBinding => ({
  action: "mic_mute",
  accelerator: null,
  channel: null,
  registered: false,
  error: null,
  ...over,
});

const initialState = useHotkeys.getState();

beforeEach(() => {
  invoke.mockReset();
  useHotkeys.setState(initialState, true);
});

describe("toAccelerator", () => {
  it("builds a backend-parsable accelerator from a captured press", () => {
    expect(toAccelerator(press("KeyM", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+KeyM");
    expect(toAccelerator(press("Digit1", { metaKey: true }))).toBe("Super+Digit1");
    // Modifier order is fixed, not the order they were pressed in, so the
    // same combination never persists under two different strings.
    expect(toAccelerator(press("KeyG", { shiftKey: true, ctrlKey: true, altKey: true }))).toBe(
      "Ctrl+Alt+Shift+KeyG",
    );
  });

  it("keeps listening while only modifiers are held", () => {
    expect(toAccelerator(press("ControlLeft", { ctrlKey: true }))).toBeNull();
    expect(toAccelerator(press("ShiftRight", { shiftKey: true }))).toBeNull();
    expect(toAccelerator(press("MetaLeft", { metaKey: true }))).toBeNull();
  });

  it("refuses a bare letter but allows a bare function key", () => {
    // A system-wide grab on "M" would eat the letter in every other app.
    expect(toAccelerator(press("KeyM"))).toBeNull();
    expect(toAccelerator(press("Space"))).toBeNull();
    expect(toAccelerator(press("F9"))).toBe("F9");
    expect(toAccelerator(press("PrintScreen"))).toBe("PrintScreen");
  });
});

describe("formatAccelerator", () => {
  it("shows the key the way it is printed on the keyboard", () => {
    expect(formatAccelerator("Ctrl+Shift+KeyM")).toBe("Ctrl + Shift + M");
    expect(formatAccelerator("Super+Digit1")).toBe("Super + 1");
    expect(formatAccelerator("Alt+ArrowUp")).toBe("Alt + Up");
    expect(formatAccelerator("F9")).toBe("F9");
  });
});

describe("useHotkeys", () => {
  it("takes the refreshed list straight from the set call", async () => {
    const bound = [binding({ accelerator: "Ctrl+Shift+KeyM", registered: true })];
    invoke.mockResolvedValue(bound);

    await useHotkeys.getState().setBinding("mic_mute", "Ctrl+Shift+KeyM");

    expect(invoke).toHaveBeenCalledWith("set_hotkey", {
      action: "mic_mute",
      accelerator: "Ctrl+Shift+KeyM",
    });
    expect(useHotkeys.getState().bindings).toEqual(bound);
    expect(useHotkeys.getState().error).toBeNull();
  });

  it("keeps a binding the session refused to grab, marked dead", async () => {
    // The Wayland case: still saved, but the UI must not claim it works.
    invoke.mockResolvedValue([
      binding({ accelerator: "Ctrl+Shift+KeyM", registered: false, error: "HotKey already registered" }),
    ]);

    await useHotkeys.getState().fetch();

    const [row] = useHotkeys.getState().bindings;
    expect(row.accelerator).toBe("Ctrl+Shift+KeyM");
    expect(row.registered).toBe(false);
    expect(row.error).toBe("HotKey already registered");
  });

  it("clears a binding by sending null", async () => {
    invoke.mockResolvedValue([binding()]);
    await useHotkeys.getState().setBinding("toggle_window", null);
    expect(invoke).toHaveBeenCalledWith("set_hotkey", { action: "toggle_window", accelerator: null });
  });

  it("surfaces a rejected accelerator instead of pretending it stuck", async () => {
    useHotkeys.setState({ bindings: [binding()] });
    invoke.mockRejectedValue("Ctrl+Nope is not a valid shortcut");

    await useHotkeys.getState().setBinding("mic_mute", "Ctrl+Nope");

    expect(useHotkeys.getState().error).toContain("not a valid shortcut");
    expect(useHotkeys.getState().bindings).toEqual([binding()]);
  });

  it("sends the channel target under the name the command expects", async () => {
    invoke.mockResolvedValue([binding({ action: "channel_mute", channel: "sink_chat" })]);
    await useHotkeys.getState().setChannel("sink_chat");
    expect(invoke).toHaveBeenCalledWith("set_hotkey_channel", { sinkName: "sink_chat" });
    expect(useHotkeys.getState().bindings[0].channel).toBe("sink_chat");
  });
});
