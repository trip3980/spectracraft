//! Bedroom-to-Studio Acoustic Translation Layer (Pre-Analysis Conditioning).
//!
//! Strictly conditions a noisy, reverberant home/bedroom recording ONLY for
//! Stage 1 cue extraction (`analyze_guitar` + source-free spectral-envelope
//! trajectory tracking). Original bedroom PCM samples are NEVER copied into or
//! blended with Stage 1 dry output (`SPECTRACRAFT_SPEC.md` invariant).
//!
//! Scientific foundations:
//! - Lower-quantile / minimum-statistics stationary noise PSD estimation (Martin 2001)
//! - Decision-directed Ephraim-Malah Wiener spectral gain smoothing (Ephraim & Malah 1984)
//! - Causal exponential late-reverberation PSD suppression (Habets / WPE-inspired spectral gating)
//! - Real-cepstrum liftering for source-free vocal-tract / body spectral-envelope extraction

use crate::{
    reperform::{analyze_guitar, Performance},
    Result,
};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

/// Measured diagnostic profile of the incoming raw bedroom/field recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BedroomProfile {
    pub raw_rms_dbfs: f64,
    pub noise_floor_dbfs: f64,
    pub estimated_snr_db: f64,
    pub mains_hum_hz: Option<f64>,
    pub estimated_room_rt60_s: Option<f64>,
    pub spectral_tilt_db_per_oct: f64,
    pub conditioned_snr_gain_db: f64,
}

/// Source-free macro spectral-envelope cues extracted per analysis frame via real-cepstrum
/// liftering (tracks dynamic instrument body / vocal-tract resonances and attack energy
/// without retaining any original audio waveform or phase).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpectralEnvelopeFrame {
    pub time_s: f64,
    pub centroid_hz: f64,
    pub tilt_db_per_oct: f64,
    /// Top 3 smoothed spectral-envelope resonance peaks (Hz) in [120, 6000] Hz.
    pub resonance_peaks_hz: [f64; 3],
    /// Normalized positive spectral-flux onset sharpness (0.0..=1.0).
    pub attack_flux: f64,
}

/// Complete translated cue packet derived from a noisy/reverberant bedroom recording.
/// Contains zero original PCM samples; ready to drive Stage 1 dry studio synthesis.
#[derive(Clone, Serialize, Deserialize)]
pub struct TranslatedPerformance {
    pub performance: Performance,
    pub bedroom_profile: BedroomProfile,
    pub envelope_trajectory: Vec<SpectralEnvelopeFrame>,
}

fn rms_dbfs(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return -120.0;
    }
    let sum_sq: f64 = samples
        .iter()
        .map(|&s| {
            let v = if s.is_finite() { f64::from(s) } else { 0.0 };
            v * v
        })
        .sum();
    let rms = (sum_sq / samples.len() as f64).sqrt();
    if rms <= 1e-12 {
        -120.0
    } else {
        20.0 * rms.log10()
    }
}

