//! One table for every SteelSeries device Inari talks to.
//!
//! Discovery used to be two near-identical `/sys/class/hidraw` scans (one per
//! device family) with the vendor id, product ids, report lengths and the
//! per-family quirks spread across the protocol, hidraw, manager and command
//! modules. Everything a new model needs now lives in [`DEVICES`] plus — for
//! headsets — one [`HeadsetOps`] impl.

use std::path::{Path, PathBuf};

use crate::headset::protocol::{self, HeadsetStatus};
use crate::headset::protocol_apw;
use crate::keyboard::protocol as kbd_protocol;
use crate::mouse::protocol as mouse_protocol;

/// SteelSeries. The single definition — the headset and mouse protocol
/// modules each used to carry their own copy.
pub const VENDOR_ID: u16 = 0x1038;

/// Where the kernel publishes hidraw nodes and their metadata.
const HIDRAW_CLASS: &str = "/sys/class/hidraw";

// --- capabilities -------------------------------------------------------

/// What a device can do, as a bitmask. Small enough that pulling in a
/// bitflags crate would cost more than it saves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps(u16);

impl Caps {
    pub const NONE: Caps = Caps(0);
    /// Drives an OLED panel we know how to render to.
    pub const OLED: Caps = Caps(1 << 0);
    /// Hardware equalizer.
    pub const EQ: Caps = Caps(1 << 1);
    /// Active noise cancelling / transparency.
    pub const ANC: Caps = Caps(1 << 2);
    /// Hardware ChatMix wheel that pushes its own events.
    pub const CHATMIX: Caps = Caps(1 << 3);
    /// Addressable RGB zones.
    pub const RGB: Caps = Caps(1 << 4);
    /// Configurable sensor DPI.
    pub const DPI: Caps = Caps(1 << 5);

    /// Union. Spelled as a method because operator impls aren't const, and the
    /// table below is a `static`.
    pub const fn with(self, other: Caps) -> Caps {
        Caps(self.0 | other.0)
    }

    /// Whether every bit of `other` is set.
    pub const fn has(self, other: Caps) -> bool {
        self.0 & other.0 == other.0
    }
}

// --- table shape --------------------------------------------------------

/// Which subsystem owns a device; also what [`scan`] filters on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    Headset,
    Mouse,
    Keyboard,
}

/// How a candidate hidraw node is confirmed to be the one we want. A product
/// id alone is not enough for devices that expose several HID interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// The node's report descriptor must start with these bytes.
    DescriptorPrefix(&'static [u8]),
    /// The owning USB interface's `bInterfaceNumber` must equal this.
    InterfaceNumber(u8),
    /// A product-id match is conclusive — the device has one control node.
    Any,
}

/// One supported model (or one transport of one model).
pub struct DeviceEntry {
    /// Product ids that resolve to this entry. Must not overlap another
    /// entry of the same class.
    pub product_ids: &'static [u16],
    /// Display name shown in the UI.
    pub name: &'static str,
    pub class: DeviceClass,
    pub probe: Probe,
    /// Length control/output reports are zero-padded to. 0 when the device
    /// takes variable-length, unpadded reports (the Aerox does).
    pub report_len: usize,
    pub caps: Caps,
    /// Highest wins when several nodes match; ties go to the first seen.
    pub priority: u8,
    /// Protocol behaviour. Headsets only — the mouse has no generations to
    /// abstract over.
    pub ops: Option<&'static dyn HeadsetOps>,
}

// --- headset protocol behaviour ----------------------------------------

/// Everything that differs between headset generations. One impl per family,
/// so command handlers and the supervisor never branch on the model.
pub trait HeadsetOps: Send + Sync {
    /// Handshake sent on connect. Must never write user settings.
    fn init_safe(&self) -> Vec<Vec<u8>>;
    /// Packets the periodic heartbeat sends to keep status fresh.
    fn heartbeat(&self) -> Vec<Vec<u8>>;
    /// Sidetone, given the UI's 0..3 step.
    fn sidetone(&self, ui_level: u8) -> Vec<Vec<u8>>;
    /// Auto shut-off, given the UI's step index.
    fn auto_off(&self, ui_idx: u8) -> Vec<Vec<u8>>;
    /// Fold an input report into the shared status shape the UI renders.
    /// Returns true when the snapshot changed.
    fn apply_status_frame(&self, buf: &[u8], status: &mut HeadsetStatus) -> bool;
}

/// Arctis Nova Pro Wireless: report id 0x06, 64-byte reports, OLED.
struct NovaProOps;

