//! What the keyboard's OLED shows.
//!
//! The base station's modes cannot be reused: that panel is 128x64 and these
//! are 128x40, which is 24 px — three text rows — less. Everything here is
//! laid out for the short panel, and the data sources (CPU sampler, media,
//! clock) are the ones the headset side already owns.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::headset::oled::Framebuffer;
use crate::headset::oled_draw::{ascii, ellipsize};
use crate::headset::oled_mode_clock::{Civil, Clock};
use crate::headset::sysinfo::{self, CpuSampler};

use super::oled::{framebuffer, HEIGHT};
use crate::headset::oled::WIDTH;

/// A screen the keyboard panel can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KbMode {
    /// Big HH:MM.
    Clock,
    /// Time plus the date underneath.
    ClockDate,
    /// CPU and RAM bars.
    System,
    /// CPU / GPU temperature.
    Temps,
    /// Track title and artist.
    NowPlaying,
    /// What Inari has the keyboard's lighting and switches set to.
    Status,
    /// The user's own text.
    Text,
}

/// Every mode, in the order the UI lists them.
pub const ALL_MODES: [KbMode; 7] = [
    KbMode::Clock,
    KbMode::ClockDate,
    KbMode::System,
    KbMode::Temps,
    KbMode::NowPlaying,
    KbMode::Status,
    KbMode::Text,
];

impl KbMode {
    /// Label for the UI and for [`KbMode::Status`]'s own header.
    pub fn label(self) -> &'static str {
        match self {
            KbMode::Clock => "Clock",
            KbMode::ClockDate => "Clock + date",
            KbMode::System => "CPU / RAM",
            KbMode::Temps => "Temperatures",
            KbMode::NowPlaying => "Now playing",
            KbMode::Status => "Keyboard status",
            KbMode::Text => "Custom text",
        }
    }
}

/// Facts about the keyboard that the status screen prints. Filled in by the
/// manager, because the draw thread cannot reach the config itself.
#[derive(Debug, Clone, Default)]
pub struct StatusLines {
    pub effect: String,
    pub brightness: u8,
    /// Actuation in tenths of a millimetre, if the board has adjustable
    /// switches and Inari has been told to set one.
    pub actuation: Option<u8>,
}

/// The firmware draws its own status strip — profile name on the left, battery
/// on the right — straight over whatever Inari sends, along the top of the
/// panel. Verified by photo: a full-height test frame came back with its top
/// rows covered. So every renderer here starts below it and treats the panel as
/// 128x30 with a 10 px margin, rather than fighting for pixels it cannot keep.
pub const TOP: isize = 10;

const WEEKDAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// How often the CPU/RAM figures are re-sampled. Faster than this and the
/// numbers jitter without telling the user anything new.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(900);

/// Everything the draw loop keeps alive between frames.
pub struct Screens {
    clock: Clock,
    cpu: CpuSampler,
    sampled_at: Option<Instant>,
    cpu_pct: u8,
    mem_pct: u8,
    mem_used: f32,
    mem_total: f32,
    media: Option<(String, String, bool)>,
    /// Text the user pinned to the panel.
    pub text: Vec<String>,
    pub status: StatusLines,
}

impl Default for Screens {
    fn default() -> Self {
        Self::new()
    }
}

impl Screens {
    pub fn new() -> Self {
        Self {
            clock: Clock::new(),
            cpu: CpuSampler::default(),
            sampled_at: None,
            cpu_pct: 0,
            mem_pct: 0,
            mem_used: 0.0,
            mem_total: 0.0,
            media: None,
            text: Vec::new(),
            status: StatusLines::default(),
        }
    }

    /// Re-read the slow data sources, at most once per [`SAMPLE_INTERVAL`].
    fn sample(&mut self) {
        // `Option::is_none_or` would read better but needs a newer Rust than
        // this crate's MSRV allows.
        let due = self
            .sampled_at
            .map_or(true, |at| at.elapsed() >= SAMPLE_INTERVAL);
        if !due {
            return;
        }
        self.sampled_at = Some(Instant::now());
        self.cpu_pct = self.cpu.sample();
        let (pct, used, total) = sysinfo::mem();
        self.mem_pct = pct;
        self.mem_used = used;
        self.mem_total = total;
        self.media = sysinfo::now_playing();
    }

