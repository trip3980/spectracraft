//! Persistent per-string modal vibration state and optional body resonator
//! for the Stage 1 dry-instrument renderer.
//!
//! Stage 1 invariants:
//! - No original PCM samples or spectrogram frames ever enter this module.
//! - Vibration state `(c, phase)` persists across bends and additive re-picks.
//! - Bends only change `f_hz`; re-picks add phase-aligned excitation without reset.

use std::f64::consts::{PI, TAU};

/// Number of modal partials tracked per physical string (modes 1..=19).
pub const MODE_COUNT: usize = 19;

/// Precomputed per-note modal parameters for one physical string.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StringModeParams {
    pub weights: [f64; MODE_COUNT],
    pub decays: [f64; MODE_COUNT],
    pub sample_losses: [f64; MODE_COUNT],
    pub pickups: [f64; MODE_COUNT],
}

impl StringModeParams {
    pub fn new(
        weights: [f64; MODE_COUNT],
        decays: [f64; MODE_COUNT],
        pickups: [f64; MODE_COUNT],
        sample_rate: f64,
    ) -> Self {
        Self {
            weights,
            decays,
            sample_losses: per_sample_losses(&decays, sample_rate),
            pickups,
        }
    }
}

/// Precompute per-sample exponential decay multipliers `exp(-decay_k / sample_rate)`.
pub fn per_sample_losses(decays: &[f64; MODE_COUNT], sample_rate: f64) -> [f64; MODE_COUNT] {
    let inv_rate = 1.0 / sample_rate;
    let mut losses = [0.0_f64; MODE_COUNT];
    for idx in 0..MODE_COUNT {
        losses[idx] = (-decays[idx] * inv_rate).exp();
    }
    losses
}

/// Persistent vibration state for one physical string.
#[derive(Clone, Debug, PartialEq)]
pub struct StringState {
    /// Complex amplitude `(real, imag)` for modes `1..=19` (index `k - 1`).
    pub c: [(f64, f64); MODE_COUNT],
    /// Continuous fundamental phase in radians.
    pub phase: f64,
}

impl Default for StringState {
    fn default() -> Self {
        Self::new()
    }
}

impl StringState {
    pub fn new() -> Self {
        Self {
            c: [(0.0, 0.0); MODE_COUNT],
            phase: 0.0,
        }
    }

    /// Additive re-pick: injects stroke excitation into `c` without resetting `c` or `phase`.
    ///
    /// Silent modes seed along `(cos(phase), sin(phase))`; already-vibrating modes
    /// add constructively along their current envelope direction so a re-pick never
    /// cancels existing energy when `phase` is near `pi`.
    pub fn repick(&mut self, strength: f64, excitation_weights: &[f64; MODE_COUNT]) {
        let (sin_p, cos_p) = self.phase.sin_cos();
        for (idx, &weight_k) in excitation_weights.iter().enumerate() {
            let amp = strength * weight_k;
            let (re, im) = self.c[idx];
            let mag = re.hypot(im);
            if mag > 1e-12 {
                let scale = 1.0 + amp.abs() / mag;
                self.c[idx] = (re * scale, im * scale);
            } else {
                self.c[idx] = (re + amp * cos_p, im + amp * sin_p);
            }
        }
    }

    /// Advance one sample using precomputed per-sample decay multipliers.
    ///
    /// Uses a single fundamental `sin_cos()` plus angle-addition recurrence across
    /// harmonics `1..=19` to avoid 19 per-sample `exp()` and 19 `sin_cos()` calls.
    #[inline]
    pub fn tick_cached(
        &mut self,
        f_hz: f64,
        sample_rate: f64,
        sample_losses: &[f64; MODE_COUNT],
        pickup_weights: &[f64; MODE_COUNT],
    ) -> f64 {
        self.phase = (self.phase + TAU * f_hz / sample_rate).rem_euclid(TAU);
        let (sin_1, cos_1) = self.phase.sin_cos();
        let nyquist = sample_rate * 0.5;
        let max_harmonic = ((nyquist / f_hz.max(1e-9)).ceil() as usize)
            .saturating_sub(1)
            .min(MODE_COUNT);

        let mut sample = 0.0_f64;
        let mut cos_k = cos_1;
        let mut sin_k = sin_1;
        for idx in 0..max_harmonic {
            let loss = sample_losses[idx];
            let re = self.c[idx].0 * loss;
            let im = self.c[idx].1 * loss;
            self.c[idx] = (re, im);
            sample += (re * cos_k + im * sin_k) * pickup_weights[idx];
            let next_cos = cos_k * cos_1 - sin_k * sin_1;
            let next_sin = sin_k * cos_1 + cos_k * sin_1;
            cos_k = next_cos;
            sin_k = next_sin;
        }
        for idx in max_harmonic..MODE_COUNT {
            let loss = sample_losses[idx];
            self.c[idx].0 *= loss;
            self.c[idx].1 *= loss;
        }
        sample
    }

