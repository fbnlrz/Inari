//! Tauri commands for SteelSeries Apex keyboards.

use serde::Serialize;
use tauri::State;

use crate::keyboard::effects::{EffectKind, Lighting};
use crate::keyboard::firmware::{self, KeyActuation};
use crate::keyboard::keys::{self, Key, Physical};
use crate::keyboard::manager::KeyboardStatus;
use crate::keyboard::oled::{self, ALL_WIRES};
use crate::keyboard::oled_modes::{KbMode, ALL_MODES};
use crate::keyboard::protocol::{self, FormFactor, OledWire};
use crate::keyboard::scenes::{Scene, ALL_SCENES};
use crate::persistence::keyboard::KeyboardConfig;
use crate::state::AppState;

/// One selectable OLED transport, for the picker.
#[derive(Serialize)]
pub struct WireOption {
    pub wire: OledWire,
    pub label: &'static str,
}

/// One themed scene, for the picker.
#[derive(Serialize)]
pub struct SceneOption {
    pub scene: Scene,
    pub label: &'static str,
    pub hint: &'static str,
}

/// One OLED mode, for the picker.
#[derive(Serialize)]
pub struct ModeOption {
    pub mode: KbMode,
    pub label: &'static str,
}

/// Everything the keyboard screen needs in one round trip.
#[derive(Serialize)]
pub struct KeyboardSnapshot {
    pub connected: bool,
    pub status: KeyboardStatus,
    pub config: KeyboardConfig,
    /// The key layout to draw, already positioned.
    pub layout: Vec<Key>,
    /// Board size in key units, for the aspect ratio.
    pub extent: (f32, f32),
    /// Colour each key currently has, keyed by HID id — the preview.
    pub preview: Vec<(u8, [u8; 3])>,
    pub oled_modes: Vec<ModeOption>,
    pub oled_wires: Vec<WireOption>,
    pub scenes: Vec<SceneOption>,
}

/// The layout to draw: the connected keyboard's, or a TKL as a stand-in so the
/// screen is not empty before anything is plugged in.
fn layout_for(status: &KeyboardStatus, physical: Physical) -> (Vec<Key>, FormFactor) {
    let form = status.form.unwrap_or(FormFactor::Tkl);
    (keys::layout(form, physical), form)
}

#[tauri::command]
pub fn get_keyboard_status(state: State<'_, AppState>) -> KeyboardSnapshot {
    let status = state.keyboard.status();
    let config = state.keyboard.config();
    let (layout, form) = layout_for(&status, config.physical);
    let frame = crate::keyboard::effects::render(&config.lighting, form, config.physical, 0.0, 0.0);
    let preview = layout
        .iter()
        .map(|key| {
            let color = protocol::key_index(key.hid)
                .and_then(|i| frame.get(i).copied())
                .unwrap_or([0, 0, 0]);
            (key.hid, color)
        })
        .collect();
    KeyboardSnapshot {
        connected: state.keyboard.is_connected(),
        status,
        extent: keys::extent(&layout),
        layout,
        preview,
        config,
        oled_modes: ALL_MODES
            .iter()
            .map(|mode| ModeOption {
                mode: *mode,
                label: mode.label(),
            })
            .collect(),
        oled_wires: ALL_WIRES
            .iter()
            .map(|(wire, label)| WireOption { wire: *wire, label })
            .collect(),
        scenes: ALL_SCENES
            .iter()
            .map(|(scene, label, hint)| SceneOption {
                scene: *scene,
                label,
                hint,
            })
            .collect(),
    }
}

/// Master switch. Off hands the LEDs back to the keyboard's own profile.
#[tauri::command]
pub fn keyboard_set_enabled(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    state.keyboard.update_config(|c| c.enabled = enabled)
}

/// Replace the whole lighting configuration. One command rather than a dozen
/// setters: the effect, its colours and its speed are edited together, and the
/// draw loop only ever reads the finished state.
#[tauri::command]
pub fn keyboard_set_lighting(state: State<'_, AppState>, lighting: Lighting) -> Result<(), String> {
    state.keyboard.update_config(|c| c.lighting = lighting)
}