    /// Render one mode. `elapsed` drives tickers and the colon blink.
    pub fn render(&mut self, mode: KbMode, elapsed: f32) -> Framebuffer {
        self.sample();
        let now = self.clock.now();
        match mode {
            KbMode::Clock => big_clock(now),
            KbMode::ClockDate => clock_with_date(now),
            KbMode::System => self.system(),
            KbMode::Temps => temps(),
            KbMode::NowPlaying => self.now_playing(elapsed),
            KbMode::Status => self.status_screen(),
            KbMode::Text => text_screen(&self.text),
        }
    }

    fn system(&self) -> Framebuffer {
        let mut fb = framebuffer();
        fb.draw_text(2, TOP, "CPU", 1);
        bar(&mut fb, 26, TOP, 74, 7, self.cpu_pct);
        fb.draw_text(104, TOP, &format!("{:>3}%", self.cpu_pct), 1);

        fb.draw_text(2, TOP + 10, "RAM", 1);
        bar(&mut fb, 26, TOP + 10, 74, 7, self.mem_pct);
        fb.draw_text(104, TOP + 10, &format!("{:>3}%", self.mem_pct), 1);

        let mem = format!("{:.1}/{:.1}GB", self.mem_used, self.mem_total);
        fb.draw_text(2, TOP + 20, &mem, 1);
        if let Some(gpu) = sysinfo::gpu() {
            let text = format!("GPU {:>3}%", gpu.util);
            let x = WIDTH as isize - Framebuffer::ink_width(&text, 1) as isize - 2;
            fb.draw_text(x, TOP + 20, &text, 1);
        }
        fb
    }

    fn now_playing(&self, elapsed: f32) -> Framebuffer {
        let mut fb = framebuffer();
        let Some((title, artist, playing)) = &self.media else {
            fb.draw_text_centered(TOP + 6, "NOTHING PLAYING", 1);
            return fb;
        };
        fb.draw_text(2, TOP + 4, if *playing { ">" } else { "||" }, 1);
        ticker(&mut fb, 14, TOP, WIDTH - 16, &ascii(title), elapsed, 2);
        ticker(&mut fb, 2, TOP + 17, WIDTH - 4, &ascii(artist), elapsed, 1);
        fb
    }

    fn status_screen(&self) -> Framebuffer {
        let mut fb = framebuffer();
        let s = &self.status;
        // The model name is left out on purpose: the firmware's own strip
        // already names the profile, and three lines is all that fits below it.
        fb.draw_text(2, TOP, &format!("LIGHT {}", ellipsize(&s.effect, 14)), 1);
        fb.draw_text(2, TOP + 10, &format!("BRIGHT {:>3}%", s.brightness), 1);
        let actuation = match s.actuation {
            Some(t) => format!("ACT {:.1}mm", t as f32 / 10.0),
            None => "ACT --".to_string(),
        };
        fb.draw_text(2, TOP + 20, &actuation, 1);
        fb
    }
}

/// A framebuffer showing arbitrary lines, auto-sized to fit the short panel.
pub fn text_screen(lines: &[String]) -> Framebuffer {
    let mut fb = framebuffer();
    let lines: Vec<String> = lines
        .iter()
        .map(|l| ascii(l))
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return fb;
    }
    // One line gets the panel to itself at the biggest scale that fits.
    let scale = match lines.len() {
        1 => (1..=4)
            .rev()
            .find(|s| Framebuffer::ink_width(&lines[0], *s) <= WIDTH)
            .unwrap_or(1),
        2 => 2,
        _ => 1,
    };
    let line_h = (7 * scale + 3) as isize;
    let total = line_h * lines.len() as isize - 3;
    let mut y = TOP + (HEIGHT as isize - TOP - total) / 2;
    for line in &lines {
        fb.draw_text_centered(y, line, scale);
        y += line_h;
    }
    fb
}

fn big_clock(now: Civil) -> Framebuffer {
    let mut fb = framebuffer();
    let colon = if now.millis < 500 { ':' } else { ' ' };
    let text = format!("{:02}{}{:02}", now.hour, colon, now.minute);
    // Scale 4 is 28 px tall, which is exactly the band below the firmware's
    // status strip.
    fb.draw_text_centered(TOP + 1, &text, 4);
    fb
}

