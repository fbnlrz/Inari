mod audio;
mod commands;
mod device;
mod error;
mod headset;
mod mixer;
mod mouse;
mod persistence;
mod remote;
mod state;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use log::{error, info, warn};
use tauri::menu::{CheckMenuItem, Menu, MenuItem}; // CheckMenuItem: profile + mute rows
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};

use audio::backend::AudioBackend;
use audio::pactl::PactlBackend;
use audio::pw_native::levels::LevelStore;
use audio::pw_native::{GraphCoalescer, GraphNotify, PipeWireBackend, GRAPH_MAX_WAIT, GRAPH_QUIET};
use state::AppState;

/// Global level for the logger. Info by default: lifecycle events only, since
/// the OLED draw loop (25 Hz) and the audio poll log at debug/trace and would
/// otherwise churn through the rotating file in minutes. `RUST_LOG=debug`
/// raises it for a bug report.
fn log_level() -> log::LevelFilter {
    let Ok(spec) = std::env::var("RUST_LOG") else {
        return log::LevelFilter::Info;
    };
    // Accept a bare level ("debug") as well as env_logger-style directives
    // ("inari_lib=debug", "info,inari_lib=trace"); the last level named wins.
    // We deliberately don't implement per-module filtering here - the plugin's
    // `level_for` is compile-time and nobody has asked for it.
    spec.rsplit([',', '='])
        .find_map(|part| part.trim().parse().ok())
        .unwrap_or(log::LevelFilter::Info)
}

/// The logger: stderr for `cargo tauri dev` and terminal launches, plus a
/// small rotating file so a user who started Inari from the app menu or the
/// systemd autostart unit still has something to attach to a bug report.
fn log_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri_plugin_log::{Target, TargetKind, TimezoneStrategy};

    tauri_plugin_log::Builder::new()
        .level(log_level())
        // The webview/windowing stack is chatty at debug and none of it is
        // about us; it would drown the app's own lines under `RUST_LOG=debug`.
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn)
        .clear_targets()
        .targets([
            Target::new(TargetKind::Stderr),
            Target::new(TargetKind::LogDir {
                file_name: Some("inari".into()),
            }),
        ])
        // Desktop app, not a server: one capped file (the default rotation
        // strategy drops the previous one), so sitting in the tray for a week
        // can't grow the log without bound.
        .max_file_size(512 * 1024)
        // Timestamps a user can line up with "it broke around 21:40".
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .build()
}

/// The chosen backend plus its two native-only side channels: live metering
/// and graph-change notifications, both `None` on the pactl fallback.
type BackendSetup = (
    Arc<dyn AudioBackend>,
    Option<Arc<LevelStore>>,
    Option<Arc<GraphNotify>>,
);

/// Answer a command line that needs no app at all (`--help`, a typo'd
/// subcommand), returning the exit code. `None` means "go on and run".
/// Lives here so `main` can reply without paying for a whole Tauri boot.
pub fn cli_early_exit() -> Option<i32> {
    commands::cli::early_exit(&std::env::args().skip(1).collect::<Vec<_>>())
}

/// The arguments this process was started with, minus argv[0].
fn own_args() -> Vec<String> {
    std::env::args().skip(1).collect()
}

/// State was changed from outside the webview (tray, hotkey, CLI): refresh
/// the tray's check marks and let an open window re-read what moved.
pub(crate) fn after_external_change(app: &tauri::AppHandle) {
    refresh_tray(app);
    let _ = app.emit("state-changed", ());
}

