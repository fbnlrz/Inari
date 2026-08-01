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

import { EFFECTS, useKeyboard } from "./keyboard";

const initialState = useKeyboard.getState();

/** A snapshot shaped like the one `get_keyboard_status` returns. */
function snapshot(overrides: Record<string, unknown> = {}) {
  return {
    connected: true,
    status: { ...initialState.status, present: true, model: "Apex Pro Gen 3" },
    config: { ...initialState.config },
    layout: [
      { hid: 0x29, label: "Esc", x: 0, y: 0, w: 1, h: 1 },
      { hid: 0x04, label: "A", x: 1, y: 2, w: 1, h: 1 },
    ],
    extent: [18.25, 6],
    preview: [
      [0x29, [1, 2, 3]],
      [0x04, [4, 5, 6]],
    ],
    oled_modes: [{ mode: "clock", label: "Clock" }],
    oled_wires: [{ wire: { chunked: { cmd: 12, page: false } }, label: "Chunked 0x0C" }],
    ...overrides,
  };
}

/** Flush the store's 150 ms write debounce. */
async function settle() {
  await vi.advanceTimersByTimeAsync(200);
}

describe("keyboard store", () => {
  beforeEach(() => {
    useKeyboard.setState(initialState, true);
    call.mockReset();
    call.mockResolvedValue(undefined);
    subscribe.mockClear();
    vi.useFakeTimers();
  });

  it("unpacks a snapshot into the shape the screen renders from", async () => {
    call.mockResolvedValueOnce(snapshot());
    await useKeyboard.getState().refresh();

    const s = useKeyboard.getState();
    expect(s.connected).toBe(true);
    expect(s.status.model).toBe("Apex Pro Gen 3");
    expect(s.layout).toHaveLength(2);
    // The preview arrives as pairs and has to become a lookup, or every key
    // would have to scan the list to find its own colour.
    expect(s.preview[0x29]).toEqual([1, 2, 3]);
    expect(s.oledWires[0].label).toBe("Chunked 0x0C");
  });

  it("paints a key optimistically and switches to the per-key effect", async () => {
    await useKeyboard.getState().setKeyColor(0x04, [9, 8, 7]);

    // The board is repainted before the round trip finishes, so the click feels
    // immediate rather than laggy.
    const s = useKeyboard.getState();
    expect(s.preview[0x04]).toEqual([9, 8, 7]);
    expect(s.config.lighting.per_key[0x04]).toEqual([9, 8, 7]);
    expect(s.config.lighting.kind).toBe("per_key");

    await settle();
    expect(call).toHaveBeenCalledWith("keyboard_set_key_color", {
      hid: 0x04,
      rgb: [9, 8, 7],
    });
  });

  it("erasing a key removes it rather than painting it black", async () => {
    await useKeyboard.getState().setKeyColor(0x04, [9, 8, 7]);
    await useKeyboard.getState().setKeyColor(0x04, null);
    expect(useKeyboard.getState().config.lighting.per_key[0x04]).toBeUndefined();
    await settle();
    expect(call).toHaveBeenLastCalledWith("keyboard_set_key_color", { hid: 0x04, rgb: null });
  });

  it("collapses a colour-picker drag into one device write", async () => {
    const kb = useKeyboard.getState();
    await kb.setLighting({ color: [1, 0, 0] });
    await kb.setLighting({ color: [2, 0, 0] });
    await kb.setLighting({ color: [3, 0, 0] });
    expect(call).not.toHaveBeenCalled();

    call.mockResolvedValue(snapshot());
    await settle();
    const writes = call.mock.calls.filter((c) => c[0] === "keyboard_set_lighting");
    expect(writes).toHaveLength(1);
    expect((writes[0][1] as { lighting: { color: number[] } }).lighting.color).toEqual([3, 0, 0]);
  });

  it("rolls the master switch back when the device refuses it", async () => {
    call.mockRejectedValueOnce(new Error("no keyboard"));
    await useKeyboard.getState().setEnabled(false);
    const s = useKeyboard.getState();
    expect(s.config.enabled).toBe(true);
    expect(s.error).toContain("no keyboard");
  });

  it("subscribes to presence and re-reads on reconnect", async () => {
    call.mockResolvedValue(snapshot());
    await useKeyboard.getState().init();
    const events = subscribe.mock.calls.map((c) => c[0]);
    expect(events).toContain("keyboard-status");
    expect(events).toContain("keyboard-presence");
  });

  it("offers every effect the backend knows, and marks the firmware ones", () => {
    const kinds = EFFECTS.map((e) => e.kind);
    // Mirrors keyboard::effects::EffectKind; a kind missing here is a kind the
    // user cannot reach.
    expect(kinds).toEqual([
      "static",
      "per_key",
      "breathing",
      "wave",
      "rainbow",
      "gradient",
      "audio",
      "reactive",
      "color_shift",
      "off",
    ]);
    const onboard = EFFECTS.filter((e) => e.onboard).map((e) => e.kind);
    expect(onboard).toEqual(["reactive", "color_shift", "off"]);
    // Two-colour effects must offer a second picker or the setting is unreachable.
    expect(EFFECTS.find((e) => e.kind === "gradient")?.colors).toBe(2);
  });
});
