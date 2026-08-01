import { create } from "zustand";
import { call, subscribe } from "../lib/ipc";
import { createDebouncer } from "../lib/debounce";

// Mirrors the Rust structs in src-tauri/src/keyboard/.

export type EffectKind =
  | "off"
  | "static"
  | "per_key"
  | "breathing"
  | "wave"
  | "rainbow"
  | "gradient"
  | "audio"
  | "scene"
  | "reactive"
  | "color_shift";

export type Rgb = [number, number, number];

export interface Lighting {
  kind: EffectKind;
  scene: Scene;
  color: Rgb;
  color2: Rgb;
  speed: number;
  brightness: number;
  reverse: boolean;
  per_key: Record<number, Rgb>;
}

/** How a model's OLED is addressed. Opaque to the UI — it round-trips. */
export type OledWire =
  | { single: { cmd: number; page: boolean } }
  | "gen3_wired"
  | { chunked: { cmd: number; page: boolean } };

export type KbMode =
  | "clock"
  | "clock_date"
  | "system"
  | "temps"
  | "now_playing"
  | "status"
  | "text";

export type Physical = "ansi" | "iso";

/** Themed scenes, mirroring keyboard::scenes::Scene. */
export type Scene =
  | "tokyo_night"
  | "osaka_neon"
  | "sakura"
  | "kanagawa"
  | "foxfire"
  | "rain"
  | "fuji_sunrise"
  | "ripple"
  | "aurora"
  | "lantern"
  | "starfield"
  | "slash"
  | "hanabi"
  | "torii"
  | "shibuya"
  | "neon_rain"
  | "glitch"
  | "vaporwave"
  | "koi"
  | "komorebi"
  | "taiko"
  | "inferno"
  | "plasma"
  | "kaminari";

/** Swatches for the scene chips — the palettes the Rust scenes paint with. */
export const SCENE_SWATCHES: Record<Scene, string[]> = {
  tokyo_night: ["#7aa2f7", "#bb9af7", "#7dcfff", "#f7768e", "#9ece6a"],
  osaka_neon: ["#ff28aa", "#28e6ff", "#ffaa1e", "#8cff3c"],
  sakura: ["#3c182e", "#ff82b4", "#fff0f5"],
  kanagawa: ["#040a30", "#102e78", "#78beeb", "#ffffff"],
  foxfire: ["#06040e", "#5aaaff", "#e1f5ff"],
  rain: ["#03060f", "#96dcff", "#ffffff"],
  fuji_sunrise: ["#080a28", "#281a4e", "#be463c", "#ff963c"],
  ripple: ["#000000", "#ff5200", "#ffffff"],
  aurora: ["#14c88c", "#28e6c8", "#7878ff", "#b450dc"],
  lantern: ["#ff781e", "#ffcd78"],
  starfield: ["#050510", "#282878", "#ffffeb"],
  slash: ["#020204", "#ff5200", "#ffffff"],
  hanabi: ["#030310", "#ff5a3c", "#ffd25a", "#78dcff", "#ff78dc"],
  torii: ["#02040f", "#dc280a", "#ff823c"],
  shibuya: ["#040408", "#ffdc96", "#50e6ff"],
  neon_rain: ["#06020c", "#ff28be", "#28dcff"],
  glitch: ["#14dcff", "#b428ff", "#ff2878", "#ffdc3c"],
  vaporwave: ["#280a46", "#ff3c8c", "#ffc83c", "#50fff0"],
  koi: ["#020c0e", "#ff6e14", "#fff5eb"],
  komorebi: ["#04100a", "#3caa46", "#e6ffbe"],
  taiko: ["#100202", "#dc1e1e", "#ffc83c"],
  inferno: ["#000000", "#5a0000", "#dc3c00", "#ffaa14", "#fffac8"],
  plasma: ["#ff0078", "#ffb400", "#00ffa0", "#008cff", "#a000ff"],
  kaminari: ["#060814", "#5a6ebe", "#ffffff"],
};

export interface KeyboardStatus {
  present: boolean;
  model: string | null;
  product_id: number;
  form: "full" | "tkl" | "mini" | null;
  generation: "gen1" | "gen2" | "gen3" | null;
  wireless: boolean;
  firmware: string | null;
  battery_percent: number | null;
  region: number | null;
  has_oled: boolean;
  has_actuation: boolean;
  oled_wire: OledWire | null;
  /** What the keyboard itself reports, so the sliders start on reality. */
  lighting: { brightness: number; idle_brightness: number; idle_timeout_ms: number } | null;
  power: { timeout_ms: number; high_efficiency: boolean } | null;
}