pub fn run() {
    // Prefer the native PipeWire backend (Phase 2); fall back to pactl
    // subprocess calls if the native loop can't come up. Levels (real VU
    // metering) are native-only. This runs before the logger exists, so the
    // outcome is logged from `setup` below instead.
    let mut backend_fallback: Option<String> = None;
    let (backend, levels, graph): BackendSetup = match PipeWireBackend::new() {
        Ok(backend) => {
            let levels = backend.levels.clone();
            let graph = backend.graph.clone();
            (Arc::new(backend), Some(levels), Some(graph))
        }
        Err(e) => {
            backend_fallback = Some(e.to_string());
            (Arc::new(PactlBackend::new()), None, None)
        }
    };
    let backend_native = levels.is_some();
    let app_state = AppState::new(backend, backend_native);

    let result = tauri::Builder::default()
        // Must be the FIRST plugin. When a second instance is launched (KDE
        // session-restore firing alongside our systemd autostart unit, or the
        // user relaunching from the menu), this callback runs in the ALREADY
        // running instance and the new process exits — no duplicate. We reveal
        // the existing window unless the relaunch asked to stay in the tray
        // (`--minimized`), so a menu relaunch brings Inari to the front.
        //
        // This is also the CLI's transport: `inari mute chat` is just such a
        // second launch, handled here and — like `--minimized` — without
        // touching the window.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let args: Vec<String> = argv.into_iter().skip(1).collect();
            if commands::cli::handle_control(app, &args) {
                return;
            }
            if args.iter().any(|a| a == "--minimized") {
                return;
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(log_plugin())
        .plugin(tauri_plugin_dialog::init())
        // Registration happens later, from `setup`: nothing is bound by
        // default, so the plugin starts empty.
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(app_state)
        .manage(commands::hotkeys::HotkeyErrors::default())
        .manage(remote::RemoteState::new())
        .invoke_handler(tauri::generate_handler![
            commands::devices::get_virtual_devices,
            commands::devices::get_app_streams,
            commands::devices::get_output_devices,
            commands::devices::init_virtual_devices,
            commands::devices::teardown_virtual_devices,
            commands::devices::get_channel_outputs,
            commands::devices::get_resolved_outputs,
            commands::devices::get_channel_failover,
            commands::devices::set_channel_failover,
            commands::devices::set_channel_output,
            commands::apps::get_seen_apps,
            commands::apps::set_app_ignored,
            commands::apps::forget_app,
            commands::apps::set_app_assignment,
            commands::channels::add_channel,
            commands::channels::rename_channel,
            commands::channels::reorder_channels,
            commands::channels::remove_channel,
            commands::channels::set_channel_icon,
            commands::buses::list_buses,
            commands::buses::add_bus,
            commands::buses::rename_bus,
            commands::buses::remove_bus,
            commands::buses::set_bus_members,
            commands::buses::set_bus_exclude,
            commands::buses::set_bus_volume,
            commands::buses::set_bus_mute,
            commands::routing::route_app_to_channel,
            commands::routing::set_channel_volume,
            commands::routing::toggle_channel_mute,
            commands::routing::set_app_volume,
            commands::routing::rename_app,
            commands::routing::set_monitor,
            commands::mic::get_mic_config,
            commands::mic::set_mic_config,
            commands::mic::get_input_devices,
            commands::eq::get_channel_eq_configs,
            commands::eq::set_channel_eq,
            commands::eq::list_eq_presets,
            commands::eq::save_user_eq_preset,
            commands::eq::delete_user_eq_preset,
            commands::eq::export_channel_eq,
            commands::eq::export_channel_eq_to_file,
            commands::eq::import_eq_config,
            commands::eq::import_eq_file,
            commands::profiles::list_profiles,
            commands::profiles::load_profile,
            commands::profiles::delete_profile,
            commands::profiles::set_profile_trigger,
            commands::profiles::create_blank_profile,
            commands::profiles::get_active_profile,
            commands::settings::get_backend_info,
            commands::settings::get_autostart,
            commands::settings::set_autostart,
            commands::settings::get_default_devices,
            commands::settings::set_default_output,
            commands::settings::set_default_input,
            commands::settings::get_prefs,
            commands::settings::set_device_label_style,
            commands::settings::set_onboarded,
            commands::settings::set_balance_channels,
            commands::settings::set_balance_visible,
            commands::settings::set_start_minimized,
            commands::settings::reset_app,
            commands::settings::open_log_dir,
            commands::hotkeys::get_hotkeys,
            commands::hotkeys::set_hotkey,
            commands::hotkeys::set_hotkey_channel,
            commands::headset::get_headset_status,
            commands::headset::headset_set_sidetone,
            commands::headset::headset_set_mic_volume,
            commands::headset::headset_set_mic_led,
            commands::headset::headset_set_anc,
            commands::headset::headset_set_transparency,
            commands::headset::headset_set_auto_off,
            commands::headset::headset_set_gain_high,
            commands::headset::headset_set_wireless_range,
            commands::headset::headset_set_line_out,
            commands::headset::headset_set_line_out_volumes,
            commands::headset::headset_set_eq_bands,
            commands::headset::headset_set_eq_preset,
            commands::headset::headset_eq_presets,
            commands::headset::headset_apply_eq_preset,
            commands::headset::headset_save,
            commands::headset::headset_get_alsa_headroom,
            commands::headset::headset_set_alsa_headroom,
            commands::headset::headset_oled_text,
            commands::headset::headset_oled_status,
            commands::headset::headset_oled_modes,
            commands::headset::headset_oled_system,
            commands::headset::headset_oled_now_playing,
            commands::headset::headset_get_notify_mirror,
            commands::headset::headset_set_notify_mirror,
            commands::headset::headset_get_notify_display,
            commands::headset::headset_set_notify_display,
            commands::headset::headset_oled_notify,
            commands::headset::headset_oled_media,
            commands::headset::headset_oled_clips,
            commands::headset::headset_oled_clip,
            commands::headset::headset_oled_mode,
            commands::headset::headset_oled_rotate,
            commands::headset::headset_oled_auto,
            commands::headset::headset_timer_countdown,
            commands::headset::headset_timer_stopwatch,
            commands::headset::headset_timer_toggle,
            commands::headset::headset_timer_reset,
            commands::headset::headset_oled_brightness,
            commands::headset::headset_oled_return_ui,
            commands::mouse::get_mouse_status,
            commands::mouse::mouse_set_dpi,
            commands::mouse::mouse_set_polling,
            commands::mouse::mouse_set_zone_color,
            commands::mouse::mouse_set_rainbow,
            commands::mouse::mouse_set_reactive,
            commands::mouse::mouse_set_sleep,
            commands::mouse::mouse_set_dim,
            commands::mouse::mouse_set_startup_lighting,
            commands::remote::get_remote_status,
            commands::remote::set_remote_enabled,
            commands::remote::get_remote_pairing,
            commands::remote::set_remote_bind,
            commands::remote::regenerate_remote_token,
            commands::update::check_update,
            commands::update::apply_update,
            commands::update::restart_app,
            commands::update::open_url,
        ])
        .setup(move |app| {
            // Reaching setup means no other Inari was running - the
            // single-instance plugin exits the process before this when one
            // is. So a control command has nothing to act on: say so instead
            // of silently booting a whole mixer the user did not ask for.
            let args = own_args();
            if matches!(commands::cli::parse(&args), commands::cli::Cli::Control(_)) {
                eprintln!("inari: not running - start Inari first");
                std::process::exit(1);
            }
            // First lines in the log: the two facts every bug report needs.
            info!("Inari {} starting", env!("CARGO_PKG_VERSION"));
            match &backend_fallback {
                Some(e) => warn!("native PipeWire backend unavailable ({e}); using pactl fallback"),
                None => info!("audio backend: native PipeWire"),
            }
            build_tray(app)?;
            // Off the main thread on purpose: registering a hotkey hops to the
            // main thread, which is still inside `setup` and would deadlock.
            let handle = app.handle().clone();
            std::thread::spawn(move || commands::hotkeys::apply(&handle));
            // The window starts hidden (config) to avoid a flash; show it
            // now unless launched with --minimized (autostart-to-tray).
            let minimized = args.iter().any(|a| a == "--minimized");
            if !minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                }
            }
            // Inari Remote. The bridge is installed unconditionally - it costs
            // nothing while nobody is connected - but the server only comes up
            // if the user turned it on, and only where they told it to listen.
            remote::bridge_events(app.handle());
            let remote_config = app
                .state::<AppState>()
                .lock_mixer()
                .map(|mixer| mixer.prefs.remote.clone())
                .unwrap_or_default();
            remote::apply_saved(app.handle(), &remote_config);
            let remote_clients = app.state::<remote::RemoteState>().client_count();

            if let Some(levels) = levels {
                // The OLED VU mode reads the same peak store as the UI meters.
                app.state::<AppState>().headset.set_levels(levels.clone());
                spawn_level_emitter(app.handle().clone(), levels, remote_clients.clone());
            }
            if let Some(graph) = graph {
                spawn_graph_emitter(app.handle().clone(), graph, remote_clients);
            }
            // Start the Arctis base-station supervisor (discovery, status
            // stream, OLED). No-op on machines without the headset.
            app.state::<AppState>()
                .headset
                .start(app.handle().clone());
            // Same for the SteelSeries mouse (no-op without one attached).
            app.state::<AppState>()
                .mouse
                .start(app.handle().clone());
            // Feed the OLED the data it can't read itself (mouse, app list).
            spawn_oled_aux_feeder(app.handle().clone());
            Ok(())
        })
        // Close button hides to tray instead of quitting.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(e) = window.hide() {
                    error!("failed to hide window: {e}");
                }
            }
        })
        .run(tauri::generate_context!());

    if let Err(e) = result {
        // Also on stderr: this can fire before the log plugin is initialized,
        // in which case the macro below goes nowhere.
        eprintln!("fatal error while running tauri application: {e}");
        error!("fatal error while running tauri application: {e}");
        std::process::exit(1);
    }
}

