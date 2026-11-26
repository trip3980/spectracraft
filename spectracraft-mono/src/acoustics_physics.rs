//! Peer-reviewed acoustic physics & DSP models for SpectraCraft.
//!
//! Pure Rust, deterministic, zero runtime network access, and zero external
//! training dependencies. Every equation preserves `SPECTRACRAFT_SPEC.md`
//! invariants: published reference parameters are never invented when missing,
//! and bypassed stages remain bit-identical pass-throughs.
//!
//! Implemented models:
//! - ISO 9613-1 atmospheric air absorption (`iso9613_air_absorption_db_per_m`)
//! - Johnson-Champoux-Allard (JCA) porous impedance & oblique reflection (`jca_reflection_magnitude`)
//! - Spherical-wave pressure-gradient proximity effect (Olson 1957 / Eargle 2004) (`ProximityFilter`)
//! - Cepstral minimum-phase FIR synthesis from digitized Mic Archive curves (`minimum_phase_fir_from_fr_points`)
//! - Frequency-dependent 6-band off-axis polar attenuation (`offaxis_6band_pressure`)
//! - Energy-preserving 8-channel Householder Feedback Delay Network (`LateRoomFdn`, Jot & Chaigne 1991)
//! - Stiff-string inharmonicity dispersion (Fletcher 1964) (`stiff_string_ratio`)
//! - 4-tap cubic Lagrange fractional-delay weights (Laakso et al. 1996) (`lagrange4_weights`)

use crate::studio_catalog::{
    BiotParameters, Pattern, PublishedOffaxisTable, SourceRegenKind, BANDS_HZ,
};
use rustfft::{num_complex::Complex, FftPlanner};
use std::f64::consts::{PI, TAU};

// ======================= 1. ISO 9613-1 AIR ABSORPTION =======================

/// ISO 9613-1:1993 atmospheric sound absorption coefficient in dB/m at frequency `freq_hz`,
/// ambient temperature `temp_c` (°C), and relative humidity `rel_humidity_pct` (0..=100%).
/// Assumes standard sea-level atmospheric pressure `p_a = 101.325 kPa`.
pub fn iso9613_air_absorption_db_per_m(freq_hz: f64, temp_c: f64, rel_humidity_pct: f64) -> f64 {
    if !freq_hz.is_finite() || freq_hz <= 0.0 {
        return 0.0;
    }
    let t_k = (temp_c.clamp(-40.0, 60.0)) + 273.15;
    let t0 = 293.15_f64; // reference temperature (20 °C)
    let t01 = 273.16_f64; // triple-point isotherm temperature
    let hr = rel_humidity_pct.clamp(1.0, 100.0);
    let pr_ratio = 1.0_f64; // p_a / p_r = 101.325 / 101.325

    // Saturation vapor pressure ratio (ISO 9613-1 Annex B)
    let c_exp = -6.8346 * (t01 / t_k).powf(1.261) + 4.6151;
    let psat_pr = 10.0_f64.powf(c_exp);
    // Molar concentration of water vapor (%)
    let h = hr * psat_pr / pr_ratio;

    // Oxygen and nitrogen vibrational relaxation frequencies (Hz)
    let f_ro = pr_ratio * (24.0 + 4.04e4 * h * (0.02 + h) / (0.391 + h));
    let f_rn = pr_ratio
        * (t_k / t0).powf(-0.5)
        * (9.0 + 280.0 * h * (-4.170 * ((t_k / t0).powf(-1.0 / 3.0) - 1.0)).exp());

    let f2 = freq_hz * freq_hz;
    let term_classic = 1.84e-11 / pr_ratio * (t_k / t0).sqrt();
    let term_o2 = 0.01275 * (-2239.1 / t_k).exp() / (f_ro + f2 / f_ro);
    let term_n2 = 0.1068 * (-3352.0 / t_k).exp() / (f_rn + f2 / f_rn);

    let alpha_db_m = 8.686 * f2 * (term_classic + (t_k / t0).powf(-2.5) * (term_o2 + term_n2));
    if alpha_db_m.is_finite() {
        alpha_db_m.max(0.0)
    } else {
        0.0
    }
}

/// Linear pressure attenuation factor `10^(-alpha_dB * dist_m / 20)` across the
/// 6 octave bands `[125, 250, 500, 1000, 2000, 4000]` Hz for propagation distance `dist_m`.
pub fn air_absorption_amplitude_6band(dist_m: f64, temp_c: f64, rel_humidity_pct: f64) -> [f64; 6] {
    if !dist_m.is_finite() || dist_m <= 0.0 {
        return [1.0; 6];
    }
    BANDS_HZ.map(|f| {
        let db_per_m = iso9613_air_absorption_db_per_m(f, temp_c, rel_humidity_pct);
        10.0_f64.powf(-db_per_m * dist_m / 20.0).clamp(0.0, 1.0)
    })
}

// ======================= 2. JOHNSON-CHAMPOUX-ALLARD (JCA) =======================

/// Compute oblique-incidence pressure reflection magnitude `|R(f, theta)|` for a
/// rigid-backed porous absorber using the Johnson-Champoux-Allard (JCA) equivalent-fluid
/// model (Allard & Atalla 2009).
///
/// Returns `None` if any required Biot-JCA physical parameter or thickness is missing,
/// strictly upholding `SPECTRACRAFT_SPEC.md` Section 10 (never invent missing measurements).
pub fn jca_reflection_magnitude(
    freq_hz: f64,
    incidence_rad: f64,
    thickness_mm: Option<f32>,
    biot: &BiotParameters,
) -> Option<f64> {
    let phi = f64::from(biot.porosity?);
    let sigma = f64::from(biot.airflow_resistivity_pa_s_m2?);
    let alpha_inf = f64::from(biot.tortuosity?);
    let vcl_m = f64::from(biot.vcl_um?) * 1e-6;
    let tcl_m = f64::from(biot.tcl_um?) * 1e-6;
    let d_m = f64::from(thickness_mm?) * 1e-3;

    if !freq_hz.is_finite()
        || freq_hz <= 0.0
        || !incidence_rad.is_finite()
        || !(0.01..=1.0).contains(&phi)
        || sigma <= 0.0
        || alpha_inf < 1.0
        || vcl_m <= 0.0
        || tcl_m <= 0.0
        || d_m <= 0.0
    {
        return None;
    }

    let omega = TAU * freq_hz;
    let rho0 = 1.204_f64; // air density at 20 °C (kg/m³)
    let c0 = 343.0_f64; // speed of sound (m/s)
    let z0 = rho0 * c0; // characteristic acoustic impedance of air
    let eta = 1.81e-5_f64; // dynamic viscosity of air (Pa·s)
    let gamma = 1.4_f64; // ratio of specific heats
    let p0 = 101_325.0_f64; // ambient atmospheric pressure (Pa)
    let prandtl = 0.71_f64; // Prandtl number of air
    let j = Complex::new(0.0_f64, 1.0_f64);

    // Dynamic effective density rho_eff(omega)
    let visc_inner = Complex::new(
        1.0,
        (4.0 * alpha_inf * alpha_inf * eta * rho0 * omega)
            / (sigma * sigma * vcl_m * vcl_m * phi * phi),
    )
    .sqrt();
    let rho_eff = Complex::new(alpha_inf * rho0 / phi, 0.0)
        * (Complex::new(1.0, 0.0)
            + (Complex::new(sigma * phi, 0.0) / (j * (omega * rho0 * alpha_inf))) * visc_inner);

    // Dynamic effective bulk modulus K_eff(omega)
    let therm_inner =
        Complex::new(1.0, (rho0 * omega * prandtl * tcl_m * tcl_m) / (16.0 * eta)).sqrt();
    let denom_therm = Complex::new(1.0, 0.0)
        + (Complex::new(8.0 * eta, 0.0) / (j * (omega * prandtl * rho0 * tcl_m * tcl_m)))
            * therm_inner;
    let k_eff = Complex::new(gamma * p0 / phi, 0.0)
        / (Complex::new(gamma, 0.0) - Complex::new(gamma - 1.0, 0.0) / denom_therm);

    // Characteristic impedance Z_c and complex wavenumber k_c
    let z_c = (rho_eff * k_eff).sqrt();
    let k_c = (rho_eff / k_eff).sqrt() * omega;
    let kd = k_c * d_m;

    // Rigid-backed surface impedance Z_s = -j * Z_c * cot(k_c * d)
    let sin_kd = kd.sin();
    if sin_kd.norm() < 1e-12 {
        return None;
    }
    let z_s = -j * z_c * (kd.cos() / sin_kd);
    let cos_theta = incidence_rad.cos().clamp(0.05, 1.0);
    let z0_c = Complex::new(z0, 0.0);
    let r = (z_s * cos_theta - z0_c) / (z_s * cos_theta + z0_c);
    let mag = r.norm();
    if mag.is_finite() {
        Some(mag.clamp(0.0, 1.0))
    } else {
        None
    }
}

