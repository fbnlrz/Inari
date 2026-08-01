//! Themed lighting scenes for the keyboard.
//!
//! The plain effects in [`super::effects`] are the ones every vendor ships —
//! static, wave, breathing. These are the ones worth having a per-key board
//! for: each is a small piece of shader-style maths over the key's normalised
//! position and a phase, so they animate smoothly across whatever board is
//! attached rather than being baked for one layout.
//!
//! Everything here is pure and deterministic: same (x, y, phase) in, same
//! colour out. That is what lets the UI preview a scene without a keyboard, and
//! what makes them testable.

use serde::{Deserialize, Serialize};

/// A named scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scene {
    /// The Tokyo Night palette, drifting diagonally.
    TokyoNight,
    /// Dotonbori after dark: saturated neon with tube flicker.
    OsakaNeon,
    /// Hanami — petals drifting across a pale dusk.
    Sakura,
    /// Hokusai's wave: indigo sea, white foam crest rolling through.
    Kanagawa,
    /// Kitsune-bi — foxfire drifting over a dark board.
    Foxfire,
    /// Rain running down the board.
    Rain,
    /// Fuji at dawn: night at the top, sunrise climbing the horizon.
    FujiSunrise,
    /// Rings spreading from the middle in the chosen colour.
    Ripple,
    /// Slow aurora curtains.
    Aurora,
    /// Paper lanterns: warm amber, each flickering on its own.
    Lantern,
    /// Amanogawa — the Milky Way, with keys twinkling in it.
    Starfield,
    /// A blade sweeping across, leaving a fading trail.
    Slash,
    /// Hanabi — fireworks bursting over the board.
    Hanabi,
    /// A vermilion torii gate, breathing.
    Torii,
    /// Shibuya scramble: two streams of traffic crossing.
    Shibuya,
    /// Blade Runner rain — magenta and cyan, with the puddle glow.
    NeonRain,
    /// Datamosh: bands tearing, channels splitting.
    Glitch,
    /// Vaporwave sun over a perspective grid.
    Vaporwave,
    /// Koi circling in dark water.
    Koi,
    /// Komorebi — light falling through bamboo.
    Komorebi,
    /// Taiko: rings on the beat, red and gold.
    Taiko,
    /// Fire climbing the board.
    Inferno,
    /// Demoscene plasma, all of the colours.
    Plasma,
    /// Kaminari — a bolt, then the dark.
    Kaminari,
}

/// Every scene, in the order the UI lists them, with a label and a blurb.
pub const ALL_SCENES: [(Scene, &str, &str); 24] = [
    (
        Scene::TokyoNight,
        "Tokyo Night",
        "The theme's palette, drifting",
    ),
    (
        Scene::OsakaNeon,
        "Osaka Neon",
        "Dotonbori signage, tube flicker and all",
    ),
    (
        Scene::Sakura,
        "Sakura",
        "Petals drifting across a pale dusk",
    ),
    (
        Scene::Kanagawa,
        "Kanagawa",
        "Indigo sea with a white foam crest",
    ),
    (
        Scene::Foxfire,
        "Foxfire",
        "Kitsune-bi wandering over a dark board",
    ),
    (Scene::Rain, "Rain", "Drops running down the rows"),
    (
        Scene::FujiSunrise,
        "Fuji Sunrise",
        "Night above, dawn climbing the horizon",
    ),
    (Scene::Ripple, "Ripple", "Rings spreading from the middle"),
    (Scene::Aurora, "Aurora", "Slow curtains of green and violet"),
    (
        Scene::Lantern,
        "Lantern",
        "Warm amber, each key flickering alone",
    ),
    (Scene::Starfield, "Amanogawa", "The Milky Way, twinkling"),
    (
        Scene::Slash,
        "Slash",
        "A blade sweeping across, trailing light",
    ),
    (Scene::Hanabi, "Hanabi", "Fireworks bursting over the board"),
    (Scene::Torii, "Torii", "A vermilion gate, breathing"),
    (Scene::Shibuya, "Shibuya", "Two streams of traffic crossing"),
    (
        Scene::NeonRain,
        "Neon Rain",
        "Blade Runner downpour, puddles and all",
    ),
    (
        Scene::Glitch,
        "Glitch",
        "Datamosh: bands tearing, channels splitting",
    ),
    (Scene::Vaporwave, "Vaporwave", "Sun over a perspective grid"),
    (Scene::Koi, "Koi", "Two koi circling in dark water"),
    (Scene::Komorebi, "Komorebi", "Light falling through bamboo"),
    (Scene::Taiko, "Taiko", "Rings on the beat, red and gold"),
    (Scene::Inferno, "Inferno", "Fire climbing the board"),
    (
        Scene::Plasma,
        "Plasma",
        "Demoscene plasma, all of the colours",
    ),
    (Scene::Kaminari, "Kaminari", "A bolt, then the dark"),
];