/// Pushes data the OLED modes need but the draw thread can't read itself —
/// the mouse's battery and the list of apps currently making sound. Runs on a
/// slow cadence because none of it changes quickly.
fn spawn_oled_aux_feeder(handle: tauri::AppHandle) {
    use headset::oled_controller::AuxData;
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3));
        let state = handle.state::<AppState>();
        if !state.headset.is_connected() {
            continue;
        }
        let mouse = state.mouse.status();
        let apps = state
            .backend
            .list_app_streams()
            .map(|streams| {
                streams
                    .into_iter()
                    .map(|s| (s.alias.unwrap_or(s.app_name), s.active))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        state.headset.push_aux(AuxData {
            mouse_battery: mouse.battery_percent,
            mouse_charging: mouse.charging,
            mouse_model: mouse.model.unwrap_or_else(|| "Mouse".to_string()),
            active_apps: apps,
        });
    });
}

/// Turns PipeWire graph changes into coalesced `graph-changed` events, so the
/// UI refetches streams and devices when something actually happened instead
/// of every 2s forever (TD-009). Native backend only - the pactl fallback has
/// no graph to listen to and keeps polling.
fn spawn_graph_emitter(
    handle: tauri::AppHandle,
    graph: Arc<GraphNotify>,
    remote_clients: Arc<remote::Clients>,
) {
    /// How long to sit idle before looking again. Only a safety net against a
    /// lost condvar wakeup - a quiet session costs one wakeup a minute.
    const IDLE_WAIT: Duration = Duration::from_secs(60);
    /// Re-check cadence while a burst is settling.
    const TICK: Duration = Duration::from_millis(25);

    std::thread::spawn(move || {
        let mut coalescer = GraphCoalescer::new(GRAPH_QUIET, GRAPH_MAX_WAIT);
        // Starts at 0 like the coalescer, so whatever the loop thread already
        // saw while the app was booting counts as one change.
        let mut revision = 0;
        loop {
            let wait = if coalescer.pending() { TICK } else { IDLE_WAIT };
            revision = graph.wait_changed(revision, wait);
            if !coalescer.poll(revision, std::time::Instant::now()) {
                continue;
            }
            // Same reason the level emitter checks: don't wake a webview
            // nobody can see. The UI refetches on visibilitychange anyway,
            // so nothing is lost by dropping the event here (TD-008).
            let onscreen = handle
                .get_webview_window("main")
                .map(|w| w.is_visible().unwrap_or(true) && !w.is_minimized().unwrap_or(false))
                .unwrap_or(true);
            // A tablet on the LAN has no visibilitychange to refetch on, and
            // it is being used precisely when the desktop window is hidden.
            if !onscreen && !remote_clients.any() {
                continue;
            }
            // No payload: the event says "the graph moved", the UI decides
            // what it needs to reread.
            if handle.emit("graph-changed", ()).is_err() {
                // App is shutting down.
                break;
            }
        }
    });
}

