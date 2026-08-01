//! Discovery, connection supervision and the draw loop for the keyboard.
//!
//! One worker owns the device: lighting frames and OLED frames go down the same
//! fd, and serialising them in one place is simpler — and kinder to the USB
//! stack — than two threads fighting over a mutex. It wakes 30 times a second,
//! but only writes when something actually changed, so a static colour costs
//! one report and then nothing.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::audio::pw_native::levels::LevelStore;
use crate::device::{self, DeviceClass};
use crate::headset::hidraw::HidDevice;
use crate::headset::oled::Framebuffer;
use crate::persistence::keyboard::{self, KeyboardConfig};

use super::effects::{self, EffectKind};
use super::firmware::{self, LightingConfig, PowerSaving};
use super::oled;
use super::oled_modes::{Screens, StatusLines};
use super::protocol::{self, FormFactor, Gen, Model, OledWire};

const EV_STATUS: &str = "keyboard-status";
const EV_PRESENCE: &str = "keyboard-presence";

/// Draw cadence. Fast enough for a smooth wave, slow enough that a keyboard
/// doing nothing costs nothing.
const TICK: Duration = Duration::from_millis(33);
/// How often the battery is re-read on a wireless board.
const BATTERY_INTERVAL: Duration = Duration::from_secs(60);
/// How long to wait between discovery attempts while nothing is attached.
const RESCAN_INTERVAL: Duration = Duration::from_secs(5);
/// Consecutive failed writes before the device is treated as gone.
const MAX_FAILURES: u32 = 8;
/// How long to wait for the reply that matches a query. Generous, because the
/// device interleaves answers and this loop discards the ones that belong to
/// other commands.
const QUERY_TIMEOUT: Duration = Duration::from_millis(700);
/// How often an unchanged OLED frame is re-sent. The keyboard's firmware
/// repaints its own screen whenever it feels like it — a volume change, a
/// profile switch — and would otherwise leave Inari's picture half overwritten.
const OLED_REFRESH: Duration = Duration::from_millis(500);

/// Live keyboard state pushed to the UI.
#[derive(Debug, Clone, Serialize, Default)]
pub struct KeyboardStatus {
    pub present: bool,
    pub model: Option<String>,
    pub product_id: u16,
    pub form: Option<FormFactor>,
    pub generation: Option<Gen>,
    pub wireless: bool,
    pub firmware: Option<String>,
    pub battery_percent: Option<u8>,
    /// On the cable and charging.
    pub charging: bool,
    /// The keyboard's region byte. Gen 2+ boards have no serial; this stands
    /// in for one, and it also hints at the physical layout.
    pub region: Option<u8>,
    /// The keyboard has a panel Inari knows how to drive.
    pub has_oled: bool,
    /// The switches are adjustable — which is not the same as Inari being able
    /// to adjust them; see [`protocol::actuation`].
    pub has_actuation: bool,
    /// Which OLED transport ended up being used, for the UI to show.
    pub oled_wire: Option<OledWire>,
    /// The keyboard's own lighting timers, as it reports them. The UI shows
    /// these rather than Inari's stored guess, so the sliders start on what
    /// the board is actually doing.
    pub lighting: Option<LightingConfig>,
    pub power: Option<PowerSaving>,
}

pub struct KeyboardManager {
    dev: Mutex<Option<device::Found>>,
    status: Mutex<KeyboardStatus>,
    config: Mutex<KeyboardConfig>,
    connected: AtomicBool,
    /// Set whenever the config changes, so the worker repaints once even for a
    /// still effect.
    dirty: AtomicBool,
    /// The keyboard only wants the fallback report envelope. Latched on the
    /// first successful write so the retry happens once, not every frame.
    alt_envelope: AtomicBool,
    /// Index into [`oled::candidates`] that the panel accepted.
    oled_choice: AtomicU8,
    app: Mutex<Option<AppHandle>>,
    levels: Mutex<Option<Arc<LevelStore>>>,
}

impl Default for KeyboardManager {
    /// A manager with default settings and nothing connected. [`Self::new`] is
    /// what the app builds — it starts from the saved configuration; this one
    /// deliberately touches no disk, which is also what the tests want.
    fn default() -> Self {
        Self {
            dev: Mutex::new(None),
            status: Mutex::new(KeyboardStatus::default()),
            config: Mutex::new(KeyboardConfig::default()),
            connected: AtomicBool::new(false),
            dirty: AtomicBool::new(true),
            alt_envelope: AtomicBool::new(false),
            oled_choice: AtomicU8::new(0),
            app: Mutex::new(None),
            levels: Mutex::new(None),
        }
    }
}

