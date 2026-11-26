//! Reference Shape extraction for shape-guided regeneration.
//!
//! Extracts an inspectable, non-destructive description of the performance shape
//! already present in a single-source reference recording:
//!   - Pitch trajectory (including bends and slides, never forced into published ranges)
//!   - Amplitude envelope and dynamic contrast
//!   - Broad spectral envelope / formant-like prominence regions
//!   - Harmonic vs noise balance & single-source bleed caution
//!   - Attack character, decay / sustain behavior, and re-articulation strikes
//!   - Gesture transitions and overlapping ringing tails
//!   - Explicit evidence & uncertainty accounting (never flattened or invented)
//!
//! All analysis runs locally in Rust with zero external training sets or network access.
//! Original samples are read-only evidence and are never copied into synthesized audio.

use crate::{
    digest,
    reperform::{EvidenceStatus, Performance},
    studio_catalog::SourceEntry,
    Result,
};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

pub const SHAPE_BANDS_HZ: [f64; 6] = [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PitchTrajectoryShape {
    pub note_count: usize,
    pub min_hz: Option<f64>,
    pub max_hz: Option<f64>,
    pub median_hz: Option<f64>,
    pub max_bend_semitones: f64,
    pub mean_pitch_span_semitones: f64,
    pub notes_with_bends: usize,
    pub out_of_published_range_notes: usize,
    pub published_range_note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynamicsShape {
    pub peak_dbfs: f64,
    pub rms_dbfs: f64,
    pub dynamic_range_db: f64,
    pub note_level_contrast_db: f64,
    pub strongest_note_index: Option<usize>,
    pub quietest_note_index: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpectralProminence {
    pub center_hz: f64,
    pub relative_db: f64,
    pub band_label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpectralEnvelopeShape {
    pub spectral_centroid_hz: f64,
    pub band_centers_hz: [f64; 6],
    pub band_relative_db: [f64; 6],
    pub prominence_regions: Vec<SpectralProminence>,
    pub envelope_note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HarmonicNoiseShape {
    pub mean_tonal_fraction: f64,
    pub min_tonal_fraction: f64,
    pub max_tonal_fraction: f64,
    pub mean_noise_fraction: f64,
    pub balance_label: String,
    pub bleed_risk: String,
    pub bleed_note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttackDecayShape {
    pub median_attack_ms: f64,
    pub median_low_decay_per_s: f64,
    pub median_high_decay_per_s: f64,
    pub sustain_ratio: f64,
    pub total_repicks: usize,
    pub attack_character_label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransitionsShape {
    pub overlapping_ring_notes: usize,
    pub connected_transitions: usize,
    pub separated_transitions: usize,
    pub continuity_ratio: f64,
    pub transition_note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UncertaintySummary {
    pub total_frames: usize,
    pub supported_frames: usize,
    pub uncertain_frames: usize,
    pub out_of_target_frames: usize,
    pub silence_frames: usize,
    pub uncertain_pct: f64,
    pub out_of_target_pct: f64,
    pub flags: Vec<String>,
    pub limits_note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReferenceShape {
    pub shape_sha256: String,
    pub sample_rate: u32,
    pub duration_s: f64,
    pub pitch_trajectory: PitchTrajectoryShape,
    pub dynamics: DynamicsShape,
    pub spectral_envelope: SpectralEnvelopeShape,
    pub harmonic_vs_noise: HarmonicNoiseShape,
    pub attack_and_decay: AttackDecayShape,
    pub transitions: TransitionsShape,
    pub uncertainty: UncertaintySummary,
    pub signal_character: crate::reference_profiles::SignalCharacterEstimate,
}

fn median_f64(vals: &mut [f64]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    vals.sort_by(f64::total_cmp);
    let n = vals.len();
    if n % 2 == 1 {
        vals[n / 2]
    } else {
        0.5 * (vals[n / 2 - 1] + vals[n / 2])
    }
}

fn to_dbfs(linear: f64) -> f64 {
    if linear <= 1e-9 {
        -120.0
    } else {
        (20.0 * linear.log10()).max(-120.0)
    }
}

fn compute_spectral_envelope(samples: &[f32], rate: u32) -> SpectralEnvelopeShape {
    if samples.is_empty() || rate == 0 {
        return SpectralEnvelopeShape {
            spectral_centroid_hz: 0.0,
            band_centers_hz: SHAPE_BANDS_HZ,
            band_relative_db: [-60.0; 6],
            prominence_regions: Vec::new(),
            envelope_note: "No audio samples available for spectral envelope estimation.".into(),
        };
    }

    let n = 2048_usize;
    let hop = 512_usize;
    let fft = FftPlanner::<f64>::new().plan_fft_forward(n);
    let win: Vec<f64> = (0..n)
        .map(|i| 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos())
        .collect();

    let mut band_power = [0.0_f64; 6];
    let mut active_frames = 0_usize;
    let mut num_centroid = 0.0_f64;
    let mut den_centroid = 0.0_f64;

    // Octave-band edges around 125, 250, 500, 1000, 2000, 4000 Hz
    let band_edges: [(f64, f64); 6] = [
        (88.4, 176.8),
        (176.8, 353.6),
        (353.6, 707.1),
        (707.1, 1414.2),
        (1414.2, 2828.4),
        (2828.4, 5656.9),
    ];

    let mut buf = vec![Complex::new(0.0_f64, 0.0_f64); n];
    let bin_hz = f64::from(rate) / n as f64;

    let mut pos = 0_usize;
    while pos < samples.len() {
        let mut frame_energy = 0.0_f64;
        for i in 0..n {
            let s = samples.get(pos + i).copied().unwrap_or(0.0) as f64;
            frame_energy += s * s;
            buf[i] = Complex::new(s * win[i], 0.0);
        }
        let rms = (frame_energy / n as f64).sqrt();
        if rms > 0.002 {
            fft.process(&mut buf);
            active_frames += 1;
            for (k, z) in buf.iter().enumerate().take(n / 2).skip(1) {
                let hz = k as f64 * bin_hz;
                let p = z.norm_sqr();
                num_centroid += hz * p;
                den_centroid += p;
                for (b, &(lo, hi)) in band_edges.iter().enumerate() {
                    if hz >= lo && hz < hi {
                        band_power[b] += p;
                        break;
                    }
                }
            }
        }
        if samples.len() <= n {
            break;
        }
        pos += hop;
    }

    let spectral_centroid_hz = if den_centroid > 1e-18 {
        num_centroid / den_centroid
    } else {
        0.0
    };

    let peak_band = band_power
        .iter()
        .copied()
        .fold(0.0_f64, f64::max)
        .max(1e-18);
    let mut band_relative_db = [-60.0_f64; 6];
    for b in 0..6 {
        let ratio = (band_power[b] / peak_band).max(1e-6);
        band_relative_db[b] = (10.0 * ratio.log10()).clamp(-60.0, 0.0);
    }

    let labels = [
        "Low body (125 Hz)",
        "Warmth / chest (250 Hz)",
        "Lower midrange (500 Hz)",
        "Core vocal/body (1 kHz)",
        "Presence / attack (2 kHz)",
        "Brilliance / air (4 kHz)",
    ];
    let mut ranked: Vec<(usize, f64)> = band_relative_db.iter().copied().enumerate().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let prominence_regions: Vec<SpectralProminence> = ranked
        .into_iter()
        .take(2)
        .filter(|&(_, db)| db > -24.0 && active_frames > 0)
        .map(|(idx, db)| SpectralProminence {
            center_hz: SHAPE_BANDS_HZ[idx],
            relative_db: (db * 10.0).round() / 10.0,
            band_label: labels[idx].to_string(),
        })
        .collect();

    SpectralEnvelopeShape {
        spectral_centroid_hz,
        band_centers_hz: SHAPE_BANDS_HZ,
        band_relative_db,
        prominence_regions,
        envelope_note: format!(
            "Broad 6-band reference envelope across {} active frames (formant-like prominence guide only; no original samples copied).",
            active_frames
        ),
    }
}

/// Extract a full, inspectable ReferenceShape from the imported single-source WAV
/// and its extracted Performance cues. Optionally compares observed pitch against
/// the selected SourceEntry's published range without ever clamping or discarding notes.
pub fn extract_reference_shape(
    samples: &[f32],
    rate: u32,
    perf: &Performance,
    selected_source: Option<&SourceEntry>,
) -> Result<ReferenceShape> {
    let cues_json = serde_json::to_vec(perf)?;
    let shape_sha256 = digest::sha256(&cues_json);
    let summary = perf.summary();
    let notes = perf.notes();
    let evidence = perf.frame_evidence();

    // 1. Pitch trajectory & bends (never clamped to published instrument range)
    let mut all_note_medians = Vec::with_capacity(notes.len());
    let mut min_hz = f64::INFINITY;
    let mut max_hz = 0.0_f64;
    let mut max_bend_semitones = 0.0_f64;
    let mut total_span_semitones = 0.0_f64;
    let mut notes_with_bends = 0_usize;
    let mut out_of_published_range_notes = 0_usize;

    let pub_lo = selected_source.and_then(|s| s.published_range_hz.map(|(lo, _)| f64::from(lo)));
    let pub_hi = selected_source.and_then(|s| {
        s.published_practical_range_hz
            .or(s.published_range_hz)
            .map(|(_, hi)| f64::from(hi))
    });

    for note in notes {
        let mut hz_vals: Vec<f64> = note
            .hz
            .iter()
            .copied()
            .filter(|v| v.is_finite() && *v > 0.0)
            .collect();
        if hz_vals.is_empty() {
            continue;
        }
        let n_min = hz_vals.iter().copied().fold(f64::INFINITY, f64::min);
        let n_max = hz_vals.iter().copied().fold(0.0_f64, f64::max);
        let n_med = median_f64(&mut hz_vals);
        all_note_medians.push(n_med);
        if n_min < min_hz {
            min_hz = n_min;
        }
        if n_max > max_hz {
            max_hz = n_max;
        }
        let span_semi = if n_min > 1.0 && n_max > n_min {
            12.0 * (n_max / n_min).log2()
        } else {
            0.0
        };
        total_span_semitones += span_semi;
        if span_semi > max_bend_semitones {
            max_bend_semitones = span_semi;
        }
        if span_semi >= 0.35 {
            notes_with_bends += 1;
        }
        let below = pub_lo.is_some_and(|lo| n_med < lo);
        let above = pub_hi.is_some_and(|hi| n_med > hi);
        if below || above {
            out_of_published_range_notes += 1;
        }
    }

    let mean_pitch_span_semitones = if !all_note_medians.is_empty() {
        total_span_semitones / all_note_medians.len() as f64
    } else {
        0.0
    };
    let median_hz = if !all_note_medians.is_empty() {
        Some(median_f64(&mut all_note_medians))
    } else {
        None
    };

    let published_range_note = match (selected_source, out_of_published_range_notes) {
        (Some(src), 0) => format!(
            "All {} extracted notes sit within published range for {} (cues remain source of truth).",
            notes.len(),
            src.name
        ),
        (Some(src), n) => format!(
            "{} note(s) lie outside published range for {} — kept exactly as performed (never clamped).",
            n, src.name
        ),
        (None, _) => "No source selected for range comparison; extracted pitch trajectory kept as performed.".into(),
    };

    let pitch_trajectory = PitchTrajectoryShape {
        note_count: notes.len(),
        min_hz: if min_hz.is_finite() {
            Some(min_hz)
        } else {
            None
        },
        max_hz: if max_hz > 0.0 { Some(max_hz) } else { None },
        median_hz,
        max_bend_semitones,
        mean_pitch_span_semitones,
        notes_with_bends,
        out_of_published_range_notes,
        published_range_note,
    };

    // 2. Amplitude envelope & dynamics
    let peak_lin = samples
        .iter()
        .map(|v| f64::from(v.abs()))
        .fold(0.0_f64, f64::max);
    let rms_lin = if samples.is_empty() {
        0.0
    } else {
        (samples
            .iter()
            .map(|v| {
                let x = f64::from(*v);
                x * x
            })
            .sum::<f64>()
            / samples.len() as f64)
            .sqrt()
    };
    let peak_dbfs = to_dbfs(peak_lin);
    let rms_dbfs = to_dbfs(rms_lin);
    let dynamic_range_db = (peak_dbfs - rms_dbfs).max(0.0);

    let mut strongest: Option<(usize, f64)> = None;
    let mut quietest: Option<(usize, f64)> = None;
    for (idx, note) in notes.iter().enumerate() {
        let p = note.level_steps.iter().copied().fold(0.0_f64, f64::max);
        if p > 1e-6 {
            if strongest.is_none_or(|(_, best)| p > best) {
                strongest = Some((idx, p));
            }
            if quietest.is_none_or(|(_, low)| p < low) {
                quietest = Some((idx, p));
            }
        }
    }
    let note_level_contrast_db = match (strongest, quietest) {
        (Some((_, hi)), Some((_, lo))) if lo > 1e-6 => (20.0 * (hi / lo).log10()).max(0.0),
        _ => 0.0,
    };

    let dynamics = DynamicsShape {
        peak_dbfs,
        rms_dbfs,
        dynamic_range_db,
        note_level_contrast_db,
        strongest_note_index: strongest.map(|(i, _)| i),
        quietest_note_index: quietest.map(|(i, _)| i),
    };

    // 3. Broad spectral envelope / formant-like prominence regions
    let spectral_envelope = compute_spectral_envelope(samples, rate);

    // 4. Harmonic vs noise balance & single-source bleed assessment
    let active_tonals: Vec<f64> = evidence
        .iter()
        .filter(|f| f.status != EvidenceStatus::Silence && f.tonal_fraction.is_finite())
        .map(|f| f.tonal_fraction.clamp(0.0, 1.0))
        .collect();
    let (mean_tonal, min_tonal, max_tonal) = if active_tonals.is_empty() {
        (0.0, 0.0, 0.0)
    } else {
        let sum: f64 = active_tonals.iter().sum();
        let mn = active_tonals.iter().copied().fold(1.0_f64, f64::min);
        let mx = active_tonals.iter().copied().fold(0.0_f64, f64::max);
        (sum / active_tonals.len() as f64, mn, mx)
    };
    let mean_noise = (1.0 - mean_tonal).clamp(0.0, 1.0);
    let balance_label = if active_tonals.is_empty() {
        "No active frames"
    } else if mean_tonal >= 0.72 {
        "Predominantly harmonic / tonal"
    } else if mean_tonal >= 0.45 {
        "Mixed harmonic + breath / bow / pick texture"
    } else {
        "Noise- or transient-heavy"
    }
    .to_string();

    let non_silence =
        (summary.supported_frames + summary.uncertain_frames + summary.out_of_target_frames).max(1);
    let out_frac = summary.out_of_target_frames as f64 / non_silence as f64;
    let unc_frac = summary.uncertain_frames as f64 / non_silence as f64;
    // Treat a frame as competing multi-source / bleed energy only when the primary
    // fundamental leaves significant unexplained tonal energy (< 0.72) AND a second
    // non-harmonic hypothesis competes strongly (excluding natural sub/super-harmonics).
    let multi_hyp_frames = evidence
        .iter()
        .filter(|f| {
            if f.status == EvidenceStatus::Silence || f.hypotheses.len() < 2 {
                return false;
            }
            let h0 = &f.hypotheses[0];
            let h1 = &f.hypotheses[1];
            if h0.hz <= 1.0 || h0.explained_fraction >= 0.72 || h1.score < h0.score * 0.80 {
                return false;
            }
            let ratio = h1.hz / h0.hz;
            let harmonic_ratios = [0.25, 0.3333, 0.5, 0.6667, 1.5, 2.0, 3.0, 4.0];
            let is_harmonic_alias = harmonic_ratios
                .iter()
                .any(|&r| ((ratio - r) / r).abs() < 0.05);
            !is_harmonic_alias
        })
        .count();
    let multi_hyp_frac = multi_hyp_frames as f64 / non_silence as f64;

    let (bleed_risk, bleed_note) = if out_frac >= 0.20 || multi_hyp_frac >= 0.25 {
        (
            "Elevated (possible bleed or multi-source overlap)".to_string(),
            format!(
                "{:.1}% out-of-target frames and {:.1}% competing pitch hypotheses. Shape-guided synthesis is strongest on an isolated single source; significant bleed can mix shapes.",
                out_frac * 100.0,
                multi_hyp_frac * 100.0
            ),
        )
    } else if out_frac >= 0.06 || unc_frac >= 0.15 {
        (
            "Moderate (ambiguous / residual energy present)".to_string(),
            format!(
                "{:.1}% uncertain and {:.1}% out-of-target active frames. Unclear spans stay marked uncertain rather than invented.",
                unc_frac * 100.0,
                out_frac * 100.0
            ),
        )
    } else {
        (
            "Low (coherent single-source evidence)".to_string(),
            "Active frames show coherent single-source harmonic tracking with minimal competing energy.".to_string(),
        )
    };

    let harmonic_vs_noise = HarmonicNoiseShape {
        mean_tonal_fraction: mean_tonal,
        min_tonal_fraction: min_tonal,
        max_tonal_fraction: max_tonal,
        mean_noise_fraction: mean_noise,
        balance_label,
        bleed_risk,
        bleed_note,
    };

    // 5. Attack character & decay / sustain behavior
    let mut attack_ms_vals = Vec::with_capacity(notes.len());
    let mut sustain_ratios = Vec::with_capacity(notes.len());
    let mut low_decays = Vec::new();
    let mut high_decays = Vec::new();
    let hop_ms = perf.hop_s() * 1000.0;

    for note in notes {
        if !note.level_steps.is_empty() {
            let peak = note.level_steps.iter().copied().fold(0.0_f64, f64::max);
            if peak > 1e-6 {
                let thresh_attack = peak * 0.85;
                let rise_idx = note
                    .level_steps
                    .iter()
                    .position(|&v| v >= thresh_attack)
                    .unwrap_or(0);
                attack_ms_vals.push(((rise_idx + 1) as f64 * hop_ms).max(hop_ms));

                let thresh_sustain = peak * 0.25; // ~ -12 dB of note peak
                let sustained_steps = note
                    .level_steps
                    .iter()
                    .filter(|&&v| v >= thresh_sustain)
                    .count();
                sustain_ratios.push(sustained_steps as f64 / note.level_steps.len() as f64);
            }
        }
        for (&k, &d) in &note.shape.decay_per_second {
            if k <= 3 {
                low_decays.push(d);
            } else {
                high_decays.push(d);
            }
        }
    }

    let median_attack_ms = if attack_ms_vals.is_empty() {
        0.0
    } else {
        median_f64(&mut attack_ms_vals)
    };
    let sustain_ratio = if sustain_ratios.is_empty() {
        0.0
    } else {
        median_f64(&mut sustain_ratios)
    };
    let median_low_decay_per_s = if low_decays.is_empty() {
        1.1
    } else {
        median_f64(&mut low_decays)
    };
    let median_high_decay_per_s = if high_decays.is_empty() {
        2.8
    } else {
        median_f64(&mut high_decays)
    };

    let attack_character_label = if notes.is_empty() {
        "No note onsets extracted"
    } else if median_attack_ms <= 18.0 && sustain_ratio < 0.60 {
        "Percussive / plucked onset with decaying tail"
    } else if median_attack_ms <= 24.0 {
        "Crisp onset with sustained body"
    } else {
        "Gradual / bowed / breath-led onset"
    }
    .to_string();

    let attack_and_decay = AttackDecayShape {
        median_attack_ms,
        median_low_decay_per_s,
        median_high_decay_per_s,
        sustain_ratio,
        total_repicks: summary.total_repicks,
        attack_character_label,
    };

    // 6. Gesture transitions & ring overlaps
    let mut connected = 0_usize;
    let mut separated = 0_usize;
    for pair in notes.windows(2) {
        let gap = pair[1].start - pair[0].end;
        if gap <= 0.045 {
            connected += 1;
        } else {
            separated += 1;
        }
    }
    let total_trans = (connected + separated).max(1);
    let continuity_ratio = connected as f64 / total_trans as f64;
    let transition_note = if notes.len() < 2 {
        "Single gesture or empty track — no inter-note transitions.".to_string()
    } else {
        format!(
            "{} overlapping ring tail(s), {} connected/legato handoff(s), {} separated gap(s).",
            summary.overlapping_ring_notes, connected, separated
        )
    };

    let transitions = TransitionsShape {
        overlapping_ring_notes: summary.overlapping_ring_notes,
        connected_transitions: connected,
        separated_transitions: separated,
        continuity_ratio,
        transition_note,
    };

    // 7. Evidence & uncertainty accounting
    let total_frames = evidence.len();
    let total_f = total_frames.max(1) as f64;
    let uncertain_pct = (summary.uncertain_frames as f64 / total_f) * 100.0;
    let out_of_target_pct = (summary.out_of_target_frames as f64 / total_f) * 100.0;

    let mut flags = Vec::new();
    if summary.uncertain_frames > 0 {
        flags.push(format!(
            "{} uncertain frame(s) ({:.1}%) preserved as ambiguous — not flattened into confident notes.",
            summary.uncertain_frames, uncertain_pct
        ));
    }
    if summary.out_of_target_frames > 0 {
        flags.push(format!(
            "{} out-of-target frame(s) ({:.1}%) excluded from dry synthesis rather than converted into noise.",
            summary.out_of_target_frames, out_of_target_pct
        ));
    }
    if out_of_published_range_notes > 0 {
        flags.push(format!(
            "{} note(s) outside selected source published range — kept untouched from signal evidence.",
            out_of_published_range_notes
        ));
    }
    if notes_with_bends > 0 {
        flags.push(format!(
            "{} note(s) carry pitch bends/slides (up to {:.2} semitones) — preserved without pitch quantization.",
            notes_with_bends, max_bend_semitones
        ));
    }
    if flags.is_empty() {
        flags.push(
            "Coherent single-source evidence extracted; human Dry Listening Gate still required before capture.".into(),
        );
    }

    let uncertainty = UncertaintySummary {
        total_frames,
        supported_frames: summary.supported_frames,
        uncertain_frames: summary.uncertain_frames,
        out_of_target_frames: summary.out_of_target_frames,
        silence_frames: summary.silence_frames,
        uncertain_pct,
        out_of_target_pct,
        flags,
        limits_note: "Extracted shape is an interpretation of the signal, not a perfect record of player intent. Ambiguous events stay marked uncertain; no original samples are blended into output.".into(),
    };

    let signal_character = crate::reference_profiles::estimate_signal_character(
        samples,
        rate,
        perf,
        &spectral_envelope.band_relative_db,
        harmonic_vs_noise.mean_tonal_fraction,
    );

    Ok(ReferenceShape {
        shape_sha256,
        sample_rate: rate,
        duration_s: summary.duration_s,
        pitch_trajectory,
        dynamics,
        spectral_envelope,
        harmonic_vs_noise,
        attack_and_decay,
        transitions,
        uncertainty,
        signal_character,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{reperform::analyze_guitar, studio_catalog::source_catalog};

    #[test]
    fn reference_shape_extracts_all_descriptors_and_holds_shape_hash_fixed_across_sources() {
        let rate = 48_000_u32;
        let len = rate as usize * 2;
        let mut x = vec![0.0_f32; len];
        // Two clear notes: first with a 2-semitone upward bend (220 -> 247 Hz), second overlapping at 330 Hz
        let mut phase1 = 0.0_f64;
        for (i, s) in x.iter_mut().enumerate().take(rate as usize) {
            let t = i as f64 / f64::from(rate);
            let hz = 220.0 + 27.0 * (t / 1.0).clamp(0.0, 1.0);
            phase1 += TAU * hz / f64::from(rate);
            let env = (t / 0.015).min(1.0) * (-2.2 * t).exp();
            *s += (0.35
                * env
                * (phase1.sin() + 0.4 * (2.0 * phase1).sin() + 0.2 * (3.0 * phase1).sin()))
                as f32;
        }
        let mut phase2 = 0.0_f64;
        for (i, s) in x.iter_mut().enumerate().skip((rate as f64 * 0.9) as usize) {
            let t = (i as f64 / f64::from(rate)) - 0.9;
            phase2 += TAU * 330.0 / f64::from(rate);
            let env = (t / 0.012).min(1.0) * (-1.8 * t).exp();
            *s += (0.28 * env * (phase2.sin() + 0.35 * (2.0 * phase2).sin())) as f32;
        }

        let perf = analyze_guitar(&x, rate).unwrap();
        let sources = source_catalog();
        let src_a = &sources[0]; // Electric Guitar
        let src_b = sources
            .iter()
            .find(|s| {
                s.name.contains("Piccolo") || s.published_range_hz.is_some_and(|(lo, _)| lo > 400.0)
            })
            .unwrap_or(&sources[1]);

        let shape_a = extract_reference_shape(&x, rate, &perf, Some(src_a)).unwrap();
        let shape_b = extract_reference_shape(&x, rate, &perf, Some(src_b)).unwrap();

        // Extracted shape identity MUST stay identical regardless of chosen voicing source
        assert_eq!(shape_a.shape_sha256, shape_b.shape_sha256);
        assert!(shape_a.pitch_trajectory.note_count >= 1);
        assert!(shape_a.pitch_trajectory.max_bend_semitones > 0.5);
        assert!(shape_a.spectral_envelope.spectral_centroid_hz > 100.0);
        assert!(shape_a.dynamics.peak_dbfs > -20.0);
        assert!(shape_a.harmonic_vs_noise.mean_tonal_fraction > 0.3);
        // Selecting a high-pitched source flags out-of-range notes WITHOUT dropping or clamping them
        if src_b.published_range_hz.is_some_and(|(lo, _)| lo > 350.0) {
            assert!(shape_b.pitch_trajectory.out_of_published_range_notes >= 1);
            assert_eq!(
                shape_b.pitch_trajectory.note_count,
                shape_a.pitch_trajectory.note_count
            );
            assert_eq!(
                shape_b.pitch_trajectory.min_hz,
                shape_a.pitch_trajectory.min_hz
            );
        }
    }

    #[test]
    fn noisy_ambiguous_audio_preserves_uncertainty_instead_of_inventing_notes() {
        let rate = 48_000_u32;
        let mut seed: u64 = 0x1234_5678_9abc_def0;
        let x: Vec<f32> = (0..rate as usize)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let u = ((seed >> 33) as f64 / u32::MAX as f64) * 2.0 - 1.0;
                (0.18 * u) as f32
            })
            .collect();
        let perf = analyze_guitar(&x, rate).unwrap();
        let shape = extract_reference_shape(&x, rate, &perf, None).unwrap();
        // Pure broadband noise must NOT be silently converted into confident tonal notes
        assert_eq!(shape.pitch_trajectory.note_count, 0);
        assert!(
            shape.uncertainty.uncertain_frames + shape.uncertainty.out_of_target_frames > 0,
            "expected non-zero uncertain or out-of-target frames on noise"
        );
        assert!(shape.harmonic_vs_noise.mean_noise_fraction > 0.5);
    }
}
