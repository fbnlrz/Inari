/**
 * The single point where the app talks to the backend.
 *
 * Every command and every event goes through here so the transport is one
 * decision rather than 165. Inside the Tauri window that is the native IPC
 * bridge; a browser client (the tablet remote) swaps in a WebSocket without a
 * single call site changing.
 *
 * `Command` is not decoration: command names used to travel as bare strings,
 * sometimes through helpers, so a rename on the Rust side broke at runtime and
 * `tsc` never saw it. A Rust test pins this union to the actual
 * `generate_handler!` list, so drift fails the build instead of the app.
 */
import { convertFileSrc, invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";

/** Every command registered in src-tauri/src/lib.rs. Keep sorted. */
export type Command =
  | "add_bus"
  | "add_channel"
  | "apply_update"
  | "check_update"
  | "create_blank_profile"
  | "delete_profile"
  | "delete_user_eq_preset"
  | "export_channel_eq"
  | "export_channel_eq_to_file"
  | "forget_app"
  | "get_active_profile"
  | "get_app_streams"
  | "get_autostart"
  | "get_backend_info"
  | "get_channel_eq_configs"
  | "get_channel_failover"
  | "get_channel_outputs"
  | "get_default_devices"
  | "get_headset_status"
  | "get_hotkeys"
  | "get_input_devices"
  | "get_mic_config"
  | "get_mouse_status"
  | "get_output_devices"
  | "get_prefs"
  | "get_remote_pairing"
  | "get_remote_status"
  | "get_resolved_outputs"
  | "get_seen_apps"
  | "get_virtual_devices"
  | "headset_apply_eq_preset"
  | "headset_eq_presets"
  | "headset_get_alsa_headroom"
  | "headset_get_notify_display"
  | "headset_get_notify_mirror"
  | "headset_oled_auto"
  | "headset_oled_brightness"
  | "headset_oled_clip"
  | "headset_oled_clips"
  | "headset_oled_media"
  | "headset_oled_mode"
  | "headset_oled_modes"
  | "headset_oled_notify"
  | "headset_oled_now_playing"
  | "headset_oled_return_ui"
  | "headset_oled_rotate"
  | "headset_oled_status"
  | "headset_oled_system"
  | "headset_oled_text"
  | "headset_save"
  | "headset_set_alsa_headroom"
  | "headset_set_anc"
  | "headset_set_auto_off"
  | "headset_set_eq_bands"
  | "headset_set_eq_preset"
  | "headset_set_gain_high"
  | "headset_set_line_out"
  | "headset_set_line_out_volumes"
  | "headset_set_mic_led"
  | "headset_set_mic_volume"
  | "headset_set_notify_display"
  | "headset_set_notify_mirror"
  | "headset_set_sidetone"
  | "headset_set_transparency"
  | "headset_set_wireless_range"
  | "headset_timer_countdown"
  | "headset_timer_reset"
  | "headset_timer_stopwatch"
  | "headset_timer_toggle"
  | "import_eq_config"
  | "import_eq_file"
  | "init_virtual_devices"
  | "list_buses"
  | "list_eq_presets"
  | "list_profiles"
  | "load_profile"
  | "mouse_set_dim"
  | "mouse_set_dpi"
  | "mouse_set_polling"
  | "mouse_set_rainbow"
  | "mouse_set_reactive"
  | "mouse_set_sleep"
  | "mouse_set_startup_lighting"
  | "mouse_set_zone_color"
  | "open_log_dir"
  | "open_url"
  | "regenerate_remote_token"
  | "remove_bus"
  | "remove_channel"
  | "rename_app"
  | "rename_bus"
  | "rename_channel"
  | "reorder_channels"
  | "reset_app"
  | "restart_app"
  | "route_app_to_channel"
  | "save_user_eq_preset"
  | "set_app_assignment"
  | "set_app_ignored"
  | "set_app_volume"
  | "set_autostart"
  | "set_balance_channels"
  | "set_balance_visible"
  | "set_bus_exclude"
  | "set_bus_members"
  | "set_bus_mute"
  | "set_bus_volume"
  | "set_channel_eq"
  | "set_channel_failover"
  | "set_channel_icon"
  | "set_channel_output"
  | "set_channel_volume"
  | "set_default_input"
  | "set_default_output"
  | "set_device_label_style"
  | "set_hotkey"
  | "set_hotkey_channel"
  | "set_mic_config"
  | "set_monitor"
  | "set_onboarded"
  | "set_profile_trigger"
  | "set_remote_bind"
  | "set_remote_enabled"
  | "set_start_minimized"
  | "teardown_virtual_devices"
  | "toggle_channel_mute";

/** Every event the backend emits. */
export type EventName =
  | "graph-changed"
  | "headset-chatmix"
  | "headset-presence"
  | "headset-status"
  | "levels"
  | "mouse-presence"
  | "mouse-status"
  | "profile-changed"
  | "state-changed";

export type Args = Record<string, unknown>;
export type Unsubscribe = () => void;

export interface Transport {
  call<T>(cmd: Command, args?: Args): Promise<T>;
  /** Resolves once subscribed; call the result to stop listening. */
  subscribe<T>(event: EventName, onPayload: (payload: T) => void): Promise<Unsubscribe>;
  /**
   * Browser-loadable URL for a file on the host machine (app icons are the
   * only use). The asset protocol resolves only inside the Tauri webview, so
   * the remote client has to fetch the same file over HTTP instead.
   */
  assetUrl(path: string): string;
}

/**
 * Tauri IPC. Note `subscribe` hands the callback the payload directly rather
 * than Tauri's event wrapper — a network transport has no such wrapper, and
 * call sites only ever wanted the payload.
 */
export const tauriTransport: Transport = {
  call: <T,>(cmd: Command, args?: Args) => tauriInvoke<T>(cmd, args),
  subscribe: <T,>(event: EventName, onPayload: (payload: T) => void) =>
    tauriListen<T>(event, (e) => onPayload(e.payload)),
  assetUrl: (path: string) => convertFileSrc(path),
};

let active: Transport = tauriTransport;

/** Swap the transport (the remote client installs a WebSocket one). */
export function setTransport(t: Transport): void {
  active = t;
}

export function call<T>(cmd: Command, args?: Args): Promise<T> {
  return active.call<T>(cmd, args);
}

export function subscribe<T>(
  event: EventName,
  onPayload: (payload: T) => void,
): Promise<Unsubscribe> {
  return active.subscribe<T>(event, onPayload);
}

export function assetUrl(path: string): string {
  return active.assetUrl(path);
}
