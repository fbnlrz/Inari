/**
 * The soundboard: the clip library, what is playing, and mic ducking.
 *
 * Three things shape this store.
 *
 * 1. **There are no events.** `soundboard_status` is polled, and only while
 *    the screen is open - the same epoch/refcount handling `store/media.ts`
 *    uses, for the same reason: a reply landing after the tab closed must not
 *    write into a store nobody is watching, and nothing may keep ticking in
 *    the background. One read a second is enough; the seconds right after a
 *    press are polled faster, because that is when the lit pad would
 *    otherwise be wrong.
 * 2. **Firing is a toggle, and the backend does the toggling.** At most one
 *    clip plays: pressing the lit pad stops it, pressing another swaps. Doing
 *    that here as stop-then-play would be two round trips, and over the
 *    remote's WebSocket they can overtake each other - the stop would kill
 *    the clip it was meant to precede. So a press is one command,
 *    `soundboard_toggle`, and `soundboard_play` is left to the hotkeys.
 * 3. **Curating is desktop-only.** `soundboard_add_clip`, `_remove_clip`,
 *    `_rename_clip` and `_set_clip_volume` are off the remote allowlist (see
 *    src-tauri/src/remote/allowlist.rs). The actions live here, but only
 *    `isTauri` UI ever reaches them.
 */
import { create } from "zustand";
import { createDebouncer } from "../lib/debounce";
import { call } from "../lib/ipc";

/** Mirrors `decode::ClipFormat`. */
export type ClipFormat = "native" | "ffmpeg";

/** Mirrors `ClipInfo` in src-tauri/src/soundboard/mod.rs. No path, by design. */
export interface ClipInfo {
  id: string;
  name: string;
  volume_percent: number;
  format: ClipFormat;
  /** The file behind it is gone. Still listed - the drive may come back. */
  missing: boolean;
  /** False when firing it would only produce an error right now. */
  playable: boolean;
}

/** Mirrors `persistence::soundboard::Duck`. */
export interface Duck {
  enabled: boolean;
  /** Negative dB the microphone drops by while a clip plays. */
  attenuation_db: number;
}

/** Mirrors `SoundboardStatus`. */
export interface SoundboardStatus {
  /** How many clips are playing. Exclusive playback makes this 0 or 1. */
  playing: number;
  /** Whether compressed formats can be decoded at all. */
  ffmpeg: boolean;
  duck: Duck;
  /** False on the pactl fallback, where clips cannot be published. */
  available: boolean;
  /** Which clips are playing - what the lit pad is drawn from. A list even
   *  though exclusive playback keeps it to at most one entry. */
  playing_ids: string[];
}

/** Where a clip is heard. `both` is the default a soundboard normally means. */
export type PlayTargets = "both" | "chat" | "me";

/** Per-clip trim, matching `MAX_CLIP_VOLUME`. */
export const MAX_CLIP_VOLUME = 200;
/** Ducking range, matching `MIN_DUCK_DB`/`MAX_DUCK_DB`. */
export const MIN_DUCK_DB = -40;
export const MAX_DUCK_DB = 0;

/** Idle cadence: enough to notice a clip ending, cheap enough to leave on. */
const POLL_MS = 1000;
/** Right after a press, until the highlight has certainly settled. */
const BUSY_MS = 300;
const BUSY_FOR_MS = 1500;

/** Writes that a drag can produce hundreds of; each one persists a file. */
const WRITE_DEBOUNCE_MS = 200;

/** Nothing playing, and no reason yet to think anything is missing. */
export const IDLE_STATUS: SoundboardStatus = {
  playing: 0,
  ffmpeg: true,
  duck: { enabled: false, attenuation_db: -12 },
  available: true,
  playing_ids: [],
};

interface SoundboardState {
  clips: ClipInfo[];
  status: SoundboardStatus;
  /** Which pads are lit. Kept apart from `status` so a press can answer the
   *  finger immediately and let the next poll confirm it. */
  playingIds: string[];
  /** Where the next press sends its clip. Session-only, like the mixer's
   *  other momentary choices. */
  targets: PlayTargets;
  /** False until the first read lands, so the screen can stay quiet instead
   *  of flashing an empty state it is about to replace. */
  loaded: boolean;
  error: string | null;
  clearError: () => void;

  setTargets: (t: PlayTargets) => void;

  /** Begin polling. Reference-counted, so a second screen cannot end the
   *  first one's loop when it leaves. */
  start: () => void;
  stop: () => void;
  /** One status read. */
  poll: () => Promise<void>;
  /** Re-read the library. */
  refresh: () => Promise<void>;

  /** Fire the clip, or stop it if it is the one playing. */
  toggle: (id: string) => Promise<void>;
  stopAll: () => Promise<void>;
  setDuck: (enabled: boolean, attenuationDb: number) => void;

  // Desktop only - see the module doc.
  /** Extensions for the file dialog, straight from the decoder. */
  formats: () => Promise<string[]>;
  addClip: (path: string) => Promise<void>;
  removeClip: (id: string) => Promise<void>;
  renameClip: (id: string, name: string) => Promise<void>;
  setClipVolume: (id: string, volumePercent: number) => void;
}