impl KeyboardManager {
    pub fn new() -> Arc<Self> {
        let manager = Self::default();
        if let Ok(mut config) = manager.config.lock() {
            *config = keyboard::load();
        }
        Arc::new(manager)
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn status(&self) -> KeyboardStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn config(&self) -> KeyboardConfig {
        self.config
            .lock()
            .map(|c| c.clone())
            .unwrap_or_else(|_| KeyboardConfig::default())
    }

    /// The peak-level store, for the audio-reactive effect. Same source the
    /// mixer's meters and the headset's VU mode read.
    pub fn set_levels(&self, levels: Arc<LevelStore>) {
        if let Ok(mut slot) = self.levels.lock() {
            *slot = Some(levels);
        }
    }

    /// Edit the live configuration and wake the worker, without touching disk.
    fn edit_config<F>(&self, edit: F) -> Result<KeyboardConfig, String>
    where
        F: FnOnce(&mut KeyboardConfig),
    {
        let updated = {
            let mut guard = self.config.lock().map_err(|_| "keyboard config poisoned")?;
            edit(&mut guard);
            guard.clamp_ranges();
            guard.clone()
        };
        self.dirty.store(true, Ordering::Relaxed);
        Ok(updated)
    }

    /// Replace the configuration, persist it, and wake the worker.
    pub fn update_config<F>(&self, edit: F) -> Result<(), String>
    where
        F: FnOnce(&mut KeyboardConfig),
    {
        let updated = self.edit_config(edit)?;
        keyboard::save(&updated).map_err(|e| e.to_string())?;
        self.emit_status();
        Ok(())
    }

    /// The connected model, if any.
    fn model(&self) -> Option<&'static Model> {
        let pid = self.dev.lock().ok()?.as_ref()?.product_id;
        protocol::model(pid)
    }

    /// Send one already-built control (output) report.
    pub fn send_control(&self, packet: &[u8]) -> Result<(), String> {
        let path = self
            .dev
            .lock()
            .map_err(|_| "keyboard lock poisoned")?
            .clone()
            .ok_or("no SteelSeries keyboard connected")?;
        let mut dev = HidDevice::open(&path).map_err(|e| format!("open keyboard: {e}"))?;
        dev.write_command(packet)
            .map_err(|e| format!("write keyboard: {e}"))
    }

    /// Send one already-built feature report, retrying with the other envelope
    /// once. Used by the OLED transport test, which has to reach the device
    /// outside the draw loop.
    pub fn send_feature(&self, packet: &[u8]) -> Result<(), String> {
        let path = self
            .dev
            .lock()
            .map_err(|_| "keyboard lock poisoned")?
            .clone()
            .ok_or("no SteelSeries keyboard connected")?;
        let mut dev = HidDevice::open(&path).map_err(|e| format!("open keyboard: {e}"))?;
        self.feature(&mut dev, packet)
            .map_err(|e| format!("write keyboard: {e}"))
    }

    /// Build a feature report from the connected model and send it.
    ///
    /// The per-key tables and the firmware config all need the model to size
    /// their report, so the caller gets it handed to them rather than having
    /// to ask for it and handle the "nothing connected" case itself.
    pub fn send_feature_with_model<F>(&self, build: F) -> Result<(), String>
    where
        F: FnOnce(&Model) -> Vec<u8>,
    {
        let model = self.model().ok_or("no SteelSeries keyboard connected")?;
        self.send_feature(&build(model))
    }