/// Paint (or clear) one key. `rgb` of `None` removes it from the map, which is
/// how the UI's eraser works.
#[tauri::command]
pub fn keyboard_set_key_color(
    state: State<'_, AppState>,
    hid: u8,
    rgb: Option<[u8; 3]>,
) -> Result<(), String> {
    if protocol::key_index(hid).is_none() {
        return Err(format!("key {hid:#04x} cannot be addressed"));
    }
    state.keyboard.update_config(|c| {
        match rgb {
            Some(color) => c.lighting.per_key.insert(hid, color),
            None => c.lighting.per_key.remove(&hid),
        };
        // Painting a key is the user asking for their painting to be shown.
        c.lighting.kind = EffectKind::PerKey;
    })
}

/// Paint every key at once — the UI's "fill" button.
#[tauri::command]
pub fn keyboard_fill_keys(state: State<'_, AppState>, rgb: [u8; 3]) -> Result<(), String> {
    state.keyboard.update_config(|c| {
        for hid in protocol::KEY_ORDER {
            c.lighting.per_key.insert(hid, rgb);
        }
        c.lighting.kind = EffectKind::PerKey;
    })
}

/// Drop the whole per-key painting.
#[tauri::command]
pub fn keyboard_clear_keys(state: State<'_, AppState>) -> Result<(), String> {
    state.keyboard.update_config(|c| c.lighting.per_key.clear())
}

/// Which physical layout to draw (ANSI or ISO).
#[tauri::command]
pub fn keyboard_set_physical(state: State<'_, AppState>, physical: Physical) -> Result<(), String> {
    state.keyboard.update_config(|c| c.physical = physical)
}

/// What the OLED shows. `None` hands the panel back to the keyboard.
#[tauri::command]
pub fn keyboard_set_oled(
    state: State<'_, AppState>,
    mode: Option<KbMode>,
    text: Option<Vec<String>>,
) -> Result<(), String> {
    state.keyboard.update_config(|c| {
        c.oled_mode = mode;
        if let Some(text) = text {
            c.oled_text = text;
        }
    })
}

/// Blank the panel after this many idle minutes; 0 keeps it lit.
#[tauri::command]
pub fn keyboard_set_screensaver(state: State<'_, AppState>, minutes: u16) -> Result<(), String> {
    state
        .keyboard
        .update_config(|c| c.oled_screensaver_minutes = minutes)
}

/// Pin the OLED transport. `None` goes back to the model's default.
#[tauri::command]
pub fn keyboard_set_oled_wire(
    state: State<'_, AppState>,
    wire: Option<OledWire>,
) -> Result<(), String> {
    state.keyboard.reset_oled_choice();
    state.keyboard.update_config(|c| c.oled_wire = wire)
}

/// Send a test frame with a given transport, so the user can find the one their
/// board understands by looking at it.
///
/// The frame is deliberately unmistakable — a border, a diagonal cross and a
/// filled corner block — because the failure modes are "nothing", "noise" and
/// "blank", and all three have to be told apart by eye.
#[tauri::command]
pub fn keyboard_oled_test(state: State<'_, AppState>, wire: OledWire) -> Result<(), String> {
    let model = state.keyboard.status().product_id;
    let model = protocol::model(model).ok_or("no SteelSeries keyboard connected")?;
    let fb = test_pattern();
    for packet in oled::frame_packets(&fb, wire, model.feature_len) {
        state.keyboard.send_feature(&packet)?;
    }
    Ok(())
}

/// The frame [`keyboard_oled_test`] sends.
fn test_pattern() -> crate::headset::oled::Framebuffer {
    use crate::headset::oled::WIDTH;
    let mut fb = oled::framebuffer();
    let h = oled::HEIGHT;
    fb.stroke_rect(0, 0, WIDTH, h);
    fb.fill_rect(3, 3, 10, 10, true);
    for x in 0..WIDTH as isize {
        let y = x * (h as isize - 1) / (WIDTH as isize - 1);
        fb.set(x, y, true);
        fb.set(x, h as isize - 1 - y, true);
    }
    fb
}

/// Every key the connected board draws, for the per-key tables. The device
/// takes a `num_keys` count and honours it, so sending exactly the keys that
/// exist is both correct and smaller than padding to 70.
fn all_keys(state: &AppState) -> Vec<u8> {
    let status = state.keyboard.status();
    let config = state.keyboard.config();
    let form = status.form.unwrap_or(FormFactor::Tkl);
    keys::layout(form, config.physical)
        .into_iter()
        .map(|k| k.hid)
        .collect()
}