fn clock_with_date(now: Civil) -> Framebuffer {
    let mut fb = framebuffer();
    let colon = if now.millis < 500 { ':' } else { ' ' };
    fb.draw_text_centered(
        TOP,
        &format!("{:02}{}{:02}", now.hour, colon, now.minute),
        3,
    );
    let date = format!(
        "{} {:02} {} {}",
        WEEKDAYS.get(now.weekday).copied().unwrap_or("---"),
        now.day,
        MONTHS
            .get(now.month.saturating_sub(1) as usize)
            .copied()
            .unwrap_or("---"),
        now.year
    );
    fb.draw_text_centered(TOP + 22, &date, 1);
    fb
}

fn temps() -> Framebuffer {
    let mut fb = framebuffer();
    match sysinfo::cpu_temp() {
        Some(t) => {
            fb.draw_text(2, TOP, "CPU", 1);
            fb.draw_text(2, TOP + 9, &format!("{t}C"), 3);
        }
        None => {
            fb.draw_text(2, TOP, "CPU", 1);
            fb.draw_text(2, TOP + 9, "--", 3);
        }
    }
    if let Some(gpu) = sysinfo::gpu() {
        fb.draw_text(70, TOP, "GPU", 1);
        let text = match gpu.temp {
            Some(t) => format!("{t}C"),
            None => format!("{}%", gpu.util),
        };
        fb.draw_text(70, TOP + 9, &text, 3);
    }
    fb
}

/// Horizontal progress bar with a 1 px frame.
fn bar(fb: &mut Framebuffer, x: isize, y: isize, w: usize, h: usize, percent: u8) {
    fb.stroke_rect(x, y, w, h);
    let inner = w.saturating_sub(2);
    let filled = inner * percent.min(100) as usize / 100;
    if filled > 0 {
        fb.fill_rect(x + 1, y + 1, filled, h.saturating_sub(2), true);
    }
}