    /// Advance one sample of continuous free ring or pitch bend from decay rates in `1/s`.
    pub fn tick(
        &mut self,
        f_hz: f64,
        sample_rate: f64,
        decays: &[f64; MODE_COUNT],
        pickup_weights: &[f64; MODE_COUNT],
    ) -> f64 {
        let losses = per_sample_losses(decays, sample_rate);
        self.tick_cached(f_hz, sample_rate, &losses, pickup_weights)
    }

    /// Advance un-fretted ring decay across `samples` silent ticks in one analytic step.
    pub fn decay_only(
        &mut self,
        samples: usize,
        f_hz: f64,
        sample_rate: f64,
        decays: &[f64; MODE_COUNT],
    ) {
        if samples == 0 {
            return;
        }
        let dt = samples as f64 / sample_rate;
        self.phase = (self.phase + TAU * f_hz * dt).rem_euclid(TAU);
        for idx in 0..MODE_COUNT {
            let loss = (-decays[idx] * dt).exp();
            self.c[idx].0 *= loss;
            self.c[idx].1 *= loss;
        }
    }

    /// Total modal energy `sum_k |c[k]|^2`.
    pub fn modal_energy(&self) -> f64 {
        self.c.iter().map(|&(re, im)| re * re + im * im).sum()
    }
}

/// Default Stage 1 modal parameters for modes `1..=19`:
/// - `excitation_weights[k - 1] = sin(pi * k * pick) * sinc(pi * 0.029 * k) / k^0.91`
/// - `decays[k - 1] = 0.71 + 0.39 * k^0.84`
/// - `pickup_weights[k - 1] = sin(pi * k * pickup)`
pub fn default_string_mode_params(
    median_hz: f64,
) -> ([f64; MODE_COUNT], [f64; MODE_COUNT], [f64; MODE_COUNT]) {
    let fret = (median_hz / 330.0).clamp(1.0, 2.7);
    let pick = (0.175 * fret).clamp(0.16, 0.36);
    let pickup = (0.145 * fret).clamp(0.11, 0.275);
    let mut weights = [0.0_f64; MODE_COUNT];
    let mut decays = [0.0_f64; MODE_COUNT];
    let mut pickups = [0.0_f64; MODE_COUNT];
    for idx in 0..MODE_COUNT {
        let k = (idx + 1) as f64;
        let sinc_arg = PI * 0.029 * k;
        let sinc_term = sinc_arg.sin() / sinc_arg;
        weights[idx] = (PI * k * pick).sin() * sinc_term / k.powf(0.91);
        decays[idx] = 0.71 + 0.39 * k.powf(0.84);
        pickups[idx] = (PI * k * pickup).sin();
    }
    (weights, decays, pickups)
}

/// Parallel two-biquad body resonance configuration (Stage 1 optional coloration).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyResonanceConfig {
    pub air_hz: f64,
    pub air_q: f64,
    pub top_hz: f64,
    pub top_q: f64,
    pub dry_mix: f64,
    pub air_mix: f64,
    pub top_mix: f64,
}

impl Default for BodyResonanceConfig {
    fn default() -> Self {
        Self {
            air_hz: 100.0,
            air_q: 3.5,
            top_hz: 200.0,
            top_q: 4.0,
            dry_mix: 0.70,
            air_mix: 0.20,
            top_mix: 0.10,
        }
    }
}

