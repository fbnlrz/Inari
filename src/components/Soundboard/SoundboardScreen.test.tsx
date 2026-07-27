import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";

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

// The native file dialog has no browser equivalent; it is only ever reached
// from the desktop-gated button.
const openDialog = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openDialog(...args),
}));

import { SoundboardScreen } from "./SoundboardScreen";
import { IDLE_STATUS, useSoundboard } from "../../store/soundboard";
import type { ClipInfo, SoundboardStatus } from "../../store/soundboard";

const clip = (over: Partial<ClipInfo> = {}): ClipInfo => ({
  id: "a1",
  name: "Airhorn",
  volume_percent: 100,
  format: "native",
  missing: false,
  playable: true,
  ...over,
});

/** What the fake backend answers with; tests reassign between polls. */
let clips: ClipInfo[];
let status: SoundboardStatus;

const cmds = (name: string) => call.mock.calls.filter(([cmd]) => cmd === name);
const sent = () => call.mock.calls.map(([cmd]) => cmd as string);

/** Let the poll loop run: fake time plus a few microtask turns. */
const settle = async (ms = 0) => {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
    for (let i = 0; i < 10; i++) await Promise.resolve();
  });
};

const mount = async () => {
  const view = render(<SoundboardScreen />);
  await settle();
  return view;
};

const press = async (name: string) => {
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name }));
  });
  await settle();
};

const initial = useSoundboard.getState();

beforeEach(() => {
  vi.useFakeTimers();
  clips = [clip()];
  status = { ...IDLE_STATUS };
  call.mockReset();
  call.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
    switch (cmd) {
      case "soundboard_clips":
        return Promise.resolve(clips);
      case "soundboard_status":
        return Promise.resolve(status);
      case "soundboard_formats":
        return Promise.resolve(["wav", "flac", "mp3"]);
      case "soundboard_set_duck": {
        // The real command stores the setting, so every later status read
        // reports it - a fake that forgot would make the poll fight the UI.
        const duck = {
          enabled: Boolean(args?.enabled),
          attenuation_db: Number(args?.attenuationDb),
        };
        status = { ...status, duck };
        return Promise.resolve(duck);
      }
      default:
        return Promise.resolve(null);
    }
  });
  openDialog.mockReset();
  useSoundboard.setState(initial, true);
});

afterEach(() => {
  vi.useRealTimers();
  desktop = true;
});

describe("curating the library", () => {
  /** The controls whose commands the remote denies (allowlist.rs). */
  const CURATION = ["Add clip…", "Rename Airhorn", "Remove Airhorn"];

  it("is offered in the desktop window", async () => {
    await mount();

    for (const name of CURATION) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    expect(screen.getByRole("slider", { name: "Volume for Airhorn" })).toBeInTheDocument();
  });

  it("is absent over the remote - not offered and disabled", async () => {
    desktop = false;

    await mount();

    for (const name of CURATION) {
      expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
    }
    expect(screen.queryByRole("slider", { name: "Volume for Airhorn" })).not.toBeInTheDocument();
    // Absence only means something if the board itself rendered.
    expect(screen.getByRole("button", { name: "Play Airhorn" })).toBeInTheDocument();
  });

  it("never sends a command the remote would reject", async () => {
    desktop = false;

    await mount();
    await settle(3000);

    for (const cmd of [
      "soundboard_add_clip",
      "soundboard_formats",
      "soundboard_remove_clip",
      "soundboard_rename_clip",
      "soundboard_set_clip_volume",
    ]) {
      expect(sent()).not.toContain(cmd);
    }
  });

  it("renames a clip in place", async () => {
    await mount();
    await press("Rename Airhorn");

    const input = screen.getByRole("textbox", { name: "New name for Airhorn" });
    await act(async () => {
      fireEvent.change(input, { target: { value: "Foghorn" } });
      fireEvent.keyDown(input, { key: "Enter" });
    });

    expect(cmds("soundboard_rename_clip")[0][1]).toMatchObject({ id: "a1", name: "Foghorn" });
  });

  it("picks a file through the dialog, filtered by what the decoder accepts", async () => {
    openDialog.mockResolvedValue("/home/u/horn.wav");

    await mount();
    await press("Add clip…");

    expect(openDialog).toHaveBeenCalledWith(
      expect.objectContaining({
        filters: [{ name: "Audio", extensions: ["wav", "flac", "mp3"] }],
      }),
    );
    expect(cmds("soundboard_add_clip")[0][1]).toMatchObject({ path: "/home/u/horn.wav" });
  });
});

