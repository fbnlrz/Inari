//! Direct Linux `hidraw` access for the base station — no libhidapi/libusb.
//!
//! Control and status packets are ordinary HID output/input reports (plain
//! `write`/`read`). The OLED framebuffer is a **Feature** report, which cannot
//! go through `write` — it needs the `HIDIOCSFEATURE` ioctl.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::device::{self, DeviceClass, DeviceEntry};

use super::protocol::COMMAND_LEN;

/// How long one [`HidReader::read_report`] wait parks in `poll` before handing
/// control back. It is *not* a device deadline — a healthy base station may
/// legitimately stay silent for minutes — only how often the read loop gets a
/// chance to notice it has been asked to stop. 500 ms is two wake-ups per
/// second doing nothing but re-checking a flag: far too cheap to matter, far
/// too short to make teardown feel stuck.
pub const READ_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Bound on the pre-flight writability wait in [`HidDevice::write_command`].
/// hidraw always reports itself writable (it queues the report to the USB
/// stack rather than to a bounded buffer), so on a live device this returns on
/// the first `poll` and the value is never reached; it only caps the wait if a
/// future kernel ever does block on writability.
const WRITE_READY_TIMEOUT: Duration = Duration::from_secs(1);

/// `HIDIOCSFEATURE(len)` = `_IOC(_IOC_WRITE | _IOC_READ, 'H', 0x06, len)`.
/// Encoded per the asm-generic ioctl layout (dir<<30 | size<<16 | type<<8 | nr).
fn hidiocsfeature(len: usize) -> libc::c_ulong {
    const DIR_WRITE_READ: libc::c_ulong = 3;
    ((DIR_WRITE_READ << 30)
        | ((len as libc::c_ulong) << 16)
        | ((b'H' as libc::c_ulong) << 8)
        | 0x06) as libc::c_ulong
}

/// Outcome of waiting on the hidraw node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Readiness {
    /// The requested direction is ready right now.
    Ready,
    /// The wait expired; the node is fine, just idle.
    NotReady,
    /// The node is dead (device unplugged, or the fd went bad).
    Gone,
}

/// `poll(2)` takes whole milliseconds in an `int`. Round *up*: truncating
/// would both let a wait expire before its deadline and turn a sub-millisecond
/// remainder into poll's non-blocking probe, busy-spinning the read loop.
/// Clamp long waits so the cast can't wrap into a negative (= infinite) one.
fn poll_timeout_ms(d: Duration) -> libc::c_int {
    let ms = d.as_nanos().div_ceil(1_000_000);
    ms.min(libc::c_int::MAX as u128) as libc::c_int
}

/// Interpret a `revents` mask for a wait on `want`.
///
/// Error flags are checked first on purpose: hidraw reports a node as
/// *writable* unconditionally and only ever adds `POLLERR`/`POLLHUP` once the
/// underlying device is gone, so on the write side those bits are the only
/// signal there is.
fn classify(revents: libc::c_short, want: libc::c_short) -> Readiness {
    if revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        Readiness::Gone
    } else if revents & want != 0 {
        Readiness::Ready
    } else {
        Readiness::NotReady
    }
}

/// Wait for `fd` to become ready for `events`, giving up after `timeout`.
/// Retries across `EINTR` against the original deadline so a stray signal
/// cannot masquerade as a timeout.
fn poll_fd(fd: RawFd, events: libc::c_short, timeout: Duration) -> io::Result<Readiness> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let mut pfd = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        // SAFETY: one initialised `pollfd`, and the count we pass matches.
        let ret = unsafe { libc::poll(&mut pfd, 1, poll_timeout_ms(remaining)) };
        if ret == 0 {
            return Ok(Readiness::NotReady);
        }
        if ret > 0 {
            return Ok(classify(pfd.revents, events));
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
        if Instant::now() >= deadline {
            return Ok(Readiness::NotReady);
        }
    }
}

/// The node is still there but no longer usable.
fn gone() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "hidraw node hung up")
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
        // Bounded pre-flight check: `write` on hidraw blocks in the USB stack
        // and callers hold the device mutex across it, so a node whose device
        // has vanished must surface as an error here rather than as a stalled
        // command layer. Cannot false-fail — hidraw is always writable until
        // it hangs up.
        self.wait_writable()?;
        self.file.write_all(&report)?;
        Ok(())
    }

    /// Wait (briefly) until the node accepts writes, mapping a hung-up device
    /// to an error instead of letting the write park in the kernel.
    fn wait_writable(&self) -> io::Result<()> {
        match poll_fd(self.file.as_raw_fd(), libc::POLLOUT, WRITE_READY_TIMEOUT)? {
            Readiness::Ready => Ok(()),
            Readiness::Gone => Err(gone()),
            Readiness::NotReady => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "hidraw node not writable",
            )),
        }
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

    /// Open a second handle to the same node for a reader thread, so reads
    /// never contend with command writes on one fd.
    pub fn open_reader(&self) -> io::Result<HidReader> {
        // No O_NONBLOCK (the default): the wait happens in `poll`, and the
        // `read` that follows only runs once poll says a report is queued, so
        // it returns immediately and never spins on EAGAIN.
        let file = OpenOptions::new().read(true).write(true).open(&self.path)?;
        Ok(HidReader { file })
    }
}