impl HeadsetOps for NovaProOps {
    fn init_safe(&self) -> Vec<Vec<u8>> {
        protocol::init_safe()
    }

    fn heartbeat(&self) -> Vec<Vec<u8>> {
        vec![protocol::status_query(), protocol::volume_query()]
    }

    fn sidetone(&self, ui_level: u8) -> Vec<Vec<u8>> {
        // The UI's scale is this generation's scale; nothing to translate.
        vec![protocol::set_sidetone(ui_level)]
    }

    fn auto_off(&self, ui_idx: u8) -> Vec<Vec<u8>> {
        vec![protocol::set_auto_off(ui_idx)]
    }

    fn apply_status_frame(&self, buf: &[u8], status: &mut HeadsetStatus) -> bool {
        status.apply_frame(buf)
    }
}

/// Arctis Pro Wireless (pre-Nova): 0xAA-suffixed, 31-byte reports, no OLED.
struct ArctisProOps;

/// The UI's auto-off steps in minutes. The Nova generation takes the index
/// straight through, so the translation belongs to this family alone.
const APW_STEP_MINUTES: [u16; 7] = [0, 1, 5, 10, 15, 30, 60];

impl HeadsetOps for ArctisProOps {
    fn init_safe(&self) -> Vec<Vec<u8>> {
        protocol_apw::init_safe()
    }

    fn heartbeat(&self) -> Vec<Vec<u8>> {
        vec![protocol_apw::status_query()]
    }

    /// The UI works in the Nova generation's 0..3 steps; this generation takes
    /// 0..9, so scale across. Settings only stick after an explicit save.
    fn sidetone(&self, ui_level: u8) -> Vec<Vec<u8>> {
        let scaled = (ui_level.min(3) as u16 * 9 / 3) as u8;
        vec![protocol_apw::set_sidetone(scaled), protocol_apw::save()]
    }

    /// This generation wants minutes rather than a step index.
    fn auto_off(&self, ui_idx: u8) -> Vec<Vec<u8>> {
        let minutes = APW_STEP_MINUTES[ui_idx.min(6) as usize];
        vec![protocol_apw::set_auto_off(minutes), protocol_apw::save()]
    }

    fn apply_status_frame(&self, buf: &[u8], status: &mut HeadsetStatus) -> bool {
        // Pre-Nova frame: power state + battery only. Fold it into the shared
        // status shape so the UI keeps rendering one snapshot type.
        let mut apw = protocol_apw::ApwStatus::default();
        if !apw.apply_frame(buf) {
            return false;
        }
        status.present = true;
        status.headset_battery_percent = apw.battery_percent;
        status.power_status = Some(if apw.online { "online" } else { "offline" }.to_string());
        true
    }
}

// --- the table ----------------------------------------------------------

static NOVA_PRO_OPS: NovaProOps = NovaProOps;
static ARCTIS_PRO_OPS: ArctisProOps = ArctisProOps;