// ======================= 3. SPHERICAL-WAVE PROXIMITY EFFECT =======================

/// First-order spherical-wave pressure-gradient proximity effect filter (Olson 1957; Eargle 2004).
///
/// For a capsule with omnidirectional fraction `alpha` (1.0 = omni, 0.5 = cardioid, 0.0 = figure-8)
/// at distance `distance_m` from a point source, acoustic particle velocity exceeds pressure by
/// `1 + c / (j * omega * r)`, producing the exact low-frequency proximity shelf:
/// `H(s) = 1 + ((1 - alpha) * c / r) / (s + omega_min)`,
/// where `omega_min = 2*pi*28 Hz` models the physical diaphragm low-frequency resonance limit so
/// gain remains bounded and finite at DC.
#[derive(Clone, Copy, Debug)]
pub struct ProximityFilter {
    bypass: bool,
    b0: f64,
    b1: f64,
    a1: f64,
    x1: f64,
    y1: f64,
}

impl ProximityFilter {
    pub fn new(
        sample_rate: u32,
        pattern: Pattern,
        distance_m: f64,
        speed_of_sound_mps: f64,
    ) -> Self {
        let grad_frac = (1.0 - pattern.alpha()).clamp(0.0, 1.0);
        let r = distance_m.max(0.05);
        let c = speed_of_sound_mps.clamp(250.0, 420.0);
        let fs = f64::from(sample_rate.max(1));

        // Pure omni (alpha == 1.0) or very far field (> 8 m) has negligible proximity boost.
        if grad_frac < 1e-6 || r > 8.0 {
            return Self {
                bypass: true,
                b0: 1.0,
                b1: 0.0,
                a1: 0.0,
                x1: 0.0,
                y1: 0.0,
            };
        }

        // Physical corner frequency omega_p = (1 - alpha) * c / r, bounded to realistic capsule LF boost
        let omega_min = TAU * 28.0; // physical diaphragm rolloff pole (~28 Hz)
        let max_lf_gain = 4.0; // cap at +12 dB at 28 Hz to prevent subsonic overload
        let omega_zero_raw = omega_min + grad_frac * c / r;
        let omega_zero = omega_zero_raw.min(omega_min * max_lf_gain);

        // Bilinear transform of H(s) = (s + omega_zero) / (s + omega_min)
        let k = 2.0 * fs;
        let denom = k + omega_min;
        let b0 = (k + omega_zero) / denom;
        let b1 = (omega_zero - k) / denom;
        let a1 = (omega_min - k) / denom;

        Self {
            bypass: false,
            b0,
            b1,
            a1,
            x1: 0.0,
            y1: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        if self.bypass {
            return x;
        }
        let y = self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    pub fn apply_slice(&mut self, input: &[f32]) -> Vec<f32> {
        if self.bypass {
            return input.to_vec();
        }
        input
            .iter()
            .map(|&s| {
                let v = if s.is_finite() { f64::from(s) } else { 0.0 };
                let out = self.process(v);
                if out.is_finite() {
                    out as f32
                } else {
                    0.0
                }
            })
            .collect()
    }
}

// ======================= 4. DIGITIZED MIC FR MINIMUM-PHASE FIR =======================

/// Interpolate a digitized frequency-response curve `[[hz, db], ...]` in log-frequency space,
/// clamping cleanly to endpoint values outside the published range.
fn interpolate_fr_db(points: &[[f32; 2]], freq_hz: f64) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    let f = freq_hz.max(1.0);
    if f <= f64::from(points[0][0]) {
        return f64::from(points[0][1]).clamp(-18.0, 18.0);
    }
    let last = points[points.len() - 1];
    if f >= f64::from(last[0]) {
        return f64::from(last[1]).clamp(-18.0, 18.0);
    }
    let log_f = f.ln();
    for pair in points.windows(2) {
        let f0 = f64::from(pair[0][0]).max(1.0);
        let f1 = f64::from(pair[1][0]).max(f0 + 1e-6);
        if f >= f0 && f <= f1 {
            let t = ((log_f - f0.ln()) / (f1.ln() - f0.ln())).clamp(0.0, 1.0);
            let db = f64::from(pair[0][1]) + t * (f64::from(pair[1][1]) - f64::from(pair[0][1]));
            return db.clamp(-18.0, 18.0);
        }
    }
    0.0
}

/// Synthesize a causal minimum-phase FIR kernel (Oppenheim & Schafer real-cepstrum method)
/// from digitized manufacturer frequency-response points `[[hz, db], ...]`.
///
/// Returns `None` when fewer than 8 valid points exist (in which case callers fall back
/// cleanly to the parametric `MicRendererProfile`). The resulting filter is unity-calibrated
/// at 1 kHz and transparently labeled as digitized-spec-derived (not a measured hardware IR).
pub fn minimum_phase_fir_from_fr_points(
    raw_points: &[[f32; 2]],
    sample_rate: u32,
    taps: usize,
) -> Option<Vec<f64>> {
    let mut pts: Vec<[f32; 2]> = raw_points
        .iter()
        .copied()
        .filter(|p| p[0].is_finite() && p[1].is_finite() && p[0] > 1.0)
        .collect();
    if pts.len() < 8 {
        return None;
    }
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));

    let n_taps = taps.clamp(32, 256);
    let fft_len = (n_taps * 8).next_power_of_two().max(512);
    let fs = f64::from(sample_rate.max(8_000));

    // Normalize curve relative to 1 kHz reference so overall loudness stays calibrated
    let ref_1k_db = interpolate_fr_db(&pts, 1000.0);

    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(fft_len);
    let ifft = planner.plan_fft_inverse(fft_len);

    // 1. Build log-magnitude spectrum ln(|H(k)|)
    let mut log_mag = vec![Complex::new(0.0_f64, 0.0_f64); fft_len];
    for k in 0..=fft_len / 2 {
        let freq = (k as f64) * fs / (fft_len as f64);
        let db = if k == 0 {
            // Gentle subsonic anchor at 15 Hz so DC stays stable
            interpolate_fr_db(&pts, 15.0) - ref_1k_db
        } else {
            interpolate_fr_db(&pts, freq) - ref_1k_db
        };
        let lin = 10.0_f64.powf(db.clamp(-18.0, 15.0) / 20.0).max(1e-4);
        let lm = Complex::new(lin.ln(), 0.0);
        log_mag[k] = lm;
        if k > 0 && k < fft_len / 2 {
            log_mag[fft_len - k] = lm;
        }
    }

    // 2. Inverse FFT -> real cepstrum c[n]
    let mut cep = log_mag;
    ifft.process(&mut cep);
    let inv_n = 1.0 / (fft_len as f64);
    for v in &mut cep {
        *v *= inv_n;
    }

    // 3. Causal minimum-phase lifter: c_min[0] = c[0], c_min[1..N/2] = 2*c[n], c_min[N/2] = c[N/2], 0 elsewhere
    for v in cep.iter_mut().take(fft_len / 2).skip(1) {
        *v *= 2.0;
    }
    for v in cep.iter_mut().skip(fft_len / 2 + 1) {
        *v = Complex::new(0.0, 0.0);
    }

    // 4. Forward FFT -> complex log spectrum -> exp -> Inverse FFT -> minimum-phase impulse response
    fft.process(&mut cep);
    for v in &mut cep {
        *v = v.exp();
    }
    ifft.process(&mut cep);

    // Extract first `n_taps` with half-Hann fade on the tail quarter to prevent truncation ripple
    let mut kernel = Vec::with_capacity(n_taps);
    let fade_start = (n_taps * 3) / 4;
    for (i, c) in cep.iter().take(n_taps).enumerate() {
        let mut h = c.re * inv_n;
        if i >= fade_start {
            let t = (i - fade_start) as f64 / ((n_taps - fade_start).max(1) as f64);
            h *= 0.5 * (1.0 + (PI * t).cos());
        }
        if !h.is_finite() {
            return None;
        }
        kernel.push(h);
    }
    Some(kernel)
}

/// Apply a causal FIR kernel cleanly to a mono signal (preserving input length).
pub fn apply_fir(input: &[f32], kernel: &[f64]) -> Vec<f32> {
    if input.is_empty() || kernel.is_empty() {
        return input.to_vec();
    }
    let n = input.len();
    let m = kernel.len();
    let mut out = vec![0.0_f32; n];
    for (i, dst) in out.iter_mut().enumerate() {
        let max_k = m.min(i + 1);
        let mut acc = 0.0_f64;
        for k in 0..max_k {
            let s = f64::from(input[i - k]);
            if s.is_finite() {
                acc += s * kernel[k];
            }
        }
        *dst = if acc.is_finite() { acc as f32 } else { 0.0 };
    }
    out
}

