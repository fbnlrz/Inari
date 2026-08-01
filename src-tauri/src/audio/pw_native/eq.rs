//! Per-channel parametric EQ core: RBJ Audio EQ Cookbook biquads in a
//! cascade of up to MAX_EQ_BANDS, preceded by a preamp trim.
//!
//! Pure Rust, no external DSP crates (same stance as the mic chain in
//! `dsp.rs`). The frequency-response math is hand-mirrored in
//! `src/lib/eqMath.ts` for the UI curve - keep both in sync.
//!
//! Threading model: the command thread writes band parameters into
//! `EqParams` (plain atomics) and bumps a generation counter with Release
//! ordering; the RT capture callback owns an `EqEngine` and redesigns its
//! coefficients only when an Acquire load of the generation sees a change.
//! Coefficient design (a few sin/cos) off the hot path per *change*, not
//! per buffer, and never a lock on the RT thread.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};

use crate::audio::types::{EqBand, EqBandKind, EqConfig, MAX_EQ_BANDS};

/// Absolute dB bound the DSP itself enforces on band gain and preamp.
/// Deliberately wider than the config policy (`EqConfig::clamp_ranges`,
/// ±24 dB): this layer is not the policy, it only guarantees the cascade
/// can never be handed an inf/NaN that poisons every later sample.
const MAX_DSP_GAIN_DB: f32 = 60.0;

/// Non-finite values (a corrupted config, a torn read) must never reach the
/// math - `powf(inf)` is inf, and one inf sample sticks forever in the
/// filter memory.
#[inline]
fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

/// Biquad transfer-function coefficients, normalized so a0 == 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadCoeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

/// Largest shelf slope that keeps the cookbook's radicand positive.
///
/// `(A + 1/A)(1/S - 1) + 2 > 0` rearranges to `S < A' / (A' - 2)` with
/// `A' = A + 1/A`; at or below unity gain (`A' <= 2`) every slope is fine.
/// Note `A' > 2` holds for a cut as well as a boost — `A` and `1/A` are
/// symmetric — so negative gains need the same cap.
/// The margin keeps the poles off the unit circle rather than merely on the
/// right side of it — the difference between those two is an oscillator.
fn shelf_slope_limit(slope: f32, a: f32) -> f32 {
    const MARGIN: f32 = 0.995;
    let ap = a + 1.0 / a;
    let slope = slope.max(0.01);
    if ap <= 2.0 {
        return slope;
    }
    slope.min(ap / (ap - 2.0) * MARGIN)
}