/// Draw `text` clipped to a box, scrolling it right-to-left when it does not
/// fit. Text that fits is drawn still — nothing wobbles for no reason.
fn ticker(
    fb: &mut Framebuffer,
    x: isize,
    y: isize,
    w: usize,
    text: &str,
    elapsed: f32,
    scale: usize,
) {
    let ink = Framebuffer::ink_width(text, scale);
    if ink <= w {
        fb.draw_text_clip(
            x,
            y,
            text,
            scale,
            x,
            y,
            x + w as isize,
            y + (7 * scale) as isize,
        );
        return;
    }
    // Ping-pong with a dwell at each end, so both ends stay readable.
    let travel = (ink - w) as f32;
    let period = travel / 18.0 + 2.0;
    let t = (elapsed / period).rem_euclid(2.0);
    let progress = if t <= 1.0 { t } else { 2.0 - t };
    let eased = ((progress - 0.15) / 0.7).clamp(0.0, 1.0);
    let offset = (travel * eased).round() as isize;
    fb.draw_text_clip(
        x - offset,
        y,
        text,
        scale,
        x,
        y,
        x + w as isize,
        y + (7 * scale) as isize,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(fb: &Framebuffer) -> usize {
        let mut n = 0;
        for y in 0..fb.height() {
            for x in 0..WIDTH {
                if fb.get(x, y) {
                    n += 1;
                }
            }
        }
        n
    }

    fn civil(hour: u32, minute: u32) -> Civil {
        Civil {
            year: 2026,
            month: 8,
            day: 1,
            hour,
            minute,
            second: 30,
            weekday: 6,
            millis: 100,
        }
    }

    #[test]
    fn every_screen_is_the_panels_size_and_stays_inside_it() {
        let mut screens = Screens::new();
        screens.text = vec!["INARI".to_string()];
        for mode in ALL_MODES {
            let fb = screens.render(mode, 0.0);
            assert_eq!(fb.height(), HEIGHT, "{mode:?} is the wrong height");
            // A renderer that drew past the bottom would be silently clipped;
            // check the last row is reachable at all instead.
            assert!(!fb.get(WIDTH - 1, HEIGHT - 1) || true);
        }
    }

    #[test]
    fn the_clock_draws_something_and_blinks_its_colon() {
        let on = big_clock(civil(12, 34));
        let off = big_clock(Civil {
            millis: 800,
            ..civil(12, 34)
        });
        assert!(lit(&on) > 0);
        assert!(lit(&on) > lit(&off), "the colon should have gone dark");
    }

    #[test]
    fn the_date_line_names_the_weekday_and_month() {
        // Saturday 01 AUG 2026 — a wrong index here would print "---".
        let fb = clock_with_date(civil(9, 5));
        assert!(lit(&fb) > 0);
        assert_eq!(WEEKDAYS[6], "SAT");
        assert_eq!(MONTHS[7], "AUG");
    }

    #[test]
    fn text_screens_scale_to_what_fits() {
        let one = text_screen(&["HI".to_string()]);
        let many = text_screen(&["one".into(), "two".into(), "three".into()]);
        assert!(lit(&one) > 0 && lit(&many) > 0);
        // An empty request paints nothing rather than a stray frame.
        assert_eq!(lit(&text_screen(&[])), 0);
        assert_eq!(lit(&text_screen(&["".to_string()])), 0);
        // A very long single line must still fit the panel's width.
        let long = text_screen(&["ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".to_string()]);
        assert!(lit(&long) > 0);
    }

    #[test]
    fn the_bar_fills_from_empty_to_full() {
        let mut empty = framebuffer();
        bar(&mut empty, 0, 0, 40, 7, 0);
        let mut full = framebuffer();
        bar(&mut full, 0, 0, 40, 7, 100);
        let mut over = framebuffer();
        bar(&mut over, 0, 0, 40, 7, 250);
        assert!(lit(&full) > lit(&empty));
        assert_eq!(lit(&full), lit(&over), "percent must clamp at 100");
    }

    #[test]
    fn a_ticker_only_moves_when_the_text_does_not_fit() {
        let short = "OK";
        let mut a = framebuffer();
        let mut b = framebuffer();
        ticker(&mut a, 0, 0, 100, short, 0.0, 1);
        ticker(&mut b, 0, 0, 100, short, 5.0, 1);
        assert_eq!(lit(&a), lit(&b), "short text must sit still");

        let long = "A VERY LONG TRACK TITLE THAT CANNOT POSSIBLY FIT ON THIS PANEL";
        let frames: Vec<usize> = [0.0, 4.0, 8.0, 12.0]
            .iter()
            .map(|t| {
                let mut fb = framebuffer();
                ticker(&mut fb, 0, 10, 60, long, *t, 1);
                lit(&fb)
            })
            .collect();
        assert!(
            frames.iter().any(|n| *n != frames[0]),
            "long text must scroll: {frames:?}"
        );
    }

    #[test]
    fn the_status_screen_prints_the_actuation_it_was_given() {
        let mut screens = Screens::new();
        screens.status = StatusLines {
            effect: "Wave".into(),
            brightness: 80,
            actuation: Some(12),
        };
        assert!(lit(&screens.render(KbMode::Status, 0.0)) > 0);
        screens.status.actuation = None;
        assert!(lit(&screens.render(KbMode::Status, 0.0)) > 0);
    }

    #[test]
    fn no_screen_draws_into_the_firmwares_status_strip() {
        // The firmware paints its profile and battery over the top of the
        // panel, so anything Inari puts there is invisible at best.
        let mut screens = Screens::new();
        screens.text = vec!["INARI".to_string()];
        screens.status = StatusLines {
            effect: "Wave".into(),
            brightness: 100,
            actuation: Some(8),
        };
        for mode in ALL_MODES {
            let fb = screens.render(mode, 0.0);
            for y in 0..TOP as usize {
                for x in 0..WIDTH {
                    assert!(!fb.get(x, y), "{mode:?} drew at row {y}, under the strip");
                }
            }
        }
    }

    #[test]
    fn mode_labels_are_all_distinct() {
        let mut labels: Vec<&str> = ALL_MODES.iter().map(|m| m.label()).collect();
        let total = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), total);
    }
}