/// Detect prominent stationary 50 Hz or 60 Hz AC mains hum using narrowband
/// Hann-windowed DFT probes across 0.25 s blocks (4 Hz resolution at any sample rate).
fn detect_mains_hum_hz(input: &[f32], fs: f64) -> Option<f64> {
    let block_len = ((fs * 0.25).round() as usize).clamp(512, input.len());
    if block_len < 512 {
        return None;
    }
    let hop = (block_len / 2).max(1);
    let win: Vec<f64> = (0..block_len)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / block_len as f64).cos()))
        .collect();
    let win_sum: f64 = win.iter().sum::<f64>().max(1.0);

    let probe_power = |slice: &[f32], freq_hz: f64| -> f64 {
        let w = TAU * freq_hz / fs;
        let mut re = 0.0_f64;
        let mut im = 0.0_f64;
        for (i, &s) in slice.iter().enumerate() {
            let v = if s.is_finite() {
                f64::from(s) * win[i]
            } else {
                0.0
            };
            let (sn, cs) = (w * i as f64).sin_cos();
            re += v * cs;
            im -= v * sn;
        }
        (re * re + im * im) / (win_sum * win_sum)
    };

    let check_candidate = |target_hz: f64| -> Option<f64> {
        let mut peaks = Vec::new();
        let mut flanks = Vec::new();
        let mut block_powers = Vec::new();
        let mut start = 0usize;
        while start + block_len <= input.len() {
            let seg = &input[start..start + block_len];
            let p = probe_power(seg, target_hz);
            let f_lo = probe_power(seg, target_hz - 12.0);
            let f_hi = probe_power(seg, target_hz + 12.0);
            let blk_p = seg
                .iter()
                .map(|&s| {
                    let v = if s.is_finite() { f64::from(s) } else { 0.0 };
                    v * v
                })
                .sum::<f64>()
                / block_len as f64;
            peaks.push(p);
            flanks.push(0.5 * (f_lo + f_hi));
            block_powers.push(blk_p);
            start += hop;
        }
        if peaks.is_empty() {
            return None;
        }
        peaks.sort_by(f64::total_cmp);
        flanks.sort_by(f64::total_cmp);
        block_powers.sort_by(f64::total_cmp);
        let q = ((peaks.len() as f64 * 0.15).floor() as usize).min(peaks.len() - 1);
        let active_q =
            ((block_powers.len() as f64 * 0.85).floor() as usize).min(block_powers.len() - 1);
        let peak = peaks[q];
        let flank = flanks[q].max(1e-18);
        let active_power = block_powers[active_q].max(1e-18);
        // Hann-windowed single-tone probe power is A^2 / 4, so equivalent sine RMS power is 2 * peak.
        // Real AC mains hum is a background interferer (< 25% of active signal power); a dominant
        // 50/60 Hz musical tone accounts for most of the active power and must not be notched.
        let hum_rms_power = 2.0 * peak;
        let is_background_hum = hum_rms_power < active_power * 0.25;
        let ratio_db = 10.0 * (peak / flank).max(1e-12).log10();
        (is_background_hum && ratio_db >= 10.0 && peak > 1e-8).then_some(ratio_db)
    };

    match (check_candidate(50.0), check_candidate(60.0)) {
        (Some(a), Some(b)) => Some(if a >= b { 50.0 } else { 60.0 }),
        (Some(_), None) => Some(50.0),
        (None, Some(_)) => Some(60.0),
        (None, None) => None,
    }
}

/// Apply a narrow 2nd-order IIR notch filter to remove stationary mains hum + harmonics
/// on the internal analysis buffer.
fn apply_notch_inplace(buf: &mut [f64], rate: f64, center_hz: f64, q: f64) {
    if center_hz <= 10.0 || center_hz >= rate * 0.45 {
        return;
    }
    let w0 = TAU * center_hz / rate;
    let cos_w = w0.cos();
    let alpha = w0.sin() / (2.0 * q.max(1.0));
    let a0 = 1.0 + alpha;
    let b0 = 1.0 / a0;
    let b1 = (-2.0 * cos_w) / a0;
    let b2 = 1.0 / a0;
    let a1 = (-2.0 * cos_w) / a0;
    let a2 = (1.0 - alpha) / a0;
    let (mut x1, mut x2, mut y1, mut y2) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for s in buf.iter_mut() {
        let x0 = *s;
        let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
        *s = y0;
    }
}