impl BiquadCoeffs {
    /// Pass-through (unity) filter.
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    /// RBJ Audio EQ Cookbook design. For shelves, `q` is the shelf slope S
    /// (not a resonance Q) - the schema shares one field for both, see
    /// `EqBand::q`. LowPass/HighPass ignore `gain_db`.
    pub fn design(kind: EqBandKind, freq_hz: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        // Guard the math: freq must sit below Nyquist, q must be positive
        // and the gain must be finite. Config-level clamps enforce this for
        // real input; this is the last line of defense against a
        // divide-by-zero or an inf leaking into the cascade.
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48000.0
        };
        // .max(1.0) keeps the clamp bounds ordered at absurd sample rates
        // (clamp panics when min > max).
        let freq = finite_or(freq_hz, 1000.0).clamp(1.0, (sample_rate * 0.49).max(1.0));
        let q = finite_or(q, 1.0).max(0.01);
        let gain_db = finite_or(gain_db, 0.0).clamp(-MAX_DSP_GAIN_DB, MAX_DSP_GAIN_DB);
        let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let a = 10.0f32.powf(gain_db / 40.0);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            EqBandKind::Peaking => {
                let alpha = sin_w0 / (2.0 * q);
                (
                    1.0 + alpha * a,
                    -2.0 * cos_w0,
                    1.0 - alpha * a,
                    1.0 + alpha / a,
                    -2.0 * cos_w0,
                    1.0 - alpha / a,
                )
            }
            EqBandKind::LowShelf | EqBandKind::HighShelf => {
                // Shelf slope form: alpha from S, the cookbook's
                // "shelf slope" parameterization.
                //
                // The radicand `(A + 1/A)(1/S - 1) + 2` goes negative once
                // `A + 1/A > 2S/(S - 1)` — at slope 10 that is a shelf gain of
                // only ±8.1 dB, at slope 5 it is ±12, so an ordinary steep bass
                // boost reaches it. Clamping the radicand at zero avoids the
                // NaN but produces `alpha == 0`, and then a0 and a2 are
                // literally the same expression: after normalisation a2 is
                // exactly 1.0, both poles sit on the unit circle, and the band
                // becomes an undamped resonator. It rings on after the signal
                // stops — measured at 77 Hz for a 100 Hz corner, at an
                // amplitude above full scale, until the setting is changed.
                //
                // So clamp S instead, to the largest slope this gain can
                // actually support, and leave the radicand alone.
                let s = shelf_slope_limit(q, a);
                let alpha = sin_w0 / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                let (ap1, am1) = (a + 1.0, a - 1.0);
                if kind == EqBandKind::LowShelf {
                    (
                        a * (ap1 - am1 * cos_w0 + two_sqrt_a_alpha),
                        2.0 * a * (am1 - ap1 * cos_w0),
                        a * (ap1 - am1 * cos_w0 - two_sqrt_a_alpha),
                        ap1 + am1 * cos_w0 + two_sqrt_a_alpha,
                        -2.0 * (am1 + ap1 * cos_w0),
                        ap1 + am1 * cos_w0 - two_sqrt_a_alpha,
                    )
                } else {
                    (
                        a * (ap1 + am1 * cos_w0 + two_sqrt_a_alpha),
                        -2.0 * a * (am1 + ap1 * cos_w0),
                        a * (ap1 + am1 * cos_w0 - two_sqrt_a_alpha),
                        ap1 - am1 * cos_w0 + two_sqrt_a_alpha,
                        2.0 * (am1 - ap1 * cos_w0),
                        ap1 - am1 * cos_w0 - two_sqrt_a_alpha,
                    )
                }
            }
            EqBandKind::LowPass => {
                let alpha = sin_w0 / (2.0 * q);
                let b1 = 1.0 - cos_w0;
                (
                    b1 / 2.0,
                    b1,
                    b1 / 2.0,
                    1.0 + alpha,
                    -2.0 * cos_w0,
                    1.0 - alpha,
                )
            }
            EqBandKind::HighPass => {
                let alpha = sin_w0 / (2.0 * q);
                let b1 = 1.0 + cos_w0;
                (
                    b1 / 2.0,
                    -b1,
                    b1 / 2.0,
                    1.0 + alpha,
                    -2.0 * cos_w0,
                    1.0 - alpha,
                )
            }
        };

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
}

/// Per-channel filter memory (transposed direct form II: two states, good
/// numerical behavior at f32).
#[derive(Debug, Default, Clone, Copy)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, x: f32, c: &BiquadCoeffs) -> f32 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}

const KIND_PEAKING: u8 = 0;
const KIND_LOW_SHELF: u8 = 1;
const KIND_HIGH_SHELF: u8 = 2;
const KIND_LOW_PASS: u8 = 3;
const KIND_HIGH_PASS: u8 = 4;

fn kind_to_u8(kind: EqBandKind) -> u8 {
    match kind {
        EqBandKind::Peaking => KIND_PEAKING,
        EqBandKind::LowShelf => KIND_LOW_SHELF,
        EqBandKind::HighShelf => KIND_HIGH_SHELF,
        EqBandKind::LowPass => KIND_LOW_PASS,
        EqBandKind::HighPass => KIND_HIGH_PASS,
    }
}

fn kind_from_u8(v: u8) -> EqBandKind {
    match v {
        KIND_LOW_SHELF => EqBandKind::LowShelf,
        KIND_HIGH_SHELF => EqBandKind::HighShelf,
        KIND_LOW_PASS => EqBandKind::LowPass,
        KIND_HIGH_PASS => EqBandKind::HighPass,
        _ => EqBandKind::Peaking,
    }
}

struct AtomicBand {
    kind: AtomicU8,
    freq_bits: AtomicU32,
    gain_bits: AtomicU32,
    q_bits: AtomicU32,
}

impl AtomicBand {
    fn flat() -> Self {
        Self {
            kind: AtomicU8::new(KIND_PEAKING),
            freq_bits: AtomicU32::new(1000.0f32.to_bits()),
            gain_bits: AtomicU32::new(0.0f32.to_bits()),
            q_bits: AtomicU32::new(1.0f32.to_bits()),
        }
    }

