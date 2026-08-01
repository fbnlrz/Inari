//! Settings the keyboard's own firmware holds: switch behaviour, idle/dim
//! timers and power saving.
//!
//! These are the ones Inari long listed as "nobody has captured these" —
//! Rapid Trigger, Rapid Tap/SOCD, Protection Mode, per-key actuation. They do
//! exist as ordinary commands; they were simply not in any public
//! reverse-engineering write-up. The layouts here come from a specification
//! extracted from the vendor software, and the parts marked **verified** were
//! measured on an Apex Pro TKL Wireless (2023) on 2026-08-01.
//!
//! Two shapes recur and are worth naming:
//!
//! * **Per-key tables.** `[cmd][…][num_keys][entry × num_keys]`, each entry
//!   starting with the key's HID usage id. `num_keys` is honest — writing a
//!   single key works, which is what the actuation test did.
//! * **Field-at-a-time config.** `lighting_config` carries every lighting
//!   setting at once, each with its own `set_*` flag; only the fields whose
//!   flag is 1 are applied. So changing the idle timer cannot disturb the
//!   brightness, which is exactly what a naive "write the whole struct" port
//!   would do.
//!
//! Transport is **not** uniform here, and getting it wrong is silent:
//!
//! * The per-key tables are bulk payloads and go out as **feature** reports —
//!   70 keys is over 200 bytes and could not fit a 64-byte output report.
//! * The small config commands (`0x20` lighting, `0x29` power saving, `0x17`
//!   rapid tap) are ordinary **output** reports of 65 bytes. Their payloads are
//!   32, 7 and 1 bytes. Sent as feature reports the ioctl still succeeds — the
//!   length happens to be valid — and the firmware discards them, so the
//!   setting looks applied and is not. That is exactly the failure the idle
//!   timer was meant to fix, so it is worth being explicit about.

use serde::{Deserialize, Serialize};

use super::protocol::{feature, Model};

/// Per-key actuation table (`0x2F`, layer 0).
const CMD_HALL_THRESHOLDS: u8 = 0x2f;
/// Rapid Trigger sensitivity per key (`0x37`).
const CMD_RAPID_TRIGGER: u8 = 0x37;
/// Per-key release mode (`0x36`) — what actually switches Rapid Trigger on.
const CMD_RELEASE_MODE: u8 = 0x36;
/// Protection Mode release behaviour per key (`0x14`).
const CMD_PROTECTION_MODE: u8 = 0x14;
/// Rapid Tap / SOCD on/off (`0x17`).
const CMD_RAPID_TAP_ENABLE: u8 = 0x17;
/// Lighting configuration, field-at-a-time (`0x20`).
const CMD_LIGHTING_CONFIG: u8 = 0x20;
/// Power saving: sleep timeout and high-efficiency mode (`0x29`).
const CMD_POWER_SAVING: u8 = 0x29;
/// Read back the lighting configuration (`0xA0`).
const CMD_READ_LIGHTING: u8 = 0xa0;
/// Read back power saving (`0xA9`).
const CMD_READ_POWER: u8 = 0xa9;

/// Actuation travel, in tenths of a millimetre.
///
/// **Verified**: writing 40 to a single key made it trigger visibly later, and
/// 15 restored normal behaviour — so the unit is 0.1 mm and the switch's range
/// is its full travel.
pub const ACTUATION_MIN: u8 = 1;
pub const ACTUATION_MAX: u8 = 40;

/// One key's actuation, as the wire carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyActuation {
    /// HID usage id.
    pub hid: u8,
    /// Press point, in tenths of a millimetre.
    pub press: u8,
    /// Release point. Equal to `press` unless a separate reset point is wanted.
    pub release: u8,
}

impl KeyActuation {
    /// One key at a given travel, in tenths of a millimetre.
    ///
    /// `model` matters: the wireless boards take the travel directly, while the
    /// wired Gen 3 boards want **raw sensor counts** instead. Sending tenths to
    /// those would ask for a tenth of the intended depth.
    pub fn uniform(model: &Model, hid: u8, tenths: u8) -> Self {
        let level = tenths.clamp(ACTUATION_MIN, ACTUATION_MAX);
        if model.raw_actuation {
            let (press, release) = raw_from_level(level);
            Self {
                hid,
                press,
                release,
            }
        } else {
            Self {
                hid,
                press: level,
                release: level,
            }
        }
    }
}