/// Constant-0 dB-peak bandpass biquad (Audio EQ Cookbook, Direct Form II Transposed).
/// Exploits `b1 == 0` and `b2 == -b0` for minimal per-sample arithmetic.
#[derive(Clone, Debug)]
struct BandpassBiquad {
    b0: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl BandpassBiquad {
    fn new(freq_hz: f64, q: f64, sample_rate: f64) -> Self {
        let f = freq_hz.clamp(20.0, sample_rate * 0.45);
        let q_clamped = q.clamp(0.5, 24.0);
        let (sin_w0, cos_w0) = (TAU * f / sample_rate).sin_cos();
        let alpha = sin_w0 / (2.0 * q_clamped);
        let inv_a0 = 1.0 / (1.0 + alpha);
        Self {
            b0: alpha * inv_a0,
            a1: (-2.0 * cos_w0) * inv_a0,
            a2: (1.0 - alpha) * inv_a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let bx = self.b0 * x;
        let y = bx + self.z1;
        let z1 = -self.a1 * y + self.z2;
        let z2 = -bx - self.a2 * y;
        self.z1 = if z1.abs() < 1e-30 { 0.0 } else { z1 };
        self.z2 = if z2.abs() < 1e-30 { 0.0 } else { z2 };
        if y.abs() < 1e-30 {
            0.0
        } else {
            y
        }
    }
}

/// Parallel air-cavity (100 Hz) + top-plate (200 Hz) dry body resonator.
#[derive(Clone, Debug)]
pub struct BodyResonator {
    air: BandpassBiquad,
    top: BandpassBiquad,
    config: BodyResonanceConfig,
}

impl BodyResonator {
    pub fn new(sample_rate: f64, config: BodyResonanceConfig) -> Self {
        Self {
            air: BandpassBiquad::new(config.air_hz, config.air_q, sample_rate),
            top: BandpassBiquad::new(config.top_hz, config.top_q, sample_rate),
            config,
        }
    }

    #[inline]
    pub fn process_sample(&mut self, dry: f64) -> f64 {
        let air_out = self.air.process(dry);
        let top_out = self.top.process(dry);
        self.config.dry_mix * dry + self.config.air_mix * air_out + self.config.top_mix * top_out
    }

    pub fn process_buffer(&mut self, samples: &mut [f64]) {
        for s in samples.iter_mut() {
            *s = self.process_sample(*s);
        }
    }
}

/// Single-mode damped oscillator for non-guitar stiff-string sources
/// with cached Fletcher dispersion ratio `freq_ratio`.
#[derive(Clone)]
pub(crate) struct Mode {
    freq_ratio: f64,
    loss: f64,
    coupling: f64,
    re: f64,
    im: f64,
}

impl Mode {
    #[cfg(test)]
    pub(crate) fn new(harmonic: f64, coupling: f64, decay: f64, rate: f64) -> Self {
        Self::new_with_inharmonicity(harmonic, coupling, decay, 0.0, rate)
    }

    pub(crate) fn new_with_inharmonicity(
        harmonic: f64,
        coupling: f64,
        decay: f64,
        inharmonicity_b: f64,
        rate: f64,
    ) -> Self {
        Self {
            freq_ratio: crate::acoustics_physics::stiff_string_ratio(
                harmonic,
                inharmonicity_b.max(0.0),
            ),
            loss: (-decay / rate).exp(),
            coupling,
            re: 0.,
            im: 0.,
        }
    }

    #[inline]
    pub(crate) fn advance(&mut self, f: f64, rate: f64, kick: f64) -> f64 {
        let hz = self.freq_ratio * f;
        if hz >= rate * 0.5 {
            self.re = 0.;
            self.im = 0.;
            return 0.;
        }
        let (s, c) = (TAU * hz / rate).sin_cos();
        let (re, im) = (self.re, self.im);
        self.re = self.loss * (c * re - s * im) + self.coupling * kick;
        self.im = self.loss * (s * re + c * im);
        self.im
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_state_pure_bend_preserves_continuity_and_additive_repick_adds_energy_without_reset() {
        let rate = 48_000.0;
        let (weights, decays, pickups) = default_string_mode_params(220.0);
        let params = StringModeParams::new(weights, decays, pickups, rate);

        let mut bend_only = StringState::new();
        assert_eq!(bend_only.phase, 0.0);
        assert_eq!(bend_only.modal_energy(), 0.0);
        bend_only.repick(1.0, &params.weights);

        let mut bend_repick = bend_only.clone();
        let n_samples = 24_000;
        let repick_at = 12_000;

        let mut y_bend = Vec::with_capacity(n_samples);
        let mut y_repick = Vec::with_capacity(n_samples);

        for i in 0..n_samples {
            let u = i as f64 / (n_samples - 1) as f64;
            let f_hz = 220.0 * 2.0_f64.powf(3.0 * u / 12.0);

            let s_b = bend_only.tick_cached(f_hz, rate, &params.sample_losses, &params.pickups);
            let s_r = bend_repick.tick_cached(f_hz, rate, &params.sample_losses, &params.pickups);
            y_bend.push(s_b);
            y_repick.push(s_r);

            if i == repick_at {
                let phase_before = bend_repick.phase;
                let mag0_before = bend_repick.c[0].0.hypot(bend_repick.c[0].1);
                bend_repick.repick(0.65, &params.weights);
                assert_eq!(bend_repick.phase, phase_before);
                let mag0_after = bend_repick.c[0].0.hypot(bend_repick.c[0].1);
                let expected_mag = mag0_before + (0.65 * params.weights[0]).abs();
                assert!((mag0_after - expected_mag).abs() < 1e-12);
            }
        }

        assert_eq!(y_bend[..=repick_at], y_repick[..=repick_at]);
        let max_step_bend = y_bend
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f64, f64::max);
        assert!(max_step_bend < 0.25, "pure bend had jump: {max_step_bend}");
        let e_bend: f64 = y_bend[repick_at + 1..repick_at + 4800]
            .iter()
            .map(|v| v * v)
            .sum();
        let e_repick: f64 = y_repick[repick_at + 1..repick_at + 4800]
            .iter()
            .map(|v| v * v)
            .sum();
        assert!(
            e_repick > e_bend * 1.15,
            "additive re-pick should inject energy (bend={e_bend}, repick={e_repick})"
        );
    }

    #[test]
    fn tick_cached_matches_direct_harmonic_trig_within_machine_precision() {
        let rate = 48_000.0;
        let (weights, decays, pickups) = default_string_mode_params(330.0);
        let losses = per_sample_losses(&decays, rate);

        let mut fast = StringState::new();
        fast.repick(1.0, &weights);
        let mut ref_state = fast.clone();

        let mut max_err = 0.0_f64;
        for i in 0..48_000 {
            let f_hz = 220.0 + 180.0 * (i as f64 / 48_000.0);
            let y_fast = fast.tick_cached(f_hz, rate, &losses, &pickups);

            // Direct per-harmonic trig reference
            ref_state.phase = (ref_state.phase + TAU * f_hz / rate).rem_euclid(TAU);
            let nyquist = rate * 0.5;
            let mut y_ref = 0.0_f64;
            for idx in 0..MODE_COUNT {
                let k = (idx + 1) as f64;
                let loss = (-decays[idx] / rate).exp();
                ref_state.c[idx].0 *= loss;
                ref_state.c[idx].1 *= loss;
                if k * f_hz < nyquist {
                    let (sin_kp, cos_kp) = (k * ref_state.phase).sin_cos();
                    y_ref +=
                        (ref_state.c[idx].0 * cos_kp + ref_state.c[idx].1 * sin_kp) * pickups[idx];
                }
            }
            max_err = max_err.max((y_fast - y_ref).abs());
        }
        assert!(
            max_err < 1e-13,
            "harmonic angle recurrence drifted too far from direct trig: {max_err:e}"
        );
    }

    #[test]
    fn body_resonator_accentuates_air_and_top_bands_and_preserves_silence() {
        let rate = 48_000.0;
        let cfg = BodyResonanceConfig::default();

        let mut silent_res = BodyResonator::new(rate, cfg);
        let mut zeros = vec![0.0_f64; 2048];
        silent_res.process_buffer(&mut zeros);
        assert!(zeros.iter().all(|&v| v == 0.0));

        let measure_rms = |freq_hz: f64| -> f64 {
            let mut res = BodyResonator::new(rate, cfg);
            let n = 48_000;
            let mut acc = 0.0;
            let start = 24_000;
            for i in 0..n {
                let x = (TAU * freq_hz * i as f64 / rate).sin();
                let y = res.process_sample(x);
                assert!(y.is_finite());
                if i >= start {
                    acc += y * y;
                }
            }
            (acc / (n - start) as f64).sqrt()
        };

        let rms_100 = measure_rms(100.0);
        let rms_200 = measure_rms(200.0);
        let rms_1000 = measure_rms(1000.0);
        assert!(
            rms_100 > rms_1000 * 1.15,
            "100 Hz air cavity should boost ({rms_100} vs {rms_1000})"
        );
        assert!(
            rms_200 > rms_1000 * 1.08,
            "200 Hz top plate should boost ({rms_200} vs {rms_1000})"
        );
    }

    #[test]
    fn bend_rotates_old_energy_and_repick_adds_without_reset() {
        let mut note = Mode::new(1., 0.8, 1.2, 48000.);
        note.advance(220., 48000., 1.);
        for i in 0..1200 {
            note.advance(220. + i as f64 / 40., 48000., 0.);
        }
        let energy = note.re.hypot(note.im);
        assert!((energy - 0.8 * (-1.2_f64 * 1200. / 48000.).exp()).abs() < 1e-10);
        let mut baseline = note.clone();
        let a = baseline.advance(250., 48000., 0.);
        let b = note.advance(250., 48000., 0.5);
        assert_eq!(a, b);
        assert!((note.re - baseline.re - 0.4).abs() < 1e-12);
        assert!((note.advance(250., 48000., 0.) - baseline.advance(250., 48000., 0.)).abs() > 0.01);
    }

    #[test]
    fn stiff_string_inharmonicity_stretches_upper_partial_phase() {
        let mut ideal = Mode::new(8., 0.5, 1.0, 48000.);
        let mut stiff = Mode::new_with_inharmonicity(8., 0.5, 1.0, 3.5e-4, 48000.);
        ideal.advance(220., 48000., 1.);
        stiff.advance(220., 48000., 1.);
        let mut diff = 0.0_f64;
        for _ in 0..480 {
            diff += (ideal.advance(220., 48000., 0.) - stiff.advance(220., 48000., 0.)).abs();
        }
        assert!(
            diff > 1.0,
            "stiff-string upper partial should diverge in phase from integer harmonic"
        );
    }
}
