//! SteelSeries **Apex** keyboard protocol (Apex 5/7/9/Pro, all generations).
//!
//! ## What was measured, and on what
//!
//! Most of this was **verified on an Apex Pro TKL Wireless (2023)**
//! (`1038:1632`, firmware 3.24.1) over `/dev/hidraw` on 2026-08-01. Its vendor
//! interface (USB interface 3, usage page `0xFFC0`) declares three reports, and
//! probing confirmed each one:
//!
//! * **Output, 64 bytes** — configuration. Written as 65 bytes,
//!   `[0x00 = report id][cmd][payload…]`. Zone colour, brightness, apply and
//!   the queries all take this shape.
//! * **Input, 64 bytes** — query replies. Note they arrive one command behind:
//!   read after a write, and drain the queue before trusting an answer.
//! * **Feature, 641 bytes** — bulk payloads (per-key lighting, OLED), with the
//!   **command byte first**: `[cmd][payload…]`, padded to exactly 641. This is
//!   the detail that matters: prefixing a `0x00` report id instead — what
//!   OpenRGB does — makes this device stall the transfer (EPIPE), and lengths
//!   of 640, 644 or 64 are refused too. Repeated stalls make the keyboard
//!   re-enumerate, which is how the encoding was found in the first place.
//!
//! Confirmed working on that board: per-key direct lighting (`0x61`, a
//! two-colour split showed up exactly as sent), the OLED (see [`super::oled`]),
//! brightness, zone colour, apply, and the firmware query. Confirmed *not*
//! working: the actuation command `0x2D` — the keyboard accepts it and nothing
//! changes, so Inari offers it as experimental and says so.
//!
//! The wired boards (Apex Pro Gen 3 among them) were not available. Their
//! entries follow OpenRGB's `SteelSeriesApexController` and `apex-tux`, and the
//! manager falls back to [`alt_envelope`] if the primary shape is refused.

use serde::{Deserialize, Serialize};

// --- product ids --------------------------------------------------------

/// Apex Pro (full size, 2019).
pub const PID_APEX_PRO: u16 = 0x1610;
/// Apex 7.
pub const PID_APEX_7: u16 = 0x1612;
/// Apex Pro TKL (2019).
pub const PID_APEX_PRO_TKL: u16 = 0x1614;
/// Apex 7 TKL.
pub const PID_APEX_7_TKL: u16 = 0x1618;
/// Apex 5.
pub const PID_APEX_5: u16 = 0x161c;
/// Apex 9 Mini.
pub const PID_APEX_9_MINI: u16 = 0x1620;
/// Apex 9 TKL.
pub const PID_APEX_9_TKL: u16 = 0x1634;
/// Apex Pro TKL (2023), wired.
pub const PID_APEX_PRO_TKL_2023: u16 = 0x1628;
/// Apex Pro TKL Wireless (2023) — 2.4 GHz dongle.
pub const PID_APEX_PRO_TKL_2023_WL_DONGLE: u16 = 0x1630;
/// Apex Pro TKL Wireless (2023) — USB cable. **The verified device.**
pub const PID_APEX_PRO_TKL_2023_WL_WIRED: u16 = 0x1632;
/// Apex Pro Gen 3 (full size).
pub const PID_APEX_PRO_GEN3: u16 = 0x1640;
/// Apex Pro TKL Gen 3, wired.
pub const PID_APEX_PRO_TKL_GEN3: u16 = 0x1642;
/// Apex Pro TKL Wireless Gen 3 — 2.4 GHz dongle.
pub const PID_APEX_PRO_TKL_GEN3_WL_DONGLE: u16 = 0x1644;
/// Apex Pro TKL Wireless Gen 3 — USB cable.
pub const PID_APEX_PRO_TKL_GEN3_WL_WIRED: u16 = 0x1646;

// --- report shape -------------------------------------------------------

/// Configuration reports: 64 payload bytes plus the leading report id.
pub const COMMAND_LEN: usize = 65;

/// Feature-report size on the wireless boards, straight from their report
/// descriptor. Anything else is refused.
pub const FEATURE_LEN_WIRELESS: usize = 641;
/// Feature-report size the wired boards are driven with by `apex-tux` and
/// OpenRGB (one byte more).
pub const FEATURE_LEN_WIRED: usize = 642;
/// Apex 9 Mini / 9 TKL take a shorter one (OpenRGB's `APEX_9_PACKET_LENGTH`).
pub const FEATURE_LEN_APEX9: usize = 512;