    fn store(&self, band: &EqBand) {
        self.kind.store(kind_to_u8(band.kind), Ordering::Relaxed);
        self.freq_bits
            .store(band.freq_hz.to_bits(), Ordering::Relaxed);
        self.gain_bits
            .store(band.gain_db.to_bits(), Ordering::Relaxed);
        self.q_bits.store(band.q.to_bits(), Ordering::Relaxed);
    }

    fn load(&self) -> EqBand {
        EqBand {
            kind: kind_from_u8(self.kind.load(Ordering::Relaxed)),
            freq_hz: f32::from_bits(self.freq_bits.load(Ordering::Relaxed)),
            gain_db: f32::from_bits(self.gain_bits.load(Ordering::Relaxed)),
            q: f32::from_bits(self.q_bits.load(Ordering::Relaxed)),
        }
    }
}

/// Low bits of `published` carry the band count (MAX_EQ_BANDS fits in 8),
/// the rest is the generation counter.
const COUNT_BITS: u32 = 8;
const COUNT_MASK: u64 = (1 << COUNT_BITS) - 1;

/// Live-tunable EQ parameters shared with the RT capture callback.
///
/// Single writer (the loop thread handling commands), single reader (the RT
/// callback). The individual fields are written Relaxed and published by one
/// Release store of `published`, which carries the band count with the
/// generation - so a reader that sees a new generation always sees the
/// matching count, never a stale one, and never walks a band slot the new
/// config does not define.
///
/// What this does *not* guarantee: each band is its own atomic, so two
/// apply() calls racing one buffer can leave the reader with fields from
/// both. That costs at most one buffer of wrong curve - the next generation
/// converges - and it buys a reader with no locks and no retries.
pub struct EqParams {
    enabled: AtomicBool,
    preamp_bits: AtomicU32,
    bands: [AtomicBand; MAX_EQ_BANDS],
    published: AtomicU64,
}

impl EqParams {
    pub fn from_config(config: &EqConfig) -> Self {
        let p = Self {
            enabled: AtomicBool::new(false),
            preamp_bits: AtomicU32::new(0.0f32.to_bits()),
            bands: std::array::from_fn(|_| AtomicBand::flat()),
            published: AtomicU64::new(0),
        };
        p.apply(config);
        p
    }

    /// Publish a new config to the RT reader (command thread only).
    pub fn apply(&self, config: &EqConfig) {
        let count = config.bands.len().min(MAX_EQ_BANDS) as u64;
        for (slot, band) in self.bands.iter().zip(config.bands.iter()) {
            slot.store(band);
        }
        self.enabled.store(config.enabled, Ordering::Relaxed);
        self.preamp_bits
            .store(config.preamp_db.to_bits(), Ordering::Relaxed);
        // Single Release store publishes count *and* generation together.
        let generation = (self.published.load(Ordering::Relaxed) >> COUNT_BITS).wrapping_add(1);
        self.published
            .store((generation << COUNT_BITS) | count, Ordering::Release);
    }

    fn published(&self) -> u64 {
        self.published.load(Ordering::Acquire)
    }
}

/// The RT-side processor: owns coefficient + filter state, refreshed from
/// `EqParams` when the generation changes or the sample rate renegotiates.
pub struct EqEngine {
    sample_rate: f32,
    /// Last `EqParams::published` word acted on. u64::MAX = "must refresh"
    /// sentinel (set on rate change / creation); apply() never produces it,
    /// its low bits would mean a band count of 255.
    seen_published: u64,
    enabled: bool,
    preamp_linear: f32,
    coeffs: [BiquadCoeffs; MAX_EQ_BANDS],
    count: usize,
    /// Per band, per stereo channel. A rate change resets filter memory -
    /// accepted, same as the mic chain rebuilding DspChain on rate change.
    state: [[BiquadState; 2]; MAX_EQ_BANDS],
}