    /// Write a command and read the answer that belongs to it.
    ///
    /// This hardware answers one command behind: a read straight after a write
    /// returns the previous command's reply. Draining first is not enough
    /// either — a live probe found 24 stale replies queued, and a read after
    /// `0xBC` returned the firmware string left over from an earlier `0x90`.
    /// So the caller says what a valid answer looks like and everything else
    /// is discarded.
    ///
    /// `accept` differs per command because the device is not consistent:
    /// most replies echo the command in byte 0, but the firmware query answers
    /// with bare ASCII and no echo at all. Insisting on the echo everywhere
    /// silently turned the firmware into "unknown".
    fn query(
        &self,
        dev: &mut HidDevice,
        packet: &[u8],
        accept: impl Fn(&[u8]) -> bool,
    ) -> Option<Vec<u8>> {
        let mut reader = dev.open_reader().ok()?;
        // The reader hands back whole input reports; this device's are 64 B.
        let mut buf = [0u8; crate::headset::protocol::COMMAND_LEN];
        // Clear whatever the last conversation left behind.
        while let Ok(Some(_)) = reader.read_report(&mut buf, Duration::from_millis(5)) {}
        dev.write_command(packet).ok()?;
        let deadline = Instant::now() + QUERY_TIMEOUT;
        while Instant::now() < deadline {
            match reader.read_report(&mut buf, Duration::from_millis(60)) {
                Ok(Some(n)) if n > 0 && accept(&buf[..n]) => return Some(buf[..n].to_vec()),
                // Somebody else's answer, or an unsolicited frame: keep looking.
                Ok(Some(_)) | Ok(None) => continue,
                Err(_) => return None,
            }
        }
        debug!(
            "keyboard did not answer command {:#04x}",
            packet.get(1).copied().unwrap_or(0)
        );
        None
    }

