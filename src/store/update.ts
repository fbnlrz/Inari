import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface UpdateInfo {
  current: string;
  latest: string;
  available: boolean;
  url: string;
  notes: string;
  deb_url: string | null;
  can_self_install: boolean;
}

interface UpdateState {
  info: UpdateInfo | null;
  checking: boolean;
  applying: boolean;
  installed: boolean; // update applied, restart pending
  error: string | null;
  dismissed: boolean;
  check: () => Promise<void>;
  apply: () => Promise<void>;
  restart: () => Promise<void>;
  openNotes: () => Promise<void>;
  dismiss: () => void;
}

export const useUpdate = create<UpdateState>((set, get) => ({
  info: null,
  checking: false,
  applying: false,
  installed: false,
  error: null,
  dismissed: false,

  check: async () => {
    if (get().checking) return;
    set({ checking: true, error: null });
    try {
      const info = await invoke<UpdateInfo>("check_update");
      set({ info, checking: false, dismissed: false });
    } catch (e) {
      set({ checking: false, error: String(e) });
    }
  },

  apply: async () => {
    const info = get().info;
    if (!info?.deb_url || get().applying) return;
    set({ applying: true, error: null });
    try {
      await invoke("apply_update", { debUrl: info.deb_url });
      set({ applying: false, installed: true });
      // The new .deb is in place; relaunch so it takes over. The manual
      // "Restart now" button stays as a fallback if this ever doesn't fire.
      await invoke("restart_app");
    } catch (e) {
      set({ applying: false, error: String(e) });
    }
  },

  restart: async () => {
    await invoke("restart_app");
  },

  openNotes: async () => {
    const url = get().info?.url;
    if (url) await invoke("open_url", { url });
  },

  dismiss: () => set({ dismissed: true }),
}));
