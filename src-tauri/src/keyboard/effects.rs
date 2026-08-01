//! Host-side per-key lighting effects.
//!
//! The keyboard's own firmware can only do what SteelSeries built into it
//! (solid zones, reactive, colour shift). Direct mode hands Inari every key on
//! every frame instead, so the animations live here — the same place the audio
//! level and the key positions are already available.
//!
//! Everything in this module is pure: [`render`] maps (effect, phase) to 112
//! colours in wire order, and the manager decides when to push them.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::keys::Physical;
use super::protocol::{FormFactor, KEY_ORDER};

/// One frame of colours, indexed like [`KEY_ORDER`].
pub type Frame = Vec<[u8; 3]>;

/// What the keyboard is currently doing with its LEDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    /// LEDs handed back to the keyboard's onboard profile.
    Off,
    /// One colour across the board.
    #[default]
    Static,
    /// Whatever the user painted key by key.
    PerKey,
    /// The whole board fades in and out.
    Breathing,
    /// A rainbow sweeping across the board.
    Wave,
    /// The whole board cycles through the spectrum together.
    Rainbow,
    /// Left-to-right blend between two colours.
    Gradient,
    /// Brightness follows what is playing.
    Audio,
    /// Firmware effect: keys flash when pressed. Survives Inari closing.
    Reactive,
    /// Firmware effect: fade between two colours.
    ColorShift,
}

impl EffectKind {
    /// Whether Inari has to keep pushing frames for this effect.
    pub fn animated(self) -> bool {
        matches!(
            self,
            EffectKind::Breathing | EffectKind::Wave | EffectKind::Rainbow | EffectKind::Audio
        )
    }

    /// Whether the keyboard renders this itself once it has been told to.
    pub fn onboard(self) -> bool {
        matches!(
            self,
            EffectKind::Off | EffectKind::Reactive | EffectKind::ColorShift
        )
    }
}

/// The user's lighting configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lighting {
    pub kind: EffectKind,
    /// Primary colour (static, breathing, gradient start, colour shift A).
    pub color: [u8; 3],
    /// Secondary colour (gradient end, colour shift B).
    pub color2: [u8; 3],
    /// 1–100, mapped to a per-effect rate.
    pub speed: u8,
    /// 0–100. Applied host-side in direct mode so it never fights the
    /// firmware's own brightness.
    pub brightness: u8,
    /// Sweep the other way.
    pub reverse: bool,
    /// Per-key colours, keyed by HID usage id. Only used by
    /// [`EffectKind::PerKey`], but kept across effect changes so switching
    /// away and back does not lose the user's work.
    #[serde(default)]
    pub per_key: HashMap<u8, [u8; 3]>,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            kind: EffectKind::Static,
            // Inari's own vermilion, so a fresh install looks deliberate.
            color: [255, 82, 0],
            color2: [0, 120, 255],
            speed: 50,
            brightness: 100,
            reverse: false,
            per_key: HashMap::new(),
        }
    }
}

/// HSV (all 0..1) to 8-bit RGB.
pub fn hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as u32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    ]
}

fn scale(color: [u8; 3], factor: f32) -> [u8; 3] {
    let f = factor.clamp(0.0, 1.0);
    [
        (color[0] as f32 * f).round() as u8,
        (color[1] as f32 * f).round() as u8,
        (color[2] as f32 * f).round() as u8,
    ]
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
    ]
}

/// How far an effect advances per second at a given speed setting. Slow enough
/// at 1 to be a slow drift, fast enough at 100 to be obvious, and linear in
/// between so the slider feels honest.
fn rate(speed: u8) -> f32 {
    0.05 + (speed.clamp(1, 100) as f32 / 100.0) * 0.95
}

