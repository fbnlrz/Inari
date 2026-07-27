//! What a remote client is allowed to do, and nothing else.
//!
//! The Tauri handler carries 121 commands and several of them are lethal the
//! moment they are reachable from a network rather than from a webview the
//! user is looking at: `apply_update` ends in `pkexec apt-get install` as root,
//! `reset_app` deletes the config directory and the WirePlumber fragment,
//! `import_eq_file` / `export_channel_eq_to_file` / `headset_oled_media` take
//! filesystem paths, `open_url` and `set_notify_mirror` spawn processes, and
//! `init_virtual_devices` / `teardown_virtual_devices` take the audio chain
//! apart.
//!
//! So the table below is a *positive* list: a command that is not named here
//! is rejected before anything is dispatched, and the list is a compile-time
//! constant that nothing the client sends can influence. Adding a command to
//! the remote is a deliberate edit to this file, which is exactly the review
//! surface it deserves. Structural edits (creating, renaming or deleting
//! channels, mixes and profiles) stay out on purpose: a mis-tap on a
//! touchscreen must not rebuild someone's mixer.
//!
//! The handlers are the very same `#[tauri::command]` functions the desktop UI
//! calls. They are ordinary Rust functions, and `AppHandle::state()` produces
//! the `State<'_, AppState>` they expect, so there is no second implementation
//! of 68 command bodies here to drift out of sync with the first.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};

use crate::commands;

/// The `args` object of a request frame.
///
/// Lookups accept both the Rust parameter name and its camelCase form,
/// because the frontend sends the same argument object it sends to Tauri's
/// `invoke`, which camelCases by convention.
#[derive(Debug, Default, Clone)]
pub struct Args(Map<String, Value>);

impl Args {
    pub fn new(map: Map<String, Value>) -> Self {
        Self(map)
    }

    /// One argument, deserialized into whatever the handler's parameter is.
    fn get<T: DeserializeOwned>(&self, name: &str) -> Result<T, String> {
        let value = self
            .0
            .get(name)
            .or_else(|| self.0.get(&camel_case(name)))
            .ok_or_else(|| format!("missing argument `{name}`"))?;
        serde_json::from_value(value.clone()).map_err(|e| format!("argument `{name}`: {e}"))
    }
}

/// `sink_name` -> `sinkName`.
fn camel_case(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut upper = false;
    for c in snake.chars() {
        match c {
            '_' => upper = true,
            _ if upper => {
                out.extend(c.to_uppercase());
                upper = false;
            }
            _ => out.push(c),
        }
    }
    out
}

/// Wrap a handler's return value in the reply frame's `data`.
fn ok<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| format!("cannot serialize the reply: {e}"))
}

/// Builds the allowlist and the dispatcher from one list, so the set of names
/// that passes the gate and the set of names that has a handler are the same
/// set by construction. Two hand-maintained lists would eventually disagree,
/// and the failure mode of that disagreement is a command reachable without
/// having been reviewed.
macro_rules! sync_table {
    ($($name:ident => $handler:expr),* $(,)?) => {
        /// Every synchronously-dispatched command name, in table order.
        const SYNC: &[&str] = &[$(stringify!($name)),*];

        /// `None` means "not in the table" - i.e. denied.
        fn call_sync(app: &AppHandle, cmd: &str, args: &Args) -> Option<Result<Value, String>> {
            match cmd {
                $(stringify!($name) => {
                    let handler: fn(&AppHandle, &Args) -> Result<Value, String> = $handler;
                    Some(handler(app, args))
                })*
                _ => None,
            }
        }
    };
}

