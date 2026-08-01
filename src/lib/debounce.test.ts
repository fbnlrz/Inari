import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { createDebouncer } from "./debounce";

describe("createDebouncer", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("fires once per key with the last call of a burst", async () => {
    const calls: string[] = [];
    const debounce = createDebouncer(150, () => {});
    debounce("a", async () => void calls.push("a1"));
    debounce("a", async () => void calls.push("a2"));
    debounce("b", async () => void calls.push("b1"));

    await vi.advanceTimersByTimeAsync(150);
    expect(calls).toEqual(["a2", "b1"]);
  });

  it("cancel drops pending calls by prefix and leaves the rest alone", async () => {
    const calls: string[] = [];
    const debounce = createDebouncer(150, () => {});
    debounce("key:4", async () => void calls.push("key:4"));
    debounce("key:5", async () => void calls.push("key:5"));
    debounce("brightness", async () => void calls.push("brightness"));

    debounce.cancel("key:");
    await vi.advanceTimersByTimeAsync(150);

    // The point of the prefix: a bulk "clear" supersedes the per-key writes it
    // replaces, and only those. A pending key colour landing afterwards would
    // repaint that key on the device with nothing left to tell the UI.
    expect(calls).toEqual(["brightness"]);
  });

  it("cancel does not block later calls with the same key", async () => {
    const calls: string[] = [];
    const debounce = createDebouncer(150, () => {});
    debounce("key:4", async () => void calls.push("stale"));
    debounce.cancel("key:");
    debounce("key:4", async () => void calls.push("fresh"));

    await vi.advanceTimersByTimeAsync(150);
    expect(calls).toEqual(["fresh"]);
  });
});
