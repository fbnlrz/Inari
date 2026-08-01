//! Physical key layouts for the Apex boards.
//!
//! The wire format addresses keys by HID usage id and is the same for every
//! model ([`super::protocol::KEY_ORDER`]) — but Inari also has to *draw* the
//! keyboard and run effects that sweep across it, and for that it needs to
//! know where each key sits. That is what this module holds: one table per
//! form factor and physical layout, in key units (1.0 = one 1u key), which the
//! UI scales to pixels and the effect engine normalises to 0..1.
//!
//! Only the lighting matters here, so a layout is allowed to be approximate
//! about keys that carry no LED (the volume roller, the OLED button).

use serde::Serialize;

use super::protocol::{key_index, FormFactor, KEY_ORDER};

/// Widths and offsets are quarter key units so the tables stay integers:
/// 4 = 1u, 5 = 1.25u, 25 = 6.25u (space).
const U: u16 = 4;

/// One drawable key.
#[derive(Debug, Clone, Serialize)]
pub struct Key {
    /// HID usage id — the address the keyboard lights by.
    pub hid: u8,
    /// Caption drawn on the key.
    pub label: &'static str,
    /// Left edge and top edge in key units.
    pub x: f32,
    pub y: f32,
    /// Size in key units.
    pub w: f32,
    pub h: f32,
}

/// Physical layout: which keys are fitted and how big they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Physical {
    /// US ANSI: horizontal Enter, full-width left Shift.
    Ansi,
    /// ISO (German captions): tall Enter, extra key left of Y and right of Ä.
    #[default]
    Iso,
}