/// A read handle for the status/event stream.
pub struct HidReader {
    file: File,
}
impl HidReader {
    /// Wait up to `timeout` for the next HID input report and return its
    /// length. `Ok(None)` means the wait expired with nothing queued — a
    /// present but silent device — which lets the caller re-check its stop
    /// flag instead of parking in `read` for the lifetime of the process.
    pub fn read_report(
        &mut self,
        buf: &mut [u8; COMMAND_LEN],
        timeout: Duration,
    ) -> io::Result<Option<usize>> {
        match poll_fd(self.file.as_raw_fd(), libc::POLLIN, timeout)? {
            Readiness::NotReady => Ok(None),
            Readiness::Gone => Err(gone()),
            Readiness::Ready => self.file.read(buf).map(Some),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pipe stands in for the hidraw node: same poll semantics on the read
    /// end (data queued -> POLLIN, writer gone -> POLLHUP), no hardware.
    ///
    /// `O_CLOEXEC` is not decoration. Other tests in this binary shell out
    /// (`nvidia-smi`, `playerctl`, `sensors`), and a plain `pipe(2)` leaks its
    /// write end into every process forked while this test runs. That extra
    /// reference keeps the pipe alive after the test closes its own end, so
    /// the hangup this test is about never arrives and it fails — but only
    /// when run alongside the tests that fork, which is exactly the sort of
    /// failure that gets written off as flaky.
    fn pipe() -> (RawFd, RawFd) {
        let mut fds = [0 as RawFd; 2];
        // SAFETY: `fds` is a two-element array, exactly what pipe2(2) fills.
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        (fds[0], fds[1])
    }

    fn close(fd: RawFd) {
        // SAFETY: fd was handed to us by pipe(2) and is closed once.
        unsafe { libc::close(fd) };
    }

    #[test]
    fn feature_ioctl_matches_asm_generic_layout() {
        // dir=3, size=1024, type='H', nr=6 — pins the encoding the OLED
        // framebuffer depends on.
        assert_eq!(hidiocsfeature(1024), 0xC400_4806);
    }

    #[test]
    fn sub_millisecond_timeout_never_degrades_to_a_zero_wait() {
        assert_eq!(poll_timeout_ms(Duration::ZERO), 0);
        assert_eq!(poll_timeout_ms(Duration::from_nanos(1)), 1);
        assert_eq!(poll_timeout_ms(Duration::from_micros(999)), 1);
        assert_eq!(poll_timeout_ms(Duration::from_millis(500)), 500);
        assert_eq!(poll_timeout_ms(Duration::from_secs(u64::MAX / 2)), i32::MAX);
    }

    #[test]
    fn hangup_outranks_a_ready_direction() {
        assert_eq!(classify(libc::POLLIN, libc::POLLIN), Readiness::Ready);
        assert_eq!(classify(0, libc::POLLIN), Readiness::NotReady);
        assert_eq!(classify(libc::POLLIN, libc::POLLOUT), Readiness::NotReady);
        // hidraw keeps asserting POLLOUT after the device disappears, so the
        // error bits have to win or a dead node would look writable forever.
        assert_eq!(
            classify(libc::POLLOUT | libc::POLLHUP, libc::POLLOUT),
            Readiness::Gone
        );
        assert_eq!(classify(libc::POLLERR, libc::POLLIN), Readiness::Gone);
        assert_eq!(classify(libc::POLLNVAL, libc::POLLIN), Readiness::Gone);
    }

    #[test]
    fn poll_fd_reports_idle_then_ready_then_hangup() {
        let (r, w) = pipe();

        // Silent writer: the wait must expire rather than return early or
        // spuriously ready — that is what keeps the read loop from spinning.
        let started = Instant::now();
        assert_eq!(
            poll_fd(r, libc::POLLIN, Duration::from_millis(20)).unwrap(),
            Readiness::NotReady
        );
        assert!(started.elapsed() >= Duration::from_millis(20));

        // SAFETY: writing one byte from a valid one-byte buffer to our pipe.
        assert_eq!(unsafe { libc::write(w, b"x".as_ptr().cast(), 1) }, 1);
        assert_eq!(
            poll_fd(r, libc::POLLIN, Duration::from_secs(1)).unwrap(),
            Readiness::Ready
        );

        close(w);
        assert_eq!(
            poll_fd(r, libc::POLLIN, Duration::from_secs(1)).unwrap(),
            Readiness::Gone
        );
        close(r);
    }
}