describe("firing a clip", () => {
  it("sends the toggle command with the chosen target", async () => {
    await mount();
    await press("Chat only");
    await press("Play Airhorn");

    expect(cmds("soundboard_toggle")).toHaveLength(1);
    expect(cmds("soundboard_toggle")[0][1]).toMatchObject({ id: "a1", targets: "chat" });
  });

  it("defaults to playing it to the chat and to yourself", async () => {
    await mount();
    await press("Play Airhorn");

    expect(cmds("soundboard_toggle")[0][1]).toMatchObject({ targets: "both" });
  });

  it("marks the running clip and stops it with the same pad", async () => {
    status = { ...IDLE_STATUS, playing: 1, playing_ids: ["a1"] };

    await mount();

    // Which pad is on has to be visible: at most one clip plays, so the next
    // press either starts something or stops this.
    const pad = screen.getByRole("button", { name: "Stop Airhorn" });
    expect(pad).toHaveAttribute("aria-pressed", "true");
    expect(pad).toHaveClass("on");

    await press("Stop Airhorn");

    // One command, with toggle semantics - not a stop followed by a play,
    // which over the remote's socket could arrive in either order.
    expect(cmds("soundboard_toggle")).toHaveLength(1);
    expect(cmds("soundboard_toggle")[0][1]).toMatchObject({ id: "a1" });
    expect(sent()).not.toContain("soundboard_stop_all");
  });

  it("keeps a panic button for whatever is running", async () => {
    status = { ...IDLE_STATUS, playing: 1, playing_ids: ["a1"] };

    await mount();
    await press("Stop all clips");

    expect(cmds("soundboard_stop_all")).toHaveLength(1);
  });

  it("says so, and offers nothing to press, when nothing is running", async () => {
    await mount();

    expect(screen.getByRole("status")).toHaveTextContent("Nothing playing");
    expect(screen.getByRole("button", { name: "Stop all clips" })).toBeDisabled();
  });
});

describe("honest states", () => {
  it("explains the pactl fallback instead of showing a dead board", async () => {
    status = { ...IDLE_STATUS, available: false };

    await mount();

    expect(screen.getByText(/needs the native audio engine/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Play Airhorn" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Stop all clips" })).not.toBeInTheDocument();
  });

  it("says ffmpeg is missing once, not on every clip", async () => {
    status = { ...IDLE_STATUS, ffmpeg: false };
    clips = [clip(), clip({ id: "b2", name: "Sad trombone", format: "ffmpeg", playable: false })];

    await mount();

    expect(screen.getAllByText(/isn't installed/)).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Sad trombone - needs ffmpeg" })).toBeDisabled();
    // The clip that does not need it is unaffected.
    expect(screen.getByRole("button", { name: "Play Airhorn" })).toBeEnabled();
  });

  it("keeps a clip whose file vanished visible, but not firable", async () => {
    clips = [clip({ missing: true, playable: false })];

    await mount();

    const pad = screen.getByRole("button", { name: "Airhorn - file missing" });
    expect(pad).toBeInTheDocument();
    expect(pad).toBeDisabled();

    await act(async () => {
      fireEvent.click(pad);
    });

    expect(cmds("soundboard_toggle")).toHaveLength(0);
  });

  it("says how to add clips when the library is empty", async () => {
    clips = [];

    await mount();

    expect(screen.getByText("No clips yet.")).toBeInTheDocument();
    expect(screen.getByText(/turns up here as a pad/)).toBeInTheDocument();
  });

  it("does not point the tablet at a dialog it cannot open", async () => {
    desktop = false;
    clips = [];

    await mount();

    expect(screen.getByText(/added on the PC/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add clip…" })).not.toBeInTheDocument();
  });
});

describe("ducking", () => {
  it("is off by default, and its attenuation is inert until it is on", async () => {
    // The switch is the shared Toggle, which carries no name of its own -
    // queried the way the other panels' switches are.
    const { container } = await mount();

    expect(screen.getByRole("slider", { name: "Ducking attenuation" })).toBeDisabled();

    await act(async () => {
      fireEvent.click(container.querySelector(".hs-card-right .toggle") as HTMLElement);
      await vi.advanceTimersByTimeAsync(400);
    });

    expect(cmds("soundboard_set_duck")[0][1]).toMatchObject({
      enabled: true,
      attenuationDb: -12,
    });
    expect(screen.getByRole("slider", { name: "Ducking attenuation" })).toBeEnabled();
  });

  it("writes a changed attenuation once the drag settles", async () => {
    const { container } = await mount();
    await act(async () => {
      fireEvent.click(container.querySelector(".hs-card-right .toggle") as HTMLElement);
      await vi.advanceTimersByTimeAsync(400);
    });

    await act(async () => {
      fireEvent.change(screen.getByRole("slider", { name: "Ducking attenuation" }), {
        target: { value: "-25" },
      });
      await vi.advanceTimersByTimeAsync(400);
    });

    const writes = cmds("soundboard_set_duck");
    expect(writes[writes.length - 1][1]).toMatchObject({ enabled: true, attenuationDb: -25 });
  });
});

describe("the poll loop", () => {
  it("keeps asking while the screen is open", async () => {
    await mount();
    const before = cmds("soundboard_status").length;

    await settle(3000);

    expect(cmds("soundboard_status").length).toBeGreaterThan(before);
  });

  it("stops the moment the screen goes away", async () => {
    const { unmount } = await mount();
    unmount();
    const after = cmds("soundboard_status").length;

    await settle(10_000);

    expect(cmds("soundboard_status")).toHaveLength(after);
  });
});