/// High-resolution, low-sidelobe (4-term Blackman-Harris, -92 dB sidelobes) stationary
/// noise-floor and 85th-percentile active-frame power estimator.
///
/// Using a 4096-point window at <=48 kHz (8192 at >48 kHz, gracefully reduced on short clips)
/// resolves closely spaced musical notes without Hann sidelobe leakage contaminating adjacent
/// bins during continuous legato sustain, while preserving unbiased Parseval-scaled calibration
/// on broadband Gaussian noise (`min(q12 / -ln(0.88), q50 / ln(2))` per bin).
fn estimate_stationary_noise_and_active_power(input: &[f32], rate: u32) -> (f64, f64) {
    let n = input.len();
    let target_win = if rate > 48_000 {
        8192_usize
    } else {
        4096_usize
    };
    let mut win_len = target_win;
    while win_len > 1024 && n < win_len * 4 {
        win_len /= 2;
    }
    if n < win_len {
        let p = input
            .iter()
            .map(|&s| {
                let v = if s.is_finite() { f64::from(s) } else { 0.0 };
                v * v
            })
            .sum::<f64>()
            / (n.max(1) as f64);
        let safe = p.max(1e-18);
        return (safe, safe);
    }
    let hop = (win_len / 2).max(1);
    let n_bins = win_len / 2 + 1;
    let n_frames = (n - win_len) / hop + 1;

    // 4-term Blackman-Harris window (-92 dB highest sidelobe)
    let win: Vec<f64> = (0..win_len)
        .map(|i| {
            let phase = TAU * i as f64 / win_len as f64;
            0.35875 - 0.48829 * phase.cos() + 0.14128 * (2.0 * phase).cos()
                - 0.01168 * (3.0 * phase).cos()
        })
        .collect();
    let win_pow_sum: f64 = win.iter().map(|w| w * w).sum::<f64>().max(1.0);

    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(win_len);
    let mut fft_buf = vec![Complex::new(0.0_f64, 0.0_f64); win_len];

    let mut frame_powers: Vec<Vec<f64>> = Vec::with_capacity(n_frames);
    let mut frame_energies: Vec<f64> = Vec::with_capacity(n_frames);
    for f_idx in 0..n_frames {
        let start = f_idx * hop;
        for i in 0..win_len {
            let s = input[start + i];
            let v = if s.is_finite() { f64::from(s) } else { 0.0 };
            fft_buf[i] = Complex::new(v * win[i], 0.0);
        }
        fft.process(&mut fft_buf);
        let mut psd = vec![0.0_f64; n_bins];
        let mut total_e = 0.0_f64;
        for k in 0..n_bins {
            let scale = if k == 0 || k == n_bins - 1 { 1.0 } else { 2.0 };
            let p = scale * fft_buf[k].norm_sqr() / (win_len as f64 * win_pow_sum);
            psd[k] = p;
            total_e += p;
        }
        frame_energies.push(total_e);
        frame_powers.push(psd);
    }

    let q12_idx = ((n_frames as f64 * 0.12).floor() as usize).min(n_frames - 1);
    let q50_idx = (n_frames / 2).min(n_frames - 1);
    let active_idx = ((n_frames as f64 * 0.85).floor() as usize).min(n_frames - 1);

    let exp_q12_factor = -(1.0_f64 - 0.12_f64).ln(); // ~0.127833
    let exp_q50_factor = std::f64::consts::LN_2; // ~0.693147

    let mut col = vec![0.0_f64; n_frames];
    let mut unbiased_stationary_e = 0.0_f64;
    for k in 0..n_bins {
        for (fi, row) in frame_powers.iter().enumerate() {
            col[fi] = row[k];
        }
        col.sort_by(f64::total_cmp);
        let est_q12 = col[q12_idx] / exp_q12_factor;
        let est_q50 = col[q50_idx] / exp_q50_factor;
        unbiased_stationary_e += est_q12.min(est_q50);
    }

    let mut sorted_e = frame_energies;
    sorted_e.sort_by(f64::total_cmp);
    let frame_quiet_e = sorted_e[q12_idx].max(1e-18);
    let quiet_e = frame_quiet_e
        .min(unbiased_stationary_e.max(1e-18))
        .max(1e-18);
    let active_e = sorted_e[active_idx].max(quiet_e);
    (quiet_e, active_e)
}