/// Render one frame.
///
/// `elapsed` is seconds since the effect started, `level` the current audio
/// level (0..1) — ignored by every effect but [`EffectKind::Audio`].
pub fn render(
    lighting: &Lighting,
    form: FormFactor,
    physical: Physical,
    elapsed: f32,
    level: f32,
) -> Frame {
    let positions = super::keys::positions(form, physical);
    let brightness = lighting.brightness.min(100) as f32 / 100.0;
    let phase = elapsed * rate(lighting.speed);
    let mut frame = vec![[0u8; 3]; KEY_ORDER.len()];

    for (i, color) in frame.iter_mut().enumerate() {
        let (x, y) = positions[i];
        let x = if lighting.reverse { 1.0 - x } else { x };
        *color = match lighting.kind {
            // Off is painted black rather than skipped: the manager releases
            // the LEDs to the firmware instead of sending this frame at all,
            // and a black frame is the safe thing to fall back to.
            EffectKind::Off => [0, 0, 0],
            EffectKind::Static | EffectKind::Reactive | EffectKind::ColorShift => lighting.color,
            EffectKind::PerKey => *lighting.per_key.get(&KEY_ORDER[i]).unwrap_or(&[0, 0, 0]),
            EffectKind::Breathing => {
                // Never fully dark: a board that blinks out entirely reads as
                // broken rather than as an effect.
                let wave = (phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
                scale(lighting.color, 0.15 + 0.85 * wave)
            }
            EffectKind::Wave => hsv(phase - x * 0.75 - y * 0.15, 1.0, 1.0),
            EffectKind::Rainbow => hsv(phase, 1.0, 1.0),
            EffectKind::Gradient => mix(lighting.color, lighting.color2, x),
            EffectKind::Audio => {
                // Keys light up left to right as the level rises, with a soft
                // edge so the boundary doesn't crawl one key at a time.
                // The head runs slightly past 1.0 so the rightmost key still
                // reaches full brightness at peak level.
                let head = level.clamp(0.0, 1.0) * 1.1;
                let lit = ((head - x) * 6.0).clamp(0.0, 1.0);
                scale(lighting.color, 0.06 + 0.94 * lit)
            }
        };
        *color = scale(*color, brightness);
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::super::protocol::key_index;
    use super::*;

    fn lighting(kind: EffectKind) -> Lighting {
        Lighting {
            kind,
            ..Lighting::default()
        }
    }

    fn frame(l: &Lighting, t: f32, level: f32) -> Frame {
        render(l, FormFactor::Tkl, Physical::Iso, t, level)
    }

    #[test]
    fn a_frame_covers_every_wire_key() {
        assert_eq!(frame(&lighting(EffectKind::Static), 0.0, 0.0).len(), 112);
    }

    #[test]
    fn static_paints_one_colour_everywhere() {
        let f = frame(&lighting(EffectKind::Static), 3.7, 0.5);
        assert!(f.iter().all(|c| *c == [255, 82, 0]));
    }

    #[test]
    fn brightness_scales_the_whole_frame_and_zero_is_dark() {
        let mut l = lighting(EffectKind::Static);
        l.brightness = 50;
        assert_eq!(frame(&l, 0.0, 0.0)[0], [128, 41, 0]);
        l.brightness = 0;
        assert!(frame(&l, 0.0, 0.0).iter().all(|c| *c == [0, 0, 0]));
        // Out-of-range input clamps rather than wrapping to darkness.
        l.brightness = 200;
        assert_eq!(frame(&l, 0.0, 0.0)[0], [255, 82, 0]);
    }

    #[test]
    fn per_key_colours_land_on_the_key_that_was_painted() {
        let mut l = lighting(EffectKind::PerKey);
        l.per_key.insert(0x29, [10, 20, 30]); // Escape
        let f = frame(&l, 0.0, 0.0);
        assert_eq!(f[key_index(0x29).unwrap()], [10, 20, 30]);
        assert_eq!(f[key_index(0x04).unwrap()], [0, 0, 0], "A was not painted");
    }

    #[test]
    fn the_wave_differs_across_the_board_and_moves_with_time() {
        let l = lighting(EffectKind::Wave);
        let f = frame(&l, 0.0, 0.0);
        let left = f[key_index(0x29).unwrap()]; // Escape, far left
        let right = f[key_index(0x4e).unwrap()]; // Page Down, far right
        assert_ne!(left, right, "a wave that is one colour is not a wave");
        let later = frame(&l, 1.0, 0.0)[key_index(0x29).unwrap()];
        assert_ne!(left, later, "the wave must advance");
    }

    #[test]
    fn reverse_mirrors_the_sweep() {
        let mut l = lighting(EffectKind::Gradient);
        let normal = frame(&l, 0.0, 0.0);
        l.reverse = true;
        let mirrored = frame(&l, 0.0, 0.0);
        let esc = key_index(0x29).unwrap();
        assert_ne!(normal[esc], mirrored[esc]);
        // Escape reversed should look like the far right unreversed.
        assert!(mirrored[esc][2] > normal[esc][2], "gradient did not flip");
    }

    #[test]
    fn rainbow_is_uniform_but_never_still() {
        let l = lighting(EffectKind::Rainbow);
        let f = frame(&l, 0.5, 0.0);
        assert!(f.iter().all(|c| *c == f[0]));
        assert_ne!(f[0], frame(&l, 1.5, 0.0)[0]);
    }

    #[test]
    fn breathing_dims_but_never_goes_fully_dark() {
        let l = lighting(EffectKind::Breathing);
        let mut seen_dim = false;
        for step in 0..40 {
            let c = frame(&l, step as f32 * 0.1, 0.0)[0];
            assert!(c != [0, 0, 0], "breathing blacked the board out");
            if c[0] < 100 {
                seen_dim = true;
            }
        }
        assert!(seen_dim, "breathing never dimmed");
    }

    #[test]
    fn audio_lights_the_board_from_the_left_as_the_level_rises() {
        let l = lighting(EffectKind::Audio);
        let esc = key_index(0x29).unwrap();
        let pgdn = key_index(0x4e).unwrap();
        let quiet = frame(&l, 0.0, 0.0);
        assert!(quiet[esc][0] < 40, "silence should be near dark");
        let half = frame(&l, 0.0, 0.5);
        assert!(half[esc][0] > half[pgdn][0], "the left should lead");
        let loud = frame(&l, 0.0, 1.0);
        assert!(loud[pgdn][0] > half[pgdn][0], "the right should catch up");
    }

    #[test]
    fn hsv_hits_the_primaries_and_wraps() {
        assert_eq!(hsv(0.0, 1.0, 1.0), [255, 0, 0]);
        assert_eq!(hsv(1.0 / 3.0, 1.0, 1.0), [0, 255, 0]);
        assert_eq!(hsv(2.0 / 3.0, 1.0, 1.0), [0, 0, 255]);
        assert_eq!(hsv(1.0, 1.0, 1.0), hsv(0.0, 1.0, 1.0));
        assert_eq!(hsv(-0.5, 1.0, 1.0), hsv(0.5, 1.0, 1.0));
        assert_eq!(hsv(0.0, 0.0, 1.0), [255, 255, 255]);
    }

    #[test]
    fn effects_declare_who_renders_them() {
        assert!(EffectKind::Wave.animated());
        assert!(EffectKind::Audio.animated());
        assert!(!EffectKind::Static.animated());
        assert!(!EffectKind::PerKey.animated());
        assert!(EffectKind::Reactive.onboard());
        assert!(EffectKind::Off.onboard());
        assert!(!EffectKind::Static.onboard());
    }
}
