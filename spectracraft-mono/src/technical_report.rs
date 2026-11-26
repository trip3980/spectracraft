//! Blind Before-and-After Technical Report Engine + Dark-Mode PDF/Markdown Export.
//!
//! Architectural guarantees:
//! - `analyze_before_blind(raw, rate)` inspects ONLY the raw input buffer (or Bedroom
//!   Translator private copy) with zero access to the final export or pipeline settings.
//! - `analyze_after_blind(final_out, rate)` inspects ONLY the final exported buffer with
//!   zero access to the Before report or raw input samples.
//! - `build_comparison_summary(&before, &after)` runs strictly AFTER both blind reports
//!   exist and lists only significant numeric deltas without narrative invention.
//! - Stage 1 dry physics, additive re-pick, and continuous-bend behavior are never altered.
//! - All `storytold/craft-libs` calls (`craft-pdf`, `craft-atomic`) are isolated inside
//!   the thin `crate::report_export` compatibility layer so future pin bumps affect only
//!   that module.

use crate::{acoustics_physics, research_diagnostics, translation_layer, Result};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;
use std::fmt::Write as _;

pub use crate::report_export::{export_report, export_report_markdown, export_report_pdf_craft};

/// Standard ISO 28 one-third-octave center frequencies (Hz) spanning 25 Hz .. 16 kHz.
pub const THIRD_OCTAVE_CENTERS_HZ: [f64; 28] = [
    25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0,
    630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0,
    10000.0, 12500.0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceTag {
    Measured,
    Inferred,
}

impl EvidenceTag {
    pub fn label(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::Inferred => "inferred",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReportFinding {
    pub category: String,
    pub label: String,
    pub value: String,
    pub tag: EvidenceTag,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpectralBalance {
    pub centroid_hz: f64,
    pub tilt_db_per_oct: f64,
    pub low_band_dbfs: f64,
    pub mid_band_dbfs: f64,
    pub presence_band_dbfs: f64,
    pub air_band_dbfs: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HarmonicSignature {
    pub fundamental_range_hz: [f64; 2],
    pub median_f0_hz: f64,
    pub harmonic_to_noise_ratio_db: f64,
    pub odd_even_harmonic_ratio_db: f64,
    pub mean_attack_sharpness: f64,
    pub mean_decay_rate_db_per_s: f64,
    pub spectral_rolloff_95_hz: f64,
    pub harmonic_richness_partials: f64,
    pub voiced_frame_fraction: f64,
    pub harmonic_profile_summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InstrumentSignature {
    pub inferred_family: String,
    pub display_name: String,
    pub confidence: f64,
    pub detected_foreign_signatures: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpatialSignature {
    pub direct_to_reverberant_ratio_db: f64,
    pub early_reflection_delay_ms: Option<f64>,
    pub estimated_mic_distance_ft: Option<f64>,
    pub estimated_source_height_ft: Option<f64>,
    pub spatial_assessment: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoomSignature {
    pub estimated_rt60_s: Option<f64>,
    pub hf_damping_ratio_db: f64,
    pub early_reflection_energy_db: f64,
    pub inferred_room_type: String,
    pub inferred_material_class: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MicSignature {
    pub sub_55hz_rolloff_db: f64,
    pub low_mid_warmth_200hz_db: f64,
    pub presence_3_to_6khz_db: f64,
    pub hf_rolloff_above_10khz_db: f64,
    pub effective_bandwidth_hz: f64,
    pub sm7_signature_score: f64,
    pub matches_sm7_class: bool,
    pub inferred_mic_model: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlindSignatureProfile {
    pub instrument: InstrumentSignature,
    pub harmonic: HarmonicSignature,
    pub spatial: SpatialSignature,
    pub room: RoomSignature,
    pub mic: MicSignature,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureConditionsSpec {
    pub source_height_ft: f64,
    pub source_centered: bool,
    pub mic_model: String,
    pub mic_distance_ft: f64,
    pub room_dimensions_ft: [f64; 3],
    pub room_material: String,
}

impl CaptureConditionsSpec {
    /// Standard initial capture conditions: 5 ft source height at center of a
    /// 10 × 10 × 10 ft padded room, Shure SM7 at 2 ft distance.
    pub fn sm7_padded_10ft_reference() -> Self {
        Self {
            source_height_ft: 5.0,
            source_centered: true,
            mic_model: "Shure SM7".into(),
            mic_distance_ft: 2.0,
            room_dimensions_ft: [10.0, 10.0, 10.0],
            room_material: "padded".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignatureCheckItem {
    pub id: String,
    pub label: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignatureVerificationReport {
    pub generated_after_both_blind_reports: bool,
    pub expected_conditions: CaptureConditionsSpec,
    pub no_foreign_instrument_signatures: bool,
    pub original_archive_signatures_absent: bool,
    pub applied_sm7_and_padded_room_match: bool,
    pub harmonic_content_consistent: bool,
    pub all_passed: bool,
    pub checks: Vec<SignatureCheckItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeforeReport {
    pub blind_scope: String,
    pub sample_rate: u32,
    pub samples: usize,
    pub duration_s: f64,
    pub rms_dbfs: f64,
    pub sample_peak_dbfs: f64,
    pub true_peak_dbtp: f64,
    pub integrated_lufs: f64,
    pub noise_floor_dbfs: f64,
    pub estimated_snr_db: f64,
    pub dynamic_range_db: f64,
    pub crest_factor_db: f64,
    pub dc_offset: f64,
    pub dc_offset_dbfs: f64,
    pub dc_present: bool,
    pub clipped_samples: usize,
    pub clipping_present: bool,
    pub hum_50hz_db: f64,
    pub hum_60hz_db: f64,
    pub detected_mains_hum_hz: Option<f64>,
    pub stationary_noise_ratio: f64,
    pub spectral_balance: SpectralBalance,
    pub third_octave_dbfs: Vec<f64>,
    pub body_100hz_prominence_db: f64,
    pub body_200hz_prominence_db: f64,
    pub upper_band_shimmer_index: f64,
    pub upper_band_phase_coherence: f64,
    pub dropout_count: usize,
    pub click_count: usize,
    pub estimated_rt60_s: Option<f64>,
    pub mic_room_signature: String,
    #[serde(default)]
    pub signatures: BlindSignatureProfile,
    pub parseval_verified: bool,
    pub parseval_max_rel_err: f64,
    pub findings: Vec<ReportFinding>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AfterReport {
    pub blind_scope: String,
    pub sample_rate: u32,
    pub samples: usize,
    pub duration_s: f64,
    pub rms_dbfs: f64,
    pub sample_peak_dbfs: f64,
    pub true_peak_dbtp: f64,
    pub integrated_lufs: f64,
    pub residual_noise_dbfs: f64,
    pub estimated_snr_db: f64,
    pub dynamic_range_db: f64,
    pub crest_factor_db: f64,
    pub dc_offset: f64,
    pub dc_offset_dbfs: f64,
    pub dc_present: bool,
    pub clipped_samples: usize,
    pub clipping_present: bool,
    pub hum_50hz_db: f64,
    pub hum_60hz_db: f64,
    pub detected_mains_hum_hz: Option<f64>,
    pub stationary_noise_ratio: f64,
    pub spectral_balance: SpectralBalance,
    pub third_octave_dbfs: Vec<f64>,
    pub body_100hz_prominence_db: f64,
    pub body_200hz_prominence_db: f64,
    pub body_resonance_visible: bool,
    pub upper_band_shimmer_index: f64,
    pub upper_band_phase_coherence: f64,
    pub dropout_count: usize,
    pub click_count: usize,
    pub room_tail_rt60_s: Option<f64>,
    pub tail_decay_dbfs: f64,
    pub defects_summary: String,
    #[serde(default)]
    pub signatures: BlindSignatureProfile,
    pub parseval_verified: bool,
    pub parseval_max_rel_err: f64,
    pub findings: Vec<ReportFinding>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeltaItem {
    pub metric: String,
    pub summary: String,
    pub before_value: String,
    pub after_value: String,
    pub delta_numeric: Option<f64>,
    pub unit: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComparisonSummary {
    pub generated_after_both_blind_reports: bool,
    pub deltas: Vec<DeltaItem>,
    pub body_resonance_peaks_noted: bool,
    pub no_unexpected_dropouts_or_clicks: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThirdOctavePlot {
    pub centers_hz: Vec<f32>,
    pub before_db: Vec<f32>,
    pub after_db: Vec<f32>,
    pub delta_db: Vec<f32>,
    pub db_range: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UpperBandDiagnosticPlot {
    pub band_hz: [f32; 2],
    pub times_s: Vec<f32>,
    pub before_phase_coherence: Vec<f32>,
    pub after_phase_coherence: Vec<f32>,
    pub before_modulation_depth: Vec<f32>,
    pub after_modulation_depth: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReportViewerPlots {
    pub sample_rate: u32,
    pub duration_s: f64,
    pub max_hz: f32,
    pub waveform_scale_linear: f32,
    pub waveform_bins: usize,
    pub waveform_before: Vec<[f32; 2]>,
    pub waveform_after: Vec<[f32; 2]>,
    pub residual_waveform: Vec<[f32; 2]>,
    pub spectrogram_cols: usize,
    pub spectrogram_rows: usize,
    pub spectrogram_freq_hz: Vec<f32>,
    pub spectrogram_db_range: [f32; 2],
    pub diff_db_range: [f32; 2],
    /// Flattened row-major `[row * cols + col]`, row 0 = lowest frequency (`20 Hz`).
    pub spectrogram_before_db: Vec<f32>,
    pub spectrogram_after_db: Vec<f32>,
    pub spectrogram_diff_db: Vec<f32>,
    pub third_octave: ThirdOctavePlot,
    pub upper_band: UpperBandDiagnosticPlot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftLibsIntegration {
    pub crates_used: Vec<String>,
    pub unavailable_fallbacks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TechnicalReport {
    pub schema_version: u32,
    pub title: String,
    pub pipeline_label: String,
    pub before: BeforeReport,
    pub after: AfterReport,
    pub comparison: ComparisonSummary,
    pub plots: ReportViewerPlots,
    pub whitepaper_markdown: String,
    pub craft_libs: CraftLibsIntegration,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportReceipt {
    pub requested_format: String,
    pub actual_format: String,
    pub extension: String,
    pub content_type: String,
    pub engine: String,
    pub fallback_used: bool,
    pub fallback_note: Option<String>,
    pub bytes: usize,
    pub sha256: String,
}

// ======================= BLIND ACOUSTIC ANALYSIS CORE =======================

struct BlindMetrics {
    sample_rate: u32,
    samples: usize,
    duration_s: f64,
    rms_dbfs: f64,
    sample_peak_dbfs: f64,
    true_peak_dbtp: f64,
    integrated_lufs: f64,
    noise_floor_dbfs: f64,
    estimated_snr_db: f64,
    dynamic_range_db: f64,
    crest_factor_db: f64,
    dc_offset: f64,
    dc_offset_dbfs: f64,
    dc_present: bool,
    clipped_samples: usize,
    clipping_present: bool,
    hum_50hz_db: f64,
    hum_60hz_db: f64,
    detected_mains_hum_hz: Option<f64>,
    stationary_noise_ratio: f64,
    spectral_balance: SpectralBalance,
    third_octave_dbfs: Vec<f64>,
    body_100hz_prominence_db: f64,
    body_200hz_prominence_db: f64,
    upper_band_shimmer_index: f64,
    upper_band_phase_coherence: f64,
    dropout_count: usize,
    click_count: usize,
    estimated_rt60_s: Option<f64>,
    tail_decay_dbfs: f64,
    signatures: BlindSignatureProfile,
    parseval_verified: bool,
    parseval_max_rel_err: f64,
}

fn db_from_power(p: f64) -> f64 {
    if !p.is_finite() || p <= 1e-24 {
        -120.0
    } else {
        (10.0 * p.log10()).clamp(-120.0, 24.0)
    }
}

fn db_from_amp(a: f64) -> f64 {
    if !a.is_finite() || a <= 1e-12 {
        -120.0
    } else {
        (20.0 * a.log10()).clamp(-120.0, 24.0)
    }
}

fn probe_narrowband_tone_db(samples: &[f32], fs: f64, freq_hz: f64) -> f64 {
    if samples.len() < 512 || freq_hz <= 0.0 || freq_hz >= fs * 0.48 {
        return -120.0;
    }
    let block_len = ((fs * 0.25).round() as usize).clamp(512, samples.len());
    let hop = (block_len / 2).max(1);
    let win: Vec<f64> = (0..block_len)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / block_len as f64).cos()))
        .collect();
    let win_sum: f64 = win.iter().sum::<f64>().max(1.0);
    let w = TAU * freq_hz / fs;

    let mut powers = Vec::new();
    let mut start = 0usize;
    while start + block_len <= samples.len() {
        let mut re = 0.0_f64;
        let mut im = 0.0_f64;
        for i in 0..block_len {
            let s = samples[start + i];
            let v = if s.is_finite() {
                f64::from(s) * win[i]
            } else {
                0.0
            };
            let (sn, cs) = (w * i as f64).sin_cos();
            re += v * cs;
            im -= v * sn;
        }
        // Hann-windowed sine RMS power = 2 * |X|^2 / (sum w)^2
        let rms_power = 2.0 * (re * re + im * im) / (win_sum * win_sum);
        powers.push(rms_power);
        start += hop;
    }
    if powers.is_empty() {
        return -120.0;
    }
    // Lower-quartile (20th percentile) measures persistent stationary hum rather than transient sweeps.
    powers.sort_by(f64::total_cmp);
    let idx = ((powers.len() as f64 * 0.20).floor() as usize).min(powers.len() - 1);
    db_from_power(powers[idx])
}

fn measure_single_signal_blind(samples: &[f32], rate: u32) -> Result<BlindMetrics> {
    if samples.is_empty() || !(8_000..=192_000).contains(&rate) {
        return Err("Technical report requires non-empty mono audio at 8–192 kHz".into());
    }
    let fs = f64::from(rate);
    let n = samples.len();
    let duration_s = n as f64 / fs;

    // 1. Time-domain DC, RMS, peak, clipping
    let mut sum = 0.0_f64;
    let mut sum_sq = 0.0_f64;
    let mut peak = 0.0_f64;
    let mut clipped_samples = 0usize;
    for &s in samples {
        let v = if s.is_finite() { f64::from(s) } else { 0.0 };
        sum += v;
        sum_sq += v * v;
        let av = v.abs();
        if av > peak {
            peak = av;
        }
        if av >= 0.999 {
            clipped_samples += 1;
        }
    }
    let dc_offset = sum / n as f64;
    let dc_offset_dbfs = db_from_amp(dc_offset.abs());
    let dc_present = dc_offset.abs() >= 1.5e-3;
    let rms = (sum_sq / n as f64).sqrt();
    let rms_dbfs = db_from_amp(rms);
    let sample_peak_dbfs = db_from_amp(peak);
    let crest_factor_db = (sample_peak_dbfs - rms_dbfs).max(0.0);
    let clipping_present = clipped_samples >= 3 || peak >= 0.9995;

    // 2. ITU-R BS.1770-4 integrated loudness & 4x oversampled true peak
    let loudness = acoustics_physics::measure_loudness_bs1770(&[samples], rate);
    let true_peak_dbtp = loudness.true_peak_dbtp;
    let integrated_lufs = loudness.integrated_lufs;

    // 3. Bedroom Translator blind profile (noise floor, hum detection, RT60 estimate)
    let (_, bedroom_profile) = translation_layer::condition_bedroom_for_analysis(samples, rate);
    let hum_50hz_db = probe_narrowband_tone_db(samples, fs, 50.0);
    let hum_60hz_db = probe_narrowband_tone_db(samples, fs, 60.0);

    // 4. Short-frame envelope for dynamic range, dropouts, clicks, and tail decay
    let frame_len = ((0.020 * fs).round() as usize).clamp(128, n);
    let frame_hop = (frame_len / 2).max(1);
    let mut frame_rms_db = Vec::new();
    let mut pos = 0usize;
    while pos + frame_len <= n {
        let ms = samples[pos..pos + frame_len]
            .iter()
            .map(|&s| {
                let v = if s.is_finite() { f64::from(s) } else { 0.0 };
                v * v
            })
            .sum::<f64>()
            / frame_len as f64;
        frame_rms_db.push(db_from_power(ms));
        pos += frame_hop;
    }
    if frame_rms_db.is_empty() {
        frame_rms_db.push(rms_dbfs);
    }

    let mut sorted_frames = frame_rms_db.clone();
    sorted_frames.sort_by(f64::total_cmp);
    let p10 = sorted_frames
        [((sorted_frames.len() as f64 * 0.10).floor() as usize).min(sorted_frames.len() - 1)];
    let p95 = sorted_frames
        [((sorted_frames.len() as f64 * 0.95).floor() as usize).min(sorted_frames.len() - 1)];
    let active_thresh = (p95 - 30.0).max(-72.0);
    let active_sorted: Vec<f64> = sorted_frames
        .iter()
        .copied()
        .filter(|&v| v >= active_thresh)
        .collect();
    let dynamic_range_db = if active_sorted.len() >= 4 {
        let lo = active_sorted[active_sorted.len() / 10];
        let hi = active_sorted[(active_sorted.len() * 95) / 100];
        (hi - lo).max(0.0)
    } else {
        (p95 - p10).max(0.0)
    };

    // Dropout detection: sudden abrupt > 24 dB drop out of a steady sustained region followed
    // by an abrupt recovery to a comparable level (distinguishes genuine buffer/splice dropouts
    // from natural musical rests where a plucked note decays smoothly downward before silence).
    let mut dropout_count = 0usize;
    if frame_rms_db.len() >= 7 {
        let mut i = 3usize;
        while i + 3 < frame_rms_db.len() {
            let l_prev = frame_rms_db[i - 3];
            let l_edge = frame_rms_db[i - 2];
            let r_edge = frame_rms_db[i + 2];
            let r_next = frame_rms_db[i + 3];
            let left = l_prev.min(l_edge);
            let right = r_edge.min(r_next);
            let anchor = left.min(right);
            // A genuine buffer/splice dropout interrupts a steady sustained region and resumes
            // at a comparable steady level, rather than smoothly fading monotonically into a
            // musical rest before a brand-new plucked note attack.
            let pre_steady = (l_prev - l_edge).abs() <= 2.5;
            let post_steady = (r_next - r_edge).abs() <= 3.0;
            let post_matched = (r_edge - l_edge).abs() <= 6.0;
            let monotonic_fade = l_prev > l_edge + 1.0
                && l_edge > frame_rms_db[i - 1] + 2.0
                && frame_rms_db[i - 1] > frame_rms_db[i] + 2.0;
            let abrupt_drop = frame_rms_db[i] < anchor - 24.0;
            if anchor > active_thresh + 8.0
                && pre_steady
                && post_steady
                && post_matched
                && !monotonic_fade
                && abrupt_drop
            {
                dropout_count += 1;
                i += 3;
                continue;
            }
            i += 1;
        }
    }

    // Click / impulse transient detection using 2nd-difference ratio against local RMS
    // (ignores smooth musical transients and high-frequency sine waves by normalizing against
    // local derivative envelope in a 10 ms neighborhood).
    let mut click_count = 0usize;
    if n >= 64 {
        let d2: Vec<f64> = (1..n - 1)
            .map(|i| {
                let a = f64::from(samples[i - 1]);
                let b = f64::from(samples[i]);
                let c = f64::from(samples[i + 1]);
                (c - 2.0 * b + a).abs()
            })
            .collect();
        let block = ((0.010 * fs).round() as usize).clamp(64, d2.len());
        let mut b_start = 0usize;
        while b_start + block <= d2.len() {
            let slice = &d2[b_start..b_start + block];
            let mean = slice.iter().sum::<f64>() / block as f64;
            let max_v = slice.iter().copied().fold(0.0_f64, f64::max);
            // A genuine single-sample digital click has peak 2nd-diff >> 22x local mean & > 0.08 full scale
            if max_v > 0.08 && max_v > mean.max(1e-5) * 22.0 {
                click_count += 1;
            }
            b_start += block;
        }
    }

    // Tail decay (last 5% of signal vs peak active RMS)
    let tail_start = (frame_rms_db.len() * 95) / 100;
    let tail_decay_dbfs = if tail_start < frame_rms_db.len() {
        frame_rms_db[tail_start..].iter().sum::<f64>()
            / (frame_rms_db.len() - tail_start).max(1) as f64
    } else {
        rms_dbfs
    };

    // 5. STFT spectral balance, 1/3-octave spectrum, stationary noise ratio, shimmer & upper-band coherence
    let nfft = if rate > 48_000 {
        4096_usize
    } else {
        2048_usize
    };
    let hop = nfft / 2;
    let n_bins = nfft / 2 + 1;
    let bin_hz = fs / nfft as f64;
    let win: Vec<f64> = (0..nfft)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / nfft as f64).cos()))
        .collect();
    let win_pow_sum: f64 = win.iter().map(|w| w * w).sum::<f64>().max(1.0);
    let fft = FftPlanner::<f64>::new().plan_fft_forward(nfft);
    let mut fft_buf = vec![Complex::new(0.0_f64, 0.0_f64); nfft];

    let n_frames = if n >= nfft { (n - nfft) / hop + 1 } else { 1 };
    let mut avg_psd = vec![0.0_f64; n_bins];
    let mut frame_hi_env = Vec::with_capacity(n_frames);
    let mut prev_phase = vec![0.0_f64; n_bins];
    let mut prev_dphase = vec![0.0_f64; n_bins];
    let mut coherence_sum = 0.0_f64;
    let mut coherence_frames = 0usize;

    let hi_lo = ((4_000.0 / bin_hz).ceil() as usize).clamp(1, n_bins - 1);
    let hi_hi = ((16_000.0 / bin_hz).floor() as usize).clamp(hi_lo, n_bins - 1);

    for f_idx in 0..n_frames {
        let start = f_idx * hop;
        for i in 0..nfft {
            let s = samples.get(start + i).copied().unwrap_or(0.0);
            let v = if s.is_finite() { f64::from(s) } else { 0.0 };
            fft_buf[i] = Complex::new(v * win[i], 0.0);
        }
        fft.process(&mut fft_buf);
        let mut hi_e = 0.0_f64;
        let mut coh_re = 0.0_f64;
        let mut coh_im = 0.0_f64;
        let mut coh_w = 0.0_f64;

        for k in 0..n_bins {
            let mag2 = fft_buf[k].norm_sqr();
            let scale = if k == 0 || k == n_bins - 1 { 1.0 } else { 2.0 };
            let p = scale * mag2 / (nfft as f64 * win_pow_sum);
            avg_psd[k] += p;

            if k >= hi_lo && k <= hi_hi {
                hi_e += p;
                let ph = fft_buf[k].arg();
                if f_idx >= 1 {
                    let mut dp = ph - prev_phase[k];
                    while dp > std::f64::consts::PI {
                        dp -= TAU;
                    }
                    while dp < -std::f64::consts::PI {
                        dp += TAU;
                    }
                    if f_idx >= 2 {
                        let ddp = dp - prev_dphase[k];
                        let w_bin = p.sqrt();
                        coh_re += w_bin * ddp.cos();
                        coh_im += w_bin * ddp.sin();
                        coh_w += w_bin;
                    }
                    prev_dphase[k] = dp;
                }
                prev_phase[k] = ph;
            }
        }
        frame_hi_env.push(hi_e.sqrt());
        if f_idx >= 2 && coh_w > 1e-10 {
            coherence_sum += (coh_re * coh_re + coh_im * coh_im).sqrt() / coh_w;
            coherence_frames += 1;
        }
    }

    for v in &mut avg_psd {
        *v /= n_frames.max(1) as f64;
    }

    // Band energies (sum of PSD power inside each frequency band -> dBFS)
    let band_power = |f_lo: f64, f_hi: f64| -> f64 {
        let mut total = 0.0_f64;
        for (k, &p) in avg_psd.iter().enumerate().skip(1) {
            let hz = k as f64 * bin_hz;
            if hz >= f_lo && hz < f_hi {
                total += p;
            }
        }
        total
    };
    let low_band_dbfs = db_from_power(band_power(20.0, 250.0));
    let mid_band_dbfs = db_from_power(band_power(250.0, 2_000.0));
    let presence_band_dbfs = db_from_power(band_power(2_000.0, 6_000.0));
    let air_band_dbfs = db_from_power(band_power(6_000.0, (fs * 0.49).min(20_000.0)));

    // Spectral centroid
    let mut cent_num = 0.0_f64;
    let mut cent_den = 0.0_f64;
    for (k, &p) in avg_psd.iter().enumerate().skip(1) {
        let hz = k as f64 * bin_hz;
        cent_num += hz * p;
        cent_den += p;
    }
    let centroid_hz = if cent_den > 1e-24 {
        cent_num / cent_den
    } else {
        0.0
    };

    // 1/3-octave band levels (dBFS)
    let third_oct_factor = 2.0_f64.powf(1.0 / 6.0);
    let mut third_octave_dbfs = Vec::with_capacity(THIRD_OCTAVE_CENTERS_HZ.len());
    for &fc in &THIRD_OCTAVE_CENTERS_HZ {
        let f_lo = fc / third_oct_factor;
        let f_hi = fc * third_oct_factor;
        let mut p_sum = 0.0_f64;
        let mut hits = 0usize;
        for (k, &p) in avg_psd.iter().enumerate().skip(1) {
            let hz = k as f64 * bin_hz;
            if hz >= f_lo && hz <= f_hi {
                p_sum += p;
                hits += 1;
            }
        }
        if hits == 0 {
            let nearest = ((fc / bin_hz).round() as usize).clamp(1, n_bins - 1);
            p_sum = avg_psd[nearest];
        }
        third_octave_dbfs.push(db_from_power(p_sum));
    }

    // Body resonance prominence at 100 Hz (index 6) and 200 Hz (index 9) relative to flanking
    // 1/3-octave bands (strictly blind — computed solely on this single buffer!).
    // 100 Hz flanks: 63 Hz (idx 4) & 160 Hz (idx 8)
    // 200 Hz flanks: 160 Hz (idx 8) & 315 Hz (idx 11)
    let flank_100 = 0.5 * (third_octave_dbfs[4] + third_octave_dbfs[8]);
    let body_100hz_prominence_db = third_octave_dbfs[6] - flank_100;
    let flank_200 = 0.5 * (third_octave_dbfs[8] + third_octave_dbfs[11]);
    let body_200hz_prominence_db = third_octave_dbfs[9] - flank_200;

    // Upper-band shimmer / modulation index (coefficient of variation of 4-16 kHz amplitude envelope)
    let active_hi: Vec<f64> = frame_hi_env.iter().copied().filter(|&v| v > 1e-6).collect();
    let upper_band_shimmer_index = if active_hi.len() >= 4 {
        let mean = active_hi.iter().sum::<f64>() / active_hi.len() as f64;
        let var = active_hi
            .iter()
            .map(|&v| (v - mean) * (v - mean))
            .sum::<f64>()
            / active_hi.len() as f64;
        (var.sqrt() / mean.max(1e-12)).clamp(0.0, 2.0)
    } else {
        0.0
    };
    let upper_band_phase_coherence = if coherence_frames > 0 {
        (coherence_sum / coherence_frames as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // Stationary noise ratio: ratio of 12th-percentile noise floor energy to total RMS energy
    let stationary_noise_ratio = 10.0_f64
        .powf((bedroom_profile.noise_floor_dbfs - rms_dbfs) / 10.0)
        .clamp(0.0, 1.0);

    // Parseval energy conservation check (diagnostic only — never modifies audio)
    let p_len = 4096_usize.min(n).max(16);
    let p_start = (n / 2).saturating_sub(p_len / 2);
    let p_slice = &samples[p_start..(p_start + p_len).min(n)];
    let (parseval_verified, parseval_max_rel_err) =
        match research_diagnostics::check_rustfft_real_frame_f32(p_slice) {
            Ok((full, one_sided)) => (
                full.passes && one_sided.passes,
                full.relative_error
                    .unwrap_or(0.0)
                    .max(one_sided.relative_error.unwrap_or(0.0)),
            ),
            Err(_) => (false, 1.0),
        };

    // 6. Strictly blind acoustic signature extraction (instrument, harmonic, spatial, room, mic)
    let signatures = extract_blind_signatures(
        samples,
        rate,
        &avg_psd,
        bin_hz,
        &third_octave_dbfs,
        &frame_rms_db,
        active_thresh,
        bedroom_profile.noise_floor_dbfs,
        bedroom_profile.estimated_snr_db,
        bedroom_profile.estimated_room_rt60_s,
        stationary_noise_ratio,
        click_count,
        upper_band_phase_coherence,
        body_100hz_prominence_db,
        body_200hz_prominence_db,
        low_band_dbfs,
        mid_band_dbfs,
        presence_band_dbfs,
        air_band_dbfs,
        centroid_hz,
        bedroom_profile.spectral_tilt_db_per_oct,
    );

    Ok(BlindMetrics {
        sample_rate: rate,
        samples: n,
        duration_s,
        rms_dbfs,
        sample_peak_dbfs,
        true_peak_dbtp,
        integrated_lufs,
        noise_floor_dbfs: bedroom_profile.noise_floor_dbfs,
        estimated_snr_db: bedroom_profile.estimated_snr_db,
        dynamic_range_db,
        crest_factor_db,
        dc_offset,
        dc_offset_dbfs,
        dc_present,
        clipped_samples,
        clipping_present,
        hum_50hz_db,
        hum_60hz_db,
        detected_mains_hum_hz: bedroom_profile.mains_hum_hz,
        stationary_noise_ratio,
        spectral_balance: SpectralBalance {
            centroid_hz,
            tilt_db_per_oct: bedroom_profile.spectral_tilt_db_per_oct,
            low_band_dbfs,
            mid_band_dbfs,
            presence_band_dbfs,
            air_band_dbfs,
        },
        third_octave_dbfs,
        body_100hz_prominence_db,
        body_200hz_prominence_db,
        upper_band_shimmer_index,
        upper_band_phase_coherence,
        dropout_count,
        click_count,
        estimated_rt60_s: bedroom_profile.estimated_room_rt60_s,
        tail_decay_dbfs,
        signatures,
        parseval_verified,
        parseval_max_rel_err,
    })
}

/// Strictly blind single-buffer acoustic signature extractor.
/// Inspects ONLY the provided buffer and its own blind metrics — zero access to
/// pipeline settings, target rig specs, or any other audio buffer.
#[allow(clippy::too_many_arguments)]
fn extract_blind_signatures(
    samples: &[f32],
    rate: u32,
    avg_psd: &[f64],
    bin_hz: f64,
    third_octave_dbfs: &[f64],
    frame_rms_db: &[f64],
    active_thresh: f64,
    noise_floor_dbfs: f64,
    estimated_snr_db: f64,
    estimated_rt60_s: Option<f64>,
    stationary_noise_ratio: f64,
    click_count: usize,
    upper_band_phase_coherence: f64,
    body_100hz_prominence_db: f64,
    body_200hz_prominence_db: f64,
    low_band_dbfs: f64,
    mid_band_dbfs: f64,
    presence_band_dbfs: f64,
    air_band_dbfs: f64,
    centroid_hz: f64,
    spectral_tilt_db_per_oct: f64,
) -> BlindSignatureProfile {
    let fs = f64::from(rate.max(8_000));
    let n = samples.len();

    // --- A. Spectral rolloff (95% cumulative power) & effective bandwidth (-30 dB edge) ---
    let total_pow: f64 = avg_psd.iter().skip(1).sum::<f64>().max(1e-24);
    let mut cum_pow = 0.0_f64;
    let mut spectral_rolloff_95_hz = 0.0_f64;
    for (k, &p) in avg_psd.iter().enumerate().skip(1) {
        cum_pow += p;
        if cum_pow >= 0.95 * total_pow {
            spectral_rolloff_95_hz = k as f64 * bin_hz;
            break;
        }
    }
    let peak_mid_3oct = third_octave_dbfs
        .iter()
        .skip(5)
        .take(16)
        .copied()
        .fold(-120.0_f64, f64::max);
    let mut effective_bandwidth_hz = 200.0_f64;
    for (&fc, &db) in THIRD_OCTAVE_CENTERS_HZ.iter().zip(third_octave_dbfs.iter()) {
        if db >= peak_mid_3oct - 30.0 {
            effective_bandwidth_hz = fc;
        }
    }

    // --- B. Voiced frame F0, HNR, odd/even harmonic ratio, and partial richness ---
    let f0_win = ((0.046 * fs).round() as usize)
        .clamp(512, 4096)
        .min(n.max(1));
    let f0_hop = ((0.040 * fs).round() as usize).max(256);
    let min_lag = ((fs / 1_600.0).floor() as usize).max(4);
    let max_lag = ((fs / 62.0).ceil() as usize).min(f0_win.saturating_sub(2));
    let nfft_h = 2048_usize;
    let h_bin_hz = fs / nfft_h as f64;
    let fft_h = FftPlanner::<f64>::new().plan_fft_forward(nfft_h);
    let win_h: Vec<f64> = (0..nfft_h)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / nfft_h as f64).cos()))
        .collect();
    let mut h_buf = vec![Complex::new(0.0_f64, 0.0_f64); nfft_h];

    let mut f0_list: Vec<f64> = Vec::new();
    let mut hnr_list: Vec<f64> = Vec::new();
    let mut odd_even_list: Vec<f64> = Vec::new();
    let mut partials_list: Vec<f64> = Vec::new();
    let mut active_frames_checked = 0usize;

    // Sample up to 240 evenly spaced frames across the recording so analysis stays fast.
    let total_possible = if n >= f0_win {
        (n - f0_win) / f0_hop + 1
    } else {
        0
    };
    let stride = (total_possible / 240).max(1);
    let mut idx_f = 0usize;
    while idx_f < total_possible {
        let start = idx_f * f0_hop;
        let seg = &samples[start..start + f0_win];
        let mut mean = 0.0_f64;
        for &s in seg {
            if s.is_finite() {
                mean += f64::from(s);
            }
        }
        mean /= f0_win as f64;
        let mut energy = 0.0_f64;
        for &s in seg {
            let v = if s.is_finite() {
                f64::from(s) - mean
            } else {
                0.0
            };
            energy += v * v;
        }
        let rms_db = db_from_power(energy / f0_win as f64);
        if rms_db >= active_thresh + 2.0 && max_lag > min_lag + 2 {
            active_frames_checked += 1;
            // Normalized autocorrelation peak search with first-major-peak preference to avoid
            // sub-harmonic octave doubling.
            let mut best_corr = 0.0_f64;
            let mut best_lag = 0usize;
            let mut corrs = Vec::with_capacity(max_lag - min_lag + 1);
            let check_len = f0_win - max_lag;
            let mut e0 = 0.0_f64;
            for j in 0..check_len {
                let a = f64::from(seg[j]) - mean;
                e0 += a * a;
            }
            if e0 > 1e-18 {
                for lag in min_lag..=max_lag {
                    let mut num = 0.0_f64;
                    let mut e1 = 0.0_f64;
                    for j in 0..check_len {
                        let a = f64::from(seg[j]) - mean;
                        let b = f64::from(seg[j + lag]) - mean;
                        num += a * b;
                        e1 += b * b;
                    }
                    let r = num / (e0 * e1).sqrt().max(1e-18);
                    corrs.push((lag, r));
                    if r > best_corr {
                        best_corr = r;
                        best_lag = lag;
                    }
                }
                // Pick the shortest lag that is a local maximum within 88% of global peak.
                if best_corr >= 0.42 {
                    let thresh = best_corr * 0.88;
                    for w in corrs.windows(3) {
                        if w[1].1 >= thresh && w[1].1 >= w[0].1 && w[1].1 >= w[2].1 {
                            best_lag = w[1].0;
                            break;
                        }
                    }
                    let f0 = fs / best_lag as f64;
                    if (65.0..=1_550.0).contains(&f0) {
                        f0_list.push(f0);
                        // Measure harmonic-to-noise ratio and odd/even partial balance via FFT
                        for i in 0..nfft_h {
                            let s = samples.get(start + i).copied().unwrap_or(0.0);
                            let v = if s.is_finite() {
                                f64::from(s) - mean
                            } else {
                                0.0
                            };
                            h_buf[i] = Complex::new(v * win_h[i], 0.0);
                        }
                        fft_h.process(&mut h_buf);
                        let max_bin = nfft_h / 2;
                        let mut harm_e = 0.0_f64;
                        let mut valley_e = 0.0_f64;
                        let mut odd_e = 0.0_f64;
                        let mut even_e = 0.0_f64;
                        let mut active_partials = 0usize;
                        let bin_peak = |freq: f64| -> f64 {
                            let c = (freq / h_bin_hz).round() as isize;
                            let mut mx = 0.0_f64;
                            for d in -1..=1 {
                                let k = (c + d).clamp(1, max_bin as isize - 1) as usize;
                                mx = mx.max(h_buf[k].norm_sqr());
                            }
                            mx
                        };
                        let f1_pow = bin_peak(f0).max(1e-24);
                        for h_idx in 1..=8 {
                            let fh = f0 * h_idx as f64;
                            if fh >= fs * 0.42 {
                                break;
                            }
                            let hp = bin_peak(fh);
                            let vp = bin_peak(fh + 0.5 * f0);
                            harm_e += hp;
                            valley_e += vp;
                            if h_idx % 2 == 1 {
                                odd_e += hp;
                            } else {
                                even_e += hp;
                            }
                            if hp > f1_pow * 1e-3 && hp > vp * 2.0 {
                                active_partials += 1;
                            }
                        }
                        let hnr_db = 10.0 * (harm_e / valley_e.max(1e-24)).log10();
                        let oe_db = 10.0 * (odd_e.max(1e-24) / even_e.max(1e-24)).log10();
                        hnr_list.push(hnr_db.clamp(-10.0, 45.0));
                        odd_even_list.push(oe_db.clamp(-24.0, 24.0));
                        partials_list.push(active_partials as f64);
                    }
                }
            }
        }
        idx_f += stride;
    }

    let median_of = |vals: &mut Vec<f64>| -> f64 {
        if vals.is_empty() {
            return 0.0;
        }
        vals.sort_by(f64::total_cmp);
        vals[vals.len() / 2]
    };
    let percentile_of = |sorted: &[f64], p: f64| -> f64 {
        if sorted.is_empty() {
            return 0.0;
        }
        let idx = ((sorted.len() as f64 * p).floor() as usize).min(sorted.len() - 1);
        sorted[idx]
    };

    f0_list.sort_by(f64::total_cmp);
    let fundamental_range_hz = if f0_list.len() >= 3 {
        [percentile_of(&f0_list, 0.10), percentile_of(&f0_list, 0.90)]
    } else if !f0_list.is_empty() {
        [f0_list[0], *f0_list.last().unwrap()]
    } else {
        [0.0, 0.0]
    };
    let median_f0_hz = if !f0_list.is_empty() {
        f0_list[f0_list.len() / 2]
    } else {
        0.0
    };
    let harmonic_to_noise_ratio_db = median_of(&mut hnr_list);
    let odd_even_harmonic_ratio_db = median_of(&mut odd_even_list);
    let harmonic_richness_partials = median_of(&mut partials_list);
    let voiced_frame_fraction = if active_frames_checked > 0 {
        (f0_list.len() as f64 / active_frames_checked as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // --- C. Envelope attack sharpness & post-onset decay slope (plucked vs sustained vs sweep) ---
    let mut attack_rises = Vec::new();
    let mut decay_rates = Vec::new();
    let mut direct_vs_tail_diffs = Vec::new();
    let frame_step_s = 0.010_f64; // 20 ms frames with 50% overlap -> 10 ms hop
    if frame_rms_db.len() >= 14 {
        let mut i = 2usize;
        while i + 10 < frame_rms_db.len() {
            let rise = frame_rms_db[i] - frame_rms_db[i - 2];
            if frame_rms_db[i] > active_thresh + 6.0
                && rise >= 4.0
                && frame_rms_db[i] >= frame_rms_db[i - 1]
                && frame_rms_db[i] >= frame_rms_db[i + 1]
            {
                attack_rises.push(rise);
                // Measure decay slope over next 80 ms (8 frames) unless interrupted by next onset
                let mut end_k = 1usize;
                for k in 2..=8 {
                    if frame_rms_db[i + k] > frame_rms_db[i + k - 1] + 2.5 {
                        break;
                    }
                    end_k = k;
                }
                if end_k >= 3 {
                    let drop = (frame_rms_db[i] - frame_rms_db[i + end_k]).max(0.0);
                    decay_rates.push(drop / (end_k as f64 * frame_step_s));
                    // Direct peak vs early tail (30-60 ms later) gives blind DRR proxy
                    let early_tail = frame_rms_db[i + 3..=(i + end_k).min(i + 6)]
                        .iter()
                        .sum::<f64>()
                        / ((i + end_k).min(i + 6) - (i + 3) + 1) as f64;
                    direct_vs_tail_diffs.push((frame_rms_db[i] - early_tail).max(0.0));
                }
                i += 4;
                continue;
            }
            i += 1;
        }
    }
    let mean_attack_sharpness = if !attack_rises.is_empty() {
        attack_rises.iter().sum::<f64>() / attack_rises.len() as f64
    } else {
        0.0
    };
    let mean_decay_rate_db_per_s = if !decay_rates.is_empty() {
        decay_rates.iter().sum::<f64>() / decay_rates.len() as f64
    } else {
        0.0
    };

    // Detect monotonic test sine sweep / continuous single-tone signal (so sweeps are never
    // misclassified as musical instruments).
    let is_monotonic_sweep_or_tone = attack_rises.len() <= 2
        && harmonic_richness_partials <= 2.5
        && upper_band_phase_coherence >= 0.55;

    // --- D. Instrument family inference & foreign signature check ---
    let mut detected_foreign_signatures = Vec::new();
    let sub_55hz_band = 0.5 * (third_octave_dbfs[2] + third_octave_dbfs[3]); // 40 & 50 Hz
    let low_125_250 = (third_octave_dbfs[7]
        + third_octave_dbfs[8]
        + third_octave_dbfs[9]
        + third_octave_dbfs[10])
        / 4.0;
    let sub_55hz_rolloff_db = sub_55hz_band - low_125_250;

    let (inferred_family, display_name, confidence) = if is_monotonic_sweep_or_tone {
        (
            "test_sweep_or_synthetic".to_string(),
            "Test Sweep / Continuous Pure Tone".to_string(),
            0.92,
        )
    } else if voiced_frame_fraction >= 0.25
        && (70.0..=1_450.0).contains(&median_f0_hz)
        && harmonic_to_noise_ratio_db >= 4.0
        && mean_attack_sharpness >= 4.5
        && mean_decay_rate_db_per_s >= 12.0
    {
        // Plucked acoustic string / solo guitar signature: guitar pitch register, impulsive pluck
        // attacks, natural exponential string decay, harmonic series with both odd & even partials.
        let mut conf = 0.70_f64;
        if (90.0..=650.0).contains(&median_f0_hz) {
            conf += 0.12;
        }
        if odd_even_harmonic_ratio_db.abs() <= 10.0 {
            conf += 0.08;
        }
        if harmonic_richness_partials >= 3.0 {
            conf += 0.06;
        }
        (
            "plucked_string_guitar".to_string(),
            "Solo Plucked String (Acoustic Guitar)".to_string(),
            conf.min(0.96),
        )
    } else if voiced_frame_fraction >= 0.25
        && harmonic_to_noise_ratio_db >= 6.0
        && mean_decay_rate_db_per_s < 12.0
    {
        if harmonic_richness_partials <= 3.0 || odd_even_harmonic_ratio_db > 9.0 {
            (
                "woodwind_flute".to_string(),
                "Solo Woodwind / Sustained Tone".to_string(),
                0.72,
            )
        } else {
            (
                "bowed_string".to_string(),
                "Solo Bowed String".to_string(),
                0.72,
            )
        }
    } else if voiced_frame_fraction < 0.15 && mean_attack_sharpness >= 6.0 {
        (
            "percussive_unpitched".to_string(),
            "Unpitched Percussive / Transient".to_string(),
            0.70,
        )
    } else {
        (
            "unknown_or_mixed".to_string(),
            "Mixed / Unclassified Acoustic Source".to_string(),
            0.40,
        )
    };

    // Foreign signature audit relative to the identified solo instrument family
    if inferred_family == "plucked_string_guitar" {
        if sub_55hz_rolloff_db > -6.0 && sub_55hz_band > noise_floor_dbfs + 18.0 {
            detected_foreign_signatures.push(format!(
                "Excessive sub-55 Hz energy ({sub_55hz_rolloff_db:+.1} dB vs low-mid; possible bass/kick/rumble)"
            ));
        }
        if odd_even_harmonic_ratio_db > 12.0 {
            detected_foreign_signatures.push(format!(
                "Odd-only harmonic dominance ({odd_even_harmonic_ratio_db:+.1} dB; reed/woodwind anomaly)"
            ));
        }
        if fundamental_range_hz[1] > 1_800.0 {
            detected_foreign_signatures.push(format!(
                "Out-of-register upper fundamental ({:.0} Hz)",
                fundamental_range_hz[1]
            ));
        }
        if air_band_dbfs > mid_band_dbfs - 4.0 && upper_band_phase_coherence < 0.15 {
            detected_foreign_signatures.push(
                "Incoherent high-band broadband energy (cymbal/hiss/percussive artifact)".into(),
            );
        }
    }

    let harmonic_profile_summary = if is_monotonic_sweep_or_tone {
        "Single-frequency continuous sweep / tonal test stimulus".to_string()
    } else if inferred_family == "plucked_string_guitar" {
        format!(
            "Harmonic plucked-string series (median F0 {:.1} Hz, range {:.0}–{:.0} Hz, HNR {:.1} dB, odd/even {:+.1} dB, ~{:.1} partials)",
            median_f0_hz,
            fundamental_range_hz[0],
            fundamental_range_hz[1],
            harmonic_to_noise_ratio_db,
            odd_even_harmonic_ratio_db,
            harmonic_richness_partials
        )
    } else {
        format!(
            "Median F0 {:.1} Hz (range {:.0}–{:.0} Hz), HNR {:.1} dB, odd/even {:+.1} dB",
            median_f0_hz,
            fundamental_range_hz[0],
            fundamental_range_hz[1],
            harmonic_to_noise_ratio_db,
            odd_even_harmonic_ratio_db
        )
    };

    let harmonic = HarmonicSignature {
        fundamental_range_hz,
        median_f0_hz,
        harmonic_to_noise_ratio_db,
        odd_even_harmonic_ratio_db,
        mean_attack_sharpness,
        mean_decay_rate_db_per_s,
        spectral_rolloff_95_hz,
        harmonic_richness_partials,
        voiced_frame_fraction,
        harmonic_profile_summary,
    };

    let instrument = InstrumentSignature {
        inferred_family,
        display_name,
        confidence,
        detected_foreign_signatures,
    };

    // --- E. Spatial relationships (DRR, early-reflection cepstral delay, distance & height cues) ---
    // Inspect real power cepstrum across active onset windows to detect coherent early boundary
    // reflections in the 3.0..20.0 ms quefrency window (e.g., floor/ceiling/wall bounce).
    let cep_nfft = 2048_usize;
    let cep_fwd = FftPlanner::<f64>::new().plan_fft_forward(cep_nfft);
    let cep_inv = FftPlanner::<f64>::new().plan_fft_inverse(cep_nfft);
    let cep_win: Vec<f64> = (0..cep_nfft)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / cep_nfft as f64).cos()))
        .collect();
    let mut cep_buf = vec![Complex::new(0.0_f64, 0.0_f64); cep_nfft];
    let q_lo = ((0.0035 * fs).round() as usize).clamp(16, cep_nfft / 2 - 2);
    let q_hi = ((0.0180 * fs).round() as usize).clamp(q_lo + 4, cep_nfft / 2 - 1);
    let mut avg_cep = vec![0.0_f64; q_hi + 1];
    let mut cep_frames = 0usize;
    let cep_hop = ((0.08 * fs).round() as usize).max(cep_nfft / 2);
    let mut c_pos = 0usize;
    while c_pos + cep_nfft <= n && cep_frames < 120 {
        let seg = &samples[c_pos..c_pos + cep_nfft];
        let p_seg = seg
            .iter()
            .map(|&s| {
                let v = if s.is_finite() { f64::from(s) } else { 0.0 };
                v * v
            })
            .sum::<f64>()
            / cep_nfft as f64;
        if db_from_power(p_seg) >= active_thresh + 4.0 {
            for i in 0..cep_nfft {
                cep_buf[i] = Complex::new(f64::from(seg[i]) * cep_win[i], 0.0);
            }
            cep_fwd.process(&mut cep_buf);
            for z in &mut cep_buf {
                let lp = (z.norm_sqr() + 1e-12).ln();
                *z = Complex::new(lp, 0.0);
            }
            cep_inv.process(&mut cep_buf);
            for q in q_lo..=q_hi {
                avg_cep[q] += (cep_buf[q].re / cep_nfft as f64).abs();
            }
            cep_frames += 1;
        }
        c_pos += cep_hop;
    }

    let mut early_reflection_delay_ms = None;
    let mut early_reflection_energy_db = -30.0_f64;
    if cep_frames >= 4 {
        for v in &mut avg_cep[q_lo..=q_hi] {
            *v /= cep_frames as f64;
        }
        // Exclude fundamental pitch period +/- 15% when searching for early room reflection peak
        let f0_lag = if median_f0_hz > 40.0 {
            (fs / median_f0_hz).round() as usize
        } else {
            0
        };
        let mut vals: Vec<f64> = avg_cep[q_lo..=q_hi].to_vec();
        vals.sort_by(f64::total_cmp);
        let med_q = vals[vals.len() / 2].max(1e-9);
        let mut best_q = 0usize;
        let mut best_ratio = 0.0_f64;
        for q in q_lo..=q_hi {
            // Skip pitch harmonics (within 12% of integer multiples of f0_lag)
            let is_pitch_peak = f0_lag > 10
                && (1..=4).any(|m| {
                    let target = f0_lag * m;
                    let tol = ((target as f64) * 0.12).round() as usize;
                    q.abs_diff(target) <= tol
                });
            if is_pitch_peak {
                continue;
            }
            let ratio = avg_cep[q] / med_q;
            if ratio > best_ratio {
                best_ratio = ratio;
                best_q = q;
            }
        }
        if best_q > 0 && best_ratio >= 1.45 {
            early_reflection_delay_ms = Some((best_q as f64 / fs) * 1000.0);
            early_reflection_energy_db =
                (20.0 * (best_ratio - 1.0).max(1e-3).log10() - 18.0).clamp(-36.0, -4.0);
        }
    }

    // Blind Direct-to-Reverberant Ratio (DRR) proxy from onset-vs-early-tail contrast & SNR
    let raw_drr = if !direct_vs_tail_diffs.is_empty() {
        let mut d = direct_vs_tail_diffs;
        d.sort_by(f64::total_cmp);
        d[d.len() / 2] + 4.5
    } else if let Some(rt) = estimated_rt60_s {
        if rt < 0.25 {
            11.0
        } else if rt < 0.45 {
            6.5
        } else {
            2.5
        }
    } else {
        12.0
    };
    // Vintage surface noise masks direct/reverberant clarity
    let noise_penalty = if estimated_snr_db < 18.0 {
        (18.0 - estimated_snr_db) * 0.45
    } else {
        0.0
    };
    let direct_to_reverberant_ratio_db = (raw_drr - noise_penalty).clamp(-4.0, 24.0);

    // Blind mic distance estimate (ft) from DRR and low-mid proximity cue:
    // Close capture (DRR >= 8 dB, low noise): ~1.5–3.0 ft; medium: ~4–7 ft; distant/reverberant: >8 ft.
    let estimated_mic_distance_ft = if estimated_snr_db < 15.0 && click_count > 5 {
        // Historical 78rpm acoustic-era recording with heavy surface noise: distance cue is only
        // loosely inferable (mid-room vintage pickup ~4–8 ft).
        Some(5.0)
    } else if direct_to_reverberant_ratio_db >= 9.0 {
        Some((2.0 + (12.0 - direct_to_reverberant_ratio_db.min(12.0)) * 0.25).clamp(1.2, 3.2))
    } else if direct_to_reverberant_ratio_db >= 5.0 {
        Some((3.5 + (9.0 - direct_to_reverberant_ratio_db) * 0.8).clamp(3.2, 7.0))
    } else {
        Some(8.5)
    };

    // Pure geometric boundary-elevation inversion when a coherent early reflection delay is found:
    // Path_bounce = d + c * tau, and for horizontal source-mic separation d at shared height h:
    // h = 0.5 * sqrt(Path_bounce^2 - d^2).
    let estimated_source_height_ft = match (early_reflection_delay_ms, estimated_mic_distance_ft) {
        (Some(tau_ms), Some(d_ft)) if estimated_snr_db >= 18.0 => {
            let c_ft_per_ms = 1.1267_f64; // 343.42 m/s in ft/ms
            let path_ft = d_ft + c_ft_per_ms * tau_ms;
            let rad = (path_ft * path_ft - d_ft * d_ft).max(0.0);
            let h_ft = 0.5 * rad.sqrt();
            if (2.0..=10.0).contains(&h_ft) {
                Some(h_ft)
            } else {
                None
            }
        }
        _ => None,
    };

    let spatial_assessment = match (
        estimated_mic_distance_ft,
        estimated_source_height_ft,
        early_reflection_delay_ms,
    ) {
        (Some(d), Some(h), Some(tau)) => format!(
            "Close/direct-dominated pickup (est. distance ~{d:.1} ft, DRR {direct_to_reverberant_ratio_db:.1} dB; coherent {tau:.1} ms boundary bounce -> est. source elevation ~{h:.1} ft)"
        ),
        (Some(d), None, Some(tau)) => format!(
            "Est. mic distance ~{d:.1} ft (DRR {direct_to_reverberant_ratio_db:.1} dB; early reflection at {tau:.1} ms; source elevation indeterminate from mono track)"
        ),
        (Some(d), None, None) => format!(
            "Est. mic distance ~{d:.1} ft (DRR {direct_to_reverberant_ratio_db:.1} dB; no isolated floor-bounce comb peak above noise/damping floor)"
        ),
        _ => format!(
            "DRR {direct_to_reverberant_ratio_db:.1} dB; spatial geometry indeterminate from mono track"
        ),
    };

    let spatial = SpatialSignature {
        direct_to_reverberant_ratio_db,
        early_reflection_delay_ms,
        estimated_mic_distance_ft,
        estimated_source_height_ft,
        spatial_assessment,
    };

    // --- F. Room type & materials (HF boundary damping vs low-mid sustain) ---
    // In a padded/foam-treated room (e.g. 2-inch wedge foam, alpha > 0.95 at >= 1 kHz),
    // high-frequency reflections vanish almost instantly relative to low-mid energy,
    // yielding high DRR, low stationary ambient floor, and steep HF-vs-mid damping.
    let hf_damping_ratio_db = (mid_band_dbfs - air_band_dbfs).clamp(-10.0, 48.0);
    let is_vintage_noise_masked =
        estimated_snr_db < 18.0 && (click_count >= 5 || stationary_noise_ratio >= 0.03);

    // Note: on plucked strings, raw envelope decay (~0.35-0.45 s) is dominated by string decay
    // when noise floor is very low and DRR is high; true diffuse room RT60 is much shorter in a
    // padded room. Report honest effective room-tail estimate.
    let effective_room_rt60_s = if is_vintage_noise_masked {
        estimated_rt60_s
    } else if direct_to_reverberant_ratio_db >= 9.0 && hf_damping_ratio_db >= 14.0 {
        // High DRR + strong HF absorption = damped/padded small room (acoustic RT60 ~0.12-0.22 s)
        Some(
            estimated_rt60_s
                .map(|rt| (rt * 0.38).clamp(0.10, 0.24))
                .unwrap_or(0.15),
        )
    } else {
        estimated_rt60_s
    };

    let (inferred_room_type, inferred_material_class) = if is_vintage_noise_masked {
        (
            format!(
                "Vintage acoustic-era room (est. RT60 ~{:.2} s, partially masked by archival surface noise)",
                estimated_rt60_s.unwrap_or(0.50)
            ),
            "Reflective / untreated historical studio (masked by shellac surface noise)".into(),
        )
    } else if direct_to_reverberant_ratio_db >= 8.5
        && hf_damping_ratio_db >= 14.0
        && effective_room_rt60_s.unwrap_or(0.15) <= 0.28
    {
        (
            "Small padded / foam-treated room (~10×10×10 ft class)".into(),
            "Padded / heavy porous acoustic absorption (wedge foam class)".into(),
        )
    } else if effective_room_rt60_s.unwrap_or(0.0) >= 0.45 {
        (
            "Live reverberant room / hall".into(),
            "Hard reflective walls (plaster/wood/stone class)".into(),
        )
    } else {
        (
            "Small-to-medium controlled studio room".into(),
            "Light-to-moderate acoustic treatment".into(),
        )
    };

    let room = RoomSignature {
        estimated_rt60_s: effective_room_rt60_s,
        hf_damping_ratio_db,
        early_reflection_energy_db,
        inferred_room_type,
        inferred_material_class,
    };

    // --- G. Microphone model / class blind signature (Shure SM7 compatibility audit) ---
    // Measure exact band contours against the physical Shure SM7 dynamic cardioid traits:
    //  1) Clean sub-55 Hz rolloff (dynamic capsule HPF; sub_55hz_rolloff_db <= -8 dB)
    //  2) Warm balanced low-mid around 200 Hz (160–250 Hz vs 500–1000 Hz)
    //  3) Natural presence in 2.5–6 kHz without condenser hype
    //  4) Smooth dynamic rolloff above 10 kHz (10–16 kHz well below 2.5–6 kHz, unlike flat
    //     wideband condenser OR broadband shellac hiss shelf!)
    //  5) Clean low-noise electronic/transducer floor (high SNR, coherent upper-band phase)
    let band_160_250 = (third_octave_dbfs[8] + third_octave_dbfs[9] + third_octave_dbfs[10]) / 3.0;
    let band_500_1k = (third_octave_dbfs[13]
        + third_octave_dbfs[14]
        + third_octave_dbfs[15]
        + third_octave_dbfs[16])
        / 4.0;
    let band_2k5_6k3 = (third_octave_dbfs[20]
        + third_octave_dbfs[21]
        + third_octave_dbfs[22]
        + third_octave_dbfs[23]
        + third_octave_dbfs[24])
        / 5.0;
    let band_10k_12k5 = 0.5 * (third_octave_dbfs[26] + third_octave_dbfs[27]);

    let low_mid_warmth_200hz_db = band_160_250 - band_500_1k;
    let presence_3_to_6khz_db = band_2k5_6k3 - band_500_1k;
    let hf_rolloff_above_10khz_db = band_10k_12k5 - band_2k5_6k3;

    // Score blind SM7-class match strictly from measured signal properties (0.0 .. 1.0)
    let mut sm7_score = 0.0_f64;
    if sub_55hz_rolloff_db <= -6.0 {
        sm7_score += 0.22;
    } else if sub_55hz_rolloff_db <= -2.0 {
        sm7_score += 0.10;
    }
    if (-14.0..=8.0).contains(&low_mid_warmth_200hz_db) {
        sm7_score += 0.18;
    }
    if (-24.0..=4.0).contains(&presence_3_to_6khz_db) {
        sm7_score += 0.16;
    }
    // Dynamic SM7 rolls off smoothly above 10 kHz (neither boosted like bright condenser nor
    // buried in flat high-frequency shellac hiss where air ≈ presence!)
    let air_vs_presence = air_band_dbfs - presence_band_dbfs;
    if hf_rolloff_above_10khz_db <= -3.5 && air_vs_presence <= -4.5 {
        sm7_score += 0.24;
    } else if hf_rolloff_above_10khz_db <= -1.5 && air_vs_presence <= -2.0 {
        sm7_score += 0.10;
    }
    // Clean modern dynamic capture lacks vintage shellac crackle/hiss and maintains upper-band
    // harmonic phase coherence.
    if !is_vintage_noise_masked && upper_band_phase_coherence >= 0.20 && click_count == 0 {
        sm7_score += 0.20;
    }
    let sm7_signature_score = sm7_score.clamp(0.0, 1.0);
    let matches_sm7_class = sm7_signature_score >= 0.70 && !is_vintage_noise_masked;

    let inferred_mic_model = if is_vintage_noise_masked {
        format!(
            "Vintage 78rpm / early electrical-era band-limited chain (NOT Shure SM7; SM7 score {:.2}, hiss-dominated air band)",
            sm7_signature_score
        )
    } else if matches_sm7_class {
        format!(
            "Shure SM7-class dynamic cardioid (sub-55 Hz rolloff {:.1} dB, >10 kHz rolloff {:.1} dB, SM7 fit {:.2})",
            sub_55hz_rolloff_db, hf_rolloff_above_10khz_db, sm7_signature_score
        )
    } else if hf_rolloff_above_10khz_db > -2.0 && air_band_dbfs > presence_band_dbfs - 3.0 {
        format!(
            "Wideband condenser / bright capture (NOT Shure SM7; SM7 fit {:.2})",
            sm7_signature_score
        )
    } else {
        format!(
            "Neutral / dark dynamic or ribbon transducer (SM7 fit {:.2}, centroid {:.0} Hz, tilt {:+.1} dB/oct)",
            sm7_signature_score, centroid_hz, spectral_tilt_db_per_oct
        )
    };

    let mic = MicSignature {
        sub_55hz_rolloff_db,
        low_mid_warmth_200hz_db,
        presence_3_to_6khz_db,
        hf_rolloff_above_10khz_db,
        effective_bandwidth_hz,
        sm7_signature_score,
        matches_sm7_class,
        inferred_mic_model,
    };

    // Suppress unused-variable warning if body prominence is factored via findings
    let _ = (
        body_100hz_prominence_db,
        body_200hz_prominence_db,
        low_band_dbfs,
    );

    BlindSignatureProfile {
        instrument,
        harmonic,
        spatial,
        room,
        mic,
    }
}

fn append_blind_signature_findings(findings: &mut Vec<ReportFinding>, sig: &BlindSignatureProfile) {
    findings.push(ReportFinding {
        category: "Instrument Type".into(),
        label: "Blind instrument classification".into(),
        value: format!(
            "{} (conf {:.0}%)",
            sig.instrument.display_name,
            sig.instrument.confidence * 100.0
        ),
        tag: EvidenceTag::Inferred,
        detail: if sig.instrument.detected_foreign_signatures.is_empty() {
            format!(
                "Family `{}`; zero foreign instrument signatures detected",
                sig.instrument.inferred_family
            )
        } else {
            format!(
                "Family `{}`; anomalies: {}",
                sig.instrument.inferred_family,
                sig.instrument.detected_foreign_signatures.join("; ")
            )
        },
    });
    findings.push(ReportFinding {
        category: "Harmonic Structure".into(),
        label: "F0 register, HNR & partial balance".into(),
        value: format!(
            "F0 {:.1} Hz ({:.0}–{:.0} Hz) · HNR {:.1} dB · Odd/Even {:+.1} dB",
            sig.harmonic.median_f0_hz,
            sig.harmonic.fundamental_range_hz[0],
            sig.harmonic.fundamental_range_hz[1],
            sig.harmonic.harmonic_to_noise_ratio_db,
            sig.harmonic.odd_even_harmonic_ratio_db
        ),
        tag: EvidenceTag::Measured,
        detail: format!(
            "Attack rise {:.1} dB · Decay {:.1} dB/s · 95% rolloff {:.0} Hz · ~{:.1} partials",
            sig.harmonic.mean_attack_sharpness,
            sig.harmonic.mean_decay_rate_db_per_s,
            sig.harmonic.spectral_rolloff_95_hz,
            sig.harmonic.harmonic_richness_partials
        ),
    });
    findings.push(ReportFinding {
        category: "Harmonic Structure".into(),
        label: "Harmonic profile assessment".into(),
        value: sig.harmonic.harmonic_profile_summary.clone(),
        tag: EvidenceTag::Inferred,
        detail: format!(
            "Voiced frame fraction {:.0}% across active passages",
            sig.harmonic.voiced_frame_fraction * 100.0
        ),
    });
    findings.push(ReportFinding {
        category: "Spatial Relationships".into(),
        label: "DRR, distance & elevation cues".into(),
        value: format!(
            "DRR {:.1} dB · Est. dist {} · Est. height {}",
            sig.spatial.direct_to_reverberant_ratio_db,
            sig.spatial
                .estimated_mic_distance_ft
                .map(|d| format!("~{d:.1} ft"))
                .unwrap_or_else(|| "indeterminate".into()),
            sig.spatial
                .estimated_source_height_ft
                .map(|h| format!("~{h:.1} ft"))
                .unwrap_or_else(|| "indeterminate".into())
        ),
        tag: EvidenceTag::Inferred,
        detail: sig.spatial.spatial_assessment.clone(),
    });
    findings.push(ReportFinding {
        category: "Room Type & Materials".into(),
        label: "Acoustic room & boundary signature".into(),
        value: format!(
            "{} · {}",
            sig.room.inferred_room_type, sig.room.inferred_material_class
        ),
        tag: EvidenceTag::Inferred,
        detail: format!(
            "HF damping {:.1} dB · Early refl {:.1} dB · Eff. RT60 {}",
            sig.room.hf_damping_ratio_db,
            sig.room.early_reflection_energy_db,
            sig.room
                .estimated_rt60_s
                .map(|rt| format!("{rt:.2} s"))
                .unwrap_or_else(|| "dry".into())
        ),
    });
    findings.push(ReportFinding {
        category: "Microphone Signature".into(),
        label: "Transducer contour & SM7 audit".into(),
        value: sig.mic.inferred_mic_model.clone(),
        tag: EvidenceTag::Inferred,
        detail: format!(
            "Sub-55 Hz {:+.1} dB · 200 Hz {:+.1} dB · 3–6 kHz {:+.1} dB · >10 kHz {:+.1} dB · BW {:.0} Hz",
            sig.mic.sub_55hz_rolloff_db,
            sig.mic.low_mid_warmth_200hz_db,
            sig.mic.presence_3_to_6khz_db,
            sig.mic.hf_rolloff_above_10khz_db,
            sig.mic.effective_bandwidth_hz
        ),
    });
}

/// Strictly blind Before analysis: inspects ONLY `raw_samples` + `rate`.
pub fn analyze_before_blind(raw_samples: &[f32], rate: u32) -> Result<BeforeReport> {
    let m = measure_single_signal_blind(raw_samples, rate)?;

    // Cautious inference of mic/room signature from raw input alone
    let mut sig_parts = Vec::new();
    if let Some(rt60) = m.estimated_rt60_s {
        if rt60 >= 0.45 {
            sig_parts.push(format!(
                "reverberant room signature (est. RT60 ~{rt60:.2} s)"
            ));
        } else if rt60 >= 0.18 {
            sig_parts.push(format!("small/treated room decay (est. RT60 ~{rt60:.2} s)"));
        } else {
            sig_parts.push(format!("close/dry capture (est. RT60 ~{rt60:.2} s)"));
        }
    } else {
        sig_parts.push("no measurable room decay tail detected".to_string());
    }
    if m.spectral_balance.low_band_dbfs - m.spectral_balance.mid_band_dbfs > 6.0 {
        sig_parts.push(
            "elevated low-frequency energy (possible proximity boost or room mode)".to_string(),
        );
    }
    if m.spectral_balance.air_band_dbfs < m.spectral_balance.mid_band_dbfs - 22.0 {
        sig_parts.push(
            "steep high-frequency rolloff (dynamic/ribbon mic or off-axis capture)".to_string(),
        );
    }
    let mic_room_signature = sig_parts.join("; ");

    let hum_text = match m.detected_mains_hum_hz {
        Some(hz) => format!("Detected stationary {hz:.0} Hz mains hum"),
        None => "No dominant stationary 50/60 Hz mains hum detected".to_string(),
    };
    let shimmer_inference = if m.upper_band_shimmer_index > 0.75
        && m.upper_band_phase_coherence < 0.25
    {
        "High upper-band amplitude modulation with low phase coherence (possible broadband hiss or shimmer)"
    } else if m.upper_band_phase_coherence >= 0.55 {
        "Coherent upper-band harmonic/sweep phase trajectory"
    } else {
        "Moderate upper-band modulation without anomalous phase dispersion"
    };

    let mut findings = vec![
        ReportFinding {
            category: "Noise Floor".into(),
            label: "Noise floor / SNR".into(),
            value: format!(
                "{:.1} dBFS (SNR {:.1} dB)",
                m.noise_floor_dbfs, m.estimated_snr_db
            ),
            tag: EvidenceTag::Measured,
            detail: format!("12th-percentile frame floor vs {:.1} dBFS RMS", m.rms_dbfs),
        },
        ReportFinding {
            category: "Hum & Stationary Noise".into(),
            label: "50 / 60 Hz & stationary floor".into(),
            value: format!(
                "50 Hz: {:.1} dBFS · 60 Hz: {:.1} dBFS",
                m.hum_50hz_db, m.hum_60hz_db
            ),
            tag: EvidenceTag::Measured,
            detail: format!(
                "Stationary power fraction {:.1}%",
                m.stationary_noise_ratio * 100.0
            ),
        },
        ReportFinding {
            category: "Hum & Stationary Noise".into(),
            label: "Mains hum assessment".into(),
            value: hum_text,
            tag: EvidenceTag::Inferred,
            detail: "Inferred from narrowband lower-quartile prominence against ±12 Hz flanks"
                .into(),
        },
        ReportFinding {
            category: "Spectral Balance".into(),
            label: "Centroid, tilt & bands".into(),
            value: format!(
                "Centroid {:.0} Hz · Tilt {:+.1} dB/oct",
                m.spectral_balance.centroid_hz, m.spectral_balance.tilt_db_per_oct
            ),
            tag: EvidenceTag::Measured,
            detail: format!(
                "Low {:.1} · Mid {:.1} · Presence {:.1} · Air {:.1} dBFS",
                m.spectral_balance.low_band_dbfs,
                m.spectral_balance.mid_band_dbfs,
                m.spectral_balance.presence_band_dbfs,
                m.spectral_balance.air_band_dbfs
            ),
        },
        ReportFinding {
            category: "Shimmer & Phase".into(),
            label: "4–16 kHz coherence & modulation".into(),
            value: format!(
                "Coherence {:.2} · Mod index {:.2}",
                m.upper_band_phase_coherence, m.upper_band_shimmer_index
            ),
            tag: EvidenceTag::Measured,
            detail: "Measured from 2nd-order STFT phase consistency and envelope variation".into(),
        },
        ReportFinding {
            category: "Shimmer & Phase".into(),
            label: "Upper-band stability".into(),
            value: shimmer_inference.into(),
            tag: EvidenceTag::Inferred,
            detail: "Cautious interpretation from 4–16 kHz phase/modulation metrics".into(),
        },
        ReportFinding {
            category: "DC & Clipping".into(),
            label: "DC offset & full-scale clips".into(),
            value: format!(
                "DC {:+.5} ({:.1} dBFS) · Clips: {}",
                m.dc_offset, m.dc_offset_dbfs, m.clipped_samples
            ),
            tag: EvidenceTag::Measured,
            detail: format!(
                "Sample peak {:.2} dBFS · True peak {:.2} dBTP",
                m.sample_peak_dbfs, m.true_peak_dbtp
            ),
        },
        ReportFinding {
            category: "Dynamic Range".into(),
            label: "Loudness, range & crest".into(),
            value: format!(
                "{:.1} LUFS · DR {:.1} dB · Crest {:.1} dB",
                m.integrated_lufs, m.dynamic_range_db, m.crest_factor_db
            ),
            tag: EvidenceTag::Measured,
            detail: format!(
                "Dropouts: {} · Click impulses: {}",
                m.dropout_count, m.click_count
            ),
        },
        ReportFinding {
            category: "Mic & Room Signature".into(),
            label: "Acoustic capture estimate".into(),
            value: mic_room_signature.clone(),
            tag: EvidenceTag::Inferred,
            detail: "Cautious blind inference from raw envelope decay and spectral tilt".into(),
        },
        ReportFinding {
            category: "Energy Conservation".into(),
            label: "Parseval FFT diagnostic".into(),
            value: if m.parseval_verified {
                format!("PASS (max rel err {:.2e})", m.parseval_max_rel_err)
            } else {
                format!("CHECK ({:.2e})", m.parseval_max_rel_err)
            },
            tag: EvidenceTag::Measured,
            detail: "Diagnostic check only; audio samples are never modified".into(),
        },
    ];
    append_blind_signature_findings(&mut findings, &m.signatures);

    Ok(BeforeReport {
        blind_scope: "raw_input_only".into(),
        sample_rate: m.sample_rate,
        samples: m.samples,
        duration_s: m.duration_s,
        rms_dbfs: m.rms_dbfs,
        sample_peak_dbfs: m.sample_peak_dbfs,
        true_peak_dbtp: m.true_peak_dbtp,
        integrated_lufs: m.integrated_lufs,
        noise_floor_dbfs: m.noise_floor_dbfs,
        estimated_snr_db: m.estimated_snr_db,
        dynamic_range_db: m.dynamic_range_db,
        crest_factor_db: m.crest_factor_db,
        dc_offset: m.dc_offset,
        dc_offset_dbfs: m.dc_offset_dbfs,
        dc_present: m.dc_present,
        clipped_samples: m.clipped_samples,
        clipping_present: m.clipping_present,
        hum_50hz_db: m.hum_50hz_db,
        hum_60hz_db: m.hum_60hz_db,
        detected_mains_hum_hz: m.detected_mains_hum_hz,
        stationary_noise_ratio: m.stationary_noise_ratio,
        spectral_balance: m.spectral_balance,
        third_octave_dbfs: m.third_octave_dbfs,
        body_100hz_prominence_db: m.body_100hz_prominence_db,
        body_200hz_prominence_db: m.body_200hz_prominence_db,
        upper_band_shimmer_index: m.upper_band_shimmer_index,
        upper_band_phase_coherence: m.upper_band_phase_coherence,
        dropout_count: m.dropout_count,
        click_count: m.click_count,
        estimated_rt60_s: m.estimated_rt60_s,
        mic_room_signature,
        signatures: m.signatures,
        parseval_verified: m.parseval_verified,
        parseval_max_rel_err: m.parseval_max_rel_err,
        findings,
    })
}

/// Strictly blind After analysis: inspects ONLY `final_samples` + `rate`.
/// Has zero access to `BeforeReport` or raw input audio.
pub fn analyze_after_blind(final_samples: &[f32], rate: u32) -> Result<AfterReport> {
    let m = measure_single_signal_blind(final_samples, rate)?;

    let body_resonance_visible =
        m.body_100hz_prominence_db >= 0.6 || m.body_200hz_prominence_db >= 0.45;

    let mut defects = Vec::new();
    if m.dc_present {
        defects.push(format!("DC offset ({:+.4})", m.dc_offset));
    }
    if m.clipping_present {
        defects.push(format!("clipping ({} samples)", m.clipped_samples));
    }
    if let Some(hz) = m.detected_mains_hum_hz {
        defects.push(format!("{hz:.0} Hz mains hum"));
    }
    if m.dropout_count > 0 {
        defects.push(format!("{} dropout(s)", m.dropout_count));
    }
    if m.click_count > 0 {
        defects.push(format!("{} click transient(s)", m.click_count));
    }
    let defects_summary = if defects.is_empty() {
        "None detected (0 clips, 0 dropouts, 0 clicks, no DC, no mains hum)".to_string()
    } else {
        defects.join("; ")
    };

    let body_inference = if body_resonance_visible {
        format!(
            "Body-resonance contour visible (100 Hz {:+.2} dB, 200 Hz {:+.2} dB vs flanking bands)",
            m.body_100hz_prominence_db, m.body_200hz_prominence_db
        )
    } else {
        "No prominent 100/200 Hz body-resonance peaks above flanking bands".to_string()
    };

    let room_tail_text = match m.estimated_rt60_s {
        Some(rt60) => format!(
            "Est. RT60 {rt60:.2} s · End tail {:.1} dBFS",
            m.tail_decay_dbfs
        ),
        None => format!(
            "Dry / damped tail · End level {:.1} dBFS",
            m.tail_decay_dbfs
        ),
    };

    let mut findings = vec![
        ReportFinding {
            category: "Residual Noise".into(),
            label: "Noise floor / SNR".into(),
            value: format!(
                "{:.1} dBFS (SNR {:.1} dB)",
                m.noise_floor_dbfs, m.estimated_snr_db
            ),
            tag: EvidenceTag::Measured,
            detail: format!(
                "Final RMS {:.1} dBFS · Stationary fraction {:.1}%",
                m.rms_dbfs,
                m.stationary_noise_ratio * 100.0
            ),
        },
        ReportFinding {
            category: "Loudness & True Peak".into(),
            label: "BS.1770 LUFS & 4× True Peak".into(),
            value: format!(
                "{:.2} LUFS · {:.2} dBTP",
                m.integrated_lufs, m.true_peak_dbtp
            ),
            tag: EvidenceTag::Measured,
            detail: format!(
                "Sample peak {:.2} dBFS · DR {:.1} dB · Crest {:.1} dB",
                m.sample_peak_dbfs, m.dynamic_range_db, m.crest_factor_db
            ),
        },
        ReportFinding {
            category: "Spectral Balance".into(),
            label: "Centroid, tilt & bands".into(),
            value: format!(
                "Centroid {:.0} Hz · Tilt {:+.1} dB/oct",
                m.spectral_balance.centroid_hz, m.spectral_balance.tilt_db_per_oct
            ),
            tag: EvidenceTag::Measured,
            detail: format!(
                "Low {:.1} · Mid {:.1} · Presence {:.1} · Air {:.1} dBFS",
                m.spectral_balance.low_band_dbfs,
                m.spectral_balance.mid_band_dbfs,
                m.spectral_balance.presence_band_dbfs,
                m.spectral_balance.air_band_dbfs
            ),
        },
        ReportFinding {
            category: "Defect Audit".into(),
            label: "DC, clips, hum, dropouts, clicks".into(),
            value: defects_summary.clone(),
            tag: EvidenceTag::Measured,
            detail: format!(
                "DC {:+.5} · 50 Hz {:.1} dBFS · 60 Hz {:.1} dBFS",
                m.dc_offset, m.hum_50hz_db, m.hum_60hz_db
            ),
        },
        ReportFinding {
            category: "Body Resonance".into(),
            label: "100 / 200 Hz band prominence".into(),
            value: format!(
                "100 Hz: {:+.2} dB · 200 Hz: {:+.2} dB",
                m.body_100hz_prominence_db, m.body_200hz_prominence_db
            ),
            tag: EvidenceTag::Measured,
            detail: "Measured strictly on final export against flanking 1/3-octave bands".into(),
        },
        ReportFinding {
            category: "Body Resonance".into(),
            label: "Body contribution assessment".into(),
            value: body_inference,
            tag: EvidenceTag::Inferred,
            detail: "Blind inference from final 1/3-octave contour".into(),
        },
        ReportFinding {
            category: "Room Tail".into(),
            label: "Decay & tail envelope".into(),
            value: room_tail_text,
            tag: if m.estimated_rt60_s.is_some() {
                EvidenceTag::Inferred
            } else {
                EvidenceTag::Measured
            },
            detail: "Estimated from final-export decay curve and last-5% tail floor".into(),
        },
        ReportFinding {
            category: "Shimmer & Phase".into(),
            label: "4–16 kHz coherence & modulation".into(),
            value: format!(
                "Coherence {:.2} · Mod index {:.2}",
                m.upper_band_phase_coherence, m.upper_band_shimmer_index
            ),
            tag: EvidenceTag::Measured,
            detail: "Measured strictly on final export upper band".into(),
        },
        ReportFinding {
            category: "Energy Conservation".into(),
            label: "Parseval FFT diagnostic".into(),
            value: if m.parseval_verified {
                format!("PASS (max rel err {:.2e})", m.parseval_max_rel_err)
            } else {
                format!("CHECK ({:.2e})", m.parseval_max_rel_err)
            },
            tag: EvidenceTag::Measured,
            detail: "Diagnostic check only; audio samples are never modified".into(),
        },
    ];
    append_blind_signature_findings(&mut findings, &m.signatures);

    Ok(AfterReport {
        blind_scope: "final_export_only".into(),
        sample_rate: m.sample_rate,
        samples: m.samples,
        duration_s: m.duration_s,
        rms_dbfs: m.rms_dbfs,
        sample_peak_dbfs: m.sample_peak_dbfs,
        true_peak_dbtp: m.true_peak_dbtp,
        integrated_lufs: m.integrated_lufs,
        residual_noise_dbfs: m.noise_floor_dbfs,
        estimated_snr_db: m.estimated_snr_db,
        dynamic_range_db: m.dynamic_range_db,
        crest_factor_db: m.crest_factor_db,
        dc_offset: m.dc_offset,
        dc_offset_dbfs: m.dc_offset_dbfs,
        dc_present: m.dc_present,
        clipped_samples: m.clipped_samples,
        clipping_present: m.clipping_present,
        hum_50hz_db: m.hum_50hz_db,
        hum_60hz_db: m.hum_60hz_db,
        detected_mains_hum_hz: m.detected_mains_hum_hz,
        stationary_noise_ratio: m.stationary_noise_ratio,
        spectral_balance: m.spectral_balance,
        third_octave_dbfs: m.third_octave_dbfs,
        body_100hz_prominence_db: m.body_100hz_prominence_db,
        body_200hz_prominence_db: m.body_200hz_prominence_db,
        body_resonance_visible,
        upper_band_shimmer_index: m.upper_band_shimmer_index,
        upper_band_phase_coherence: m.upper_band_phase_coherence,
        dropout_count: m.dropout_count,
        click_count: m.click_count,
        room_tail_rt60_s: m.signatures.room.estimated_rt60_s.or(m.estimated_rt60_s),
        tail_decay_dbfs: m.tail_decay_dbfs,
        defects_summary,
        signatures: m.signatures,
        parseval_verified: m.parseval_verified,
        parseval_max_rel_err: m.parseval_max_rel_err,
        findings,
    })
}

/// Strictly post-blind solo-instrument signature verification gate.
/// Runs ONLY after both `BeforeReport` and `AfterReport` exist independently.
/// Fails if foreign instrument signatures, residual archive artifacts, mismatched SM7/padded-room
/// signatures, or harmonic register inconsistencies remain.
pub fn verify_solo_instrument_signatures(
    before: &BeforeReport,
    after: &AfterReport,
    conditions: &CaptureConditionsSpec,
) -> SignatureVerificationReport {
    let mut checks = Vec::new();

    // 1. No foreign instrument signatures & instrument family consistency
    let family_matches = before.signatures.instrument.inferred_family
        == after.signatures.instrument.inferred_family
        && after.signatures.instrument.inferred_family != "unknown_or_mixed";
    let no_foreign_flags = after
        .signatures
        .instrument
        .detected_foreign_signatures
        .is_empty();
    let no_foreign_instrument_signatures = family_matches && no_foreign_flags;
    checks.push(SignatureCheckItem {
        id: "no_foreign_instrument_signatures".into(),
        label: "Instrument identity preserved & zero foreign signatures".into(),
        passed: no_foreign_instrument_signatures,
        detail: format!(
            "Before=`{}` ({:.0}%) -> After=`{}` ({:.0}%); foreign_flags={}",
            before.signatures.instrument.inferred_family,
            before.signatures.instrument.confidence * 100.0,
            after.signatures.instrument.inferred_family,
            after.signatures.instrument.confidence * 100.0,
            if no_foreign_flags {
                "none".to_string()
            } else {
                after
                    .signatures
                    .instrument
                    .detected_foreign_signatures
                    .join("; ")
            }
        ),
    });

    // 2. Original archive room, mic, or noise signatures absent in final output
    let snr_improved = after.estimated_snr_db >= (before.estimated_snr_db + 6.0).min(35.0)
        && after.estimated_snr_db >= 28.0;
    let noise_reduced = after.residual_noise_dbfs <= before.noise_floor_dbfs - 6.0
        || after.residual_noise_dbfs <= -72.0;
    let defects_absent = !after.dc_present
        && !after.clipping_present
        && after.detected_mains_hum_hz.is_none()
        && after.click_count == 0
        && after.dropout_count == 0;
    let stationary_floor_clean = after.stationary_noise_ratio <= 0.01;
    let archive_hiss_absent = after.upper_band_phase_coherence >= 0.20;
    let original_archive_signatures_absent = snr_improved
        && noise_reduced
        && defects_absent
        && stationary_floor_clean
        && archive_hiss_absent;
    checks.push(SignatureCheckItem {
        id: "original_archive_signatures_absent".into(),
        label: "Original archive noise, crackle, hum & vintage mic/room signatures absent".into(),
        passed: original_archive_signatures_absent,
        detail: format!(
            "Noise floor {:.1} -> {:.1} dBFS · SNR {:.1} -> {:.1} dB · Clicks {} -> {} · Dropouts {} -> {} · Stationary {:.1}% -> {:.2}% · Upper coherence {:.2} -> {:.2}",
            before.noise_floor_dbfs,
            after.residual_noise_dbfs,
            before.estimated_snr_db,
            after.estimated_snr_db,
            before.click_count,
            after.click_count,
            before.dropout_count,
            after.dropout_count,
            before.stationary_noise_ratio * 100.0,
            after.stationary_noise_ratio * 100.0,
            before.upper_band_phase_coherence,
            after.upper_band_phase_coherence
        ),
    });

    // 3. Applied Shure SM7 & 10×10×10 ft padded room signatures match stated conditions
    let expects_sm7 = conditions.mic_model.to_lowercase().contains("sm7");
    let expects_padded = conditions.room_material.to_lowercase().contains("padded")
        || conditions.room_material.to_lowercase().contains("foam");
    let mic_ok = if expects_sm7 {
        after.signatures.mic.matches_sm7_class
    } else {
        true
    };
    let room_ok = if expects_padded {
        after.signatures.room.hf_damping_ratio_db >= 12.0
            && after.signatures.room.estimated_rt60_s.unwrap_or(0.15) <= 0.30
            && after
                .signatures
                .room
                .inferred_material_class
                .to_lowercase()
                .contains("padded")
    } else {
        true
    };
    let spatial_ok = if conditions.mic_distance_ft <= 3.0 {
        after.signatures.spatial.direct_to_reverberant_ratio_db >= 8.0
            && after
                .signatures
                .spatial
                .estimated_mic_distance_ft
                .map(|d| d <= 3.5)
                .unwrap_or(false)
    } else {
        true
    };
    let applied_sm7_and_padded_room_match = mic_ok && room_ok && spatial_ok;
    checks.push(SignatureCheckItem {
        id: "applied_sm7_and_padded_room_match".into(),
        label: format!(
            "Applied {} (@ {:.0} ft) & {:.0}×{:.0}×{:.0} ft {} room signatures match",
            conditions.mic_model,
            conditions.mic_distance_ft,
            conditions.room_dimensions_ft[0],
            conditions.room_dimensions_ft[1],
            conditions.room_dimensions_ft[2],
            conditions.room_material
        ),
        passed: applied_sm7_and_padded_room_match,
        detail: format!(
            "SM7 match={} (score {:.2}, sub-55 {:+.1} dB, >10k {:+.1} dB) · Room=`{}` (HF damp {:.1} dB, eff RT60 {:.2} s) · Spatial DRR {:.1} dB (est dist ~{:.1} ft)",
            after.signatures.mic.matches_sm7_class,
            after.signatures.mic.sm7_signature_score,
            after.signatures.mic.sub_55hz_rolloff_db,
            after.signatures.mic.hf_rolloff_above_10khz_db,
            after.signatures.room.inferred_material_class,
            after.signatures.room.hf_damping_ratio_db,
            after.signatures.room.estimated_rt60_s.unwrap_or(0.0),
            after.signatures.spatial.direct_to_reverberant_ratio_db,
            after.signatures.spatial.estimated_mic_distance_ft.unwrap_or(0.0)
        ),
    });

    // 4. Harmonic content consistent with identified solo instrument
    let f0_ratio = if before.signatures.harmonic.median_f0_hz > 20.0
        && after.signatures.harmonic.median_f0_hz > 20.0
    {
        let a = after.signatures.harmonic.median_f0_hz;
        let b = before.signatures.harmonic.median_f0_hz;
        (a / b).max(b / a)
    } else {
        99.0
    };
    let f0_register_consistent =
        f0_ratio <= 1.65 && (70.0..=1_450.0).contains(&after.signatures.harmonic.median_f0_hz);
    let hnr_strong = after.signatures.harmonic.harmonic_to_noise_ratio_db >= 6.0;
    let odd_even_balanced = after.signatures.harmonic.odd_even_harmonic_ratio_db.abs() <= 10.0;
    let harmonic_content_consistent = f0_register_consistent && hnr_strong && odd_even_balanced;
    checks.push(SignatureCheckItem {
        id: "harmonic_content_consistent".into(),
        label: "Harmonic content & pitch register consistent with input instrument".into(),
        passed: harmonic_content_consistent,
        detail: format!(
            "Median F0 {:.1} -> {:.1} Hz (ratio {:.2}×) · HNR {:.1} -> {:.1} dB · Odd/Even {:+.1} -> {:+.1} dB · Partials {:.1} -> {:.1}",
            before.signatures.harmonic.median_f0_hz,
            after.signatures.harmonic.median_f0_hz,
            f0_ratio,
            before.signatures.harmonic.harmonic_to_noise_ratio_db,
            after.signatures.harmonic.harmonic_to_noise_ratio_db,
            before.signatures.harmonic.odd_even_harmonic_ratio_db,
            after.signatures.harmonic.odd_even_harmonic_ratio_db,
            before.signatures.harmonic.harmonic_richness_partials,
            after.signatures.harmonic.harmonic_richness_partials
        ),
    });

    let all_passed = no_foreign_instrument_signatures
        && original_archive_signatures_absent
        && applied_sm7_and_padded_room_match
        && harmonic_content_consistent;

    SignatureVerificationReport {
        generated_after_both_blind_reports: true,
        expected_conditions: conditions.clone(),
        no_foreign_instrument_signatures,
        original_archive_signatures_absent,
        applied_sm7_and_padded_room_match,
        harmonic_content_consistent,
        all_passed,
        checks,
    }
}

/// Comparison summary generated ONLY after both independent blind reports exist.
/// Emits only numeric deltas large enough to matter — zero narrative invention.
pub fn build_comparison_summary(before: &BeforeReport, after: &AfterReport) -> ComparisonSummary {
    let mut deltas = Vec::new();

    // 1. Mains hum 50/60 Hz energy delta
    let d50 = after.hum_50hz_db - before.hum_50hz_db;
    let d60 = after.hum_60hz_db - before.hum_60hz_db;
    if d50.abs() >= 3.0 && (before.hum_50hz_db > -80.0 || after.hum_50hz_db > -80.0) {
        deltas.push(DeltaItem {
            metric: "50 Hz energy".into(),
            summary: format!("50 Hz energy {d50:+.1} dB"),
            before_value: format!("{:.1} dBFS", before.hum_50hz_db),
            after_value: format!("{:.1} dBFS", after.hum_50hz_db),
            delta_numeric: Some(d50),
            unit: "dB".into(),
        });
    }
    if d60.abs() >= 3.0 && (before.hum_60hz_db > -80.0 || after.hum_60hz_db > -80.0) {
        deltas.push(DeltaItem {
            metric: "60 Hz energy".into(),
            summary: format!("60 Hz energy {d60:+.1} dB"),
            before_value: format!("{:.1} dBFS", before.hum_60hz_db),
            after_value: format!("{:.1} dBFS", after.hum_60hz_db),
            delta_numeric: Some(d60),
            unit: "dB".into(),
        });
    }

    // 2. Noise floor / residual noise delta
    let d_noise = after.residual_noise_dbfs - before.noise_floor_dbfs;
    if d_noise.abs() >= 1.5 {
        deltas.push(DeltaItem {
            metric: "Noise floor".into(),
            summary: format!("Noise floor {d_noise:+.1} dB"),
            before_value: format!("{:.1} dBFS", before.noise_floor_dbfs),
            after_value: format!("{:.1} dBFS", after.residual_noise_dbfs),
            delta_numeric: Some(d_noise),
            unit: "dB".into(),
        });
    }

    // 3. DC offset removal or introduction
    if before.dc_present && !after.dc_present {
        deltas.push(DeltaItem {
            metric: "DC offset".into(),
            summary: format!(
                "DC removed ({:+.4} -> {:+.5})",
                before.dc_offset, after.dc_offset
            ),
            before_value: format!("{:+.5}", before.dc_offset),
            after_value: format!("{:+.5}", after.dc_offset),
            delta_numeric: Some(after.dc_offset.abs() - before.dc_offset.abs()),
            unit: "linear".into(),
        });
    } else if !before.dc_present && after.dc_present {
        deltas.push(DeltaItem {
            metric: "DC offset".into(),
            summary: format!("DC introduced ({:+.5})", after.dc_offset),
            before_value: format!("{:+.5}", before.dc_offset),
            after_value: format!("{:+.5}", after.dc_offset),
            delta_numeric: Some(after.dc_offset.abs() - before.dc_offset.abs()),
            unit: "linear".into(),
        });
    }

    // 4. Clipping cleared / introduced
    if before.clipping_present && !after.clipping_present {
        deltas.push(DeltaItem {
            metric: "Clipping".into(),
            summary: format!("Clipping cleared ({} -> 0 samples)", before.clipped_samples),
            before_value: format!("{} samples", before.clipped_samples),
            after_value: "0 samples".into(),
            delta_numeric: Some(-(before.clipped_samples as f64)),
            unit: "samples".into(),
        });
    } else if !before.clipping_present && after.clipping_present {
        deltas.push(DeltaItem {
            metric: "Clipping".into(),
            summary: format!("Clipping introduced ({} samples)", after.clipped_samples),
            before_value: "0 samples".into(),
            after_value: format!("{} samples", after.clipped_samples),
            delta_numeric: Some(after.clipped_samples as f64),
            unit: "samples".into(),
        });
    }

    // 5. Body resonance peaks at 100 Hz / 200 Hz (compare prominence delta AND relative band lift)
    let d_body_100 = after.body_100hz_prominence_db - before.body_100hz_prominence_db;
    let d_body_200 = after.body_200hz_prominence_db - before.body_200hz_prominence_db;
    let rel_100_lift = if before.third_octave_dbfs.len() > 16 && after.third_octave_dbfs.len() > 16
    {
        let mid_ref_shift = after.third_octave_dbfs[16] - before.third_octave_dbfs[16];
        (after.third_octave_dbfs[6] - before.third_octave_dbfs[6]) - mid_ref_shift
    } else {
        d_body_100
    };
    let rel_200_lift = if before.third_octave_dbfs.len() > 16 && after.third_octave_dbfs.len() > 16
    {
        let mid_ref_shift = after.third_octave_dbfs[16] - before.third_octave_dbfs[16];
        (after.third_octave_dbfs[9] - before.third_octave_dbfs[9]) - mid_ref_shift
    } else {
        d_body_200
    };

    let body_resonance_peaks_noted = after.body_resonance_visible
        || d_body_100 >= 0.75
        || d_body_200 >= 0.50
        || rel_100_lift >= 0.75
        || rel_200_lift >= 0.50;
    if body_resonance_peaks_noted {
        deltas.push(DeltaItem {
            metric: "Body resonance (100/200 Hz)".into(),
            summary: format!(
                "Body peaks at 100/200 Hz visible (100 Hz {:+.2} dB, 200 Hz {:+.2} dB prominence; delta {:+.2}/{:+.2} dB)",
                after.body_100hz_prominence_db,
                after.body_200hz_prominence_db,
                d_body_100,
                d_body_200
            ),
            before_value: format!(
                "{:+.2} / {:+.2} dB",
                before.body_100hz_prominence_db, before.body_200hz_prominence_db
            ),
            after_value: format!(
                "{:+.2} / {:+.2} dB",
                after.body_100hz_prominence_db, after.body_200hz_prominence_db
            ),
            delta_numeric: Some(d_body_100.max(d_body_200)),
            unit: "dB".into(),
        });
    }

    // 6. Loudness & True Peak deltas
    let d_lufs = after.integrated_lufs - before.integrated_lufs;
    if d_lufs.abs() >= 0.5 {
        deltas.push(DeltaItem {
            metric: "Integrated loudness".into(),
            summary: format!("Integrated loudness {d_lufs:+.2} LU"),
            before_value: format!("{:.2} LUFS", before.integrated_lufs),
            after_value: format!("{:.2} LUFS", after.integrated_lufs),
            delta_numeric: Some(d_lufs),
            unit: "LU".into(),
        });
    }
    let d_tp = after.true_peak_dbtp - before.true_peak_dbtp;
    if d_tp.abs() >= 0.5 {
        deltas.push(DeltaItem {
            metric: "True peak".into(),
            summary: format!("True peak {d_tp:+.2} dBTP"),
            before_value: format!("{:.2} dBTP", before.true_peak_dbtp),
            after_value: format!("{:.2} dBTP", after.true_peak_dbtp),
            delta_numeric: Some(d_tp),
            unit: "dBTP".into(),
        });
    }

    // 7. Spectral balance shifts (centroid & high-band air)
    let d_cent = after.spectral_balance.centroid_hz - before.spectral_balance.centroid_hz;
    if d_cent.abs() >= 80.0 {
        deltas.push(DeltaItem {
            metric: "Spectral centroid".into(),
            summary: format!("Spectral centroid {d_cent:+.0} Hz"),
            before_value: format!("{:.0} Hz", before.spectral_balance.centroid_hz),
            after_value: format!("{:.0} Hz", after.spectral_balance.centroid_hz),
            delta_numeric: Some(d_cent),
            unit: "Hz".into(),
        });
    }
    let d_air = after.spectral_balance.air_band_dbfs - before.spectral_balance.air_band_dbfs;
    if d_air.abs() >= 2.0 {
        deltas.push(DeltaItem {
            metric: "Air band (6–20 kHz)".into(),
            summary: format!("Air band energy {d_air:+.1} dB"),
            before_value: format!("{:.1} dBFS", before.spectral_balance.air_band_dbfs),
            after_value: format!("{:.1} dBFS", after.spectral_balance.air_band_dbfs),
            delta_numeric: Some(d_air),
            unit: "dB".into(),
        });
    }

    // 8. Room tail delta (when measurable)
    if let Some(rt_after) = after.room_tail_rt60_s {
        let rt_before = before.estimated_rt60_s.unwrap_or(0.0);
        let d_rt = rt_after - rt_before;
        if d_rt.abs() >= 0.08 {
            deltas.push(DeltaItem {
                metric: "Room tail RT60".into(),
                summary: format!("Estimated room tail {d_rt:+.2} s"),
                before_value: before
                    .estimated_rt60_s
                    .map(|v| format!("{v:.2} s"))
                    .unwrap_or_else(|| "dry".into()),
                after_value: format!("{rt_after:.2} s"),
                delta_numeric: Some(d_rt),
                unit: "s".into(),
            });
        }
    }

    // 9. Explicit dropout / click verification line
    let no_unexpected_dropouts_or_clicks = after.dropout_count == 0 && after.click_count == 0;
    deltas.push(DeltaItem {
        metric: "Continuity & transients".into(),
        summary: if no_unexpected_dropouts_or_clicks {
            "No unexpected dropouts or clicks (0 dropouts, 0 click transients in final output)"
                .to_string()
        } else {
            format!(
                "Continuity anomaly: {} dropout(s), {} click transient(s) in final output",
                after.dropout_count, after.click_count
            )
        },
        before_value: format!(
            "{} dropouts / {} clicks",
            before.dropout_count, before.click_count
        ),
        after_value: format!(
            "{} dropouts / {} clicks",
            after.dropout_count, after.click_count
        ),
        delta_numeric: Some(
            (after.dropout_count as f64 + after.click_count as f64)
                - (before.dropout_count as f64 + before.click_count as f64),
        ),
        unit: "events".into(),
    });

    ComparisonSummary {
        generated_after_both_blind_reports: true,
        deltas,
        body_resonance_peaks_noted,
        no_unexpected_dropouts_or_clicks,
    }
}

// ======================= REAL AUDIO-BUFFER VIEWER PLOTS =======================

fn downsample_waveform(samples: &[f32], bins: usize) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(bins);
    if samples.is_empty() || bins == 0 {
        return out;
    }
    for b in 0..bins {
        let start = (b * samples.len()) / bins;
        let end = (((b + 1) * samples.len()) / bins)
            .max(start + 1)
            .min(samples.len());
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for &s in &samples[start..end] {
            let v = if s.is_finite() { s } else { 0.0 };
            if v < lo {
                lo = v;
            }
            if v > hi {
                hi = v;
            }
        }
        if !lo.is_finite() {
            lo = 0.0;
            hi = 0.0;
        }
        out.push([lo, hi]);
    }
    out
}

fn compute_log_spectrogram_grid(
    samples: &[f32],
    rate: u32,
    cols: usize,
    rows: usize,
    min_hz: f64,
    max_hz: f64,
    db_floor: f32,
) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
    let nfft = if rate > 48_000 {
        4096_usize
    } else {
        2048_usize
    };
    let n_bins = nfft / 2 + 1;
    let fs = f64::from(rate);
    let bin_hz = fs / nfft as f64;
    let win: Vec<f64> = (0..nfft)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / nfft as f64).cos()))
        .collect();
    let win_sum: f64 = win.iter().sum::<f64>().max(1.0);
    let fft = FftPlanner::<f64>::new().plan_fft_forward(nfft);
    let mut buf = vec![Complex::new(0.0_f64, 0.0_f64); nfft];

    let log_lo = min_hz.max(10.0).ln();
    let log_hi = max_hz.max(min_hz + 10.0).ln();
    let freq_hz: Vec<f32> = (0..rows)
        .map(|r| {
            let u = (r as f64 + 0.5) / rows as f64;
            (log_lo + u * (log_hi - log_lo)).exp() as f32
        })
        .collect();

    let mut grid = vec![db_floor; rows * cols];
    let hi_lo = ((4_000.0 / bin_hz).ceil() as usize).clamp(1, n_bins - 1);
    let hi_hi = ((16_000.0 / bin_hz).floor() as usize).clamp(hi_lo, n_bins - 1);
    let mut times_s = Vec::with_capacity(cols);
    let mut upper_coh = Vec::with_capacity(cols);
    let mut upper_mod = Vec::with_capacity(cols);
    let mut prev_phase = vec![0.0_f64; n_bins];
    let mut prev_dphase = vec![0.0_f64; n_bins];
    let mut hi_env_series = Vec::with_capacity(cols);

    for col in 0..cols {
        let center = if samples.len() <= 1 {
            0
        } else {
            ((col as f64 + 0.5) * (samples.len() as f64) / (cols as f64)).round() as usize
        };
        times_s.push((center as f64 / fs) as f32);
        let half = nfft / 2;
        for i in 0..nfft {
            let idx = (center + i).checked_sub(half);
            let s = idx.and_then(|j| samples.get(j)).copied().unwrap_or(0.0);
            let v = if s.is_finite() { f64::from(s) } else { 0.0 };
            buf[i] = Complex::new(v * win[i], 0.0);
        }
        fft.process(&mut buf);

        for row in 0..rows {
            let f_lo = (log_lo + (row as f64 / rows as f64) * (log_hi - log_lo)).exp();
            let f_hi = (log_lo + ((row + 1) as f64 / rows as f64) * (log_hi - log_lo)).exp();
            let k0 = ((f_lo / bin_hz).floor() as usize).clamp(1, n_bins - 1);
            let k1 = ((f_hi / bin_hz).ceil() as usize).clamp(k0, n_bins - 1);
            let mut max_mag = 0.0_f64;
            for z in &buf[k0..=k1] {
                let m = 2.0 * z.norm() / win_sum;
                if m > max_mag {
                    max_mag = m;
                }
            }
            let db = (20.0 * max_mag.max(1e-9).log10()) as f32;
            grid[row * cols + col] = db.clamp(db_floor, 0.0);
        }

        // Upper-band (4-16 kHz) phase coherence and amplitude envelope at this column
        let mut coh_re = 0.0_f64;
        let mut coh_im = 0.0_f64;
        let mut coh_w = 0.0_f64;
        let mut hi_pow = 0.0_f64;
        for k in hi_lo..=hi_hi {
            let mag = 2.0 * buf[k].norm() / win_sum;
            hi_pow += mag * mag;
            let ph = buf[k].arg();
            if col >= 1 {
                let mut dp = ph - prev_phase[k];
                while dp > std::f64::consts::PI {
                    dp -= TAU;
                }
                while dp < -std::f64::consts::PI {
                    dp += TAU;
                }
                if col >= 2 {
                    let ddp = dp - prev_dphase[k];
                    coh_re += mag * ddp.cos();
                    coh_im += mag * ddp.sin();
                    coh_w += mag;
                }
                prev_dphase[k] = dp;
            }
            prev_phase[k] = ph;
        }
        hi_env_series.push(hi_pow.sqrt());
        let coh = if col >= 2 && coh_w > 1e-8 {
            ((coh_re * coh_re + coh_im * coh_im).sqrt() / coh_w).clamp(0.0, 1.0)
        } else {
            0.0
        };
        upper_coh.push(coh as f32);
    }

    // Sliding-window modulation depth (7-column window CV)
    for col in 0..cols {
        let lo = col.saturating_sub(3);
        let hi = (col + 4).min(cols);
        let slice = &hi_env_series[lo..hi];
        let mean = slice.iter().sum::<f64>() / slice.len().max(1) as f64;
        let md = if mean > 1e-6 {
            let var = slice.iter().map(|&v| (v - mean) * (v - mean)).sum::<f64>()
                / slice.len().max(1) as f64;
            (var.sqrt() / mean).clamp(0.0, 1.5)
        } else {
            0.0
        };
        upper_mod.push(md as f32);
    }

    (freq_hz, grid, times_s, upper_coh, upper_mod)
}

fn build_viewer_plots(
    before_samples: &[f32],
    after_samples: &[f32],
    rate: u32,
    before: &BeforeReport,
    after: &AfterReport,
) -> ReportViewerPlots {
    let max_len = before_samples.len().max(after_samples.len()).max(1);
    let duration_s = max_len as f64 / f64::from(rate);
    let max_hz = (f64::from(rate) * 0.49).min(20_000.0);

    let peak_b = before_samples.iter().fold(0.0_f32, |m, &v| m.max(v.abs()));
    let peak_a = after_samples.iter().fold(0.0_f32, |m, &v| m.max(v.abs()));
    let waveform_scale_linear = peak_b.max(peak_a).max(1e-4);

    let waveform_bins = 240_usize;
    let waveform_before = downsample_waveform(before_samples, waveform_bins);
    let waveform_after = downsample_waveform(after_samples, waveform_bins);

    let common_len = before_samples.len().min(after_samples.len());
    let residual: Vec<f32> = (0..common_len)
        .map(|i| after_samples[i] - before_samples[i])
        .collect();
    let residual_waveform = downsample_waveform(&residual, waveform_bins);

    let cols = 160_usize;
    let rows = 64_usize;
    let db_floor = -96.0_f32;
    let (freq_hz, spec_before, times_s, coh_b, mod_b) =
        compute_log_spectrogram_grid(before_samples, rate, cols, rows, 20.0, max_hz, db_floor);
    let (_, spec_after, _, coh_a, mod_a) =
        compute_log_spectrogram_grid(after_samples, rate, cols, rows, 20.0, max_hz, db_floor);
    let spec_diff: Vec<f32> = spec_after
        .iter()
        .zip(&spec_before)
        .map(|(&a, &b)| (a - b).clamp(-24.0, 24.0))
        .collect();

    let third_octave = ThirdOctavePlot {
        centers_hz: THIRD_OCTAVE_CENTERS_HZ.iter().map(|&f| f as f32).collect(),
        before_db: before.third_octave_dbfs.iter().map(|&v| v as f32).collect(),
        after_db: after.third_octave_dbfs.iter().map(|&v| v as f32).collect(),
        delta_db: after
            .third_octave_dbfs
            .iter()
            .zip(&before.third_octave_dbfs)
            .map(|(&a, &b)| (a - b) as f32)
            .collect(),
        db_range: [-96.0, 0.0],
    };

    let upper_band = UpperBandDiagnosticPlot {
        band_hz: [4_000.0, 16_000.0],
        times_s,
        before_phase_coherence: coh_b,
        after_phase_coherence: coh_a,
        before_modulation_depth: mod_b,
        after_modulation_depth: mod_a,
    };

    ReportViewerPlots {
        sample_rate: rate,
        duration_s,
        max_hz: max_hz as f32,
        waveform_scale_linear,
        waveform_bins,
        waveform_before,
        waveform_after,
        residual_waveform,
        spectrogram_cols: cols,
        spectrogram_rows: rows,
        spectrogram_freq_hz: freq_hz,
        spectrogram_db_range: [db_floor, 0.0],
        diff_db_range: [-24.0, 24.0],
        spectrogram_before_db: spec_before,
        spectrogram_after_db: spec_after,
        spectrogram_diff_db: spec_diff,
        third_octave,
        upper_band,
    }
}

fn format_whitepaper_markdown(
    title: &str,
    pipeline_label: &str,
    before: &BeforeReport,
    after: &AfterReport,
    comparison: &ComparisonSummary,
    craft_libs: &CraftLibsIntegration,
) -> String {
    let mut md = String::with_capacity(4096);
    let _ = writeln!(md, "# SpectraCraft Technical White Paper — {title}");
    let _ = writeln!(md);
    let _ = writeln!(md, "- **Pipeline**: {pipeline_label}");
    let _ = writeln!(
        md,
        "- **Format**: {:.1} kHz mono · {:.3} s ({} samples before / {} samples after)",
        f64::from(before.sample_rate) / 1000.0,
        before.duration_s,
        before.samples,
        after.samples
    );
    let _ = writeln!(
        md,
        "- **Methodology**: Strictly blind analysis (`Before` inspects raw input only; `After` inspects final export only; `Comparison` reports numeric deltas only after both blind passes complete)."
    );
    let _ = writeln!(md);

    let _ = writeln!(md, "## 1. Comparison Summary (Significant Numeric Deltas)");
    let _ = writeln!(md);
    let _ = writeln!(md, "| Metric | Before | After | Delta |");
    let _ = writeln!(md, "| --- | --- | --- | --- |");
    for d in &comparison.deltas {
        let _ = writeln!(
            md,
            "| {} | {} | {} | {} |",
            d.metric, d.before_value, d.after_value, d.summary
        );
    }
    let _ = writeln!(md);

    let _ = writeln!(md, "## 2. Before Analysis (Blind — Raw Input Only)");
    let _ = writeln!(md);
    let _ = writeln!(md, "| Category | Metric | Value | Tag | Notes |");
    let _ = writeln!(md, "| --- | --- | --- | --- | --- |");
    for f in &before.findings {
        let _ = writeln!(
            md,
            "| {} | {} | {} | `{}` | {} |",
            f.category,
            f.label,
            f.value,
            f.tag.label(),
            f.detail
        );
    }
    let _ = writeln!(md);

    let _ = writeln!(md, "## 3. After Analysis (Blind — Final Export Only)");
    let _ = writeln!(md);
    let _ = writeln!(md, "| Category | Metric | Value | Tag | Notes |");
    let _ = writeln!(md, "| --- | --- | --- | --- | --- |");
    for f in &after.findings {
        let _ = writeln!(
            md,
            "| {} | {} | {} | `{}` | {} |",
            f.category,
            f.label,
            f.value,
            f.tag.label(),
            f.detail
        );
    }
    let _ = writeln!(md);

    let _ = writeln!(md, "## 4. Export Libraries & Fallbacks");
    let _ = writeln!(md);
    let _ = writeln!(
        md,
        "- **craft-libs crates used**: {}",
        craft_libs.crates_used.join(", ")
    );
    let _ = writeln!(
        md,
        "- **Unavailable / fallback notes**: {}",
        craft_libs.unavailable_fallbacks.join(" ")
    );
    md
}

/// Top-level blind Before-and-After technical report builder.
/// Runs `analyze_before_blind` and `analyze_after_blind` independently, then computes
/// `build_comparison_summary` and shared-scale viewer plots from the real audio buffers.
pub fn build_technical_report(
    before_samples: &[f32],
    after_samples: &[f32],
    rate: u32,
    title: &str,
    pipeline_label: &str,
) -> Result<TechnicalReport> {
    let before = analyze_before_blind(before_samples, rate)?;
    let after = analyze_after_blind(after_samples, rate)?;
    let comparison = build_comparison_summary(&before, &after);
    let plots = build_viewer_plots(before_samples, after_samples, rate, &before, &after);
    let craft_libs = crate::report_export::craft_libs_integration_summary();
    let whitepaper_markdown = format_whitepaper_markdown(
        title,
        pipeline_label,
        &before,
        &after,
        &comparison,
        &craft_libs,
    );

    Ok(TechnicalReport {
        schema_version: 1,
        title: title.to_string(),
        pipeline_label: pipeline_label.to_string(),
        before,
        after,
        comparison,
        plots,
        whitepaper_markdown,
        craft_libs,
    })
}

// Export functions (`export_report`, `export_report_pdf_craft`, `export_report_markdown`)
// are isolated in `crate::report_export` and re-exported at the top of this module.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blind_after_report_is_completely_independent_of_before_input() {
        let rate = 48_000_u32;
        let n = rate as usize * 2;
        let clean: Vec<f32> = (0..n)
            .map(|i| (0.25 * (TAU * 440.0 * i as f64 / f64::from(rate)).sin()) as f32)
            .collect();
        let noisy_dc: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                (0.08 + 0.15 * (TAU * 60.0 * t).sin() + 0.25 * (TAU * 440.0 * t).sin()) as f32
            })
            .collect();

        let rep_a = build_technical_report(&clean, &clean, rate, "A", "bypass").unwrap();
        let rep_b = build_technical_report(&noisy_dc, &clean, rate, "B", "cleaned").unwrap();

        // After report MUST be byte-for-byte identical regardless of Before input!
        assert_eq!(rep_a.after, rep_b.after);
        assert!(rep_b.before.dc_present);
        assert!(!rep_b.after.dc_present);
        assert!(
            rep_b
                .comparison
                .deltas
                .iter()
                .any(|d| d.metric == "DC offset" && d.summary.contains("DC removed")),
            "Expected DC removal delta: {:?}",
            rep_b.comparison.deltas
        );
    }

    #[test]
    fn dropout_detector_catches_mid_tone_dropouts_and_ignores_musical_rests() {
        let rate = 48_000_u32;
        let fs = f64::from(rate);
        let n = rate as usize * 2;
        // 1. Steady tone with an abrupt 20 ms zero dropout in the middle -> MUST detect >= 1 dropout
        let mut with_dropout: Vec<f32> = (0..n)
            .map(|i| (0.3 * (TAU * 330.0 * i as f64 / fs).sin()) as f32)
            .collect();
        for s in &mut with_dropout[48_000..49_200] {
            *s = 0.0;
        }
        let rep_drop = analyze_after_blind(&with_dropout, rate).unwrap();
        assert!(
            rep_drop.dropout_count >= 1,
            "Expected genuine abrupt 25 ms mid-tone dropout to be caught"
        );

        // 2. Clean plucked notes that naturally decay into short musical rests -> 0 dropouts
        let mut plucked = vec![0.0_f32; n];
        for note_idx in 0..4 {
            let start = note_idx * 24_000;
            let len = 19_200; // 400 ms note followed by 100 ms rest
            for j in 0..len {
                let t = j as f64 / fs;
                let env = (1.0 - (-t / 0.004).exp()) * (-t / 0.09).exp();
                plucked[start + j] = (0.35 * env * (TAU * 220.0 * t).sin()) as f32;
            }
        }
        let rep_plucked = analyze_after_blind(&plucked, rate).unwrap();
        assert_eq!(
            rep_plucked.dropout_count, 0,
            "Natural plucked-note decay into musical rests must not be misclassified as dropouts"
        );
    }
}
