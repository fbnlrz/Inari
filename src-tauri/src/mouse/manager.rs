//! Discovery, connection supervision and command dispatch for the mouse.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::device::{self, DeviceClass};

use super::protocol::{self, MouseConfig};

const EV_STATUS: &str = "mouse-status";
const EV_PRESENCE: &str = "mouse-presence";

/// Live mouse state pushed to the UI.
#[derive(Debug, Clone, Serialize, Default)]
pub struct MouseStatus {
    pub present: bool,
    pub model: Option<String>,
    pub wireless: bool,
    pub battery_percent: Option<u8>,
    pub charging: bool,
}

/// A discovered mouse control node. Discovery — the interface-3 probe and the
/// cable-beats-dongle preference — lives in [`crate::device`].
type MousePath = device::Found;

pub struct MouseManager {
    dev: Mutex<Option<MousePath>>,
    status: Mutex<MouseStatus>,
    config: Mutex<MouseConfig>,
    connected: AtomicBool,
    app: Mutex<Option<AppHandle>>,
}

impl Default for MouseManager {
    fn default() -> Self {
        Self {
            dev: Mutex::new(None),
            status: Mutex::new(MouseStatus::default()),
            config: Mutex::new(MouseConfig::default()),
            connected: AtomicBool::new(false),
            app: Mutex::new(None),
        }
    }
}

impl MouseManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn status(&self) -> MouseStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn config(&self) -> MouseConfig {
        self.config.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// The product id of the connected mouse (commands are encoded per
    /// transport, so callers need it).
    pub fn product_id(&self) -> Option<u16> {
        self.dev.lock().ok().and_then(|d| d.as_ref().map(|d| d.product_id))
    }

    /// Send a pre-encoded command to the mouse.
    pub fn send(&self, packet: &[u8]) -> Result<(), String> {
        let guard = self.dev.lock().map_err(|_| "mouse lock poisoned")?;
        let path = guard.as_ref().ok_or("no SteelSeries mouse connected")?;
        let mut file = OpenOptions::new()
            .write(true)
            .open(&path.dev)
            .map_err(|e| format!("open mouse: {e}"))?;
        file.write_all(packet)
            .map_err(|e| format!("write mouse: {e}"))?;
        // The device drops commands sent back-to-back; rivalcfg uses ~50 ms.
        std::thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    /// Apply a command built from the connected product id, then persist.
    pub fn send_with_pid<F>(&self, build: F) -> Result<(), String>
    where
        F: FnOnce(u16) -> Vec<u8>,
    {
        let pid = self.product_id().ok_or("no SteelSeries mouse connected")?;
        self.send(&build(pid))
    }

    /// Persist current settings to the mouse's onboard memory.
    pub fn save(&self) -> Result<(), String> {
        self.send_with_pid(protocol::save)
    }

    /// Replace the cached config (so the UI survives a reconnect).
    pub fn set_config(&self, config: MouseConfig) {
        if let Ok(mut c) = self.config.lock() {
            *c = config;
        }
    }

    pub fn start(self: &Arc<Self>, app: AppHandle) {
        if let Ok(mut slot) = self.app.lock() {
            *slot = Some(app);
        }
        let me = Arc::clone(self);
        std::thread::spawn(move || me.supervise());
    }

    fn emit_presence(&self, present: bool) {
        self.connected.store(present, Ordering::Relaxed);
        if let Ok(app) = self.app.lock() {
            if let Some(app) = app.as_ref() {
                let _ = app.emit(EV_PRESENCE, present);
            }
        }
    }

    fn emit_status(&self) {
        if let (Ok(app), Ok(status)) = (self.app.lock(), self.status.lock()) {
            if let Some(app) = app.as_ref() {
                let _ = app.emit(EV_STATUS, &*status);
            }
        }
    }

    /// Poll for the mouse; while it's present, refresh battery periodically.
    fn supervise(self: Arc<Self>) {
        loop {
            let found = device::scan(DeviceClass::Mouse);
            match found {
                Some(path) => {
                    let wireless = protocol::is_wireless(path.product_id);
                    let first_time = !self.is_connected();
                    let model = path.entry.name;
                    if let Ok(mut slot) = self.dev.lock() {
                        *slot = Some(path.clone());
                    }
                    if first_time {
                        if let Ok(mut s) = self.status.lock() {
                            s.present = true;
                            s.wireless = wireless;
                            s.model = Some(model.to_string());
                        }
                        self.emit_presence(true);
                    }
                    self.refresh_battery(&path);
                    std::thread::sleep(Duration::from_secs(30));
                }
                None => {
                    if self.is_connected() {
                        if let Ok(mut slot) = self.dev.lock() {
                            *slot = None;
                        }
                        if let Ok(mut s) = self.status.lock() {
                            *s = MouseStatus::default();
                        }
                        self.emit_presence(false);
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
        }
    }

    /// Query battery and push the result to the UI. Best effort: a mouse that
    /// is asleep simply won't answer.
    fn refresh_battery(&self, path: &MousePath) {
        let Ok(mut file) = OpenOptions::new().read(true).write(true).open(&path.dev) else {
            return;
        };
        if file
            .write_all(&protocol::battery_query(path.product_id))
            .is_err()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(60));
        let mut buf = [0u8; 8];
        use std::io::Read;
        let Ok(n) = file.read(&mut buf) else { return };
        if n == 0 {
            return;
        }
        if let Some((percent, charging)) = protocol::parse_battery(&buf[..n]) {
            if let Ok(mut s) = self.status.lock() {
                s.battery_percent = Some(percent);
                s.charging = charging;
            }
            self.emit_status();
        }
    }
}
