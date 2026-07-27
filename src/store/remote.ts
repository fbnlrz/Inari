import { create } from "zustand";
import { call } from "../lib/ipc";

/**
 * Inari Remote: the app serves its own UI on the LAN so a tablet can drive the
 * mixer by touch. This store is only the desktop side of that - the pairing
 * and control surface in Settings. It never decides what a remote client may
 * do; the allowlist that gates the 121 commands lives in Rust, deny by
 * default, and nothing here can widen it.
 */

/** A bind target the backend found: one address the listener could sit on. */
export interface RemoteInterface {
  /** What the listener binds to, e.g. "127.0.0.1" or "192.168.1.42". */
  address: string;
  /** Human name for the picker, e.g. "Wi-Fi (wlan0)". */
  label: string;
  /** Loopback reaches this machine only - the safe default. */
  loopback: boolean;
  /** 0.0.0.0: every interface at once, including any the user forgot about. */
  wildcard: boolean;
}

export interface RemoteStatus {
  enabled: boolean;
  /** The address currently bound, or the one that will be used when enabled. */
  address: string;
  port: number;
  /** Base URL to type on the tablet. Carries no token, so it is not a secret. */
  url: string;
  /** Clients holding a live connection right now. */
  clients: number;
  interfaces: RemoteInterface[];
}

/**
 * Everything needed to pair a device. All three fields carry the token, so
 * treat the whole struct as the secret it is - see RemoteSection for why the
 * QR is as sensitive as the string next to it.
 */
export interface RemotePairing {
  /** Pairing URL: base URL plus the token. */
  url: string;
  /** QR of `url`, rendered by the Rust side - the bundle ships no QR library. */
  qr_svg: string;
  token: string;
}

/**
 * Command names the Rust side registers for the remote server, in one place
 * so a rename is a single edit rather than a hunt through call sites. They are
 * members of `Command`, which a Rust test pins to the `generate_handler!`
 * list - so a rename that only happens on one side fails the build.
 */
export const REMOTE_COMMANDS = {
  status: "get_remote_status",
  setEnabled: "set_remote_enabled",
  setBind: "set_remote_bind",
  pairing: "get_remote_pairing",
  regenerate: "regenerate_remote_token",
} as const;

interface RemoteState {
  status: RemoteStatus | null;
  /** Null whenever the server is off - no token on screen for a dead listener. */
  pairing: RemotePairing | null;
  /** A command is in flight; the switch and the buttons stay put until it lands. */
  busy: boolean;
  error: string | null;
  /** Read the current status. Also used by the poll that keeps `clients` fresh. */
  fetch: () => Promise<void>;
  setEnabled: (enabled: boolean) => Promise<void>;
  setBind: (address: string, port: number) => Promise<void>;
  regenerateToken: () => Promise<void>;
}

export const useRemote = create<RemoteState>((set, get) => ({
  status: null,
  pairing: null,
  busy: false,
  error: null,

  fetch: async () => {
    try {
      const status = await call<RemoteStatus>(REMOTE_COMMANDS.status);
      if (!status.enabled) {
        // A server that went down on its own (port taken, interface gone) must
        // not leave a stale QR up that pairs nobody.
        set({ status, pairing: null, error: null });
        return;
      }
      // Inari can start with the server already on, so the pairing has to be
      // pulled here too - otherwise the QR only appears after a toggle. The
      // `??` keeps the 3s client-count poll from re-fetching it every tick.
      const pairing = get().pairing ?? (await call<RemotePairing>(REMOTE_COMMANDS.pairing));
      set({ status, pairing, error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setEnabled: async (enabled) => {
    if (get().busy) return;
    set({ busy: true, error: null });
    try {
      const status = await call<RemoteStatus>(REMOTE_COMMANDS.setEnabled, { enabled });
      // The backend answers with what it actually did: a bind that failed comes
      // back disabled, so the switch cannot show "on" over a dead listener.
      const pairing = status.enabled
        ? await call<RemotePairing>(REMOTE_COMMANDS.pairing)
        : null;
      set({ status, pairing, busy: false });
    } catch (e) {
      set({ busy: false, error: String(e) });
    }
  },

  setBind: async (address, port) => {
    if (get().busy) return;
    set({ busy: true, error: null });
    try {
      const status = await call<RemoteStatus>(REMOTE_COMMANDS.setBind, { address, port });
      // Moving the listener changes the URL, so the old QR now points nowhere.
      const pairing = status.enabled
        ? await call<RemotePairing>(REMOTE_COMMANDS.pairing)
        : null;
      set({ status, pairing, busy: false });
    } catch (e) {
      set({ busy: false, error: String(e) });
    }
  },

  regenerateToken: async () => {
    if (get().busy) return;
    set({ busy: true, error: null });
    try {
      const pairing = await call<RemotePairing>(REMOTE_COMMANDS.regenerate);
      // Every paired device was just cut off, so refresh the count rather than
      // leaving the old one to read as if they were still connected.
      set({ pairing, busy: false });
      await get().fetch();
    } catch (e) {
      set({ busy: false, error: String(e) });
    }
  },
}));
