//! Stage 2 — Surgical Region Editing on the Dry Take.
//!
//! Operates strictly on the cached dry take before any optional microphone, room, or plugin stages.
//! Re-processes only the shape cues inside the marked time (or time-frequency marquee) region through
//! the DDSP-style synthesizer, then splices the segment back using raised-cosine crossfades so all
//! samples outside the region remain bit-identical. Original audio is never opened or blended.

use crate::{
    reperform::{Performance, StraightenMode, VoicingControls},
    studio_catalog::SourceEntry,
    Result,
};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionFixAction {
    Straighten,
    Smooth,
    ReduceNoise,
    Stabilize,
    NoteOnly,
}

impl Default for RegionFixAction {
    fn default() -> Self {
        Self::Stabilize
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DryRegionEdit {
    #[serde(default = "default_region_id")]
    pub id: String,
    /// "marquee" (time + frequency bounds) or "range" (time bounds only).
    #[serde(default = "default_region_kind")]
    pub kind: String,
    pub start_s: f64,
    pub end_s: f64,
    #[serde(default)]
    pub min_hz: Option<f64>,
    #[serde(default)]
    pub max_hz: Option<f64>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub action: RegionFixAction,
    /// Mild user-controlled strength (0.0..=1.0).
    #[serde(default = "default_strength")]
    pub strength: f64,
    /// Edge crossfade duration in milliseconds (4..=60 ms).
    #[serde(default = "default_crossfade_ms")]
    pub crossfade_ms: f64,
}

fn default_region_id() -> String {
    "region-1".into()
}
fn default_region_kind() -> String {
    "range".into()
}
fn default_strength() -> f64 {
    0.55
}
fn default_crossfade_ms() -> f64 {
    12.0
}

#[derive(Clone, Debug)]
pub struct RegionEditOutput {
    pub edited_dry: Vec<f32>,
    pub before_slice: Vec<f32>,
    pub after_slice: Vec<f32>,
    pub start_sample: usize,
    pub end_sample: usize,
    pub crossfade_samples: usize,
    pub affected_notes: usize,
    pub effective_action: RegionFixAction,
    pub unresolved_note: Option<String>,
    pub outside_bit_identical: bool,
}

/// Resolve an explicit localized action when user writes a short note in `NoteOnly` mode.
/// Returns `None` if the note is empty or does not match a supported localized edit,
/// rather than silently executing an unrequested DSP pass.
fn resolve_effective_action(action: RegionFixAction, note: &str) -> Option<RegionFixAction> {
    if action != RegionFixAction::NoteOnly {
        return Some(action);
    }
    let lower = note.to_ascii_lowercase();
    if lower.contains("straight") || lower.contains("pitch") || lower.contains("tune") {
        Some(RegionFixAction::Straighten)
    } else if lower.contains("noise")
        || lower.contains("hiss")
        || lower.contains("clean")
        || lower.contains("distort")
    {
        Some(RegionFixAction::ReduceNoise)
    } else if lower.contains("smooth") || lower.contains("harsh") {
        Some(RegionFixAction::Smooth)
    } else if lower.contains("stabiliz")
        || lower.contains("steady")
        || lower.contains("level")
        || lower.contains("even")
    {
        Some(RegionFixAction::Stabilize)
    } else {
        None
    }
}

/// Filter a time-domain edit delta so only frequencies inside `[flo, fhi]` are retained,
/// using 75%-overlap Hann STFT overlap-add masking for long regions (preventing temporal
/// pre-echo) and a smooth raised-cosine spectral taper to avoid brickwall ringing.
fn bandpass_delta_fft(delta: &[f64], rate: f64, flo: f64, fhi: f64) -> Vec<f64> {
    let n = delta.len();
    if n == 0 {
        return Vec::new();
    }

    let nyquist = rate * 0.5;
    let lo = flo.clamp(0.0, nyquist);
    let hi = fhi.clamp(lo, nyquist);
    let bw = (hi - lo).max(1.0);
    let taper = (bw * 0.10).clamp(15.0, 200.0).min(bw * 0.45);

    let bin_gain = |f: f64| -> f64 {
        if f < lo - taper || f > hi + taper {
            0.0
        } else if f < lo && taper > 0.0 {
            let t = (f - (lo - taper)) / taper;
            0.5 * (1.0 - (PI * t).cos())
        } else if f > hi && taper > 0.0 {
            let t = ((hi + taper) - f) / taper;
            0.5 * (1.0 - (PI * t).cos())
        } else {
            1.0
        }
    };

    // Short selections use a single zero-padded FFT so spectral resolution stays tight.
    if n < 1024 {
        let fft_len = (n * 2).next_power_of_two().max(64);
        let mut planner = FftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(fft_len);
        let ifft = planner.plan_fft_inverse(fft_len);
        let mut spectrum = vec![Complex::new(0.0_f64, 0.0_f64); fft_len];
        for (i, &v) in delta.iter().enumerate() {
            spectrum[i].re = v;
        }
        fft.process(&mut spectrum);
        let bin_hz = rate / fft_len as f64;
        for (k, bin) in spectrum.iter_mut().enumerate() {
            let sym_k = if k <= fft_len / 2 { k } else { fft_len - k };
            *bin *= bin_gain(sym_k as f64 * bin_hz);
        }
        ifft.process(&mut spectrum);
        let inv_len = 1.0 / fft_len as f64;
        return spectrum
            .into_iter()
            .take(n)
            .map(|c| c.re * inv_len)
            .collect();
    }

    // Long selections use 75%-overlap Hann STFT overlap-add (window=1024, hop=256)
    // to eliminate long-range temporal pre-echo across multi-second regions.
    let win_len = 1024_usize;
    let hop = win_len / 4;
    let fft_len = win_len * 2; // 2x zero-padding prevents circular convolution wrap-around
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(fft_len);
    let ifft = planner.plan_fft_inverse(fft_len);
    let inv_fft = 1.0 / fft_len as f64;
    let bin_hz = rate / fft_len as f64;

    let win: Vec<f64> = (0..win_len)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / win_len as f64).cos()))
        .collect();
    let mask: Vec<f64> = (0..fft_len)
        .map(|k| {
            let sym_k = if k <= fft_len / 2 { k } else { fft_len - k };
            bin_gain(sym_k as f64 * bin_hz)
        })
        .collect();

    let mut out = vec![0.0_f64; n];
    let mut norm = vec![0.0_f64; n];
    let mut buf = vec![Complex::new(0.0_f64, 0.0_f64); fft_len];

    let mut start_i = -(win_len as isize) + hop as isize;
    while start_i < n as isize {
        for c in &mut buf {
            *c = Complex::new(0.0, 0.0);
        }
        for i in 0..win_len {
            let idx = start_i + i as isize;
            if idx >= 0 && (idx as usize) < n {
                buf[i].re = delta[idx as usize] * win[i];
            }
        }
        fft.process(&mut buf);
        for (c, &g) in buf.iter_mut().zip(mask.iter()) {
            *c *= g;
        }
        ifft.process(&mut buf);
        for i in 0..win_len {
            let idx = start_i + i as isize;
            if idx >= 0 && (idx as usize) < n {
                let u = idx as usize;
                let w = win[i];
                out[u] += buf[i].re * inv_fft * w;
                norm[u] += w * w;
            }
        }
        start_i += hop as isize;
    }

    for i in 0..n {
        if norm[i] > 1e-12 {
            out[i] /= norm[i];
        }
    }
    out
}

