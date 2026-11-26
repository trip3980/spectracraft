//! Loose in-code reference library of spectral profiles and rough signal-character estimation.
//!
//! Profiles here are modest, editable approximations derived from broad acoustic characteristics.
//! They guide reshaping gently and produce low-stakes suggestions with confidence; they are NEVER
//! ground truth, NEVER hard constraints, and NEVER override extracted performance cues.

use crate::reperform::{EvidenceStatus, Performance};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LooseInstrumentProfile {
    pub id: String,
    pub label: String,
    pub body_pct: f64,
    pub presence_pct: f64,
    pub high_pct: f64,
    pub decay_hint: String,
    pub noise_texture: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LooseMicStyleProfile {
    pub id: String,
    pub label: String,
    pub description: String,
    pub body_bias_db: f64,
    pub presence_bias_db: f64,
    pub high_bias_db: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LooseCharacterProfile {
    pub id: String,
    pub label: String,
    pub description: String,
    pub warmth_db: f64,
    pub saturation_drive: f64,
    pub hf_rolloff_hz: Option<f64>,
    pub residual_scale: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LooseRoomCategoryProfile {
    pub id: String,
    pub label: String,
    pub description: String,
    pub expected_tail_ratio: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LooseProfileLibrary {
    pub policy: String,
    pub instrument_families: Vec<LooseInstrumentProfile>,
    pub mic_styles: Vec<LooseMicStyleProfile>,
    pub signal_characters: Vec<LooseCharacterProfile>,
    pub room_categories: Vec<LooseRoomCategoryProfile>,
}

/// Editable in-code loose spectral profile library (zero external dependencies).
pub fn loose_profile_library() -> LooseProfileLibrary {
    LooseProfileLibrary {
        policy: "Loose in-code reference profiles for gentle guided reshaping and rough suggestions. Never ground truth; never overrides extracted performance shape.".into(),
        instrument_families: vec![
            LooseInstrumentProfile {
                id: "plucked_string".into(),
                label: "Plucked / Fretted String".into(),
                body_pct: 34.0,
                presence_pct: 48.0,
                high_pct: 18.0,
                decay_hint: "Transient attack with exponential upper-partial decay".into(),
                noise_texture: "Brief pick/finger onset transient".into(),
            },
            LooseInstrumentProfile {
                id: "bowed_string".into(),
                label: "Bowed String".into(),
                body_pct: 40.0,
                presence_pct: 44.0,
                high_pct: 16.0,
                decay_hint: "Continuous bow sustain with resonant wood cavity".into(),
                noise_texture: "Band-limited bow-rosin friction".into(),
            },
            LooseInstrumentProfile {
                id: "human_voice".into(),
                label: "Human Voice".into(),
                body_pct: 28.0,
                presence_pct: 52.0,
                high_pct: 20.0,
                decay_hint: "Formant-shaped sustained phrasing".into(),
                noise_texture: "Glottal breath and consonant aspiration".into(),
            },
            LooseInstrumentProfile {
                id: "woodwind".into(),
                label: "Woodwind / Flute / Reed".into(),
                body_pct: 30.0,
                presence_pct: 50.0,
                high_pct: 20.0,
                decay_hint: "Breath-sustained air column".into(),
                noise_texture: "Air-jet or reed turbulence".into(),
            },
            LooseInstrumentProfile {
                id: "brass".into(),
                label: "Brass".into(),
                body_pct: 26.0,
                presence_pct: 52.0,
                high_pct: 22.0,
                decay_hint: "Dynamic harmonic bloom on louder phrases".into(),
                noise_texture: "Mild lip-buzz onset".into(),
            },
            LooseInstrumentProfile {
                id: "bass_low".into(),
                label: "Bass / Low Instrument".into(),
                body_pct: 64.0,
                presence_pct: 30.0,
                high_pct: 6.0,
                decay_hint: "Weighty fundamental with restrained high harmonics".into(),
                noise_texture: "Low mechanical fret/finger noise".into(),
            },
            LooseInstrumentProfile {
                id: "keyboard_struck".into(),
                label: "Keyboard / Hammer-Struck".into(),
                body_pct: 38.0,
                presence_pct: 44.0,
                high_pct: 18.0,
                decay_hint: "Crisp hammer attack with multi-stage sustain".into(),
                noise_texture: "Subtle hammer/key mechanical transient".into(),
            },
            LooseInstrumentProfile {
                id: "percussion_membrane".into(),
                label: "Percussion / Membrane".into(),
                body_pct: 42.0,
                presence_pct: 36.0,
                high_pct: 22.0,
                decay_hint: "Fast impact transient with rapid modal decay".into(),
                noise_texture: "Broadband stick/mallet strike".into(),
            },
        ],
        mic_styles: vec![
            LooseMicStyleProfile {
                id: "close_present".into(),
                label: "Close / Present".into(),
                description: "Proximity warmth with forward midrange articulation".into(),
                body_bias_db: 2.0,
                presence_bias_db: 2.2,
                high_bias_db: 0.8,
            },
            LooseMicStyleProfile {
                id: "neutral".into(),
                label: "Neutral / Balanced".into(),
                description: "Flat reference balance with minimal coloration".into(),
                body_bias_db: 0.0,
                presence_bias_db: 0.0,
                high_bias_db: 0.0,
            },
            LooseMicStyleProfile {
                id: "distant".into(),
                label: "Distant / Airy".into(),
                description: "Lighter proximity low end with softer top-end presence".into(),
                body_bias_db: -1.8,
                presence_bias_db: -0.6,
                high_bias_db: -1.6,
            },
            LooseMicStyleProfile {
                id: "direct".into(),
                label: "Direct / Pickup".into(),
                description: "Tight direct feed with controlled top-end rolloff".into(),
                body_bias_db: 0.8,
                presence_bias_db: 1.4,
                high_bias_db: -2.2,
            },
        ],
        signal_characters: vec![
            LooseCharacterProfile {
                id: "natural".into(),
                label: "Natural (Acoustic / Open)".into(),
                description: "Open acoustic balance with gentle organic dynamics and natural air".into(),
                warmth_db: 0.8,
                saturation_drive: 0.0,
                hf_rolloff_hz: None,
                residual_scale: 1.0,
            },
            LooseCharacterProfile {
                id: "analog".into(),
                label: "Analog (Warm / Mild Saturation)".into(),
                description: "Gentle soft-clip harmonic warmth, fuller low-mids, smooth top rolloff".into(),
                warmth_db: 2.4,
                saturation_drive: 0.42,
                hf_rolloff_hz: Some(11500.0),
                residual_scale: 0.9,
            },
            LooseCharacterProfile {
                id: "digital".into(),
                label: "Digital (Clean / Linear)".into(),
                description: "Ultra-linear harmonic precision, cleaner residual noise floor, extended highs".into(),
                warmth_db: -0.4,
                saturation_drive: 0.0,
                hf_rolloff_hz: None,
                residual_scale: 0.45,
            },
        ],
        room_categories: vec![
            LooseRoomCategoryProfile {
                id: "dry".into(),
                label: "Dry / Close".into(),
                description: "Minimal inter-note decay energy; direct-dominated".into(),
                expected_tail_ratio: 0.06,
            },
            LooseRoomCategoryProfile {
                id: "moderate".into(),
                label: "Moderate Room".into(),
                description: "Noticeable early reflections and gentle room floor".into(),
                expected_tail_ratio: 0.16,
            },
            LooseRoomCategoryProfile {
                id: "live".into(),
                label: "Live / Reverberant".into(),
                description: "Substantial ambient tail between notes".into(),
                expected_tail_ratio: 0.32,
            },
        ],
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileSuggestion {
    pub id: String,
    pub label: String,
    pub confidence: String,
    pub score: f64,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignalCharacterEstimate {
    /// Rough starting guess: "Natural", "Analog", "Digital", or "Unclear".
    pub guess: String,
    pub confidence: String,
    pub noise_floor_dbfs: f64,
    pub hf_rolloff_ratio: f64,
    pub crest_factor_db: f64,
    pub room_energy_estimate: f64,
    pub linearity_score: f64,
    pub wandering_frame_pct: f64,
    pub straighten_recommended: bool,
    pub evidence_summary: String,
    pub disclaimer: String,
    pub source_family_guess: ProfileSuggestion,
    pub mic_style_guess: ProfileSuggestion,
    pub room_category_guess: ProfileSuggestion,
}

/// Produce a rough, transparent signal-character estimate + loose profile suggestions
/// from measurable traits of the single-source signal and extracted performance cues.
pub fn estimate_signal_character(
    samples: &[f32],
    sample_rate: u32,
    perf: &Performance,
    band_relative_db: &[f64; 6],
    mean_tonal_fraction: f64,
) -> SignalCharacterEstimate {
    let lib = loose_profile_library();
    let frame_len = ((sample_rate as usize) / 20).max(256); // 50 ms windows
    let mut frame_rms: Vec<f64> = Vec::new();
    let mut global_peak = 0.0_f64;
    let mut global_sq = 0.0_f64;

    for chunk in samples.chunks(frame_len) {
        let mut sq = 0.0_f64;
        for &s in chunk {
            let v = f64::from(s);
            global_peak = global_peak.max(v.abs());
            sq += v * v;
        }
        global_sq += sq;
        let rms = (sq / chunk.len().max(1) as f64).sqrt();
        frame_rms.push(rms);
    }

    let global_rms = (global_sq / samples.len().max(1) as f64).sqrt();
    let crest_factor_db = if global_rms > 1e-9 && global_peak > 1e-9 {
        20.0 * (global_peak / global_rms).log10()
    } else {
        0.0
    };

    let mut sorted_rms = frame_rms.clone();
    sorted_rms.sort_by(f64::total_cmp);
    let p10 = sorted_rms
        .get(sorted_rms.len() / 10)
        .copied()
        .unwrap_or(0.0);
    let p85 = sorted_rms
        .get((sorted_rms.len() * 85) / 100)
        .copied()
        .unwrap_or(global_rms)
        .max(1e-9);
    let noise_floor_dbfs = if p10 > 1e-9 {
        20.0 * p10.log10()
    } else {
        -120.0
    };

    // Estimate inter-note / room tail floor using frames outside supported note bodies
    let mut quiet_gap_rms = Vec::new();
    for (idx, &rms) in frame_rms.iter().enumerate() {
        let t = idx as f64 * (frame_len as f64 / f64::from(sample_rate.max(1)));
        let inside_note = perf.notes.iter().any(|n| t >= n.start && t <= n.end);
        if !inside_note && rms > 1e-6 {
            quiet_gap_rms.push(rms);
        }
    }
    let room_energy_estimate = if !quiet_gap_rms.is_empty() {
        let mean_gap = quiet_gap_rms.iter().sum::<f64>() / quiet_gap_rms.len() as f64;
        (mean_gap / p85).clamp(0.0, 1.0)
    } else {
        (p10 / p85).clamp(0.0, 1.0)
    };

    // High-frequency rolloff ratio: 4 kHz band vs 1 kHz + 2 kHz average (linear power ratio)
    let mid_db = 0.5 * (band_relative_db[3] + band_relative_db[4]);
    let hf_diff_db = band_relative_db[5] - mid_db;
    let hf_rolloff_ratio = 10.0_f64.powf((hf_diff_db / 20.0).clamp(-3.0, 1.5));

    // Measure pitch wander (frame-to-frame jitter between 8 and 45 cents inside notes,
    // excluding intentional smooth bends where direction stays consistent)
    let mut total_note_frames = 0_usize;
    let mut wandering_frames = 0_usize;
    for note in &perf.notes {
        if note.hz.len() < 4 {
            continue;
        }
        for w in note.hz.windows(3) {
            total_note_frames += 1;
            let d1 = 1200.0 * (w[1] / w[0].max(1e-6)).log2();
            let d2 = 1200.0 * (w[2] / w[1].max(1e-6)).log2();
            // Alternating-sign jitter > 7 cents indicates wobble/instability rather than smooth bend
            if d1 * d2 < 0.0 && d1.abs().max(d2.abs()) > 7.0 && d1.abs().max(d2.abs()) < 120.0 {
                wandering_frames += 1;
            }
        }
    }
    let wandering_frame_pct = if total_note_frames > 0 {
        100.0 * wandering_frames as f64 / total_note_frames as f64
    } else {
        0.0
    };
    let uncertain_ratio = if !perf.frame_evidence.is_empty() {
        let unc = perf
            .frame_evidence
            .iter()
            .filter(|f| f.status == EvidenceStatus::Uncertain)
            .count();
        unc as f64 / perf.frame_evidence.len() as f64
    } else {
        0.0
    };

    // Linearity heuristic: clean tonal fraction + low noise floor + open dynamics
    let linearity_score = (0.55 * mean_tonal_fraction.clamp(0.0, 1.0)
        + 0.35 * ((-noise_floor_dbfs - 35.0) / 55.0).clamp(0.0, 1.0)
        + 0.10 * ((crest_factor_db - 3.0) / 14.0).clamp(0.0, 1.0))
    .clamp(0.0, 1.0);

    // Rough starting character label (always low-stakes & overridable)
    let (guess, confidence, evidence_summary) = if perf.notes.is_empty() || global_rms < 1e-5 {
        (
            "Unclear".to_string(),
            "Unclear".to_string(),
            "Signal too quiet or sparse to estimate character reliably.".to_string(),
        )
    } else if uncertain_ratio > 0.45 {
        (
            "Unclear".to_string(),
            "Low".to_string(),
            format!(
                "High uncertain-frame share ({:.0}%) — character kept marked Unclear.",
                uncertain_ratio * 100.0
            ),
        )
    } else if noise_floor_dbfs < -78.0 && linearity_score > 0.72 && room_energy_estimate < 0.08 {
        (
            "Digital".to_string(),
            "Moderate".to_string(),
            format!(
                "Very low noise floor ({noise_floor_dbfs:.1} dBFS), high linearity ({:.0}%), minimal ambient tail.",
                linearity_score * 100.0
            ),
        )
    } else if crest_factor_db < 11.5 && hf_rolloff_ratio < 0.55 && noise_floor_dbfs > -74.0 {
        (
            "Analog".to_string(),
            "Moderate".to_string(),
            format!(
                "Softer crest ({crest_factor_db:.1} dB), gentle top-end rolloff, and audible floor ({noise_floor_dbfs:.1} dBFS)."
            ),
        )
    } else {
        (
            "Natural".to_string(),
            if room_energy_estimate > 0.05 || crest_factor_db >= 11.5 {
                "Moderate".to_string()
            } else {
                "Low".to_string()
            },
            format!(
                "Open dynamics (crest {crest_factor_db:.1} dB), tonal clarity {:.0}%, ambient floor ratio {:.2}.",
                mean_tonal_fraction * 100.0,
                room_energy_estimate
            ),
        )
    };

    // Convert 6-band relative dB to 3 broad percentages (body: 125+250, presence: 500+1k+2k, high: 4k)
    let lin: [f64; 6] = std::array::from_fn(|i| 10.0_f64.powf(band_relative_db[i] / 10.0));
    let body = lin[0] + lin[1];
    let presence = lin[2] + lin[3] + lin[4];
    let high = lin[5];
    let sum = (body + presence + high).max(1e-12);
    let obs_body = 100.0 * body / sum;
    let obs_pres = 100.0 * presence / sum;
    let obs_high = 100.0 * high / sum;

    // Best loose instrument family match
    let mut best_inst = (&lib.instrument_families[0], f64::INFINITY);
    for prof in &lib.instrument_families {
        let dist = ((obs_body - prof.body_pct).powi(2)
            + (obs_pres - prof.presence_pct).powi(2)
            + (obs_high - prof.high_pct).powi(2))
        .sqrt();
        if dist < best_inst.1 {
            best_inst = (prof, dist);
        }
    }
    let inst_score = (1.0 - best_inst.1 / 85.0).clamp(0.0, 1.0);
    let source_family_guess = if perf.notes.is_empty() || inst_score < 0.45 {
        ProfileSuggestion {
            id: "unclear".into(),
            label: "Unclear".into(),
            confidence: "Unclear".into(),
            score: inst_score,
            note: "Broad spectral balance does not strongly favor one family.".into(),
        }
    } else {
        ProfileSuggestion {
            id: best_inst.0.id.clone(),
            label: best_inst.0.label.clone(),
            confidence: if inst_score > 0.72 { "Moderate" } else { "Low" }.into(),
            score: inst_score,
            note: format!(
                "Observed body/pres/high {:.0}%/{:.0}%/{:.0}% resembles {} (suggestion only).",
                obs_body, obs_pres, obs_high, best_inst.0.label
            ),
        }
    };

    // Loose room category suggestion
    let (room_id, room_label, room_conf, room_note) = if perf.notes.is_empty() {
        (
            "unclear",
            "Unclear",
            "Unclear",
            "Insufficient note-to-gap contrast to estimate room energy.",
        )
    } else if room_energy_estimate < 0.09 {
        (
            "dry",
            "Dry / Close",
            "Moderate",
            "Low energy between notes suggests a dry or close-captured track.",
        )
    } else if room_energy_estimate < 0.22 {
        (
            "moderate",
            "Moderate Room",
            "Low",
            "Modest ambient floor between notes suggests moderate room energy.",
        )
    } else {
        (
            "live",
            "Live / Reverberant",
            "Moderate",
            "Elevated inter-note tail suggests noticeable room/reverb energy.",
        )
    };
    let room_category_guess = ProfileSuggestion {
        id: room_id.into(),
        label: room_label.into(),
        confidence: room_conf.into(),
        score: (1.0 - (room_energy_estimate - 0.15).abs()).clamp(0.3, 0.9),
        note: room_note.into(),
    };

    // Loose mic style suggestion
    let (mic_id, mic_label, mic_conf, mic_note) = if perf.notes.is_empty() {
        (
            "unclear",
            "Unclear",
            "Unclear",
            "No clear spectral cue for pickup distance.",
        )
    } else if room_energy_estimate > 0.22 {
        (
            "distant",
            "Distant / Airy",
            "Low",
            "Higher ambient tail resembles a more distant mic placement.",
        )
    } else if noise_floor_dbfs < -80.0 && hf_rolloff_ratio < 0.45 {
        (
            "direct",
            "Direct / Pickup",
            "Low",
            "Clean floor and gentle top-end rolloff resemble a direct/pickup feed.",
        )
    } else if obs_body > 38.0 && room_energy_estimate < 0.10 {
        (
            "close_present",
            "Close / Present",
            "Low",
            "Strong low-mid body with low room tail resembles close miking.",
        )
    } else {
        (
            "neutral",
            "Neutral / Balanced",
            "Low",
            "Balanced mid/high profile resembles a neutral capture.",
        )
    };
    let mic_style_guess = ProfileSuggestion {
        id: mic_id.into(),
        label: mic_label.into(),
        confidence: mic_conf.into(),
        score: 0.55,
        note: mic_note.into(),
    };

    let straighten_recommended = wandering_frame_pct >= 12.0;

    SignalCharacterEstimate {
        guess,
        confidence,
        noise_floor_dbfs,
        hf_rolloff_ratio,
        crest_factor_db,
        room_energy_estimate,
        linearity_score,
        wandering_frame_pct,
        straighten_recommended,
        evidence_summary,
        disclaimer: "Rough starting guess from measurable traits — accept, ignore, or override. Never claims exact gear chain, cable, or conversion history.".into(),
        source_family_guess,
        mic_style_guess,
        room_category_guess,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reperform::{Note, Shape};

    #[test]
    fn loose_library_is_editable_and_estimator_handles_clean_vs_silent_honestly() {
        let lib = loose_profile_library();
        assert!(lib.instrument_families.len() >= 8);
        assert_eq!(lib.signal_characters.len(), 3);
        assert_eq!(lib.room_categories.len(), 3);

        // Silent / empty performance -> Unclear
        let empty = Performance {
            rate: 48_000,
            samples: 48_000,
            hop_s: 0.005,
            notes: vec![],
            frame_evidence: vec![],
        };
        let silent = vec![0.0_f32; 48_000];
        let est_empty = estimate_signal_character(&silent, 48_000, &empty, &[-60.0; 6], 0.0);
        assert_eq!(est_empty.guess, "Unclear");
        assert_eq!(est_empty.source_family_guess.label, "Unclear");

        // Clean high-linearity tone with near-zero floor -> Digital
        let rate = 48_000_u32;
        let clean: Vec<f32> = (0..rate as usize)
            .map(|i| {
                let t = i as f64 / rate as f64;
                if (0.1..0.8).contains(&t) {
                    (0.45 * (std::f64::consts::TAU * 440.0 * t).sin()) as f32
                } else {
                    0.0
                }
            })
            .collect();
        let perf = Performance {
            rate,
            samples: rate as usize,
            hop_s: 0.005,
            notes: vec![Note {
                string_id: None,
                re_picks: None,
                start: 0.1,
                end: 0.8,
                hz: vec![440.0; 141],
                level_steps: vec![0.4; 141],
                _accent: None,
                shape: Shape::default(),
            }],
            frame_evidence: vec![],
        };
        let est_clean = estimate_signal_character(
            &clean,
            rate,
            &perf,
            &[-20.0, -10.0, 0.0, -4.0, -8.0, -14.0],
            0.94,
        );
        assert_eq!(est_clean.guess, "Digital");
        assert!(!est_clean.straighten_recommended);
    }
}