export const useSoundboard = create<SoundboardState>((set, get) => {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running = 0;
  /** Bumped by `stop()`. Every in-flight read carries the epoch it started in
   *  and drops its result if that no longer matches. */
  let epoch = 0;
  /** Until when the fast cadence applies - `Date.now()` based, so a press
   *  during a wait shortens the *next* interval rather than none. */
  let busyUntil = 0;
  /** A ducking change the backend has not confirmed yet. The status carries
   *  `duck` too, so without this the poll in the middle of the write's
   *  debounce would flip the switch back under the finger. */
  let duckDirty = false;

  const fail = (e: unknown) => set({ error: String(e) });
  const debounced = createDebouncer(WRITE_DEBOUNCE_MS, (_key, e) => fail(e));

  const loop = async () => {
    const mine = epoch;
    await get().poll();
    if (mine !== epoch || running === 0) return;
    timer = setTimeout(() => void loop(), Date.now() < busyUntil ? BUSY_MS : POLL_MS);
  };

  /** Ask again right away, and keep asking quickly for a moment. A press
   *  changes which pad is lit, and that is the one thing a stale second is
   *  visible in. */
  const hurry = () => {
    busyUntil = Date.now() + BUSY_FOR_MS;
    if (running > 0) void get().poll();
  };

  /** Replace one clip in the list, leaving the rest identical. */
  const patch = (id: string, change: Partial<ClipInfo>) =>
    set({ clips: get().clips.map((c) => (c.id === id ? { ...c, ...change } : c)) });

  return {
    clips: [],
    status: IDLE_STATUS,
    playingIds: [],
    targets: "both",
    loaded: false,
    error: null,
    clearError: () => set({ error: null }),

    setTargets: (targets) => set({ targets }),

    start: () => {
      running++;
      if (running > 1) return;
      void get().refresh();
      void loop();
    },

    stop: () => {
      if (running > 0) running--;
      if (running > 0) return;
      epoch++;
      if (timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
    },

    poll: async () => {
      const mine = epoch;
      try {
        const status = await call<SoundboardStatus>("soundboard_status");
        if (mine !== epoch || !status) return;
        set({
          // An uncommitted ducking change is newer than what the status says.
          status: duckDirty ? { ...status, duck: get().status.duck } : status,
          // The backend is the authority on what is playing; the optimistic
          // highlight a press left behind only stands until this lands.
          playingIds: Array.isArray(status.playing_ids) ? status.playing_ids : [],
          loaded: true,
        });
      } catch (e) {
        if (mine !== epoch) return;
        set({ error: String(e), loaded: true });
      }
    },

    refresh: async () => {
      const mine = epoch;
      try {
        const clips = await call<ClipInfo[]>("soundboard_clips");
        if (mine !== epoch || !Array.isArray(clips)) return;
        set({ clips, loaded: true });
      } catch (e) {
        if (mine !== epoch) return;
        set({ error: String(e), loaded: true });
      }
    },

    toggle: async (id) => {
      const wasPlaying = get().playingIds.includes(id);
      // Answer the press now: at most one clip plays, so the new state is
      // known without waiting for a poll to confirm it.
      set({ playingIds: wasPlaying ? [] : [id] });
      try {
        await call("soundboard_toggle", { id, targets: get().targets });
      } catch (e) {
        fail(e);
      }
      hurry();
    },

    stopAll: async () => {
      set({ playingIds: [] });
      try {
        await call("soundboard_stop_all");
      } catch (e) {
        fail(e);
      }
      hurry();
    },

    setDuck: (enabled, attenuationDb) => {
      const attenuation_db = Math.max(MIN_DUCK_DB, Math.min(MAX_DUCK_DB, attenuationDb));
      duckDirty = true;
      set({ status: { ...get().status, duck: { enabled, attenuation_db } } });
      debounced(
        "duck",
        async () => {
          const duck = await call<Duck>("soundboard_set_duck", {
            enabled,
            attenuationDb: attenuation_db,
          });
          duckDirty = false;
          // The reply is what was actually stored, clamp and all.
          if (duck) set({ status: { ...get().status, duck } });
        },
        {
          onError: (e) => {
            // Stop shielding a value the backend rejected; the next poll is
            // then free to put the real setting back on screen.
            duckDirty = false;
            fail(e);
          },
        },
      );
    },

    formats: async () => {
      try {
        return await call<string[]>("soundboard_formats");
      } catch (e) {
        fail(e);
        return [];
      }
    },

    addClip: async (path) => {
      try {
        const clip = await call<ClipInfo>("soundboard_add_clip", { path });
        if (clip) set({ clips: [...get().clips, clip] });
      } catch (e) {
        fail(e);
      }
    },

    removeClip: async (id) => {
      const before = get().clips;
      set({ clips: before.filter((c) => c.id !== id) });
      try {
        await call("soundboard_remove_clip", { id });
      } catch (e) {
        set({ clips: before });
        fail(e);
      }
    },

    renameClip: async (id, name) => {
      const trimmed = name.trim();
      if (!trimmed) return;
      const before = get().clips;
      patch(id, { name: trimmed });
      try {
        await call("soundboard_rename_clip", { id, name: trimmed });
      } catch (e) {
        set({ clips: before });
        fail(e);
      }
    },

    setClipVolume: (id, volumePercent) => {
      const percent = Math.max(0, Math.min(MAX_CLIP_VOLUME, Math.round(volumePercent)));
      // The slider follows the finger; the write is the trailing edge of it.
      patch(id, { volume_percent: percent });
      debounced(`volume:${id}`, () =>
        call("soundboard_set_clip_volume", { id, volumePercent: percent }),
      );
    },
  };
});