/// Apply a localized DDSP-style fix to ONLY the shape inside `edit`'s bounds and splice it
/// back into `cached_dry` with raised-cosine edge crossfades.
/// Samples strictly outside `[start_sample, end_sample]` remain bit-identical to `cached_dry`.
pub fn apply_dry_region_edit(
    cached_dry: &[f32],
    perf: &Performance,
    source: &SourceEntry,
    base_voicing: &VoicingControls,
    edit: &DryRegionEdit,
) -> Result<RegionEditOutput> {
    if cached_dry.len() != perf.samples {
        return Err("Cached dry WAV length does not match performance sample count".into());
    }
    let rate = f64::from(perf.rate);
    let duration_s = perf.samples as f64 / rate;
    if !edit.start_s.is_finite() || !edit.end_s.is_finite() || edit.end_s <= edit.start_s {
        return Err("Region bounds must satisfy end_s > start_s".into());
    }
    let start_s = edit.start_s.clamp(0.0, duration_s);
    let end_s = edit.end_s.clamp(start_s, duration_s);
    let start_sample = (start_s * rate).round() as usize;
    let end_sample = ((end_s * rate).round() as usize).min(cached_dry.len());
    if end_sample.saturating_sub(start_sample) < 32 {
        return Err("Region is too short to process (< 32 samples)".into());
    }

    let band_bounds: Option<(f64, f64)> = match (edit.min_hz, edit.max_hz) {
        (None, None) => None,
        (min_opt, max_opt) => {
            let nyquist = rate * 0.5;
            let raw_lo = min_opt.unwrap_or(0.0);
            let raw_hi = max_opt.unwrap_or(nyquist);
            if !raw_lo.is_finite() || !raw_hi.is_finite() || raw_lo < 0.0 || raw_hi <= 0.0 {
                return Err("Marquee frequency bounds must be finite and positive".into());
            }
            let (flo, fhi) = (raw_lo.min(raw_hi), raw_lo.max(raw_hi));
            if (fhi - flo) < 1.0 {
                return Err("Marquee frequency band must span at least 1 Hz".into());
            }
            Some((flo, fhi))
        }
    };

    let region_len = end_sample - start_sample;
    let fade_ms = edit.crossfade_ms.clamp(4.0, 60.0);
    let fade_samples = ((fade_ms * 0.001 * rate).round() as usize)
        .min(region_len / 2)
        .max(1);

    let strength = edit.strength.clamp(0.0, 1.0);
    let Some(eff_action) = resolve_effective_action(edit.action, &edit.note) else {
        let slice = cached_dry[start_sample..end_sample].to_vec();
        return Ok(RegionEditOutput {
            edited_dry: cached_dry.to_vec(),
            before_slice: slice.clone(),
            after_slice: slice,
            start_sample,
            end_sample,
            crossfade_samples: fade_samples,
            affected_notes: 0,
            effective_action: RegionFixAction::NoteOnly,
            unresolved_note: Some(
                "Note did not match a supported localized fix (straighten/pitch, smooth/harsh, noise/clean, or stabilize/level); dry take left unchanged."
                    .into(),
            ),
            outside_bit_identical: true,
        });
    };

    // Clone shape and apply localized refinement ONLY to frames inside [start_s, end_s]
    // (and overlapping optional [min_hz, max_hz] marquee bounds).
    let mut local_perf = perf.clone();
    let mut affected_notes = 0_usize;

    for note in &mut local_perf.notes {
        if note.end < start_s || note.start > end_s {
            continue;
        }
        // If marquee specifies a frequency band, verify the note's fundamental or first 4 harmonics
        // intersect [flo, fhi] before altering pitch/envelope cues.
        let mut hz_match = true;
        if let Some((flo, fhi)) = band_bounds {
            hz_match = note.hz.iter().any(|&f| {
                (1..=4).any(|h| {
                    let fh = f * h as f64;
                    fh >= flo && fh <= fhi
                })
            });
        }
        if !hz_match {
            continue;
        }
        affected_notes += 1;

        let n_frames = note.hz.len();
        if n_frames < 3 {
            continue;
        }
        // Compute local median pitch & mean level strictly within the region overlap
        let mut reg_hz = Vec::new();
        let mut reg_lvl = Vec::new();
        for (k, (&f, &l)) in note.hz.iter().zip(note.level_steps.iter()).enumerate() {
            let t = note.start + k as f64 * perf.hop_s;
            if t >= start_s && t <= end_s {
                reg_hz.push(f);
                reg_lvl.push(l);
            }
        }
        if reg_hz.is_empty() {
            continue;
        }
        reg_hz.sort_by(f64::total_cmp);
        let median_hz = reg_hz[reg_hz.len() / 2];
        let mean_lvl = reg_lvl.iter().sum::<f64>() / reg_lvl.len() as f64;

        let orig_hz = note.hz.clone();
        let orig_lvl = note.level_steps.clone();
        for k in 0..n_frames {
            let t = note.start + k as f64 * perf.hop_s;
            if t < start_s || t > end_s {
                continue;
            }
            match eff_action {
                RegionFixAction::Straighten => {
                    // Mildly pull wandering pitch toward local pitch center while preserving strong bends
                    let semi_diff = 12.0 * (orig_hz[k] / median_hz.max(1e-6)).log2();
                    if semi_diff.abs() < 1.2 {
                        let pull = (0.55 * strength).clamp(0.1, 0.72);
                        note.hz[k] = orig_hz[k] + (median_hz - orig_hz[k]) * pull;
                    }
                }
                RegionFixAction::Stabilize => {
                    // Combine gentle pitch smoothing (3-frame moving average) + level stabilization
                    let prev = orig_hz[k.saturating_sub(1)];
                    let next = orig_hz[(k + 1).min(n_frames - 1)];
                    let avg_hz = (prev + orig_hz[k] + next) / 3.0;
                    note.hz[k] = orig_hz[k] + (avg_hz - orig_hz[k]) * (0.65 * strength);
                    note.level_steps[k] =
                        orig_lvl[k] + (mean_lvl - orig_lvl[k]) * (0.35 * strength);
                }
                RegionFixAction::Smooth => {
                    // Smooth local amplitude bumps and soften upper modal harshness
                    let prev = orig_lvl[k.saturating_sub(1)];
                    let next = orig_lvl[(k + 1).min(n_frames - 1)];
                    let avg_lvl = (prev + orig_lvl[k] + next) / 3.0;
                    note.level_steps[k] = orig_lvl[k] + (avg_lvl - orig_lvl[k]) * (0.7 * strength);
                }
                RegionFixAction::ReduceNoise | RegionFixAction::NoteOnly => {}
            }
        }

        // For Smooth or ReduceNoise, gently tame upper harmonics on affected notes
        if matches!(
            eff_action,
            RegionFixAction::Smooth | RegionFixAction::ReduceNoise
        ) {
            for h in 5..=14 {
                let cur = note.shape.mode_weights.get(&h).copied().unwrap_or(1.0);
                let factor = (1.0 - 0.28 * strength).clamp(0.45, 1.0);
                note.shape.mode_weights.insert(h, cur * factor);
            }
        }
    }

    // If a frequency-bounded marquee matched zero notes and action is note-pitch/level oriented,
    // keep the dry take strictly unchanged rather than applying global voicing shifts.
    if band_bounds.is_some()
        && affected_notes == 0
        && !matches!(eff_action, RegionFixAction::ReduceNoise)
    {
        let slice = cached_dry[start_sample..end_sample].to_vec();
        return Ok(RegionEditOutput {
            edited_dry: cached_dry.to_vec(),
            before_slice: slice.clone(),
            after_slice: slice,
            start_sample,
            end_sample,
            crossfade_samples: fade_samples,
            affected_notes: 0,
            effective_action: eff_action,
            unresolved_note: None,
            outside_bit_identical: true,
        });
    }

    // Configure localized voicing overlay for re-synthesizing the region through the DDSP engine
    let mut local_voicing = base_voicing.clone();
    match eff_action {
        RegionFixAction::Straighten => {
            local_voicing.straighten_mode = if strength > 0.65 {
                StraightenMode::Moderate
            } else {
                StraightenMode::Light
            };
        }
        RegionFixAction::ReduceNoise => {
            local_voicing.noise_reduction_override = Some((0.65 * strength).clamp(0.2, 0.8));
        }
        RegionFixAction::Smooth => {
            local_voicing.smooth_hf_override = Some((0.55 * strength).clamp(0.15, 0.75));
        }
        RegionFixAction::Stabilize => {
            if local_voicing.straighten_mode == StraightenMode::Off {
                local_voicing.straighten_mode = StraightenMode::Light;
            }
        }
        RegionFixAction::NoteOnly => {}
    }

    // Synthesize fresh localized DDSP candidate strictly from cues (zero original audio!)
    let regen_full = local_perf.synthesize_dry_for_source_with_voicing(source, &local_voicing)?;

    // Compute region delta; if marquee specifies frequency bounds, isolate delta strictly
    // inside [flo, fhi] so out-of-band frequencies in the same time slice remain unaffected.
    let raw_delta: Vec<f64> = (0..region_len)
        .map(|i| {
            let idx = start_sample + i;
            f64::from(regen_full[idx]) - f64::from(cached_dry[idx])
        })
        .collect();
    let scoped_delta = match band_bounds {
        Some((flo, fhi)) => bandpass_delta_fft(&raw_delta, rate, flo, fhi),
        None => raw_delta,
    };

    // Splice ONLY [start_sample..end_sample] back into cached_dry using raised-cosine crossfades
    let mut edited_dry = cached_dry.to_vec();
    for i in 0..region_len {
        let idx = start_sample + i;
        let w_in = if i < fade_samples {
            0.5 * (1.0 - (PI * i as f64 / fade_samples as f64).cos())
        } else {
            1.0
        };
        let dist_end = region_len - 1 - i;
        let w_out = if dist_end < fade_samples {
            0.5 * (1.0 - (PI * dist_end as f64 / fade_samples as f64).cos())
        } else {
            1.0
        };
        let w = w_in * w_out;
        edited_dry[idx] = (f64::from(cached_dry[idx]) + w * scoped_delta[i]) as f32;
    }

    // Verify structural invariant: every sample outside [start_sample..end_sample] is bit-identical
    let outside_bit_identical = edited_dry[..start_sample] == cached_dry[..start_sample]
        && edited_dry[end_sample..] == cached_dry[end_sample..];

    let before_slice = cached_dry[start_sample..end_sample].to_vec();
    let after_slice = edited_dry[start_sample..end_sample].to_vec();

    Ok(RegionEditOutput {
        edited_dry,
        before_slice,
        after_slice,
        start_sample,
        end_sample,
        crossfade_samples: fade_samples,
        affected_notes,
        effective_action: eff_action,
        unresolved_note: None,
        outside_bit_identical,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reperform::{Note, Shape};
    use crate::studio_catalog::source_catalog;

    #[test]
    fn surgical_region_edit_modifies_only_bounds_and_keeps_outside_bit_identical() {
        let sources = source_catalog();
        let cello = sources.iter().find(|s| s.name == "Cello").unwrap();
        let rate = 48_000_u32;
        let hop_s = 0.005_f64;
        let frames = 101;
        // Note 0 (0.10..0.60 s): wobbly pitch around 220 Hz (+/- 25 cents)
        let wobbly_hz: Vec<f64> = (0..frames)
            .map(|i| {
                let wobble = if i % 2 == 0 { 1.015 } else { 0.985 };
                220.0 * wobble
            })
            .collect();
        // Note 1 (1.10..1.60 s): clean untouched note outside region
        let clean_hz = vec![330.0; frames];

        let perf = Performance {
            rate,
            samples: (rate as usize) * 2,
            hop_s,
            notes: vec![
                Note {
                    string_id: None,
                    re_picks: None,
                    start: 0.10,
                    end: 0.60,
                    hz: wobbly_hz,
                    level_steps: vec![0.35; frames],
                    _accent: None,
                    shape: Shape::default(),
                },
                Note {
                    string_id: None,
                    re_picks: None,
                    start: 1.10,
                    end: 1.60,
                    hz: clean_hz,
                    level_steps: vec![0.35; frames],
                    _accent: None,
                    shape: Shape::default(),
                },
            ],
            frame_evidence: vec![],
        };

        let base_voicing = VoicingControls::default();
        let cached_dry = perf
            .synthesize_dry_for_source_with_voicing(cello, &base_voicing)
            .unwrap();

        let edit = DryRegionEdit {
            id: "r1".into(),
            kind: "marquee".into(),
            start_s: 0.08,
            end_s: 0.64,
            min_hz: Some(150.0),
            max_hz: Some(1200.0),
            note: "straighten pitch wobble".into(),
            action: RegionFixAction::Straighten,
            strength: 0.75,
            crossfade_ms: 12.0,
        };

        let out = apply_dry_region_edit(&cached_dry, &perf, cello, &base_voicing, &edit).unwrap();
        assert!(
            out.outside_bit_identical,
            "samples outside region MUST be bit-identical"
        );
        assert_eq!(
            out.affected_notes, 1,
            "only Note 0 intersects the marked region"
        );
        assert_ne!(
            out.before_slice, out.after_slice,
            "region edit must update audio inside bounds"
        );
        // Verify Note 1 (1.10..1.60 s) is 100% untouched bit-for-bit
        let n1_a = (1.10 * rate as f64) as usize;
        let n1_b = (1.60 * rate as f64) as usize;
        assert_eq!(out.edited_dry[n1_a..n1_b], cached_dry[n1_a..n1_b]);
        assert!(out.unresolved_note.is_none());
    }

    #[test]
    fn unresolved_note_only_returns_explicit_noop_without_hidden_stabilize() {
        let sources = source_catalog();
        let cello = sources.iter().find(|s| s.name == "Cello").unwrap();
        let rate = 48_000_u32;
        let frames = 81;
        let perf = Performance {
            rate,
            samples: rate as usize,
            hop_s: 0.005,
            notes: vec![Note {
                string_id: None,
                re_picks: None,
                start: 0.10,
                end: 0.50,
                hz: (0..frames)
                    .map(|i| 220.0 * if i % 2 == 0 { 1.02 } else { 0.98 })
                    .collect(),
                level_steps: vec![0.35; frames],
                _accent: None,
                shape: Shape::default(),
            }],
            frame_evidence: vec![],
        };
        let base_voicing = VoicingControls::default();
        let cached_dry = perf
            .synthesize_dry_for_source_with_voicing(cello, &base_voicing)
            .unwrap();

        let edit = DryRegionEdit {
            id: "r-unrecognized".into(),
            kind: "range".into(),
            start_s: 0.08,
            end_s: 0.55,
            min_hz: None,
            max_hz: None,
            note: "make this feel more nostalgic".into(),
            action: RegionFixAction::NoteOnly,
            strength: 0.8,
            crossfade_ms: 12.0,
        };
        let out = apply_dry_region_edit(&cached_dry, &perf, cello, &base_voicing, &edit).unwrap();
        assert_eq!(out.effective_action, RegionFixAction::NoteOnly);
        assert!(out.unresolved_note.is_some());
        assert_eq!(out.affected_notes, 0);
        assert_eq!(
            out.edited_dry, cached_dry,
            "unrecognized NoteOnly must leave dry audio bit-identical"
        );
    }

    #[test]
    fn marquee_frequency_bounds_isolate_delta_to_selected_band() {
        let rate = 48_000.0_f64;
        let n = 4800;
        // Synthetic delta containing both an in-band tone (1200 Hz) and an out-of-band tone (220 Hz)
        let delta: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / rate;
                (2.0 * PI * 220.0 * t).sin() + (2.0 * PI * 1200.0 * t).sin()
            })
            .collect();
        let filtered = bandpass_delta_fft(&delta, rate, 900.0, 1600.0);

        let goertzel_amp = |sig: &[f64], target_hz: f64| -> f64 {
            let mut re = 0.0_f64;
            let mut im = 0.0_f64;
            for (i, &v) in sig.iter().enumerate() {
                let ph = 2.0 * PI * target_hz * i as f64 / rate;
                re += v * ph.cos();
                im += v * ph.sin();
            }
            (re * re + im * im).sqrt() * 2.0 / sig.len() as f64
        };

        let in_band = goertzel_amp(&filtered, 1200.0);
        let out_band = goertzel_amp(&filtered, 220.0);
        assert!(
            in_band > 0.9,
            "in-band delta energy should be preserved, got {in_band}"
        );
        assert!(
            out_band < 0.02,
            "out-of-band delta energy should be suppressed, got {out_band}"
        );
    }
}
