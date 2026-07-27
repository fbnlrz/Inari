//! Native mic DSP chain (Phase 3): noise gate → gain → compressor →
//! limiter. Pure Rust, no LV2/LADSPA. Runs per-sample inside the mic
//! capture stream's process callback (mono).
//!
//! All stages use one-pole envelope followers with attack/release smoothing
//! so gain changes never click.

/// Tunable parameters, updated from the UI thread via atomics in `mic.rs`.
#[derive(Debug, Clone, Copy)]
pub struct DspSettings {
    pub gate_enabled: bool,
    pub comp_enabled: bool,
    pub limiter_enabled: bool,
    /// Linear gain multiplier (UI percent / 100).
    pub gain: f32,
    pub muted: bool,
    /// Soundboard ducking: a linear attenuation applied *on top of* `gain`
    /// while a clip is playing, 1.0 when nothing is ducking. It is a separate
    /// field on purpose - folding it into `gain` would mean writing a
    /// temporary value into the user's own microphone setting and leaving
    /// whatever the last clip left behind in their config.
    pub duck: f32,
    /// Tunable stage parameters (UI-exposed; time constants stay fixed).
    pub gate_threshold_db: f32,
    pub comp_threshold_db: f32,
    pub comp_ratio: f32,
    pub limiter_ceiling_db: f32,
}

impl Default for DspSettings {
    fn default() -> Self {
        Self {
            gate_enabled: true,
            comp_enabled: true,
            limiter_enabled: true,
            gain: 1.0,
            muted: false,
            duck: 1.0,
            gate_threshold_db: -40.0,
            comp_threshold_db: -18.0,
            comp_ratio: 3.0,
            limiter_ceiling_db: -1.0,
        }
    }
}

// Fixed time constants (voice-chain guidance - OBS-style starting
// points). Thresholds/ratio/ceiling are user-tunable via DspSettings.
const GATE_ATTACK_MS: f32 = 5.0;
const GATE_RELEASE_MS: f32 = 150.0;
const GATE_HOLD_MS: f32 = 200.0;

const COMP_ATTACK_MS: f32 = 6.0;
const COMP_RELEASE_MS: f32 = 60.0;
const COMP_MAKEUP_DB: f32 = 4.0;

const LIMIT_RELEASE_MS: f32 = 60.0;

/// Time constant of the ducking fade, in and back out. Between the gate's
/// attack and its release: fast enough that the mic is down before the clip's
/// first syllable, slow enough not to click - stepping the gain in a single
/// sample would put an audible tick at both ends of every clip. It settles in
/// roughly three of these.
const DUCK_RAMP_MS: f32 = 15.0;

fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// One-pole smoothing coefficient for a time constant in ms.
fn coeff(ms: f32, sample_rate: f32) -> f32 {
    if ms <= 0.0 {
        return 0.0;
    }
    (-1.0 / (ms * 0.001 * sample_rate)).exp()
}

pub struct DspChain {
    sample_rate: f32,
    // gate
    gate_env: f32,
    gate_gain: f32,
    gate_hold: u32,
    // compressor
    comp_env: f32,
    // limiter
    limit_gain: f32,
    // soundboard ducking
    duck_gain: f32,
}

