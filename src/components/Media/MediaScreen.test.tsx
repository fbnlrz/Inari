import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";

// The store talks to the Rust backend through src/lib/ipc; mock that boundary.
const call = vi.fn();
vi.mock("../../lib/ipc", () => ({
  call: (...args: unknown[]) => call(...args),
  subscribe: () => Promise.resolve(() => {}),
}));

import { MediaScreen } from "./MediaScreen";
import { displayPosition, useMedia } from "../../store/media";
import type { MediaStatus } from "../../store/media";

const track = (over: Partial<MediaStatus> = {}): MediaStatus => ({
  player: "spotify",
  title: "Midnight City",
  artist: "M83",
  album: "Hurry Up, We're Dreaming",
  playing: false,
  position_us: 30_000_000,
  length_us: 180_000_000,
  art_generation: 0,
  available: true,
  ...over,
});

/** What the fake backend answers with; tests reassign between polls. */
let status: MediaStatus;
let players: { id: string; label: string }[];
let art: { generation: number; mime: string; base64: string } | null;

const cmds = (name: string) => call.mock.calls.filter(([cmd]) => cmd === name);

/**
 * Let the poll loop run. `ms` of fake time plus a few microtask turns, since
 * one tick is a chain of awaited commands rather than a single promise.
 */
const settle = async (ms = 0) => {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
    for (let i = 0; i < 10; i++) await Promise.resolve();
  });
};

const mount = async () => {
  const view = render(<MediaScreen />);
  await settle();
  return view;
};

/** jsdom measures nothing, so the scrub track is given a width to drag over. */
const sized = (el: HTMLElement, left: number, width: number) => {
  el.getBoundingClientRect = () =>
    ({
      left,
      right: left + width,
      width,
      top: 0,
      bottom: 6,
      height: 6,
      x: left,
      y: 0,
      toJSON: () => ({}),
    }) as DOMRect;
};

const initial = useMedia.getState();