// --- command bytes ------------------------------------------------------

/// Save the current configuration (`0x09`).
const CMD_APPLY: u8 = 0x09;
/// Zone colour (`0x21 zone R G B`), zone `0xFF` = every zone at once.
const CMD_ZONE: u8 = 0x21;
/// Lighting brightness in percent (`0x22 pct`).
const CMD_BRIGHTNESS: u8 = 0x22;
/// Reactive (key-press flash) on/off (`0x25 on`).
const CMD_REACTIVE: u8 = 0x25;
/// Colour shift between two colours (`0x26 R G B R G B speed`).
const CMD_COLOR_SHIFT: u8 = 0x26;
/// Actuation point in 0.1 mm steps (`0x2D value`). Accepted but ineffective on
/// firmware 3.24.1 — see the module docs.
const CMD_ACTUATION: u8 = 0x2d;
/// Direct per-key lighting, generation 1.
const CMD_DIRECT_GEN1: u8 = 0x3a;
/// Hand the LEDs back to the onboard profile, generation 1.
const CMD_ONBOARD_GEN1: u8 = 0x3b;
/// Direct per-key lighting, 2023/Gen 3 wired.
const CMD_DIRECT_NEW: u8 = 0x40;
/// Hand the LEDs back to the onboard profile, 2023/Gen 3.
const CMD_ONBOARD_NEW: u8 = 0x41;
/// Take control of the LEDs, Gen 3 only.
const CMD_INIT_NEW: u8 = 0x4b;
/// Direct per-key lighting, wireless models. **Verified.**
const CMD_DIRECT_WIRELESS: u8 = 0x61;
/// Select an onboard profile (`0x89 index`).
const CMD_PROFILE: u8 = 0x89;
/// Firmware version query (`0x90`). **Verified** — replies with ASCII.
const CMD_FIRMWARE: u8 = 0x90;
/// Battery query (`0x92`). **Verified** on the wireless board.
const CMD_BATTERY: u8 = 0x92;
/// Region byte query (`0xF5`), Gen 2+ only.
const CMD_REGION: u8 = 0xf5;

/// The zone selector that addresses every zone in one write.
pub const ZONE_ALL: u8 = 0xff;

// --- model table --------------------------------------------------------

/// Which dialect of the lighting protocol a keyboard speaks.
///
/// Gen 1: 2019–2022 models. Gen 2: the 2023 refresh and the Apex 9 series.
/// Gen 3: OmniPoint 3.0 models, plus Gen 2 models that took the "prism"
/// firmware update (1.19.7+) — the verified 2023 board reports 3.24.1 and is
/// squarely in that group, which is why it answers to the Gen 3 opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gen {
    Gen1,
    Gen2,
    Gen3,
}

/// Physical size, which decides the key layout the UI draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormFactor {
    Full,
    Tkl,
    Mini,
}

/// How this model's OLED is fed. Encodings live in [`super::oled`].
///
/// Two things vary independently — how a frame is addressed, and how its
/// pixels are packed — so both are spelled out rather than hidden behind a
/// per-model name. `page` selects SSD1306 page-major packing over row-major.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OledWire {
    /// One report: `[cmd][bitmap]`. `0x61` on the 2019 boards (row major),
    /// `0x0A`/`0x4A` on the Gen 3 wireless ones (page major) per OmniLED.
    Single { cmd: u8, page: bool },
    /// One report with a four-byte prefix, `[0x38 0x83 0x00 0x00][bitmap]`,
    /// page major. OmniLED drives the wired Apex Pro TKL Gen 3 this way.
    Gen3Wired,
    /// Eight offset-addressed chunks of 80 bytes. **Verified** as
    /// `{cmd: 0x0C, page: false}` on the 2023 wireless board over its cable;
    /// `apex-tux` uses the page-major form for the Gen 3 wireless boards.
    Chunked { cmd: u8, page: bool },
}

/// One supported keyboard.
#[derive(Debug, Clone, Copy)]
pub struct Model {
    pub product_id: u16,
    pub name: &'static str,
    pub form: FormFactor,
    pub gen: Gen,
    /// USB interface carrying the vendor collection.
    pub interface: u8,
    /// Reached over the 2.4 GHz dongle or its cable — either way this is the
    /// wireless *model*, which uses its own direct-lighting opcode.
    pub wireless: bool,
    /// Runs on a battery Inari can read.
    pub battery: bool,
    /// `None` on models without a panel we can drive.
    pub oled: Option<OledWire>,
    /// Exact size of a feature report on this model.
    pub feature_len: usize,
    /// Adjustable (HyperMagnetic / OmniPoint) switches.
    pub actuation: bool,
}