/// Every supported device. A new model is one entry here — plus a
/// [`HeadsetOps`] impl if it's a headset.
pub static DEVICES: &[DeviceEntry] = &[
    DeviceEntry {
        product_ids: &protocol::PRODUCT_IDS,
        name: "Arctis Nova Pro Wireless",
        class: DeviceClass::Headset,
        // Two HID interfaces, and only the vendor control collection can drive
        // status and the OLED. It opens with usage page 0xFFC0 in HID's
        // long-item encoding (`06 c0 ff`); the other interface (consumer/media
        // keys) starts with a different page, which is what separates them.
        probe: Probe::DescriptorPrefix(&[0x06, 0xc0, 0xff]),
        report_len: protocol::COMMAND_LEN,
        caps: Caps::OLED.with(Caps::EQ).with(Caps::ANC).with(Caps::CHATMIX),
        // Beats the pre-Nova generation when both are somehow present: it is
        // the richer device.
        priority: 20,
        ops: Some(&NOVA_PRO_OPS),
    },
    DeviceEntry {
        product_ids: &protocol_apw::PRODUCT_IDS,
        name: "Arctis Pro Wireless",
        class: DeviceClass::Headset,
        // One control interface, so the product id settles it.
        probe: Probe::Any,
        report_len: protocol_apw::COMMAND_LEN,
        // No driveable OLED, no hardware EQ or ANC we speak to on this one.
        caps: Caps::NONE,
        priority: 10,
        ops: Some(&ARCTIS_PRO_OPS),
    },
    DeviceEntry {
        product_ids: &mouse_protocol::PRODUCT_IDS_WIRED,
        name: "Aerox 9 Wireless",
        class: DeviceClass::Mouse,
        // Several HID interfaces; only interface 3 accepts config commands.
        probe: Probe::InterfaceNumber(mouse_protocol::CONTROL_INTERFACE),
        // Reports are variable-length and unpadded on this device.
        report_len: 0,
        caps: Caps::RGB.with(Caps::DPI),
        // Cable beats dongle: both can be plugged in at once, and when the
        // cable is connected the mouse is physically on it — the idle dongle
        // just answers 0xff.
        priority: 20,
        ops: None,
    },
    DeviceEntry {
        product_ids: &mouse_protocol::PRODUCT_IDS_WIRELESS,
        name: "Aerox 9 Wireless",
        class: DeviceClass::Mouse,
        probe: Probe::InterfaceNumber(mouse_protocol::CONTROL_INTERFACE),
        report_len: 0,
        caps: Caps::RGB.with(Caps::DPI),
        priority: 10,
        ops: None,
    },
    DeviceEntry {
        product_ids: &kbd_protocol::PRODUCT_IDS_WIRELESS,
        name: "SteelSeries Apex (wireless)",
        class: DeviceClass::Keyboard,
        // Six HID interfaces on the board that was measured, and only the one
        // opening with the vendor usage page 0xFFC0 takes configuration. That
        // is interface 3 there, but the descriptor is the honest test — it is
        // what tells the vendor collection apart from the plain keyboard, the
        // media keys and the mouse-emulation collection.
        probe: Probe::DescriptorPrefix(&[0x06, 0xc0, 0xff]),
        report_len: kbd_protocol::COMMAND_LEN,
        caps: Caps::RGB.with(Caps::OLED),
        // Beats the wired entry: a board that exposes both is on its cable,
        // and this is the entry that knows the wireless opcodes.
        priority: 20,
        ops: None,
    },
    DeviceEntry {
        product_ids: &kbd_protocol::PRODUCT_IDS_WIRED,
        name: "SteelSeries Apex",
        class: DeviceClass::Keyboard,
        probe: Probe::InterfaceNumber(1),
        report_len: kbd_protocol::COMMAND_LEN,
        caps: Caps::RGB.with(Caps::OLED),
        priority: 10,
        ops: None,
    },
];

/// The table entry claiming this product id within a class, if any.
pub fn lookup(class: DeviceClass, product_id: u16) -> Option<&'static DeviceEntry> {
    DEVICES
        .iter()
        .find(|e| e.class == class && e.product_ids.contains(&product_id))
}

// --- discovery ----------------------------------------------------------

/// A hidraw node that matched the table.
#[derive(Clone)]
pub struct Found {
    pub dev: PathBuf,
    pub product_id: u16,
    pub entry: &'static DeviceEntry,
}

/// Walk `/sys/class/hidraw` and return the best matching node for `class`.
/// Replaces the per-family scans that used to live in `headset::hidraw` and
/// `mouse::manager`.
pub fn scan(class: DeviceClass) -> Option<Found> {
    pick(candidates(class))
}

/// Resolve competing nodes: highest priority wins, first seen breaks a tie.
/// Kept free of I/O so "Nova Pro over Arctis Pro" and "cable over dongle" are
/// testable without hardware.
pub fn pick(candidates: Vec<Found>) -> Option<Found> {
    let mut best: Option<Found> = None;
    for found in candidates {
        let better = match &best {
            None => true,
            Some(b) => found.entry.priority > b.entry.priority,
        };
        if better {
            best = Some(found);
        }
    }
    best
}

/// Every hidraw node of `class` the table claims, unresolved.
fn candidates(class: DeviceClass) -> Vec<Found> {
    let Ok(dir) = std::fs::read_dir(HIDRAW_CLASS) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in dir.flatten() {
        let name = node.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("hidraw") {
            continue;
        }
        let sys = Path::new(HIDRAW_CLASS).join(&*name).join("device");
        let Ok(uevent) = std::fs::read_to_string(sys.join("uevent")) else {
            continue;
        };
        let Some((vendor, product)) = parse_hid_id(&uevent) else {
            continue;
        };
        if vendor != VENDOR_ID {
            continue;
        }
        let Some(entry) = lookup(class, product) else {
            continue;
        };
        if !probe_matches(&entry.probe, &sys) {
            continue;
        }
        out.push(Found {
            dev: PathBuf::from("/dev").join(&*name),
            product_id: product,
            entry,
        });
    }
    out
}