/// Condition a noisy, reverberant bedroom recording into a clean internal analysis buffer
/// so cue extraction (`analyze_guitar` + spectral envelope tracking) sees the dry performance
/// rather than room boxiness, fan hiss, or AC hum.
///
/// The returned buffer is ONLY for cue analysis and must NEVER be written as Stage 1 output.
pub fn condition_bedroom_for_analysis(input: &[f32], rate: u32) -> (Vec<f32>, BedroomProfile) {
    let n = input.len();
    let fs = f64::from(rate.max(8_000));
    if n < 512 {
        let raw_db = rms_dbfs(input);
        return (
            input.to_vec(),
            BedroomProfile {
                raw_rms_dbfs: raw_db,
                noise_floor_dbfs: raw_db,
                estimated_snr_db: 0.0,
                mains_hum_hz: None,
                estimated_room_rt60_s: None,
                spectral_tilt_db_per_oct: 0.0,
                conditioned_snr_gain_db: 0.0,
            },
        );
    }

    let raw_rms_dbfs = rms_dbfs(input);

    // 75%-overlap Hann STFT (win = 1024 @ <=48 kHz, 2048 @ >48 kHz)
    let win_len = if rate > 48_000 {
        2048_usize
    } else {
        1024_usize
    };
    let hop = win_len / 4;
    let n_bins = win_len / 2 + 1;
    let bin_hz = fs / (win_len as f64);

    let win: Vec<f64> = (0..win_len)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / win_len as f64).cos()))
        .collect();
    let win_pow_sum: f64 = win.iter().map(|w| w * w).sum::<f64>().max(1.0);

    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(win_len);
    let ifft = planner.plan_fft_inverse(win_len);

    // First pass: frame power spectra for noise-floor & mains-hum estimation
    let n_frames = (n.saturating_sub(win_len)) / hop + 1;
    let mut frame_powers: Vec<Vec<f64>> = Vec::with_capacity(n_frames);
    let mut frame_energies: Vec<f64> = Vec::with_capacity(n_frames);
    let mut fft_buf = vec![Complex::new(0.0_f64, 0.0_f64); win_len];

    for f_idx in 0..n_frames {
        let start = f_idx * hop;
        for i in 0..win_len {
            let s = input.get(start + i).copied().unwrap_or(0.0);
            let clean = if s.is_finite() { f64::from(s) } else { 0.0 };
            fft_buf[i] = Complex::new(clean * win[i], 0.0);
        }
        fft.process(&mut fft_buf);
        let mut psd = vec![0.0_f64; n_bins];
        let mut total_e = 0.0_f64;
        for k in 0..n_bins {
            let p = fft_buf[k].norm_sqr() / (win_len as f64 * win_pow_sum);
            psd[k] = p;
            total_e += p;
        }
        frame_energies.push(total_e);
        frame_powers.push(psd);
    }

    // Martin-inspired lower-percentile (12th percentile) noise PSD estimator across frames
    let mut noise_psd = vec![0.0_f64; n_bins];
    let q_idx = ((n_frames as f64 * 0.12).floor() as usize).min(n_frames.saturating_sub(1));
    let mut col = vec![0.0_f64; n_frames];
    for k in 0..n_bins {
        for (fi, row) in frame_powers.iter().enumerate() {
            col[fi] = row[k];
        }
        col.sort_by(f64::total_cmp);
        noise_psd[k] = col[q_idx];
    }

    let _ = bin_hz;
    let mains_hum_hz = detect_mains_hum_hz(input, fs);

    // Optional clean notch pre-pass for detected 50/60 Hz hum + 2nd/3rd harmonics
    let mut work: Vec<f64> = input
        .iter()
        .map(|&s| if s.is_finite() { f64::from(s) } else { 0.0 })
        .collect();
    if let Some(hum) = mains_hum_hz {
        for harmonic in [1.0, 2.0, 3.0] {
            apply_notch_inplace(&mut work, fs, hum * harmonic, 22.0);
        }
    }

    // Estimate signal-vs-noise ratio from high-resolution Blackman-Harris stationary noise floor.
    // This eliminates Hann sidelobe leakage across closely spaced legato notes while staying
    // Parseval-calibrated on real broadband Gaussian noise and stationary mains hum.
    let (quiet_e, active_e) = estimate_stationary_noise_and_active_power(input, rate);
    let noise_floor_dbfs = (10.0 * quiet_e.log10()).clamp(-120.0, 0.0);
    let estimated_snr_db = (10.0 * (active_e / quiet_e).log10()).clamp(0.0, 90.0);

    // Estimate bedroom late decay time from sharpest downward energy slopes
    // (frame_energies uses half-spectrum scaling without the 2x one-sided multiplier).
    let stft_quiet_e = (quiet_e * 0.5).max(1e-18);
    let hop_s = hop as f64 / fs;
    let mut decay_rates = Vec::new();
    for i in 1..frame_energies.len() {
        let prev = frame_energies[i - 1];
        let cur = frame_energies[i];
        if prev > stft_quiet_e * 8.0 && cur > stft_quiet_e * 2.0 && cur < prev * 0.96 {
            let db_drop = 10.0 * (prev / cur).log10();
            if db_drop > 0.25 && db_drop < 12.0 {
                decay_rates.push(db_drop / hop_s);
            }
        }
    }
    let estimated_room_rt60_s = if decay_rates.len() >= 4 {
        decay_rates.sort_by(f64::total_cmp);
        let med_db_s = decay_rates[decay_rates.len() / 2];
        Some((60.0 / med_db_s.max(5.0)).clamp(0.06, 2.5))
    } else {
        None
    };

    // Second pass: Decision-directed Ephraim-Malah Wiener denoise + causal late-reverb suppression
    let mut out = vec![0.0_f64; n];
    let mut norm = vec![0.0_f64; n];
    let mut prev_clean_psd = vec![0.0_f64; n_bins];
    let mut reverb_psd = vec![0.0_f64; n_bins];

    let dd_alpha = 0.92_f64; // Ephraim-Malah decision-directed smoothing
    let rev_decay = (-hop_s / 0.14_f64).exp(); // ~140 ms bedroom late-tail tracker
    let rev_weight = 0.38_f64; // conservative dereverb weight so true dry sustain survives
    let gain_floor = 0.06_f64; // -24 dB floor prevents musical-noise birdies
    let inv_n = 1.0 / (win_len as f64);
    // Only subtract the 12th-percentile noise PSD when the clip actually contains quiet pauses
    // (active-to-quiet ratio >= 3 dB); otherwise a continuous musical tone/drone with no gaps
    // would be misclassified as stationary noise and crushed to gain_floor.
    let noise_weight = if estimated_snr_db >= 3.0 { 1.35 } else { 0.0 };

    // Track average spectral tilt across active frames
    let mut low_band_sum = 0.0_f64;
    let mut high_band_sum = 0.0_f64;

    let mut start_i = -(win_len as isize) + hop as isize;
    while start_i < n as isize {
        for i in 0..win_len {
            let idx = start_i + i as isize;
            let s = if idx >= 0 && (idx as usize) < n {
                work[idx as usize]
            } else {
                0.0
            };
            fft_buf[i] = Complex::new(s * win[i], 0.0);
        }
        fft.process(&mut fft_buf);

        for k in 0..n_bins {
            let freq = k as f64 * bin_hz;
            let obs_psd = fft_buf[k].norm_sqr() / (win_len as f64 * win_pow_sum);
            // Total unwanted interference = stationary noise + decayed late bedroom reverb
            let interf_psd = (noise_weight * noise_psd[k] + rev_weight * reverb_psd[k]).max(1e-18);
            let post_snr = (obs_psd / interf_psd).max(1.0);
            let prior_snr = dd_alpha * (prev_clean_psd[k] / interf_psd)
                + (1.0 - dd_alpha) * (post_snr - 1.0).max(0.0);
            let wiener = (prior_snr / (1.0 + prior_snr)).clamp(gain_floor, 1.0);

            // Gentle subsonic HVAC rumble high-pass below 42 Hz
            let rumble_cut = if freq < 42.0 {
                (freq / 42.0).powi(2).clamp(0.05, 1.0)
            } else {
                1.0
            };
            let g = wiener * rumble_cut;
            fft_buf[k] *= g;
            if k > 0 && k < win_len / 2 {
                fft_buf[win_len - k] *= g;
            }

            let clean_psd = obs_psd * g * g;
            prev_clean_psd[k] = clean_psd;
            // Update causal delayed reverb energy estimate
            reverb_psd[k] = rev_decay * reverb_psd[k] + (1.0 - rev_decay) * clean_psd * 0.35;

            if (150.0..=600.0).contains(&freq) {
                low_band_sum += clean_psd;
            } else if (1200.0..=4800.0).contains(&freq) {
                high_band_sum += clean_psd;
            }
        }

        ifft.process(&mut fft_buf);
        for i in 0..win_len {
            let idx = start_i + i as isize;
            if idx >= 0 && (idx as usize) < n {
                let u = idx as usize;
                let w = win[i];
                out[u] += fft_buf[i].re * inv_n * w;
                norm[u] += w * w;
            }
        }
        start_i += hop as isize;
    }

    let mut conditioned = vec![0.0_f32; n];
    for i in 0..n {
        let v = if norm[i] > 1e-12 {
            out[i] / norm[i]
        } else {
            0.0
        };
        conditioned[i] = if v.is_finite() { v as f32 } else { 0.0 };
    }

    // Measure post-conditioning noise-floor improvement with the same calibrated estimator
    let (cond_quiet, cond_active) = estimate_stationary_noise_and_active_power(&conditioned, rate);
    let cond_snr_db = (10.0 * (cond_active / cond_quiet).log10()).clamp(0.0, 90.0);

    // 3-octave span from ~300 Hz to ~2400 Hz
    let spectral_tilt_db_per_oct = if low_band_sum > 1e-18 && high_band_sum > 1e-18 {
        (10.0 * (high_band_sum / low_band_sum).log10()) / 3.0
    } else {
        0.0
    };

    (
        conditioned,
        BedroomProfile {
            raw_rms_dbfs,
            noise_floor_dbfs,
            estimated_snr_db,
            mains_hum_hz,
            estimated_room_rt60_s,
            spectral_tilt_db_per_oct,
            conditioned_snr_gain_db: (cond_snr_db - estimated_snr_db).max(0.0),
        },
    )
}