/// Every Apex keyboard Inari speaks to.
///
/// The Apex 3 family is deliberately absent: it is a zone-only keyboard with
/// its own controller dialect in OpenRGB, and this one would do nothing useful
/// on it.
pub static MODELS: &[Model] = &[
    Model {
        product_id: PID_APEX_PRO,
        name: "Apex Pro",
        form: FormFactor::Full,
        gen: Gen::Gen1,
        interface: 1,
        wireless: false,
        battery: false,
        oled: Some(OledWire::Single {
            cmd: 0x61,
            page: false,
        }),
        feature_len: FEATURE_LEN_WIRED,
        actuation: true,
    },
    Model {
        product_id: PID_APEX_PRO_TKL,
        name: "Apex Pro TKL",
        form: FormFactor::Tkl,
        gen: Gen::Gen1,
        interface: 1,
        wireless: false,
        battery: false,
        oled: Some(OledWire::Single {
            cmd: 0x61,
            page: false,
        }),
        feature_len: FEATURE_LEN_WIRED,
        actuation: true,
    },
    Model {
        product_id: PID_APEX_7,
        name: "Apex 7",
        form: FormFactor::Full,
        gen: Gen::Gen1,
        interface: 1,
        wireless: false,
        battery: false,
        oled: Some(OledWire::Single {
            cmd: 0x61,
            page: false,
        }),
        feature_len: FEATURE_LEN_WIRED,
        actuation: false,
    },
    Model {
        product_id: PID_APEX_7_TKL,
        name: "Apex 7 TKL",
        form: FormFactor::Tkl,
        gen: Gen::Gen1,
        interface: 1,
        wireless: false,
        battery: false,
        oled: Some(OledWire::Single {
            cmd: 0x61,
            page: false,
        }),
        feature_len: FEATURE_LEN_WIRED,
        actuation: false,
    },
    Model {
        product_id: PID_APEX_5,
        name: "Apex 5",
        form: FormFactor::Full,
        gen: Gen::Gen1,
        interface: 1,
        wireless: false,
        battery: false,
        oled: Some(OledWire::Single {
            cmd: 0x61,
            page: false,
        }),
        feature_len: FEATURE_LEN_WIRED,
        actuation: false,
    },
    Model {
        product_id: PID_APEX_9_MINI,
        name: "Apex 9 Mini",
        form: FormFactor::Mini,
        gen: Gen::Gen2,
        interface: 1,
        wireless: false,
        battery: false,
        oled: None,
        feature_len: FEATURE_LEN_APEX9,
        actuation: false,
    },
    Model {
        product_id: PID_APEX_9_TKL,
        name: "Apex 9 TKL",
        form: FormFactor::Tkl,
        gen: Gen::Gen2,
        interface: 1,
        wireless: false,
        battery: false,
        oled: None,
        feature_len: FEATURE_LEN_APEX9,
        actuation: false,
    },
    Model {
        product_id: PID_APEX_PRO_TKL_2023,
        name: "Apex Pro TKL (2023)",
        form: FormFactor::Tkl,
        gen: Gen::Gen2,
        interface: 1,
        wireless: false,
        battery: false,
        // Unverified. Its wireless sibling wants chunked row-major, so that
        // is a better guess than the 2019 transport; the manager probes on.
        oled: Some(OledWire::Chunked {
            cmd: 0x0c,
            page: false,
        }),
        feature_len: FEATURE_LEN_WIRED,
        actuation: true,
    },
    Model {
        product_id: PID_APEX_PRO_TKL_2023_WL_DONGLE,
        name: "Apex Pro TKL Wireless (2023)",
        form: FormFactor::Tkl,
        gen: Gen::Gen3,
        interface: 3,
        wireless: true,
        battery: true,
        oled: Some(OledWire::Chunked {
            cmd: 0x4c,
            page: false,
        }),
        feature_len: FEATURE_LEN_WIRELESS,
        actuation: true,
    },
    Model {
        product_id: PID_APEX_PRO_TKL_2023_WL_WIRED,
        name: "Apex Pro TKL Wireless (2023)",
        form: FormFactor::Tkl,
        gen: Gen::Gen3,
        interface: 3,
        wireless: true,
        battery: true,
        // Verified on hardware, cable attached.
        oled: Some(OledWire::Chunked {
            cmd: 0x0c,
            page: false,
        }),
        feature_len: FEATURE_LEN_WIRELESS,
        actuation: true,
    },
    Model {
        product_id: PID_APEX_PRO_GEN3,
        name: "Apex Pro Gen 3",
        form: FormFactor::Full,
        gen: Gen::Gen3,
        interface: 1,
        wireless: false,
        battery: false,
        // apex-tux drives this one with a single page-major report.
        oled: Some(OledWire::Single {
            cmd: 0x61,
            page: true,
        }),
        feature_len: FEATURE_LEN_WIRED,
        actuation: true,
    },
    Model {
        product_id: PID_APEX_PRO_TKL_GEN3,
        name: "Apex Pro TKL Gen 3",
        form: FormFactor::Tkl,
        gen: Gen::Gen3,
        interface: 1,
        wireless: false,
        battery: false,
        // OmniLED's working config for this exact product id.
        oled: Some(OledWire::Gen3Wired),
        feature_len: FEATURE_LEN_WIRED,
        actuation: true,
    },
    Model {
        product_id: PID_APEX_PRO_TKL_GEN3_WL_DONGLE,
        name: "Apex Pro TKL Wireless Gen 3",
        form: FormFactor::Tkl,
        gen: Gen::Gen3,
        interface: 3,
        wireless: true,
        battery: true,
        oled: Some(OledWire::Single {
            cmd: 0x4a,
            page: true,
        }),
        feature_len: FEATURE_LEN_WIRELESS,
        actuation: true,
    },
    Model {
        product_id: PID_APEX_PRO_TKL_GEN3_WL_WIRED,
        name: "Apex Pro TKL Wireless Gen 3",
        form: FormFactor::Tkl,
        gen: Gen::Gen3,
        interface: 3,
        wireless: true,
        battery: true,
        oled: Some(OledWire::Single {
            cmd: 0x0a,
            page: true,
        }),
        feature_len: FEATURE_LEN_WIRELESS,
        actuation: true,
    },
];

