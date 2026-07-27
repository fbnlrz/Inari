import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

/** Wire ids of the bindable actions; mirrors `HotkeyAction` in prefs.rs. */
export type HotkeyAction = "mic_mute" | "channel_mute" | "cycle_profile" | "toggle_window";

export interface HotkeyBinding {
  action: HotkeyAction;
  /** null = unbound. */
  accelerator: string | null;
  /** Sink the channel-mute row acts on (null = the first channel). */
  channel: string | null;
  /** True only when a binding exists *and* the session granted the grab. */
  registered: boolean;
  /** Why the grab failed, when it did. */
  error: string | null;
}

export const HOTKEY_LABELS: Record<HotkeyAction, { title: string; sub: string; icon: string }> = {
  mic_mute: { title: "Mute microphone", sub: "Toggles the Inari mic chain", icon: "mic_off" },
  channel_mute: { title: "Mute a channel", sub: "Toggles the channel picked below", icon: "volume_off" },
  cycle_profile: { title: "Next profile", sub: "Loads the next saved profile", icon: "swap_horiz" },
  toggle_window: { title: "Show/hide Inari", sub: "Brings the window up, or back to the tray", icon: "web_asset" },
};

/**
 * Codes that are only ever a modifier. A capture that ends on one of these is
 * still in progress - the user is holding Ctrl, not asking to bind Ctrl.
 */
const MODIFIER_CODES = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/;

/**
 * Keys allowed to be bound on their own. Everything else needs a modifier:
 * a bare letter registered system-wide swallows that letter in every other
 * app, which is a trap the user only discovers later.
 */
const STANDALONE_CODES = /^(F\d{1,2}|Pause|ScrollLock|PrintScreen|Insert)$/;

/**
 * Turn a captured key press into an accelerator the backend can parse
 * (`Ctrl+Shift+KeyM`). Returns null while the press is not bindable yet, so a
 * capture control can just keep listening.
 */
export function toAccelerator(event: {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}): string | null {
  if (MODIFIER_CODES.test(event.code)) return null;
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Super");
  if (parts.length === 0 && !STANDALONE_CODES.test(event.code)) return null;
  parts.push(event.code);
  return parts.join("+");
}

/** `Ctrl+Shift+KeyM` -> `Ctrl + Shift + M`, for display only. */
export function formatAccelerator(accelerator: string): string {
  return accelerator
    .split("+")
    .map((part) => part.replace(/^Key/, "").replace(/^Digit/, "").replace(/^Arrow/, ""))
    .join(" + ");
}

interface HotkeyStore {
  bindings: HotkeyBinding[];
  /** Last command failure (an unparsable accelerator, a failed save). */
  error: string | null;
  fetch: () => Promise<void>;
  /** Bind an action, or clear it with null. */
  setBinding: (action: HotkeyAction, accelerator: string | null) => Promise<void>;
  /** Pick the channel the channel-mute binding acts on (null = first). */
  setChannel: (sinkName: string | null) => Promise<void>;
}

export const useHotkeys = create<HotkeyStore>((set) => ({
  bindings: [],
  error: null,

  fetch: async () => {
    try {
      set({ bindings: await invoke<HotkeyBinding[]>("get_hotkeys"), error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setBinding: async (action, accelerator) => {
    try {
      // The backend re-registers and answers with the fresh list, so a grab
      // the session refused shows up immediately instead of after a restart.
      const bindings = await invoke<HotkeyBinding[]>("set_hotkey", { action, accelerator });
      set({ bindings, error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setChannel: async (sinkName) => {
    try {
      const bindings = await invoke<HotkeyBinding[]>("set_hotkey_channel", { sinkName });
      set({ bindings, error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },
}));