impl Scene {
    pub fn label(self) -> &'static str {
        ALL_SCENES
            .iter()
            .find(|(s, _, _)| *s == self)
            .map(|(_, label, _)| *label)
            .unwrap_or("Scene")
    }
}

// --- small deterministic helpers ---------------------------------------

/// Integer hash → 0..1. A scene needs per-key randomness that is the same on
/// every frame (a key that re-rolls its "random" brightness each tick strobes
/// instead of twinkling), so this is a hash of the key, not a PRNG stream.
fn hash01(mut n: u32) -> f32 {
    n = n.wrapping_mul(0x2545_f491);
    n ^= n >> 15;
    n = n.wrapping_mul(0x85eb_ca6b);
    n ^= n >> 13;
    (n % 100_003) as f32 / 100_003.0
}

/// Smooth value noise in one dimension, for flicker and drift.
fn wobble(seed: u32, t: f32) -> f32 {
    use std::f32::consts::TAU;
    let a = hash01(seed);
    let b = hash01(seed ^ 0x9e37_79b9);
    let c = hash01(seed.wrapping_add(7));
    ((t * (0.7 + a) + b * TAU).sin() + (t * (1.9 + c) + a * TAU).sin() * 0.5) / 1.5
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
    ]
}

fn dim(c: [u8; 3], f: f32) -> [u8; 3] {
    let f = f.clamp(0.0, 1.0);
    [
        (c[0] as f32 * f) as u8,
        (c[1] as f32 * f) as u8,
        (c[2] as f32 * f) as u8,
    ]
}

fn add(a: [u8; 3], b: [u8; 3]) -> [u8; 3] {
    [
        a[0].saturating_add(b[0]),
        a[1].saturating_add(b[1]),
        a[2].saturating_add(b[2]),
    ]
}

/// Pick from a palette by a continuous position, blending between neighbours.
fn ramp(palette: &[[u8; 3]], pos: f32) -> [u8; 3] {
    let n = palette.len();
    let p = pos.rem_euclid(1.0) * n as f32;
    let i = p.floor() as usize % n;
    lerp(palette[i], palette[(i + 1) % n], p - p.floor())
}

/// Like [`ramp`] but clamped at both ends — for gradients that are a scale
/// (cold to hot), not a cycle.
fn ramp_clamped(palette: &[[u8; 3]], pos: f32) -> [u8; 3] {
    let n = palette.len();
    let p = (pos.clamp(0.0, 1.0) * (n - 1) as f32).min((n - 1) as f32 - 0.0001);
    let i = p.floor() as usize;
    lerp(palette[i], palette[(i + 1).min(n - 1)], p - p.floor())
}

/// A soft round falloff: 1 at the centre, 0 past `radius`.
fn glow(dx: f32, dy: f32, radius: f32) -> f32 {
    let d = (dx * dx + dy * dy).sqrt() / radius.max(0.001);
    (1.0 - d).clamp(0.0, 1.0).powi(2)
}

// --- palettes -----------------------------------------------------------

/// The Tokyo Night palette Inari already ships as a theme.
const TOKYO_NIGHT: [[u8; 3]; 5] = [
    [122, 162, 247], // blue
    [187, 154, 247], // purple
    [125, 207, 255], // cyan
    [247, 118, 142], // red
    [158, 206, 106], // green
];

/// Neon-sign primaries: magenta, cyan, amber, lime.
const OSAKA: [[u8; 3]; 4] = [
    [255, 40, 170],
    [40, 230, 255],
    [255, 170, 30],
    [140, 255, 60],
];

// --- the scenes ---------------------------------------------------------