export interface KeyboardConfig {
  version: number;
  enabled: boolean;
  lighting: Lighting;
  physical: Physical;
  oled_mode: KbMode | null;
  oled_text: string[];
  oled_wire: OledWire | null;
  oled_screensaver_minutes: number;
  actuation: number | null;
  per_key_actuation: Record<number, number>;
  /** Rapid Trigger sensitivity for every key; 0 is off. */
  rapid_trigger: number;
  protection_mode: boolean;
  rapid_tap: boolean;
  /** The keyboard's OWN dim timer, in seconds; 0 never dims. */
  idle_timeout_secs: number;
  idle_brightness: number;
  /** Minutes before the board sleeps; 0 never sleeps. */
  sleep_minutes: number;
  high_efficiency: boolean;
}

export interface KeyDef {
  hid: number;
  label: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

interface KeyboardSnapshot {
  connected: boolean;
  status: KeyboardStatus;
  config: KeyboardConfig;
  layout: KeyDef[];
  extent: [number, number];
  preview: [number, Rgb][];
  oled_modes: { mode: KbMode; label: string }[];
  oled_wires: { wire: OledWire; label: string }[];
  scenes: { scene: Scene; label: string; hint: string }[];
}

const emptyStatus: KeyboardStatus = {
  present: false,
  model: null,
  product_id: 0,
  form: null,
  generation: null,
  wireless: false,
  firmware: null,
  battery_percent: null,
  region: null,
  has_oled: false,
  has_actuation: false,
  oled_wire: null,
  lighting: null,
  power: null,
};

const defaultLighting: Lighting = {
  kind: "static",
  scene: "tokyo_night",
  color: [255, 82, 0],
  color2: [0, 120, 255],
  speed: 50,
  brightness: 100,
  reverse: false,
  per_key: {},
};

const defaultConfig: KeyboardConfig = {
  version: 1,
  enabled: true,
  lighting: defaultLighting,
  physical: "iso",
  oled_mode: null,
  oled_text: [],
  oled_wire: null,
  oled_screensaver_minutes: 10,
  actuation: null,
  per_key_actuation: {},
  rapid_trigger: 0,
  protection_mode: false,
  rapid_tap: false,
  idle_timeout_secs: 60,
  idle_brightness: 3,
  sleep_minutes: 5,
  high_efficiency: false,
};

/** Effects the user can pick, with what each one needs from the UI. */
export const EFFECTS: {
  kind: EffectKind;
  label: string;
  hint: string;
  /** Rendered by the keyboard itself, so it survives Inari closing. */
  onboard?: boolean;
  colors?: 0 | 1 | 2;
  speed?: boolean;
}[] = [
  { kind: "static", label: "Static", hint: "One colour across the board", colors: 1 },
  { kind: "per_key", label: "Per key", hint: "Paint each key yourself", colors: 0 },
  { kind: "breathing", label: "Breathing", hint: "Fades in and out", colors: 1, speed: true },
  { kind: "wave", label: "Wave", hint: "Rainbow sweeping across", colors: 0, speed: true },
  { kind: "rainbow", label: "Rainbow", hint: "Whole board cycles together", colors: 0, speed: true },
  { kind: "gradient", label: "Gradient", hint: "Blend between two colours", colors: 2 },
  { kind: "audio", label: "Audio", hint: "Follows what is playing", colors: 1 },
  {
    kind: "scene",
    label: "Scenes",
    hint: "Twenty-four themed scenes — Tokyo Night, Osaka neon, sakura, Kanagawa and more",
    colors: 1,
    speed: true,
  },
  {
    kind: "reactive",
    label: "Reactive",
    hint: "Keys flash on press — rendered by the keyboard",
    colors: 1,
    onboard: true,
  },
  {
    kind: "color_shift",
    label: "Colour shift",
    hint: "Two-colour fade — rendered by the keyboard",
    colors: 2,
    speed: true,
    onboard: true,
  },
  { kind: "off", label: "Off", hint: "Hand the LEDs back to the keyboard", colors: 0, onboard: true },
];

interface KeyboardState {
  connected: boolean;
  status: KeyboardStatus;
  config: KeyboardConfig;
  layout: KeyDef[];
  extent: [number, number];
  preview: Record<number, Rgb>;
  oledModes: { mode: KbMode; label: string }[];
  oledWires: { wire: OledWire; label: string }[];
  scenes: { scene: Scene; label: string; hint: string }[];
  error: string | null;
  clearError: () => void;
  _initialized: boolean;