/// Wired models — their control collection sits on USB interface 1.
pub const PRODUCT_IDS_WIRED: [u16; 10] = [
    PID_APEX_PRO,
    PID_APEX_PRO_TKL,
    PID_APEX_7,
    PID_APEX_7_TKL,
    PID_APEX_5,
    PID_APEX_9_MINI,
    PID_APEX_9_TKL,
    PID_APEX_PRO_TKL_2023,
    PID_APEX_PRO_GEN3,
    PID_APEX_PRO_TKL_GEN3,
];

/// Wireless models — interface 3, and the collection opens with the vendor
/// usage page `0xFFC0`, which is what discovery actually probes for.
pub const PRODUCT_IDS_WIRELESS: [u16; 4] = [
    PID_APEX_PRO_TKL_2023_WL_DONGLE,
    PID_APEX_PRO_TKL_2023_WL_WIRED,
    PID_APEX_PRO_TKL_GEN3_WL_DONGLE,
    PID_APEX_PRO_TKL_GEN3_WL_WIRED,
];

/// The model behind a product id.
pub fn model(product_id: u16) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.product_id == product_id)
}

impl Model {
    /// The same model at a different protocol generation.
    ///
    /// The generation is not fixed by the product id: OpenRGB's rule is that
    /// any Gen 1/2 board running firmware 1.19.7 or newer speaks Gen 3, and the
    /// board measured here proves the point — a 2023 model on firmware 3.24.1
    /// answers to the Gen 3 opcodes. So the table's generation is the starting
    /// guess and the firmware string gets the final say.
    pub fn at_gen(self, gen: Gen) -> Model {
        Model { gen, ..self }
    }
}

// --- key order on the wire ----------------------------------------------

/// The 112 HID usage ids a direct-lighting packet carries, in the order the
/// firmware expects. No keyboard has all of them fitted; the firmware picks the
/// ones it has and ignores the rest, which is why one packet layout works
/// across full-size, TKL and mini boards.
///
/// Transcribed from OpenRGB's `SteelSeriesApexController` and confirmed on
/// hardware: colouring entries 0–55 magenta and 56–111 cyan lit exactly the
/// expected halves of the board.
pub const KEY_ORDER: [u8; 112] = [
    0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13,
    0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23,
    0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x32, 0x33, 0x34,
    0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, 0x41, 0x42, 0x43, 0x44,
    0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x50, 0x51, 0x52, 0x64, 0xe0,
    0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xf0, 0x31, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x53, 0x54,
    0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f, 0x60, 0x61, 0x62, 0x63, 0xfb,
];