impl EqEngine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            seen_published: u64::MAX,
            enabled: false,
            preamp_linear: 1.0,
            coeffs: [BiquadCoeffs::identity(); MAX_EQ_BANDS],
            count: 0,
            state: [[BiquadState::default(); 2]; MAX_EQ_BANDS],
        }
    }

    /// Coefficients are frequency-relative, so a renegotiated rate forces a
    /// redesign on the next process() even if the params are unchanged.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate > 0.0 && sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            self.seen_published = u64::MAX;
            self.state = [[BiquadState::default(); 2]; MAX_EQ_BANDS];
        }
    }

    fn refresh(&mut self, params: &EqParams) {
        let published = params.published();
        if published == self.seen_published {
            return;
        }
        self.seen_published = published;
        self.enabled = params.enabled.load(Ordering::Relaxed);
        // Same defence as the band gains: an unclamped preamp turns into an
        // inf multiplier and NaNs the whole cascade.
        let preamp_db = f32::from_bits(params.preamp_bits.load(Ordering::Relaxed));
        let preamp_db = finite_or(preamp_db, 0.0).clamp(-MAX_DSP_GAIN_DB, MAX_DSP_GAIN_DB);
        self.preamp_linear = 10.0f32.powf(preamp_db / 20.0);
        self.count = ((published & COUNT_MASK) as usize).min(MAX_EQ_BANDS);
        for i in 0..self.count {
            let band = params.bands[i].load();
            self.coeffs[i] =
                BiquadCoeffs::design(band.kind, band.freq_hz, band.gain_db, band.q, self.sample_rate);
        }
    }

    /// Process an interleaved stereo buffer in place. Pass-through when the
    /// config is disabled (the chain is normally torn down on disable; this
    /// covers the window between a disable apply() and the relink).
    pub fn process_interleaved(&mut self, buf: &mut [f32], params: &EqParams) {
        self.refresh(params);
        if !self.enabled {
            return;
        }
        for frame in buf.chunks_exact_mut(2) {
            for (ch, sample) in frame.iter_mut().enumerate() {
                let mut x = *sample * self.preamp_linear;
                for i in 0..self.count {
                    x = self.state[i][ch].process(x, &self.coeffs[i]);
                }
                *sample = x;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pole radius of a normalised biquad (a0 == 1).
    ///
    /// The stability question is entirely in the denominator: with
    /// `z^2 + a1 z + a2`, the product of the two pole radii is `|a2|`, so
    /// `|a2| >= 1` is a filter that never stops ringing.
    fn pole_radius(c: &BiquadCoeffs) -> f64 {
        f64::from(c.a2).abs().sqrt()
    }

    #[test]
    fn no_band_setting_the_ui_permits_puts_a_pole_on_the_unit_circle() {
        // A shelf with a steep slope used to land exactly on the unit circle:
        // the cookbook's radicand goes negative, clamping it to zero makes
        // alpha zero, and then a0 and a2 are the same expression. The result
        // is an undamped resonator that keeps sounding after the audio stops,
        // at an amplitude above full scale. Slope 10 reached it at ±8.1 dB —
        // an ordinary bass boost, not an extreme.
        //
        // The bar here is stability itself, `|pole| < 1`, not some comfortable
        // distance from it: a narrow peaking band genuinely sits very close to
        // the circle (Q 10 at 20 Hz on 192 kHz reaches 0.99999), and that is
        // what such a filter is, not a defect. What must never happen is
        // reaching or passing it.
        let mut worst = (0.0f64, String::new());
        for kind in [
            EqBandKind::Peaking,
            EqBandKind::LowShelf,
            EqBandKind::HighShelf,
            EqBandKind::LowPass,
            EqBandKind::HighPass,
        ] {
            for gain_i in -24..=24 {
                for q_i in 1..=100 {
                    let (gain_db, q) = (gain_i as f32, q_i as f32 / 10.0);
                    for &rate in &[44_100.0, 48_000.0, 96_000.0, 192_000.0] {
                        for &freq in &[20.0, 100.0, 1_000.0, 10_000.0, 19_000.0] {
                            let c = BiquadCoeffs::design(kind, freq, gain_db, q, rate);
                            let r = pole_radius(&c);
                            assert!(
                                r.is_finite(),
                                "{kind:?} {freq} Hz {gain_db} dB Q{q} @{rate}: non-finite"
                            );
                            if r > worst.0 {
                                worst = (r, format!("{kind:?} {freq} Hz {gain_db} dB Q{q} @{rate}"));
                            }
                        }
                    }
                }
            }
        }
        assert!(
            worst.0 < 1.0,
            "pole radius {} at {} — this band never stops ringing",
            worst.0,
            worst.1
        );
    }

    #[test]
    fn a_shelf_goes_quiet_after_the_audio_stops() {
        // The user-visible half of the same bug, on the settings that were
        // measured to ring: a second of signal, then silence, and the output
        // has to actually fall away.
        for &(gain_db, slope) in &[
            (24.0f32, 2.0f32),
            (9.0, 10.0),
            (12.0, 10.0),
            (-12.0, 10.0),
            (24.0, 10.0),
            (8.1, 10.0),
        ] {
            for kind in [EqBandKind::LowShelf, EqBandKind::HighShelf] {
                let c = BiquadCoeffs::design(kind, 100.0, gain_db, slope, 48_000.0);
                let mut st = BiquadState::default();
                // A second of full-scale tone at the corner frequency.
                for n in 0..48_000 {
                    let t = n as f32 / 48_000.0;
                    st.process((2.0 * std::f32::consts::PI * 100.0 * t).sin(), &c);
                }
                // Then five seconds of nothing.
                // Peak over the last second, so a slow decay still fails.
                let mut tail = 0.0f32;
                for n in 0..240_000 {
                    let y = st.process(0.0, &c).abs();
                    if n >= 192_000 {
                        tail = tail.max(y);
                    }
                }
                assert!(
                    tail < 1e-3,
                    "{kind:?} {gain_db} dB slope {slope}: still at {tail} after five seconds of silence"
                );
            }
        }
    }

    #[test]
    fn a_shelf_slope_is_capped_by_its_gain_not_by_clamping_the_radicand() {
        // Below unity gain every slope is representable...
        assert_eq!(shelf_slope_limit(10.0, 1.0), 10.0);
        // ...and above it the cap tightens as the gain grows.
        let a24 = 10.0f32.powf(24.0 / 40.0);
        let a6 = 10.0f32.powf(6.0 / 40.0);
        assert!(shelf_slope_limit(10.0, a24) < shelf_slope_limit(10.0, a6));
        // A slope the gain does support is left alone.
        assert_eq!(shelf_slope_limit(0.7, a24), 0.7);
    }

    /// Analytic magnitude response |H(e^jw)| in dB - exact, no time-domain
    /// sampling artifacts. This is the same formula the UI curve uses
    /// (src/lib/eqMath.ts), so these tests also pin the shared math.
    fn measured_gain_db(c: &BiquadCoeffs, freq: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * std::f64::consts::PI * f64::from(freq) / f64::from(sample_rate);
        let (b0, b1, b2) = (f64::from(c.b0), f64::from(c.b1), f64::from(c.b2));
        let (a1, a2) = (f64::from(c.a1), f64::from(c.a2));
        let num_re = b0 + b1 * w.cos() + b2 * (2.0 * w).cos();
        let num_im = -(b1 * w.sin() + b2 * (2.0 * w).sin());
        let den_re = 1.0 + a1 * w.cos() + a2 * (2.0 * w).cos();
        let den_im = -(a1 * w.sin() + a2 * (2.0 * w).sin());
        let mag = ((num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im))
            .sqrt();
        (20.0 * mag.log10()) as f32
    }

    fn config(bands: Vec<EqBand>, preamp_db: f32) -> EqConfig {
        EqConfig {
            enabled: true,
            preamp_db,
            bands,
        }
    }

    fn band(kind: EqBandKind, freq_hz: f32, gain_db: f32, q: f32) -> EqBand {
        EqBand {
            kind,
            freq_hz,
            gain_db,
            q,
        }
    }

    const SR: f32 = 48000.0;

    #[test]
    fn identity_passes_signal_unchanged() {
        let c = BiquadCoeffs::identity();
        let mut state = BiquadState::default();
        for x in [0.0f32, 1.0, -0.5, 0.25] {
            assert_eq!(state.process(x, &c), x);
        }
    }

    #[test]
    fn peaking_matches_gain_at_center_freq() {
        let c = BiquadCoeffs::design(EqBandKind::Peaking, 1000.0, 6.0, 1.0, SR);
        let g = measured_gain_db(&c, 1000.0, SR);
        assert!((g - 6.0).abs() < 0.3, "expected ~+6 dB at center, got {g}");
    }

    #[test]
    fn peaking_is_flat_far_from_center() {
        let c = BiquadCoeffs::design(EqBandKind::Peaking, 1000.0, 12.0, 2.0, SR);
        let g = measured_gain_db(&c, 60.0, SR);
        assert!(g.abs() < 0.5, "expected ~0 dB two+ octaves away, got {g}");
    }

    #[test]
    fn low_shelf_boosts_below_corner_flat_above() {
        let c = BiquadCoeffs::design(EqBandKind::LowShelf, 200.0, 6.0, 0.71, SR);
        let low = measured_gain_db(&c, 40.0, SR);
        let high = measured_gain_db(&c, 4000.0, SR);
        assert!((low - 6.0).abs() < 0.5, "low end should be ~+6 dB, got {low}");
        assert!(high.abs() < 0.5, "high end should be flat, got {high}");
    }

    #[test]
    fn high_shelf_boosts_above_corner_flat_below() {
        let c = BiquadCoeffs::design(EqBandKind::HighShelf, 5000.0, -6.0, 0.71, SR);
        let low = measured_gain_db(&c, 200.0, SR);
        let high = measured_gain_db(&c, 15000.0, SR);
        assert!(low.abs() < 0.5, "low end should be flat, got {low}");
        assert!((high + 6.0).abs() < 0.6, "high end should be ~-6 dB, got {high}");
    }

    #[test]
    fn low_pass_attenuates_above_cutoff() {
        let c = BiquadCoeffs::design(EqBandKind::LowPass, 1000.0, 0.0, 0.71, SR);
        let pass = measured_gain_db(&c, 100.0, SR);
        let stop = measured_gain_db(&c, 8000.0, SR);
        assert!(pass.abs() < 0.5, "passband should be flat, got {pass}");
        assert!(stop < -30.0, "stopband should be strongly attenuated, got {stop}");
    }

    #[test]
    fn high_pass_attenuates_below_cutoff() {
        let c = BiquadCoeffs::design(EqBandKind::HighPass, 1000.0, 0.0, 0.71, SR);
        let stop = measured_gain_db(&c, 100.0, SR);
        let pass = measured_gain_db(&c, 8000.0, SR);
        assert!(stop < -30.0, "stopband should be strongly attenuated, got {stop}");
        assert!(pass.abs() < 0.5, "passband should be flat, got {pass}");
    }

    #[test]
    fn params_apply_then_engine_refresh_picks_up_bands() {
        let params = EqParams::from_config(&config(
            vec![band(EqBandKind::Peaking, 2000.0, 4.0, 1.5)],
            -3.0,
        ));
        let mut engine = EqEngine::new(SR);
        engine.refresh(&params);
        assert!(engine.enabled);
        assert_eq!(engine.count, 1);
        assert!((engine.preamp_linear - 10.0f32.powf(-3.0 / 20.0)).abs() < 1e-6);
        let expected = BiquadCoeffs::design(EqBandKind::Peaking, 2000.0, 4.0, 1.5, SR);
        assert_eq!(engine.coeffs[0], expected);

        // A second apply with different bands is picked up on next refresh.
        params.apply(&config(vec![band(EqBandKind::HighPass, 80.0, 0.0, 0.71)], 0.0));
        engine.refresh(&params);
        assert_eq!(engine.count, 1);
        let expected = BiquadCoeffs::design(EqBandKind::HighPass, 80.0, 0.0, 0.71, SR);
        assert_eq!(engine.coeffs[0], expected);
    }

    #[test]
    fn set_sample_rate_forces_recompute_at_new_rate() {
        let params = EqParams::from_config(&config(
            vec![band(EqBandKind::Peaking, 2000.0, 4.0, 1.5)],
            0.0,
        ));
        let mut engine = EqEngine::new(48000.0);
        engine.refresh(&params);
        let at_48k = engine.coeffs[0];
        engine.set_sample_rate(96000.0);
        engine.refresh(&params);
        assert_ne!(engine.coeffs[0], at_48k);
        assert_eq!(
            engine.coeffs[0],
            BiquadCoeffs::design(EqBandKind::Peaking, 2000.0, 4.0, 1.5, 96000.0)
        );
    }

    #[test]
    fn preamp_is_linear_multiply_before_cascade() {
        let params = EqParams::from_config(&config(vec![], 6.0));
        let mut engine = EqEngine::new(SR);
        let mut buf = vec![0.5f32, -0.5, 0.25, -0.25];
        engine.process_interleaved(&mut buf, &params);
        let expected = 10.0f32.powf(6.0 / 20.0);
        assert!((buf[0] - 0.5 * expected).abs() < 1e-6);
        assert!((buf[1] + 0.5 * expected).abs() < 1e-6);
    }

    #[test]
    fn disabled_config_passes_through_untouched() {
        let mut cfg = config(vec![band(EqBandKind::Peaking, 1000.0, 12.0, 1.0)], 12.0);
        cfg.enabled = false;
        let params = EqParams::from_config(&cfg);
        let mut engine = EqEngine::new(SR);
        let original = vec![0.5f32, -0.5, 0.25, -0.25];
        let mut buf = original.clone();
        engine.process_interleaved(&mut buf, &params);
        assert_eq!(buf, original);
    }

    #[test]
    fn cascade_of_ten_flat_bands_is_still_flat() {
        let bands: Vec<EqBand> = (0..MAX_EQ_BANDS)
            .map(|i| band(EqBandKind::Peaking, 100.0 * (i as f32 + 1.0) * 2.0, 0.0, 1.0))
            .collect();
        let params = EqParams::from_config(&config(bands, 0.0));
        let mut engine = EqEngine::new(SR);
        // A 1 kHz sine through ten 0 dB bands should come out at unity.
        let mut buf: Vec<f32> = (0..2000)
            .flat_map(|n| {
                let s = (2.0 * std::f32::consts::PI * 1000.0 * n as f32 / SR).sin() * 0.5;
                [s, s]
            })
            .collect();
        let peak_in = 0.5f32;
        engine.process_interleaved(&mut buf, &params);
        let peak_out = buf[2000..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            (peak_out - peak_in).abs() < 0.01,
            "flat cascade drifted: in {peak_in}, out {peak_out}"
        );
    }

    #[test]
    fn non_finite_band_input_yields_finite_coefficients() {
        let c = BiquadCoeffs::design(EqBandKind::Peaking, f32::NAN, f32::INFINITY, f32::NAN, SR);
        for v in [c.b0, c.b1, c.b2, c.a1, c.a2] {
            assert!(v.is_finite(), "design leaked a non-finite coefficient: {v}");
        }
    }

    #[test]
    fn absurd_preamp_never_reaches_the_cascade() {
        let params = EqParams::from_config(&config(
            vec![band(EqBandKind::Peaking, 1000.0, f32::INFINITY, 1.0)],
            f32::INFINITY,
        ));
        let mut engine = EqEngine::new(SR);
        let mut buf = vec![0.5f32, -0.5, 0.25, -0.25];
        engine.process_interleaved(&mut buf, &params);
        assert!(buf.iter().all(|s| s.is_finite()), "NaN/inf leaked: {buf:?}");
    }

    #[test]
    fn published_word_pairs_count_with_generation() {
        let params = EqParams::from_config(&config(vec![], 0.0));
        let first = params.published();
        params.apply(&config(
            vec![
                band(EqBandKind::Peaking, 1000.0, 1.0, 1.0),
                band(EqBandKind::Peaking, 2000.0, 1.0, 1.0),
            ],
            0.0,
        ));
        let second = params.published();
        assert_eq!(first & COUNT_MASK, 0);
        assert_eq!(second & COUNT_MASK, 2);
        assert_eq!(second >> COUNT_BITS, (first >> COUNT_BITS) + 1);
    }

    #[test]
    fn stereo_channels_are_processed_independently() {
        // A DC-blocking high-pass fed L=DC, R=silence must not bleed.
        let params = EqParams::from_config(&config(
            vec![band(EqBandKind::HighPass, 500.0, 0.0, 0.71)],
            0.0,
        ));
        let mut engine = EqEngine::new(SR);
        let mut buf: Vec<f32> = (0..2000).flat_map(|_| [1.0f32, 0.0]).collect();
        engine.process_interleaved(&mut buf, &params);
        let right_energy: f32 = buf.iter().skip(1).step_by(2).map(|s| s.abs()).sum();
        assert_eq!(right_energy, 0.0, "silent right channel picked up energy");
        let left_tail = buf[3000..].iter().step_by(2).fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(left_tail < 0.01, "DC should be blocked on the left, got {left_tail}");
    }
}
