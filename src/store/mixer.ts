import { create } from "zustand";
import { call, subscribe, type Command } from "../lib/ipc";
import { createDebouncer } from "../lib/debounce";
import { isTauri } from "../lib/platform";
import type {
  AppStream,
  BusDef,
  EqConfig,
  MicConfig,
  OutputDevice,
  ProfileInfo,
  SeenApp,
  VirtualSink,
} from "../types";

// Faders fire on every pointer move; debounce backend calls per target so a
// drag doesn't spawn a pactl subprocess per pixel. UI state updates
// optimistically and immediately. Every call site passes its own handler,
// since most also need to re-sync the state the failed write was guessing at.
const debounce = createDebouncer(90, (_key, e) =>
  useMixerStore.setState({ error: String(e) }),
);
function debouncedInvoke(key: string, cmd: Command, args: Record<string, unknown>, onError: (e: unknown) => void) {
  debounce(key, () => call(cmd, args), { onError });
}

/** Per-sink [left, right] peak amplitudes (0-1), streamed from the native backend. */
export type Levels = Record<string, [number, number]>;

/**
 * Channel and mic state can move without the UI asking: the tray's mute rows,
 * a global hotkey, `inari mute chat` from a terminal. The backend emits
 * `state-changed` after each of those; re-read the two stores they touch.
 *
 * Subscribed from `initialize` (once per session) rather than at module scope
 * so importing the store never depends on a Tauri runtime being present.
 */
let externalSubscribed = false;
function subscribeExternalChanges() {
  if (externalSubscribed) return;
  externalSubscribed = true;
  void subscribe("state-changed", () => {
    const s = useMixerStore.getState();
    void s.fetchChannels();
    void s.fetchMic();
  }).catch(() => {
    // No event bridge (tests, or a webview that failed to boot): the poll and
    // the next screen change still refresh everything.
    externalSubscribed = false;
  });
}