/// Position of a HID usage id within [`KEY_ORDER`], if the wire format carries
/// it at all.
pub fn key_index(hid: u8) -> Option<usize> {
    KEY_ORDER.iter().position(|k| *k == hid)
}

// --- packet builders ----------------------------------------------------

/// A zero-padded 65-byte **output** report: `[0x00][cmd][payload…]`.
fn control(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let mut buf = vec![0u8; COMMAND_LEN];
    buf[1] = cmd;
    let n = payload.len().min(COMMAND_LEN - 2);
    buf[2..2 + n].copy_from_slice(&payload[..n]);
    buf
}

/// A zero-padded **feature** report: `[cmd][payload…]`, exactly the length this
/// model declares.
pub fn feature(model: &Model, cmd: u8, payload: &[u8]) -> Vec<u8> {
    let mut buf = vec![0u8; model.feature_len];
    buf[0] = cmd;
    let n = payload.len().min(model.feature_len - 1);
    buf[1..1 + n].copy_from_slice(&payload[..n]);
    buf
}

/// The other way round: a `0x00` report id in front, one byte longer. This is
/// what OpenRGB and hidapi's conventions produce, and the shape the verified
/// board rejects — but a board that rejects ours may well want this one, so the
/// manager keeps it as a fallback rather than giving up.
pub fn alt_envelope(packet: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(packet.len() + 1);
    out.push(0x00);
    out.extend_from_slice(packet);
    out
}

/// Take control of the LEDs. Gen 3 firmware documents this as required; the
/// verified board accepted direct lighting with and without it, so it is sent
/// on connect and never depended on.
pub fn init(model: &Model) -> Vec<Vec<u8>> {
    if model.gen >= Gen::Gen3 {
        vec![feature(model, CMD_INIT_NEW, &[])]
    } else {
        Vec::new()
    }
}

/// Hand the LEDs back to the keyboard's own profile. Sent on shutdown, and
/// whenever the user switches Inari's lighting off — without it the board keeps
/// whatever Inari painted last, which it does indefinitely.
pub fn release_to_onboard(model: &Model) -> Vec<u8> {
    let cmd = if model.gen >= Gen::Gen2 {
        CMD_ONBOARD_NEW
    } else {
        CMD_ONBOARD_GEN1
    };
    control(cmd, &[])
}

/// The direct-lighting command byte for this model.
fn direct_cmd(model: &Model) -> u8 {
    match (model.gen, model.wireless) {
        (Gen::Gen1, _) => CMD_DIRECT_GEN1,
        (_, true) => CMD_DIRECT_WIRELESS,
        (_, false) => CMD_DIRECT_NEW,
    }
}

/// Build the per-key lighting feature report. `colors` is indexed by
/// [`KEY_ORDER`]; shorter slices simply address fewer keys.
pub fn direct_packet(model: &Model, colors: &[[u8; 3]]) -> Vec<u8> {
    // [cmd][count][hid R G B]… — never past the report or past the colours.
    let capacity = model.feature_len.saturating_sub(2) / 4;
    let count = colors.len().min(KEY_ORDER.len()).min(capacity);
    let mut payload = Vec::with_capacity(1 + count * 4);
    payload.push(count as u8);
    for (i, color) in colors.iter().take(count).enumerate() {
        payload.push(KEY_ORDER[i]);
        payload.extend_from_slice(color);
    }
    feature(model, direct_cmd(model), &payload)
}

/// Solid colour for one lighting zone, or every zone at [`ZONE_ALL`].
pub fn zone_color(zone: u8, rgb: [u8; 3]) -> Vec<u8> {
    control(CMD_ZONE, &[zone, rgb[0], rgb[1], rgb[2]])
}

/// Lighting brightness, 0–100 %.
pub fn brightness(percent: u8) -> Vec<u8> {
    control(CMD_BRIGHTNESS, &[percent.min(100)])
}

/// Persist the current configuration to the keyboard.
pub fn apply() -> Vec<u8> {
    control(CMD_APPLY, &[])
}

/// Reactive mode: keys flash on press. Rendered by the firmware, so it keeps
/// working while Inari is closed.
pub fn reactive(on: bool) -> Vec<u8> {
    control(CMD_REACTIVE, &[u8::from(on)])
}