/// The wired Gen 3 boards' actuation table: level 1..40 (0.1..4.0 mm) to the
/// raw sensor pair they expect, transcribed from the vendor software.
///
/// Worth keeping as a table rather than a formula: the curve is far from
/// linear — level 20 is 50, not the ~100 a straight line between the ends
/// would give — and the last few rows are not even monotonic (37 → 201,
/// 38 → 197, 39 → 199, 40 → 201). Those are the vendor's numbers, quirks
/// included, and guessing them would put a key at twice the intended depth.
const GEN3_ACTUATION: [(u8, u8); 40] = [
    (5, 5),
    (4, 5),
    (5, 6),
    (6, 8),
    (8, 10),
    (10, 12),
    (12, 14),
    (14, 16),
    (16, 18),
    (18, 21),
    (20, 23),
    (23, 26),
    (25, 28),
    (28, 31),
    (31, 34),
    (34, 38),
    (38, 42),
    (42, 46),
    (46, 50),
    (50, 55),
    (55, 60),
    (60, 65),
    (65, 71),
    (71, 77),
    (77, 84),
    (84, 91),
    (91, 99),
    (99, 108),
    (108, 118),
    (118, 128),
    (128, 134),
    (139, 151),
    (152, 165),
    (165, 180),
    (184, 188),
    (192, 197),
    (201, 206),
    (197, 215),
    (199, 215),
    (201, 215),
];

/// Actuation level (1..40) to the raw sensor pair the wired Gen 3 boards want.
fn raw_from_level(level: u8) -> (u8, u8) {
    let index = level.clamp(ACTUATION_MIN, ACTUATION_MAX) as usize - 1;
    GEN3_ACTUATION[index]
}

/// Per-key actuation. `layer` 0 is the actuation point, 1 the second
/// actuation ("dual-step") point.
pub fn set_actuation(model: &Model, keys: &[KeyActuation], layer: u8) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + keys.len() * 3);
    payload.push(layer);
    payload.push(keys.len() as u8);
    for key in keys {
        // Already in the model's own units — clamping to the millimetre range
        // here would break the raw-sensor boards.
        payload.push(key.hid);
        payload.push(key.press);
        payload.push(key.release);
    }
    feature(model, CMD_HALL_THRESHOLDS, &payload)
}

/// Rapid Trigger takes **two** commands, and sending only the first is the
/// mistake worth naming: `0x37` sets the movement threshold per key, but the
/// feature is switched on per key by the release mode (`0x36`). A sensitivity
/// alone changes nothing.
///
/// The "on" value differs by transport: 1 on the wireless boards, 2 on the
/// wired ones. 0 is fixed actuation, i.e. Rapid Trigger off.
pub fn rapid_trigger_on_value(model: &Model) -> u8 {
    if model.wireless {
        1
    } else {
        2
    }
}

/// The sensitivity table (`0x37`).
pub fn set_rapid_trigger_sensitivity(model: &Model, keys: &[(u8, u8)]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1 + keys.len() * 2);
    payload.push(keys.len() as u8);
    for (hid, sensitivity) in keys {
        payload.push(*hid);
        payload.push(*sensitivity);
    }
    feature(model, CMD_RAPID_TRIGGER, &payload)
}

/// The per-key on/off table (`0x36`). Same shape as Protection Mode, different
/// command and different meaning — the specification is explicit that these two
/// share a struct and nothing else.
pub fn set_release_mode(model: &Model, keys: &[(u8, u8)]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1 + keys.len() * 2);
    payload.push(keys.len() as u8);
    for (hid, mode) in keys {
        payload.push(*hid);
        payload.push(*mode);
    }
    feature(model, CMD_RELEASE_MODE, &payload)
}

/// Protection Mode per key: `true` gives the key the protected release
/// behaviour that suppresses accidental neighbours.
pub fn set_protection_mode(model: &Model, keys: &[(u8, bool)]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1 + keys.len() * 2);
    payload.push(keys.len() as u8);
    for (hid, on) in keys {
        payload.push(*hid);
        payload.push(u8::from(*on));
    }
    feature(model, CMD_PROTECTION_MODE, &payload)
}

