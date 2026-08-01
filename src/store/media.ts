/**
 * Now playing, and the transport controls for it.
 *
 * The backend (src-tauri/src/commands/media.rs) shells out to `playerctl`, so
 * every read here costs a fork+exec. Two consequences shape this store:
 *
 * 1. Status is polled once a second and no faster. The UI does not poll for a
 *    smooth progress bar; it advances the last known position by the wall
 *    clock (`displayPosition`), exactly as the OLED's media mode does.
 * 2. Cover art travels as base64 and runs to a few hundred kilobytes, so it is
 *    fetched only when `art_generation` changes. The generation of whatever is
 *    held is remembered - including a generation that turned out to have no
 *    readable image, so a miss is not retried every second either.
 */
import { create } from "zustand";
import { call } from "../lib/ipc";

/** Mirrors `MediaPlayer` in src-tauri/src/commands/media.rs. */
export interface MediaPlayer {
  /** The playerctl name, e.g. `chromium.instance1652439`. */
  id: string;
  /** The part worth showing: `chromium`, `spotify`, `vlc`. */
  label: string;
}

/** Mirrors `MediaStatus`. */
export interface MediaStatus {
  /** Which player this describes; null when nothing is running. */
  player: string | null;
  title: string;
  artist: string;
  album: string;
  playing: boolean;
  position_us: number;
  /** 0 when the player reports no length (live streams). */
  length_us: number;
  /** Changes when the cover changes; 0 means there is none. */
  art_generation: number;
  /** False when `playerctl` is not installed. */
  available: boolean;
}

/** Mirrors `MediaArt`. */
interface MediaArtReply {
  generation: number;
  mime: string;
  base64: string;
}

/** The cover we currently hold. `url` is null for "asked, there was none". */
export interface MediaArt {
  generation: number;
  url: string | null;
}

/** Nothing playing, and no reason yet to think playerctl is missing. */
export const IDLE: MediaStatus = {
  player: null,
  title: "",
  artist: "",
  album: "",
  playing: false,
  position_us: 0,
  length_us: 0,
  art_generation: 0,
  available: true,
};

/** One status read per second: enough to catch a track change, cheap enough
 *  to leave running while the screen is open. */
const POLL_MS = 1000;

/** Players come and go far more slowly than positions move, so the list is
 *  refreshed every fifth poll rather than on each one. */
const PLAYERS_EVERY = 5;

/**
 * The last polled position advanced by the wall clock, so the bar moves
 * smoothly between polls instead of stepping once a second. The same rule the
 * OLED uses (`display_position` in headset/oled_mode_media.rs): only a playing
 * track advances, and it never runs past a known length.
 */
export function displayPosition(status: MediaStatus, statusAt: number, now: number): number {
  let pos = status.position_us;
  if (status.playing) pos += Math.max(0, now - statusAt) * 1000;
  return status.length_us > 0 ? Math.min(pos, status.length_us) : pos;
}

interface MediaState {
  status: MediaStatus;
  /** `Date.now()` when `status` arrived - the anchor `displayPosition` reads. */
  statusAt: number;
  players: MediaPlayer[];
  /** Chosen player, or null for "whatever playerctl picks". Session-only. */
  selected: string | null;
  art: MediaArt | null;
  /** False until the first status lands, so the screen can stay quiet
   *  instead of flashing an empty state it is about to replace. */
  loaded: boolean;
  error: string | null;
  clearError: () => void;

  /** Begin polling. Reference-counted, so a second screen cannot end the
   *  first one's loop when it leaves. */
  start: () => void;
  stop: () => void;
  /** One status (and, if the cover changed, one art) read. */
  poll: () => Promise<void>;
  select: (id: string | null) => void;

  playPause: () => Promise<void>;
  next: () => Promise<void>;
  previous: () => Promise<void>;
  seek: (positionUs: number) => Promise<void>;
}