/// Render one key. `x`/`y` are normalised (0..1) board coordinates, `phase`
/// advances with time, `seed` is the key's index (for per-key randomness) and
/// `accent` is the user's chosen colour where a scene uses one.
pub fn render(scene: Scene, x: f32, y: f32, phase: f32, seed: u32, accent: [u8; 3]) -> [u8; 3] {
    match scene {
        Scene::TokyoNight => {
            // Diagonal drift so the bands cross the board rather than marching
            // along one axis.
            ramp(&TOKYO_NIGHT, phase * 0.35 + x * 0.55 + y * 0.25)
        }

        Scene::OsakaNeon => {
            // Signs are lit in blocks, not gradients, so quantise into bands —
            // then let each band flicker like a tube that has seen better days.
            let band = (x * 4.0 + y * 0.7 + phase * 0.5).floor();
            let base = OSAKA[(band.rem_euclid(4.0)) as usize % 4];
            let flicker = wobble(band as u32 ^ 0x51ed, phase * 9.0);
            // Rare hard dropouts read as a failing tube; a constant shimmer
            // just looks like noise.
            let out = if hash01(band as u32 ^ (phase * 3.0) as u32) > 0.97 {
                0.15
            } else {
                0.72 + 0.28 * flicker
            };
            dim(base, out)
        }

        Scene::Sakura => {
            // Dusk sky, and a handful of petals drifting down-right with sway.
            let sky = lerp([28, 12, 26], [60, 24, 48], y);
            let mut out = sky;
            for petal in 0..7u32 {
                let speed = 0.09 + hash01(petal) * 0.07;
                let px = (hash01(petal ^ 0x1234) + phase * speed).rem_euclid(1.4) - 0.2;
                let py = (hash01(petal ^ 0x5678) + phase * speed * 0.55).rem_euclid(1.4) - 0.2;
                // Sway, so petals fall like petals and not like rain.
                let sway = (phase * 2.0 + petal as f32).sin() * 0.03;
                let g = glow(x - px - sway, (y - py) * 1.6, 0.17);
                if g > 0.0 {
                    let pink = lerp([255, 130, 180], [255, 240, 245], hash01(petal ^ 0xaaaa));
                    out = add(out, dim(pink, g));
                }
            }
            out
        }

        Scene::Kanagawa => {
            // A crest that curls: the wave front is not a straight line, it
            // rides higher on one side, which is what makes it read as a wave.
            let front = (phase * 0.45).rem_euclid(1.6) - 0.3;
            let curl = (y * std::f32::consts::PI).sin() * 0.12;
            let d = x - front - curl;
            let sea = lerp([4, 10, 48], [16, 46, 120], 1.0 - y);
            if d.abs() < 0.16 {
                // Foam, brightest right at the crest.
                let f = 1.0 - (d.abs() / 0.16);
                let foam = lerp([120, 190, 235], [255, 255, 255], f * f);
                lerp(sea, foam, f)
            } else if d < 0.0 {
                // Behind the crest the water is churned and lighter.
                lerp(sea, [40, 90, 170], 0.45)
            } else {
                sea
            }
        }

        Scene::Foxfire => {
            let mut out = [6, 4, 14];
            for flame in 0..3u32 {
                // Each flame wanders on its own slow Lissajous path.
                let fx = 0.5 + 0.42 * (phase * (0.31 + hash01(flame) * 0.2) + flame as f32).sin();
                let fy = 0.5 + 0.38 * (phase * (0.23 + hash01(flame ^ 9) * 0.2) + 1.7).cos();
                let pulse = 0.75 + 0.25 * (phase * 3.0 + flame as f32 * 2.1).sin();
                let g = glow(x - fx, (y - fy) * 0.75, 0.34) * pulse;
                let core = lerp([90, 170, 255], [225, 245, 255], g);
                out = add(out, dim(core, g));
            }
            out
        }

        Scene::Rain => {
            // One drop per column, each with its own speed and offset. Six rows
            // is not much, so the tail is what sells the motion.
            let col = (x * 24.0).floor() as u32;
            let speed = 0.55 + hash01(col) * 0.9;
            let head = (phase * speed + hash01(col ^ 0x77)).rem_euclid(1.45) - 0.15;
            let d = y - head;
            let sky = [3, 6, 18];
            if !(-0.55..=0.0).contains(&d) {
                sky
            } else {
                let t = 1.0 + d / 0.55; // 1 at the head, 0 at the tail's end
                lerp(sky, [150, 220, 255], t * t)
            }
        }

        Scene::FujiSunrise => {
            // The horizon climbs, so the board goes from night to dawn and back.
            let horizon = 0.55 + 0.35 * (phase * 0.25).sin();
            let sky = ramp(
                &[[8, 10, 40], [40, 26, 78], [190, 70, 60], [255, 150, 60]],
                (y * 0.5 + (1.0 - horizon) * 0.5).clamp(0.0, 0.999),
            );
            // The sun itself, sitting on the horizon.
            let g = glow(x - 0.5, (y - horizon) * 1.4, 0.22);
            add(sky, dim([255, 220, 140], g))
        }

        Scene::Ripple => {
            let d = ((x - 0.5).powi(2) + ((y - 0.5) * 0.55).powi(2)).sqrt();
            // Rings travel outward; the falloff keeps the far edge from
            // flickering as fast as the middle.
            let ring = ((d * 9.0 - phase * 3.0).sin() * 0.5 + 0.5).powi(3);
            dim(accent, 0.08 + 0.92 * ring)
        }

        Scene::Aurora => {
            // Curtains: vertical bands warped by slow noise, greens into violet.
            let warp = (x * 3.0 + phase * 0.5).sin() * 0.15 + (y * 5.0 - phase * 0.3).sin() * 0.05;
            let band = ((x + warp) * 1.6 + phase * 0.18).rem_euclid(1.0);
            let hue = ramp(
                &[
                    [20, 200, 140],
                    [40, 230, 200],
                    [120, 120, 255],
                    [180, 80, 220],
                ],
                band,
            );
            // Brighter at the top, like the real thing hanging in the sky.
            let shimmer = 0.55 + 0.45 * (band * std::f32::consts::TAU).sin().abs();
            dim(hue, (0.35 + 0.65 * (1.0 - y)) * shimmer)
        }

        Scene::Lantern => {
            // Every key is its own lantern, so the flicker must not be in sync.
            let f = 0.62 + 0.38 * wobble(seed, phase * 2.2);
            let warm = lerp([255, 120, 30], [255, 205, 120], hash01(seed ^ 0x3333));
            dim(warm, f)
        }

        Scene::Starfield => {
            // A denser diagonal band — the galaxy — over a deep indigo sky.
            let band = 1.0 - ((y - 0.35 - x * 0.3).abs() * 3.2).clamp(0.0, 1.0);
            let sky = dim([40, 40, 120], 0.10 + 0.35 * band * band);
            // Only some keys are stars at all, and they twinkle at their own
            // rate rather than all together.
            let is_star = hash01(seed ^ 0xbeef);
            if is_star > 0.62 - band * 0.25 {
                let rate = 1.5 + hash01(seed) * 3.0;
                let offset = hash01(seed ^ 5) * std::f32::consts::TAU;
                let tw = 0.5 + 0.5 * (phase * rate + offset).sin();
                add(sky, dim([255, 255, 235], tw.powi(3)))
            } else {
                sky
            }
        }

        Scene::Hanabi => {
            // Several shells in flight at once, each on its own cycle. A burst
            // is a bright shock front with sparks trailing behind it, so the
            // ring has to be thin and the inside has to fade fast.
            let mut out = [3, 3, 10];
            for shell in 0..4u32 {
                let cycle = 2.4 + hash01(shell) * 1.6;
                let t = (phase + hash01(shell ^ 0x30) * cycle) / cycle;
                let epoch = t.floor() as u32;
                let age = t.fract();
                // A new place to burst on every cycle, or it looks mechanical.
                let cx = 0.1 + hash01(shell ^ epoch.wrapping_mul(2_654_435_761)) * 0.8;
                let cy = 0.15 + hash01(shell ^ epoch.wrapping_add(77)) * 0.5;
                let d = ((x - cx).powi(2) + ((y - cy) * 0.6).powi(2)).sqrt();
                let radius = age * 0.75;
                let shock = 1.0 - ((d - radius).abs() / 0.09).clamp(0.0, 1.0);
                // Sparks lag behind the front and fall.
                let spark = if d < radius {
                    let flick = hash01(seed ^ shell ^ (d * 90.0) as u32);
                    (1.0 - age).powi(3) * flick
                } else {
                    0.0
                };
                let colour = ramp(
                    &[
                        [255, 90, 60],
                        [255, 210, 90],
                        [120, 220, 255],
                        [255, 120, 220],
                    ],
                    hash01(shell ^ epoch),
                );
                let fade = (1.0 - age).powi(2);
                out = add(out, dim(colour, (shock.powi(2) + spark * 0.7) * fade));
            }
            out
        }

        Scene::Torii => {
            // Two pillars and two beams, in the vermilion Inari is named for.
            // Drawn in board space so it lands on the same keys on any layout.
            let breathe = 0.72 + 0.28 * (phase * 1.1).sin();
            let pillar = ((x - 0.24).abs() < 0.055) || ((x - 0.76).abs() < 0.055);
            let kasagi = y < 0.18; // the top beam, with its upward sweep
            let nuki = (y - 0.42).abs() < 0.07; // the second beam
            let on_gate = (pillar && y > 0.1) || kasagi || nuki;
            let night = lerp([2, 4, 16], [10, 8, 34], 1.0 - y);
            if on_gate {
                let vermilion = lerp([220, 40, 10], [255, 130, 60], breathe);
                add(dim(vermilion, 0.85 + 0.15 * breathe), dim([60, 20, 0], 0.4))
            } else {
                // The gate throws light onto what is behind it.
                let dx = (x - 0.24).abs().min((x - 0.76).abs());
                add(
                    night,
                    dim([120, 30, 10], glow(dx, 0.0, 0.22) * 0.5 * breathe),
                )
            }
        }

        Scene::Shibuya => {
            // Traffic in two directions, crossing. Streaks are long and thin,
            // which is what makes them read as movement rather than as blinks.
            let mut out = [4, 4, 8];
            for lane in 0..4u32 {
                let row = 0.15 + hash01(lane) * 0.7;
                if (y - row).abs() < 0.09 {
                    let speed = 0.5 + hash01(lane ^ 3) * 1.1;
                    let head = (phase * speed + hash01(lane ^ 9)).rem_euclid(1.5) - 0.25;
                    let d = x - head;
                    if d < 0.0 && d > -0.3 {
                        let t = 1.0 + d / 0.3;
                        out = add(out, dim([255, 220, 150], t.powi(3)));
                    }
                }
            }
            for lane in 0..5u32 {
                let col = 0.1 + hash01(lane ^ 0x55) * 0.8;
                if (x - col).abs() < 0.035 {
                    let speed = 0.7 + hash01(lane ^ 0x77) * 1.4;
                    let head = (phase * speed + hash01(lane)).rem_euclid(1.6) - 0.3;
                    let d = y - head;
                    if d < 0.0 && d > -0.6 {
                        let t = 1.0 + d / 0.6;
                        out = add(out, dim([80, 230, 255], t.powi(2)));
                    }
                }
            }
            out
        }

        Scene::NeonRain => {
            // Rain, but lit by signage: every drop takes a side of the palette,
            // and the bottom row is the puddle it lands in.
            let col = (x * 26.0).floor() as u32;
            let speed = 0.8 + hash01(col) * 1.4;
            let head = (phase * speed + hash01(col ^ 0x99)).rem_euclid(1.5) - 0.2;
            let tint = if hash01(col ^ 0x1f) > 0.5 {
                [255, 40, 190]
            } else {
                [40, 220, 255]
            };
            let mut out = [6, 2, 12];
            let d = y - head;
            if d <= 0.0 && d > -0.45 {
                let t = 1.0 + d / 0.45;
                out = add(out, dim(tint, t * t));
            }
            // Puddle: the bottom rows glow whenever a drop has just landed.
            if y > 0.72 {
                let splash = (1.0 - (head - 1.0).abs() * 4.0).clamp(0.0, 1.0);
                out = add(out, dim(tint, splash * 0.35 * (y - 0.72) * 3.0));
            }
            out
        }

        Scene::Glitch => {
            // A clean gradient, torn. The tearing is per-band and per-epoch, so
            // it holds still for a beat and then jumps — continuous noise reads
            // as static, not as a glitch.
            let epoch = (phase * 6.0) as u32;
            let band = (y * 6.0).floor() as u32;
            let broken = hash01(band ^ epoch.wrapping_mul(2_246_822_519)) > 0.72;
            let shift = if broken {
                (hash01(band ^ epoch ^ 0xabc) - 0.5) * 0.6
            } else {
                0.0
            };
            let base = ramp(
                &[
                    [20, 220, 255],
                    [180, 40, 255],
                    [255, 40, 120],
                    [255, 220, 60],
                ],
                (x + shift) * 0.8 + phase * 0.15,
            );
            if broken {
                // Channel split: shove red one way and blue the other.
                let r = ramp(
                    &[
                        [20, 220, 255],
                        [180, 40, 255],
                        [255, 40, 120],
                        [255, 220, 60],
                    ],
                    (x + shift + 0.05) * 0.8 + phase * 0.15,
                );
                let b = ramp(
                    &[
                        [20, 220, 255],
                        [180, 40, 255],
                        [255, 40, 120],
                        [255, 220, 60],
                    ],
                    (x + shift - 0.05) * 0.8 + phase * 0.15,
                );
                let out = [r[0], base[1], b[2]];
                // The odd blown-out block.
                if hash01(seed ^ epoch) > 0.93 {
                    [255, 255, 255]
                } else {
                    out
                }
            } else {
                dim(base, 0.85)
            }
        }

        Scene::Vaporwave => {
            // Sun above the horizon with its slice lines, grid below.
            let horizon = 0.45;
            if y < horizon {
                let sun = glow(x - 0.5, (y - horizon + 0.28) * 1.5, 0.3);
                let sky = lerp([40, 10, 70], [10, 4, 30], y / horizon);
                // The sun's horizontal cuts, which is the whole look.
                let sliced = ((y * 34.0 + phase * 1.5).sin() * 0.5 + 0.5).powf(0.6);
                let disc = lerp([255, 60, 140], [255, 200, 60], 1.0 - y / horizon);
                add(sky, dim(disc, sun * sliced))
            } else {
                let depth = (y - horizon) / (1.0 - horizon);
                // Lines converge as they recede, and scroll towards the viewer.
                let rows = ((depth * depth * 9.0 - phase * 0.8).fract() * 4.0).min(1.0);
                let cols = (((x - 0.5) / (depth * 0.9 + 0.12)).fract() - 0.5).abs() * 6.0;
                let line = (1.0 - rows).max((1.0 - cols).clamp(0.0, 1.0));
                add([8, 2, 24], dim([80, 255, 240], line.clamp(0.0, 1.0) * 0.9))
            }
        }

        Scene::Koi => {
            let mut out = lerp([2, 12, 14], [4, 26, 30], y);
            for fish in 0..2u32 {
                // A slow circle, one fish trailing the other.
                let t = phase * 0.5 + fish as f32 * std::f32::consts::PI;
                let fx = 0.5 + 0.34 * t.cos();
                let fy = 0.5 + 0.3 * (t * 0.9).sin();
                // The body is an ellipse aligned with the direction of travel.
                let dx = x - fx;
                let dy = (y - fy) * 0.6;
                let body = glow(dx, dy, 0.2);
                let colour = if fish == 0 {
                    [255, 110, 20]
                } else {
                    [255, 245, 235]
                };
                out = add(out, dim(colour, body.powf(1.4)));
                // Wake behind it.
                let wake = glow(dx + t.cos() * 0.12, dy, 0.26) * 0.25;
                out = add(out, dim([120, 200, 210], wake));
            }
            out
        }

        Scene::Komorebi => {
            // Bamboo stalks, and shafts of light drifting across them.
            let stalk = ((x * 9.0).fract() - 0.5).abs();
            let green = if stalk < 0.22 {
                lerp([10, 60, 20], [60, 170, 70], 1.0 - stalk / 0.22)
            } else {
                [4, 16, 10]
            };
            let shaft = (1.0 - ((x - 0.5 - (phase * 0.25).sin() * 0.4 + y * 0.3).abs() * 3.5))
                .clamp(0.0, 1.0);
            add(green, dim([230, 255, 190], shaft.powi(2) * 0.55))
        }

        Scene::Taiko => {
            // On the beat, not continuously: the silence between hits is what
            // makes a drum a drum.
            let beat = phase * 1.6;
            let hit = beat.fract();
            let d = ((x - 0.5).powi(2) + ((y - 0.5) * 0.5).powi(2)).sqrt();
            let ring = 1.0 - ((d - hit * 0.8) / 0.12).abs().clamp(0.0, 1.0);
            let decay = (1.0 - hit).powi(2);
            // Every fourth hit is the big one.
            let accent_beat = (beat as u32) % 4 == 0;
            let colour = if accent_beat {
                [255, 200, 60]
            } else {
                [220, 30, 30]
            };
            add(dim([16, 2, 2], 1.0), dim(colour, ring.powi(2) * decay))
        }

        Scene::Inferno => {
            // Heat rises: intensity is highest at the bottom and eaten away by
            // noise on the way up.
            let heat = (1.0 - y).powf(1.6);
            let turbulence = 0.5
                + 0.5 * ((x * 7.0 + phase * 2.2).sin() * 0.5 + (y * 9.0 - phase * 3.4).sin() * 0.5);
            let f = (heat * (0.55 + 0.75 * turbulence)).clamp(0.0, 1.0);
            ramp_clamped(
                &[
                    [0, 0, 0],
                    [90, 0, 0],
                    [220, 60, 0],
                    [255, 170, 20],
                    [255, 250, 200],
                ],
                f,
            )
        }

        Scene::Plasma => {
            // The demoscene classic: a sum of sines, straight into a wide
            // palette. Cheap, and still the most colourful thing here.
            let v = (x * 6.0 + phase).sin()
                + (y * 5.0 + phase * 1.3).sin()
                + ((x + y) * 4.0 + phase * 0.7).sin()
                + ((x * x + y * y).sqrt() * 8.0 - phase * 1.9).sin();
            ramp(
                &[
                    [255, 0, 120],
                    [255, 180, 0],
                    [0, 255, 160],
                    [0, 140, 255],
                    [160, 0, 255],
                ],
                v * 0.125 + 0.5,
            )
        }

        Scene::Kaminari => {
            // Dark, then a bolt. The flash lights the whole board for an
            // instant, which is the part you actually notice.
            let strike = phase * 0.55;
            let epoch = strike as u32;
            let age = strike.fract();
            if age > 0.22 {
                return dim([6, 8, 20], 1.0 - age * 0.5);
            }
            // The bolt is a jagged vertical path: each row offsets the last.
            let row = (y * 6.0).floor() as u32;
            let mut bolt_x = 0.2 + hash01(epoch) * 0.6;
            for step in 0..=row {
                bolt_x += (hash01(epoch ^ step.wrapping_mul(2_246_822_519)) - 0.5) * 0.22;
            }
            let d = (x - bolt_x).abs();
            let flash = (1.0 - age / 0.22).powi(2);
            let sky = dim([90, 110, 190], flash * 0.45);
            let core = (1.0 - (d / 0.06).clamp(0.0, 1.0)).powi(2);
            add(sky, dim([255, 255, 255], core * flash))
        }

        Scene::Slash => {
            // Mostly dark, then a blade crosses. The gap between strikes is the
            // point — a constant sweep is just a wave.
            let cycle = phase.rem_euclid(3.0);
            let pos = cycle * 0.75 - 0.4;
            let d = (x + (y - 0.5) * 0.55) - pos;
            if cycle > 1.9 || d > 0.0 {
                [2, 2, 4]
            } else {
                // Trail fades behind the edge.
                let t = (1.0 + d / 0.5).clamp(0.0, 1.0);
                let edge = t.powi(6);
                add(dim(accent, t * t * 0.55), dim([255, 255, 255], edge))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key of a coarse grid, for "does this scene do anything" checks.
    fn sample(scene: Scene, phase: f32) -> Vec<[u8; 3]> {
        let mut out = Vec::new();
        for row in 0..6 {
            for col in 0..20 {
                let x = col as f32 / 19.0;
                let y = row as f32 / 5.0;
                out.push(render(
                    scene,
                    x,
                    y,
                    phase,
                    (row * 20 + col) as u32,
                    [255, 82, 0],
                ));
            }
        }
        out
    }

    fn brightness(c: [u8; 3]) -> u32 {
        c[0] as u32 + c[1] as u32 + c[2] as u32
    }

    /// Frames across a whole cycle. A single instant says nothing about a
    /// scene that is deliberately dark between events — Kaminari is a bolt and
    /// then the dark, Taiko is a hit and then silence — and two instants can
    /// land on the same beat by pure aliasing, which is exactly what a
    /// two-point version of these tests did.
    fn over_time(scene: Scene) -> Vec<Vec<[u8; 3]>> {
        (0..48).map(|i| sample(scene, i as f32 * 0.23)).collect()
    }

    #[test]
    fn every_scene_lights_the_board_and_is_not_flat() {
        for (scene, label, blurb) in ALL_SCENES {
            assert!(!label.is_empty() && !blurb.is_empty());
            let frames = over_time(scene);
            let brightest: u32 = frames
                .iter()
                .map(|f| f.iter().map(|c| brightness(*c)).sum())
                .max()
                .unwrap_or(0);
            assert!(brightest > 0, "{label} never lights the board");
            // A scene that paints one colour everywhere, always, is not a
            // scene — that is what Static is for.
            let richest = frames
                .iter()
                .map(|f| f.iter().collect::<std::collections::HashSet<_>>().len())
                .max()
                .unwrap_or(0);
            assert!(richest > 3, "{label} never gets past {richest} colours");
        }
    }

    #[test]
    fn every_scene_animates() {
        for (scene, label, _) in ALL_SCENES {
            let frames = over_time(scene);
            assert!(
                frames.iter().any(|f| *f != frames[0]),
                "{label} does not move"
            );
        }
    }

    #[test]
    fn scenes_are_deterministic() {
        // The UI previews a scene by rendering it; if the same inputs gave
        // different colours the preview would never match the board.
        for (scene, label, _) in ALL_SCENES {
            assert_eq!(
                sample(scene, 1.0),
                sample(scene, 1.0),
                "{label} is unstable"
            );
        }
    }

    #[test]
    fn nothing_wraps_around_into_a_dark_key() {
        // Saturating adds and clamped dims everywhere: a bright scene must not
        // produce a black key by overflowing.
        for (scene, label, _) in ALL_SCENES {
            for step in 0..40 {
                let frame = sample(scene, step as f32 * 0.37);
                // Saturating adds and clamped dims everywhere: a bright scene
                // must not wrap a channel round into darkness. Overflow would
                // show up as a lit board suddenly going black in one channel.
                assert!(
                    frame.iter().any(|c| brightness(*c) > 0),
                    "{label} went fully dark at step {step}"
                );
            }
        }
    }

    #[test]
    fn the_accent_colour_reaches_the_scenes_that_use_one() {
        let red = render(Scene::Ripple, 0.5, 0.5, 0.0, 0, [255, 0, 0]);
        let blue = render(Scene::Ripple, 0.5, 0.5, 0.0, 0, [0, 0, 255]);
        assert_ne!(red, blue, "Ripple ignores the chosen colour");
    }

    #[test]
    fn the_slash_has_a_gap_between_strikes() {
        // Sampled at the end of the cycle the board should be nearly dark; a
        // blade that never stops is just a wave with extra steps.
        let quiet: u32 = sample(Scene::Slash, 2.95)
            .iter()
            .map(|c| brightness(*c))
            .sum();
        let strike: u32 = sample(Scene::Slash, 0.6)
            .iter()
            .map(|c| brightness(*c))
            .sum();
        assert!(strike > quiet * 3, "strike {strike} vs quiet {quiet}");
    }

    #[test]
    fn rain_falls_down_rather_than_sideways() {
        // The drop head is a function of y; two keys in the same column and
        // different rows must differ, while the scene stays column-wise.
        let top = render(Scene::Rain, 0.5, 0.0, 0.8, 0, [0, 0, 0]);
        let bottom = render(Scene::Rain, 0.5, 1.0, 0.8, 0, [0, 0, 0]);
        assert_ne!(top, bottom);
    }

    #[test]
    fn labels_are_unique_so_the_picker_is_unambiguous() {
        let mut labels: Vec<&str> = ALL_SCENES.iter().map(|(_, l, _)| *l).collect();
        let total = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), total);
        assert_eq!(Scene::TokyoNight.label(), "Tokyo Night");
    }
}