/// Streams per-channel peak levels to the UI at 10 Hz as `levels` events.
/// Peaks are drained (read-and-reset), so silence decays to zero.
fn spawn_level_emitter(
    handle: tauri::AppHandle,
    levels: Arc<LevelStore>,
    remote_clients: Arc<remote::Clients>,
) {
    std::thread::spawn(move || {
        let mut prev_all_zero = false;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            // The app's dominant state is sitting in the tray during a game.
            // Don't lock the registry, serialize a map and wake the webview
            // for a window nobody can see (TD-008).
            let onscreen = handle
                .get_webview_window("main")
                .map(|w| w.is_visible().unwrap_or(true) && !w.is_minimized().unwrap_or(false))
                .unwrap_or(true);
            // ...unless a tablet is watching the meters, which is the whole
            // point of the remote: the window is hidden while it is in use.
            // The webview gets a wasted wakeup in that case; the alternative
            // is meters that only work when nobody needs them.
            if !onscreen && !remote_clients.any() {
                // Force a fresh frame when the window returns.
                prev_all_zero = false;
                continue;
            }
            // The meter registry is dynamic (user-defined channels + mic).
            let payload: HashMap<String, [f32; 2]> = levels
                .names()
                .into_iter()
                .map(|(name, slot)| (name, [levels.drain(slot, 0), levels.drain(slot, 1)]))
                .collect();
            // Emit the first all-zero frame so the meters settle to zero, then
            // go quiet until sound returns instead of pushing silence at 10 Hz.
            let all_zero = payload.values().all(|[l, r]| *l < 1e-4 && *r < 1e-4);
            if all_zero && prev_all_zero {
                continue;
            }
            prev_all_zero = all_zero;
            if handle.emit("levels", &payload).is_err() {
                // App is shutting down.
                break;
            }
        }
    });
}