/// A slot in a row: either a key or empty space.
enum Slot {
    K(u8, &'static str, u16),
    /// A taller key (numpad +, numpad Enter, ISO Enter): width, height.
    Tall(u8, &'static str, u16, u16),
    Gap(u16),
}

use Slot::{Gap, Tall, K};

/// Turn rows of slots into positioned keys. `origin` shifts a whole block
/// (the nav cluster and the numpad start part-way across the board).
fn flow(rows: &[(f32, &[Slot])], out: &mut Vec<Key>) {
    for (row, slots) in rows {
        let mut x = 0.0f32;
        for slot in *slots {
            match slot {
                Gap(w) => x += *w as f32 / U as f32,
                K(hid, label, w) => {
                    let w = *w as f32 / U as f32;
                    out.push(Key {
                        hid: *hid,
                        label,
                        x,
                        y: *row,
                        w,
                        h: 1.0,
                    });
                    x += w;
                }
                Tall(hid, label, w, h) => {
                    let (w, h) = (*w as f32 / U as f32, *h as f32 / U as f32);
                    out.push(Key {
                        hid: *hid,
                        label,
                        x,
                        y: *row,
                        w,
                        h,
                    });
                    x += w;
                }
            }
        }
    }
}

// --- HID usage ids, named so the tables read like a keyboard -------------

const ESC: u8 = 0x29;
const BACKSPACE: u8 = 0x2a;
const TAB: u8 = 0x2b;
const CAPS: u8 = 0x39;
const ENTER: u8 = 0x28;
const SPACE: u8 = 0x2c;
const LSHIFT: u8 = 0xe1;
const RSHIFT: u8 = 0xe5;
const LCTRL: u8 = 0xe0;
const RCTRL: u8 = 0xe4;
const LALT: u8 = 0xe2;
const RALT: u8 = 0xe6;
const LGUI: u8 = 0xe3;
const RGUI: u8 = 0xe7;
/// The SteelSeries / Fn key.
const FN: u8 = 0xf0;
/// ISO's extra key left of Y/Z.
const ISO_LT: u8 = 0x64;
/// ISO's extra key right of Ä (`#`).
const ISO_HASH: u8 = 0x32;

/// The function row plus the print-screen cluster, shared by every layout.
fn function_row() -> Vec<Slot> {
    vec![
        K(ESC, "Esc", U),
        Gap(U),
        K(0x3a, "F1", U),
        K(0x3b, "F2", U),
        K(0x3c, "F3", U),
        K(0x3d, "F4", U),
        Gap(2),
        K(0x3e, "F5", U),
        K(0x3f, "F6", U),
        K(0x40, "F7", U),
        K(0x41, "F8", U),
        Gap(2),
        K(0x42, "F9", U),
        K(0x43, "F10", U),
        K(0x44, "F11", U),
        K(0x45, "F12", U),
        Gap(1),
        K(0x46, "Prt", U),
        K(0x47, "Scr", U),
        K(0x48, "Pse", U),
    ]
}

/// Number row: identical positions in both layouts, different captions.
fn number_row(iso: bool) -> Vec<Slot> {
    let (grave, minus, equal) = if iso {
        ("^", "ß", "´")
    } else {
        ("`", "-", "=")
    };
    vec![
        K(0x35, grave, U),
        K(0x1e, "1", U),
        K(0x1f, "2", U),
        K(0x20, "3", U),
        K(0x21, "4", U),
        K(0x22, "5", U),
        K(0x23, "6", U),
        K(0x24, "7", U),
        K(0x25, "8", U),
        K(0x26, "9", U),
        K(0x27, "0", U),
        K(0x2d, minus, U),
        K(0x2e, equal, U),
        K(BACKSPACE, "⌫", 2 * U),
        Gap(1),
        K(0x49, "Ins", U),
        K(0x4a, "Hm", U),
        K(0x4b, "PgU", U),
    ]
}

fn alpha_rows(iso: bool) -> Vec<Vec<Slot>> {
    // Y and Z swap on the German layout; the usage ids stay put.
    let (y_key, z_key) = if iso { ("Z", "Y") } else { ("Y", "Z") };
    let mut top = vec![
        K(TAB, "Tab", 6),
        K(0x14, "Q", U),
        K(0x1a, "W", U),
        K(0x08, "E", U),
        K(0x15, "R", U),
        K(0x17, "T", U),
        K(0x1c, y_key, U),
        K(0x18, "U", U),
        K(0x0c, "I", U),
        K(0x12, "O", U),
        K(0x13, "P", U),
    ];
    let mut home = vec![
        K(CAPS, "Caps", 7),
        K(0x04, "A", U),
        K(0x16, "S", U),
        K(0x07, "D", U),
        K(0x09, "F", U),
        K(0x0a, "G", U),
        K(0x0b, "H", U),
        K(0x0d, "J", U),
        K(0x0e, "K", U),
        K(0x0f, "L", U),
    ];
    let mut bottom = vec![K(LSHIFT, "⇧", if iso { 5 } else { 9 })];
    if iso {
        bottom.push(K(ISO_LT, "<", U));
    }
    bottom.extend([
        K(0x1d, z_key, U),
        K(0x1b, "X", U),
        K(0x06, "C", U),
        K(0x19, "V", U),
        K(0x05, "B", U),
        K(0x11, "N", U),
        K(0x10, "M", U),
        K(0x36, ",", U),
        K(0x37, ".", U),
        K(0x38, if iso { "-" } else { "/" }, U),
        K(RSHIFT, "⇧", 11),
        Gap(1),
        Gap(U),
        K(0x52, "↑", U),
    ]);

    if iso {
        top.extend([
            K(0x2f, "Ü", U),
            K(0x30, "+", U),
            // ISO Enter is tall: it starts here and reaches into the home row.
            Tall(ENTER, "⏎", 6, 2 * U),
            Gap(1),
            K(0x4c, "Del", U),
            K(0x4d, "End", U),
            K(0x4e, "PgD", U),
        ]);
        home.extend([K(0x33, "Ö", U), K(0x34, "Ä", U), K(ISO_HASH, "#", U)]);
    } else {
        top.extend([
            K(0x2f, "[", U),
            K(0x30, "]", U),
            K(0x31, "\\", 6),
            Gap(1),
            K(0x4c, "Del", U),
            K(0x4d, "End", U),
            K(0x4e, "PgD", U),
        ]);
        home.extend([K(0x33, ";", U), K(0x34, "'", U), K(ENTER, "⏎", 9)]);
    }
    vec![top, home, bottom]
}

fn modifier_row() -> Vec<Slot> {
    vec![
        K(LCTRL, "Ctrl", 5),
        K(LGUI, "⊞", 5),
        K(LALT, "Alt", 5),
        K(SPACE, "", 25),
        K(RALT, "Alt", 5),
        K(FN, "SS", 5),
        K(RGUI, "≡", 5),
        K(RCTRL, "Ctrl", 5),
        Gap(1),
        K(0x50, "←", U),
        K(0x51, "↓", U),
        K(0x4f, "→", U),
    ]
}

/// The numpad, offset to sit right of the nav cluster.
fn numpad(out: &mut Vec<Key>) {
    const X: f32 = 18.5;
    let pad: &[(u8, &str, f32, f32, f32, f32)] = &[
        (0x53, "Num", 0.0, 1.0, 1.0, 1.0),
        (0x54, "/", 1.0, 1.0, 1.0, 1.0),
        (0x55, "*", 2.0, 1.0, 1.0, 1.0),
        (0x56, "-", 3.0, 1.0, 1.0, 1.0),
        (0x5f, "7", 0.0, 2.0, 1.0, 1.0),
        (0x60, "8", 1.0, 2.0, 1.0, 1.0),
        (0x61, "9", 2.0, 2.0, 1.0, 1.0),
        (0x57, "+", 3.0, 2.0, 1.0, 2.0),
        (0x5c, "4", 0.0, 3.0, 1.0, 1.0),
        (0x5d, "5", 1.0, 3.0, 1.0, 1.0),
        (0x5e, "6", 2.0, 3.0, 1.0, 1.0),
        (0x59, "1", 0.0, 4.0, 1.0, 1.0),
        (0x5a, "2", 1.0, 4.0, 1.0, 1.0),
        (0x5b, "3", 2.0, 4.0, 1.0, 1.0),
        (0x58, "⏎", 3.0, 4.0, 1.0, 2.0),
        (0x62, "0", 0.0, 5.0, 2.0, 1.0),
        (0x63, ",", 2.0, 5.0, 1.0, 1.0),
    ];
    for (hid, label, x, y, w, h) in pad {
        out.push(Key {
            hid: *hid,
            label,
            x: X + x,
            y: *y,
            w: *w,
            h: *h,
        });
    }
}

/// Every key of one board, in draw order.
pub fn layout(form: FormFactor, physical: Physical) -> Vec<Key> {
    let iso = physical == Physical::Iso;
    let mut out = Vec::new();
    let alpha = alpha_rows(iso);
    let functions = function_row();
    let numbers = number_row(iso);
    let modifiers = modifier_row();
    if form == FormFactor::Mini {
        // The 9 Mini has no function row, no nav cluster and no arrows of its
        // own; everything else is the 60 % core, so each row stops before the
        // cluster the board does not have.
        let rows: Vec<(f32, &[Slot])> = vec![
            (0.0, &numbers[..14]),
            (1.0, &alpha[0]),
            (2.0, &alpha[1]),
            (3.0, &alpha[2]),
            (4.0, &modifiers[..8]),
        ];
        flow(&rows, &mut out);
        return out;
    }

    let rows: Vec<(f32, &[Slot])> = vec![
        (0.0, &functions),
        (1.0, &numbers),
        (2.0, &alpha[0]),
        (3.0, &alpha[1]),
        (4.0, &alpha[2]),
        (5.0, &modifiers),
    ];
    flow(&rows, &mut out);
    if form == FormFactor::Full {
        numpad(&mut out);
    }
    out
}

/// Board size in key units, for the UI's aspect ratio.
pub fn extent(keys: &[Key]) -> (f32, f32) {
    keys.iter().fold((0.0f32, 0.0f32), |(w, h), k| {
        (w.max(k.x + k.w), h.max(k.y + k.h))
    })
}

/// Normalised (0..1) centre of every wire-order key, for effects that sweep
/// across the board. Keys the layout does not fit — the ambient/logo slot, and
/// the international keys on a US board — fall back to the middle, so a wave
/// never leaves them stuck on one colour.
pub fn positions(form: FormFactor, physical: Physical) -> [(f32, f32); KEY_ORDER.len()] {
    let keys = layout(form, physical);
    let (w, h) = extent(&keys);
    let (w, h) = (w.max(1.0), h.max(1.0));
    let mut out = [(0.5f32, 0.5f32); KEY_ORDER.len()];
    for key in &keys {
        if let Some(i) = key_index(key.hid) {
            out[i] = ((key.x + key.w / 2.0) / w, (key.y + key.h / 2.0) / h);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hids(keys: &[Key]) -> Vec<u8> {
        keys.iter().map(|k| k.hid).collect()
    }

    #[test]
    fn no_layout_lists_a_key_twice() {
        for form in [FormFactor::Full, FormFactor::Tkl, FormFactor::Mini] {
            for physical in [Physical::Ansi, Physical::Iso] {
                let mut ids = hids(&layout(form, physical));
                let total = ids.len();
                ids.sort_unstable();
                ids.dedup();
                assert_eq!(ids.len(), total, "{form:?}/{physical:?} repeats a key");
            }
        }
    }

    #[test]
    fn every_drawn_key_is_addressable_on_the_wire() {
        // A key the packet cannot carry would be dead in the UI: the user
        // could colour it and nothing would happen.
        for form in [FormFactor::Full, FormFactor::Tkl, FormFactor::Mini] {
            for physical in [Physical::Ansi, Physical::Iso] {
                for key in layout(form, physical) {
                    assert!(
                        key_index(key.hid).is_some(),
                        "{:#04x} ({}) is not in KEY_ORDER",
                        key.hid,
                        key.label
                    );
                }
            }
        }
    }

    #[test]
    fn rows_add_up_to_the_standard_width() {
        // Every main-block row of a TKL is 15u wide; the nav cluster starts a
        // quarter unit later. If a row drifts, the whole picture shears.
        let keys = layout(FormFactor::Tkl, Physical::Ansi);
        for row in 1..=5 {
            let end = keys
                .iter()
                .filter(|k| k.y as u8 == row && k.x < 15.0)
                .fold(0.0f32, |acc, k| acc.max(k.x + k.w));
            assert!((end - 15.0).abs() < 0.01, "row {row} ends at {end}u");
        }
        let (w, h) = extent(&keys);
        assert!((w - 18.25).abs() < 0.01, "TKL is {w}u wide");
        assert_eq!(h, 6.0);
    }

    #[test]
    fn iso_swaps_y_and_z_and_adds_two_keys() {
        let ansi = layout(FormFactor::Tkl, Physical::Ansi);
        let iso = layout(FormFactor::Tkl, Physical::Iso);
        assert_eq!(iso.len(), ansi.len() + 1, "ISO adds < and drops nothing");
        let label = |keys: &[Key], hid: u8| {
            keys.iter()
                .find(|k| k.hid == hid)
                .unwrap()
                .label
                .to_string()
        };
        assert_eq!(label(&ansi, 0x1c), "Y");
        assert_eq!(label(&iso, 0x1c), "Z");
        assert_eq!(label(&iso, 0x1d), "Y");
        assert!(iso.iter().any(|k| k.hid == ISO_LT));
        assert!(iso.iter().any(|k| k.hid == ISO_HASH));
        assert!(!ansi.iter().any(|k| k.hid == ISO_LT));
        // The ISO Enter is the tall one.
        let enter = iso.iter().find(|k| k.hid == ENTER).unwrap();
        assert_eq!(enter.h, 2.0);
    }

    #[test]
    fn full_size_adds_a_numpad_and_tkl_does_not() {
        let full = layout(FormFactor::Full, Physical::Iso);
        let tkl = layout(FormFactor::Tkl, Physical::Iso);
        assert!(full.iter().any(|k| k.hid == 0x62), "numpad 0 is missing");
        assert!(!tkl.iter().any(|k| k.hid == 0x62));
        assert_eq!(full.len(), tkl.len() + 17);
        let (w, _) = extent(&full);
        assert!((w - 22.5).abs() < 0.01, "full size is {w}u wide");
    }

    #[test]
    fn positions_are_normalised_and_ordered_left_to_right() {
        let pos = positions(FormFactor::Tkl, Physical::Ansi);
        for (x, y) in pos {
            assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
        }
        let esc = pos[key_index(ESC).unwrap()];
        let enter = pos[key_index(ENTER).unwrap()];
        assert!(esc.0 < enter.0, "Escape must sit left of Enter");
        assert!(esc.1 < enter.1, "Escape must sit above Enter");
        // The ambient slot has no place on the board and falls back to centre.
        assert_eq!(pos[key_index(0xfb).unwrap()], (0.5, 0.5));
    }
}