// ======================= 5. FREQUENCY-DEPENDENT OFF-AXIS POLAR =======================

fn interp_table_db(freqs: &[f32], vals: &[f32], target_hz: f64) -> Option<f64> {
    if freqs.is_empty() || freqs.len() != vals.len() {
        return None;
    }
    if target_hz <= f64::from(freqs[0]) {
        return Some(f64::from(vals[0]).clamp(-36.0, 6.0));
    }
    let last = freqs.len() - 1;
    if target_hz >= f64::from(freqs[last]) {
        return Some(f64::from(vals[last]).clamp(-36.0, 6.0));
    }
    let log_t = target_hz.max(1.0).ln();
    for i in 0..last {
        let f0 = f64::from(freqs[i]).max(1.0);
        let f1 = f64::from(freqs[i + 1]).max(f0 + 1e-6);
        if target_hz >= f0 && target_hz <= f1 {
            let u = ((log_t - f0.ln()) / (f1.ln() - f0.ln())).clamp(0.0, 1.0);
            let db = f64::from(vals[i]) + u * (f64::from(vals[i + 1]) - f64::from(vals[i]));
            return Some(db.clamp(-36.0, 6.0));
        }
    }
    None
}

/// Evaluate frequency-dependent 6-band polar pressure gain for arrival cosine `cos_theta`
/// using a Mic Archive `PublishedOffaxisTable` (preserving polarity sign from `fallback_pattern`).
pub fn offaxis_6band_pressure(
    table: &PublishedOffaxisTable,
    cos_theta: f64,
    fallback_pattern: Pattern,
) -> Option<[f64; 6]> {
    if table.freqs_hz.len() < 2
        || table.atten_90deg_db.len() != table.freqs_hz.len()
        || table.atten_180deg_db.len() != table.freqs_hz.len()
    {
        return None;
    }
    let c = cos_theta.clamp(-1.0, 1.0);
    let deg = c.acos().to_degrees();
    let sign = if fallback_pattern.pressure(c) < 0.0 {
        -1.0
    } else {
        1.0
    };

    let mut out = [0.0_f64; 6];
    for (b, &hz) in BANDS_HZ.iter().enumerate() {
        let db90 = interp_table_db(&table.freqs_hz, &table.atten_90deg_db, hz)?;
        let db180 = interp_table_db(&table.freqs_hz, &table.atten_180deg_db, hz)?;
        let g90 = 10.0_f64.powf(db90 / 20.0).clamp(0.0, 1.25);
        let g180 = 10.0_f64.powf(db180 / 20.0).clamp(0.0, 1.25);
        let mag = if deg <= 90.0 {
            let t = deg / 90.0;
            // Smooth cosine interpolation between 0° (1.0) and 90° (g90)
            let w = 0.5 * (1.0 - (PI * t).cos());
            1.0 + (g90 - 1.0) * w
        } else {
            let t = (deg - 90.0) / 90.0;
            let w = 0.5 * (1.0 - (PI * t).cos());
            g90 + (g180 - g90) * w
        };
        out[b] = sign * mag;
    }
    Some(out)
}

// ======================= 6. HYBRID FDN LATE REVERBERATION =======================

/// Eyring reverberation time estimate (seconds) from room volume `V`, total surface area `S`,
/// and area-weighted mean effective absorption `alpha_bar`. Returns `None` if invalid or anechoic.
pub fn eyring_rt60_seconds(volume_m3: f64, area_m2: f64, alpha_bar: f64) -> Option<f64> {
    if !volume_m3.is_finite()
        || !area_m2.is_finite()
        || !alpha_bar.is_finite()
        || volume_m3 <= 0.1
        || area_m2 <= 0.1
        || alpha_bar <= 1e-4
        || alpha_bar >= 0.999
    {
        return None;
    }
    let denom = -area_m2 * (1.0 - alpha_bar).ln();
    if denom <= 1e-9 {
        return None;
    }
    let rt60 = 0.161 * volume_m3 / denom;
    rt60.is_finite().then_some(rt60.clamp(0.04, 12.0))
}

/// Physical diffuse mixing time estimate (seconds): `t_m ≈ sqrt(V) / 1000 * 20` (Polack / Lindau),
/// bounded cleanly to `[0.012, 0.12]` s.
pub fn mixing_time_seconds(volume_m3: f64) -> f64 {
    if !volume_m3.is_finite() || volume_m3 <= 0.1 {
        return 0.025;
    }
    (0.020 * volume_m3.sqrt()).clamp(0.012, 0.120)
}

/// Energy-preserving 8-channel Householder Feedback Delay Network (Jot & Chaigne 1991;
/// Schlecht & Habets 2017) calibrated strictly from room dimensions and 6-face absorption.
///
/// The Householder matrix `H = I - (2/N) * 1 * 1^T` is strictly orthogonal (`H^T H = I`),
/// ensuring lossless energy circulation whose decay rate is governed exclusively by the
/// per-channel 1-pole frequency-dependent Eyring absorption filters.
pub struct LateRoomFdn {
    delays: [Vec<f64>; 8],
    pos: [usize; 8],
    lp_state: [f64; 8],
    g_dc: [f64; 8],
    g_lp_coeff: [f64; 8],
    predelay: Vec<f64>,
    predelay_pos: usize,
    wet_gain: f64,
}

impl LateRoomFdn {
    /// Construct an 8-channel FDN strictly from physical room dimensions `dims_m` and
    /// 6-face effective absorption `[face][band]`. Returns `None` for non-reflective or
    /// open/anechoic spaces so zero artificial reverb is ever injected.
    pub fn from_room_physics(
        sample_rate: u32,
        dims_m: [f64; 3],
        faces_eff_alpha: &[[f64; 6]; 6],
        speed_of_sound_mps: f64,
        direct_distance_m: f64,
        first_late_delay_s: f64,
    ) -> Option<Self> {
        let [w, l, h] = dims_m;
        if w <= 0.2 || l <= 0.2 || h <= 0.2 {
            return None;
        }
        let volume = w * l * h;
        let face_areas = [l * h, l * h, w * h, w * h, w * l, w * l];
        let total_area: f64 = face_areas.iter().sum();

        // Compute area-weighted low-band (125-500 Hz) and high-band (1-4 kHz) absorption
        let mut a_low = 0.0;
        let mut a_high = 0.0;
        for f in 0..6 {
            let low_avg =
                (faces_eff_alpha[f][0] + faces_eff_alpha[f][1] + faces_eff_alpha[f][2]) / 3.0;
            let high_avg =
                (faces_eff_alpha[f][3] + faces_eff_alpha[f][4] + faces_eff_alpha[f][5]) / 3.0;
            a_low += face_areas[f] * low_avg.clamp(0.0, 1.0);
            a_high += face_areas[f] * high_avg.clamp(0.0, 1.0);
        }
        let alpha_low = (a_low / total_area).clamp(0.0, 0.999);
        let alpha_high = (a_high / total_area).clamp(0.0, 0.999);

        // If the room is nearly anechoic (>92% average absorption), no diffuse tail exists
        if alpha_low > 0.92 && alpha_high > 0.92 {
            return None;
        }

        let rt_low = eyring_rt60_seconds(volume, total_area, alpha_low)?;
        let rt_high = eyring_rt60_seconds(volume, total_area, alpha_high).unwrap_or(rt_low * 0.65);

        let fs = f64::from(sample_rate.max(8_000));
        let c = speed_of_sound_mps.clamp(250.0, 420.0);
        // Mean free path L_mfp = 4V / S -> base delay scaled across 8 mutually prime lengths
        let mfp_samples = ((4.0 * volume / total_area) / c * fs).clamp(97.0, fs * 0.08);
        let prime_ratios: [f64; 8] = [0.71, 0.83, 0.97, 1.13, 1.29, 1.49, 1.73, 1.99];
        let base_primes: [usize; 8] = [149, 191, 239, 293, 353, 419, 499, 577];

        let max_delay = (fs as usize / 2).max(128);
        let mut delay_lens = [0_usize; 8];
        for i in 0..8 {
            let scaled = (mfp_samples * prime_ratios[i]).round() as usize;
            // Ensure mutually distinct odd/prime-like lengths
            delay_lens[i] = scaled.max(base_primes[i] / 2).min(max_delay) | 1;
        }

        let mut g_dc = [0.0_f64; 8];
        let mut g_lp_coeff = [0.0_f64; 8];
        for i in 0..8 {
            let tau_i = delay_lens[i] as f64 / fs;
            let gain_low = 10.0_f64.powf(-3.0 * tau_i / rt_low).clamp(0.0, 0.996);
            let gain_high = 10.0_f64
                .powf(-3.0 * tau_i / rt_high.min(rt_low))
                .clamp(0.0, gain_low);
            g_dc[i] = gain_low;
            // 1-pole Jot damping coefficient p in y[n] = (1 - p)*x[n] + p*y[n-1]
            let ratio = (gain_high / gain_low.max(1e-6)).clamp(0.05, 1.0);
            g_lp_coeff[i] = ((1.0 - ratio) / (1.0 + ratio)).clamp(0.0, 0.88);
        }

        // Physical diffuse-field energy scaling relative to direct field (critical distance r_c)
        let alpha_mean = 0.5 * (alpha_low + alpha_high);
        let room_const = total_area * alpha_mean / (1.0 - alpha_mean).max(0.02);
        let r_direct = direct_distance_m.max(0.2);
        // Diffuse-to-direct pressure ratio ~ sqrt(4 / (room_const / (4*pi*r^2))) = 4 * sqrt(pi) * r / sqrt(R)
        // Attenuated by 0.22 because early orders 1..4 are already synthesized by image-source.
        let diffuse_ratio =
            (0.22 * r_direct * (16.0 * PI / room_const.max(0.5)).sqrt()).clamp(0.0, 0.55);

        let predelay_s = first_late_delay_s.max(mixing_time_seconds(volume));
        let predelay_len = ((predelay_s * fs).round() as usize).clamp(16, max_delay);

        Some(Self {
            delays: std::array::from_fn(|i| vec![0.0_f64; delay_lens[i]]),
            pos: [0; 8],
            lp_state: [0.0; 8],
            g_dc,
            g_lp_coeff,
            predelay: vec![0.0_f64; predelay_len],
            predelay_pos: 0,
            wet_gain: diffuse_ratio,
        })
    }