interface MixerStore {
  channels: VirtualSink[];
  appStreams: AppStream[];
  /** Live VU levels; stays empty under the pactl fallback backend. */
  levels: Levels;
  setLevels: (levels: Levels) => void;
  /** Physical output devices. */
  outputDevices: OutputDevice[];
  /** Channel -> chosen output node name (null = follow system default). */
  channelOutputs: Record<string, string | null>;
  /**
   * Channel -> the device node name it is actually routed to right now (after
   * default/fallback resolution). Lets a follow-default strip show where its
   * audio really goes, and reflects failover. Empty on the pactl fallback.
   */
  resolvedOutputs: Record<string, string | null>;
  /**
   * Channel -> whether it fails over to another device when its chosen device
   * (or the default) is gone. Off = play only on the chosen device / exact
   * default, silence otherwise. Defaults to on (absent treated as true).
   */
  channelFailover: Record<string, boolean>;
  fetchOutputs: () => Promise<void>;
  setChannelOutput: (sinkName: string, outputName: string | null) => Promise<void>;
  setChannelFailover: (sinkName: string, enabled: boolean) => Promise<void>;
  /** Sonar-style "same device on all channels". */
  setAllOutputs: (outputName: string | null) => Promise<void>;
  /** Channel -> parametric EQ (absent = never configured, i.e. default). */
  eqConfigs: Record<string, EqConfig>;
  fetchEq: () => Promise<void>;
  setChannelEq: (sinkName: string, config: EqConfig) => Promise<void>;
  /** Mic chain (Phase 3). Null until loaded. */
  micConfig: MicConfig | null;
  inputDevices: OutputDevice[];
  fetchMic: () => Promise<void>;
  setMicConfig: (patch: Partial<MicConfig>) => Promise<void>;
  profiles: ProfileInfo[];
  /** Bind/clear an output device that auto-loads a profile (Phase 5). */
  setProfileTrigger: (name: string, device: string | null) => Promise<void>;
  /** Create a clean-slate profile (saved, not applied). */
  createBlankProfile: (name: string) => Promise<void>;
  /** A profile was switched outside the UI (tray) - sync everything. */
  onProfileChanged: (name: string) => Promise<void>;
  /** App history (live + gone + ignored). */
  seenApps: SeenApp[];
  fetchSeenApps: () => Promise<void>;
  setAppIgnored: (app: { match_prop: string; match_value: string }, ignored: boolean) => Promise<void>;
  forgetApp: (app: { match_prop: string; match_value: string }) => Promise<void>;
  /** Pre-route an app that isn't currently running (null clears). */
  setAppAssignment: (
    app: { match_prop: string; match_value: string },
    sinkName: string | null,
  ) => Promise<void>;
  /** Channel management: labels are free-form, sink names are stable. */
  addChannel: (label: string, icon: string | null) => Promise<void>;
  renameChannel: (sinkName: string, label: string) => Promise<void>;
  removeChannel: (sinkName: string) => Promise<void>;
  /** Visual-only reorder while dragging a strip. */
  moveChannel: (from: string, to: string) => void;
  /** Persist the current strip order (called on drag end). */
  commitChannelOrder: () => Promise<void>;
  setChannelIcon: (sinkName: string, icon: string) => Promise<void>;
  /** User-defined mixes (record buses). */
  buses: BusDef[];
  fetchBuses: () => Promise<void>;
  addBus: (label: string) => Promise<void>;
  renameBus: (name: string, label: string) => Promise<void>;
  removeBus: (name: string) => Promise<void>;
  setBusMembers: (name: string, channels: string[]) => Promise<void>;
  /** Manual vs auto-include mode (carried set preserved). */
  setBusExclude: (name: string, exclude: boolean) => Promise<void>;
  /** A mix's playback level for recorders (0-150%); persisted. */
  setBusVolume: (name: string, volume: number) => Promise<void>;
  /** Mute a mix for recorders; persisted. */
  setBusMute: (name: string, muted: boolean) => Promise<void>;
  /** Session-scoped "listen on default output" toggles per node. */
  monitors: Record<string, boolean>;
  toggleMonitor: (name: string) => Promise<void>;
  /** Name of the most recently saved/loaded profile this session. */
  activeProfile: string | null;
  /** Fatal error surfaced to the UI (e.g. pactl missing, PipeWire down). */
  error: string | null;
  /** Dismiss the error banner. */
  clearError: () => void;
  initialized: boolean;
  /** True on the native PipeWire backend; false on the pactl fallback
   * (mixes/mic/monitoring unavailable). Null until known. */
  backendNative: boolean | null;
  /** False once the PipeWire loop thread has died. Distinct from the pactl
   * fallback: that one still works, a stopped engine means no audio control
   * until a restart. */
  engineAlive: boolean;
  /** Re-read the engine's liveness. Cheap, so any failed command can ask. */
  checkEngine: () => Promise<void>;
  /** First-run tutorial visible. */
  showOnboarding: boolean;
  /** True when the tutorial was reopened from Settings (no setup choice). */
  onboardingReplay: boolean;
  /** Close the tutorial; blank = collapse to a single starter channel. */
  finishOnboarding: (blank: boolean) => Promise<void>;
  /** Reopen the tutorial (view-only - no starting-point choice). */
  replayOnboarding: () => void;
  /** Balance slider channel picks (null = auto Game/Chat or first two). */
  balanceA: string | null;
  balanceB: string | null;
  setBalanceChannels: (a: string | null, b: string | null) => Promise<void>;
  showBalance: boolean;
  setBalanceVisible: (visible: boolean) => Promise<void>;