/// Pull `(vendor, product)` out of a uevent's `HID_ID=0003:00001038:000012E0`.
/// Hex throughout, as the kernel writes it.
pub fn parse_hid_id(uevent: &str) -> Option<(u16, u16)> {
    let hid_id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut parts = hid_id.split(':');
    let _bus = parts.next()?;
    let vendor = u16::from_str_radix(parts.next()?.trim(), 16).ok()?;
    let product = u16::from_str_radix(parts.next()?.trim(), 16).ok()?;
    Some((vendor, product))
}

/// Confirm a product-id match really is the node we want.
fn probe_matches(probe: &Probe, sys_device: &Path) -> bool {
    match probe {
        Probe::DescriptorPrefix(prefix) => std::fs::read(sys_device.join("report_descriptor"))
            .unwrap_or_default()
            .starts_with(prefix),
        Probe::InterfaceNumber(n) => interface_number(sys_device) == Some(*n),
        Probe::Any => true,
    }
}

/// The USB interface number this HID node belongs to.
///
/// Read from `bInterfaceNumber` on the owning USB interface directory rather
/// than parsed out of the path: the HID node's own directory name looks like
/// `0003:1038:185A.0020`, whose trailing number is a HID device index, not an
/// interface — reading that instead silently matches the wrong node.
fn interface_number(sys_device: &Path) -> Option<u8> {
    let real = std::fs::canonicalize(sys_device).ok()?;
    // Walk up until a directory exposes bInterfaceNumber (usually the parent).
    let mut dir = real.as_path();
    for _ in 0..4 {
        let candidate = dir.join("bInterfaceNumber");
        if let Ok(raw) = std::fs::read_to_string(&candidate) {
            return u8::from_str_radix(raw.trim(), 16).ok();
        }
        dir = dir.parent()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(class: DeviceClass, pid: u16) -> Found {
        Found {
            dev: PathBuf::from(format!("/dev/hidraw{pid:x}")),
            product_id: pid,
            entry: lookup(class, pid).expect("pid is in the table"),
        }
    }

    const NOVA_PRO: u16 = 0x12e0;
    const ARCTIS_PRO: u16 = 0x1290;

    // --- priority resolution --------------------------------------------

    #[test]
    fn nova_pro_wins_over_arctis_pro_either_way_round() {
        let nova = found(DeviceClass::Headset, NOVA_PRO);
        let apw = found(DeviceClass::Headset, ARCTIS_PRO);
        assert_eq!(
            pick(vec![apw.clone(), nova.clone()]).unwrap().product_id,
            NOVA_PRO
        );
        assert_eq!(pick(vec![nova, apw]).unwrap().product_id, NOVA_PRO);
    }

    #[test]
    fn cable_wins_over_dongle_either_way_round() {
        let wired = found(DeviceClass::Mouse, mouse_protocol::PID_WIRED);
        let dongle = found(DeviceClass::Mouse, mouse_protocol::PID_WIRELESS);
        assert_eq!(
            pick(vec![dongle.clone(), wired.clone()]).unwrap().product_id,
            mouse_protocol::PID_WIRED
        );
        assert_eq!(
            pick(vec![wired, dongle]).unwrap().product_id,
            mouse_protocol::PID_WIRED
        );
        // The WOW variants share the same two entries, so the same rule holds.
        let wired = found(DeviceClass::Mouse, mouse_protocol::PID_WIRED_WOW);
        let dongle = found(DeviceClass::Mouse, mouse_protocol::PID_WIRELESS_WOW);
        assert_eq!(
            pick(vec![dongle, wired]).unwrap().product_id,
            mouse_protocol::PID_WIRED_WOW
        );
    }

    #[test]
    fn equal_priority_keeps_the_first_node_seen() {
        let a = found(DeviceClass::Headset, 0x12e0);
        let b = found(DeviceClass::Headset, 0x12e5);
        assert_eq!(pick(vec![a, b]).unwrap().product_id, 0x12e0);
    }

    #[test]
    fn pick_of_nothing_is_none() {
        assert!(pick(Vec::new()).is_none());
    }

    // --- table lookup ----------------------------------------------------

    #[test]
    fn lookup_is_scoped_to_the_class() {
        assert!(lookup(DeviceClass::Headset, NOVA_PRO).is_some());
        assert!(lookup(DeviceClass::Mouse, NOVA_PRO).is_none());
        assert!(lookup(DeviceClass::Mouse, mouse_protocol::PID_WIRED).is_some());
        assert!(lookup(DeviceClass::Headset, mouse_protocol::PID_WIRED).is_none());
        assert!(lookup(DeviceClass::Headset, 0x0000).is_none());
    }

    #[test]
    fn no_product_id_is_claimed_twice_within_a_class() {
        for (i, a) in DEVICES.iter().enumerate() {
            for b in DEVICES.iter().skip(i + 1) {
                if a.class != b.class {
                    continue;
                }
                for pid in a.product_ids {
                    assert!(
                        !b.product_ids.contains(pid),
                        "product id {pid:#06x} is claimed twice"
                    );
                }
            }
        }
    }

    // --- capabilities ----------------------------------------------------

    #[test]
    fn caps_per_entry() {
        let nova = lookup(DeviceClass::Headset, NOVA_PRO).unwrap();
        assert!(nova.caps.has(Caps::OLED));
        assert!(nova.caps.has(Caps::EQ));
        assert!(nova.caps.has(Caps::ANC));
        assert!(nova.caps.has(Caps::CHATMIX));
        assert!(!nova.caps.has(Caps::DPI));

        // The old DeviceKind::has_oled() contract, now a capability.
        let apw = lookup(DeviceClass::Headset, ARCTIS_PRO).unwrap();
        assert!(!apw.caps.has(Caps::OLED));
        assert!(!apw.caps.has(Caps::CHATMIX));

        for pid in [
            mouse_protocol::PID_WIRED,
            mouse_protocol::PID_WIRELESS,
            mouse_protocol::PID_WIRED_WOW,
            mouse_protocol::PID_WIRELESS_WOW,
        ] {
            let mouse = lookup(DeviceClass::Mouse, pid).unwrap();
            assert!(mouse.caps.has(Caps::RGB.with(Caps::DPI)));
            assert!(!mouse.caps.has(Caps::OLED));
        }
    }

    #[test]
    fn caps_none_contains_nothing() {
        assert!(!Caps::NONE.has(Caps::OLED));
        assert!(Caps::OLED.with(Caps::EQ).has(Caps::OLED.with(Caps::EQ)));
        assert!(!Caps::OLED.has(Caps::OLED.with(Caps::EQ)));
    }

    // --- report lengths and probes ---------------------------------------

    #[test]
    fn report_len_matches_each_family() {
        assert_eq!(
            lookup(DeviceClass::Headset, NOVA_PRO).unwrap().report_len,
            protocol::COMMAND_LEN
        );
        assert_eq!(
            lookup(DeviceClass::Headset, ARCTIS_PRO).unwrap().report_len,
            protocol_apw::COMMAND_LEN
        );
    }

    #[test]
    fn probes_match_how_each_device_is_identified() {
        assert_eq!(
            lookup(DeviceClass::Headset, NOVA_PRO).unwrap().probe,
            Probe::DescriptorPrefix(&[0x06, 0xc0, 0xff])
        );
        assert_eq!(
            lookup(DeviceClass::Headset, ARCTIS_PRO).unwrap().probe,
            Probe::Any
        );
        assert_eq!(
            lookup(DeviceClass::Mouse, mouse_protocol::PID_WIRED)
                .unwrap()
                .probe,
            Probe::InterfaceNumber(3)
        );
    }

    #[test]
    fn every_headset_entry_carries_ops() {
        for entry in DEVICES.iter().filter(|e| e.class == DeviceClass::Headset) {
            assert!(entry.ops.is_some(), "{} has no ops", entry.name);
        }
    }

    // --- HID_ID parsing ---------------------------------------------------

    #[test]
    fn parses_a_real_uevent() {
        let uevent = "DRIVER=hid-generic\nHID_ID=0003:00001038:000012E0\nHID_NAME=SteelSeries\n";
        assert_eq!(parse_hid_id(uevent), Some((0x1038, 0x12e0)));
    }

    #[test]
    fn hid_id_is_parsed_as_hex_not_decimal() {
        // 0x1858 would be 6232 in decimal; a decimal parse would fail outright.
        assert_eq!(
            parse_hid_id("HID_ID=0003:00001038:00001858"),
            Some((0x1038, 0x1858))
        );
    }

    #[test]
    fn rejects_broken_hid_ids() {
        assert_eq!(parse_hid_id(""), None);
        assert_eq!(parse_hid_id("DRIVER=hid-generic\n"), None);
        // Missing the product field.
        assert_eq!(parse_hid_id("HID_ID=0003:00001038"), None);
        // Non-hex garbage.
        assert_eq!(parse_hid_id("HID_ID=0003:zzzz:000012E0"), None);
        assert_eq!(parse_hid_id("HID_ID=0003:00001038:oops"), None);
        // Doesn't fit a u16 (leading zeros are fine, an extra digit isn't).
        assert_eq!(parse_hid_id("HID_ID=0003:00011038:000012E0"), None);
        assert_eq!(parse_hid_id("HID_ID=0003:00001038:000112E0"), None);
        // Right prefix, no value.
        assert_eq!(parse_hid_id("HID_ID="), None);
    }

    #[test]
    fn tolerates_trailing_whitespace() {
        assert_eq!(
            parse_hid_id("HID_ID=0003:00001038 : 000012E5 "),
            Some((0x1038, 0x12e5))
        );
    }

    // --- headset ops ------------------------------------------------------

    fn ops(pid: u16) -> &'static dyn HeadsetOps {
        lookup(DeviceClass::Headset, pid).unwrap().ops.unwrap()
    }

    #[test]
    fn arctis_pro_scales_sidetone_from_the_uis_0_3_to_0_9() {
        for (ui, wire) in [(0u8, 0u8), (1, 3), (2, 6), (3, 9)] {
            let packets = ops(ARCTIS_PRO).sidetone(ui);
            assert_eq!(packets[0], protocol_apw::set_sidetone(wire));
            // Nothing sticks on this generation without an explicit save.
            assert_eq!(packets[1], protocol_apw::save());
            assert_eq!(packets.len(), 2);
        }
        // Out-of-range input clamps to the top step, not past it.
        assert_eq!(ops(ARCTIS_PRO).sidetone(200)[0][2], 9);
    }

    #[test]
    fn nova_pro_passes_sidetone_through_unscaled() {
        let packets = ops(NOVA_PRO).sidetone(2);
        assert_eq!(packets, vec![protocol::set_sidetone(2)]);
    }

    #[test]
    fn arctis_pro_translates_the_auto_off_step_index_to_minutes() {
        // Step index -> minutes -> the device's minutes/10 wire value.
        for (idx, wire) in [
            (0u8, 0x00u8),
            (1, 0x01), // 1 min  -> 10 min
            (2, 0x01), // 5 min  -> 10 min
            (3, 0x01), // 10 min
            (4, 0x01), // 15 min -> still the 10 min bucket
            (5, 0x03), // 30 min
            (6, 0x06), // 60 min
        ] {
            let packets = ops(ARCTIS_PRO).auto_off(idx);
            assert_eq!(packets[0][2], wire, "step {idx}");
            assert_eq!(packets[1], protocol_apw::save());
        }
        // Past the end of the table clamps instead of panicking.
        assert_eq!(ops(ARCTIS_PRO).auto_off(200)[0][2], 0x06);
    }

    #[test]
    fn nova_pro_passes_the_auto_off_index_through() {
        assert_eq!(ops(NOVA_PRO).auto_off(4), vec![protocol::set_auto_off(4)]);
    }

    #[test]
    fn heartbeats_match_each_family() {
        assert_eq!(
            ops(NOVA_PRO).heartbeat(),
            vec![protocol::status_query(), protocol::volume_query()]
        );
        assert_eq!(
            ops(ARCTIS_PRO).heartbeat(),
            vec![protocol_apw::status_query()]
        );
    }

    #[test]
    fn init_safe_matches_each_family() {
        assert_eq!(ops(NOVA_PRO).init_safe(), protocol::init_safe());
        assert_eq!(ops(ARCTIS_PRO).init_safe(), protocol_apw::init_safe());
    }

    #[test]
    fn arctis_pro_folds_its_small_frame_into_the_shared_status() {
        let mut status = HeadsetStatus::default();
        // [4, 2, ...] = online, battery level 2 of 4.
        assert!(ops(ARCTIS_PRO).apply_status_frame(&[0x04, 0x02, 0x00], &mut status));
        assert!(status.present);
        assert_eq!(status.headset_battery_percent, Some(50));
        assert_eq!(status.power_status.as_deref(), Some("online"));

        assert!(ops(ARCTIS_PRO).apply_status_frame(&[0x02, 0x04, 0x00], &mut status));
        assert_eq!(status.power_status.as_deref(), Some("offline"));

        // Unrelated frames leave the snapshot alone.
        assert!(!ops(ARCTIS_PRO).apply_status_frame(&[0xff, 0x00], &mut status));
        assert_eq!(status.power_status.as_deref(), Some("offline"));
    }
}
