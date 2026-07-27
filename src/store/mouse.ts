import { create } from "zustand";
import { call, subscribe } from "../lib/ipc";
import { createDebouncer } from "../lib/debounce";

// Mirrors the Rust structs in src-tauri/src/mouse/.
export interface MouseStatus {
  present: boolean;
  model: string | null;
  wireless: boolean;
  battery_percent: number | null;
  charging: boolean;
}

export interface MouseConfig {
  dpi_presets: number[];
  dpi_selected: number;
  polling_hz: number;
  zones: [number, number, number][];
  rainbow: boolean;
  reactive: [number, number, number] | null;
  sleep_minutes: number;
  dim_seconds: number;
}

interface MouseSnapshot {
  connected: boolean;
  status: MouseStatus;
  config: MouseConfig;
  dpi_steps: number[];
  polling_rates: number[];
}

const emptyStatus: MouseStatus = {
  present: false,
  model: null,
  wireless: false,
  battery_percent: null,
  charging: false,
};

const defaultConfig: MouseConfig = {
  dpi_presets: [400, 800, 1200, 2400, 3200],
  dpi_selected: 0,
  polling_hz: 1000,
  zones: [
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
  ],
  rainbow: true,
  reactive: null,
  sleep_minutes: 5,
  dim_seconds: 30,
};

/** Zone labels in device order. */
export const ZONE_LABELS = ["Top", "Middle", "Bottom"] as const;

interface MouseState {
  connected: boolean;
  status: MouseStatus;
  config: MouseConfig;
  dpiSteps: number[];
  pollingRates: number[];
  error: string | null;
  /** Dismiss the error banner. */
  clearError: () => void;
  _initialized: boolean;

  init: () => Promise<void>;
  refresh: () => Promise<void>;
  setDpi: (presets: number[], selected: number) => Promise<void>;
  setPolling: (hz: number) => Promise<void>;
  setZoneColor: (zone: number, rgb: [number, number, number]) => Promise<void>;
  setRainbow: () => Promise<void>;
  setReactive: (rgb: [number, number, number] | null) => Promise<void>;
  setSleep: (minutes: number) => Promise<void>;
  setDim: (seconds: number) => Promise<void>;
}

export const useMouse = create<MouseState>((set, get) => {
  // Device writes are slow (the mouse needs ~50 ms between commands), so a
  // colour-picker drag must not fire one invoke per pixel.
  const debounced = createDebouncer(200, (_key, e) => set({ error: String(e) }));

  return {
    connected: false,
    status: emptyStatus,
    config: defaultConfig,
    dpiSteps: [],
    pollingRates: [125, 250, 500, 1000],
    error: null,
    clearError: () => set({ error: null }),
    _initialized: false,

    refresh: async () => {
      const snap = await call<MouseSnapshot>("get_mouse_status");
      set({
        connected: snap.connected,
        status: snap.status,
        config: snap.config,
        dpiSteps: snap.dpi_steps,
        pollingRates: snap.polling_rates,
      });
    },

    init: async () => {
      if (get()._initialized) return;
      set({ _initialized: true });
      try {
        await get().refresh();
      } catch (e) {
        console.error("mouse init:", e);
      }
      void subscribe<MouseStatus>("mouse-status", (status) => set({ status }));
      void subscribe<boolean>("mouse-presence", (connected) => {
        set((s) => ({
          connected,
          status: connected ? s.status : emptyStatus,
        }));
        if (connected) {
          void get().refresh().catch((err: unknown) => set({ error: String(err) }));
        }
      });
    },

    setDpi: async (presets, selected) => {
      set((s) => ({
        config: { ...s.config, dpi_presets: presets, dpi_selected: selected },
      }));
      debounced("dpi", () => call("mouse_set_dpi", { presets, selected }));
    },
    setPolling: async (hz) => {
      const prev = get().config.polling_hz;
      set((s) => ({ config: { ...s.config, polling_hz: hz } }));
      try {
        await call("mouse_set_polling", { hz });
      } catch (e) {
        // Callers `void` these, so an uncaught rejection would leave the UI
        // showing a rate the mouse never accepted.
        set((s) => ({ config: { ...s.config, polling_hz: prev }, error: String(e) }));
      }
    },
    setZoneColor: async (zone, rgb) => {
      set((s) => {
        const zones = s.config.zones.map((z, i) => (i === zone ? rgb : z));
        return { config: { ...s.config, zones, rainbow: false } };
      });
      debounced(`zone:${zone}`, () =>
        call("mouse_set_zone_color", { zone, r: rgb[0], g: rgb[1], b: rgb[2] }),
      );
    },
    setRainbow: async () => {
      const prev = get().config.rainbow;
      set((s) => ({ config: { ...s.config, rainbow: true } }));
      try {
        await call("mouse_set_rainbow");
      } catch (e) {
        set((s) => ({ config: { ...s.config, rainbow: prev }, error: String(e) }));
      }
    },
    setReactive: async (rgb) => {
      const prev = get().config.reactive;
      set((s) => ({ config: { ...s.config, reactive: rgb } }));
      try {
        await call("mouse_set_reactive", { rgb });
      } catch (e) {
        set((s) => ({ config: { ...s.config, reactive: prev }, error: String(e) }));
      }
    },
    setSleep: async (minutes) => {
      set((s) => ({ config: { ...s.config, sleep_minutes: minutes } }));
      debounced("sleep", () => call("mouse_set_sleep", { minutes }));
    },
    setDim: async (seconds) => {
      set((s) => ({ config: { ...s.config, dim_seconds: seconds } }));
      debounced("dim", () => call("mouse_set_dim", { seconds }));
    },
  };
});