/// A tray mute row: what the menu must show for one mutable thing. Split out
/// as plain data so the state → menu mapping is testable without a tray.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrayMute {
    /// Menu id; the part after `mute:` is the sink name (or `mic`).
    pub id: String,
    pub label: String,
    pub muted: bool,
}

/// The mic first, then one row per live channel strip. Before
/// `init_virtual_devices` there are no strips yet, so only the mic shows -
/// muting a channel that does not exist would just fail.
pub(crate) fn tray_mutes(mixer: &mixer::state::MixerState) -> Vec<TrayMute> {
    let mut rows = vec![TrayMute {
        id: "mute:mic".to_string(),
        label: "Microphone".to_string(),
        muted: mixer.mic.muted,
    }];
    rows.extend(mixer.channels.iter().map(|c| TrayMute {
        id: format!("mute:{}", c.name),
        label: c.label.clone(),
        muted: c.muted,
    }));
    rows
}

/// Build the tray menu, including the live Mute and Profiles submenus (checks
/// on what is muted / active). Rebuilt via `refresh_tray` whenever any of that
/// changes.
fn build_tray_menu(
    app: &tauri::AppHandle,
) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    use tauri::menu::{IsMenuItem, Submenu};

    let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;

    let mutes = app
        .state::<AppState>()
        .lock_mixer()
        .map(|m| tray_mutes(&m))
        .unwrap_or_default();
    let mute_items: Vec<CheckMenuItem<tauri::Wry>> = mutes
        .iter()
        .map(|row| {
            CheckMenuItem::with_id(app, &row.id, &row.label, true, row.muted, None::<&str>)
        })
        .collect::<Result<_, _>>()?;
    let mute_refs: Vec<&dyn IsMenuItem<tauri::Wry>> = mute_items
        .iter()
        .map(|i| i as &dyn IsMenuItem<tauri::Wry>)
        .collect();
    let mute_menu = Submenu::with_items(app, "Mute", true, &mute_refs)?;

    let active = app
        .state::<AppState>()
        .lock_mixer()
        .ok()
        .and_then(|m| m.active_profile.clone());
    let profile_items: Vec<CheckMenuItem<tauri::Wry>> = persistence::profiles::list()
        .unwrap_or_default()
        .into_iter()
        .map(|info| {
            CheckMenuItem::with_id(
                app,
                format!("profile:{}", info.name),
                &info.name,
                true,
                active.as_deref() == Some(info.name.as_str()),
                None::<&str>,
            )
        })
        .collect::<Result<_, _>>()?;
    let profile_refs: Vec<&dyn IsMenuItem<tauri::Wry>> = profile_items
        .iter()
        .map(|i| i as &dyn IsMenuItem<tauri::Wry>)
        .collect();
    let profiles_menu = Submenu::with_items(app, "Profiles", true, &profile_refs)?;

    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    Ok(Menu::with_items(
        app,
        &[&show, &mute_menu, &profiles_menu, &quit],
    )?)
}