/// Send the actuation table the current configuration describes: the global
/// value everywhere, with per-key overrides on top.
/// Write the actuation table.
///
/// `0x2F` addresses keys individually, so when there is no global setting the
/// overrides are sent on their own. Inventing a board-wide baseline to hang
/// them off would silently move every other key — painting one key would
/// change the whole keyboard's feel.
/// Falls back only for keys that carry an override while nothing global is
/// set — in that case it is never actually used, but the table builder wants a
/// number. 1.5 mm is the hardware-verified normal travel.
const DEFAULT_ACTUATION: u8 = 15;

fn push_actuation(state: &AppState) -> Result<(), String> {
    let config = state.keyboard.config();
    let hids: Vec<u8> = match config.actuation {
        Some(_) => all_keys(state),
        None if !config.per_key_actuation.is_empty() => {
            let mut keys: Vec<u8> = config.per_key_actuation.keys().copied().collect();
            keys.sort_unstable();
            keys
        }
        None => return Ok(()),
    };
    let global = config.actuation.unwrap_or(DEFAULT_ACTUATION);
    // Built inside the closure: travel-to-wire is per model, because the wired
    // Gen 3 boards want raw sensor counts rather than tenths of a millimetre.
    state.keyboard.send_feature_with_model(|model| {
        let table: Vec<KeyActuation> = hids
            .into_iter()
            .map(|hid| {
                let tenths = config
                    .per_key_actuation
                    .get(&hid)
                    .copied()
                    .unwrap_or(global);
                KeyActuation::uniform(model, hid, tenths)
            })
            .collect();
        firmware::set_actuation(model, &table, 0)
    })
}

/// Global actuation point, in tenths of a millimetre (1 = 0.1 mm).
///
/// Verified on hardware: 40 makes a key trigger visibly later, 15 restores
/// normal behaviour. Per-key overrides are layered on top of this.
#[tauri::command]
pub fn keyboard_set_actuation(state: State<'_, AppState>, tenths: u8) -> Result<(), String> {
    if !(firmware::ACTUATION_MIN..=firmware::ACTUATION_MAX).contains(&tenths) {
        return Err("actuation must be between 0.1 mm and 4.0 mm".into());
    }
    state
        .keyboard
        .update_config(|c| c.actuation = Some(tenths))?;
    push_actuation(&state)
}

/// Override one key's actuation, or drop the override with `None`.
#[tauri::command]
pub fn keyboard_set_key_actuation(
    state: State<'_, AppState>,
    hid: u8,
    tenths: Option<u8>,
) -> Result<(), String> {
    if let Some(tenths) = tenths {
        if !(firmware::ACTUATION_MIN..=firmware::ACTUATION_MAX).contains(&tenths) {
            return Err("actuation must be between 0.1 mm and 4.0 mm".into());
        }
    }
    state.keyboard.update_config(|c| {
        match tenths {
            Some(value) => c.per_key_actuation.insert(hid, value),
            None => c.per_key_actuation.remove(&hid),
        };
    })?;
    push_actuation(&state)
}

/// Rapid Trigger for every key; 0 turns it off.
///
/// Two commands, not one: the sensitivity table says how far a key must travel
/// back before it re-arms, and the release-mode table is what actually enables
/// the feature per key. Sending only the sensitivity looks right and does
/// nothing — which is the kind of bug that gets blamed on the hardware.
#[tauri::command]
pub fn keyboard_set_rapid_trigger(
    state: State<'_, AppState>,
    sensitivity: u8,
) -> Result<(), String> {
    state
        .keyboard
        .update_config(|c| c.rapid_trigger = sensitivity)?;
    let hids = all_keys(&state);
    let for_sensitivity = hids.clone();
    state.keyboard.send_feature_with_model(|model| {
        let table: Vec<(u8, u8)> = for_sensitivity
            .into_iter()
            .map(|hid| (hid, sensitivity))
            .collect();
        firmware::set_rapid_trigger_sensitivity(model, &table)
    })?;
    state.keyboard.send_feature_with_model(|model| {
        let on = if sensitivity == 0 {
            0
        } else {
            firmware::rapid_trigger_on_value(model)
        };
        let table: Vec<(u8, u8)> = hids.into_iter().map(|hid| (hid, on)).collect();
        firmware::set_release_mode(model, &table)
    })
}

/// Protection Mode across the board.
#[tauri::command]
pub fn keyboard_set_protection_mode(state: State<'_, AppState>, on: bool) -> Result<(), String> {
    state.keyboard.update_config(|c| c.protection_mode = on)?;
    let table: Vec<(u8, bool)> = all_keys(&state).into_iter().map(|hid| (hid, on)).collect();
    state
        .keyboard
        .send_feature_with_model(|model| firmware::set_protection_mode(model, &table))
}