    /// Process one input sample and return the diffuse late-room contribution ONLY
    /// (caller sums this with the early image-source capture output).
    #[inline]
    pub fn tick(&mut self, x: f64) -> f64 {
        let delayed_in = self.predelay[self.predelay_pos];
        self.predelay[self.predelay_pos] = x;
        self.predelay_pos = (self.predelay_pos + 1) % self.predelay.len();

        let mut d_out = [0.0_f64; 8];
        let mut sum = 0.0_f64;
        for i in 0..8 {
            let raw = self.delays[i][self.pos[i]];
            // Per-channel 1-pole frequency-dependent Eyring damping
            let p = self.g_lp_coeff[i];
            let filt = flush_subnormal((1.0 - p) * raw + p * self.lp_state[i]);
            self.lp_state[i] = filt;
            let damped = filt * self.g_dc[i];
            d_out[i] = damped;
            sum += damped;
        }

        // Orthogonal 8x8 Householder mixing: H * v = v - (2/8) * sum(v)
        let mean2 = 0.25 * sum;
        let inj = delayed_in * (1.0 / 8.0_f64.sqrt());
        let signs: [f64; 8] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
        let mut out_sample = 0.0_f64;
        for i in 0..8 {
            let mixed = d_out[i] - mean2;
            self.delays[i][self.pos[i]] = flush_subnormal(mixed + inj * signs[i]);
            self.pos[i] = (self.pos[i] + 1) % self.delays[i].len();
            out_sample += d_out[i] * signs[i];
        }
        out_sample * (self.wet_gain / 8.0_f64.sqrt())
    }

    /// Process one input sample and return genuinely decorrelated stereo `[left, right]`
    /// late-room diffuse contributions using mutually orthogonal Walsh-Hadamard read taps
    /// (`L · R == 0`), preserving passive Eyring decay energy across both channels.
    #[inline]
    pub fn tick_stereo(&mut self, x: f64) -> [f64; 2] {
        let delayed_in = self.predelay[self.predelay_pos];
        self.predelay[self.predelay_pos] = x;
        self.predelay_pos = (self.predelay_pos + 1) % self.predelay.len();

        let mut d_out = [0.0_f64; 8];
        let mut sum = 0.0_f64;
        for i in 0..8 {
            let raw = self.delays[i][self.pos[i]];
            let p = self.g_lp_coeff[i];
            let filt = flush_subnormal((1.0 - p) * raw + p * self.lp_state[i]);
            self.lp_state[i] = filt;
            let damped = filt * self.g_dc[i];
            d_out[i] = damped;
            sum += damped;
        }

        let mean2 = 0.25 * sum;
        let inj = delayed_in * (1.0 / 8.0_f64.sqrt());
        let signs_l: [f64; 8] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
        let signs_r: [f64; 8] = [1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0];
        let mut out_l = 0.0_f64;
        let mut out_r = 0.0_f64;
        for i in 0..8 {
            let mixed = d_out[i] - mean2;
            self.delays[i][self.pos[i]] = flush_subnormal(mixed + inj * signs_l[i]);
            self.pos[i] = (self.pos[i] + 1) % self.delays[i].len();
            out_l += d_out[i] * signs_l[i];
            out_r += d_out[i] * signs_r[i];
        }
        let scale = self.wet_gain / 8.0_f64.sqrt();
        [out_l * scale, out_r * scale]
    }
}

#[inline]
fn flush_subnormal(v: f64) -> f64 {
    if v.abs() < 1e-30 {
        0.0
    } else {
        v
    }
}

// ======================= 7. STIFF-STRING INHARMONICITY =======================

/// Fletcher (1964) stiff-string partial frequency ratio: `k * sqrt(1 + B * k^2)`.
/// Returns exact `k` when `inharmonicity_b <= 0.0`.
#[inline]
pub fn stiff_string_ratio(harmonic: f64, inharmonicity_b: f64) -> f64 {
    if inharmonicity_b <= 0.0 || !inharmonicity_b.is_finite() {
        harmonic
    } else {
        harmonic * (1.0 + inharmonicity_b * harmonic * harmonic).sqrt()
    }
}

/// Physically representative dispersion coefficient `B` by source family
/// (zero for air-column / voice / neutral models so non-string instruments stay harmonic).
pub const fn inharmonicity_b_for_source(kind: SourceRegenKind) -> f64 {
    match kind {
        SourceRegenKind::PluckedElectric | SourceRegenKind::AmpCabinet => 8.0e-5,
        SourceRegenKind::PluckedAcoustic => 1.5e-4,
        SourceRegenKind::PluckedBass => 2.8e-4,
        SourceRegenKind::KeyboardStruck => 3.5e-4,
        _ => 0.0,
    }
}

// ======================= 8. CUBIC LAGRANGE FRACTIONAL DELAY =======================

/// 4-tap cubic Lagrange interpolation weights for fractional delay `frac` in `[0.0, 1.0)`
/// relative to tap indices `[-1, 0, +1, +2]` around integer delay `d0` (Laakso et al. 1996).
/// At `frac == 0.0`, returns `[0.0, 1.0, 0.0, 0.0]` identically.
#[inline]
pub fn lagrange4_weights(frac: f64) -> [f64; 4] {
    let f = frac.clamp(0.0, 1.0);
    let w_m1 = -f * (f - 1.0) * (f - 2.0) / 6.0;
    let w_0 = (f + 1.0) * (f - 1.0) * (f - 2.0) / 2.0;
    let w_p1 = -(f + 1.0) * f * (f - 2.0) / 2.0;
    let w_p2 = (f + 1.0) * f * (f - 1.0) / 6.0;
    [w_m1, w_0, w_p1, w_p2]
}

// ======================= 9. LR4 6-BAND CROSSOVER =======================