/// Rebuild the tray menu (called after anything that changes profiles, mute
/// state or the channel set).
///
/// The rebuild is posted to the main thread rather than run inline: menu items
/// are GTK objects, and the callers now include a global-hotkey handler and
/// the single-instance D-Bus callback, neither of which runs there. Posting
/// (rather than waiting) also means a caller still holding the mixer lock
/// cannot deadlock against `build_tray_menu` taking it.
pub(crate) fn refresh_tray(app: &tauri::AppHandle) {
    let handle = app.clone();
    let posted = app.run_on_main_thread(move || {
        let Some(tray) = handle.tray_by_id("sink-tray") else {
            return;
        };
        match build_tray_menu(&handle) {
            Ok(menu) => {
                if let Err(e) = tray.set_menu(Some(menu)) {
                    error!("tray menu refresh failed: {e}");
                }
            }
            Err(e) => error!("tray menu rebuild failed: {e}"),
        }
    });
    if let Err(e) = posted {
        error!("tray menu refresh could not reach the main thread: {e}");
    }
}

/// Flip the mute of one tray target (`mic` or a sink name) to the opposite of
/// what the mixer currently holds.
fn toggle_tray_mute(app: &tauri::AppHandle, target: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    if target == "mic" {
        let muted = !state.lock_mixer()?.mic.muted;
        return commands::mic::set_mic_muted(&state, muted);
    }
    let muted = state
        .lock_mixer()?
        .channels
        .iter()
        .find(|c| c.name == target)
        .ok_or_else(|| format!("unknown channel: {target}"))?
        .muted;
    commands::routing::toggle_channel_mute(app.state(), target.to_string(), !muted)
}

