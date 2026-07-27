import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useMixerStore, type Levels } from "../store/mixer";

/**
 * Refresh cadence without `graph-changed` events: the pactl fallback has no
 * PipeWire listeners, so there is nothing to ride and the timer stays the
 * primary path.
 */
const POLL_INTERVAL_MS = 2000;
/**
 * Backstop for the event-driven path. NOT the primary refresh - the backend's
 * coalesced `graph-changed` event is. This only exists so a dropped event
 * can't leave the mixer permanently stale.
 */
const BACKSTOP_INTERVAL_MS = 15000;
/** App history is not live state; it may lag the graph by a minute. */
const SEEN_INTERVAL_MS = 60000;

/**
 * Boots the audio layer: creates the virtual sinks on mount, keeps the app
 * stream list + device list in sync (also the auto-route enforcement
 * trigger), subscribes to live VU level events, and auto-loads profiles
 * bound to newly connected devices (Phase 5).
 */
export function useAudio() {
  const initialize = useMixerStore((s) => s.initialize);
  const fetchAppStreams = useMixerStore((s) => s.fetchAppStreams);
  const fetchOutputs = useMixerStore((s) => s.fetchOutputs);
  const fetchSeenApps = useMixerStore((s) => s.fetchSeenApps);
  const setLevels = useMixerStore((s) => s.setLevels);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const profiles = useMixerStore((s) => s.profiles);
  const loadProfile = useMixerStore((s) => s.loadProfile);
  // Only the native backend emits `graph-changed`; until we know which one we
  // got (null), keep the fast timer - never slower than the old behaviour.
  const eventDriven = useMixerStore((s) => s.backendNative) === true;

  useEffect(() => {
    void initialize();
  }, [initialize]);

  useEffect(() => {
    let mixerTimer: ReturnType<typeof setInterval> | undefined;
    let seenTimer: ReturnType<typeof setInterval> | undefined;
    const refreshMixer = () => Promise.all([fetchAppStreams(), fetchOutputs()]);
    const refreshSeen = () => void fetchSeenApps();
    const start = () => {
      if (mixerTimer !== undefined) return;
      // A returning window must not show stale data.
      void refreshMixer();
      refreshSeen();
      mixerTimer = setInterval(
        () => void refreshMixer(),
        eventDriven ? BACKSTOP_INTERVAL_MS : POLL_INTERVAL_MS,
      );
      seenTimer = setInterval(refreshSeen, SEEN_INTERVAL_MS);
    };
    const stop = () => {
      clearInterval(mixerTimer);
      clearInterval(seenTimer);
      mixerTimer = undefined;
      seenTimer = undefined;
    };
    // Stay quiet while hidden in the tray - the product's dominant idle state
    // - instead of round-tripping forever (TD-009).
    const onVisibility = () => (document.hidden ? stop() : start());
    if (!document.hidden) start();
    document.addEventListener("visibilitychange", onVisibility);

    // The primary path: the backend tells us when the PipeWire graph actually
    // moved (node added/removed/renamed, default device switched), already
    // coalesced, so a startup burst is one refetch and not dozens.
    const unlisten = listen("graph-changed", () => {
      if (document.hidden) return; // start() refreshes when the window returns
      // History is written backend-side while the stream list is served, so
      // read it back after that call rather than racing it.
      void refreshMixer().then(refreshSeen);
    });

    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
      void unlisten.then((fn) => fn());
    };
  }, [fetchAppStreams, fetchOutputs, fetchSeenApps, eventDriven]);

  useEffect(() => {
    const unlisten = listen<Levels>("levels", (event) => setLevels(event.payload));
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [setLevels]);

  // Profile switched from the tray menu - sync the whole UI.
  const onProfileChanged = useMixerStore((s) => s.onProfileChanged);
  useEffect(() => {
    const unlisten = listen<string>("profile-changed", (event) => {
      void onProfileChanged(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [onProfileChanged]);

  // Hardware profile auto-switch: when a device with a bound profile
  // appears, load that profile (Sonar-style).
  const seenDevices = useRef<Set<string> | null>(null);
  useEffect(() => {
    const names = new Set(outputDevices.map((d) => d.name));
    if (seenDevices.current === null) {
      // First sample: just learn the current device set.
      if (names.size > 0) seenDevices.current = names;
      return;
    }
    for (const name of names) {
      if (!seenDevices.current.has(name)) {
        const bound = profiles.find((p) => p.trigger_device === name);
        if (bound) void loadProfile(bound.name);
      }
    }
    seenDevices.current = names;
  }, [outputDevices, profiles, loadProfile]);
}