/// Rapid Tap / SOCD master switch.
///
/// The pair table (`0x18`, ten four-byte slots of `[hid][hid][mode][report
/// both]`) is specified but not exposed: without a pair editor in the UI the
/// command has nothing to say, so enabling Rapid Tap uses whatever pairs the
/// keyboard's own profile already holds.
pub fn set_rapid_tap(enabled: bool) -> Vec<u8> {
    super::protocol::raw(CMD_RAPID_TAP_ENABLE, &[u8::from(enabled)])
}

/// The lighting settings the firmware keeps, as read back by [`parse_lighting`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LightingConfig {
    /// 0..10.
    pub brightness: u8,
    /// Brightness after the idle timeout expires, 0..10.
    pub idle_brightness: u8,
    /// Milliseconds of inactivity before dimming; 0 never dims.
    pub idle_timeout_ms: u32,
}

/// Change lighting settings one field at a time.
///
/// Every field carries its own apply flag, so passing `None` genuinely leaves
/// that setting alone rather than rewriting it with a stale value.
pub fn set_lighting_config(
    brightness: Option<u8>,
    idle_brightness: Option<u8>,
    idle_timeout_ms: Option<u32>,
) -> Vec<u8> {
    // Layout after the command byte:
    //   0 brightness, 1 set, 2 idle_brightness, 3 set,
    //   4..8 idle_timeout u32, 8 set
    let mut payload = vec![0u8; 32];
    if let Some(v) = brightness {
        payload[0] = v.min(10);
        payload[1] = 1;
    }
    if let Some(v) = idle_brightness {
        payload[2] = v.min(10);
        payload[3] = 1;
    }
    if let Some(ms) = idle_timeout_ms {
        payload[4..8].copy_from_slice(&ms.to_le_bytes());
        payload[8] = 1;
    }
    super::protocol::raw(CMD_LIGHTING_CONFIG, &payload)
}

/// Ask for the current lighting configuration.
///
/// A *read*, so this is an ordinary 65-byte output report — not a feature
/// report like the tables above. That asymmetry is real: the device answers
/// `0xA0` sent as an output report and ignores it as a feature one.
pub fn read_lighting_query() -> Vec<u8> {
    super::protocol::raw(CMD_READ_LIGHTING, &[])
}

/// Decode a `0xA0` reply. Offsets are relative to the reply as Linux hands it
/// over — this device uses report id 0, so `read()` does **not** include it and
/// the specification's offsets shift down by one.
pub fn parse_lighting(reply: &[u8]) -> Option<LightingConfig> {
    if reply.first() != Some(&CMD_READ_LIGHTING) || reply.len() < 7 {
        return None;
    }
    Some(LightingConfig {
        brightness: reply[1],
        idle_brightness: reply[2],
        idle_timeout_ms: u32::from_le_bytes([reply[3], reply[4], reply[5], reply[6]]),
    })
}

/// Power saving, as read back by [`parse_power_saving`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PowerSaving {
    /// Milliseconds of inactivity before the board sleeps; 0 never sleeps.
    pub timeout_ms: u32,
    pub high_efficiency: bool,
}

pub fn set_power_saving(timeout_ms: u32, high_efficiency: bool) -> Vec<u8> {
    let mut payload = Vec::with_capacity(7);
    payload.extend_from_slice(&timeout_ms.to_le_bytes());
    payload.push(u8::from(high_efficiency));
    // style and brightness: the specification pins both to 1 as the driver's
    // defaults, and nothing here exposes them.
    payload.push(1);
    payload.push(1);
    super::protocol::raw(CMD_POWER_SAVING, &payload)
}

pub fn read_power_query() -> Vec<u8> {
    super::protocol::raw(CMD_READ_POWER, &[])
}