sync_table! {
    // -- reads -------------------------------------------------------------
    get_virtual_devices => |app, _| ok(commands::devices::get_virtual_devices(app.state())?),
    get_app_streams => |app, _| ok(commands::devices::get_app_streams(app.state())?),
    get_output_devices => |app, _| ok(commands::devices::get_output_devices(app.state())?),
    get_channel_outputs => |app, _| ok(commands::devices::get_channel_outputs(app.state())?),
    get_resolved_outputs => |app, _| ok(commands::devices::get_resolved_outputs(app.state())?),
    get_channel_failover => |app, _| ok(commands::devices::get_channel_failover(app.state())?),
    get_seen_apps => |app, _| ok(commands::apps::get_seen_apps(app.state())?),
    list_buses => |app, _| ok(commands::buses::list_buses(app.state())?),
    get_mic_config => |app, _| ok(commands::mic::get_mic_config(app.state())?),
    get_input_devices => |app, _| ok(commands::mic::get_input_devices(app.state())?),
    get_channel_eq_configs => |app, _| ok(commands::eq::get_channel_eq_configs(app.state())?),
    list_eq_presets => |_, _| ok(commands::eq::list_eq_presets()?),
    get_headset_status => |app, _| ok(commands::headset::get_headset_status(app.state())),
    headset_eq_presets => |_, _| ok(commands::headset::headset_eq_presets()),
    headset_oled_modes => |_, _| ok(commands::headset::headset_oled_modes()),
    headset_oled_clips => |_, _| ok(commands::headset::headset_oled_clips()),
    headset_oled_status => |app, _| ok(commands::headset::headset_oled_status(app.state())?),
    // The headset page reads these on mount. Denying them did not protect
    // anything -- they only report state -- it just left the page broken.
    // Their `set` counterparts stay off the list: one spawns a dbus-monitor
    // process, the other writes a WirePlumber fragment that only takes effect
    // at the next login, neither of which belongs to a tablet.
    headset_get_notify_mirror => |app, _| ok(commands::headset::headset_get_notify_mirror(app.state())),
    headset_get_notify_display => |app, _| ok(commands::headset::headset_get_notify_display(app.state())),
    headset_get_alsa_headroom => |_, _| ok(commands::headset::headset_get_alsa_headroom()),
    list_profiles => |_, _| ok(commands::profiles::list_profiles()?),
    // The mixer reads this to know which channels the balance slider blends
    // and whether to show it at all. Prefs carry no secret.
    get_prefs => |app, _| ok(commands::settings::get_prefs(app.state())?),
    get_active_profile => |app, _| ok(commands::profiles::get_active_profile(app.state())?),
    get_backend_info => |app, _| ok(commands::settings::get_backend_info(app.state())),

    // -- mixer writes ------------------------------------------------------
    set_channel_volume => |app, a| ok(commands::routing::set_channel_volume(
        app.state(), a.get("sink_name")?, a.get("volume")?)?),
    toggle_channel_mute => |app, a| ok(commands::routing::toggle_channel_mute(
        app.state(), a.get("sink_name")?, a.get("muted")?)?),
    set_app_volume => |app, a| ok(commands::routing::set_app_volume(
        app.state(), a.get("stream_index")?, a.get("volume")?)?),
    route_app_to_channel => |app, a| ok(commands::routing::route_app_to_channel(
        app.state(), a.get("stream_index")?, a.get("sink_name")?)?),
    set_monitor => |app, a| ok(commands::routing::set_monitor(
        app.state(), a.get("sink_name")?, a.get("enabled")?)?),
    set_channel_output => |app, a| ok(commands::devices::set_channel_output(
        app.state(), a.get("sink_name")?, a.get("output_name")?)?),
    set_channel_failover => |app, a| ok(commands::devices::set_channel_failover(
        app.state(), a.get("sink_name")?, a.get("enabled")?)?),
    set_app_assignment => |app, a| ok(commands::apps::set_app_assignment(
        app.state(), a.get("match_prop")?, a.get("match_value")?, a.get("sink_name")?)?),
    set_app_ignored => |app, a| ok(commands::apps::set_app_ignored(
        app.state(), a.get("match_prop")?, a.get("match_value")?, a.get("ignored")?)?),

    // -- mixes -------------------------------------------------------------
    set_bus_volume => |app, a| ok(commands::buses::set_bus_volume(
        app.state(), a.get("name")?, a.get("volume")?)?),
    set_bus_mute => |app, a| ok(commands::buses::set_bus_mute(
        app.state(), a.get("name")?, a.get("muted")?)?),
    set_bus_members => |app, a| ok(commands::buses::set_bus_members(
        app.state(), a.get("name")?, a.get("channels")?)?),
    set_bus_exclude => |app, a| ok(commands::buses::set_bus_exclude(
        app.state(), a.get("name")?, a.get("exclude")?)?),

    // -- mic ---------------------------------------------------------------
    set_mic_config => |app, a| ok(commands::mic::set_mic_config(app.state(), a.get("config")?)?),

    // -- software EQ -------------------------------------------------------
    set_channel_eq => |app, a| ok(commands::eq::set_channel_eq(
        app.state(), a.get("sink_name")?, a.get("config")?)?),
    save_user_eq_preset => |_, a| ok(commands::eq::save_user_eq_preset(
        a.get("name")?, a.get("config")?)?),
    delete_user_eq_preset => |_, a| ok(commands::eq::delete_user_eq_preset(a.get("name")?)?),
    // Text in, text out. The `_file` variants of these two take filesystem
    // paths and are deliberately absent.
    export_channel_eq => |app, a| ok(commands::eq::export_channel_eq(app.state(), a.get("sink_name")?)?),
    import_eq_config => |_, a| ok(commands::eq::import_eq_config(a.get("text")?)?),

    // -- headset hardware --------------------------------------------------
    headset_set_sidetone => |app, a| ok(commands::headset::headset_set_sidetone(app.state(), a.get("level")?)?),
    headset_set_mic_volume => |app, a| ok(commands::headset::headset_set_mic_volume(app.state(), a.get("level")?)?),
    headset_set_mic_led => |app, a| ok(commands::headset::headset_set_mic_led(app.state(), a.get("level")?)?),
    headset_set_anc => |app, a| ok(commands::headset::headset_set_anc(app.state(), a.get("mode")?)?),
    headset_set_transparency => |app, a| ok(commands::headset::headset_set_transparency(app.state(), a.get("level")?)?),
    headset_set_auto_off => |app, a| ok(commands::headset::headset_set_auto_off(app.state(), a.get("idx")?)?),
    headset_set_gain_high => |app, a| ok(commands::headset::headset_set_gain_high(app.state(), a.get("high")?)?),
    headset_set_wireless_range => |app, a| ok(commands::headset::headset_set_wireless_range(app.state(), a.get("range")?)?),
    headset_set_line_out => |app, a| ok(commands::headset::headset_set_line_out(app.state(), a.get("mode")?)?),
    headset_set_line_out_volumes => |app, a| ok(commands::headset::headset_set_line_out_volumes(
        app.state(), a.get("left")?, a.get("right")?, a.get("aux")?)?),
    // The balance slider is a mixer control, and the ChatMix blend is one of
    // the things you actually want to reach for mid-game. Both only write
    // prefs.
    set_balance_visible => |app, a| ok(commands::settings::set_balance_visible(app.state(), a.get("visible")?)?),
    set_balance_channels => |app, a| ok(commands::settings::set_balance_channels(
        app.state(), a.get("a")?, a.get("b")?)?),
    // Writes prefs and tells the OLED thread; no process, no path, no system
    // config -- so how long a mirrored notification stays up is fair game.
    headset_set_notify_display => |app, a| ok(commands::headset::headset_set_notify_display(
        app.state(), a.get("duration_secs")?, a.get("scroll")?)?),
    headset_set_eq_bands => |app, a| ok(commands::headset::headset_set_eq_bands(app.state(), a.get("bands")?)?),
    headset_set_eq_preset => |app, a| ok(commands::headset::headset_set_eq_preset(app.state(), a.get("preset")?)?),
    headset_apply_eq_preset => |app, a| ok(commands::headset::headset_apply_eq_preset(app.state(), a.get("name")?)?),
    headset_save => |app, _| ok(commands::headset::headset_save(app.state())?),

    // -- OLED --------------------------------------------------------------
    // `headset_oled_media` is absent: it takes a filesystem path.
    headset_oled_text => |app, a| ok(commands::headset::headset_oled_text(app.state(), a.get("lines")?)?),
    headset_oled_mode => |app, a| ok(commands::headset::headset_oled_mode(app.state(), a.get("id")?)?),
    headset_oled_rotate => |app, a| ok(commands::headset::headset_oled_rotate(
        app.state(), a.get("ids")?, a.get("secs")?)?),
    headset_oled_auto => |app, _| ok(commands::headset::headset_oled_auto(app.state())?),
    headset_oled_brightness => |app, a| ok(commands::headset::headset_oled_brightness(app.state(), a.get("level")?)?),
    headset_oled_return_ui => |app, _| ok(commands::headset::headset_oled_return_ui(app.state())?),
    headset_oled_system => |app, _| ok(commands::headset::headset_oled_system(app.state())?),
    headset_oled_now_playing => |app, _| ok(commands::headset::headset_oled_now_playing(app.state())?),
    headset_oled_notify => |app, a| ok(commands::headset::headset_oled_notify(
        app.state(), a.get("lines")?, a.get("duration_ms")?)?),
    headset_timer_countdown => |app, a| ok(commands::headset::headset_timer_countdown(app.state(), a.get("secs")?)?),
    headset_timer_stopwatch => |app, _| ok(commands::headset::headset_timer_stopwatch(app.state())?),
    headset_timer_toggle => |app, _| ok(commands::headset::headset_timer_toggle(app.state())?),
    headset_timer_reset => |app, _| ok(commands::headset::headset_timer_reset(app.state())?),

    // -- profiles ----------------------------------------------------------
    // Switching between saved profiles only. `create_blank_profile` and
    // `delete_profile` are structural and stay off the remote.
    load_profile => |app, a| ok(commands::profiles::load_profile(
        app.clone(), app.state(), a.get("name")?)?),
}