beforeEach(() => {
  vi.useFakeTimers();
  status = track();
  players = [{ id: "spotify", label: "spotify" }];
  art = null;
  call.mockReset();
  call.mockImplementation((cmd: string) => {
    switch (cmd) {
      case "media_players":
        return Promise.resolve(players);
      case "media_status":
        return Promise.resolve(status);
      case "media_art":
        return Promise.resolve(art);
      default:
        return Promise.resolve(null);
    }
  });
  useMedia.setState(initial, true);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("cover art", () => {
  it("fetches a cover once per generation, and again when it changes", async () => {
    status = track({ art_generation: 7 });
    art = { generation: 7, mime: "image/jpeg", base64: "AAAA" };

    const { container } = await mount();

    expect(cmds("media_art")).toHaveLength(1);
    expect(container.querySelector("img.media-art")).toHaveAttribute(
      "src",
      "data:image/jpeg;base64,AAAA",
    );

    // Two more polls with the same cover: a few hundred kilobytes that do not
    // have to travel, so they must not.
    await settle(2000);

    expect(cmds("media_status").length).toBeGreaterThan(1);
    expect(cmds("media_art")).toHaveLength(1);

    status = track({ art_generation: 8 });
    art = { generation: 8, mime: "image/jpeg", base64: "BBBB" };
    await settle(1000);

    expect(cmds("media_art")).toHaveLength(2);
    expect(container.querySelector("img.media-art")).toHaveAttribute(
      "src",
      "data:image/jpeg;base64,BBBB",
    );
  });

  it("never asks for art the player says it has none of", async () => {
    status = track({ art_generation: 0 });

    const { container } = await mount();
    await settle(2000);

    expect(cmds("media_art")).toHaveLength(0);
    expect(container.querySelector("img.media-art")).toBeNull();
    // A placeholder plate, not a broken image.
    expect(container.querySelector(".media-art-empty")).toBeInTheDocument();
  });

  it("stops asking for a cover that could not be read", async () => {
    status = track({ art_generation: 7 });
    art = null; // the backend found nothing decodable behind the art URL

    await mount();
    await settle(3000);

    expect(cmds("media_art")).toHaveLength(1);
  });
});

describe("playerctl missing", () => {
  it("says so instead of looking broken", async () => {
    status = track({ available: false, player: null, title: "" });

    await mount();

    expect(screen.getByText(/isn't installed/)).toBeInTheDocument();
    // Named twice: the headline and the note about the optional dependency.
    expect(screen.getAllByText("playerctl").length).toBeGreaterThan(0);
    expect(screen.getByText(/optional dependency/)).toBeInTheDocument();
    expect(screen.queryByRole("slider")).not.toBeInTheDocument();
  });
});

describe("tracks without a length", () => {
  it("drops the scrub bar and shows the elapsed time alone", async () => {
    status = track({ length_us: 0, position_us: 95_000_000 });

    await mount();

    expect(screen.queryByRole("slider")).not.toBeInTheDocument();
    expect(screen.getByText("1:35")).toBeInTheDocument();
    expect(screen.getByText("Live")).toBeInTheDocument();
  });

  it("keeps the bar when the player does report one", async () => {
    await mount();

    const slider = screen.getByRole("slider");
    expect(slider).toHaveAttribute("aria-valuemax", "180");
    expect(slider).toHaveAttribute("aria-valuenow", "30");
    expect(slider).toHaveAttribute("aria-valuetext", "0:30 of 3:00");
  });
});

describe("scrubbing", () => {
  it("seeks on release, not on every move", async () => {
    await mount();
    const slider = screen.getByRole("slider");
    sized(slider, 0, 200);

    fireEvent.pointerDown(slider, { clientX: 20 });
    fireEvent.pointerMove(window, { clientX: 120 });
    fireEvent.pointerMove(window, { clientX: 150 });

    // The handle follows the finger…
    expect(slider).toHaveAttribute("aria-valuenow", "135");
    // …but the player is left alone until the finger lifts.
    expect(cmds("media_seek")).toHaveLength(0);

    fireEvent.pointerUp(window, { clientX: 150 });

    expect(cmds("media_seek")).toHaveLength(1);
    expect(cmds("media_seek")[0][1]).toMatchObject({ positionUs: 135_000_000 });
  });

  it("ignores pointer movement that did not start on the track", async () => {
    await mount();
    sized(screen.getByRole("slider"), 0, 200);

    fireEvent.pointerMove(window, { clientX: 150 });
    fireEvent.pointerUp(window, { clientX: 150 });

    expect(cmds("media_seek")).toHaveLength(0);
  });

  it("is operable from the keyboard", async () => {
    await mount();

    fireEvent.keyDown(screen.getByRole("slider"), { key: "ArrowRight" });

    expect(cmds("media_seek")[0][1]).toMatchObject({ positionUs: 35_000_000 });
  });
});

describe("the transport", () => {
  it("labels every button for assistive tech", async () => {
    await mount();

    expect(screen.getByRole("button", { name: "Previous track" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Play" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next track" })).toBeInTheDocument();
  });

  it("flips play to pause before the next poll answers", async () => {
    await mount();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Play" }));
    });

    expect(cmds("media_play_pause")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Pause" })).toBeInTheDocument();
  });
});

describe("the player picker", () => {
  it("stays out of the way when there is only one player", async () => {
    await mount();

    expect(screen.queryByRole("button", { name: "Player" })).not.toBeInTheDocument();
  });

  it("targets the chosen player and falls back when it disappears", async () => {
    players = [
      { id: "chromium.instance1", label: "chromium" },
      { id: "spotify", label: "spotify" },
    ];

    await mount();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Player" }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitemradio", { name: /spotify/ }));
    });
    await settle();

    const reads = cmds("media_status");
    expect(reads[reads.length - 1][1]).toMatchObject({ player: "spotify" });

    // It quits mid-session: the picker must not stay pointed at a name
    // playerctl will refuse.
    players = [{ id: "chromium.instance1", label: "chromium" }];
    await settle(6000);

    expect(useMedia.getState().selected).toBeNull();
  });
});

describe("displayPosition", () => {
  it("advances a playing track by the wall clock", () => {
    const s = track({ playing: true, position_us: 30_000_000 });

    expect(displayPosition(s, 1_000, 1_000)).toBe(30_000_000);
    expect(displayPosition(s, 1_000, 1_500)).toBe(30_500_000);
  });

  it("holds a paused track still", () => {
    const s = track({ playing: false, position_us: 30_000_000 });

    expect(displayPosition(s, 1_000, 9_000)).toBe(30_000_000);
  });

  it("never runs past a known length, and is unbounded without one", () => {
    expect(displayPosition(track({ playing: true }), 0, 10_000_000)).toBe(180_000_000);
    expect(displayPosition(track({ playing: true, length_us: 0 }), 0, 5_000)).toBe(35_000_000);
  });
});