/// Colour shift between two colours. `speed` is 0–100.
pub fn color_shift(a: [u8; 3], b: [u8; 3], speed: u8) -> Vec<u8> {
    control(
        CMD_COLOR_SHIFT,
        &[a[0], a[1], a[2], b[0], b[1], b[2], speed.min(100)],
    )
}

/// Actuation point in tenths of a millimetre (1 = 0.1 mm … 40 = 4.0 mm).
///
/// **Does nothing on the one board this could be tried on** (Apex Pro TKL
/// Wireless 2023, firmware 3.24.1): the write is accepted, the switches do not
/// move. Kept because it costs nothing and a different model or firmware may
/// well answer to it — but the UI must not present it as working.
pub fn actuation(tenths: u8) -> Vec<u8> {
    control(CMD_ACTUATION, &[tenths.clamp(1, 40)])
}

/// Switch to one of the keyboard's onboard profiles.
pub fn select_profile(index: u8) -> Vec<u8> {
    control(CMD_PROFILE, &[index])
}

/// Ask for the firmware version string.
pub fn firmware_query() -> Vec<u8> {
    control(CMD_FIRMWARE, &[])
}

/// Ask for the battery level (wireless boards).
pub fn battery_query() -> Vec<u8> {
    control(CMD_BATTERY, &[])
}

/// Ask for the region byte (Gen 2+; it stands in for the missing serial).
pub fn region_query() -> Vec<u8> {
    control(CMD_REGION, &[])
}

/// An arbitrary command, for the experimental probe in the UI. Same 65-byte
/// envelope as everything else — the point is to let someone with hardware find
/// the commands nobody has captured yet (Rapid Trigger, Protection Mode).
pub fn raw(cmd: u8, payload: &[u8]) -> Vec<u8> {
    control(cmd, payload)
}