export const useMedia = create<MediaState>((set, get) => {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running = 0;
  let ticks = 0;
  /** Bumped by `stop()`. Every in-flight read carries the epoch it started in
   *  and drops its result if that no longer matches, so a reply arriving after
   *  the screen closed cannot write into a store nobody is watching. */
  let epoch = 0;
  /** Bumped by every optimistic write, so a poll that started earlier knows
   *  its answer is stale and steps aside. `epoch` only counts subscriber
   *  restarts and cannot serve for this. */
  let localEdits = 0;
  /** Generation currently being fetched; keeps a slow art read from being
   *  issued again by the next poll. */
  let artInFlight = 0;

  const target = () => get().selected ?? undefined;

  const syncArt = async (status: MediaStatus, player: string | undefined, mine: number) => {
    if (status.art_generation === 0) {
      if (get().art) set({ art: null });
      return;
    }
    // The whole point of the generation: same cover, no transfer.
    if (get().art?.generation === status.art_generation) return;
    if (artInFlight === status.art_generation) return;
    artInFlight = status.art_generation;
    try {
      const art = await call<MediaArtReply | null>("media_art", { player });
      if (mine !== epoch) return;
      set({
        art: art
          ? { generation: art.generation, url: `data:${art.mime};base64,${art.base64}` }
          : // Remember the miss under the generation we asked for, or every
            // poll would ask again for a cover that cannot be read.
            { generation: status.art_generation, url: null },
      });
    } catch {
      // Keep whatever cover is on screen; the next poll retries.
    } finally {
      artInFlight = 0;
    }
  };

  const refreshPlayers = async (mine: number) => {
    try {
      const players = await call<MediaPlayer[]>("media_players");
      if (mine !== epoch || !Array.isArray(players)) return;
      const selected = get().selected;
      set({
        players,
        // A player that quit must not keep the picker pointed at a name
        // playerctl will refuse; fall back to automatic.
        selected: selected && players.some((p) => p.id === selected) ? selected : null,
      });
    } catch {
      // A stale list is harmless - the status poll surfaces real failures.
    }
  };

  const tick = async () => {
    const mine = epoch;
    if (ticks % PLAYERS_EVERY === 0) await refreshPlayers(mine);
    ticks++;
    if (mine !== epoch) return;
    await get().poll();
  };

  const loop = async () => {
    const mine = epoch;
    await tick();
    if (mine !== epoch || running === 0) return;
    timer = setTimeout(() => void loop(), POLL_MS);
  };

  /** Fire a transport command and keep the failure out of the console. */
  const control = async (
    cmd: "media_play_pause" | "media_next" | "media_previous",
  ): Promise<void> => {
    try {
      await call(cmd, { player: target() });
    } catch (e) {
      set({ error: String(e) });
    }
  };

  return {
    status: IDLE,
    statusAt: 0,
    players: [],
    selected: null,
    art: null,
    loaded: false,
    error: null,
    clearError: () => set({ error: null }),

    start: () => {
      running++;
      if (running > 1) return;
      ticks = 0;
      // Re-anchor before the first poll lands. Reopening the tab an hour
      // later would otherwise interpolate from an hour-old reading and show
      // the track pinned at its end for the moment before the poll answers.
      set({ statusAt: Date.now() });
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
      const mineLocal = localEdits;
      const player = target();
      try {
        const status = await call<MediaStatus>("media_status", { player });
        if (mine !== epoch || !status) return;
        // A read that was already in flight when the user tapped pause or
        // dragged the bar carries the state from before the tap. The backend
        // shells out to playerctl, so that read can easily be the slower of
        // the two — and writing it back flipped the button straight back to
        // "pause" and threw the handle back to where it started.
        if (mineLocal !== localEdits) return;
        set({ status, statusAt: Date.now(), loaded: true });
        await syncArt(status, player, mine);
      } catch (e) {
        if (mine !== epoch) return;
        set({ error: String(e), loaded: true });
      }
    },

    select: (id) => {
      if (get().selected === id) return;
      // Another player is another track: drop the cover rather than leave the
      // old one sitting over the new title until the next generation lands.
      set({ selected: id, art: null });
      void get().poll();
    },

    playPause: async () => {
      const { status, statusAt } = get();
      const at = Date.now();
      // Answer the tap now. Freeze the interpolated position first, or a
      // pause would keep the bar creeping until the next poll corrects it.
      localEdits++;
      set({
        status: {
          ...status,
          position_us: displayPosition(status, statusAt, at),
          playing: !status.playing,
        },
        statusAt: at,
      });
      await control("media_play_pause");
    },

    next: async () => {
      await control("media_next");
      await get().poll();
    },

    previous: async () => {
      await control("media_previous");
      await get().poll();
    },

    seek: async (positionUs) => {
      localEdits++;
      set({ status: { ...get().status, position_us: positionUs }, statusAt: Date.now() });
      try {
        await call("media_seek", { player: target(), positionUs });
      } catch (e) {
        set({ error: String(e) });
      }
    },
  };
});