/// The one allowlisted command whose handler is an `async fn`. Kept as its own
/// constant next to the arm that serves it, so the gate and the dispatcher
/// still cannot disagree.
const ASYNC: &[&str] = &["headset_oled_clip"];

async fn call_async(app: &AppHandle, cmd: &str, args: &Args) -> Option<Result<Value, String>> {
    match cmd {
        "headset_oled_clip" => Some(match args.get("name") {
            Ok(name) => commands::headset::headset_oled_clip(app.state(), name)
                .await
                .map(|()| Value::Null),
            Err(e) => Err(e),
        }),
        _ => None,
    }
}

/// Is `cmd` on the allowlist? Deny by default.
pub fn is_allowed(cmd: &str) -> bool {
    SYNC.contains(&cmd) || ASYNC.contains(&cmd)
}

/// Every command a remote client may send. Test-only: nothing in the app has
/// a reason to enumerate the list, and it must not become a thing a client can
/// ask for either.
#[cfg(test)]
fn allowed() -> Vec<&'static str> {
    SYNC.iter().chain(ASYNC.iter()).copied().collect()
}

/// Does `cmd` only read? Everything else on the list changes something, which
/// is what the desktop UI has to be nudged about after a remote client acts.
pub fn is_read(cmd: &str) -> bool {
    cmd.starts_with("get_")
        || cmd.starts_with("list_")
        || matches!(
            cmd,
            "headset_eq_presets" | "headset_oled_modes" | "headset_oled_clips"
        )
}