/// Rapid Tap / SOCD master switch.
#[tauri::command]
pub fn keyboard_set_rapid_tap(state: State<'_, AppState>, on: bool) -> Result<(), String> {
    state.keyboard.update_config(|c| c.rapid_tap = on)?;
    state.keyboard.send_control(&firmware::set_rapid_tap(on))
}

/// The keyboard's own idle dimming — the timer that actually controls the
/// board, as opposed to Inari's OLED blanking.
#[tauri::command]
pub fn keyboard_set_idle(
    state: State<'_, AppState>,
    seconds: u32,
    brightness: u8,
) -> Result<(), String> {
    state.keyboard.update_config(|c| {
        c.idle_timeout_secs = seconds;
        c.idle_brightness = brightness;
    })?;
    state
        .keyboard
        .send_control(&firmware::set_lighting_config(
            None,
            Some(brightness),
            Some(seconds * 1000),
        ))?;
    // The board is not re-read after a write, so mirror what it now holds;
    // otherwise the next status event restores the connect-time snapshot.
    state.keyboard.patch_status(|s| {
        if let Some(lighting) = s.lighting.as_mut() {
            lighting.idle_brightness = brightness;
            lighting.idle_timeout_ms = seconds * 1000;
        }
    });
    Ok(())
}

/// Sleep timeout and high-efficiency mode.
#[tauri::command]
pub fn keyboard_set_power_saving(
    state: State<'_, AppState>,
    minutes: u32,
    high_efficiency: bool,
) -> Result<(), String> {
    state.keyboard.update_config(|c| {
        c.sleep_minutes = minutes;
        c.high_efficiency = high_efficiency;
    })?;
    state
        .keyboard
        .send_control(&firmware::set_power_saving(minutes * 60_000, high_efficiency))?;
    state.keyboard.patch_status(|s| {
        if let Some(power) = s.power.as_mut() {
            power.timeout_ms = minutes * 60_000;
            power.high_efficiency = high_efficiency;
        }
    });
    Ok(())
}

/// Switch the keyboard to one of its onboard profiles.
#[tauri::command]
pub fn keyboard_select_profile(state: State<'_, AppState>, index: u8) -> Result<(), String> {
    state
        .keyboard
        .send_control(&protocol::select_profile(index))
}

/// Send an arbitrary command — the experimental probe.
///
/// This exists because Rapid Trigger, Protection Mode and Rapid Tap have never
/// been captured by anyone, and the only way anybody will find them is with a
/// keyboard in front of them. Payload bytes are decimal or hex (`0x..`).
#[tauri::command]
pub fn keyboard_send_raw(
    state: State<'_, AppState>,
    cmd: u8,
    payload: Vec<u8>,
) -> Result<(), String> {
    if payload.len() > 62 {
        return Err("a command carries at most 62 payload bytes".into());
    }
    state.keyboard.send_control(&protocol::raw(cmd, &payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_pattern_marks_all_four_corners_and_the_middle() {
        let fb = test_pattern();
        assert_eq!(fb.height(), oled::HEIGHT);
        assert!(fb.get(0, 0), "top left corner of the border");
        assert!(fb.get(127, 0));
        assert!(fb.get(0, oled::HEIGHT - 1));
        assert!(fb.get(127, oled::HEIGHT - 1));
        assert!(fb.get(5, 5), "the filled block");
        // The two diagonals cross in the middle.
        assert!(fb.get(64, 20) || fb.get(63, 19) || fb.get(64, 19));
    }

    #[test]
    fn every_scene_reaches_the_picker() {
        // A scene missing from the snapshot is a scene the user cannot select.
        assert_eq!(ALL_SCENES.len(), 24);
        for (_, label, hint) in ALL_SCENES {
            assert!(!label.is_empty() && !hint.is_empty());
        }
    }

    #[test]
    fn the_stand_in_layout_is_a_tkl_until_a_board_is_found() {
        let (layout, form) = layout_for(&KeyboardStatus::default(), Physical::Iso);
        assert_eq!(form, FormFactor::Tkl);
        assert!(!layout.is_empty());
        // A connected full-size board gets its own numpad.
        let full = KeyboardStatus {
            form: Some(FormFactor::Full),
            ..KeyboardStatus::default()
        };
        let (layout, form) = layout_for(&full, Physical::Iso);
        assert_eq!(form, FormFactor::Full);
        assert!(layout.iter().any(|k| k.hid == 0x62), "numpad 0 is missing");
    }
}