/// Parse a firmware reply. The verified board answers `0x90` with bare ASCII
/// ("3.24.1") rather than an echo of the command, while OpenRGB's older models
/// prefix a four-byte header — so take the first run that looks like a version
/// instead of assuming an offset.
pub fn parse_firmware(buf: &[u8]) -> Option<String> {
    let text: String = buf
        .iter()
        .map(|b| *b as char)
        .map(|c| {
            if c.is_ascii_digit() || c == '.' {
                c
            } else {
                ' '
            }
        })
        .collect();
    text.split_whitespace()
        .find(|part| part.contains('.') && part.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(str::to_string)
}

/// Whether a firmware version speaks the Gen 3 dialect. OpenRGB puts the
/// cut-off at 1.19.7 — the "prism" update that retrofits the 2023 boards.
pub fn firmware_is_gen3(version: &str) -> bool {
    let digits: Vec<u32> = version
        .split('.')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    match digits.as_slice() {
        [major, minor, patch, ..] => {
            *major > 1 || (*major == 1 && (*minor > 19 || (*minor == 19 && *patch >= 7)))
        }
        _ => false,
    }
}

/// Battery level from a `0x92` reply, as `(percent, charging)`.
///
/// The reply echoes the command in byte 0 — which matters more than it looks:
/// this device answers queries **one command behind**, so a caller that reads
/// whatever arrives next gets the previous answer. Checking the echo is what
/// makes that harmless.
///
/// The byte itself is decoded exactly like every other SteelSeries battery
/// (see [`crate::device::parse_battery_byte`]). An earlier version read it as
/// two BCD digits, which agrees at 95 % and disagrees everywhere else; the
/// vendor's own specification and the Aerox reading both say otherwise, and
/// BCD cannot express 100 % at all.
pub fn parse_battery(buf: &[u8]) -> Option<(u8, bool)> {
    if buf.first() != Some(&CMD_BATTERY) {
        return None;
    }
    crate::device::parse_battery_byte(*buf.get(1)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_id_is_unique() {
        for (i, a) in MODELS.iter().enumerate() {
            for b in MODELS.iter().skip(i + 1) {
                assert_ne!(a.product_id, b.product_id, "{} is listed twice", a.name);
            }
        }
    }

    #[test]
    fn the_users_keyboards_are_in_the_table() {
        assert_eq!(model(PID_APEX_PRO_GEN3).unwrap().name, "Apex Pro Gen 3");
        assert_eq!(model(PID_APEX_PRO_GEN3).unwrap().form, FormFactor::Full);
        let verified = model(PID_APEX_PRO_TKL_2023_WL_WIRED).unwrap();
        assert!(verified.wireless && verified.battery);
        assert_eq!(verified.interface, 3);
        assert_eq!(verified.feature_len, 641);
        assert_eq!(
            verified.oled,
            Some(OledWire::Chunked {
                cmd: 0x0c,
                page: false
            })
        );
        assert!(model(0x0000).is_none());
    }

    #[test]
    fn wireless_models_live_on_interface_three() {
        for m in MODELS {
            assert_eq!(
                m.interface,
                if m.wireless { 3 } else { 1 },
                "{} has the wrong control interface",
                m.name
            );
            assert_eq!(
                m.battery, m.wireless,
                "{} battery/transport mismatch",
                m.name
            );
        }
    }

    #[test]
    fn the_discovery_lists_match_the_model_table() {
        // These two arrays are what device discovery matches on; a model added
        // to the table but not to its list would never be found.
        let on = |interface: u8| -> Vec<u16> {
            MODELS
                .iter()
                .filter(|m| m.interface == interface)
                .map(|m| m.product_id)
                .collect()
        };
        assert_eq!(on(1), PRODUCT_IDS_WIRED.to_vec());
        assert_eq!(on(3), PRODUCT_IDS_WIRELESS.to_vec());
        assert_eq!(
            PRODUCT_IDS_WIRED.len() + PRODUCT_IDS_WIRELESS.len(),
            MODELS.len()
        );
    }

    #[test]
    fn a_firmware_upgrade_can_move_a_board_to_the_gen3_dialect() {
        let board = *model(PID_APEX_PRO_TKL_2023).unwrap();
        assert_eq!(board.gen, Gen::Gen2);
        assert_eq!(direct_packet(&board, &[[1, 2, 3]])[0], 0x40);
        let upgraded = board.at_gen(Gen::Gen3);
        assert_eq!(upgraded.gen, Gen::Gen3);
        assert_eq!(
            upgraded.product_id, board.product_id,
            "still the same board"
        );
        assert_eq!(release_to_onboard(&upgraded)[1], 0x41);
        assert_eq!(init(&upgraded).len(), 1);
    }

    #[test]
    fn key_order_holds_112_distinct_usage_ids() {
        assert_eq!(KEY_ORDER.len(), 112);
        let mut seen = KEY_ORDER.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), KEY_ORDER.len(), "a usage id is listed twice");
        // Spot-check the ends and the two entries that are out of ascending
        // order — those are exactly where a transcription slip would land.
        assert_eq!(KEY_ORDER[0], 0x04);
        assert_eq!(KEY_ORDER[87], 0xf0);
        assert_eq!(KEY_ORDER[88], 0x31);
        assert_eq!(KEY_ORDER[111], 0xfb);
        assert_eq!(key_index(0x29), Some(37)); // Escape
        assert_eq!(key_index(0x03), None);
    }

    fn verified() -> &'static Model {
        model(PID_APEX_PRO_TKL_2023_WL_WIRED).unwrap()
    }

    #[test]
    fn control_reports_are_65_bytes_behind_an_unnumbered_report_id() {
        let packet = brightness(60);
        assert_eq!(packet.len(), COMMAND_LEN);
        assert_eq!(&packet[..3], &[0x00, 0x22, 60]);
        assert!(packet[3..].iter().all(|b| *b == 0));
        assert_eq!(brightness(200)[2], 100, "brightness must clamp");
    }

    #[test]
    fn feature_reports_lead_with_the_command_and_fill_the_declared_size() {
        // The detail the hardware was strict about: command first, exact size.
        let packet = feature(verified(), 0x0c, &[0x01, 0x02]);
        assert_eq!(packet.len(), 641);
        assert_eq!(&packet[..3], &[0x0c, 0x01, 0x02]);
        // The wired boards take one byte more.
        assert_eq!(
            feature(model(PID_APEX_PRO_GEN3).unwrap(), 0x61, &[]).len(),
            642
        );
    }

    #[test]
    fn the_fallback_envelope_only_prepends_a_report_id() {
        let primary = feature(verified(), 0x61, &[1, 2, 3]);
        let alt = alt_envelope(&primary);
        assert_eq!(alt.len(), primary.len() + 1);
        assert_eq!(alt[0], 0x00);
        assert_eq!(&alt[1..], &primary[..]);
    }

    #[test]
    fn direct_packet_is_addressed_per_key_and_sized_per_model() {
        let colors = vec![[0x11, 0x22, 0x33]; KEY_ORDER.len()];
        let packet = direct_packet(verified(), &colors);
        assert_eq!(packet.len(), 641);
        assert_eq!(packet[0], 0x61, "the wireless boards use their own opcode");
        assert_eq!(packet[1], 112);
        assert_eq!(&packet[2..6], &[0x04, 0x11, 0x22, 0x33]);
        // Last key of the wire order lands just inside the report.
        let last = 111 * 4 + 2;
        assert_eq!(packet[last], 0xfb);
        assert!(last + 3 < 641);
    }

    #[test]
    fn wired_and_legacy_models_use_their_own_direct_opcodes() {
        assert_eq!(
            direct_packet(model(PID_APEX_PRO_GEN3).unwrap(), &[[1, 2, 3]])[0],
            0x40
        );
        assert_eq!(
            direct_packet(model(PID_APEX_PRO_TKL).unwrap(), &[[1, 2, 3]])[0],
            0x3a
        );
    }

    #[test]
    fn the_apex_9_short_report_cannot_overflow() {
        let apex9 = model(PID_APEX_9_TKL).unwrap();
        let colors = vec![[9, 9, 9]; KEY_ORDER.len()];
        let packet = direct_packet(apex9, &colors);
        assert_eq!(packet.len(), FEATURE_LEN_APEX9);
        // (512 - 2) / 4 = 127 slots, but only 112 keys exist.
        assert_eq!(packet[1], 112);
    }

    #[test]
    fn init_is_gen3_only_and_onboard_follows_the_generation() {
        assert_eq!(init(verified()).len(), 1);
        assert_eq!(init(verified())[0][0], 0x4b);
        assert!(init(model(PID_APEX_PRO_TKL).unwrap()).is_empty());
        assert_eq!(release_to_onboard(verified())[1], 0x41);
        assert_eq!(release_to_onboard(model(PID_APEX_7).unwrap())[1], 0x3b);
    }

    #[test]
    fn actuation_clamps_to_the_switchs_travel() {
        assert_eq!(actuation(0)[2], 1, "0.0 mm is not a thing");
        assert_eq!(actuation(8)[2], 8);
        assert_eq!(actuation(255)[2], 40, "4.0 mm is the bottom of the switch");
    }

    #[test]
    fn zone_and_shift_payloads_match_the_captured_layout() {
        assert_eq!(
            &zone_color(ZONE_ALL, [1, 2, 3])[..6],
            &[0, 0x21, 0xff, 1, 2, 3]
        );
        assert_eq!(
            &color_shift([1, 2, 3], [4, 5, 6], 250)[..10],
            &[0, 0x26, 1, 2, 3, 4, 5, 6, 100, 0]
        );
        assert_eq!(&reactive(true)[..3], &[0, 0x25, 1]);
        assert_eq!(&apply()[..2], &[0, 0x09]);
    }

    #[test]
    fn firmware_replies_parse_and_gate_the_gen3_dialect() {
        // Exactly what the hardware answered: bare ASCII, no echo.
        let mut real = b"3.24.1".to_vec();
        real.resize(64, 0);
        assert_eq!(parse_firmware(&real).as_deref(), Some("3.24.1"));
        // OpenRGB's older shape, with a header in front.
        let mut older = vec![0x90, 0x00, 0x00, 0x00];
        older.extend_from_slice(b"1.19.7\0");
        assert_eq!(parse_firmware(&older).as_deref(), Some("1.19.7"));
        assert_eq!(parse_firmware(&[0, 0, 0]), None);

        assert!(firmware_is_gen3("3.24.1"), "the verified board is Gen 3");
        assert!(firmware_is_gen3("1.19.7"));
        assert!(!firmware_is_gen3("1.19.6"));
        assert!(!firmware_is_gen3("1.18.9"));
        assert!(!firmware_is_gen3("garbage"));
    }

    #[test]
    fn battery_replies_decode_like_every_other_steelseries_device() {
        // The captured reply: full, and charging over the cable.
        assert_eq!(parse_battery(&[0x92, 0x95, 0x00]), Some((100, true)));
        assert_eq!(parse_battery(&[0x92, 0x0b]), Some((50, false)));
        // Another command's answer must never be read as a battery level —
        // this device replies one command behind, so this happens for real.
        assert_eq!(parse_battery(&[0x90, 0x95]), None);
        assert_eq!(parse_battery(&[0x92, 0x00]), None, "0 is not a reading");
        assert_eq!(parse_battery(&[0x92, 0xff]), None, "0xff means no reading");
        assert_eq!(parse_battery(&[]), None);
    }
}