pub fn parse_power_saving(reply: &[u8]) -> Option<PowerSaving> {
    if reply.first() != Some(&CMD_READ_POWER) || reply.len() < 6 {
        return None;
    }
    Some(PowerSaving {
        timeout_ms: u32::from_le_bytes([reply[1], reply[2], reply[3], reply[4]]),
        high_efficiency: reply[5] == 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::protocol::{model, PID_APEX_PRO_TKL_2023_WL_WIRED};

    fn board() -> &'static Model {
        model(PID_APEX_PRO_TKL_2023_WL_WIRED).unwrap()
    }

    #[test]
    fn actuation_is_a_per_key_table_in_tenths_of_a_millimetre() {
        // Exactly the packet that made F12 trigger later on real hardware.
        let packet = set_actuation(board(), &[KeyActuation::uniform(board(), 0x45, 40)], 0);
        assert_eq!(packet[0], 0x2f);
        assert_eq!(packet[1], 0, "layer 0 is the actuation point");
        assert_eq!(packet[2], 1, "one key, and the device accepts that");
        assert_eq!(&packet[3..6], &[0x45, 40, 40]);
        assert_eq!(packet.len(), board().feature_len);
    }

    #[test]
    fn actuation_clamps_to_the_switchs_travel() {
        let packet = set_actuation(
            board(),
            &[
                KeyActuation::uniform(board(), 0x04, 0),
                KeyActuation::uniform(board(), 0x05, 200),
            ],
            0,
        );
        assert_eq!(&packet[3..6], &[0x04, 1, 1], "0.0 mm is not a thing");
        assert_eq!(&packet[6..9], &[0x05, 40, 40], "4.0 mm is the bottom");
    }

    #[test]
    fn the_second_actuation_layer_differs_only_in_its_layer_byte() {
        let first = set_actuation(board(), &[KeyActuation::uniform(board(), 0x04, 12)], 0);
        let second = set_actuation(board(), &[KeyActuation::uniform(board(), 0x04, 12)], 1);
        assert_eq!(first[1], 0);
        assert_eq!(second[1], 1);
        assert_eq!(first[2..], second[2..]);
    }

    #[test]
    fn the_wired_gen3_boards_get_raw_sensor_counts_not_millimetres() {
        // Sending tenths to a board that wants raw counts would ask for a
        // tenth of the intended depth — the ends of the vendor's table are the
        // only fixed points we have, so they are pinned here.
        let gen3 = model(crate::keyboard::protocol::PID_APEX_PRO_GEN3).unwrap();
        assert!(gen3.raw_actuation);
        assert_eq!(KeyActuation::uniform(gen3, 0x04, 1).press, 5);
        assert_eq!(KeyActuation::uniform(gen3, 0x04, 40).press, 201);
        assert_eq!(KeyActuation::uniform(gen3, 0x04, 40).release, 215);
        // The middle of the curve is where a linear guess goes wrong: 2.0 mm
        // is 50, not the ~100 an interpolation between the ends would give.
        assert_eq!(KeyActuation::uniform(gen3, 0x04, 20).press, 50);
        assert_eq!(KeyActuation::uniform(gen3, 0x04, 20).release, 55);
        // And the tail is not monotonic — the vendor's numbers, quirks kept.
        assert_eq!(raw_from_level(37), (201, 206));
        assert_eq!(raw_from_level(38), (197, 215));
        assert_eq!(GEN3_ACTUATION.len(), 40);
        // The wireless board keeps taking millimetres.
        assert_eq!(KeyActuation::uniform(board(), 0x04, 40).press, 40);
    }

    #[test]
    fn rapid_trigger_needs_the_release_mode_table_as_well() {
        // The sensitivity alone enables nothing; 0x36 is the switch, and its
        // "on" value differs by transport.
        assert_eq!(rapid_trigger_on_value(board()), 1, "wireless");
        let wired = model(crate::keyboard::protocol::PID_APEX_PRO_GEN3).unwrap();
        assert_eq!(rapid_trigger_on_value(wired), 2, "wired");
        let modes = set_release_mode(board(), &[(0x04, 1), (0x05, 0)]);
        assert_eq!(modes[0], 0x36);
        assert_eq!(&modes[1..6], &[2, 0x04, 1, 0x05, 0]);
    }

    #[test]
    fn rapid_trigger_and_protection_are_per_key_tables_too() {
        let rt = set_rapid_trigger_sensitivity(board(), &[(0x04, 5), (0x05, 0)]);
        assert_eq!(rt[0], 0x37);
        assert_eq!(rt[1], 2);
        assert_eq!(&rt[2..6], &[0x04, 5, 0x05, 0]);

        let pm = set_protection_mode(board(), &[(0x04, true), (0x05, false)]);
        assert_eq!(pm[0], 0x14);
        assert_eq!(&pm[2..6], &[0x04, 1, 0x05, 0]);
    }

    #[test]
    fn the_rapid_tap_switch_is_a_single_byte_output_report() {
        assert_eq!(&set_rapid_tap(true)[..3], &[0x00, 0x17, 1]);
        assert_eq!(&set_rapid_tap(false)[..3], &[0x00, 0x17, 0]);
        assert_eq!(set_rapid_tap(true).len(), crate::keyboard::protocol::COMMAND_LEN);
    }

    #[test]
    fn lighting_config_only_applies_the_fields_it_was_given() {
        // The whole point: changing the idle timer must not touch brightness.
        // An OUTPUT report: [0x00 report id][cmd][payload…], 65 bytes. Sent as
        // a feature report the device accepts the ioctl and ignores the
        // command, which is how a setting comes to look applied and is not.
        let only_idle = set_lighting_config(None, None, Some(0));
        assert_eq!(only_idle.len(), crate::keyboard::protocol::COMMAND_LEN);
        assert_eq!(&only_idle[..2], &[0x00, 0x20]);
        assert_eq!(only_idle[3], 0, "brightness apply flag stays clear");
        assert_eq!(only_idle[5], 0, "idle brightness apply flag stays clear");
        assert_eq!(&only_idle[6..10], &[0, 0, 0, 0], "timeout 0 = never dim");
        assert_eq!(only_idle[10], 1, "and its apply flag is set");

        let only_brightness = set_lighting_config(Some(7), None, None);
        assert_eq!(&only_brightness[2..4], &[7, 1]);
        assert_eq!(only_brightness[10], 0, "timeout apply flag stays clear");
    }

    #[test]
    fn the_read_queries_are_output_reports_not_feature_reports() {
        // Measured: the device answers 0xA0/0xA9 sent as 65-byte output
        // reports. Sending them as feature reports gets nothing back.
        let q = read_lighting_query();
        assert_eq!(q.len(), crate::keyboard::protocol::COMMAND_LEN);
        assert_eq!(&q[..2], &[0x00, 0xa0]);
        assert_eq!(&read_power_query()[..2], &[0x00, 0xa9]);
    }

    #[test]
    fn the_lighting_reply_decodes_the_values_the_hardware_returned() {
        // The real reply: brightness 10, idle brightness 10, 60 s.
        let reply = [0xa0, 0x0a, 0x0a, 0x60, 0xea, 0x00, 0x00, 0x0a];
        let config = parse_lighting(&reply).expect("a lighting reply");
        assert_eq!(config.brightness, 10);
        assert_eq!(config.idle_brightness, 10);
        assert_eq!(config.idle_timeout_ms, 60_000);
        // Another command's answer must not be read as lighting config.
        assert_eq!(parse_lighting(&[0x92, 0x95]), None);
        assert_eq!(parse_lighting(&[0xa0, 1]), None, "too short");
    }

    #[test]
    fn the_power_saving_reply_decodes_the_five_minutes_the_board_reported() {
        let reply = [0xa9, 0xe0, 0x93, 0x04, 0x00, 0x00, 0x00];
        let power = parse_power_saving(&reply).expect("a power reply");
        assert_eq!(power.timeout_ms, 300_000);
        assert!(!power.high_efficiency);
        assert_eq!(parse_power_saving(&[0xa0, 0, 0, 0, 0, 0]), None);

        let packet = set_power_saving(300_000, true);
        assert_eq!(packet.len(), crate::keyboard::protocol::COMMAND_LEN);
        assert_eq!(&packet[..2], &[0x00, 0x29]);
        assert_eq!(&packet[2..6], &300_000u32.to_le_bytes());
        assert_eq!(packet[6], 1);
    }
}