  /** Create the virtual sinks and load initial state. */
  initialize: () => Promise<void>;
  fetchChannels: () => Promise<void>;
  fetchAppStreams: () => Promise<void>;
  setChannelVolume: (sinkName: string, volume: number) => Promise<void>;
  /** Drive the balance channels from the headset's hardware ChatMix wheel. */
  applyChatMix: (game: number, chat: number) => void;
  toggleMute: (sinkName: string, muted: boolean) => Promise<void>;
  routeApp: (streamIndex: number, sinkName: string) => Promise<void>;
  setAppVolume: (streamIndex: number, volume: number) => Promise<void>;
  fetchProfiles: () => Promise<void>;
  loadProfile: (name: string) => Promise<void>;
  deleteProfile: (name: string) => Promise<void>;
  /** Set or clear (empty string) a persistent display name for an app. */
  renameApp: (stream: AppStream, alias: string) => Promise<void>;
}

/** Structural equality via JSON, to skip no-op store writes on each poll and
 *  avoid re-rendering the whole board when nothing changed (TD-029). */
const jsonEqual = (a: unknown, b: unknown): boolean =>
  JSON.stringify(a) === JSON.stringify(b);

/**
 * Channels whose volume this store has changed but not yet written.
 *
 * `state-changed` fires for plenty of things that are not this fader — a
 * hotkey, the tray, the CLI, a tablet, even showing the window — and each one
 * makes the store refetch. The backend still holds the pre-write value during
 * the 90 ms debounce, so the refetch handed the fader its old position back
 * while the write went out with the new one. The sink ended up where the user
 * put it and the strip showed something else, permanently, because nothing
 * refetches again afterwards.
 *
 * Kept outside the store: it is bookkeeping for reconciliation, not state
 * anything renders.
 */
const pendingVolume = new Set<string>();