  init: () => Promise<void>;
  refresh: () => Promise<void>;
  setEnabled: (enabled: boolean) => Promise<void>;
  setLighting: (patch: Partial<Lighting>) => Promise<void>;
  setKeyColor: (hid: number, rgb: Rgb | null) => Promise<void>;
  fillKeys: (rgb: Rgb) => Promise<void>;
  clearKeys: () => Promise<void>;
  setPhysical: (physical: Physical) => Promise<void>;
  setOled: (mode: KbMode | null, text?: string[]) => Promise<void>;
  setOledWire: (wire: OledWire | null) => Promise<void>;
  setScreensaver: (minutes: number) => Promise<void>;
  testOled: (wire: OledWire) => Promise<void>;
  setActuation: (tenths: number) => Promise<void>;
  setKeyActuation: (hid: number, tenths: number | null) => Promise<void>;
  setRapidTrigger: (sensitivity: number) => Promise<void>;
  setProtectionMode: (on: boolean) => Promise<void>;
  setRapidTap: (on: boolean) => Promise<void>;
  setIdle: (seconds: number, brightness: number) => Promise<void>;
  setPowerSaving: (minutes: number, highEfficiency: boolean) => Promise<void>;
  sendRaw: (cmd: number, payload: number[]) => Promise<void>;
}

/** Overlay the settings the keyboard reports onto the stored configuration. */
function fromDevice(config: KeyboardConfig, status: KeyboardStatus): KeyboardConfig {
  const merged = { ...config };
  if (status.lighting) {
    merged.idle_timeout_secs = Math.round(status.lighting.idle_timeout_ms / 1000);
    merged.idle_brightness = status.lighting.idle_brightness;
  }
  if (status.power) {
    merged.sleep_minutes = Math.round(status.power.timeout_ms / 60000);
    merged.high_efficiency = status.power.high_efficiency;
  }
  return merged;
}

export const useKeyboard = create<KeyboardState>((set, get) => {
  // Colour pickers fire per pixel of drag; the device gets one write per burst.
  const debounced = createDebouncer(150, (_key, e) => set({ error: String(e) }));

  const apply = (snap: KeyboardSnapshot) =>
    set({
      connected: snap.connected,
      status: snap.status,
      // The keyboard stores its own idle and power timers, so what it reports
      // wins over what Inari last wrote. Otherwise the sliders would show a
      // remembered value while the board sits on something else — changed by
      // SteelSeries GG, another machine, or a factory reset.
      config: fromDevice(snap.config, snap.status),
      layout: snap.layout,
      extent: snap.extent,
      preview: Object.fromEntries(snap.preview),
      oledModes: snap.oled_modes,
      oledWires: snap.oled_wires,
      scenes: snap.scenes,
    });

  return {
    connected: false,
    status: emptyStatus,
    config: defaultConfig,
    layout: [],
    extent: [18.25, 6],
    preview: {},
    oledModes: [],
    oledWires: [],
    scenes: [],
    error: null,
    clearError: () => set({ error: null }),
    _initialized: false,

    refresh: async () => {
      apply(await call<KeyboardSnapshot>("get_keyboard_status"));
    },

    init: async () => {
      if (get()._initialized) return;
      set({ _initialized: true });
      try {
        await get().refresh();
      } catch (e) {
        console.error("keyboard init:", e);
      }
      void subscribe<KeyboardStatus>("keyboard-status", (status) => set({ status }));
      void subscribe<boolean>("keyboard-presence", (connected) => {
        set((s) => ({ connected, status: connected ? s.status : emptyStatus }));
        void get().refresh().catch((err: unknown) => set({ error: String(err) }));
      });
    },

    setEnabled: async (enabled) => {
      set((s) => ({ config: { ...s.config, enabled } }));
      try {
        await call("keyboard_set_enabled", { enabled });
      } catch (e) {
        set((s) => ({ config: { ...s.config, enabled: !enabled }, error: String(e) }));
      }
    },

    setLighting: async (patch) => {
      const lighting = { ...get().config.lighting, ...patch };
      set((s) => ({ config: { ...s.config, lighting } }));
      debounced("lighting", async () => {
        await call("keyboard_set_lighting", { lighting });
        // The preview is rendered by the same code the device runs, so pull it
        // back rather than reimplementing every effect in TypeScript.
        await get().refresh();
      });
    },

    setKeyColor: async (hid, rgb) => {
      set((s) => {
        const per_key = { ...s.config.lighting.per_key };
        if (rgb) per_key[hid] = rgb;
        else delete per_key[hid];
        return {
          config: { ...s.config, lighting: { ...s.config.lighting, kind: "per_key", per_key } },
          preview: { ...s.preview, [hid]: rgb ?? [0, 0, 0] },
        };
      });
      debounced(`key:${hid}`, () => call("keyboard_set_key_color", { hid, rgb }));
    },

    fillKeys: async (rgb) => {
      // A per-key colour queued moments ago would otherwise land after this
      // and repaint that one key, with the UI showing the fill colour.
      debounced.cancel("key:");
      await call("keyboard_fill_keys", { rgb });
      await get().refresh();
    },

    clearKeys: async () => {
      debounced.cancel("key:");
      await call("keyboard_clear_keys");
      await get().refresh();
    },

    setPhysical: async (physical) => {
      set((s) => ({ config: { ...s.config, physical } }));
      await call("keyboard_set_physical", { physical });
      await get().refresh();
    },

    setOled: async (mode, text) => {
      set((s) => ({ config: { ...s.config, oled_mode: mode, oled_text: text ?? s.config.oled_text } }));
      debounced("oled", () => call("keyboard_set_oled", { mode, text: text ?? null }));
    },

    setScreensaver: async (minutes) => {
      set((s) => ({ config: { ...s.config, oled_screensaver_minutes: minutes } }));
      debounced("screensaver", () => call("keyboard_set_screensaver", { minutes }));
    },

    setOledWire: async (wire) => {
      set((s) => ({ config: { ...s.config, oled_wire: wire } }));
      await call("keyboard_set_oled_wire", { wire });
    },

    testOled: async (wire) => {
      try {
        await call("keyboard_oled_test", { wire });
      } catch (e) {
        set({ error: String(e) });
      }
    },

    setActuation: async (tenths) => {
      set((s) => ({ config: { ...s.config, actuation: tenths } }));
      debounced("actuation", () => call("keyboard_set_actuation", { tenths }));
    },

    setKeyActuation: async (hid, tenths) => {
      set((s) => {
        const per_key_actuation = { ...s.config.per_key_actuation };
        if (tenths == null) delete per_key_actuation[hid];
        else per_key_actuation[hid] = tenths;
        return { config: { ...s.config, per_key_actuation } };
      });
      debounced(`act:${hid}`, () => call("keyboard_set_key_actuation", { hid, tenths }));
    },

    setRapidTrigger: async (sensitivity) => {
      set((s) => ({ config: { ...s.config, rapid_trigger: sensitivity } }));
      debounced("rapidtrigger", () => call("keyboard_set_rapid_trigger", { sensitivity }));
    },
    setProtectionMode: async (on) => {
      set((s) => ({ config: { ...s.config, protection_mode: on } }));
      try {
        await call("keyboard_set_protection_mode", { on });
      } catch (e) {
        set((s) => ({ config: { ...s.config, protection_mode: !on }, error: String(e) }));
      }
    },
    setRapidTap: async (on) => {
      set((s) => ({ config: { ...s.config, rapid_tap: on } }));
      try {
        await call("keyboard_set_rapid_tap", { on });
      } catch (e) {
        set((s) => ({ config: { ...s.config, rapid_tap: !on }, error: String(e) }));
      }
    },
    setIdle: async (seconds, brightness) => {
      set((s) => ({
        config: { ...s.config, idle_timeout_secs: seconds, idle_brightness: brightness },
      }));
      debounced("idle", () => call("keyboard_set_idle", { seconds, brightness }));
    },
    setPowerSaving: async (minutes, highEfficiency) => {
      set((s) => ({
        config: { ...s.config, sleep_minutes: minutes, high_efficiency: highEfficiency },
      }));
      debounced("power", () =>
        call("keyboard_set_power_saving", { minutes, highEfficiency }),
      );
    },

    sendRaw: async (cmd, payload) => {
      try {
        await call("keyboard_send_raw", { cmd, payload });
      } catch (e) {
        set({ error: String(e) });
      }
    },
  };
});