    /// Accept a reply that echoes `cmd` in its first byte — the common case.
    fn echoes(cmd: u8) -> impl Fn(&[u8]) -> bool {
        move |reply: &[u8]| reply.first() == Some(&cmd)
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

    /// Discover the keyboard, then drive it until it goes away.
    fn supervise(self: Arc<Self>) {
        loop {
            let Some(found) = device::scan(DeviceClass::Keyboard) else {
                if self.is_connected() {
                    self.disconnect();
                }
                std::thread::sleep(RESCAN_INTERVAL);
                continue;
            };
            let Some(model) = protocol::model(found.product_id) else {
                std::thread::sleep(RESCAN_INTERVAL);
                continue;
            };
            if let Ok(mut slot) = self.dev.lock() {
                *slot = Some(found.clone());
            }
            match HidDevice::open(&found) {
                Ok(mut dev) => {
                    let model = self.on_connect(&mut dev, *model);
                    self.run(&mut dev, model);
                }
                Err(e) => warn!("keyboard {} could not be opened: {e}", model.name),
            }
            self.disconnect();
            std::thread::sleep(RESCAN_INTERVAL);
        }
    }

    fn disconnect(&self) {
        if let Ok(mut slot) = self.dev.lock() {
            *slot = None;
        }
        if let Ok(mut status) = self.status.lock() {
            *status = KeyboardStatus::default();
        }
        self.emit_presence(false);
    }

    /// Identify the board, correct its protocol generation from the firmware
    /// version, and take control of its LEDs. Returns the model to drive.
    fn on_connect(&self, dev: &mut HidDevice, model: Model) -> Model {
        // The firmware answers with bare ASCII, so "is this a version string"
        // is the only thing that identifies its reply.
        let firmware = self
            .query(dev, &protocol::firmware_query(), |reply| {
                protocol::parse_firmware(reply).is_some()
            })
            .and_then(|reply| protocol::parse_firmware(&reply));
        // A Gen 1/2 board on 1.19.7 or newer speaks the Gen 3 dialect, and only
        // the firmware string says so — the product id does not change with the
        // update. Getting this wrong means sending the wrong direct-lighting
        // opcode, which the board simply ignores.
        let model = match &firmware {
            Some(version) if protocol::firmware_is_gen3(version) && model.gen < Gen::Gen3 => {
                info!("keyboard firmware {version} speaks the Gen 3 dialect");
                model.at_gen(Gen::Gen3)
            }
            _ => model,
        };
        let region = self
            .query(dev, &protocol::region_query(), Self::echoes(0xf5))
            .and_then(|reply| reply.get(2).copied());
        let battery = model
            .battery
            .then(|| {
                self.query(dev, &protocol::battery_query(), Self::echoes(0x92))
                    .and_then(|reply| protocol::parse_battery(&reply))
            })
            .flatten();
        let (battery_percent, charging) = match battery {
            Some((percent, charging)) => (Some(percent), charging),
            None => (None, false),
        };
        let lighting = self
            .query(dev, &firmware::read_lighting_query(), Self::echoes(0xa0))
            .and_then(|reply| firmware::parse_lighting(&reply));
        let power = self
            .query(dev, &firmware::read_power_query(), Self::echoes(0xa9))
            .and_then(|reply| firmware::parse_power_saving(&reply));
        info!(
            "keyboard: {} ({:#06x}) on interface {}, firmware {}, region {}",
            model.name,
            model.product_id,
            model.interface,
            firmware.as_deref().unwrap_or("unknown"),
            region.map(|r| r.to_string()).unwrap_or_else(|| "?".into())
        );
        if let Ok(mut status) = self.status.lock() {
            *status = KeyboardStatus {
                present: true,
                model: Some(model.name.to_string()),
                product_id: model.product_id,
                form: Some(model.form),
                generation: Some(model.gen),
                wireless: model.wireless,
                firmware,
                battery_percent,
                charging,
                region,
                has_oled: model.oled.is_some(),
                has_actuation: model.actuation,
                oled_wire: self.wire_for(&model),
                lighting,
                power,
            };
        }
        // Gen 3 documents an init before direct lighting is accepted. The
        // verified board took direct frames either way, so a refusal here is
        // logged and otherwise ignored.
        for packet in protocol::init(&model) {
            if let Err(e) = self.feature(dev, &packet) {
                debug!("keyboard init refused: {e}");
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        self.emit_presence(true);
        self.emit_status();
        model
    }

    /// Send a feature report, falling back to the other envelope once.
    fn feature(&self, dev: &mut HidDevice, packet: &[u8]) -> io::Result<()> {
        if self.alt_envelope.load(Ordering::Relaxed) {
            return dev.send_feature(&protocol::alt_envelope(packet));
        }
        match dev.send_feature(packet) {
            Ok(()) => Ok(()),
            Err(first) => {
                // A stall here means the envelope is wrong for this model, not
                // that the device is gone — try the other shape before giving
                // up, and remember the answer.
                match dev.send_feature(&protocol::alt_envelope(packet)) {
                    Ok(()) => {
                        info!("keyboard takes the report-id-prefixed envelope");
                        self.alt_envelope.store(true, Ordering::Relaxed);
                        Ok(())
                    }
                    Err(_) => Err(first),
                }
            }
        }
    }

    /// The OLED transport currently in use: the user's pin, else the model's
    /// default, adjusted by whatever probing settled on.
    fn wire_for(&self, model: &Model) -> Option<OledWire> {
        let preferred = self.config().oled_wire.or(model.oled)?;
        let choice = self.oled_choice.load(Ordering::Relaxed) as usize;
        oled::candidates(preferred).get(choice).copied()
    }

    /// The draw loop. Returns when the device stops accepting writes.
    fn run(&self, dev: &mut HidDevice, model: Model) {
        let mut screens = Screens::new();
        let mut last_frame: Option<Vec<[u8; 3]>> = None;
        let mut last_oled: Option<Framebuffer> = None;
        let mut last_onboard: Option<EffectKind> = None;
        let mut battery_at = Instant::now();
        let mut oled_at = Instant::now();
        let mut awake_since = Instant::now();
        let started = Instant::now();
        let mut failures = 0u32;

        loop {
            let config = self.config();
            let dirty = self.dirty.swap(false, Ordering::Relaxed);
            if dirty {
                // A changed effect has to repaint even if it renders the same
                // pixels, and an onboard effect has to be re-sent.
                last_frame = None;
                last_oled = None;
                last_onboard = None;
                awake_since = Instant::now();
            }

            if !config.enabled {
                if last_onboard != Some(EffectKind::Off) {
                    let _ = dev.write_command(&protocol::release_to_onboard(&model));
                    last_onboard = Some(EffectKind::Off);
                }
                std::thread::sleep(TICK);
                if self.gone(dev) {
                    return;
                }
                continue;
            }

            let elapsed = started.elapsed().as_secs_f32();
            let mut wrote = false;

            // --- lighting ---
            if config.lighting.kind.onboard() {
                if last_onboard != Some(config.lighting.kind) {
                    for packet in self.onboard_packets(&config, &model) {
                        let _ = dev.write_command(&packet);
                    }
                    last_onboard = Some(config.lighting.kind);
                    wrote = true;
                }
            } else if config.lighting.kind.animated() || last_frame.is_none() {
                // A still effect is rendered once and then left alone; only the
                // animated ones need a frame per tick.
                let level = self.audio_level();
                let frame = effects::render(
                    &config.lighting,
                    model.form,
                    config.physical,
                    elapsed,
                    level,
                );
                if last_frame.as_ref() != Some(&frame) {
                    match self.feature(dev, &protocol::direct_packet(&model, &frame)) {
                        Ok(()) => {
                            last_frame = Some(frame);
                            failures = 0;
                        }
                        Err(e) => {
                            failures += 1;
                            debug!("keyboard lighting write failed: {e}");
                        }
                    }
                    wrote = true;
                }
            }

            // --- OLED ---
            if let (Some(mode), Some(_)) = (config.oled_mode, model.oled) {
                screens.text = config.oled_text.clone();
                screens.status = self.status_lines(&config);
                // Blank rather than burn in: the keyboard's own screensaver
                // only guards its own screen, and it cannot know that Inari is
                // driving the panel. Any change to the settings counts as
                // activity and brings the picture back.
                let asleep = config.oled_screensaver_minutes > 0
                    && awake_since.elapsed().as_secs()
                        >= config.oled_screensaver_minutes as u64 * 60;
                let fb = if asleep {
                    oled::framebuffer()
                } else {
                    screens.render(mode, elapsed)
                };
                // The firmware repaints its own screen on its own events, so a
                // frame that has not changed still gets re-sent now and then.
                let stale = oled_at.elapsed() >= OLED_REFRESH;
                if stale || last_oled.as_ref() != Some(&fb) {
                    oled_at = Instant::now();
                    if self.push_oled(dev, &model, &fb) {
                        last_oled = Some(fb);
                    } else {
                        failures += 1;
                    }
                    wrote = true;
                }
            }

            if failures >= MAX_FAILURES {
                warn!("keyboard stopped accepting writes; dropping the connection");
                return;
            }
            if !wrote && self.gone(dev) {
                return;
            }

            // --- battery ---
            if model.battery && battery_at.elapsed() >= BATTERY_INTERVAL {
                battery_at = Instant::now();
                if let Some((percent, charging)) = self
                    .query(dev, &protocol::battery_query(), Self::echoes(0x92))
                    .and_then(|reply| protocol::parse_battery(&reply))
                {
                    if let Ok(mut status) = self.status.lock() {
                        status.battery_percent = Some(percent);
                        status.charging = charging;
                    }
                    self.emit_status();
                }
            }

            std::thread::sleep(TICK);
        }
    }

    /// Push one OLED frame, walking the transport candidates until one is
    /// accepted. Returns false when none was.
    fn push_oled(&self, dev: &mut HidDevice, model: &Model, fb: &Framebuffer) -> bool {
        let Some(preferred) = self.config().oled_wire.or(model.oled) else {
            return false;
        };
        let candidates = oled::candidates(preferred);
        let start = self.oled_choice.load(Ordering::Relaxed) as usize;
        for offset in 0..candidates.len() {
            let index = (start + offset) % candidates.len();
            let wire = candidates[index];
            let packets = oled::frame_packets(fb, wire, model.feature_len);
            if packets
                .iter()
                .all(|packet| self.feature(dev, packet).is_ok())
            {
                if index != start {
                    info!("keyboard OLED accepted transport {wire:?}");
                    self.oled_choice.store(index as u8, Ordering::Relaxed);
                    if let Ok(mut status) = self.status.lock() {
                        status.oled_wire = Some(wire);
                    }
                    self.emit_status();
                }
                return true;
            }
        }
        false
    }

    /// Packets for the effects the firmware renders itself.
    fn onboard_packets(&self, config: &KeyboardConfig, model: &Model) -> Vec<Vec<u8>> {
        let light = &config.lighting;
        match light.kind {
            EffectKind::Off => vec![protocol::release_to_onboard(model)],
            EffectKind::Reactive => vec![
                protocol::brightness(light.brightness),
                protocol::zone_color(protocol::ZONE_ALL, light.color),
                protocol::reactive(true),
                protocol::apply(),
            ],
            EffectKind::ColorShift => vec![
                protocol::brightness(light.brightness),
                protocol::color_shift(light.color, light.color2, light.speed),
                protocol::apply(),
            ],
            _ => Vec::new(),
        }
    }

    fn status_lines(&self, config: &KeyboardConfig) -> StatusLines {
        StatusLines {
            // A scene prints its own name; "Scene" on the panel would tell the
            // user nothing they did not already know.
            effect: match config.lighting.kind {
                EffectKind::Scene => config.lighting.scene.label().to_string(),
                other => format!("{other:?}"),
            },
            brightness: config.lighting.brightness,
            actuation: config.actuation,
        }
    }

    /// Loudest thing currently playing, 0..1, for the audio-reactive effect.
    fn audio_level(&self) -> f32 {
        let Ok(guard) = self.levels.lock() else {
            return 0.0;
        };
        let Some(levels) = guard.as_ref() else {
            return 0.0;
        };
        levels
            .names()
            .into_iter()
            .map(|(_, slot)| levels.drain(slot, 0).max(levels.drain(slot, 1)))
            .fold(0.0f32, f32::max)
            .clamp(0.0, 1.0)
    }

    /// Cheap liveness check for ticks that wrote nothing: without it an idle
    /// static colour would never notice the keyboard being unplugged.
    fn gone(&self, dev: &mut HidDevice) -> bool {
        dev.write_command(&protocol::firmware_query()).is_err()
    }

    /// Hand the LEDs back on the way out, so a closed Inari does not leave the
    /// board frozen on its last frame.
    pub fn release(&self) {
        let Some(model) = self.model() else { return };
        let _ = self.send_control(&protocol::release_to_onboard(model));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_manager_is_disconnected_but_configured() {
        let manager = KeyboardManager::default();
        assert!(!manager.is_connected());
        assert!(!manager.status().present);
        assert!(manager.config().enabled);
        // Nothing to talk to yet, so commands fail cleanly instead of panicking.
        assert!(manager.send_control(&protocol::apply()).is_err());
        manager.release();
    }

    #[test]
    fn onboard_effects_carry_their_firmware_setup_and_the_rest_carry_none() {
        let manager = KeyboardManager::default();
        let model = protocol::model(protocol::PID_APEX_PRO_GEN3).unwrap();
        let mut config = KeyboardConfig::default();

        config.lighting.kind = EffectKind::Reactive;
        let packets = manager.onboard_packets(&config, model);
        assert_eq!(packets.len(), 4, "brightness, colour, reactive, apply");
        assert_eq!(packets[2][1], 0x25);

        config.lighting.kind = EffectKind::Off;
        assert_eq!(manager.onboard_packets(&config, model)[0][1], 0x41);

        // Host-rendered effects have no firmware equivalent to send.
        config.lighting.kind = EffectKind::Wave;
        assert!(manager.onboard_packets(&config, model).is_empty());
    }

    #[test]
    fn the_audio_level_is_zero_without_a_level_store() {
        assert_eq!(KeyboardManager::default().audio_level(), 0.0);
    }

    #[test]
    fn editing_the_config_marks_the_worker_dirty() {
        let manager = KeyboardManager::default();
        manager.dirty.store(false, Ordering::Relaxed);
        manager
            .edit_config(|c| c.lighting.brightness = 42)
            .expect("editing the live config cannot fail");
        assert!(manager.dirty.load(Ordering::Relaxed));
        assert_eq!(manager.config().lighting.brightness, 42);
    }

    #[test]
    fn status_lines_describe_what_the_panel_should_print() {
        let manager = KeyboardManager::default();
        let mut config = KeyboardConfig {
            actuation: Some(15),
            ..KeyboardConfig::default()
        };
        config.lighting.kind = EffectKind::Wave;
        config.lighting.brightness = 70;
        let lines = manager.status_lines(&config);
        assert_eq!(lines.effect, "Wave");
        assert_eq!(lines.brightness, 70);
        assert_eq!(lines.actuation, Some(15));
    }

    #[test]
    fn the_oled_transport_follows_the_users_pin_over_the_models_default() {
        let manager = KeyboardManager::default();
        let model = protocol::model(protocol::PID_APEX_PRO_GEN3).unwrap();
        assert_eq!(
            manager.wire_for(model),
            Some(OledWire::Single {
                cmd: 0x61,
                page: true
            })
        );
        let pinned = OledWire::Chunked {
            cmd: 0x4c,
            page: false,
        };
        manager.edit_config(|c| c.oled_wire = Some(pinned)).unwrap();
        assert_eq!(manager.wire_for(model), Some(pinned));
    }

    #[test]
    fn a_keyboard_without_a_panel_has_no_transport() {
        let manager = KeyboardManager::default();
        let apex9 = protocol::model(protocol::PID_APEX_9_TKL).unwrap();
        assert_eq!(manager.wire_for(apex9), None);
    }

    #[test]
    fn every_oled_mode_renders_at_the_panels_size() {
        let mut screens = Screens::new();
        for mode in super::super::oled_modes::ALL_MODES {
            let fb = screens.render(mode, 1.0);
            assert_eq!(fb.height(), oled::HEIGHT, "{mode:?} is the wrong height");
        }
    }
}
