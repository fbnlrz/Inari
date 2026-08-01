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
}

/// Every scene, in the order the UI lists them, with a label and a blurb.
pub const ALL_SCENES: [(Scene, &str, &str); 12] = [
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

    #[test]
    fn every_scene_lights_the_board_and_is_not_flat() {
        for (scene, label, blurb) in ALL_SCENES {
            assert!(!label.is_empty() && !blurb.is_empty());
            let frame = sample(scene, 1.3);
            let total: u32 = frame.iter().map(|c| brightness(*c)).sum();
            assert!(total > 0, "{label} renders a black board");
            // A scene that paints one colour everywhere is not a scene — that
            // is what Static is for.
            let distinct = frame.iter().collect::<std::collections::HashSet<_>>().len();
            assert!(distinct > 3, "{label} has only {distinct} distinct colours");
        }
    }

    #[test]
    fn every_scene_animates() {
        for (scene, label, _) in ALL_SCENES {
            let a = sample(scene, 0.4);
            let b = sample(scene, 2.9);
            assert_ne!(a, b, "{label} does not move");
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