/// Extract source-free macro spectral-envelope trajectory (centroid, tilt, top 3 cepstrally
/// liftered resonance/formant peaks, and onset spectral flux) from a conditioned analysis buffer.
pub fn extract_spectral_envelope_trajectory(
    analysis_audio: &[f32],
    rate: u32,
    hop_s: f64,
) -> Vec<SpectralEnvelopeFrame> {
    let n = analysis_audio.len();
    let fs = f64::from(rate.max(8_000));
    let win_len = 1024_usize;
    let hop = ((hop_s.clamp(0.002, 0.05) * fs).round() as usize).max(64);
    if n < win_len {
        return Vec::new();
    }

    let n_bins = win_len / 2 + 1;
    let bin_hz = fs / (win_len as f64);
    let win: Vec<f64> = (0..win_len)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / win_len as f64).cos()))
        .collect();

    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(win_len);
    let ifft = planner.plan_fft_inverse(win_len);
    let inv_n = 1.0 / (win_len as f64);

    // Low-quefrency lifter cutoff (~1.5 ms -> separates vocal/body resonances from F0 harmonics)
    let lifter_quef = ((0.0015 * fs).round() as usize).clamp(16, 80);

    let mut frames = Vec::new();
    let mut prev_mag = vec![0.0_f64; n_bins];
    let mut buf = vec![Complex::new(0.0_f64, 0.0_f64); win_len];
    let mut cep = vec![Complex::new(0.0_f64, 0.0_f64); win_len];

    let mut start = 0_usize;
    while start + win_len <= n {
        for i in 0..win_len {
            let s = f64::from(analysis_audio[start + i]);
            buf[i] = Complex::new(if s.is_finite() { s * win[i] } else { 0.0 }, 0.0);
        }
        fft.process(&mut buf);

        let mut mag_sum = 0.0_f64;
        let mut weighted_f = 0.0_f64;
        let mut flux = 0.0_f64;
        let mut low_e = 0.0_f64;
        let mut high_e = 0.0_f64;

        for k in 0..n_bins {
            let m = buf[k].norm();
            let f = k as f64 * bin_hz;
            mag_sum += m;
            weighted_f += m * f;
            let diff = (m - prev_mag[k]).max(0.0);
            flux += diff;
            prev_mag[k] = m;

            if (150.0..=600.0).contains(&f) {
                low_e += m * m;
            } else if (1200.0..=4800.0).contains(&f) {
                high_e += m * m;
            }
            let log_m = Complex::new(m.max(1e-6).ln(), 0.0);
            cep[k] = log_m;
            if k > 0 && k < win_len / 2 {
                cep[win_len - k] = log_m;
            }
        }

        // Cepstral smoothing -> smooth log-spectral envelope (strips harmonic comb!)
        ifft.process(&mut cep);
        for c in &mut cep {
            *c *= inv_n;
        }
        for q in 0..win_len {
            let dist = q.min(win_len - q);
            if dist > lifter_quef {
                cep[q] = Complex::new(0.0, 0.0);
            } else if dist > lifter_quef / 2 {
                let t = (dist - lifter_quef / 2) as f64
                    / ((lifter_quef - lifter_quef / 2).max(1) as f64);
                let w = 0.5 * (1.0 + (PI * t).cos());
                cep[q] *= w;
            }
        }
        fft.process(&mut cep);

        // Locate top 3 smooth spectral-envelope peaks in [120, 6000] Hz
        let mut peaks: Vec<(f64, f64)> = Vec::new();
        let k_min = ((120.0 / bin_hz).ceil() as usize).max(2);
        let k_max = ((6000.0 / bin_hz).floor() as usize).min(n_bins.saturating_sub(2));
        for k in k_min..=k_max {
            let v = cep[k].re;
            if v > cep[k - 1].re && v >= cep[k + 1].re {
                peaks.push((v, k as f64 * bin_hz));
            }
        }
        peaks.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut top3 = [0.0_f64; 3];
        let mut chosen: Vec<f64> = peaks.iter().take(3).map(|p| p.1).collect();
        chosen.sort_by(f64::total_cmp);
        for (i, &hz) in chosen.iter().enumerate() {
            top3[i] = hz;
        }

        let centroid_hz = if mag_sum > 1e-9 {
            (weighted_f / mag_sum).clamp(20.0, fs * 0.45)
        } else {
            0.0
        };
        let tilt_db_per_oct = if low_e > 1e-18 && high_e > 1e-18 {
            (10.0 * (high_e / low_e).log10()) / 3.0
        } else {
            0.0
        };
        let attack_flux = if mag_sum > 1e-9 {
            (flux / mag_sum).clamp(0.0, 1.0)
        } else {
            0.0
        };

        frames.push(SpectralEnvelopeFrame {
            time_s: (start + win_len / 2) as f64 / fs,
            centroid_hz,
            tilt_db_per_oct,
            resonance_peaks_hz: top3,
            attack_flux,
        });
        start += hop;
    }

    frames
}