/// Run one allowlisted command and return its serialized result.
///
/// The gate is checked here rather than left to the table's `None` arm so that
/// a denial is one uniform answer regardless of which table would have missed,
/// and so the check is impossible to skip by calling the wrong entry point.
///
/// The synchronous handlers run on the blocking pool: they shell out to
/// `pactl`, write HID feature reports and take the mixer lock, none of which
/// belongs on an async worker that also has to keep the WebSocket responsive.
pub async fn dispatch(app: &AppHandle, cmd: &str, args: Args) -> Result<Value, String> {
    if !is_allowed(cmd) {
        return Err(format!("`{cmd}` is not available over the remote"));
    }
    if let Some(result) = call_async(app, cmd, &args).await {
        return result;
    }
    let name = cmd.to_string();
    let handle = app.clone();
    let called = name.clone();
    match tokio::task::spawn_blocking(move || call_sync(&handle, &called, &args)).await {
        Ok(Some(result)) => result,
        // Unreachable while `is_allowed` and the tables agree; answering
        // instead of panicking keeps a future mismatch from taking the server
        // down with it.
        Ok(None) => Err(format!("`{name}` is not available over the remote")),
        Err(e) => Err(format!("`{name}` did not complete: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Commands that must never be reachable from the network. Each one is
    /// here for a concrete reason, not for tidiness.
    const DENIED: &[(&str, &str)] = &[
        ("apply_update", "ends in `pkexec apt-get install` as root"),
        ("reset_app", "deletes ~/.config/inari and the WirePlumber fragment"),
        ("import_eq_file", "takes a filesystem path"),
        ("export_channel_eq_to_file", "writes to a filesystem path"),
        ("headset_oled_media", "reads a filesystem path"),
        ("open_url", "spawns a process"),
        ("open_log_dir", "spawns a process"),
        ("set_autostart", "changes what runs at login"),
        ("init_virtual_devices", "rebuilds the whole audio chain"),
        ("teardown_virtual_devices", "tears the whole audio chain down"),
        ("add_channel", "structural: a mis-tap must not rebuild the mixer"),
        ("remove_channel", "structural: a mis-tap must not delete a channel"),
        ("delete_profile", "structural: a mis-tap must not delete a profile"),
    ];

    #[test]
    fn the_dangerous_commands_are_not_dispatchable() {
        for (cmd, why) in DENIED {
            assert!(!is_allowed(cmd), "{cmd} must stay off the remote: {why}");
            assert!(
                !call_sync_names().contains(cmd),
                "{cmd} has a dispatch arm even though the gate denies it"
            );
        }
    }

    /// The table's own names, as the macro emitted them.
    fn call_sync_names() -> Vec<&'static str> {
        SYNC.to_vec()
    }

    #[test]
    fn everything_the_user_agreed_to_is_dispatchable() {
        // The scope the user signed off on: mixer, mixes, apps, mic, both EQs,
        // headset, OLED, profile switching. Spot-checked per area rather than
        // restating all 68 - the table above is the list.
        for cmd in [
            "get_virtual_devices",
            "set_channel_volume",
            "set_bus_members",
            "set_app_ignored",
            "set_mic_config",
            "set_channel_eq",
            "headset_set_eq_bands",
            "headset_oled_clip",
            "load_profile",
        ] {
            assert!(is_allowed(cmd), "{cmd} is in scope but denied");
        }
    }

    #[test]
    fn no_structural_or_path_taking_command_slipped_in() {
        // A sweep over the shapes that must never appear, so a future addition
        // to the table trips this instead of shipping. The structural half is
        // "verb + one of the three things a mixer is made of": creating,
        // renaming, reordering or deleting a channel, a mix or a profile is
        // out of scope regardless of how safe the handler looks. (Deleting a
        // saved EQ preset is not structural - presets are content, and the EQ
        // is in scope.)
        const VERBS: &[&str] = &["add_", "create_", "remove_", "rename_", "reorder_", "delete_"];
        const THINGS: &[&str] = &["channel", "bus", "profile"];
        for cmd in allowed() {
            let structural =
                VERBS.iter().any(|v| cmd.starts_with(v)) && THINGS.iter().any(|t| cmd.contains(t));
            assert!(!structural, "{cmd} restructures the mixer");
            assert!(
                !cmd.starts_with("open_") && !cmd.ends_with("_file") && !cmd.ends_with("_dir"),
                "{cmd} takes a filesystem path or spawns a process"
            );
            assert!(
                !cmd.contains("update") && !cmd.contains("autostart") && !cmd.contains("reset_app"),
                "{cmd} changes what runs on this machine"
            );
        }
    }

    #[test]
    fn the_table_names_each_command_once() {
        let mut names = allowed();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(before, names.len(), "a command is listed twice");
    }

    #[test]
    fn an_unknown_command_is_denied_rather_than_guessed_at() {
        assert!(!is_allowed(""));
        assert!(!is_allowed("set_channel_volume "));
        assert!(!is_allowed("SET_CHANNEL_VOLUME"));
        assert!(!is_allowed("../set_channel_volume"));
    }

    #[test]
    fn arguments_are_read_under_either_spelling() {
        // The frontend sends the same camelCase object it sends to `invoke`;
        // a hand-written client may well send the Rust name.
        let args = Args::new(
            serde_json::json!({ "sinkName": "sink_game", "volume": 42 })
                .as_object()
                .expect("object")
                .clone(),
        );
        assert_eq!(args.get::<String>("sink_name").expect("camel"), "sink_game");
        assert_eq!(args.get::<u8>("volume").expect("plain"), 42);
        assert!(args.get::<String>("missing").is_err());
        // A volume that does not fit the handler's `u8` is a rejection, not a
        // wrap-around: the type in the signature is the validation.
        assert!(args.get::<u8>("sinkName").is_err());
    }

    #[test]
    fn reads_are_told_from_writes() {
        assert!(is_read("get_virtual_devices"));
        assert!(is_read("list_profiles"));
        assert!(is_read("headset_eq_presets"));
        assert!(!is_read("set_channel_volume"));
        assert!(!is_read("headset_oled_status"), "it draws on the display");
    }

    #[test]
    fn camel_case_matches_the_convention_tauri_uses() {
        assert_eq!(camel_case("sink_name"), "sinkName");
        assert_eq!(camel_case("duration_ms"), "durationMs");
        assert_eq!(camel_case("name"), "name");
    }
}

#[cfg(test)]
mod frontend_contract {
    use super::*;

    /// Every command the frontend can send, read out of the sources.
    fn called_by_frontend() -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![std::path::PathBuf::from("../src")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.ends_with(".ts") && !name.ends_with(".tsx") {
                    continue;
                }
                // ipc.ts holds the union itself; tests call whatever they like.
                if name == "ipc.ts" || name.contains(".test.") {
                    continue;
                }
                let Ok(src) = std::fs::read_to_string(&p) else { continue };
                for (i, _) in src.match_indices("call(").chain(src.match_indices("call<")) {
                    let rest = &src[i..];
                    let Some(open) = rest.find('(') else { continue };
                    let Some(q) = rest.find('"') else { continue };
                    // The name has to be the literal argument. Anything else
                    // between the paren and the quote means this is a computed
                    // command (a ternary, a variable) and not ours to read.
                    if q < open || !rest[open + 1..q].trim().is_empty() {
                        continue;
                    }
                    let after = &rest[q + 1..];
                    let Some(end) = after.find('"') else { continue };
                    let cmd = &after[..end];
                    if !cmd.is_empty()
                        && cmd.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
                    {
                        out.push(cmd.to_string());
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// A command the UI can send but the remote denies is a button that fails
    /// in the user's hand — which is how `get_hotkeys`, the headset reads and
    /// `init_virtual_devices` were each found, one error at a time. Whenever
    /// that gap changes, it has to be a decision: either the command joins the
    /// allowlist, or the control it belongs to is hidden outside the desktop
    /// shell (see `src/lib/platform.ts`). This test exists to force that
    /// decision rather than let a browser find it.
    #[test]
    fn every_command_the_ui_sends_is_either_allowed_or_deliberately_not() {
        // Denied on purpose. Each entry must have its UI gated behind
        // `isTauri`; add here only together with that gating.
        const GATED_IN_THE_UI: &[&str] = &[
            // installs as root, restarts, opens things on the PC
            "apply_update", "check_update", "restart_app", "open_url", "open_log_dir",
            // system and desktop settings
            "reset_app", "set_autostart", "get_autostart", "set_start_minimized",
            "set_device_label_style", "get_default_devices", "set_default_output",
            "set_default_input", "set_onboarded",
            // take a filesystem path
            "import_eq_file", "export_channel_eq_to_file",
            // spawn a process / write system config
            "headset_set_notify_mirror", "headset_set_alsa_headroom",
            // the desktop instance owns the audio graph's lifetime
            "init_virtual_devices", "teardown_virtual_devices",
            // global shortcuts are a property of the PC, not the tablet
            "get_hotkeys", "set_hotkey", "set_hotkey_channel",
            // the mouse is out of scope for the remote
            "get_mouse_status", "mouse_set_dpi", "mouse_set_polling",
            "mouse_set_zone_color", "mouse_set_rainbow", "mouse_set_reactive",
            "mouse_set_sleep", "mouse_set_dim", "mouse_set_startup_lighting",
            // structural: the remote operates the mixer, it does not rebuild it
            "add_channel", "remove_channel", "rename_channel", "reorder_channels",
            "set_channel_icon", "add_bus", "remove_bus", "rename_bus",
            "create_blank_profile", "delete_profile", "set_profile_trigger",
            "rename_app", "forget_app",
            // the tablet does not configure the remote
            "remote_status", "remote_set_enabled", "remote_regenerate_token",
        ];

        let unexplained: Vec<String> = called_by_frontend()
            .into_iter()
            .filter(|c| !is_allowed(c) && !GATED_IN_THE_UI.contains(&c.as_str()))
            .collect();
        assert!(
            unexplained.is_empty(),
            "the UI can send these but the remote denies them, and nothing says that is intended:\n  {unexplained:?}\n\
             Either add each to the allowlist, or hide its control behind `isTauri` and list it in GATED_IN_THE_UI."
        );
    }
}