#[derive(Clone, Copy, Debug)]
struct Biquad2 {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad2 {
    fn from_norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn butterworth_lp(fs: f64, fc: f64) -> Self {
        let w = TAU * fc.clamp(15.0, fs * 0.45) / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / 2.0_f64.sqrt();
        Self::from_norm(
            (1.0 - c) * 0.5,
            1.0 - c,
            (1.0 - c) * 0.5,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    fn butterworth_hp(fs: f64, fc: f64) -> Self {
        let w = TAU * fc.clamp(15.0, fs * 0.45) / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / 2.0_f64.sqrt();
        Self::from_norm(
            (1.0 + c) * 0.5,
            -(1.0 + c),
            (1.0 + c) * 0.5,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    fn bandpass_const_peak(fs: f64, fc: f64, q: f64) -> Self {
        let w = TAU * fc.clamp(20.0, fs * 0.45) / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.5));
        Self::from_norm(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }

    fn highpass_q(fs: f64, fc: f64, q: f64) -> Self {
        let w = TAU * fc.clamp(10.0, fs * 0.45) / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.25));
        Self::from_norm(
            (1.0 + c) * 0.5,
            -(1.0 + c),
            (1.0 + c) * 0.5,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    fn lowpass_q(fs: f64, fc: f64, q: f64) -> Self {
        let w = TAU * fc.clamp(20.0, fs * 0.46) / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.25));
        Self::from_norm(
            (1.0 - c) * 0.5,
            1.0 - c,
            (1.0 - c) * 0.5,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    fn high_shelf(fs: f64, fc: f64, q: f64, gain_db: f64) -> Self {
        let w = TAU * fc.clamp(20.0, fs * 0.45) / fs;
        let (s, c) = w.sin_cos();
        let a = 10.0_f64.powf(gain_db / 40.0);
        let beta = (a.sqrt() / q.max(0.25)) * s;
        Self::from_norm(
            a * ((a + 1.0) + (a - 1.0) * c + beta),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - beta),
            (a + 1.0) - (a - 1.0) * c + beta,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - beta,
        )
    }

    /// 2nd-order Butterworth allpass matching the phase response of `LP4 + HP4` at `fc`.
    fn allpass_butterworth(fs: f64, fc: f64) -> Self {
        let w = TAU * fc.clamp(20.0, fs * 0.45) / fs;
        let (s, c) = w.sin_cos();
        let alpha = s * std::f64::consts::FRAC_1_SQRT_2;
        Self::from_norm(
            1.0 - alpha,
            -2.0 * c,
            1.0 + alpha,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = flush_subnormal(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = flush_subnormal(self.b2 * x - self.a2 * y);
        flush_subnormal(y)
    }
}

#[derive(Clone, Copy, Debug)]
struct Lr4Pair {
    lp1: Biquad2,
    lp2: Biquad2,
    hp1: Biquad2,
    hp2: Biquad2,
}

impl Lr4Pair {
    fn new(fs: f64, fc: f64) -> Self {
        Self {
            lp1: Biquad2::butterworth_lp(fs, fc),
            lp2: Biquad2::butterworth_lp(fs, fc),
            hp1: Biquad2::butterworth_hp(fs, fc),
            hp2: Biquad2::butterworth_hp(fs, fc),
        }
    }

    #[inline]
    fn split(&mut self, x: f64) -> (f64, f64) {
        let lo = self.lp2.tick(self.lp1.tick(x));
        let hi = self.hp2.tick(self.hp1.tick(x));
        (lo, hi)
    }
}

/// 4th-order Linkwitz-Riley (LR4, 24 dB/oct) 6-band octave crossover for reflected
/// room paths across `[125, 250, 500, 1000, 2000, 4000]` Hz.
///
/// Cascaded LR4 pairs sum to flat allpass unity magnitude across all frequencies while
/// providing 4× steeper inter-band isolation than a 1-pole crossover.
pub struct Lr4SixBandSplit {
    c0: Lr4Pair, // 176.8 Hz
    c1: Lr4Pair, // 353.6 Hz
    c2: Lr4Pair, // 707.1 Hz
    c3: Lr4Pair, // 1414.2 Hz
    c4: Lr4Pair, // 2828.4 Hz
    ap_c0_for_b2: Biquad2,
    ap_c4_for_b3: Biquad2,
    ap_c3_for_lo: Biquad2,
    ap_c4_for_lo: Biquad2,
    ap_c1_for_hi: Biquad2,
    ap_c0_for_hi: Biquad2,
}

impl Lr4SixBandSplit {
    pub fn new(sample_rate: u32) -> Self {
        let fs = f64::from(sample_rate.max(8_000));
        let f0 = (125.0_f64 * 250.0).sqrt();
        let f1 = (250.0_f64 * 500.0).sqrt();
        let f2 = (500.0_f64 * 1000.0).sqrt();
        let f3 = (1000.0_f64 * 2000.0).sqrt();
        let f4 = (2000.0_f64 * 4000.0).sqrt();
        Self {
            c0: Lr4Pair::new(fs, f0),
            c1: Lr4Pair::new(fs, f1),
            c2: Lr4Pair::new(fs, f2),
            c3: Lr4Pair::new(fs, f3),
            c4: Lr4Pair::new(fs, f4),
            ap_c0_for_b2: Biquad2::allpass_butterworth(fs, f0),
            ap_c4_for_b3: Biquad2::allpass_butterworth(fs, f4),
            ap_c3_for_lo: Biquad2::allpass_butterworth(fs, f3),
            ap_c4_for_lo: Biquad2::allpass_butterworth(fs, f4),
            ap_c1_for_hi: Biquad2::allpass_butterworth(fs, f1),
            ap_c0_for_hi: Biquad2::allpass_butterworth(fs, f0),
        }
    }

    #[inline]
    pub fn split(&mut self, x: f64) -> [f64; 6] {
        // Phase-compensated balanced binary-tree LR4 split: every band traverses
        // the exact same total allpass phase A(c0)*A(c1)*A(c2)*A(c3)*A(c4),
        // guaranteeing unity flat magnitude when bands are summed.
        let (low_raw, high_raw) = self.c2.split(x);
        let low_half = self.ap_c4_for_lo.tick(self.ap_c3_for_lo.tick(low_raw));
        let high_half = self.ap_c0_for_hi.tick(self.ap_c1_for_hi.tick(high_raw));

        let (b01, b2_raw) = self.c1.split(low_half);
        let (b0, b1) = self.c0.split(b01);
        let b2 = self.ap_c0_for_b2.tick(b2_raw);

        let (b3_raw, b45) = self.c3.split(high_half);
        let b3 = self.ap_c4_for_b3.tick(b3_raw);
        let (b4, b5) = self.c4.split(b45);
        [b0, b1, b2, b3, b4, b5]
    }
}

// ======================= 10. MODAL BODY RESONATOR & ONSET TRANSIENT =======================

/// Multi-mode acoustic body, plate, cabinet, or vocal-tract parallel resonator bank
/// replacing flat static EQ with physically coupled resonant modes (`A0` Helmholtz air mode,
/// main wood/plate dipole `T(1,1)` / `B1-` / `B1+`, bridge hill, speaker cone breakup, or
/// formant clusters) summed cleanly with direct acoustic radiation.
pub struct ModalBodyResonator {
    modes: Vec<(Biquad2, f64)>,
    dry_mix: f64,
}

impl ModalBodyResonator {
    pub fn for_source(sample_rate: f64, kind: SourceRegenKind) -> Option<Self> {
        // (center_hz, Q, modal_gain)
        let specs: (&[(f64, f64, f64)], f64) = match kind {
            SourceRegenKind::Neutral => return None,
            SourceRegenKind::PluckedAcoustic => (
                &[
                    (98.0, 9.0, 0.28),   // A0 Helmholtz soundhole air mode
                    (196.0, 8.5, 0.32),  // T(1,1) main top-plate monopole
                    (265.0, 7.0, 0.16),  // back-plate coupled mode
                    (415.0, 6.5, 0.18),  // T(2,1) cross-dipole
                    (740.0, 5.5, 0.14),  // upper bout wood resonance
                    (1380.0, 4.5, 0.12), // bridge saddle coupling
                    (2650.0, 3.8, 0.14), // spruce shimmer / pick radiation
                ],
                0.76,
            ),
            SourceRegenKind::PluckedElectric | SourceRegenKind::AmpCabinet => (
                &[
                    (112.0, 5.0, 0.24),  // 1x12 open/closed-back cabinet resonance
                    (240.0, 3.5, 0.15),  // lower speaker box warmth
                    (920.0, 3.2, 0.16),  // paper-cone mid punch
                    (2350.0, 4.0, 0.24), // Celestion-class cone breakup presence peak
                    (3650.0, 4.2, 0.16), // voice-coil / dust-cap edge
                ],
                0.78,
            ),
            SourceRegenKind::PluckedBass => (
                &[
                    (68.0, 6.5, 0.30),  // cabinet / bass body fundamental support
                    (132.0, 5.5, 0.24), // wood body chest mode
                    (310.0, 4.5, 0.15), // neck/body growl
                    (820.0, 3.8, 0.12), // fingerboard articulation
                ],
                0.78,
            ),
            SourceRegenKind::BowedLow => (
                &[
                    (66.0, 10.0, 0.22),  // Cello A0 Helmholtz air mode
                    (134.0, 9.0, 0.30),  // B1- main wood resonance
                    (178.0, 8.5, 0.26),  // B1+ upper main wood mode
                    (395.0, 6.0, 0.16),  // body cavity resonance
                    (1180.0, 4.5, 0.18), // cello bridge hill
                    (2350.0, 4.0, 0.12), // bow rosin air
                ],
                0.74,
            ),
            SourceRegenKind::BowedHigh => (
                &[
                    (276.0, 11.0, 0.26), // Violin A0 Helmholtz air mode
                    (465.0, 9.5, 0.28),  // B1- signature body mode
                    (538.0, 9.0, 0.26),  // B1+ signature body mode
                    (1120.0, 6.0, 0.16), // transition hill
                    (2620.0, 4.8, 0.32), // BH bridge hill (signature Strad-class projection)
                    (4200.0, 4.2, 0.14), // E-string brilliance
                ],
                0.72,
            ),
            SourceRegenKind::KeyboardStruck => (
                &[
                    (110.0, 7.5, 0.22),  // spruce soundboard fundamental rib mode
                    (235.0, 6.5, 0.20),  // bridge-plate belly mode
                    (540.0, 5.5, 0.16),  // mid soundboard projection
                    (1450.0, 4.5, 0.14), // hammer-felt treble clarity
                    (2950.0, 4.0, 0.10), // cast-iron frame ring
                ],
                0.80,
            ),
            SourceRegenKind::WoodwindFlute | SourceRegenKind::WoodwindReed => (
                &[
                    (320.0, 5.0, 0.18),
                    (980.0, 5.5, 0.22),
                    (1780.0, 5.0, 0.24),
                    (3250.0, 4.2, 0.16),
                ],
                0.78,
            ),
            SourceRegenKind::BrassBright | SourceRegenKind::BrassWarm => (
                &[
                    (280.0, 4.5, 0.18),
                    (780.0, 5.0, 0.22),
                    (1650.0, 5.2, 0.26),
                    (2900.0, 4.8, 0.22),
                ],
                0.76,
            ),
            SourceRegenKind::VoiceMale => (
                &[
                    (510.0, 6.5, 0.28),  // F1 vocal-tract formant
                    (1280.0, 7.5, 0.24), // F2 vocal-tract formant
                    (2580.0, 8.0, 0.24), // F3 / singer's formant cluster
                    (3120.0, 7.0, 0.18), // F4 brilliance
                ],
                0.70,
            ),
            SourceRegenKind::VoiceFemale | SourceRegenKind::VoiceEnsemble => (
                &[
                    (660.0, 6.5, 0.28),  // F1
                    (1620.0, 7.5, 0.24), // F2
                    (2950.0, 8.0, 0.26), // F3 / presence cluster
                    (3850.0, 6.5, 0.16), // F4 air
                ],
                0.70,
            ),
            SourceRegenKind::KeyboardOrgan => (
                &[
                    (130.0, 5.0, 0.18),  // windchest / cabinet warmth
                    (480.0, 4.5, 0.16),  // principal pipe body
                    (1650.0, 4.2, 0.14), // upperwork presence
                    (3400.0, 4.0, 0.10), // pipe chiff air
                ],
                0.80,
            ),
            SourceRegenKind::Percussion => (
                &[
                    (125.0, 6.0, 0.26),  // shell fundamental cavity
                    (290.0, 5.5, 0.22),  // membrane dipole mode
                    (860.0, 4.5, 0.18),  // rim / shell crack
                    (2450.0, 4.0, 0.16), // stick / beater articulation
                ],
                0.74,
            ),
        };
        let modes = specs
            .0
            .iter()
            .filter(|&&(fc, _, _)| fc < sample_rate * 0.44)
            .map(|&(fc, q, g)| (Biquad2::bandpass_const_peak(sample_rate, fc, q), g))
            .collect();
        Some(Self {
            modes,
            dry_mix: specs.1,
        })
    }

    pub fn process_inplace(&mut self, buf: &mut [f64]) {
        for sample in buf.iter_mut() {
            let x = *sample;
            let mut body = 0.0_f64;
            for (filt, gain) in &mut self.modes {
                body += filt.tick(x) * *gain;
            }
            *sample = self.dry_mix * x + body;
        }
    }
}

/// Physically scaled short acoustic excitation impulse at note onset (plectrum click,
/// piano hammer felt thump, bow hair catch, or vocal/wind breath consonant).
/// Returns exact `0.0` for `Neutral` or past the onset window (~8 ms).
#[inline]
pub fn synthesize_onset_transient(
    kind: SourceRegenKind,
    t_s: f64,
    white_sample: f64,
    onset_level: f64,
) -> f64 {
    if t_s < 0.0 || t_s > 0.012 {
        return 0.0;
    }
    let (tau_s, amp) = match kind {
        SourceRegenKind::Neutral => return 0.0,
        SourceRegenKind::PluckedAcoustic => (0.0022, 0.11),
        SourceRegenKind::PluckedElectric | SourceRegenKind::AmpCabinet => (0.0018, 0.085),
        SourceRegenKind::PluckedBass => (0.0032, 0.075),
        SourceRegenKind::KeyboardStruck => (0.0028, 0.12),
        SourceRegenKind::BowedLow | SourceRegenKind::BowedHigh => (0.0065, 0.055),
        SourceRegenKind::WoodwindFlute | SourceRegenKind::WoodwindReed => (0.0055, 0.065),
        SourceRegenKind::BrassBright | SourceRegenKind::BrassWarm => (0.0040, 0.075),
        SourceRegenKind::VoiceMale
        | SourceRegenKind::VoiceFemale
        | SourceRegenKind::VoiceEnsemble => (0.0050, 0.045),
        SourceRegenKind::KeyboardOrgan => (0.0035, 0.040),
        SourceRegenKind::Percussion => (0.0016, 0.16),
    };
    let env = (-t_s / tau_s).exp();
    amp * onset_level.clamp(0.05, 1.5) * env * white_sample
}

// ======================= 11. STAGE 3 OVERSAMPLED PREAMP STAGE =======================

/// 2× oversampled asymmetric triode/transformer studio preamp coloration stage.
///
/// Strictly bit-identical pass-through when `drive <= 1e-6`, and exact `0.0` on digital
/// silence (zero synthetic hum or hiss).
pub struct PreampStage {
    bypass: bool,
    drive: f64,
    dc_bias: f64,
    bias_offset: f64,
    aa_up: Biquad2,
    aa_dn: Biquad2,
    dc_block: Biquad2,
    attack_a: f64,
    release_a: f64,
    sag_env: f64,
    prev_x: f64,
}

impl PreampStage {
    pub fn new(sample_rate: u32, drive_0_to_1: f64) -> Self {
        let fs = f64::from(sample_rate.max(8_000));
        let os_fs = fs * 2.0;
        let d = drive_0_to_1.clamp(0.0, 1.0);
        let bypass = !d.is_finite() || d <= 1e-6;
        let gain_drive = 1.0 + 3.2 * d;
        let dc_bias = 0.09 * d;
        Self {
            bypass,
            drive: gain_drive,
            dc_bias,
            bias_offset: dc_bias.tanh(),
            aa_up: Biquad2::lowpass_q(os_fs, fs * 0.43, 0.707),
            aa_dn: Biquad2::lowpass_q(os_fs, fs * 0.43, 0.707),
            dc_block: Biquad2::highpass_q(fs, 22.0, 0.707),
            attack_a: (-1.0 / (0.010 * os_fs)).exp(),
            release_a: (-1.0 / (0.120 * os_fs)).exp(),
            sag_env: 0.0,
            prev_x: 0.0,
        }
    }

    pub fn apply_slice(&mut self, input: &[f32]) -> Vec<f32> {
        if self.bypass {
            return input.to_vec();
        }
        let mut out = Vec::with_capacity(input.len());
        for &s in input {
            let cur_x = if s.is_finite() { f64::from(s) } else { 0.0 };
            let pair = [0.5 * (self.prev_x + cur_x), cur_x];
            self.prev_x = cur_x;
            let mut y = 0.0_f64;
            for &x_os in &pair {
                let xf = self.aa_up.tick(x_os);
                let abs_x = (xf * self.drive).abs();
                let coeff = if abs_x > self.sag_env {
                    self.attack_a
                } else {
                    self.release_a
                };
                self.sag_env = coeff * self.sag_env + (1.0 - coeff) * abs_x;
                let sag = 1.0 / (1.0 + 0.22 * self.sag_env);
                let driven = xf * self.drive * sag;
                // Asymmetric tube/transformer transfer: exact 0.0 when driven == 0.0
                let triode = ((driven + self.dc_bias).tanh() - self.bias_offset) / self.drive;
                let xformer = triode * (1.0 - 0.08 * (triode * self.drive).abs().min(1.0));
                y = self.aa_dn.tick(xformer);
            }
            let clean = self.dc_block.tick(y);
            out.push(if clean.is_finite() { clean as f32 } else { 0.0 });
        }
        out
    }
}

// ======================= 12. ITU-R BS.1770-4 LUFS, 4X TRUE PEAK & 24-BIT TPDF =======================

/// Loudness and delivery metering readout conforming to ITU-R BS.1770-4.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoudnessMetrics {
    pub integrated_lufs: f64,
    pub sample_peak_dbfs: f64,
    pub true_peak_dbtp: f64,
}

/// 8-tap Hann-windowed sinc fractional-delay weights for phase `frac` in `(0.0, 1.0)`
/// across tap offsets `[-3, -2, -1, 0, +1, +2, +3, +4]` (ITU-R BS.1770-4 true-peak class).
pub fn sinc8_weights_phase(frac: f64) -> [f64; 8] {
    let mut w = [0.0_f64; 8];
    let mut sum = 0.0_f64;
    for (idx, k) in (-3_i32..=4_i32).enumerate() {
        let d = f64::from(k) - frac;
        let sinc = if d.abs() < 1e-12 {
            1.0
        } else {
            let x = PI * d;
            x.sin() / x
        };
        let win = 0.5 * (1.0 + (PI * d / 4.5).cos());
        w[idx] = sinc * win;
        sum += w[idx];
    }
    if sum.abs() > 1e-12 {
        for v in &mut w {
            *v /= sum;
        }
    }
    w
}

/// Measure 4× oversampled inter-sample true peak (linear amplitude) using an 8-tap
/// Hann-windowed sinc polyphase interpolator at phases `{0.0, 0.25, 0.50, 0.75}`.
pub fn measure_true_peak_4x_linear(samples: &[f32]) -> f64 {
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }
    let mut peak = 0.0_f64;
    let phases = [
        sinc8_weights_phase(0.25),
        sinc8_weights_phase(0.50),
        sinc8_weights_phase(0.75),
    ];

    for i in 0..n {
        let s0 = f64::from(samples[i]);
        if s0.is_finite() {
            peak = peak.max(s0.abs());
        }
        if i >= 3 && i + 4 < n {
            for w in &phases {
                let mut y = 0.0_f64;
                for (t, &wt) in w.iter().enumerate() {
                    y += f64::from(samples[i + t - 3]) * wt;
                }
                if y.is_finite() {
                    peak = peak.max(y.abs());
                }
            }
        }
    }
    peak
}

/// Apply ITU-R BS.1770-4 K-weighting pre-filter (Stage 1: +4 dB high-shelf @ 1681.97 Hz,
/// Stage 2: RLB high-pass @ 38.14 Hz) and compute gated integrated loudness (LUFS) plus
/// 4× oversampled true peak (dBTP) across one or two channels.
pub fn measure_loudness_bs1770(channels: &[&[f32]], rate: u32) -> LoudnessMetrics {
    if channels.is_empty() || channels.iter().all(|c| c.is_empty()) {
        return LoudnessMetrics {
            integrated_lufs: -120.0,
            sample_peak_dbfs: -120.0,
            true_peak_dbtp: -120.0,
        };
    }
    let fs = f64::from(rate.max(8_000));
    let n = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let mut sample_peak = 0.0_f64;
    let mut true_peak = 0.0_f64;

    let mut weighted: Vec<Vec<f64>> = Vec::with_capacity(channels.len());
    for &ch in channels {
        for &s in ch {
            if s.is_finite() {
                sample_peak = sample_peak.max(f64::from(s).abs());
            }
        }
        true_peak = true_peak.max(measure_true_peak_4x_linear(ch));

        let mut shelf = Biquad2::high_shelf(fs, 1681.974450955533, 0.7071752369554196, 4.0);
        let mut rlb = Biquad2::highpass_q(fs, 38.13547087602444, 0.5003270373238773);
        let mut w_ch = Vec::with_capacity(n);
        for &s in ch.iter().take(n) {
            let v = if s.is_finite() { f64::from(s) } else { 0.0 };
            w_ch.push(rlb.tick(shelf.tick(v)));
        }
        weighted.push(w_ch);
    }

    // 400 ms gating blocks with 75% overlap (100 ms step) per ITU-R BS.1770-4
    let block_len = ((0.400 * fs).round() as usize).max(1).min(n.max(1));
    let step = ((0.100 * fs).round() as usize).max(1);
    let mut block_powers = Vec::new();
    let mut pos = 0_usize;
    while pos + block_len <= n {
        let mut z_sum = 0.0_f64;
        for ch in &weighted {
            let ms: f64 =
                ch[pos..pos + block_len].iter().map(|v| v * v).sum::<f64>() / block_len as f64;
            z_sum += ms; // L/R channel weighting G_i = 1.0
        }
        block_powers.push(z_sum);
        pos += step;
    }
    if block_powers.is_empty() && n > 0 {
        let mut z_sum = 0.0_f64;
        for ch in &weighted {
            z_sum += ch.iter().map(|v| v * v).sum::<f64>() / n as f64;
        }
        block_powers.push(z_sum);
    }

    // Absolute gate at -70 LUFS -> z_thresh_abs = 10^((-70 + 0.691) / 10)
    let abs_thresh = 10.0_f64.powf((-70.0 + 0.691) / 10.0);
    let above_abs: Vec<f64> = block_powers
        .iter()
        .copied()
        .filter(|&p| p > abs_thresh)
        .collect();

    let integrated_lufs = if above_abs.is_empty() {
        -120.0
    } else {
        let mean_abs = above_abs.iter().sum::<f64>() / above_abs.len() as f64;
        // Relative gate at -10 LU below ungated average
        let rel_thresh = mean_abs * 10.0_f64.powf(-10.0 / 10.0);
        let above_rel: Vec<f64> = above_abs
            .iter()
            .copied()
            .filter(|&p| p > rel_thresh)
            .collect();
        let final_mean = if above_rel.is_empty() {
            mean_abs
        } else {
            above_rel.iter().sum::<f64>() / above_rel.len() as f64
        };
        (-0.691 + 10.0 * final_mean.max(1e-12).log10()).clamp(-120.0, 12.0)
    };

    let to_db = |lin: f64| {
        if lin <= 1e-12 {
            -120.0
        } else {
            20.0 * lin.log10()
        }
    };

    LoudnessMetrics {
        integrated_lufs,
        sample_peak_dbfs: to_db(sample_peak),
        true_peak_dbtp: to_db(true_peak),
    }
}

/// Deterministic Triangular-PDF (TPDF) dithered 24-bit signed integer quantizer
/// (`[-8388608, 8388607]`) eliminating harmonic quantization distortion on quiet fade-outs.
pub fn quantize_i24_tpdf(samples: &[f32], seed: u64) -> Vec<i32> {
    let scale = 8_388_607.0_f64;
    let mut state = seed ^ 0x9e37_79b9_7f4a_7c15;
    let mut next_u = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 11) as f64) / ((1_u64 << 53) as f64)
    };
    samples
        .iter()
        .map(|&s| {
            let v = if s.is_finite() {
                f64::from(s).clamp(-1.0, 1.0)
            } else {
                0.0
            };
            // TPDF dither in [-1.0, +1.0] LSB (difference of two uniform [0,1) variables)
            let dither_lsb = next_u() - next_u();
            (v * scale + dither_lsb)
                .round()
                .clamp(-8_388_608.0, 8_388_607.0) as i32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso9613_air_absorption_increases_with_frequency_and_distance() {
        let a_125 = iso9613_air_absorption_db_per_m(125.0, 20.0, 50.0);
        let a_1k = iso9613_air_absorption_db_per_m(1000.0, 20.0, 50.0);
        let a_4k = iso9613_air_absorption_db_per_m(4000.0, 20.0, 50.0);
        // ISO 9613-1 at 20 °C, 50% RH: ~0.0004 dB/m @ 125 Hz, ~0.005 dB/m @ 1 kHz, ~0.032 dB/m @ 4 kHz
        assert!(a_125 > 0.0 && a_125 < a_1k && a_1k < a_4k);
        assert!(
            (a_4k - 0.0328).abs() < 0.008,
            "4 kHz ISO 9613-1 unexpected: {a_4k}"
        );

        let g_zero = air_absorption_amplitude_6band(0.0, 20.0, 50.0);
        assert_eq!(g_zero, [1.0; 6]);
        let g_30m = air_absorption_amplitude_6band(30.0, 20.0, 50.0);
        assert!(g_30m[5] < g_30m[0] && g_30m[5] > 0.5);
    }

    #[test]
    fn jca_porous_reflection_returns_none_when_parameters_missing_and_valid_when_present() {
        let missing = BiotParameters::default();
        assert!(jca_reflection_magnitude(1000.0, 0.0, Some(50.0), &missing).is_none());

        // Typical 50 mm fiberglass (Owens Corning 703 class) measured JCA parameters
        let fiberglass = BiotParameters {
            porosity: Some(0.97),
            airflow_resistivity_pa_s_m2: Some(25_000.0),
            tortuosity: Some(1.06),
            vcl_um: Some(56.0),
            tcl_um: Some(119.0),
            standard_ref: "ASTM E1050".into(),
            status: "Measured".into(),
        };
        let r_125 = jca_reflection_magnitude(125.0, 0.0, Some(50.8), &fiberglass).unwrap();
        let r_2000 = jca_reflection_magnitude(2000.0, 0.0, Some(50.8), &fiberglass).unwrap();
        // Porous 2-inch fiberglass reflects much more at 125 Hz than at 2 kHz
        assert!(
            r_125 > r_2000,
            "expected r_125 ({r_125}) > r_2000 ({r_2000})"
        );
        assert!((0.0..=1.0).contains(&r_125) && (0.0..=1.0).contains(&r_2000));
    }

    #[test]
    fn proximity_effect_is_unity_for_omni_and_boosts_low_freq_for_close_cardioid() {
        let rate = 48_000_u32;
        let low_tone: Vec<f32> = (0..4800)
            .map(|i| (0.25 * (TAU * 80.0 * i as f64 / f64::from(rate)).sin()) as f32)
            .collect();

        let mut omni = ProximityFilter::new(rate, Pattern::Omni, 0.15, 343.0);
        assert_eq!(omni.apply_slice(&low_tone), low_tone);

        let mut card_close = ProximityFilter::new(rate, Pattern::Cardioid, 0.15, 343.0);
        let mut card_far = ProximityFilter::new(rate, Pattern::Cardioid, 2.0, 343.0);
        let y_close = card_close.apply_slice(&low_tone);
        let y_far = card_far.apply_slice(&low_tone);

        let rms = |s: &[f32]| {
            (s[1200..]
                .iter()
                .map(|&v| f64::from(v) * f64::from(v))
                .sum::<f64>()
                / (s.len() - 1200) as f64)
                .sqrt()
        };
        assert!(
            rms(&y_close) > rms(&y_far) * 1.15,
            "close cardioid 80 Hz should show stronger proximity boost than far cardioid"
        );
    }

    #[test]
    fn minimum_phase_fir_tracks_digitized_fr_curve() {
        // Synthetic +6 dB presence shelf above 3 kHz
        let pts: Vec<[f32; 2]> = vec![
            [40.0, 0.0],
            [100.0, 0.0],
            [250.0, 0.0],
            [500.0, 0.0],
            [1000.0, 0.0],
            [2000.0, 1.0],
            [4000.0, 6.0],
            [8000.0, 6.0],
            [14000.0, 0.0],
        ];
        let fir = minimum_phase_fir_from_fr_points(&pts, 48_000, 128).expect("valid FIR");
        assert_eq!(fir.len(), 128);
        assert!(fir.iter().all(|v| v.is_finite()));

        // Verify gain at 4 kHz exceeds gain at 250 Hz
        let tone = |hz: f64| -> Vec<f32> {
            (0..4096)
                .map(|i| (0.2 * (TAU * hz * i as f64 / 48_000.0).sin()) as f32)
                .collect()
        };
        let y_low = apply_fir(&tone(250.0), &fir);
        let y_hi = apply_fir(&tone(4000.0), &fir);
        let rms = |s: &[f32]| {
            (s[512..]
                .iter()
                .map(|&v| f64::from(v) * f64::from(v))
                .sum::<f64>()
                / (s.len() - 512) as f64)
                .sqrt()
        };
        assert!(
            rms(&y_hi) > rms(&y_low) * 1.5,
            "minimum-phase FIR should boost 4 kHz relative to 250 Hz"
        );
    }

    #[test]
    fn fdn_late_room_decays_cleanly_and_returns_none_for_anechoic_room() {
        let anechoic = [[0.98; 6]; 6];
        assert!(LateRoomFdn::from_room_physics(
            48_000,
            [5.0, 5.0, 3.0],
            &anechoic,
            343.0,
            1.0,
            0.02
        )
        .is_none());

        let live = [[0.12; 6]; 6];
        let mut fdn =
            LateRoomFdn::from_room_physics(48_000, [8.0, 6.0, 3.5], &live, 343.0, 1.0, 0.025)
                .expect("live room FDN");
        let mut out = vec![0.0_f64; 48_000];
        for (i, dst) in out.iter_mut().enumerate() {
            *dst = fdn.tick(if i == 0 { 1.0 } else { 0.0 });
        }
        assert!(out.iter().all(|v| v.is_finite()));
        let early_energy: f64 = out[2000..12_000].iter().map(|v| v * v).sum();
        let late_energy: f64 = out[36_000..48_000].iter().map(|v| v * v).sum();
        assert!(
            early_energy > 1e-6,
            "FDN should produce diffuse tail energy"
        );
        assert!(
            late_energy < early_energy,
            "FDN energy must decay passively over time"
        );
    }

    #[test]
    fn stiff_string_dispersion_and_lagrange4_weights_are_exact() {
        assert_eq!(stiff_string_ratio(5.0, 0.0), 5.0);
        assert!(stiff_string_ratio(10.0, 3.5e-4) > 10.017);
        assert_eq!(lagrange4_weights(0.0), [0.0, 1.0, 0.0, 0.0]);
        let w = lagrange4_weights(0.37);
        let sum: f64 = w.iter().sum();
        assert!((sum - 1.0).abs() < 1e-12);
    }

    #[test]
    fn stereo_fdn_is_decorrelated_and_passive() {
        let live = [[0.14; 6]; 6];
        let mut fdn =
            LateRoomFdn::from_room_physics(48_000, [8.0, 6.0, 3.5], &live, 343.0, 1.0, 0.025)
                .expect("live room FDN");
        let mut l = vec![0.0_f64; 24_000];
        let mut r = vec![0.0_f64; 24_000];
        for i in 0..24_000 {
            let [vl, vr] = fdn.tick_stereo(if i == 0 { 1.0 } else { 0.0 });
            l[i] = vl;
            r[i] = vr;
        }
        let el: f64 = l.iter().map(|v| v * v).sum();
        let er: f64 = r.iter().map(|v| v * v).sum();
        let cross: f64 = l.iter().zip(r.iter()).map(|(a, b)| a * b).sum();
        let corr = (cross / (el * er).sqrt().max(1e-12)).abs();
        assert!(el > 1e-6 && er > 1e-6);
        assert!(
            corr < 0.35,
            "stereo FDN L/R should be well decorrelated, got corr={corr}"
        );
    }

    #[test]
    fn lr4_six_band_crossover_isolates_bands_with_flat_sum() {
        let mut xover = Lr4SixBandSplit::new(48_000);
        let rate = 48_000.0_f64;
        let mut b0_e = 0.0_f64;
        let mut b5_e = 0.0_f64;
        let mut sum_e = 0.0_f64;
        let mut in_e = 0.0_f64;
        for i in 0..4800 {
            let x = (TAU * 125.0 * i as f64 / rate).sin();
            let bands = xover.split(x);
            if i >= 1200 {
                b0_e += bands[0] * bands[0];
                b5_e += bands[5] * bands[5];
                let s: f64 = bands.iter().sum();
                sum_e += s * s;
                in_e += x * x;
            }
        }
        assert!(
            b0_e > b5_e * 100.0,
            "LR4 125 Hz tone should isolate cleanly into band 0"
        );
        assert!(
            (sum_e / in_e - 1.0).abs() < 0.05,
            "LR4 band sum energy should be flat"
        );
    }

    #[test]
    fn preamp_stage_is_bit_identical_at_zero_drive_and_zero_on_silence() {
        let sig: Vec<f32> = (0..1024)
            .map(|i| (0.25 * (TAU * 440.0 * i as f64 / 48_000.0).sin()) as f32)
            .collect();
        let mut bypassed = PreampStage::new(48_000, 0.0);
        assert_eq!(bypassed.apply_slice(&sig), sig);

        let silence = vec![0.0_f32; 512];
        let mut driven = PreampStage::new(48_000, 0.65);
        assert_eq!(driven.apply_slice(&silence), silence);
        assert_ne!(driven.apply_slice(&sig), sig);
    }

    #[test]
    fn bs1770_loudness_4x_true_peak_and_tpdf_dither_are_accurate() {
        // 1 kHz sine at -20 dBFS RMS (amp = 0.1 * sqrt(2)) -> ~-20.0 LUFS
        let amp = 0.1 * 2.0_f64.sqrt();
        let tone: Vec<f32> = (0..48_000)
            .map(|i| (amp * (TAU * 1000.0 * i as f64 / 48_000.0).sin()) as f32)
            .collect();
        let m = measure_loudness_bs1770(&[&tone], 48_000);
        assert!(
            (m.integrated_lufs - (-20.0)).abs() < 1.2,
            "1 kHz -20 dBFS RMS should read ~-20 LUFS, got {}",
            m.integrated_lufs
        );

        // Inter-sample peak test: Nyquist/2 signal sampled at +/-45° has sample peak 0.7071 but true peak ~1.0
        let isp: Vec<f32> = (0..64)
            .map(|i| (TAU * 0.25 * i as f64 + PI * 0.25).sin() as f32)
            .collect();
        let tp = measure_true_peak_4x_linear(&isp);
        assert!(
            tp > 0.90,
            "4x true peak should exceed 0.707 sample peak, got {tp}"
        );

        let q = quantize_i24_tpdf(&[0.0, 0.5, -0.5, 1.0, -1.0], 42);
        assert!(q[0].abs() <= 1);
        assert!((q[1] - 4_194_304).abs() <= 2);
        assert_eq!(q[3], 8_388_607);
    }
}