/// Full bedroom-to-studio translation entry point:
/// 1. Conditions noisy/reverberant bedroom recording strictly inside an internal analysis buffer.
/// 2. Extracts clean, source-free `Performance` cues + dynamic `SpectralEnvelopeFrame` trajectory.
/// 3. Discards the original bedroom audio completely (zero original samples retained).
pub fn analyze_from_bedroom_recording(input: &[f32], rate: u32) -> Result<TranslatedPerformance> {
    let (conditioned, bedroom_profile) = condition_bedroom_for_analysis(input, rate);
    let performance = analyze_guitar(&conditioned, rate)?;
    let envelope_trajectory =
        extract_spectral_envelope_trajectory(&conditioned, rate, performance.hop_s());
    Ok(TranslatedPerformance {
        performance,
        bedroom_profile,
        envelope_trajectory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bedroom_conditioning_suppresses_stationary_noise_and_detects_60hz_hum() {
        let rate = 48_000_u32;
        let fs = f64::from(rate);
        let n = rate as usize * 2; // 2 seconds: quiet pause + 220 Hz tone + quiet tail
        let mut seed = 0x1234_5678_9abc_def0_u64;
        let mut next_white = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (((seed >> 33) as f64) / (u32::MAX as f64)) * 2.0 - 1.0
        };

        let mut noisy = vec![0.0_f32; n];
        for (i, dst) in noisy.iter_mut().enumerate() {
            let t = i as f64 / fs;
            let hum = 0.035 * (TAU * 60.0 * t).sin();
            let hiss = 0.018 * next_white();
            let note = if (0.4..1.4).contains(&t) {
                0.32 * (TAU * 220.0 * t).sin() + 0.14 * (TAU * 440.0 * t).sin()
            } else {
                0.0
            };
            *dst = (note + hum + hiss) as f32;
        }

        let (cleaned, profile) = condition_bedroom_for_analysis(&noisy, rate);
        assert_eq!(cleaned.len(), noisy.len());
        assert_eq!(profile.mains_hum_hz, Some(60.0));
        assert!(
            profile.conditioned_snr_gain_db > 4.0,
            "expected meaningful SNR improvement, got {} dB",
            profile.conditioned_snr_gain_db
        );

        let raw_quiet_rms = rms_dbfs(&noisy[0..14_000]);
        let clean_quiet_rms = rms_dbfs(&cleaned[0..14_000]);
        assert!(
            clean_quiet_rms < raw_quiet_rms - 6.0,
            "quiet pause should be >6 dB quieter after bedroom conditioning ({clean_quiet_rms} vs {raw_quiet_rms})"
        );
    }

    #[test]
    fn spectral_envelope_extractor_tracks_resonance_peaks_source_free() {
        let rate = 48_000_u32;
        let fs = f64::from(rate);
        // Synthetic resonant tone emphasizing ~1500 Hz
        let sig: Vec<f32> = (0..12_000)
            .map(|i| {
                let t = i as f64 / fs;
                (0.15 * (TAU * 200.0 * t).sin()
                    + 0.45 * (TAU * 1400.0 * t).sin()
                    + 0.45 * (TAU * 1600.0 * t).sin()) as f32
            })
            .collect();
        let env = extract_spectral_envelope_trajectory(&sig, rate, 0.01);
        assert!(!env.is_empty());
        let mid = env[env.len() / 2];
        assert!(
            mid.centroid_hz > 900.0 && mid.centroid_hz < 2000.0,
            "centroid should reflect dominant 1.5 kHz cluster: {}",
            mid.centroid_hz
        );
        assert!(mid
            .resonance_peaks_hz
            .iter()
            .any(|&p| (1100.0..=1900.0).contains(&p)));
    }

    #[test]
    fn stationary_snr_estimator_is_calibrated_across_clean_and_noisy_sustaining_melodies() {
        let rate = 48_000_u32;
        let fs = f64::from(rate);
        let n = rate as usize * 3;
        let freqs = [196.0, 220.0, 246.94, 293.66, 329.63, 392.0];
        let mut clean = vec![0.0_f32; n];
        for i in 0..n {
            let t = i as f64 / fs;
            let step = ((t / 0.5).floor() as usize).min(freqs.len() - 1);
            let local_t = t - step as f64 * 0.5;
            let env = (-local_t / 0.35).exp();
            let f0 = freqs[step];
            clean[i] =
                (0.35 * env * ((TAU * f0 * t).sin() + 0.4 * (TAU * 2.0 * f0 * t).sin())) as f32;
        }

        // 1. Clean noiseless legato melody must not suffer sidelobe leakage false noise floor
        let (_, clean_prof) = condition_bedroom_for_analysis(&clean, rate);
        assert!(
            clean_prof.estimated_snr_db >= 40.0,
            "clean noiseless melody should report >= 40 dB SNR, got {:.1} dB (floor {:.1} dBFS)",
            clean_prof.estimated_snr_db,
            clean_prof.noise_floor_dbfs
        );

        // 2. Calibrated broadband white noise at two known amplitudes (-33.9 dBFS & -48.9 dBFS)
        let add_white = |amp: f64| -> Vec<f32> {
            let mut rng = 0x1234_5678_9abc_def0_u64;
            clean
                .iter()
                .map(|&s| {
                    rng = rng
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let u = (rng >> 32) as u32;
                    let w = (f64::from(u) / f64::from(u32::MAX)) * 2.0 - 1.0;
                    s + (w * amp) as f32
                })
                .collect()
        };

        let (_, noisy_20) = condition_bedroom_for_analysis(&add_white(0.035), rate);
        assert!(
            (17.5..=24.5).contains(&noisy_20.estimated_snr_db),
            "~20.6 dB true SNR melody should estimate within [17.5, 24.5] dB, got {:.1} dB",
            noisy_20.estimated_snr_db
        );

        let (_, noisy_35) = condition_bedroom_for_analysis(&add_white(0.0062), rate);
        assert!(
            noisy_35.estimated_snr_db > noisy_20.estimated_snr_db + 11.0
                && noisy_35.estimated_snr_db < 42.0,
            "15 dB quieter noise should raise estimated SNR linearly (~35 dB), got {:.1} dB vs {:.1} dB",
            noisy_35.estimated_snr_db,
            noisy_20.estimated_snr_db
        );
    }
}
