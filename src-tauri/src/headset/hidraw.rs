//! Direct Linux `hidraw` access for the base station — no libhidapi/libusb.
//!
//! Control and status packets are ordinary HID output/input reports (plain
//! `write`/`read`). The OLED framebuffer is a **Feature** report, which cannot
//! go through `write` — it needs the `HIDIOCSFEATURE` ioctl.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use crate::device::{self, DeviceClass, DeviceEntry};

use super::protocol::COMMAND_LEN;

/// `HIDIOCSFEATURE(len)` = `_IOC(_IOC_WRITE | _IOC_READ, 'H', 0x06, len)`.
/// Encoded per the asm-generic ioctl layout (dir<<30 | size<<16 | type<<8 | nr).
fn hidiocsfeature(len: usize) -> libc::c_ulong {
    const DIR_WRITE_READ: libc::c_ulong = 3;
    ((DIR_WRITE_READ << 30)
        | ((len as libc::c_ulong) << 16)
        | ((b'H' as libc::c_ulong) << 8)
        | 0x06) as libc::c_ulong
}

/// A resolved base-station control node plus the table entry that claimed it.
/// Discovery itself lives in [`crate::device`], which knows the descriptor
/// prefix that separates Nova Pro's control interface from its media-key one
/// and the priority that lets Nova Pro win over the pre-Nova generation.
pub type DevicePath = device::Found;

/// A writable handle to the base station: output reports (commands) and
/// feature reports (OLED). Cheap to clone the path; the file is the resource.
pub struct HidDevice {
    file: File,
    pub product_id: u16,
    pub entry: &'static DeviceEntry,
    pub path: PathBuf,
}

impl HidDevice {
    /// Open the discovered control node for read+write.
    pub fn open(path: &DevicePath) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path.dev)?;
        Ok(Self {
            file,
            product_id: path.product_id,
            entry: path.entry,
            path: path.dev.clone(),
        })
    }

    /// Discover and open the base station in one step.
    pub fn discover() -> io::Result<Self> {
        let path = device::scan(DeviceClass::Headset).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "no Arctis Nova Pro Wireless base station found",
            )
        })?;
        Self::open(&path)
    }

    /// Reconstruct the discovery path so a second fd (reader/OLED) can be
    /// opened for the same node.
    pub fn device_path(&self) -> DevicePath {
        DevicePath {
            dev: self.path.clone(),
            product_id: self.product_id,
            entry: self.entry,
        }
    }

    /// Write a command as a HID **output** report, zero-padded to this
    /// family's report length (64 bytes on Nova Pro, 31 on Arctis Pro).
    pub fn write_command(&mut self, body: &[u8]) -> io::Result<()> {
        let len = self.entry.report_len;
        let mut report = vec![0u8; len];
        let n = body.len().min(len);
        report[..n].copy_from_slice(&body[..n]);
        self.file.write_all(&report)?;
        Ok(())
    }

    /// Send a HID **feature** report (used for OLED framebuffer packets).
    /// `data[0]` must be the report id; `data` is sent verbatim.
    pub fn send_feature(&mut self, data: &[u8]) -> io::Result<()> {
        let fd = self.file.as_raw_fd();
        // SAFETY: `data` outlives the call and its length matches the size we
        // encode into the ioctl request number.
        let ret = unsafe {
            libc::ioctl(
                fd,
                hidiocsfeature(data.len()),
                data.as_ptr() as *const libc::c_void,
            )
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Open a second, blocking handle to the same node for a reader thread, so
    /// blocking reads never contend with command writes on one fd.
    pub fn open_reader(&self) -> io::Result<HidReader> {
        // No O_NONBLOCK (the default), so the reader thread blocks until an
        // event arrives instead of spinning on EAGAIN.
        let file = OpenOptions::new().read(true).write(true).open(&self.path)?;
        Ok(HidReader { file })
    }
}

/// A blocking read handle for the status/event stream.
pub struct HidReader {
    file: File,
}
impl HidReader {
    /// Block until the next HID input report arrives; returns its length.
    pub fn read_report(&mut self, buf: &mut [u8; COMMAND_LEN]) -> io::Result<usize> {
        self.file.read(buf)
    }
}
