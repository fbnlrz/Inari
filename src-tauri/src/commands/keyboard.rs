//! Tauri commands for SteelSeries Apex keyboards.

use serde::Serialize;
use tauri::State;

use crate::keyboard::effects::{EffectKind, Lighting};
use crate::keyboard::keys::{self, Key, Physical};
use crate::keyboard::manager::KeyboardStatus;
use crate::keyboard::oled::{self, ALL_WIRES};
use crate::keyboard::oled_modes::{KbMode, ALL_MODES};
use crate::keyboard::protocol::{self, FormFactor, OledWire};
use crate::persistence::keyboard::KeyboardConfig;
use crate::state::AppState;

/// One selectable OLED transport, for the picker.
#[derive(Serialize)]
pub struct WireOption {
    pub wire: OledWire,
    pub label: &'static str,
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

/// Global actuation point, in tenths of a millimetre.
///
/// Experimental on purpose: the command is accepted by the one board this could
/// be tried on and changes nothing there. The UI says so; this only refuses
/// values the switch could not travel to.
#[tauri::command]
pub fn keyboard_set_actuation(state: State<'_, AppState>, tenths: u8) -> Result<(), String> {
    if !(1..=40).contains(&tenths) {
        return Err("actuation must be between 0.1 mm and 4.0 mm".into());
    }
    state.keyboard.send_control(&protocol::actuation(tenths))?;
    state.keyboard.send_control(&protocol::apply())?;
    state.keyboard.update_config(|c| c.actuation = Some(tenths))
}

/// Per-key actuation. Same experimental caveat, plus one more: no capture of a
/// per-key actuation command exists at all, so this stores the value and sends
/// the global command's per-key form as a best guess.
#[tauri::command]
pub fn keyboard_set_key_actuation(
    state: State<'_, AppState>,
    hid: u8,
    tenths: Option<u8>,
) -> Result<(), String> {
    if let Some(tenths) = tenths {
        if !(1..=40).contains(&tenths) {
            return Err("actuation must be between 0.1 mm and 4.0 mm".into());
        }
        state
            .keyboard
            .send_control(&protocol::raw(0x2d, &[tenths, hid]))?;
    }
    state.keyboard.update_config(|c| {
        match tenths {
            Some(value) => c.per_key_actuation.insert(hid, value),
            None => c.per_key_actuation.remove(&hid),
        };
    })
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