fn build_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let menu = build_tray_menu(app.handle())?;

    // Dedicated 22px tray glyph from the icon pack (white for the common
    // dark panel; the full-color icon stays on the window/dock).
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-white-22.png"))?;

    TrayIconBuilder::with_id("sink-tray")
        .icon(icon)
        .tooltip("Inari")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| {
            let id = event.id.as_ref();
            if let Some(name) = id.strip_prefix("profile:") {
                // Switch profiles straight from the tray; tell the UI.
                match commands::profiles::load_profile(
                    app.clone(),
                    app.state(),
                    name.to_string(),
                ) {
                    Ok(()) => {
                        let _ = app.emit("profile-changed", name);
                    }
                    Err(e) => error!("tray profile switch failed: {e}"),
                }
                return;
            }
            if let Some(target) = id.strip_prefix("mute:") {
                // The check mark the user just clicked is only a request; the
                // rebuild below re-reads the real state, so a failed mute
                // snaps back instead of lying.
                if let Err(e) = toggle_tray_mute(app, target) {
                    error!("tray mute of {target} failed: {e}");
                }
                after_external_change(app);
                return;
            }
            match id {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "quit" => {
                    // Clean up our virtual sinks before exiting. Best-effort:
                    // log failures but never block quitting.
                    let state = app.state::<AppState>();
                    for err in state.teardown_virtual_sinks() {
                        warn!("teardown of virtual sinks: {err}");
                    }
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixer::state::MixerState;

    #[test]
    fn before_the_sinks_exist_only_the_mic_can_be_muted() {
        let rows = tray_mutes(&MixerState::default());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "mute:mic");
        assert!(!rows[0].muted);
    }

    #[test]
    fn tray_mute_rows_mirror_the_live_state() {
        let mut mixer = MixerState::default();
        mixer.init_defaults();
        mixer.mic.muted = true;
        mixer.channel_mut("sink_chat").expect("chat").muted = true;

        let rows = tray_mutes(&mixer);
        assert_eq!(rows.len(), 1 + mixer.channels.len());
        assert_eq!(rows[0], TrayMute {
            id: "mute:mic".into(),
            label: "Microphone".into(),
            muted: true,
        });
        // Ids carry the sink name the handler mutes, labels carry what the
        // user named the channel - mixing the two up would mute the wrong one.
        assert_eq!(rows[1].id, "mute:sink_game");
        assert_eq!(rows[1].label, "Game");
        assert!(!rows[1].muted);
        assert_eq!(rows[2].id, "mute:sink_chat");
        assert!(rows[2].muted, "the muted channel shows checked");
    }

    #[test]
    fn renaming_a_channel_moves_the_label_not_the_id() {
        let mut mixer = MixerState::default();
        mixer.init_defaults();
        mixer.channel_mut("sink_game").expect("game").label = "Battlefield".into();
        let rows = tray_mutes(&mixer);
        assert_eq!(rows[1].id, "mute:sink_game");
        assert_eq!(rows[1].label, "Battlefield");
    }
}

#[cfg(test)]
mod ipc_contract {
    /// The frontend routes every call through `src/lib/ipc.ts`, whose `Command`
    /// union is hand-maintained. Nothing else connects the two: a command
    /// renamed here would keep compiling on both sides and only break when a
    /// user clicked the thing. This pins them together.
    #[test]
    fn command_union_matches_the_handler_list() {
        let lib = include_str!("lib.rs");
        let handler = lib
            .split_once("generate_handler![")
            .expect("handler list")
            .1
            .split_once("])")
            .expect("handler list end")
            .0;
        let mut rust: Vec<&str> = handler
            .lines()
            .filter_map(|l| l.trim().trim_end_matches(',').rsplit("::").next())
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()))
            .collect();
        rust.sort_unstable();
        rust.dedup();

        let ts = include_str!("../../src/lib/ipc.ts");
        let union = ts
            .split_once("export type Command =")
            .expect("Command union")
            .1
            .split_once(';')
            .expect("union end")
            .0;
        let mut front: Vec<&str> = union
            .lines()
            .filter_map(|l| l.trim().strip_prefix("| \""))
            .filter_map(|l| l.strip_suffix('"'))
            .collect();
        front.sort_unstable();

        let missing: Vec<_> = rust.iter().filter(|c| !front.contains(c)).collect();
        let extra: Vec<_> = front.iter().filter(|c| !rust.contains(c)).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "src/lib/ipc.ts is out of sync with generate_handler!\n  missing from ipc.ts: {missing:?}\n  not a real command: {extra:?}"
        );
    }
}