export const useMixerStore = create<MixerStore>((set, get) => ({
  channels: [],
  appStreams: [],
  levels: {},
  setLevels: (levels) => {
    // Levels arrive at 10 Hz even when everything is silent; skipping
    // no-op updates avoids re-rendering every strip 10×/second at idle.
    const prev = get().levels;
    const keys = Object.keys(levels);
    const unchanged =
      keys.length === Object.keys(prev).length &&
      keys.every((k) => {
        const a = prev[k];
        const b = levels[k];
        return a && Math.abs(a[0] - b[0]) < 1e-4 && Math.abs(a[1] - b[1]) < 1e-4;
      });
    if (!unchanged) set({ levels });
  },
  outputDevices: [],
  channelOutputs: {},
  resolvedOutputs: {},
  channelFailover: {},
  micConfig: null,
  inputDevices: [],
  seenApps: [],
  profiles: [],
  activeProfile: null,
  error: null,
  clearError: () => set({ error: null }),
  initialized: false,
  backendNative: null,
  engineAlive: true,
  checkEngine: async () => {
    try {
      const i = await call<{ native: boolean; engine_alive: boolean }>("get_backend_info");
      set({ backendNative: i.native, engineAlive: i.engine_alive });
    } catch {
      // The check itself failing tells us nothing new; leave the last state.
    }
  },
  showOnboarding: false,
  onboardingReplay: false,

  replayOnboarding: () => set({ showOnboarding: true, onboardingReplay: true }),

  balanceA: null,
  balanceB: null,
  showBalance: true,

  setBalanceChannels: async (a, b) => {
    set({ balanceA: a, balanceB: b });
    try {
      await call("set_balance_channels", { a, b });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setBalanceVisible: async (visible) => {
    set({ showBalance: visible });
    try {
      await call("set_balance_visible", { visible });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  finishOnboarding: async (blank) => {
    const replay = get().onboardingReplay;
    set({ showOnboarding: false, onboardingReplay: false });
    if (replay) return; // view-only: nothing to persist or change
    try {
      await call("set_onboarded");
      if (blank) {
        // Collapse the seeded defaults to a single starter channel; the
        // active profile autosaves the result.
        const channels = get().channels;
        for (const c of channels.slice(1)) {
          await get().removeChannel(c.name);
        }
        if (channels.length > 0) {
          await get().renameChannel(channels[0].name, "Main");
          await get().setChannelIcon(channels[0].name, "graphic_eq");
        }
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  initialize: async () => {
    if (get().initialized) return;
    try {
      // Creating the virtual devices is the desktop instance's job, and the
      // remote is not allowed to do it — tearing the audio chain up or down
      // from a tablet is exactly what the allowlist exists to prevent. A
      // remote client only ever attaches to an instance that is already
      // running, so the devices are there by the time it connects.
      if (isTauri) await call("init_virtual_devices");
      set({ initialized: true, error: null });
      subscribeExternalChanges();
      // Leaving backendNative null on failure would gate native-only features
      // on an unknown, so say it failed instead of swallowing it.
      void call<{ native: boolean; engine_alive: boolean }>("get_backend_info")
        .then((i) => set({ backendNative: i.native, engineAlive: i.engine_alive }))
        .catch((e: unknown) => set({ error: String(e) }));
      void call<{
        onboarded: boolean;
        balance_a: string | null;
        balance_b: string | null;
        show_balance: boolean;
      }>("get_prefs")
        .then((p) => {
          set({ balanceA: p.balance_a, balanceB: p.balance_b, showBalance: p.show_balance });
          if (!p.onboarded) set({ showOnboarding: true });
        })
        .catch((e: unknown) => set({ error: String(e) }));
      await Promise.all([
        get().fetchChannels(),
        get().fetchAppStreams(),
        get().fetchProfiles(),
        get().fetchOutputs(),
        get().fetchEq(),
        get().fetchMic(),
        get().fetchBuses(),
      ]);
      // Active profile is tracked backend-side (survives restarts).
      try {
        const active = await call<string | null>("get_active_profile");
        if (active) {
          set({ activeProfile: active });
        } else if (get().profiles.some((p) => p.name === "Default")) {
          // First run: the backend just created "Default" from this layout.
          set({ activeProfile: "Default" });
        }
      } catch {
        /* older backend without the command - banner-worthy errors surface elsewhere */
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  fetchChannels: async () => {
    try {
      const channels = await call<VirtualSink[]>("get_virtual_devices");
      // A channel with a write in flight keeps what the user set; everything
      // else takes the backend's answer.
      set((s) => ({
        channels: channels.map((c) => {
          if (!pendingVolume.has(c.name)) return c;
          const local = s.channels.find((x) => x.name === c.name);
          return local ? { ...c, volume_percent: local.volume_percent } : c;
        }),
      }));
    } catch (e) {
      set({ error: String(e) });
    }
  },

  fetchAppStreams: async () => {
    try {
      const appStreams = await call<AppStream[]>("get_app_streams");
      const s = get();
      const patch: Partial<MixerStore> = {};
      if (!jsonEqual(s.appStreams, appStreams)) patch.appStreams = appStreams;
      if (s.error !== null) patch.error = null;
      if (Object.keys(patch).length) set(patch);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setChannelVolume: async (sinkName, volume) => {
    set((s) => ({
      channels: s.channels.map((c) =>
        c.name === sinkName ? { ...c, volume_percent: volume } : c,
      ),
    }));
    pendingVolume.add(sinkName);
    debounce(
      `chvol:${sinkName}`,
      () =>
        call("set_channel_volume", { sinkName, volume }).finally(() => {
          pendingVolume.delete(sinkName);
        }),
      {
        onError: (e) => {
          set({ error: String(e) });
          void get().fetchChannels();
        },
      },
    );
  },

  applyChatMix: (game, chat) => {
    const s = get();
    const find = (name: string | null) =>
      s.channels.find((c) => c.name === name) ?? null;
    // Same resolution as BalanceBar: saved picks, else Game/Chat, else first two.
    const a = find(s.balanceA) ?? find("sink_game") ?? s.channels[0] ?? null;
    const b =
      find(s.balanceB) ??
      find("sink_chat") ??
      s.channels.find((c) => c.name !== a?.name) ??
      null;
    if (!a || !b || a.name === b.name) return;
    const clamp = (v: number) => Math.max(0, Math.min(100, Math.round(v)));
    void s.setChannelVolume(a.name, clamp(game));
    void s.setChannelVolume(b.name, clamp(chat));
  },

  toggleMute: async (sinkName, muted) => {
    set((s) => ({
      channels: s.channels.map((c) =>
        c.name === sinkName ? { ...c, muted } : c,
      ),
    }));
    try {
      await call("toggle_channel_mute", { sinkName, muted });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  routeApp: async (streamIndex, sinkName) => {
    set((s) => ({
      appStreams: s.appStreams.map((a) =>
        a.index === streamIndex
          ? { ...a, assigned_sink: sinkName === "" ? null : sinkName }
          : a,
      ),
    }));
    try {
      await call("route_app_to_channel", { streamIndex, sinkName });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      await get().fetchAppStreams();
    }
  },

  setAppVolume: async (streamIndex, volume) => {
    set((s) => ({
      appStreams: s.appStreams.map((a) =>
        a.index === streamIndex ? { ...a, volume_percent: volume } : a,
      ),
    }));
    debouncedInvoke(
      `appvol:${streamIndex}`,
      "set_app_volume",
      { streamIndex, volume },
      (e) => set({ error: String(e) }),
    );
  },

  fetchOutputs: async () => {
    try {
      const [outputDevices, channelOutputs, resolvedOutputs, channelFailover] = await Promise.all([
        call<OutputDevice[]>("get_output_devices"),
        call<Record<string, string | null>>("get_channel_outputs"),
        call<Record<string, string | null>>("get_resolved_outputs"),
        call<Record<string, boolean>>("get_channel_failover"),
      ]);
      const s = get();
      const patch: Partial<MixerStore> = {};
      if (!jsonEqual(s.outputDevices, outputDevices)) patch.outputDevices = outputDevices;
      if (!jsonEqual(s.channelOutputs, channelOutputs)) patch.channelOutputs = channelOutputs;
      if (!jsonEqual(s.resolvedOutputs, resolvedOutputs)) patch.resolvedOutputs = resolvedOutputs;
      if (!jsonEqual(s.channelFailover, channelFailover)) patch.channelFailover = channelFailover;
      if (Object.keys(patch).length) set(patch);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setChannelOutput: async (sinkName, outputName) => {
    set((s) => ({
      channelOutputs: { ...s.channelOutputs, [sinkName]: outputName },
    }));
    try {
      await call("set_channel_output", { sinkName, outputName: outputName ?? "" });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchOutputs();
    }
  },

  setChannelFailover: async (sinkName, enabled) => {
    set((s) => ({
      channelFailover: { ...s.channelFailover, [sinkName]: enabled },
    }));
    try {
      await call("set_channel_failover", { sinkName, enabled });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchOutputs();
    }
  },

  setAllOutputs: async (outputName) => {
    for (const channel of get().channels) {
      await get().setChannelOutput(channel.name, outputName);
    }
  },

  eqConfigs: {},

  fetchEq: async () => {
    try {
      const eqConfigs = await call<Record<string, EqConfig>>("get_channel_eq_configs");
      if (!jsonEqual(get().eqConfigs, eqConfigs)) set({ eqConfigs });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setChannelEq: async (sinkName, config) => {
    set({ eqConfigs: { ...get().eqConfigs, [sinkName]: config } });
    // Debounced per channel: a band drag settles into one apply, and two
    // open EQ panels never clobber each other's pending call.
    debouncedInvoke(`eq:${sinkName}`, "set_channel_eq", { sinkName, config }, (e) => {
      set({ error: String(e) });
      void get().fetchEq();
    });
  },

  fetchMic: async () => {
    try {
      const [micConfig, inputDevices] = await Promise.all([
        call<MicConfig>("get_mic_config"),
        call<OutputDevice[]>("get_input_devices"),
      ]);
      set({ micConfig, inputDevices });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setMicConfig: async (patch) => {
    const current = get().micConfig;
    if (!current) return;
    set({ micConfig: { ...current, ...patch } });
    // Debounced: slider drags and rename typing settle into one apply.
    //
    // The payload is built when the timer fires, not when the call is made.
    // `set_mic_config` writes the whole struct, so freezing it here meant any
    // sibling field changed from outside during those 90 ms was rolled back by
    // this write: press the mic-mute hotkey mid-drag and the microphone
    // reopened while the UI, the tray and the status label all still said
    // "Muted", with nothing left to correct it.
    debounce(
      "micConfig",
      () => {
        const config = get().micConfig;
        return config ? call("set_mic_config", { config }) : Promise.resolve();
      },
      {
        onError: (e) => {
          set({ error: String(e) });
          void get().fetchMic();
        },
      },
    );
  },

  fetchProfiles: async () => {
    try {
      const profiles = await call<ProfileInfo[]>("list_profiles");
      set({ profiles });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setProfileTrigger: async (name, device) => {
    try {
      await call("set_profile_trigger", { name, device: device ?? "" });
      await get().fetchProfiles();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  onProfileChanged: async (name) => {
    set({ activeProfile: name });
    await Promise.all([
      get().fetchChannels(),
      get().fetchAppStreams(),
      get().fetchOutputs(),
      get().fetchEq(),
      get().fetchSeenApps(),
      get().fetchProfiles(),
      get().fetchBuses(),
    ]);
  },

  createBlankProfile: async (name) => {
    try {
      await call("create_blank_profile", { name });
      await get().fetchProfiles();
      // Switch to the fresh profile right away - creating a blank slate
      // and not seeing anything change reads as a bug.
      await get().loadProfile(name);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  fetchSeenApps: async () => {
    try {
      const seenApps = await call<SeenApp[]>("get_seen_apps");
      if (!jsonEqual(get().seenApps, seenApps)) set({ seenApps });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setAppIgnored: async (app, ignored) => {
    try {
      await call("set_app_ignored", {
        matchProp: app.match_prop,
        matchValue: app.match_value,
        ignored,
      });
      await Promise.all([get().fetchSeenApps(), get().fetchAppStreams()]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  forgetApp: async (app) => {
    try {
      await call("forget_app", {
        matchProp: app.match_prop,
        matchValue: app.match_value,
      });
      await get().fetchSeenApps();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setAppAssignment: async (app, sinkName) => {
    try {
      await call("set_app_assignment", {
        matchProp: app.match_prop,
        matchValue: app.match_value,
        sinkName: sinkName ?? "",
      });
      await get().fetchSeenApps();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  loadProfile: async (name) => {
    try {
      await call("load_profile", { name });
      set({ activeProfile: name });
      // Layout, volumes and routing all changed backend-side.
      await Promise.all([
        get().fetchChannels(),
        get().fetchAppStreams(),
        get().fetchOutputs(),
        get().fetchEq(),
        get().fetchSeenApps(),
        get().fetchBuses(),
      ]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  deleteProfile: async (name) => {
    try {
      await call("delete_profile", { name });
      if (get().activeProfile === name) set({ activeProfile: null });
      await get().fetchProfiles();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  addChannel: async (label, icon) => {
    try {
      await call("add_channel", { label, icon });
      // Buses too: the master (and auto-include mixes) absorb the channel.
      await Promise.all([get().fetchChannels(), get().fetchOutputs(), get().fetchBuses()]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  buses: [],

  fetchBuses: async () => {
    try {
      const buses = await call<BusDef[]>("list_buses");
      set({ buses });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  addBus: async (label) => {
    try {
      await call("add_bus", { label });
      await get().fetchBuses();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  renameBus: async (name, label) => {
    set((s) => ({
      buses: s.buses.map((b) => (b.name === name ? { ...b, label } : b)),
    }));
    try {
      await call("rename_bus", { name, label });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },

  removeBus: async (name) => {
    try {
      await call("remove_bus", { name });
      await get().fetchBuses();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setBusMembers: async (name, channels) => {
    // `channels` is the carried set; auto-include mixes store the
    // complement (mirrors the backend's conversion).
    const all = get().channels.map((c) => c.name);
    set((s) => ({
      buses: s.buses.map((b) =>
        b.name === name
          ? { ...b, channels: b.exclude ? all.filter((c) => !channels.includes(c)) : channels }
          : b,
      ),
    }));
    try {
      await call("set_bus_members", { name, channels });
      // The backend converts against its own channel set - sync up so the
      // stored complement can't drift if channels changed mid-flight.
      await get().fetchBuses();
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },

  setBusExclude: async (name, exclude) => {
    const all = get().channels.map((c) => c.name);
    set((s) => ({
      buses: s.buses.map((b) => {
        if (b.name !== name || b.exclude === exclude) return b;
        // Preserve the carried set; only the stored representation flips.
        const carried = b.exclude
          ? all.filter((c) => !b.channels.includes(c))
          : b.channels;
        return {
          ...b,
          exclude,
          channels: exclude ? all.filter((c) => !carried.includes(c)) : carried,
        };
      }),
    }));
    try {
      await call("set_bus_exclude", { name, exclude });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },

  setBusVolume: async (name, volume) => {
    set((s) => ({
      buses: s.buses.map((b) => (b.name === name ? { ...b, volume_percent: volume } : b)),
    }));
    debouncedInvoke(`busvol:${name}`, "set_bus_volume", { name, volume }, (e) => {
      set({ error: String(e) });
      void get().fetchBuses();
    });
  },

  setBusMute: async (name, muted) => {
    set((s) => ({
      buses: s.buses.map((b) => (b.name === name ? { ...b, muted } : b)),
    }));
    try {
      await call("set_bus_mute", { name, muted });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },

  monitors: {},

  toggleMonitor: async (name) => {
    const enabled = !get().monitors[name];
    set((s) => ({ monitors: { ...s.monitors, [name]: enabled } }));
    try {
      await call("set_monitor", { sinkName: name, enabled });
    } catch (e) {
      set({ error: String(e) });
      set((s) => ({ monitors: { ...s.monitors, [name]: !enabled } }));
    }
  },

  setChannelIcon: async (sinkName, icon) => {
    set((s) => ({
      channels: s.channels.map((c) => (c.name === sinkName ? { ...c, icon } : c)),
    }));
    try {
      await call("set_channel_icon", { sinkName, icon });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  renameChannel: async (sinkName, label) => {
    set((s) => ({
      channels: s.channels.map((c) => (c.name === sinkName ? { ...c, label } : c)),
    }));
    try {
      await call("rename_channel", { sinkName, label });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  // Visual-only move while dragging; commitChannelOrder persists on drop.
  moveChannel: (from, to) => {
    set((s) => {
      const arr = [...s.channels];
      const fi = arr.findIndex((c) => c.name === from);
      const ti = arr.findIndex((c) => c.name === to);
      if (fi < 0 || ti < 0 || fi === ti) return {};
      const [moved] = arr.splice(fi, 1);
      arr.splice(ti, 0, moved);
      return { channels: arr };
    });
  },

  commitChannelOrder: async () => {
    const order = get().channels.map((c) => c.name);
    try {
      await call("reorder_channels", { order });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  removeChannel: async (sinkName) => {
    try {
      await call("remove_channel", { sinkName });
      await Promise.all([
        get().fetchChannels(),
        get().fetchAppStreams(),
        get().fetchOutputs(),
        get().fetchEq(), // the channel's EQ entry is gone too
        get().fetchBuses(), // memberships dropped the channel
      ]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  renameApp: async (stream, alias) => {
    const trimmed = alias.trim();
    set((s) => ({
      appStreams: s.appStreams.map((a) =>
        a.match_prop === stream.match_prop && a.match_value === stream.match_value
          ? { ...a, alias: trimmed === "" ? null : trimmed }
          : a,
      ),
    }));
    try {
      await call("rename_app", {
        matchProp: stream.match_prop,
        matchValue: stream.match_value,
        alias: trimmed,
      });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchAppStreams();
    }
  },
}));
