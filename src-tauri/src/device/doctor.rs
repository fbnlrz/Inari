//! `inari doctor` — what Inari sees when it looks for hardware.
//!
//! This exists because "the headset only works if I unplug it and restart
//! Inari" is impossible to diagnose from the outside. Every step discovery
//! takes is invisible: which `/dev/hidraw*` nodes exist, which the table
//! claims, whether the probe matched, whether the node can be opened at all,
//! and — the part that actually decided it — whether the device answers.
//!
//! It runs in the calling process and prints to the caller's terminal, unlike
//! the control commands, which are forwarded to the running instance and print
//! into *its* log. So it works on a machine where Inari is not running, or
//! running badly, which is exactly when it is needed.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use super::{parse_hid_id, DeviceClass, DEVICES, VENDOR_ID};

/// How long a node gets to answer before the report calls it silent.
const ANSWER_TIMEOUT: Duration = Duration::from_millis(1500);

/// Build the report. Returns the text rather than printing it so it can be
/// tested and, later, shown in the app.
pub fn report() -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Inari {} — device report", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "session: {}  seat: {}",
        env_or("XDG_SESSION_TYPE"),
        env_or("XDG_SEAT")
    );
    let _ = writeln!(out);

    let nodes = steelseries_nodes();
    if nodes.is_empty() {
        let _ = writeln!(out, "No SteelSeries HID nodes found at all.");
        let _ = writeln!(
            out,
            "  The devices are either not attached, or the kernel has not bound\n  \
             them to hidraw. Check `lsusb -d {VENDOR_ID:04x}:` first."
        );
        return out;
    }

    let _ = writeln!(out, "SteelSeries HID nodes ({}):", nodes.len());
    for node in &nodes {
        let _ = writeln!(out, "  {}", node.summary());
    }
    let _ = writeln!(out);

    for class in [
        DeviceClass::Headset,
        DeviceClass::Mouse,
        DeviceClass::Keyboard,
    ] {
        let _ = writeln!(out, "{class:?}:");
        let candidates = super::scan_all(class);
        if candidates.is_empty() {
            let claimed: Vec<&Node> = nodes
                .iter()
                .filter(|n| super::lookup(class, n.product_id).is_some())
                .collect();
            if claimed.is_empty() {
                let _ = writeln!(out, "  nothing of this class attached");
            } else {
                // The interesting failure: the device is there, but no node
                // passed the probe. That is a wrong-interface or missing
                // descriptor problem, not a missing device.
                let _ = writeln!(
                    out,
                    "  {} node(s) match a known product id but NONE passed the probe:",
                    claimed.len()
                );
                for node in claimed {
                    let _ = writeln!(out, "    {}", node.summary());
                }
            }
            let _ = writeln!(out);
            continue;
        }

        let _ = writeln!(
            out,
            "  {} candidate(s), in the order Inari tries them:",
            candidates.len()
        );
        for (i, found) in candidates.iter().enumerate() {
            let dev = found.dev.display().to_string();
            let answered = probe_answer(&found.dev);
            let _ = writeln!(
                out,
                "   {}. {dev} — {} ({:#06x}) — {}",
                i + 1,
                found.entry.name,
                found.product_id,
                answered
            );
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(
        out,
        "If a node reads \"cannot open (permission denied)\", the udev rule is not\n\
         applied. It ships as /usr/lib/udev/rules.d/60-inari.rules; after installing\n\
         it, either replug the device or run:\n  \
         sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=hidraw"
    );
    out
}

fn env_or(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| "?".into())
}

/// A `/dev/hidraw*` node belonging to SteelSeries.
struct Node {
    name: String,
    product_id: u16,
    descriptor: Vec<u8>,
    interface: Option<u8>,
    open: io::Result<()>,
}

impl Node {
    fn summary(&self) -> String {
        let prefix: Vec<String> = self
            .descriptor
            .iter()
            .take(3)
            .map(|b| format!("{b:02x}"))
            .collect();
        let claimed = DEVICES
            .iter()
            .find(|e| e.product_ids.contains(&self.product_id))
            .map(|e| e.name)
            .unwrap_or("not in Inari's table");
        format!(
            "/dev/{} pid={:#06x} iface={} desc={} — {} — {}",
            self.name,
            self.product_id,
            self.interface
                .map(|i| i.to_string())
                .unwrap_or_else(|| "?".into()),
            prefix.join(" "),
            claimed,
            match &self.open {
                Ok(()) => "opens".to_string(),
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied =>
                    "cannot open (permission denied)".to_string(),
                Err(e) => format!("cannot open ({e})"),
            }
        )
    }
}

fn steelseries_nodes() -> Vec<Node> {
    let Ok(dir) = fs::read_dir("/sys/class/hidraw") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("hidraw") {
            continue;
        }
        let sys = Path::new("/sys/class/hidraw").join(&name).join("device");
        let Ok(uevent) = fs::read_to_string(sys.join("uevent")) else {
            continue;
        };
        let Some((vendor, product_id)) = parse_hid_id(&uevent) else {
            continue;
        };
        if vendor != VENDOR_ID {
            continue;
        }
        out.push(Node {
            open: fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(format!("/dev/{name}"))
                .map(|_| ()),
            descriptor: fs::read(sys.join("report_descriptor")).unwrap_or_default(),
            interface: super::interface_number(&sys),
            product_id,
            name,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Open the node and see whether anything arrives. Deliberately passive: it
/// writes nothing, so running the report cannot change a device's settings.
/// A device that only speaks when spoken to therefore reads as "silent while
/// idle", which the wording reflects.
fn probe_answer(dev: &Path) -> String {
    let Ok(file) = fs::OpenOptions::new().read(true).write(true).open(dev) else {
        return "cannot open — check permissions (udev rule)".into();
    };
    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let deadline = Instant::now() + ANSWER_TIMEOUT;
    while Instant::now() < deadline {
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one initialised pollfd, and the count matches.
        let ready = unsafe { libc::poll(&mut pfd, 1, 200) };
        if ready > 0 {
            if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return "node hung up".into();
            }
            return "opens, and is sending reports".into();
        }
    }
    "opens, silent while idle (normal for a device that only answers queries)".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_names_the_machine_and_never_panics() {
        // Runs against whatever is (or is not) attached to the build machine,
        // so it must be safe with zero devices and with a full desk.
        let text = report();
        assert!(text.contains("device report"));
        assert!(text.contains("session:"));
    }

    #[test]
    fn a_report_without_hardware_says_what_to_check() {
        // The empty case is the one a user hits first, so it has to be
        // actionable rather than just "nothing found".
        if steelseries_nodes().is_empty() {
            let text = report();
            assert!(text.contains("lsusb"));
        }
    }
}