impl DspChain {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            gate_env: 0.0,
            gate_gain: 0.0,
            gate_hold: 0,
            comp_env: 0.0,
            limit_gain: 1.0,
            duck_gain: 1.0,
        }
    }

    /// Process a mono buffer in place.
    pub fn process(&mut self, samples: &mut [f32], s: &DspSettings) {
        if s.muted {
            samples.fill(0.0);
            return;
        }

        let sr = self.sample_rate;
        let gate_thresh = db_to_linear(s.gate_threshold_db);
        let gate_att = coeff(GATE_ATTACK_MS, sr);
        let gate_rel = coeff(GATE_RELEASE_MS, sr);
        let hold_samples = (GATE_HOLD_MS * 0.001 * sr) as u32;

        let comp_thresh_db = s.comp_threshold_db;
        let comp_att = coeff(COMP_ATTACK_MS, sr);
        let comp_rel = coeff(COMP_RELEASE_MS, sr);
        let makeup = db_to_linear(COMP_MAKEUP_DB);

        let ceiling = db_to_linear(s.limiter_ceiling_db);
        let limit_rel = coeff(LIMIT_RELEASE_MS, sr);

        let duck_target = s.duck.clamp(0.0, 1.0);
        let duck_c = coeff(DUCK_RAMP_MS, sr);

        for sample in samples.iter_mut() {
            let mut x = *sample;

            // ---- noise gate ----
            if s.gate_enabled {
                let mag = x.abs();
                // envelope follower (fast attack, slower release)
                self.gate_env = if mag > self.gate_env {
                    mag + gate_att * (self.gate_env - mag)
                } else {
                    mag + gate_rel * (self.gate_env - mag)
                };
                let open = self.gate_env > gate_thresh;
                if open {
                    self.gate_hold = hold_samples;
                } else if self.gate_hold > 0 {
                    self.gate_hold -= 1;
                }
                let target = if open || self.gate_hold > 0 { 1.0 } else { 0.0 };
                let c = if target > self.gate_gain { gate_att } else { gate_rel };
                self.gate_gain = target + c * (self.gate_gain - target);
                x *= self.gate_gain;
            }

            // ---- gain ----
            x *= s.gain;

            // ---- compressor (downward, feed-forward) ----
            if s.comp_enabled {
                let mag = x.abs().max(1e-9);
                self.comp_env = if mag > self.comp_env {
                    mag + comp_att * (self.comp_env - mag)
                } else {
                    mag + comp_rel * (self.comp_env - mag)
                };
                let env_db = 20.0 * self.comp_env.log10();
                let over = env_db - comp_thresh_db;
                if over > 0.0 {
                    let reduction_db = over * (1.0 - 1.0 / s.comp_ratio.max(1.0));
                    x *= db_to_linear(-reduction_db);
                }
                x *= makeup;
            }

            // ---- limiter (hard knee, instant attack, smooth release) ----
            if s.limiter_enabled {
                let mag = x.abs();
                let needed = if mag * self.limit_gain > ceiling {
                    ceiling / mag
                } else {
                    1.0
                };
                if needed < self.limit_gain {
                    self.limit_gain = needed; // clamp instantly
                } else {
                    self.limit_gain = needed + limit_rel * (self.limit_gain - needed);
                }
                x *= self.limit_gain;
                x = x.clamp(-ceiling, ceiling);
            }

            // ---- soundboard ducking ----
            // Last in the chain, so the attenuation the user asked for is the
            // attenuation the chat hears: applied before the compressor, its
            // makeup gain and the limiter would each give part of it back.
            self.duck_gain = duck_target + duck_c * (self.duck_gain - duck_target);
            x *= self.duck_gain;

            *sample = x;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(gate: bool, comp: bool, limit: bool, gain: f32) -> DspSettings {
        DspSettings {
            gate_enabled: gate,
            comp_enabled: comp,
            limiter_enabled: limit,
            gain,
            ..DspSettings::default()
        }
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
    }

    #[test]
    fn mute_silences_everything() {
        let mut chain = DspChain::new(48000.0);
        let mut buf = vec![0.5f32; 480];
        let mut s = settings(false, false, false, 1.0);
        s.muted = true;
        chain.process(&mut buf, &s);
        assert_eq!(peak(&buf), 0.0);
    }

    #[test]
    fn gate_blocks_noise_floor_but_passes_speech() {
        let mut chain = DspChain::new(48000.0);
        // quiet hiss well below -45 dB (~0.001 ≈ -60 dB)
        let mut hiss: Vec<f32> = (0..4800).map(|i| 0.001 * ((i % 7) as f32 - 3.0) / 3.0).collect();
        chain.process(&mut hiss, &settings(true, false, false, 1.0));
        assert!(peak(&hiss) < 0.0005, "noise should be gated, got {}", peak(&hiss));

        // loud signal (~-12 dB) opens the gate
        let mut chain = DspChain::new(48000.0);
        let mut voice: Vec<f32> = (0..4800)
            .map(|i| 0.25 * (i as f32 * 0.05).sin())
            .collect();
        chain.process(&mut voice, &settings(true, false, false, 1.0));
        // after the attack settles, the tail should be near full level
        assert!(peak(&voice[2400..]) > 0.2, "speech should pass the gate");
    }

    #[test]
    fn gain_scales_linearly() {
        let mut chain = DspChain::new(48000.0);
        let mut buf = vec![0.1f32; 480];
        chain.process(&mut buf, &settings(false, false, false, 2.0));
        assert!((buf[479] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn compressor_reduces_dynamic_range() {
        // Loud signal: -6 dB in, threshold -18 dB, ratio 3 → reduction.
        let mut chain = DspChain::new(48000.0);
        let mut loud: Vec<f32> = (0..48000).map(|i| 0.5 * (i as f32 * 0.06).sin()).collect();
        chain.process(&mut loud, &settings(false, true, false, 1.0));
        let out_peak = peak(&loud[24000..]);
        // -6 dB over threshold is 12 dB; reduced by 12*(1-1/3)=8 dB, +4 makeup
        // → net -4 dB from input peak 0.5 → ~0.315. Allow generous tolerance.
        assert!(out_peak < 0.45, "expected compression, peak={out_peak}");
        assert!(out_peak > 0.2, "compression overshot, peak={out_peak}");
    }

    #[test]
    fn ducking_reaches_the_requested_attenuation_and_lets_go_again() {
        let mut chain = DspChain::new(48000.0);
        let mut s = settings(false, false, false, 1.0);
        // -12 dB while a clip plays.
        s.duck = 0.25;
        let mut buf = vec![0.5f32; 48000];
        chain.process(&mut buf, &s);
        // The tail is past the ramp: exactly a quarter of the input.
        assert!((buf[47999] - 0.125).abs() < 1e-4, "got {}", buf[47999]);

        // Clip over: back to unity, and back to the *user's* level - the gain
        // setting was never touched.
        s.duck = 1.0;
        let mut buf = vec![0.5f32; 48000];
        chain.process(&mut buf, &s);
        assert!((buf[47999] - 0.5).abs() < 1e-4, "got {}", buf[47999]);
    }

    #[test]
    fn ducking_ramps_instead_of_stepping() {
        // A one-sample jump from 1.0 to 0.25 is a click in someone's mic.
        // Both edges have to be gradual.
        let mut chain = DspChain::new(48000.0);
        let mut s = settings(false, false, false, 1.0);
        s.duck = 0.25;
        let mut buf = vec![1.0f32; 4800];
        chain.process(&mut buf, &s);
        let step = (buf[0] - buf[1]).abs();
        assert!(step < 0.01, "duck-in steps by {step} in one sample");
        assert!(buf[0] > 0.9, "the first sample is still near unity");
        // 2400 samples = 50 ms, i.e. a few time constants: essentially there.
        assert!(buf[2400] < 0.3, "duck did not reach its target: {}", buf[2400]);

        s.duck = 1.0;
        let mut back = vec![1.0f32; 4800];
        chain.process(&mut back, &s);
        let step = (back[0] - back[1]).abs();
        assert!(step < 0.01, "duck-out steps by {step} in one sample");
        assert!(back[0] < 0.3, "release starts from where ducking left off");
        assert!(back[4799] > 0.9, "and finishes back at unity");
    }

    #[test]
    fn ducking_is_inert_at_its_default() {
        // The chain's default must be indistinguishable from a build without
        // ducking at all.
        let mut chain = DspChain::new(48000.0);
        let mut buf = vec![0.3f32; 480];
        chain.process(&mut buf, &settings(false, false, false, 1.0));
        assert!((buf[479] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn limiter_holds_ceiling() {
        let mut chain = DspChain::new(48000.0);
        // grossly hot signal, gain-boosted ×4
        let mut buf: Vec<f32> = (0..48000).map(|i| 0.9 * (i as f32 * 0.07).sin()).collect();
        chain.process(&mut buf, &settings(false, false, true, 4.0));
        let ceiling = db_to_linear(-1.0);
        assert!(peak(&buf) <= ceiling + 1e-4, "peak {} above ceiling {}", peak(&buf), ceiling);
    }
}
