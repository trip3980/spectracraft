//! Dry rendering from performance cues, separate from legacy FFT-partial resynthesis.
use crate::{dsp, valid_rate, Result};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    f64::consts::{PI, TAU},
    path::Path,
    sync::OnceLock,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Supported,
    Uncertain,
    OutOfTarget,
    Silence,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PitchHypothesis {
    pub hz: f64,
    pub score: f64,
    pub explained_fraction: f64,
    pub harmonic_count: usize,
    pub odd_harmonic_ratio: f64,
    pub autocorr: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameEvidence {
    pub time_s: f64,
    pub level_rms: f64,
    pub status: EvidenceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_hz: Option<f64>,
    pub tonal_fraction: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hypotheses: Vec<PitchHypothesis>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PerformanceSummary {
    pub rate: u32,
    pub samples: usize,
    pub duration_s: f64,
    pub hop_s: f64,
    pub note_count: usize,
    pub total_repicks: usize,
    pub overlapping_ring_notes: usize,
    pub supported_frames: usize,
    pub uncertain_frames: usize,
    pub out_of_target_frames: usize,
    pub silence_frames: usize,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Performance {
    pub(crate) rate: u32,
    pub(crate) samples: usize,
    pub(crate) hop_s: f64,
    pub(crate) notes: Vec<Note>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) frame_evidence: Vec<FrameEvidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string_id: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub re_picks: Option<Vec<Strike>>,
    pub start: f64,
    pub end: f64,
    pub hz: Vec<f64>,
    pub level_steps: Vec<f64>,
    #[serde(default, rename = "accent")]
    pub _accent: Option<f64>,
    #[serde(default)]
    pub shape: Shape,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Strike {
    pub offset_s: f64,
    pub strength: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Shape {
    pub mode_weights: BTreeMap<usize, f64>,
    pub decay_per_second: BTreeMap<usize, f64>,
}

fn interp(x: &[f64], t: f64) -> f64 {
    let i = t as usize;
    if i >= x.len() - 1 {
        return x[x.len() - 1];
    }
    let a = t - i as f64;
    x[i] + a * (x[i + 1] - x[i])
}

fn smooth_kernel() -> &'static [f64; 17] {
    static KERNEL: OnceLock<[f64; 17]> = OnceLock::new();
    KERNEL.get_or_init(|| {
        let mut weights = [0.0_f64; 17];
        let mut sum = 0.0_f64;
        for (j, slot) in weights.iter_mut().enumerate() {
            let k = j as i32 - 8;
            let w = (-f64::from(k * k) / 8.0).exp();
            *slot = w;
            sum += w;
        }
        let inv_sum = 1.0 / sum;
        for w in &mut weights {
            *w *= inv_sum;
        }
        weights
    })
}

fn smooth(x: &[f64]) -> Vec<f64> {
    let kernel = smooth_kernel();
    let len = x.len() as isize;
    let period = 2 * len;
    (0..x.len())
        .map(|i| {
            let base = i as isize - 8;
            let mut acc = 0.0_f64;
            for (j, &w) in kernel.iter().enumerate() {
                let k = (base + j as isize).rem_euclid(period);
                let idx = if k < len { k } else { period - 1 - k } as usize;
                acc += w * x[idx];
            }
            acc
        })
        .collect()
}

fn median_of_slice(values: &[f64]) -> f64 {
    let n = values.len();
    debug_assert!(n > 0);
    let mut scratch = values.to_vec();
    let hi_idx = n / 2;
    let (_, &mut hi, _) = scratch.select_nth_unstable_by(hi_idx, f64::total_cmp);
    if n.is_multiple_of(2) {
        let lo = scratch[..hi_idx]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        (lo + hi) * 0.5
    } else {
        hi
    }
}

fn build_string_mode_params(
    note: &Note,
    median: f64,
    rate: f64,
) -> crate::string_state::StringModeParams {
    let (mut weights, mut decays, pickups) =
        crate::string_state::default_string_mode_params(median);
    for k in 1..=crate::string_state::MODE_COUNT {
        let idx = k - 1;
        if let Some(&w_mult) = note.shape.mode_weights.get(&k) {
            weights[idx] *= w_mult;
        }
        if let Some(&d_override) = note.shape.decay_per_second.get(&k) {
            decays[idx] = d_override;
        }
    }
    crate::string_state::StringModeParams::new(weights, decays, pickups, rate)
}

fn validate_dry_note(
    note: &Note,
    samples: usize,
    rate: f64,
    hop_s: f64,
    err_msg: &'static str,
) -> Result<()> {
    let duration_s = samples as f64 / rate;
    if !(0. ..=duration_s).contains(&note.start)
        || !note.end.is_finite()
        || note.end <= note.start
        || note.end > duration_s + hop_s
        || note.hz.is_empty()
        || note.hz.len() > 90_000
        || note.level_steps.len() != note.hz.len()
        || note.hz.iter().any(|v| !(20. ..=rate * 0.49).contains(v))
        || note.level_steps.iter().any(|v| !(0. ..=4.).contains(v))
        || note
            .shape
            .mode_weights
            .iter()
            .any(|(k, v)| !(1..20).contains(k) || !(0. ..=4.).contains(v))
        || note
            .shape
            .decay_per_second
            .iter()
            .any(|(k, v)| !(1..20).contains(k) || !(0. ..=80.).contains(v))
    {
        return Err(err_msg.into());
    }
    Ok(())
}

/// Collect initial strike strength (`offset == 0`) and sorted mid-note re-picks
/// indexed relative to `base_idx`.
fn collect_note_repicks(
    note: &Note,
    count: usize,
    rate: f64,
    base_idx: usize,
    strict_validate: bool,
) -> Result<(f64, Vec<(usize, f64)>)> {
    let mut initial_strength = 1.0_f64;
    let mut mid_repicks: Vec<(usize, f64)> = Vec::new();
    if let Some(strikes) = &note.re_picks {
        if strict_validate && strikes.len() > 1024 {
            return Err("Too many re-picks in one event".into());
        }
        mid_repicks.reserve(strikes.len());
        let max_offset_s = (count - 1) as f64 / rate;
        for strike in strikes {
            if strict_validate
                && (!(0. ..=max_offset_s).contains(&strike.offset_s)
                    || !(0. ..=1.5).contains(&strike.strength))
            {
                return Err("Invalid re-pick".into());
            }
            let offset = ((strike.offset_s * rate).round() as usize).min(count - 1);
            if offset == 0 {
                initial_strength += strike.strength;
            } else {
                mid_repicks.push((base_idx + offset, strike.strength));
            }
        }
        mid_repicks.sort_by_key(|&(idx, _)| idx);
    }
    Ok((initial_strength, mid_repicks))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum StringKey {
    Physical(usize),
    NoteDefault(usize),
}

struct PreparedDryNote<'a> {
    start: usize,
    end: usize,
    hz: &'a [f64],
    levels: Vec<f64>,
    params: crate::string_state::StringModeParams,
    initial_strength: f64,
    mid_repicks: Vec<(usize, f64)>,
}

impl Performance {
    pub fn rate(&self) -> u32 {
        self.rate
    }
    pub fn samples(&self) -> usize {
        self.samples
    }
    pub fn hop_s(&self) -> f64 {
        self.hop_s
    }
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }
    pub fn frame_evidence(&self) -> &[FrameEvidence] {
        &self.frame_evidence
    }
    pub fn summary(&self) -> PerformanceSummary {
        let mut supported = 0;
        let mut uncertain = 0;
        let mut out_of_target = 0;
        let mut silence = 0;
        for f in &self.frame_evidence {
            match f.status {
                EvidenceStatus::Supported => supported += 1,
                EvidenceStatus::Uncertain => uncertain += 1,
                EvidenceStatus::OutOfTarget => out_of_target += 1,
                EvidenceStatus::Silence => silence += 1,
            }
        }
        let total_repicks = self
            .notes
            .iter()
            .filter_map(|n| n.re_picks.as_ref())
            .map(Vec::len)
            .sum();
        let overlapping_ring_notes = self
            .notes
            .windows(2)
            .filter(|pair| pair[0].end > pair[1].start + self.hop_s * 1.5)
            .count();
        PerformanceSummary {
            rate: self.rate,
            samples: self.samples,
            duration_s: self.samples as f64 / f64::from(self.rate.max(1)),
            hop_s: self.hop_s,
            note_count: self.notes.len(),
            total_repicks,
            overlapping_ring_notes,
            supported_frames: supported,
            uncertain_frames: uncertain,
            out_of_target_frames: out_of_target,
            silence_frames: silence,
        }
    }

    fn synthesize_dry_f64(&self) -> Result<Vec<f64>> {
        let rate = f64::from(self.rate);
        if !valid_rate(self.rate)
            || self.samples == 0
            || self.samples > self.rate as usize * 90
            || !(0.001..=0.100).contains(&self.hop_s)
            || self.notes.len() > 18_000
        {
            return Err("Invalid performance duration, rate, or cue spacing".into());
        }
        let mut y = vec![0.0_f64; self.samples];
        let mut total = 0usize;
        let mut by_string: BTreeMap<StringKey, Vec<PreparedDryNote<'_>>> = BTreeMap::new();

        for (note_idx, note) in self.notes.iter().enumerate() {
            validate_dry_note(
                note,
                self.samples,
                rate,
                self.hop_s,
                "Invalid dry guitar event",
            )?;
            let start = (note.start * rate).round_ties_even() as usize;
            let end = ((note.end * rate).round_ties_even() as usize).min(self.samples);
            let count = end.saturating_sub(start);
            total += count;
            if total > 4 * self.samples {
                return Err("Too much overlapping synthesis".into());
            }
            if count < 10 {
                continue;
            }
            let median = median_of_slice(&note.hz);
            let levels = smooth(&note.level_steps);
            let params = build_string_mode_params(note, median, rate);
            let (initial_strength, mid_repicks) =
                collect_note_repicks(note, count, rate, start, true)?;

            let key = note
                .string_id
                .map(StringKey::Physical)
                .unwrap_or(StringKey::NoteDefault(note_idx));
            by_string.entry(key).or_default().push(PreparedDryNote {
                start,
                end,
                hz: &note.hz,
                levels,
                params,
                initial_strength,
                mid_repicks,
            });
        }

        let inv_rate = 1.0 / rate;
        let inv_rate_hop = inv_rate / self.hop_s;
        let inv_attack = inv_rate / 0.008;
        let inv_release = inv_rate / 0.018;

        // One persistent StringState owned per physical string; independent strings sum cleanly.
        for (_, mut notes_on_string) in by_string {
            notes_on_string.sort_by_key(|n| n.start);
            let mut state = crate::string_state::StringState::new();
            let span_end = notes_on_string.iter().map(|n| n.end).max().unwrap_or(0);
            let mut sample_idx = notes_on_string[0].start;
            let mut next_note_start = 0usize;
            let mut active_idx: Option<usize> = None;
            let mut rp_cursors = vec![0usize; notes_on_string.len()];
            let mut last_f = notes_on_string[0].hz[0];
            let mut last_decays = notes_on_string[0].params.decays;

            while sample_idx < span_end {
                // Re-pick path: any note starting at sample_idx excites the string additively.
                while next_note_start < notes_on_string.len()
                    && notes_on_string[next_note_start].start == sample_idx
                {
                    let n = &notes_on_string[next_note_start];
                    state.repick(n.initial_strength, &n.params.weights);
                    active_idx = Some(next_note_start);
                    next_note_start += 1;
                }
                if let Some(a) = active_idx {
                    if sample_idx >= notes_on_string[a].end {
                        active_idx = notes_on_string[..next_note_start]
                            .iter()
                            .rposition(|n| sample_idx >= n.start && sample_idx < n.end);
                    }
                }

                if let Some(a_idx) = active_idx {
                    let note = &notes_on_string[a_idx];
                    let i = sample_idx - note.start;
                    let rem = note.end - 1 - sample_idx;
                    let step_pos = i as f64 * inv_rate_hop;
                    let f = interp(note.hz, step_pos);
                    last_f = f;
                    last_decays = note.params.decays;
                    let envelope =
                        (i as f64 * inv_attack).min(1.0) * (rem as f64 * inv_release).min(1.0);
                    let modal_sum = state.tick_cached(
                        f,
                        rate,
                        &note.params.sample_losses,
                        &note.params.pickups,
                    );
                    y[sample_idx] += 1.53 * interp(&note.levels, step_pos) * envelope * modal_sum;
                    let cursor = &mut rp_cursors[a_idx];
                    while *cursor < note.mid_repicks.len()
                        && note.mid_repicks[*cursor].0 == sample_idx
                    {
                        state.repick(note.mid_repicks[*cursor].1, &note.params.weights);
                        *cursor += 1;
                    }
                    sample_idx += 1;
                } else if next_note_start < notes_on_string.len() {
                    let next_start = notes_on_string[next_note_start].start;
                    state.decay_only(next_start - sample_idx, last_f, rate, &last_decays);
                    sample_idx = next_start;
                } else {
                    break;
                }
            }
        }
        Ok(y)
    }

    pub fn synthesize_dry(&self) -> Result<Vec<f32>> {
        let y = self.synthesize_dry_f64()?;
        Ok(y.into_iter().map(|x| x as f32).collect())
    }

    /// Optional Stage 1 dry render with parallel two-biquad body resonance
    /// (default: 70% dry + 20% ~100 Hz air cavity + 10% ~200 Hz top plate).
    pub fn synthesize_dry_with_body_resonance(
        &self,
        body: &crate::string_state::BodyResonanceConfig,
    ) -> Result<Vec<f32>> {
        let mut buf = self.synthesize_dry_f64()?;
        let mut resonator = crate::string_state::BodyResonator::new(f64::from(self.rate), *body);
        resonator.process_buffer(&mut buf);
        Ok(buf.into_iter().map(|x| x as f32).collect())
    }

    pub fn write_dry_wav(&self, path: &Path) -> Result<()> {
        let y = self.synthesize_dry()?;
        write_dry_samples(self.rate, &y, path)
    }

    /// Synthesize a fresh, source-informed dry take purely from extracted performance cues
    /// (zero original audio blending) using default voicing controls.
    pub fn synthesize_dry_for_source(
        &self,
        source: &crate::studio_catalog::SourceEntry,
    ) -> Result<Vec<f32>> {
        self.synthesize_dry_for_source_with_voicing(source, &VoicingControls::default())
    }

    /// Stage 1 DDSP-Style Harmonic-Plus-Noise Dry Synthesizer with practical character,
    /// signal-path, source-variant, loose-profile guidance, and mild bend-preserving
    /// straightening controls. Driven 100% by extracted cues (zero original audio blended).
    pub fn synthesize_dry_for_source_with_voicing(
        &self,
        source: &crate::studio_catalog::SourceEntry,
        voicing: &VoicingControls,
    ) -> Result<Vec<f32>> {
        use crate::studio_catalog::SourceRegenKind;
        let kind = source.regen_model.family_kind;
        let default_voicing = voicing.is_neutral_default();
        if default_voicing
            && matches!(
                kind,
                SourceRegenKind::Neutral | SourceRegenKind::PluckedElectric
            )
        {
            return self.synthesize_dry();
        }
        let rate = f64::from(self.rate);
        if !valid_rate(self.rate)
            || self.samples == 0
            || self.samples > self.rate as usize * 90
            || !(0.001..=0.100).contains(&self.hop_s)
            || self.notes.len() > 18_000
        {
            return Err("Invalid performance duration, rate, or cue spacing".into());
        }

        let phys = kind.physics_constraint();
        let attack_s = (phys.attack_ms / 1000.0).max(0.002);
        let release_s = (phys.release_ms / 1000.0).max(0.008);
        let char_noise_scale = match voicing.character_target {
            CharacterTarget::Digital => 0.45,
            CharacterTarget::Analog => 0.90,
            CharacterTarget::Natural => 1.08,
            CharacterTarget::Keep => 1.0,
        };
        let nr_scale = voicing
            .noise_reduction_override
            .map(|r| (1.0 - r.clamp(0.0, 0.9)).clamp(0.1, 1.0))
            .unwrap_or(1.0);
        let breath_amt = phys.breath_texture_amount * char_noise_scale * nr_scale;
        let dyn_bright = phys.dynamic_brightness_amount;
        let variant_tilt = match voicing.source_variant {
            SourceCharacterVariant::Rich => 1.18,
            SourceCharacterVariant::Basic => 0.78,
            SourceCharacterVariant::Bright => 1.28,
            SourceCharacterVariant::Warm => 0.74,
            SourceCharacterVariant::Standard => 1.0,
        };
        let smooth_hf_scale = voicing
            .smooth_hf_override
            .map(|s| (1.0 - 0.45 * s.clamp(0.0, 1.0)).clamp(0.4, 1.0))
            .unwrap_or(1.0);
        let guide_amt = voicing.profile_guidance_strength.clamp(0.0, 1.0);

        let mut y = vec![0.0_f64; self.samples];
        let mut total = 0;
        let mut noise_state: u64 = 0x517c_c1b7_2722_0a95;
        let mut lp_noise = 0.0_f64;

        let inv_rate = 1.0 / rate;
        let inv_rate_hop = inv_rate / self.hop_s;
        let inv_attack = inv_rate / attack_s;
        let inv_release = inv_rate / release_s;
        let nyquist_margin = rate * 0.48;
        let inharm_b = crate::acoustics_physics::inharmonicity_b_for_source(kind);

        for note in &self.notes {
            validate_dry_note(
                note,
                self.samples,
                rate,
                self.hop_s,
                "Invalid dry performance event",
            )?;
            let start = (note.start * rate).round_ties_even() as usize;
            let end = ((note.end * rate).round_ties_even() as usize).min(self.samples);
            let count = end - start;
            total += count;
            if total > 4 * self.samples {
                return Err("Too much overlapping synthesis".into());
            }
            if count < 10 {
                continue;
            }
            // Optional mild bend-preserving stabilization (never quantizes or transposes pitch).
            let effective_hz: Cow<'_, [f64]> = if voicing.straighten_mode != StraightenMode::Off {
                Cow::Owned(stabilize_note_pitch_mild(
                    &note.hz,
                    voicing.straighten_mode,
                    voicing.straighten_preserve_bends,
                ))
            } else {
                Cow::Borrowed(&note.hz)
            };
            let median = median_of_slice(&effective_hz);
            let levels = smooth(&note.level_steps);
            let modes = build_source_note_modes(note, median, kind);
            let use_repick_modes = note.re_picks.is_some();
            let (initial_strength, mid_repicks) = if use_repick_modes {
                collect_note_repicks(note, count, rate, 0, true)?
            } else {
                (1.0, Vec::new())
            };
            let precomputed_modes: Vec<(f64, f64, f64, f64)> = if !use_repick_modes {
                modes
                    .iter()
                    .map(|&(h, w, d)| {
                        (
                            h,
                            w,
                            d,
                            crate::acoustics_physics::stiff_string_ratio(h, inharm_b),
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let mut modes_state: Vec<_> = if use_repick_modes {
                modes
                    .iter()
                    .map(|&(h, w, d)| {
                        crate::string_state::Mode::new_with_inharmonicity(h, w, d, inharm_b, rate)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let mut rp_cursor = 0usize;
            let onset_lvl = levels.first().copied().unwrap_or(0.1).max(1e-4);
            let mut cycles = 0.0_f64;
            for (i, sample) in y[start..end].iter_mut().enumerate() {
                let t = i as f64 * inv_rate;
                let step_pos = i as f64 * inv_rate_hop;
                let f = interp(&effective_hz, step_pos);
                cycles += f;
                let phase = TAU * cycles * inv_rate;
                let lvl = interp(&levels, step_pos);
                let envelope = (i as f64 * inv_attack).min(1.0)
                    * ((count - 1 - i) as f64 * inv_release).min(1.0);
                let bright_mod = 1.0 + dyn_bright * (lvl - 0.15).clamp(-0.2, 0.8);
                let cue_growth = (lvl / onset_lvl).clamp(0.0, 4.0);
                let sustain_relief = ((cue_growth - 0.90) / 0.80).clamp(0.0, 1.0);
                let modal_sum = if !use_repick_modes {
                    let hf_tilt = bright_mod * variant_tilt * smooth_hf_scale;
                    let mut sum = 0.0_f64;
                    for &(h, w, d, ratio) in &precomputed_modes {
                        if ratio * f >= nyquist_margin {
                            continue;
                        }
                        let tilt = if h > 2.5 { hf_tilt } else { 1.0 };
                        let eff_d = if sustain_relief > 0.0 {
                            if h <= 1.5 {
                                d * (1.0 - 0.92 * sustain_relief)
                            } else {
                                d * (1.0 - 0.65 * sustain_relief)
                            }
                        } else {
                            d
                        };
                        sum += w * tilt * (ratio * phase).sin() * (-eff_d * t).exp();
                    }
                    sum
                } else {
                    let mut kick = if i == 0 { initial_strength } else { 0.0 };
                    while rp_cursor < mid_repicks.len() && mid_repicks[rp_cursor].0 == i {
                        kick += mid_repicks[rp_cursor].1;
                        rp_cursor += 1;
                    }
                    modes_state
                        .iter_mut()
                        .map(|m| m.advance(f, rate, kick))
                        .sum::<f64>()
                };
                let ref_texture_scale = if !self.frame_evidence.is_empty() {
                    let global_t = note.start + t;
                    let ev_idx = ((global_t / self.hop_s).round() as usize)
                        .min(self.frame_evidence.len().saturating_sub(1));
                    let ev = &self.frame_evidence[ev_idx];
                    if ev.status == EvidenceStatus::Supported {
                        let ref_noise = (1.0 - ev.tonal_fraction.clamp(0.0, 1.0)).clamp(0.0, 0.45);
                        (0.55 + 1.8 * ref_noise).clamp(0.55, 1.35)
                    } else {
                        0.55
                    }
                } else {
                    1.0
                };
                let air = if breath_amt > 0.0 || t <= 0.012 {
                    let white = next_centered_white_noise(&mut noise_state);
                    lp_noise = lp_noise * 0.65 + white * 0.35;
                    let onset_boost = 1.0 + 1.5 * (-t / 0.045).exp();
                    let breath = if breath_amt > 0.0 {
                        breath_amt * ref_texture_scale * onset_boost * (white - lp_noise * 0.5)
                    } else {
                        0.0
                    };
                    let transient = crate::acoustics_physics::synthesize_onset_transient(
                        kind, t, white, onset_lvl,
                    );
                    breath + transient
                } else {
                    0.0
                };
                *sample += 1.53 * lvl * envelope * (modal_sum + air);
            }
        }

        // Apply broad, smooth source-family body / formant / cabinet contour filters
        if guide_amt > 0.05 {
            apply_source_body_contour(&mut y, rate, kind);
        }

        // Apply optional character target & signal-path style conditioning (Stage 1 only)
        apply_character_and_path_conditioning(&mut y, rate, voicing);

        // Level-match RMS against baseline dry synthesis (with strict peak ceiling < 0.94)
        let baseline = self.synthesize_dry()?;
        let base_rms = (baseline
            .iter()
            .map(|&v| f64::from(v) * f64::from(v))
            .sum::<f64>()
            / baseline.len().max(1) as f64)
            .sqrt();
        let cur_rms = (y.iter().map(|&v| v * v).sum::<f64>() / y.len().max(1) as f64).sqrt();
        let cur_peak = y.iter().fold(0.0_f64, |m, &v| m.max(v.abs()));
        let mut gain = if cur_rms > 1e-9 && base_rms > 1e-9 {
            (base_rms / cur_rms).clamp(0.25, 3.0)
        } else {
            1.0
        };
        if cur_peak * gain > 0.94 {
            gain = 0.94 / cur_peak.max(1e-9);
        }
        Ok(y.into_iter().map(|x| (x * gain) as f32).collect())
    }

    pub fn write_dry_wav_for_source(
        &self,
        source: &crate::studio_catalog::SourceEntry,
        path: &Path,
    ) -> Result<()> {
        let y = self.synthesize_dry_for_source(source)?;
        write_dry_samples(self.rate, &y, path)
    }

    /// Pre-capture expressive performance transform: alters selected cues
    /// (dynamic phrasing contrast, subtle delayed sustain vibrato, and pick/re-pick
    /// articulation) BEFORE dry string synthesis and capture.
    pub fn apply_expressive_gestures(&self) -> Result<Self> {
        let mut out = self.clone();
        if out.notes.is_empty() {
            return Ok(out);
        }
        let peaks: Vec<f64> = out
            .notes
            .iter()
            .map(|n| n.level_steps.iter().copied().fold(0., f64::max))
            .collect();
        let median_peak = dsp::median(&peaks).max(1e-4);
        for note in &mut out.notes {
            let len = note.hz.len();
            if len < 4 {
                continue;
            }
            let note_peak = note.level_steps.iter().copied().fold(0., f64::max);
            let rel = (note_peak / median_peak).clamp(0.4, 2.2);
            // Expand dynamic phrasing contrast slightly while keeping strict headroom.
            let dyn_scale = (0.86 + 0.24 * rel).clamp(0.72, 1.18);
            for (k, lvl) in note.level_steps.iter_mut().enumerate() {
                let t = k as f64 * out.hop_s;
                // Slightly crisp pick accent on first 35 ms of accented notes.
                let accent_lift = if rel > 1.05 && t < 0.035 {
                    1.0 + 0.14 * (1.0 - t / 0.035)
                } else {
                    1.0
                };
                *lvl = (*lvl * dyn_scale * accent_lift).clamp(0.0, 2.4);
            }
            if let Some(strikes) = &mut note.re_picks {
                for strike in strikes.iter_mut() {
                    strike.strength = (strike.strength * 1.18).clamp(0.05, 1.35);
                }
            }
            // Add bright pick-articulation harmonic emphasis on accented events.
            if rel > 1.0 {
                for h in 2..=6 {
                    let cur = note.shape.mode_weights.get(&h).copied().unwrap_or(1.0);
                    let boost = 1.0 + 0.06 * (h as f64 - 1.0).min(4.0);
                    note.shape
                        .mode_weights
                        .insert(h, (cur * boost).clamp(0.2, 3.0));
                }
            }
            // Subtle delayed left-hand vibrato on steady sustained notes (> 280 ms).
            let dur = note.end - note.start;
            if dur >= 0.28 {
                let mut sorted_hz = note.hz.clone();
                sorted_hz.sort_by(f64::total_cmp);
                let lo = sorted_hz[len / 5];
                let hi = sorted_hz[(len * 4) / 5];
                let span_cents = (1200. * (hi / lo.max(1e-6)).log2()).abs();
                // Do not add synthetic vibrato onto active slides or bends.
                if span_cents < 55.0 {
                    for (k, hz) in note.hz.iter_mut().enumerate() {
                        let t = k as f64 * out.hop_s;
                        let fade_in = ((t - 0.11) / 0.12).clamp(0.0, 1.0);
                        let fade_out = ((dur - t) / 0.06).clamp(0.0, 1.0);
                        let depth_cents = 15.0 * fade_in * fade_out;
                        let shift = 2_f64.powf((depth_cents * (TAU * 5.2 * t).sin()) / 1200.0);
                        *hz = (*hz * shift).clamp(25.0, f64::from(out.rate) * 0.48);
                    }
                }
            }
        }
        // Keep synthesized expressive dry output within safe headroom (< 0.85).
        let probe = out.synthesize_dry()?;
        let peak = probe.iter().map(|v| v.abs()).fold(0_f32, f32::max);
        if peak > 0.82 {
            let scale = 0.82 / f64::from(peak);
            for note in &mut out.notes {
                for lvl in &mut note.level_steps {
                    *lvl *= scale;
                }
            }
        }
        Ok(out)
    }
}

/// Source-audio analysis for a predominantly monophonic guitar performance.
/// Cue packets contain only time, pitch, evidence metadata, and relative intensity —
/// never source PCM or phase.
pub fn analyze_guitar_wav(path: &Path) -> Result<Performance> {
    let mut wav = hound::WavReader::open(path)?;
    let spec = wav.spec();
    if spec.channels != 1
        || !valid_rate(spec.sample_rate)
        || wav.duration() == 0
        || wav.duration() > spec.sample_rate * 90
    {
        return Err("Expected a mono guitar WAV at 8–96 kHz, up to 90 seconds".into());
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => wav
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 2_f32.powi(i32::from(spec.bits_per_sample) - 1);
            wav.samples::<i32>()
                .map(|v| v.map(|n| n as f32 / scale))
                .collect::<std::result::Result<_, _>>()?
        }
    };
    analyze_guitar(&samples, spec.sample_rate)
}

fn cents_diff(a: f64, b: f64) -> f64 {
    if a <= 0. || b <= 0. {
        return f64::INFINITY;
    }
    1200. * (a / b).log2()
}

fn local_floor_at_scratch(
    spec: &[f64],
    k: usize,
    max_bin: usize,
    pad_bins: usize,
    buf: &mut Vec<f64>,
) -> f64 {
    let guard = (2 * pad_bins).max(4);
    let span = (5 * pad_bins).max(16);
    buf.clear();
    let left_lo = k.saturating_sub(span).max(1);
    let left_hi = k.saturating_sub(guard);
    if left_lo < left_hi {
        buf.extend_from_slice(&spec[left_lo..left_hi]);
    }
    let right_lo = (k + guard + 1).min(max_bin + 1);
    let right_hi = (k + span).min(max_bin);
    if right_lo <= right_hi {
        buf.extend_from_slice(&spec[right_lo..=right_hi]);
    }
    if buf.is_empty() {
        return 1e-12;
    }
    let mid = buf.len() / 2;
    let (_, &mut median, _) = buf.select_nth_unstable_by(mid, f64::total_cmp);
    median.max(1e-12)
}

fn calibrate_level_steps(note: &Note, target_rms: &[f64], rate: u32, hop_s: f64) -> Vec<f64> {
    let n_frames = note.hz.len();
    if n_frames == 0 {
        return Vec::new();
    }
    let fs = f64::from(rate);
    let inv_fs_hop = 1.0 / (fs * hop_s);
    let count = ((note.end - note.start) * fs).round_ties_even().max(1.) as usize;
    let median = median_of_slice(&note.hz);
    let params = build_string_mode_params(note, median, fs);
    let (initial_strength, mid_repicks) =
        collect_note_repicks(note, count, fs, 0, false).expect("non-strict repick collect");

    let mut state = crate::string_state::StringState::new();
    state.repick(initial_strength, &params.weights);
    let mut rp_cursor = 0usize;
    let mut prefix_sq = Vec::with_capacity(count + 1);
    prefix_sq.push(0.0_f64);
    let mut acc_sq = 0.0_f64;
    for i in 0..count {
        let f = interp(&note.hz, i as f64 * inv_fs_hop);
        let modal_sum = state.tick_cached(f, fs, &params.sample_losses, &params.pickups);
        // Calibrate against the un-enveloped modal carrier so boundary ramps are not inverted.
        let out = 1.53 * modal_sum;
        acc_sq += out * out;
        prefix_sq.push(acc_sq);
        while rp_cursor < mid_repicks.len() && mid_repicks[rp_cursor].0 == i {
            state.repick(mid_repicks[rp_cursor].1, &params.weights);
            rp_cursor += 1;
        }
    }
    let hop_samples = (hop_s * fs).round().max(1.) as usize;
    let half_w = ((0.010 * fs).round() as usize).max(hop_samples);
    let mut unit_rms = vec![0.0_f64; n_frames];
    for k in 0..n_frames {
        let center = (k * hop_samples).min(count - 1);
        let mut lo = center.saturating_sub(half_w);
        let mut hi = (center + half_w).min(count);
        if hi - lo < 2 * half_w && count >= 2 * half_w {
            if lo == 0 {
                hi = 2 * half_w;
            } else if hi == count {
                lo = count - 2 * half_w;
            }
        }
        let len = (hi - lo).max(1) as f64;
        unit_rms[k] = ((prefix_sq[hi] - prefix_sq[lo]).max(0.0) / len).sqrt();
    }
    let mut steps = vec![0.0_f64; n_frames];
    let mut fallback = 0.0_f64;
    for k in 0..n_frames {
        let tgt = target_rms.get(k).copied().unwrap_or(0.0);
        if unit_rms[k] > 1e-3 {
            let g = (tgt / unit_rms[k]).clamp(0.0, 3.8);
            steps[k] = g;
            fallback = g;
        } else {
            steps[k] = (fallback * 0.9).clamp(0.0, 3.8);
            fallback = steps[k];
        }
    }
    // Two multiplicative deconvolution passes compensate for synthesize_dry()'s smooth() kernel.
    for _ in 0..2 {
        let sm = smooth(&steps);
        for k in 0..n_frames {
            let tgt = target_rms.get(k).copied().unwrap_or(0.0);
            let pred = sm[k] * unit_rms[k];
            if pred > 1e-5 && unit_rms[k] > 2e-3 {
                let corr = (tgt / pred).clamp(0.5, 1.85);
                steps[k] = (steps[k] * corr).clamp(0.0, 3.85);
            }
        }
    }
    steps
}

pub fn analyze_guitar(input: &[f32], rate: u32) -> Result<Performance> {
    if !valid_rate(rate)
        || input.is_empty()
        || input.len() > rate as usize * 90
        || input.iter().any(|v| !v.is_finite() || v.abs() > 4.)
    {
        return Err("Invalid guitar input".into());
    }
    const FS: f64 = 12000.;
    const H: usize = 60;
    const WIN: usize = 480;
    const NFFT: usize = 4096;
    const LAG: usize = 156;
    const HOP_S: f64 = 0.005;
    const MIN_F0: f64 = 75.0;
    const MAX_F0: f64 = 1400.0;

    let mut s = Vec::with_capacity((input.len() * 12000 / rate as usize) + 1);
    let ratio = f64::from(rate) / FS;
    let cutoff = 0.42 / ratio.max(1.);
    let mut smooth_dc = 0.;
    // 65 Hz DC/rumble filter preserves low E2 (82.41 Hz) fundamental energy.
    let hp = 1. - (-TAU * 65. / FS).exp();
    // Windowed sinc prevents high partials from aliasing during downsampling.
    for i in 0..(input.len() as f64 / ratio).ceil() as usize {
        let t = i as f64 * ratio;
        let c = t.floor() as isize;
        let mut v = 0.;
        let mut wsum = 0.;
        for j in -24..=24 {
            let d = (c + j) as f64 - t;
            let w = 0.5 + 0.5 * (PI * d / 25.).cos();
            let h = if d.abs() < 1e-12 {
                2. * cutoff
            } else {
                (TAU * cutoff * d).sin() / (PI * d)
            };
            if let Some(&a) = input.get((c + j) as usize) {
                v += a as f64 * h * w;
                wsum += h * w;
            }
        }
        let v = if wsum.abs() > 1e-6 { v / wsum } else { 0. };
        smooth_dc += hp * (v - smooth_dc);
        s.push(v - smooth_dc);
    }

    let frame_count = s.len().div_ceil(H);
    let max_eval_hz = 5500.0_f64.min(f64::from(rate) * 0.45);
    let df = FS / NFFT as f64;
    let max_bin = ((max_eval_hz / df).floor() as usize).min(NFFT / 2 - 1);
    let hann_win = dsp::hann(WIN);
    let spec_scale = 2.0 / hann_win.iter().sum::<f64>();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(NFFT);
    let mut fft_buf = vec![Complex::new(0., 0.); NFFT];
    let mut fft_scratch = vec![Complex::new(0., 0.); fft.get_inplace_scratch_len()];
    let mut raw_peaks: Vec<(f64, f64, f64, usize)> = Vec::new();
    let mut covered = vec![false; max_bin + 2];
    let mut floor_scratch: Vec<f64> = Vec::with_capacity(128);

    let mut levels = Vec::with_capacity(frame_count);
    let mut raw_levels = Vec::with_capacity(frame_count);
    let mut frame_specs: Vec<Vec<f64>> = Vec::with_capacity(frame_count);
    let mut frame_hyps: Vec<Vec<PitchHypothesis>> = Vec::with_capacity(frame_count);
    let mut frame_tonal: Vec<f64> = Vec::with_capacity(frame_count);
    let mut frame_peak_hz: Vec<f64> = Vec::with_capacity(frame_count);
    let raw_half = ((0.005 * f64::from(rate)).round() as usize).max(1);

    for i in 0..frame_count {
        let c = i * H;
        let get = |j: isize| s.get((c as isize + j) as usize).copied().unwrap_or(0.);
        let rms_10ms = ((-60..60).map(|j| get(j).powi(2)).sum::<f64>() / 120.).sqrt();
        levels.push(rms_10ms);
        let raw_c = ((i as f64 * HOP_S * f64::from(rate)).round() as usize).min(input.len());
        let r_lo = raw_c.saturating_sub(raw_half);
        let r_hi = (raw_c + raw_half).min(input.len());
        let r_rms = if r_hi > r_lo {
            (input[r_lo..r_hi]
                .iter()
                .map(|&v| f64::from(v) * f64::from(v))
                .sum::<f64>()
                / (r_hi - r_lo) as f64)
                .sqrt()
        } else {
            0.0
        };
        raw_levels.push(r_rms);

        let mut frame = [0.; WIN];
        for (j, x) in frame.iter_mut().enumerate() {
            *x = get(j as isize - WIN as isize / 2);
        }
        let mean = frame.iter().sum::<f64>() / WIN as f64;
        for j in 0..WIN {
            frame[j] -= mean;
            fft_buf[j] = Complex::new(frame[j] * hann_win[j], 0.0);
        }
        fft_buf[WIN..].fill(Complex::new(0., 0.));
        fft.process_with_scratch(&mut fft_buf, &mut fft_scratch);
        let spec: Vec<f64> = fft_buf[..=max_bin + 1]
            .iter()
            .map(|v| v.norm() * spec_scale)
            .collect();
        let total_power: f64 = spec[1..=max_bin].iter().map(|v| v * v).sum();
        let max_spec = spec[1..=max_bin].iter().copied().fold(0., f64::max);

        // Extract prominent spectral peaks with local median floor.
        let mut prom_peaks: Vec<(f64, f64, f64)> = Vec::new();
        let mut tonal_power = 0.0_f64;
        let mut strongest_peak_hz = 0.0_f64;
        let mut strongest_peak_amp = 0.0_f64;
        let pad_bins = NFFT / WIN;
        if max_spec > 1e-8 && total_power > 1e-16 {
            raw_peaks.clear();
            let min_peak = max_spec * 0.014;
            for k in 2..max_bin {
                let v = spec[k];
                if v > spec[k - 1] && v >= spec[k + 1] && v >= min_peak {
                    let floor =
                        local_floor_at_scratch(&spec, k, max_bin, pad_bins, &mut floor_scratch);
                    let prom = v / floor;
                    if prom >= 2.2 {
                        let hz = dsp::refined_bin(&spec, k) * df;
                        if (65.0..=max_eval_hz).contains(&hz) {
                            raw_peaks.push((hz, v, prom, k));
                        }
                    }
                }
            }
            raw_peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
            raw_peaks.truncate(24);
            let lobe_half = (pad_bins + 3).min(12);
            covered.fill(false);
            for &(hz, amp, prom, k) in &raw_peaks {
                if amp > strongest_peak_amp {
                    strongest_peak_amp = amp;
                    strongest_peak_hz = hz;
                }
                prom_peaks.push((hz, amp, prom));
                for b in k.saturating_sub(lobe_half)..=(k + lobe_half).min(max_bin) {
                    if !covered[b] {
                        covered[b] = true;
                        tonal_power += spec[b] * spec[b];
                    }
                }
            }
        }
        let tonal_fraction = (tonal_power / total_power.max(1e-18)).clamp(0., 1.);
        frame_tonal.push(tonal_fraction);
        frame_peak_hz.push(strongest_peak_hz);

        // Normalized autocorrelation (NSDF) without upper-harmonic lag penalty.
        let mut prefix = [0.; WIN + 1];
        for (j, x) in frame.iter().enumerate() {
            prefix[j + 1] = prefix[j] + x * x;
        }
        let mut nsdf = [0.; LAG + 1];
        if prefix[WIN] > 1e-11 {
            for lag in 8..=LAG {
                let cross = frame[..WIN - lag]
                    .iter()
                    .zip(&frame[lag..])
                    .map(|(a, b)| a * b)
                    .sum::<f64>();
                nsdf[lag] = 2. * cross / (prefix[WIN - lag] + prefix[WIN] - prefix[lag]).max(1e-12);
            }
        }
        let nsdf_at = |hz: f64| -> f64 {
            let lag = FS / hz;
            let lo = lag.floor() as usize;
            if lo < 8 || lo + 1 > LAG {
                return 0.0;
            }
            let frac = lag - lo as f64;
            (nsdf[lo] + frac * (nsdf[lo + 1] - nsdf[lo])).clamp(0.0, 1.0)
        };

        // Gather candidate fundamentals from spectral subharmonics and NSDF peaks.
        let mut raw_cands: Vec<f64> = Vec::new();
        for &(p_hz, _, _) in prom_peaks.iter().take(10) {
            for d in 1..=6 {
                let cand = p_hz / d as f64;
                if (MIN_F0..=MAX_F0).contains(&cand) {
                    raw_cands.push(cand);
                }
            }
        }
        for p in 10..LAG {
            if nsdf[p] > 0.30 && nsdf[p] > nsdf[p - 1] && nsdf[p] >= nsdf[p + 1] {
                let (a, b, c) = (nsdf[p - 1], nsdf[p], nsdf[p + 1]);
                let denom = (a - 2. * b + c).min(-1e-9);
                let lag = p as f64 + (0.5 * (a - c) / denom).clamp(-0.5, 0.5);
                let cand = FS / lag;
                if (MIN_F0..=MAX_F0).contains(&cand) {
                    raw_cands.push(cand);
                }
            }
        }
        raw_cands.sort_by(f64::total_cmp);
        let mut cands: Vec<f64> = Vec::new();
        for c in raw_cands {
            if cands
                .last()
                .is_none_or(|&prev| cents_diff(c, prev).abs() > 25.0)
            {
                cands.push(c);
            }
        }

        let total_peak_power: f64 = prom_peaks.iter().map(|p| p.1 * p.1).sum();
        let mut scored: Vec<PitchHypothesis> = Vec::new();
        if total_peak_power > 1e-16 {
            for &f0 in &cands {
                let mut odd_power = 0.0;
                let mut even_power = 0.0;
                let mut harmonic_count = 0usize;
                let mut h1_present = false;
                let mut h5_or_h7_present = false;
                let mut refined_num = 0.0;
                let mut refined_den = 0.0;
                let max_h = ((max_eval_hz / f0).floor() as usize).clamp(1, 10);
                for h in 1..=max_h {
                    let target = h as f64 * f0;
                    let tol = (df * 1.9).max(target * 0.019);
                    let mut best_match: Option<(f64, f64)> = None;
                    for &(p_hz, amp, _) in &prom_peaks {
                        if (p_hz - target).abs() <= tol && best_match.is_none_or(|(_, ba)| amp > ba)
                        {
                            best_match = Some((p_hz, amp));
                        }
                    }
                    if let Some((p_hz, amp)) = best_match {
                        let pw = amp * amp;
                        if h == 1 {
                            h1_present = true;
                        }
                        if h == 5 || h == 7 {
                            h5_or_h7_present = true;
                        }
                        if h % 2 == 1 {
                            odd_power += pw;
                        } else {
                            even_power += pw;
                        }
                        harmonic_count += 1;
                        let w = pw / h as f64;
                        refined_num += (p_hz / h as f64) * w;
                        refined_den += w;
                    }
                }
                // Reject phantom fundamentals inferred from a single high-frequency peak:
                // require either the fundamental itself (h=1) or >= 2 supported harmonics.
                if harmonic_count == 0 || (harmonic_count < 2 && !h1_present) {
                    continue;
                }
                let f0_ref = if refined_den > 0. {
                    (refined_num / refined_den).clamp(MIN_F0, MAX_F0)
                } else {
                    f0
                };
                let explained = ((odd_power + even_power) / total_peak_power).clamp(0., 1.);
                let odd_ratio = (odd_power / (odd_power + even_power).max(1e-18)).clamp(0., 1.);
                let ac = nsdf_at(f0_ref);

                // Penalize subharmonic aliases (e.g. f0/2 when true signal is f0) that lack
                // odd-harmonic support while leaving pure sines (h=1 => odd_ratio=1) intact.
                let mut odd_factor = if odd_ratio < 0.03 {
                    0.30
                } else if odd_ratio < 0.09 {
                    0.70
                } else {
                    1.0
                };
                // Guard against 2:3 dyad phantom subharmonics (e.g. G3+D4 mimicking G2):
                // all detected harmonics are multiples of 2 or 3 with h1, h5, and h7 absent.
                if !h1_present && !h5_or_h7_present && max_h >= 5 {
                    odd_factor *= 0.45;
                }

                // Guard against octave-up locking when true fundamental is weak: check if
                // odd half-harmonics (0.5, 1.5, 2.5, 3.5 * f0_ref) carry real tonal power.
                let mut sub_odd_power = 0.0;
                let mut sub_odd_count = 0usize;
                if f0_ref * 0.5 >= MIN_F0 {
                    for half_h in [0.5, 1.5, 2.5, 3.5, 4.5] {
                        let target = half_h * f0_ref;
                        if target >= max_eval_hz {
                            break;
                        }
                        let tol = (df * 1.9).max(target * 0.019);
                        if let Some(&(_, amp, _)) = prom_peaks
                            .iter()
                            .filter(|&&(p_hz, _, _)| (p_hz - target).abs() <= tol)
                            .max_by(|a, b| a.1.total_cmp(&b.1))
                        {
                            sub_odd_power += amp * amp;
                            sub_odd_count += 1;
                        }
                    }
                }
                let sub_odd_share = sub_odd_power / total_peak_power;
                let sub_guard =
                    if sub_odd_share > 0.14 || (sub_odd_count >= 2 && sub_odd_share > 0.05) {
                        0.50
                    } else if sub_odd_share > 0.06 {
                        0.75
                    } else {
                        1.0
                    };

                let density = (harmonic_count.min(5) as f64) / (max_h.clamp(1, 5) as f64);
                let h1_bonus = if h1_present { 0.04 } else { 0.0 };
                let score = (0.54 * explained
                    + 0.22 * ac
                    + 0.12 * density
                    + 0.08 * tonal_fraction
                    + h1_bonus)
                    * odd_factor
                    * sub_guard;
                scored.push(PitchHypothesis {
                    hz: f0_ref,
                    score,
                    explained_fraction: explained,
                    harmonic_count,
                    odd_harmonic_ratio: odd_ratio,
                    autocorr: ac,
                });
            }
        }

        // Deduplicate hypotheses within 35 cents, keeping highest score.
        scored.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut hyps: Vec<PitchHypothesis> = Vec::new();
        for h in scored {
            if hyps
                .iter()
                .all(|prev| cents_diff(h.hz, prev.hz).abs() > 35.0)
            {
                hyps.push(h);
                if hyps.len() == 4 {
                    break;
                }
            }
        }
        frame_hyps.push(hyps);
        frame_specs.push(spec);
    }

    let max_level = levels.iter().copied().fold(0., f64::max);
    if max_level < 1e-5 {
        let frame_evidence = (0..frame_count)
            .map(|i| FrameEvidence {
                time_s: i as f64 * HOP_S,
                level_rms: levels[i],
                status: EvidenceStatus::Silence,
                selected_hz: None,
                tonal_fraction: frame_tonal[i],
                hypotheses: Vec::new(),
            })
            .collect();
        return Ok(Performance {
            rate,
            samples: input.len(),
            hop_s: HOP_S,
            notes: Vec::new(),
            frame_evidence,
        });
    }
    // Low enough to retain quiet attacks (~-30 dB vs peak) while staying above silence floor.
    let silence_floor = (max_level * 0.015).max(0.0008);

    // Frame-level classification separating Silence, OutOfTarget, Uncertain, and Supported.
    let mut status = vec![EvidenceStatus::Silence; frame_count];
    let mut selected = vec![None::<f64>; frame_count];
    let mut prev_hz: Option<f64> = None;

    for i in 0..frame_count {
        if levels[i] < silence_floor {
            status[i] = EvidenceStatus::Silence;
            prev_hz = None;
            continue;
        }
        let hyps = &frame_hyps[i];
        if hyps.is_empty() || frame_tonal[i] < 0.24 {
            status[i] = EvidenceStatus::OutOfTarget;
            prev_hz = None;
            continue;
        }
        let best = &hyps[0];
        // If strongest spectral peak is above guitar fundamental band and no multi-harmonic
        // guitar series explains it, classify as out-of-target.
        if frame_peak_hz[i] > MAX_F0 * 1.1 && best.harmonic_count < 2 {
            status[i] = EvidenceStatus::OutOfTarget;
            prev_hz = None;
            continue;
        }
        if best.score < 0.34 {
            status[i] = EvidenceStatus::OutOfTarget;
            prev_hz = None;
            continue;
        }
        if best.score < 0.44 {
            status[i] = EvidenceStatus::Uncertain;
            prev_hz = None;
            continue;
        }

        // Check competing hypotheses (> 100 cents apart).
        let competing = hyps
            .iter()
            .skip(1)
            .find(|h| cents_diff(h.hz, best.hz).abs() > 100.0);
        if let Some(alt) = competing {
            let ratio = alt.score / best.score.max(1e-9);
            // If two distant pitch hypotheses have nearly equal evidence and neither explains
            // the tonal spectrum alone, check whether temporal context clearly favors one;
            // if not (or if both carry comparable independent harmonic energy), abstain.
            if ratio >= 0.86 && best.explained_fraction < 0.72 {
                let chosen = prev_hz.and_then(|p| {
                    let d0 = cents_diff(best.hz, p).abs();
                    let d1 = cents_diff(alt.hz, p).abs();
                    if d0 < 45.0 && d1 > 120.0 && best.explained_fraction >= 0.56 {
                        Some(best.hz)
                    } else if d1 < 45.0
                        && d0 > 120.0
                        && alt.explained_fraction >= 0.56
                        && ratio >= 0.93
                    {
                        Some(alt.hz)
                    } else {
                        None
                    }
                });
                if let Some(hz) = chosen {
                    status[i] = EvidenceStatus::Supported;
                    selected[i] = Some(hz);
                    prev_hz = Some(hz);
                } else {
                    status[i] = EvidenceStatus::Uncertain;
                    selected[i] = None;
                    prev_hz = None;
                }
                continue;
            }
        }

        // Gentle continuity tie-breaker only among top-scoring hypotheses within 6% of best.
        let chosen_hz = if let Some(p) = prev_hz {
            hyps.iter()
                .filter(|h| h.score >= best.score * 0.94)
                .min_by(|a, b| {
                    let ca = (1.0 - a.score) + 0.06 * (cents_diff(a.hz, p).abs() / 1200.).min(1.);
                    let cb = (1.0 - b.score) + 0.06 * (cents_diff(b.hz, p).abs() / 1200.).min(1.);
                    ca.total_cmp(&cb)
                })
                .map(|h| h.hz)
                .unwrap_or(best.hz)
        } else {
            best.hz
        };

        status[i] = EvidenceStatus::Supported;
        selected[i] = Some(chosen_hz);
        prev_hz = Some(chosen_hz);
    }

    // Reclassify isolated ultra-short islands (< 6 frames = 30 ms) or ultra-steep
    // single-partial sweeps (> 10,000 cents/s) as OutOfTarget rather than guitar notes.
    let mut idx = 0;
    while idx < frame_count {
        if status[idx] != EvidenceStatus::Supported {
            idx += 1;
            continue;
        }
        let a = idx;
        while idx < frame_count && status[idx] == EvidenceStatus::Supported {
            idx += 1;
        }
        let b = idx;
        let len = b - a;
        let mean_harm = (a..b)
            .map(|k| frame_hyps[k].first().map_or(0, |h| h.harmonic_count))
            .sum::<usize>() as f64
            / len as f64;
        let max_rate_cents_per_s = if len >= 3 {
            (a..b - 1)
                .map(|k| cents_diff(selected[k + 1].unwrap(), selected[k].unwrap()).abs() / HOP_S)
                .fold(0., f64::max)
        } else {
            0.0
        };
        if len < 6 || (mean_harm < 1.5 && max_rate_cents_per_s > 9000.0) {
            for k in a..b {
                status[k] = EvidenceStatus::OutOfTarget;
                selected[k] = None;
            }
        }
    }

    // Build FrameEvidence before micro-gap bridging so raw abstentions remain visible!
    let frame_evidence: Vec<FrameEvidence> = (0..frame_count)
        .map(|i| FrameEvidence {
            time_s: i as f64 * HOP_S,
            level_rms: levels[i],
            status: status[i],
            selected_hz: selected[i],
            tonal_fraction: frame_tonal[i],
            hypotheses: frame_hyps[i].clone(),
        })
        .collect();

    // Assemble continuous supported pitch array, bridging ONLY <= 3-frame (15 ms)
    // micro-glitches inside a continuing note where both sides agree within 100 cents
    // and energy remains active (NEVER bridge silence, OutOfTarget clicks/chirps, or
    // longer Uncertain regions).
    let mut f = vec![0.0_f64; frame_count];
    for i in 0..frame_count {
        if status[i] == EvidenceStatus::Supported {
            f[i] = selected[i].unwrap_or(0.0);
        }
    }
    let mut j = 0;
    while j < frame_count {
        if f[j] > 0. {
            j += 1;
            continue;
        }
        let a = j;
        while j < frame_count && f[j] == 0. {
            j += 1;
        }
        let gap_len = j - a;
        let non_silent =
            (a..j).all(|k| status[k] == EvidenceStatus::Uncertain && levels[k] >= silence_floor);
        if a > 0
            && j < frame_count
            && gap_len <= 3
            && non_silent
            && cents_diff(f[a - 1], f[j]).abs() < 100.0
        {
            let (l, r) = (f[a - 1], f[j]);
            for k in a..j {
                f[k] = l + (r - l) * (k - a + 1) as f64 / (gap_len + 1) as f64;
            }
        }
    }

    // Light 3-point median filter strictly inside supported runs (never across boundaries)
    // to reject single-frame glitches while preserving slides and bends.
    let f_raw = f.clone();
    for i in 1..frame_count.saturating_sub(1) {
        if f_raw[i - 1] > 0. && f_raw[i] > 0. && f_raw[i + 1] > 0. {
            let d_prev = cents_diff(f_raw[i], f_raw[i - 1]).abs();
            let d_next = cents_diff(f_raw[i], f_raw[i + 1]).abs();
            let d_span = cents_diff(f_raw[i - 1], f_raw[i + 1]).abs();
            if d_prev > 120. && d_next > 120. && d_span < 90. {
                f[i] = 0.5 * (f_raw[i - 1] + f_raw[i + 1]);
            }
        }
    }

    // Smoothed dB envelope for onset, repeated-strike, and pitch-transition segmentation.
    let db_env: Vec<f64> = levels.iter().map(|v| 20. * v.max(1e-5).log10()).collect();
    let mut smooth_db = vec![0.0_f64; frame_count];
    for i in 0..frame_count {
        let lo = i.saturating_sub(2);
        let hi = (i + 2).min(frame_count - 1);
        smooth_db[i] = db_env[lo..=hi].iter().sum::<f64>() / (hi - lo + 1) as f64;
    }

    let mut notes: Vec<Note> = Vec::new();
    let mut i = 0;
    while i < frame_count {
        if f[i] == 0. {
            i += 1;
            continue;
        }
        let start = i;
        while i < frame_count && f[i] > 0. {
            i += 1;
        }
        if i - start < 7 {
            continue;
        }

        // Segment into distinct notes on pitch steps (> 95 cents plateau change) while
        // keeping same-pitch repeated strikes inside the same persistent-string note
        // as explicit `re_picks` events!
        let mut cuts = vec![start];
        for k in start + 10..i.saturating_sub(8) {
            if k - cuts.last().unwrap() < 12 {
                continue;
            }
            let p_before = dsp::median(&f[k - 6..k - 1]);
            let p_after = dsp::median(&f[k + 1..k + 6]);
            let step_cents = cents_diff(p_after, p_before).abs();
            let before_stab = cents_diff(f[k - 5], f[k - 2]).abs();
            let after_stab = cents_diff(f[k + 2], f[k + 5]).abs();
            let db_jump = smooth_db[(k + 4).min(i - 1)] - smooth_db[k.saturating_sub(4)];
            // Distinguish a note change from a continuous slide: a note change has
            // either a clear step between two stable plateaus or a pitch jump with an attack.
            let plateau_jump = step_cents > 95.0 && before_stab < 55.0 && after_stab < 55.0;
            let attacked_step = step_cents > 85.0 && db_jump > 3.5;
            if plateau_jump || attacked_step {
                cuts.push(k);
            }
        }
        cuts.push(i);

        for bounds in cuts.windows(2) {
            let (a, b) = (bounds[0], bounds[1]);
            if b - a < 6 {
                continue;
            }
            // Detect repeated strikes (same pitch, sharp energy re-attack without rest).
            let mut repicks: Vec<Strike> = Vec::new();
            let mut last_strike = a;
            for k in a + 10..b.saturating_sub(6) {
                if k - last_strike < 16 {
                    continue;
                }
                let db_before = smooth_db[k.saturating_sub(6)..k.saturating_sub(1)]
                    .iter()
                    .copied()
                    .fold(f64::INFINITY, f64::min);
                let db_after = smooth_db[k..(k + 5).min(b)]
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max);
                let rise_db = db_after - db_before;
                let pitch_jump = cents_diff(f[k + 2], f[k - 2]).abs();
                let is_local_rise = (smooth_db[(k + 2).min(b - 1)]
                    - smooth_db[k.saturating_sub(2)])
                    >= (smooth_db[(k + 1).min(b - 1)] - smooth_db[k.saturating_sub(3)]);
                if rise_db >= 5.5
                    && pitch_jump < 65.0
                    && is_local_rise
                    && levels[k + 1] > silence_floor * 2.5
                {
                    let strength = ((rise_db - 2.5) / 11.0).clamp(0.25, 1.15);
                    repicks.push(Strike {
                        offset_s: (k - a) as f64 * HOP_S,
                        strength,
                    });
                    last_strike = k;
                }
            }

            let mut l_sorted = raw_levels[a..b].to_vec();
            l_sorted.sort_by(f64::total_cmp);
            let accent = l_sorted[((l_sorted.len() - 1) * 73) / 100];
            let mut note = Note {
                string_id: None,
                re_picks: (!repicks.is_empty()).then_some(repicks),
                start: a as f64 * HOP_S,
                end: b as f64 * HOP_S,
                hz: f[a..b].to_vec(),
                level_steps: vec![accent; b - a],
                _accent: Some(accent),
                shape: Shape::default(),
            };
            note.level_steps = calibrate_level_steps(&note, &raw_levels[a..b], rate, HOP_S);
            notes.push(note);
        }
    }

    // Spectrogram-evidenced overlapping ringing: when note[n+1] starts at a different
    // pitch shortly after note[n], probe non-colliding harmonics of note[n]'s ending pitch
    // during note[n+1] and extend note[n] ONLY where residual harmonic ridges are observed.
    let note_len = notes.len();
    for idx in 0..note_len.saturating_sub(1) {
        let prev_end_f = (notes[idx].end / HOP_S).round() as usize;
        let next_start_f = (notes[idx + 1].start / HOP_S).round() as usize;
        if next_start_f > prev_end_f + 12 {
            continue;
        }
        let p_len = notes[idx].hz.len();
        let n_len = notes[idx + 1].hz.len();
        if p_len < 4 || n_len < 4 {
            continue;
        }
        let prev_hz = dsp::median(&notes[idx].hz[p_len.saturating_sub(8)..p_len - 2]);
        let next_hz = dsp::median(&notes[idx + 1].hz[2..n_len.min(8)]);
        if cents_diff(prev_hz, next_hz).abs() < 110.0 {
            continue;
        }
        // Pick harmonics of prev_hz (h=1..=5) that are well separated (> 32 Hz) from
        // every harmonic of next_hz.
        let probe_harmonics: Vec<usize> = (1..=5)
            .filter(|&h| {
                let fh = h as f64 * prev_hz;
                fh < max_eval_hz && (1..=10).all(|k| (fh - k as f64 * next_hz).abs() > 32.0)
            })
            .collect();
        if probe_harmonics.is_empty() {
            continue;
        }
        // Reference amplitude of those non-colliding harmonics safely inside note[idx].
        let ref_frame = prev_end_f.saturating_sub(6).min(frame_count - 1);
        let pad_bins = NFFT / WIN;
        let harm_amp = |frame_i: usize, h: usize| -> (f64, f64) {
            let sp = &frame_specs[frame_i];
            let bin = ((h as f64 * prev_hz / df).round() as usize).clamp(2, max_bin - 1);
            let peak = sp[bin - 1..=bin + 1].iter().copied().fold(0., f64::max);
            let guard = (2 * pad_bins).max(4);
            let span = (4 * pad_bins).max(16);
            let side_med = |lo: usize, hi: usize| -> Option<f64> {
                if lo >= hi {
                    return None;
                }
                let mut v = sp[lo..hi].to_vec();
                v.sort_by(f64::total_cmp);
                Some(v[v.len() / 2])
            };
            let left = side_med(bin.saturating_sub(span).max(1), bin.saturating_sub(guard));
            let right = side_med(
                (bin + guard + 1).min(max_bin),
                (bin + span + 1).min(max_bin + 1),
            );
            let fl = match (left, right) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => 1e-12,
            }
            .max(1e-12);
            (peak, peak / fl)
        };
        let ref_amp: f64 = probe_harmonics
            .iter()
            .map(|&h| harm_amp(ref_frame, h).0)
            .sum();
        if ref_amp < 1e-4 {
            continue;
        }
        let req_harmonics = if probe_harmonics.len() >= 2 { 2 } else { 1 };
        let max_ext = 100.min(frame_count.saturating_sub(prev_end_f));
        let mut ext_ratios = Vec::new();
        let mut miss_run = 0usize;
        for step in 0..max_ext {
            let fi = prev_end_f + step;
            let mut amp_sum = 0.0;
            let mut supported_h = 0usize;
            for &h in &probe_harmonics {
                let (a, prom) = harm_amp(fi, h);
                let min_prom = if req_harmonics == 1 { 7.5 } else { 4.8 };
                if prom >= min_prom && a >= ref_amp * 0.07 {
                    amp_sum += a;
                    supported_h += 1;
                }
            }
            if supported_h >= req_harmonics && amp_sum > ref_amp * 0.14 {
                ext_ratios.push((amp_sum / ref_amp).clamp(0.0, 0.85));
                miss_run = 0;
            } else {
                miss_run += 1;
                if miss_run >= 2 {
                    break;
                }
                ext_ratios.push(ext_ratios.last().copied().unwrap_or(0.) * 0.55);
            }
        }
        // Trim trailing misses.
        while ext_ratios.last().is_some_and(|&r| r < 0.12) {
            ext_ratios.pop();
        }
        if ext_ratios.len() >= 6 {
            let tail_lo = notes[idx].level_steps.len().saturating_sub(4);
            let last_lvl = dsp::median(&notes[idx].level_steps[tail_lo..]);
            let ext_len = ext_ratios.len();
            notes[idx].end += ext_len as f64 * HOP_S;
            for (step, ratio) in ext_ratios.into_iter().enumerate() {
                let taper = ((ext_len - step) as f64 / 4.0).clamp(0.0, 1.0);
                notes[idx].hz.push(prev_hz);
                notes[idx]
                    .level_steps
                    .push((0.55 * last_lvl * ratio * taper).clamp(0.0, 2.5));
            }
        }
    }

    // Ensure dry output stays safely within unmastered editing headroom (< 0.85 peak).
    let mut perf = Performance {
        rate,
        samples: input.len(),
        hop_s: HOP_S,
        notes,
        frame_evidence,
    };
    let check = perf.synthesize_dry()?;
    let peak = check.iter().map(|v| v.abs()).fold(0_f32, f32::max);
    if peak > 0.84 {
        let scale = 0.84 / f64::from(peak);
        for note in &mut perf.notes {
            for lvl in &mut note.level_steps {
                *lvl *= scale;
            }
        }
    }
    Ok(perf)
}

/// Independent pitch estimator for verification (separate from `analyze_guitar`).
/// Uses spectral harmonic-product / cepstral-independent comb on a 10 ms grid,
/// abstaining when uncertain and NEVER folding octave errors away.
#[derive(Clone, Debug, Serialize)]
pub struct IndependentPitchReport {
    pub hop_s: f64,
    pub start_s: f64,
    pub end_s: f64,
    pub total_frames: usize,
    pub both_resolved_frames: usize,
    pub agreed_within_100_cents: usize,
    pub octave_errors: usize,
    pub disagreed_non_octave: usize,
    pub unresolved_frames: usize,
    pub unresolved_reference: usize,
    pub unresolved_dry: usize,
    pub unresolved_both: usize,
    pub median_abs_cents_when_resolved: Option<f64>,
    pub envelope_correlation: Option<f64>,
}

fn independent_f0_frame(
    x: &[f32],
    rate: u32,
    center: usize,
    window: &[f64],
    win_sum: f64,
    fft: &dyn rustfft::Fft<f64>,
    buf: &mut [Complex<f64>],
    scratch: &mut [Complex<f64>],
    floor_rms: f64,
) -> Option<f64> {
    let win = window.len();
    let nfft = buf.len();
    let mut energy = 0.0_f64;
    buf.fill(Complex::new(0., 0.));
    for i in 0..win {
        let v = (center + i)
            .checked_sub(win / 2)
            .and_then(|j| x.get(j))
            .copied()
            .unwrap_or(0.) as f64;
        energy += v * v;
        buf[i].re = v * window[i];
    }
    let rms = (energy / win as f64).sqrt();
    if rms < floor_rms {
        return None;
    }
    fft.process_with_scratch(buf, scratch);
    let df = f64::from(rate) / nfft as f64;
    let max_hz = 5000.0_f64.min(f64::from(rate) * 0.45);
    let max_bin = ((max_hz / df).floor() as usize).min(nfft / 2 - 1);
    let spec: Vec<f64> = buf[..=max_bin + 1]
        .iter()
        .map(|z| z.norm() * 2. / win_sum)
        .collect();
    let max_amp = spec[1..=max_bin].iter().copied().fold(0., f64::max);
    if max_amp < 1e-6 {
        return None;
    }
    // Log-spaced grid at 1/48 octave (~25 cents) from 75 Hz to 1400 Hz, evaluated via
    // harmonic log-magnitude comb with explicit odd-harmonic presence check.
    let mut best_f = 0.0;
    let mut best_s = 0.0;
    let mut second_f = 0.0;
    let mut second_s = 0.0;
    let steps = 204usize; // 75 * 2^(204/48) ≈ 1428 Hz
    let mut grid_scores = vec![(0.0_f64, 0.0_f64); steps];
    let pad_bins = (nfft / win).max(2);
    let min_pk = max_amp * 0.03;
    let mut floor_scratch: Vec<f64> = Vec::with_capacity(128);
    for (idx, slot) in grid_scores.iter_mut().enumerate() {
        let f0 = 75.0 * 2_f64.powf(idx as f64 / 48.0);
        if f0 > 1400.0 {
            break;
        }
        let mut odd_s = 0.0;
        let mut even_s = 0.0;
        let mut hits = 0usize;
        let mut h1_ok = false;
        let max_h = ((max_hz / f0).floor() as usize).clamp(1, 8);
        for h in 1..=max_h {
            let k = ((h as f64 * f0 / df).round() as usize).clamp(1, max_bin);
            let lo = k.saturating_sub(1).max(1);
            let hi = (k + 1).min(max_bin);
            let pk = spec[lo..=hi].iter().copied().fold(0., f64::max);
            if pk > min_pk {
                let fl = local_floor_at_scratch(&spec, k, max_bin, pad_bins, &mut floor_scratch);
                if pk > fl * 2.4 {
                    let contrib = (pk / max_amp).sqrt() / (h as f64).powf(0.35);
                    if h == 1 {
                        h1_ok = true;
                    }
                    if h % 2 == 1 {
                        odd_s += contrib;
                    } else {
                        even_s += contrib;
                    }
                    hits += 1;
                }
            }
        }
        if hits == 0 || (hits < 2 && !h1_ok) {
            continue;
        }
        let odd_share = odd_s / (odd_s + even_s).max(1e-12);
        let penalty = if odd_share < 0.08 { 0.35 } else { 1.0 };
        let total = (odd_s + even_s) * penalty;
        *slot = (f0, total);
    }
    // Extract local peaks on the pitch grid.
    for idx in 1..steps - 1 {
        let (f0, sc) = grid_scores[idx];
        if sc > 0. && sc >= grid_scores[idx - 1].1 && sc >= grid_scores[idx + 1].1 {
            if sc > best_s {
                if cents_diff(f0, best_f).abs() > 100.0 {
                    second_f = best_f;
                    second_s = best_s;
                }
                best_f = f0;
                best_s = sc;
            } else if sc > second_s && cents_diff(f0, best_f).abs() > 100.0 {
                second_f = f0;
                second_s = sc;
            }
        }
    }
    // Abstain if weak or genuinely ambiguous between two distant hypotheses.
    if best_s < 0.85 || (second_f > 0. && second_s / best_s > 0.88) {
        return None;
    }
    Some(best_f)
}

pub fn independent_pitch_check(
    reference: &[f32],
    dry: &[f32],
    rate: u32,
    start_s: f64,
    end_s: f64,
) -> Result<IndependentPitchReport> {
    if reference.len() != dry.len()
        || !valid_rate(rate)
        || reference.is_empty()
        || !start_s.is_finite()
        || !end_s.is_finite()
        || end_s <= start_s
    {
        return Err("Invalid independent pitch check arguments".into());
    }
    let hop_s = 0.010;
    let total_dur = reference.len() as f64 / f64::from(rate);
    let lo_s = start_s.max(0.0);
    let hi_s = end_s.min(total_dur);
    let total_frames = ((hi_s - lo_s) / hop_s).round() as usize;
    if total_frames == 0 {
        return Err("Empty evaluation window".into());
    }
    let win = ((0.046 * f64::from(rate)).round() as usize).clamp(256, 4096);
    let nfft = (win * 4).next_power_of_two().max(4096);
    let window = dsp::hann(win);
    let win_sum: f64 = window.iter().sum();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(nfft);
    let mut buf = vec![Complex::new(0., 0.); nfft];
    let mut scratch = vec![Complex::new(0., 0.); fft.get_inplace_scratch_len()];

    let hop_samples = (hop_s * f64::from(rate)).round().max(1.) as usize;
    let ref_peak_rms = reference
        .chunks_exact(hop_samples)
        .map(|w| (w.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / hop_samples as f64).sqrt())
        .fold(0., f64::max);
    let dry_peak_rms = dry
        .chunks_exact(hop_samples)
        .map(|w| (w.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / hop_samples as f64).sqrt())
        .fold(0., f64::max);
    let ref_floor = (ref_peak_rms * 0.03).max(0.001);
    let dry_floor = (dry_peak_rms * 0.03).max(0.001);

    let mut both_resolved = 0usize;
    let mut agreed = 0usize;
    let mut octave_errors = 0usize;
    let mut disagreed_non_octave = 0usize;
    let mut unres_ref = 0usize;
    let mut unres_dry = 0usize;
    let mut unres_both = 0usize;
    let mut abs_cents = Vec::new();
    let mut env_ref = Vec::with_capacity(total_frames);
    let mut env_dry = Vec::with_capacity(total_frames);

    for k in 0..total_frames {
        let t = lo_s + (k as f64 + 0.5) * hop_s;
        let center = (t * f64::from(rate)).round() as usize;
        let half = hop_samples / 2;
        let a = center.saturating_sub(half);
        let b = (center + half).min(reference.len());
        let rms_slice = |sl: &[f32]| {
            if sl.is_empty() {
                0.0
            } else {
                (sl.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / sl.len() as f64).sqrt()
            }
        };
        env_ref.push(rms_slice(&reference[a..b]));
        env_dry.push(rms_slice(&dry[a..b]));

        let fr = independent_f0_frame(
            reference,
            rate,
            center,
            &window,
            win_sum,
            fft.as_ref(),
            &mut buf,
            &mut scratch,
            ref_floor,
        );
        let fd = independent_f0_frame(
            dry,
            rate,
            center,
            &window,
            win_sum,
            fft.as_ref(),
            &mut buf,
            &mut scratch,
            dry_floor,
        );
        match (fr, fd) {
            (Some(r), Some(d)) => {
                both_resolved += 1;
                let c = cents_diff(d, r).abs();
                abs_cents.push(c);
                if c <= 100.0 {
                    agreed += 1;
                } else if (c - 1200.0).abs() <= 150.0 || (c - 2400.0).abs() <= 150.0 {
                    octave_errors += 1;
                } else {
                    disagreed_non_octave += 1;
                }
            }
            (None, None) => unres_both += 1,
            (None, Some(_)) => unres_ref += 1,
            (Some(_), None) => unres_dry += 1,
        }
    }

    let envelope_correlation = if env_ref.len() > 1 {
        let ma = env_ref.iter().sum::<f64>() / env_ref.len() as f64;
        let mb = env_dry.iter().sum::<f64>() / env_dry.len() as f64;
        let num: f64 = env_ref
            .iter()
            .zip(&env_dry)
            .map(|(a, b)| (a - ma) * (b - mb))
            .sum();
        let den = (env_ref.iter().map(|v| (v - ma).powi(2)).sum::<f64>()
            * env_dry.iter().map(|v| (v - mb).powi(2)).sum::<f64>())
        .sqrt();
        (den > 1e-18).then(|| num / den)
    } else {
        None
    };

    Ok(IndependentPitchReport {
        hop_s,
        start_s: lo_s,
        end_s: hi_s,
        total_frames,
        both_resolved_frames: both_resolved,
        agreed_within_100_cents: agreed,
        octave_errors,
        disagreed_non_octave,
        unresolved_frames: unres_ref + unres_dry + unres_both,
        unresolved_reference: unres_ref,
        unresolved_dry: unres_dry,
        unresolved_both: unres_both,
        median_abs_cents_when_resolved: (!abs_cents.is_empty()).then(|| dsp::median(&abs_cents)),
        envelope_correlation,
    })
}

/// Explicit source-supported boundary correction. It never changes event times,
/// level trajectories, re-picks or timbre controls, and is atomic on rejection.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryPatch {
    pub start_s: f64,
    pub end_s: f64,
    pub approximate_hz: f64,
}
#[derive(Debug, Serialize)]
pub struct BoundaryEvidence {
    pub time_s: f64,
    pub from_hz: f64,
    pub to_hz: f64,
    pub before_audio_hz: Option<f64>,
    pub after_audio_hz: Option<f64>,
}
fn local_peaks(source: &[f32], rate: u32, time: f64, half_seconds: f64) -> Result<Vec<(f64, f64)>> {
    if !time.is_finite() || time < 0. {
        return Err("Invalid boundary time".into());
    }
    let center = (time * rate as f64).round_ties_even() as usize;
    let half = (half_seconds * rate as f64).round_ties_even() as usize;
    if half == 0
        || center < half
        || center
            .checked_add(half)
            .is_none_or(|end| end > source.len())
    {
        return Err("Insufficient source context for boundary evidence".into());
    }
    let x: Vec<_> = source[center - half..center + half]
        .iter()
        .map(|v| *v as f64)
        .collect();
    let spec = crate::dsp::spectrum(&x, &crate::dsp::hann(x.len()), 16384);
    let mut peaks: Vec<_> = crate::dsp::peaks(&spec)
        .into_iter()
        .map(|k| (k as f64 * rate as f64 / 16384., spec[k]))
        .filter(|(hz, _)| *hz > 175. && *hz < 5500.)
        .collect();
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok(peaks)
}
fn contour(source: &[f32], rate: u32, time: f64) -> Option<f64> {
    let peaks = local_peaks(source, rate, time, 0.02).ok()?;
    let (mut best, mut selected) = (0., None);
    for &(hz, _) in peaks.iter().take(10) {
        for divisor in 1..6 {
            let f = hz / divisor as f64;
            if !(180. ..1200.).contains(&f) {
                continue;
            }
            let score = peaks
                .iter()
                .take(12)
                .map(|&(p, a)| {
                    let h = (p / f).round();
                    if (1. ..=12.).contains(&h) && (p - h * f).abs() < 10_f64.max(p * 0.012) {
                        a * a
                    } else {
                        0.
                    }
                })
                .sum::<f64>()
                / (1. + 0.014 * (divisor - 1) as f64);
            if score > best {
                best = score;
                selected = Some(f);
            }
        }
    }
    selected
}
impl Performance {
    pub fn boundary_evidence(&self, source: &[f32], rate: u32) -> Result<Vec<BoundaryEvidence>> {
        self.validate_source(source, rate)?;
        let mut rows = Vec::new();
        for note in &self.notes {
            for (i, pair) in note.hz.windows(2).enumerate() {
                if (1200. * (pair[1] / pair[0]).log2()).abs() > 300. {
                    let t = note.start + (i + 1) as f64 * self.hop_s;
                    rows.push(BoundaryEvidence {
                        time_s: t,
                        from_hz: pair[0],
                        to_hz: pair[1],
                        before_audio_hz: contour(source, rate, t - 0.023),
                        after_audio_hz: contour(source, rate, t + 0.022),
                    });
                }
            }
        }
        Ok(rows)
    }
    fn validate_source(&self, source: &[f32], rate: u32) -> Result<()> {
        if rate != self.rate
            || source.len() != self.samples
            || source.iter().any(|v| !v.is_finite() || v.abs() > 4.)
        {
            return Err("Boundary evidence must match the cue source rate and length".into());
        }
        self.synthesize_dry()?;
        Ok(())
    }
    pub fn patch_boundaries(
        &self,
        source: &[f32],
        rate: u32,
        patches: &[BoundaryPatch],
    ) -> Result<Self> {
        self.validate_source(source, rate)?;
        let mut candidate = self.clone();
        for patch in patches {
            if !patch.start_s.is_finite()
                || !patch.end_s.is_finite()
                || patch.start_s < 0.
                || patch.end_s <= patch.start_s
                || !(180. ..1200.).contains(&patch.approximate_hz)
            {
                return Err("Invalid explicit boundary patch".into());
            }
            let start = (patch.start_s / self.hop_s).round_ties_even() as usize;
            let end = (patch.end_s / self.hop_s).round_ties_even() as usize;
            if start >= end {
                return Err("Boundary patch is shorter than one cue frame".into());
            }
            let note = candidate
                .notes
                .iter_mut()
                .find(|n| {
                    let offset = (n.start / self.hop_s).round_ties_even() as usize;
                    offset <= start && offset + n.hz.len() > end
                })
                .ok_or("Patch must fit one event and retain a following pitch anchor")?;
            let offset = (note.start / self.hop_s).round_ties_even() as usize;
            let peaks = local_peaks(source, rate, patch.end_s + 0.020, 0.015)?;
            let max = peaks.iter().map(|p| p.1).fold(0., f64::max);
            let mut support = Vec::new();
            for h in 1..=3 {
                let target = h as f64 * patch.approximate_hz;
                if let Some(&(hz, amp)) = peaks
                    .iter()
                    .find(|&&(hz, _)| (hz - target).abs() < target * 0.025)
                {
                    if amp > max * 0.014 {
                        support.push(hz / h as f64);
                    }
                }
            }
            if support.len() < 2 {
                return Err(
                    "Boundary correction needs two independently supported harmonics".into(),
                );
            }
            let target = crate::dsp::median(&support);
            if (1200. * (target / note.hz[end - offset]).log2()).abs() >= 65. {
                return Err(
                    "Boundary correction disagrees with the following stable anchor".into(),
                );
            }
            note.hz[start - offset..end - offset].fill(target);
        }
        Ok(candidate)
    }
}

impl Performance {
    /// Re-evaluate only explicitly selected subharmonic-drop ranges. A range is
    /// untouched unless every selected cue is below 65% of its preceding anchor.
    pub fn restore_subharmonic_ranges(
        &self,
        source: &[f32],
        rate: u32,
        ranges: &[[f64; 2]],
    ) -> Result<Self> {
        self.validate_source(source, rate)?;
        let mut out = self.clone();
        for range in ranges {
            if range.iter().any(|v| !v.is_finite() || *v < 0.) || range[1] <= range[0] {
                return Err("Invalid restoration range".into());
            }
            let start = (range[0] / self.hop_s).round_ties_even() as usize;
            let end = (range[1] / self.hop_s).round_ties_even() as usize;
            for note in &mut out.notes {
                let offset = (note.start / self.hop_s).round_ties_even() as usize;
                if start <= offset || end >= offset + note.hz.len() {
                    continue;
                }
                let a = start - offset;
                let b = end - offset;
                if note.hz[a..b].iter().all(|v| *v < 0.65 * note.hz[a - 1]) {
                    let hz = (start..end)
                        .map(|i| {
                            contour(source, rate, i as f64 * self.hop_s)
                                .ok_or("No supported source contour in selected restoration range")
                        })
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    note.hz[a..b].copy_from_slice(&hz);
                }
            }
        }
        Ok(out)
    }
}

/// Compatibility analysis for the retired 48 kHz guitar experiment. The native
/// general guitar analyzer remains available separately; this preserves its
/// original threshold/segmentation recipe for audit and editable cue handoff.
pub fn analyze_guitar_legacy(input: &[f32], rate: u32) -> Result<Performance> {
    if rate != 48000
        || input.is_empty()
        || input.len() > 48000 * 90
        || input.iter().any(|v| !v.is_finite())
    {
        return Err("Legacy guitar profile expects finite 48 kHz mono".into());
    }
    fn i0(x: f64) -> f64 {
        let mut sum = 1.;
        let mut term = 1.;
        for k in 1..40 {
            term *= x * x / (4. * (k * k) as f64);
            sum += term;
            if term < sum * 1e-16 {
                break;
            }
        }
        sum
    }
    let mut fir: Vec<_> = (-40..=40)
        .map(|i: i32| {
            let t = i as f64;
            let sinc = if i == 0 {
                0.25
            } else {
                (PI * t * 0.25).sin() / (PI * t)
            };
            sinc * i0(5. * (1. - (t / 40.).powi(2)).max(0.).sqrt()) / i0(5.)
        })
        .collect();
    let sum: f64 = fir.iter().sum();
    for v in &mut fir {
        *v /= sum;
    }
    let mut s: Vec<_> = (0..input.len().div_ceil(4))
        .map(|i| {
            (-40..=40)
                .filter_map(|k| {
                    let at = i as isize * 4 + k;
                    (at >= 0)
                        .then(|| {
                            input
                                .get(at as usize)
                                .map(|v| *v as f64 * fir[(k + 40) as usize])
                        })
                        .flatten()
                })
                .sum::<f64>()
        })
        .collect();
    // Bilinear second-order Butterworth highpass, cutoff 215 Hz at 12 kHz.
    let k = (PI * 215. / 12000.).tan();
    let a0 = 1. + 2_f64.sqrt() * k + k * k;
    let b = [1. / a0, -2. / a0, 1. / a0];
    let a = [
        1.,
        2. * (k * k - 1.) / a0,
        (1. - 2_f64.sqrt() * k + k * k) / a0,
    ];
    let (mut z1, mut z2) = (0., 0.);
    for v in &mut s {
        let x = *v;
        let y = b[0] * x + z1;
        z1 = b[1] * x - a[1] * y + z2;
        z2 = b[2] * x - a[2] * y;
        *v = y;
    }
    let n = s.len().div_ceil(60);
    let mut levels = Vec::new();
    let mut pitch = vec![0.; n];
    let mut confidence = vec![0.; n];
    for frame in 0..n {
        let mut q: Vec<_> = (0..480)
            .map(|i| {
                (frame * 60 + i)
                    .checked_sub(240)
                    .and_then(|j| s.get(j))
                    .copied()
                    .unwrap_or(0.)
            })
            .collect();
        let mean = q.iter().sum::<f64>() / 480.;
        for v in &mut q {
            *v -= mean;
        }
        levels.push(dsp::rms(&q[180..300]));
        let score: Vec<_> = (0..111)
            .map(|lag| {
                let left = &q[..480 - lag];
                let right = &q[lag..];
                2. * left.iter().zip(right).map(|(a, b)| a * b).sum::<f64>()
                    / (left.iter().chain(right).map(|v| v * v).sum::<f64>()).max(1e-12)
            })
            .collect();
        let good: Vec<_> = dsp::peaks(&score)
            .into_iter()
            .filter(|&k| (10..=105).contains(&k) && score[k] > 0.56)
            .collect();
        let best = good.iter().map(|&k| score[k]).fold(0., f64::max);
        if let Some(&k) = good.iter().find(|&&k| score[k] > 0.56_f64.max(0.87 * best)) {
            let (a, b, c) = (score[k - 1], score[k], score[k + 1]);
            let lag = k as f64
                + if (a - 2. * b + c).abs() > 1e-8 {
                    0.5 * (a - c) / (a - 2. * b + c)
                } else {
                    0.
                };
            pitch[frame] = 12000. / lag;
            confidence[frame] = b;
        }
    }
    let floor = (levels.iter().copied().fold(0., f64::max) * 0.064).max(0.015);
    let mut mask: Vec<_> = (0..n)
        .map(|i| levels[i] > floor && confidence[i] > 0.57)
        .collect();
    let good: Vec<_> = (0..n).filter(|&i| mask[i]).collect();
    if good.is_empty() {
        return Ok(Performance {
            rate,
            samples: input.len(),
            hop_s: 0.005,
            notes: vec![],
            frame_evidence: vec![],
        });
    }
    let mut base = vec![0.; n];
    let mut at = 0;
    for i in 0..n {
        while at + 1 < good.len() && good[at + 1] < i {
            at += 1;
        }
        let a = good[at];
        let b = good[(at + 1).min(good.len() - 1)];
        base[i] = if i <= good[0] {
            pitch[good[0]]
        } else if i >= *good.last().unwrap() {
            pitch[*good.last().unwrap()]
        } else {
            pitch[a] + (pitch[b] - pitch[a]) * (i - a) as f64 / (b - a) as f64
        };
        base[i] = base[i].clamp(120., 1240.);
    }
    let reflect = |i: isize| {
        let k = i.rem_euclid(2 * n as isize);
        if k < n as isize {
            k as usize
        } else {
            2 * n - 1 - k as usize
        }
    };
    let previous = base.clone();
    for i in 0..n {
        base[i] = dsp::median(
            &(-2..=2)
                .map(|d| previous[reflect(i as isize + d)])
                .collect::<Vec<_>>(),
        );
    }
    let old = mask.clone();
    for i in 0..n {
        mask[i] = (-4..=4).filter(|&d| old[reflect(i as isize + d)]).count() >= 5;
    }
    let db: Vec<_> = levels.iter().map(|v| 20. * v.max(1e-5).log10()).collect();
    let kernel: Vec<_> = (-9..=9)
        .map(|k| (-(k * k) as f64 / (2. * 2.2 * 2.2)).exp())
        .collect();
    let sum: f64 = kernel.iter().sum();
    let smooth: Vec<_> = (0..n)
        .map(|i| {
            (-9..=9)
                .zip(&kernel)
                .map(|(d, w)| db[reflect(i as isize + d)] * w)
                .sum::<f64>()
                / sum
        })
        .collect();
    let delta: Vec<_> = (0..n)
        .map(|i| smooth[i] - smooth[i.saturating_sub(7)])
        .collect();
    let mut attacks: Vec<_> = dsp::peaks(&delta)
        .into_iter()
        .filter(|&i| delta[i] >= 5.1)
        .collect();
    attacks.sort_by(|&a, &b| delta[b].total_cmp(&delta[a]));
    let mut selected = Vec::new();
    for a in attacks {
        if selected.iter().all(|&b: &usize| a.abs_diff(b) >= 17) {
            selected.push(a);
        }
    }
    let mut bounds = Vec::new();
    let mut begin = None;
    for i in 0..=n {
        if i < n && mask[i] {
            begin.get_or_insert(i);
        } else if let Some(a) = begin.take() {
            bounds.push((a, i));
        }
    }
    let mut candidates: std::collections::BTreeSet<_> = bounds
        .iter()
        .filter(|(a, b)| b - a >= 10)
        .map(|(a, _)| *a)
        .collect();
    for a in selected {
        if mask[a] && delta[a] > 5.1 {
            candidates.insert(a);
        }
    }
    let cents: Vec<_> = base.iter().map(|v| 1200. * v.log2()).collect();
    for i in 8..n.saturating_sub(8) {
        if mask[i]
            && (dsp::median(&cents[i + 2..i + 7]) - dsp::median(&cents[i - 7..i - 2])).abs() > 105.
            && (dsp::median(&cents[i + 2..i + 5]) - dsp::median(&cents[i + 5..i + 8])).abs() < 42.
            && (cents[i + 2] - cents[i - 2]).abs() > 93.
            && !candidates.iter().any(|&j| i.abs_diff(j) < 13)
        {
            candidates.insert(i);
        }
    }
    let mut notes: Vec<Note> = Vec::new();
    for (lo, hi) in bounds {
        if hi - lo < 10 {
            continue;
        }
        let mut cuts = vec![lo];
        for &j in &candidates {
            if j >= lo + 14 && j < hi - 10 && j - cuts.last().unwrap() >= 14 {
                cuts.push(j);
            }
        }
        cuts.push(hi);
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let mut levels_sorted = levels[a..b].to_vec();
            levels_sorted.sort_by(f64::total_cmp);
            let accent = (dsp::interp(&levels_sorted, (levels_sorted.len() - 1) as f64 * 0.73)
                * 1e5)
                .round_ties_even()
                / 1e5;
            let mut c = cents[a..b].to_vec();
            let med: Vec<_> = (0..c.len())
                .map(|i| {
                    dsp::median(
                        &(-6..=6)
                            .map(|d| c[(i as isize + d).clamp(0, c.len() as isize - 1) as usize])
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            let valid: Vec<_> = (0..c.len())
                .filter(|&i| (c[i] - med[i]).abs() <= 115.)
                .collect();
            if valid.len() >= 2 {
                let old = c.clone();
                let mut j = 0;
                for i in 0..c.len() {
                    while j + 1 < valid.len() && valid[j + 1] < i {
                        j += 1;
                    }
                    let left = valid[j];
                    let right = valid[(j + 1).min(valid.len() - 1)];
                    c[i] = if i <= valid[0] {
                        old[valid[0]]
                    } else if i >= *valid.last().unwrap() {
                        old[*valid.last().unwrap()]
                    } else {
                        old[left]
                            + (old[right] - old[left]) * (i - left) as f64 / (right - left) as f64
                    };
                }
            }
            let hz: Vec<_> = c
                .into_iter()
                .map(|v| (2_f64.powf(v / 1200.) * 1000.).round_ties_even() / 1000.)
                .collect();
            if let Some(prev) = notes.last_mut() {
                if prev.end == a as f64 / 200. {
                    let d = (1200. * (hz[0] / prev.hz.last().unwrap()).log2()).abs();
                    let slope = (1200. * (hz[0] / hz[12.min(hz.len() - 1)]).log2())
                        .abs()
                        .max(
                            (1200.
                                * (prev.hz.last().unwrap()
                                    / prev.hz[prev.hz.len() - 12.min(prev.hz.len())])
                                .log2())
                            .abs(),
                        );
                    if d < 48. && (delta[a] < 8. || (slope > 45. && delta[a] < 11.)) {
                        prev.end = b as f64 / 200.;
                        prev.hz.extend(hz);
                        prev.level_steps.extend(vec![accent; b - a]);
                        continue;
                    }
                }
            }
            notes.push(Note {
                string_id: None,
                re_picks: None,
                start: a as f64 / 200.,
                end: b as f64 / 200.,
                hz,
                level_steps: vec![accent; b - a],
                _accent: Some(accent),
                shape: Default::default(),
            });
        }
    }
    Ok(Performance {
        rate,
        samples: input.len(),
        hop_s: 0.005,
        notes,
        frame_evidence: vec![],
    })
}

fn write_dry_samples(rate: u32, y: &[f32], path: &Path) -> Result<()> {
    if y.iter().any(|x| !x.is_finite() || x.abs() >= 1.) {
        return Err("Dry signal exceeds available headroom; reduce cue levels".into());
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut out = hound::WavWriter::new(
        std::io::BufWriter::new(file),
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for &sample in y {
        out.write_sample(sample)?;
    }
    out.finalize()?;
    Ok(())
}

fn build_source_note_modes(
    note: &Note,
    median: f64,
    kind: crate::studio_catalog::SourceRegenKind,
) -> Vec<(f64, f64, f64)> {
    use crate::studio_catalog::SourceRegenKind;
    let fret = (median / 330.).clamp(1., 2.7);
    let pick = (0.175 * fret).clamp(0.16, 0.36);
    let pickup = (0.145 * fret).clamp(0.11, 0.275);
    (1..20)
        .map(|k| {
            let h = k as f64;
            let user_w = note.shape.mode_weights.get(&k).copied().unwrap_or(1.);
            let (base_w, base_d) = match kind {
                SourceRegenKind::Neutral | SourceRegenKind::PluckedElectric => {
                    let sinc = PI * 0.029 * h;
                    let w = (PI * h * pick).sin() * (PI * h * pickup).sin() * sinc.sin()
                        / sinc
                        / h.powf(0.91);
                    (w, 0.71 + 0.39 * h.powf(0.84))
                }
                SourceRegenKind::PluckedAcoustic => {
                    let sinc = PI * 0.022 * h;
                    let comb = (PI * h * 0.14).sin().abs().max(0.28);
                    let w = comb * (sinc.sin() / sinc) / h.powf(0.76);
                    (w, 0.62 + 0.36 * h.powf(0.82))
                }
                SourceRegenKind::PluckedBass => {
                    let w = if k == 1 {
                        1.75
                    } else if k == 2 {
                        1.05
                    } else {
                        0.72 / h.powf(1.52)
                    };
                    (w, 0.55 + 0.42 * h.powf(0.92))
                }
                SourceRegenKind::BowedLow => {
                    let w = (1.0 / h.powf(0.96))
                        * if k == 1 {
                            1.35
                        } else if k <= 4 {
                            1.12
                        } else {
                            0.85
                        };
                    (w, 0.24 + 0.14 * h.powf(0.55))
                }
                SourceRegenKind::BowedHigh => {
                    let w = (1.0 / h.powf(0.82)) * if (3..=9).contains(&k) { 1.18 } else { 0.92 };
                    (w, 0.22 + 0.15 * h.powf(0.58))
                }
                SourceRegenKind::WoodwindFlute => {
                    let w = match k {
                        1 => 2.15,
                        2 => 0.46,
                        3 => 0.18,
                        _ => 0.22 / h.powf(2.35),
                    };
                    (w, 0.20 + 0.12 * h.powf(0.65))
                }
                SourceRegenKind::WoodwindReed => {
                    let odd_even = if k % 2 == 1 { 1.38 } else { 0.54 };
                    let w = odd_even / h.powf(1.04);
                    (w, 0.24 + 0.15 * h.powf(0.60))
                }
                SourceRegenKind::BrassBright => {
                    let bell = if (2..=9).contains(&k) { 1.42 } else { 0.95 };
                    let w = bell / h.powf(0.78);
                    (w, 0.22 + 0.14 * h.powf(0.60))
                }
                SourceRegenKind::BrassWarm => {
                    let w = (if k <= 3 { 1.32 } else { 0.90 }) / h.powf(1.24);
                    (w, 0.24 + 0.16 * h.powf(0.62))
                }
                SourceRegenKind::VoiceMale => {
                    let w = (if k == 1 { 1.25 } else { 1.0 }) / h.powf(1.16);
                    (w, 0.20 + 0.12 * h.powf(0.55))
                }
                SourceRegenKind::VoiceFemale => {
                    let w = (if k == 1 {
                        1.15
                    } else if k <= 5 {
                        1.05
                    } else {
                        0.88
                    }) / h.powf(1.05);
                    (w, 0.20 + 0.12 * h.powf(0.52))
                }
                SourceRegenKind::VoiceEnsemble => {
                    let w = 1.0 / h.powf(1.18);
                    (w, 0.18 + 0.11 * h.powf(0.55))
                }
                SourceRegenKind::KeyboardStruck => {
                    let w = (if k == 1 { 1.3 } else { 1.0 }) / h.powf(0.95);
                    (w, 0.52 + 0.30 * h.powf(0.84))
                }
                SourceRegenKind::KeyboardOrgan => {
                    let drawbar = match k {
                        1 | 2 | 3 | 4 | 6 | 8 => 1.25,
                        _ => 0.45,
                    };
                    let w = drawbar / h.powf(0.88);
                    (w, 0.10 + 0.06 * h.powf(0.45))
                }
                SourceRegenKind::Percussion => {
                    let w = 1.0 / h.powf(0.86);
                    (w, 1.85 + 0.95 * h.powf(0.88))
                }
                SourceRegenKind::AmpCabinet => {
                    let sinc = PI * 0.029 * h;
                    let w = (PI * h * pick).sin() * (PI * h * pickup).sin() * sinc.sin()
                        / sinc
                        / h.powf(0.85);
                    (w, 0.68 + 0.36 * h.powf(0.82))
                }
            };
            let d = note
                .shape
                .decay_per_second
                .get(&k)
                .copied()
                .unwrap_or(base_d);
            (h, base_w * user_w, d)
        })
        .collect()
}

fn apply_biquad_peaking(buf: &mut [f64], rate: f64, f0: f64, q: f64, gain_db: f64) {
    if f0 <= 10.0 || f0 >= rate * 0.46 || gain_db.abs() < 1e-4 {
        return;
    }
    let a = 10_f64.powf(gain_db / 40.0);
    let w0 = TAU * f0 / rate;
    let cos_w = w0.cos();
    let alpha = w0.sin() / (2.0 * q.max(0.25));
    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cos_w;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cos_w;
    let a2 = 1.0 - alpha / a;
    let (b0, b1, b2, a1, a2) = (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
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

fn apply_biquad_lowpass(buf: &mut [f64], rate: f64, f0: f64, q: f64) {
    if f0 <= 20.0 || f0 >= rate * 0.46 {
        return;
    }
    let w0 = TAU * f0 / rate;
    let cos_w = w0.cos();
    let alpha = w0.sin() / (2.0 * q.max(0.4));
    let b0 = (1.0 - cos_w) * 0.5;
    let b1 = 1.0 - cos_w;
    let b2 = (1.0 - cos_w) * 0.5;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w;
    let a2 = 1.0 - alpha;
    let (b0, b1, b2, a1, a2) = (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
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

fn apply_source_body_contour(
    buf: &mut [f64],
    rate: f64,
    kind: crate::studio_catalog::SourceRegenKind,
) {
    use crate::studio_catalog::SourceRegenKind;
    if let Some(mut modal_body) =
        crate::acoustics_physics::ModalBodyResonator::for_source(rate, kind)
    {
        modal_body.process_inplace(buf);
    }
    match kind {
        SourceRegenKind::Neutral | SourceRegenKind::PluckedElectric => {}
        SourceRegenKind::PluckedAcoustic => {
            apply_biquad_peaking(buf, rate, 185.0, 1.1, 3.2);
            apply_biquad_peaking(buf, rate, 460.0, 1.2, -2.4);
            apply_biquad_peaking(buf, rate, 3600.0, 0.9, 3.5);
        }
        SourceRegenKind::PluckedBass => {
            apply_biquad_peaking(buf, rate, 115.0, 1.0, 4.5);
            apply_biquad_peaking(buf, rate, 1400.0, 1.2, -3.5);
            apply_biquad_lowpass(buf, rate, 3800.0, 0.707);
        }
        SourceRegenKind::BowedLow => {
            // Broad cello/viola Helmholtz & wood-body warmth + mellow box dip + bow-bridge presence
            apply_biquad_peaking(buf, rate, 175.0, 1.1, 5.2);
            apply_biquad_peaking(buf, rate, 650.0, 1.2, -2.8);
            apply_biquad_peaking(buf, rate, 2100.0, 1.3, 2.4);
            apply_biquad_lowpass(buf, rate, 6200.0, 0.707);
        }
        SourceRegenKind::BowedHigh => {
            apply_biquad_peaking(buf, rate, 320.0, 1.2, 2.6);
            apply_biquad_peaking(buf, rate, 2800.0, 1.4, 4.8);
            apply_biquad_peaking(buf, rate, 6000.0, 0.9, 2.2);
        }
        SourceRegenKind::WoodwindFlute => {
            apply_biquad_peaking(buf, rate, 480.0, 0.9, 2.0);
            apply_biquad_peaking(buf, rate, 4800.0, 1.0, 2.8);
            apply_biquad_lowpass(buf, rate, 7500.0, 0.707);
        }
        SourceRegenKind::WoodwindReed => {
            apply_biquad_peaking(buf, rate, 260.0, 1.1, 2.2);
            apply_biquad_peaking(buf, rate, 1650.0, 1.8, 4.6);
            apply_biquad_lowpass(buf, rate, 6500.0, 0.707);
        }
        SourceRegenKind::BrassBright => {
            apply_biquad_peaking(buf, rate, 240.0, 1.0, -1.8);
            apply_biquad_peaking(buf, rate, 2200.0, 1.4, 5.2);
            apply_biquad_peaking(buf, rate, 4200.0, 1.1, 3.0);
        }
        SourceRegenKind::BrassWarm => {
            apply_biquad_peaking(buf, rate, 280.0, 1.0, 4.0);
            apply_biquad_peaking(buf, rate, 1300.0, 1.3, 2.0);
            apply_biquad_lowpass(buf, rate, 4200.0, 0.707);
        }
        SourceRegenKind::VoiceMale => {
            apply_biquad_peaking(buf, rate, 160.0, 1.1, 3.2);
            apply_biquad_peaking(buf, rate, 500.0, 2.2, 4.5);
            apply_biquad_peaking(buf, rate, 1350.0, 2.4, 3.8);
            apply_biquad_peaking(buf, rate, 2700.0, 2.6, 4.4);
            apply_biquad_lowpass(buf, rate, 8000.0, 0.707);
        }
        SourceRegenKind::VoiceFemale => {
            apply_biquad_peaking(buf, rate, 160.0, 1.0, -5.5);
            apply_biquad_peaking(buf, rate, 680.0, 2.2, 4.8);
            apply_biquad_peaking(buf, rate, 1650.0, 2.4, 4.6);
            apply_biquad_peaking(buf, rate, 3150.0, 2.5, 5.5);
            apply_biquad_peaking(buf, rate, 6000.0, 1.0, 2.8);
        }
        SourceRegenKind::VoiceEnsemble => {
            apply_biquad_peaking(buf, rate, 580.0, 1.8, 3.8);
            apply_biquad_peaking(buf, rate, 1500.0, 2.0, 3.4);
            apply_biquad_peaking(buf, rate, 3000.0, 2.0, 3.2);
            apply_biquad_lowpass(buf, rate, 8500.0, 0.707);
        }
        SourceRegenKind::KeyboardStruck => {
            apply_biquad_peaking(buf, rate, 220.0, 1.0, 2.6);
            apply_biquad_peaking(buf, rate, 2500.0, 1.2, 2.8);
        }
        SourceRegenKind::KeyboardOrgan => {
            apply_biquad_peaking(buf, rate, 900.0, 1.1, 3.4);
            apply_biquad_lowpass(buf, rate, 6000.0, 0.707);
        }
        SourceRegenKind::Percussion => {
            apply_biquad_peaking(buf, rate, 190.0, 1.2, 3.2);
            apply_biquad_peaking(buf, rate, 3600.0, 1.1, 4.2);
        }
        SourceRegenKind::AmpCabinet => {
            apply_biquad_peaking(buf, rate, 130.0, 1.1, 3.2);
            apply_biquad_peaking(buf, rate, 1750.0, 1.1, 4.4);
            apply_biquad_lowpass(buf, rate, 4700.0, 0.85);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterTarget {
    Keep,
    Natural,
    Analog,
    Digital,
}

impl Default for CharacterTarget {
    fn default() -> Self {
        Self::Keep
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalPathStyle {
    Default,
    Miced,
    PickupDirect,
    AnalogPreamp,
    CleanDigital,
}

impl Default for SignalPathStyle {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCharacterVariant {
    Standard,
    Rich,
    Basic,
    Warm,
    Bright,
}

impl Default for SourceCharacterVariant {
    fn default() -> Self {
        Self::Standard
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StraightenMode {
    Off,
    Light,
    Moderate,
}

impl Default for StraightenMode {
    fn default() -> Self {
        Self::Off
    }
}

/// Practical Stage-1 DDSP voicing, character, path, loose-profile guidance, and light straightening controls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoicingControls {
    #[serde(default)]
    pub character_target: CharacterTarget,
    #[serde(default)]
    pub signal_path: SignalPathStyle,
    #[serde(default)]
    pub source_variant: SourceCharacterVariant,
    #[serde(default)]
    pub straighten_mode: StraightenMode,
    #[serde(default = "default_true")]
    pub straighten_preserve_bends: bool,
    /// Strength of loose source/character profile guidance (0.0 = shape-only fallback, 1.0 = full guidance).
    #[serde(default = "default_profile_guidance")]
    pub profile_guidance_strength: f64,
    #[serde(default)]
    pub noise_reduction_override: Option<f64>,
    #[serde(default)]
    pub smooth_hf_override: Option<f64>,
}

fn default_true() -> bool {
    true
}
fn default_profile_guidance() -> f64 {
    0.75
}

impl Default for VoicingControls {
    fn default() -> Self {
        Self {
            character_target: CharacterTarget::Keep,
            signal_path: SignalPathStyle::Default,
            source_variant: SourceCharacterVariant::Standard,
            straighten_mode: StraightenMode::Off,
            straighten_preserve_bends: true,
            profile_guidance_strength: 0.75,
            noise_reduction_override: None,
            smooth_hf_override: None,
        }
    }
}

impl VoicingControls {
    pub fn is_neutral_default(&self) -> bool {
        self.character_target == CharacterTarget::Keep
            && self.signal_path == SignalPathStyle::Default
            && self.source_variant == SourceCharacterVariant::Standard
            && self.straighten_mode == StraightenMode::Off
            && self.noise_reduction_override.is_none()
            && self.smooth_hf_override.is_none()
    }
}

/// Mild, bend-preserving pitch stabilization for approximate or wonky material (e.g. AI stems).
/// Reduces alternating frame-to-frame pitch wobble without forcing MIDI quantization or erasing
/// intentional smooth bends/slides/vibrato when `preserve_bends` is enabled.
fn stabilize_note_pitch_mild(hz: &[f64], mode: StraightenMode, preserve_bends: bool) -> Vec<f64> {
    let n = hz.len();
    if n < 4 || mode == StraightenMode::Off {
        return hz.to_vec();
    }
    let mut sorted = hz.to_vec();
    sorted.sort_by(f64::total_cmp);
    let p15 = sorted[n / 7];
    let p85 = sorted[(n * 6) / 7];
    let macro_span_st = (12.0 * (p85 / p15.max(1e-6)).log2()).abs();
    let median_hz = sorted[n / 2];

    // If preserve_bends is true and the note contains an intentional bend/slide (> 1.1 semitones),
    // use local moving-window smoothing ONLY to remove micro-jitter while preserving the full bend curve!
    let bend_note = preserve_bends && macro_span_st > 1.1;
    let pull = match mode {
        StraightenMode::Off => 0.0,
        StraightenMode::Light => 0.35,
        StraightenMode::Moderate => 0.65,
    };

    let mut out = hz.to_vec();
    for i in 1..(n - 1) {
        let local_avg = (hz[i - 1] + hz[i] + hz[i + 1]) / 3.0;
        if bend_note {
            // Preserve the macroscopic bend while damping alternating frame jitter
            let d1 = hz[i] - hz[i - 1];
            let d2 = hz[i + 1] - hz[i];
            if d1 * d2 < 0.0 {
                out[i] = hz[i] + (local_avg - hz[i]) * (pull * 0.75);
            }
        } else {
            // Steady/wobbly note: gently pull toward local smoothing + median pitch center
            let blended_target = 0.55 * local_avg + 0.45 * median_hz;
            out[i] = hz[i] + (blended_target - hz[i]) * pull;
        }
    }
    out
}

fn apply_character_and_path_conditioning(buf: &mut [f64], rate: f64, voicing: &VoicingControls) {
    let guide = voicing.profile_guidance_strength.clamp(0.0, 1.0);
    if guide <= 0.02 {
        return;
    }
    match voicing.character_target {
        CharacterTarget::Keep => {}
        CharacterTarget::Natural => {
            apply_biquad_peaking(buf, rate, 220.0, 0.9, 1.1 * guide);
            apply_biquad_peaking(buf, rate, 5200.0, 0.8, 1.2 * guide);
        }
        CharacterTarget::Analog => {
            apply_biquad_peaking(buf, rate, 150.0, 0.9, 2.4 * guide);
            apply_biquad_peaking(buf, rate, 3200.0, 1.0, -1.2 * guide);
            apply_biquad_lowpass(buf, rate, 11_500.0, 0.707);
            // Subtle warm soft saturation (calibrated so peak headroom stays clean)
            let drive = 1.0 + 0.9 * guide;
            let inv_tanh = 1.0 / drive.tanh();
            for s in buf.iter_mut() {
                let x = *s;
                // Gentle asymmetric 2nd + 3rd harmonic analog warmth
                let sat = (drive * (x + 0.06 * x * x)).tanh() * inv_tanh;
                *s = x * (1.0 - 0.55 * guide) + sat * (0.55 * guide);
            }
        }
        CharacterTarget::Digital => {
            apply_biquad_peaking(buf, rate, 260.0, 1.0, -0.8 * guide);
            apply_biquad_peaking(buf, rate, 6500.0, 0.9, 1.5 * guide);
        }
    }
    match voicing.signal_path {
        SignalPathStyle::Default => {}
        SignalPathStyle::Miced => {
            apply_biquad_peaking(buf, rate, 190.0, 1.0, 1.6 * guide);
            apply_biquad_peaking(buf, rate, 4200.0, 1.1, 1.4 * guide);
        }
        SignalPathStyle::PickupDirect => {
            apply_biquad_peaking(buf, rate, 1100.0, 1.2, 2.2 * guide);
            apply_biquad_lowpass(buf, rate, 5800.0, 0.75);
        }
        SignalPathStyle::AnalogPreamp => {
            apply_biquad_peaking(buf, rate, 130.0, 0.9, 2.6 * guide);
            let drive = 1.0 + 0.75 * guide;
            let inv_tanh = 1.0 / drive.tanh();
            for s in buf.iter_mut() {
                *s = (*s * drive).tanh() * inv_tanh;
            }
        }
        SignalPathStyle::CleanDigital => {
            apply_biquad_peaking(buf, rate, 400.0, 0.9, -0.6 * guide);
            apply_biquad_peaking(buf, rate, 8000.0, 0.8, 1.0 * guide);
        }
    }
    match voicing.source_variant {
        SourceCharacterVariant::Standard => {}
        SourceCharacterVariant::Rich => {
            apply_biquad_peaking(buf, rate, 210.0, 0.9, 2.2 * guide);
            apply_biquad_peaking(buf, rate, 2600.0, 1.1, 1.6 * guide);
        }
        SourceCharacterVariant::Basic => {
            apply_biquad_peaking(buf, rate, 180.0, 1.0, -1.2 * guide);
            apply_biquad_lowpass(buf, rate, 7000.0, 0.707);
        }
        SourceCharacterVariant::Warm => {
            apply_biquad_peaking(buf, rate, 180.0, 0.9, 2.8 * guide);
            apply_biquad_lowpass(buf, rate, 6200.0, 0.707);
        }
        SourceCharacterVariant::Bright => {
            apply_biquad_peaking(buf, rate, 3500.0, 1.0, 3.0 * guide);
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SpectralSummary {
    pub peak_dbfs: Option<f64>,
    pub rms_dbfs: Option<f64>,
    pub spectral_centroid_hz: f64,
    pub low_band_pct: f64,
    pub mid_band_pct: f64,
    pub high_band_pct: f64,
}

/// Measure overall spectral centroid and 3-band energy balance (`<350 Hz`, `350–2000 Hz`, `>2000 Hz`).
pub fn measure_spectral_summary(samples: &[f32], rate: u32) -> SpectralSummary {
    let mut peak = 0.0_f64;
    let mut sq_sum = 0.0_f64;
    for &s in samples {
        let v = f64::from(s);
        peak = peak.max(v.abs());
        sq_sum += v * v;
    }
    let rms = (sq_sum / samples.len().max(1) as f64).sqrt();
    let n = 4096_usize;
    let hop = 2048_usize;
    let fft = FftPlanner::<f64>::new().plan_fft_forward(n);
    let win: Vec<f64> = (0..n)
        .map(|i| 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos())
        .collect();
    let mut buf = vec![Complex::new(0.0, 0.0); n];
    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    let mut low = 0.0_f64;
    let mut mid = 0.0_f64;
    let mut high = 0.0_f64;
    let bin_hz = f64::from(rate) / n as f64;
    let mut pos = 0;
    while pos + n <= samples.len() || pos == 0 {
        for i in 0..n {
            let x = samples.get(pos + i).copied().unwrap_or(0.0) as f64;
            buf[i] = Complex::new(x * win[i], 0.0);
        }
        fft.process(&mut buf);
        for (k, c) in buf.iter().enumerate().take(n / 2).skip(1) {
            let hz = k as f64 * bin_hz;
            let p = c.norm_sqr();
            num += hz * p;
            den += p;
            if hz < 350.0 {
                low += p;
            } else if hz < 2000.0 {
                mid += p;
            } else {
                high += p;
            }
        }
        if pos + n >= samples.len() {
            break;
        }
        pos += hop;
    }
    let total = (low + mid + high).max(1e-30);
    SpectralSummary {
        peak_dbfs: (peak > 1e-12).then(|| 20.0 * peak.log10()),
        rms_dbfs: (rms > 1e-12).then(|| 20.0 * rms.log10()),
        spectral_centroid_hz: if den > 1e-30 { num / den } else { 0.0 },
        low_band_pct: 100.0 * low / total,
        mid_band_pct: 100.0 * mid / total,
        high_band_pct: 100.0 * high / total,
    }
}

#[cfg(test)]
mod source_regen_tests {
    use super::*;
    use crate::studio_catalog::source_catalog;

    /// Estimate fundamental frequency in a mono audio slice via normalized autocorrelation.
    fn estimate_f0_autocorr(slice: &[f32], rate: u32, min_hz: f64, max_hz: f64) -> f64 {
        let min_lag = (f64::from(rate) / max_hz).floor().max(2.0) as usize;
        let max_lag = (f64::from(rate) / min_hz)
            .ceil()
            .min((slice.len() / 2) as f64) as usize;
        let mut best_lag = min_lag;
        let mut best_corr = f64::NEG_INFINITY;
        for lag in min_lag..=max_lag {
            let mut num = 0.0_f64;
            let mut d1 = 0.0_f64;
            let mut d2 = 0.0_f64;
            for i in 0..(slice.len() - lag) {
                let a = f64::from(slice[i]);
                let b = f64::from(slice[i + lag]);
                num += a * b;
                d1 += a * a;
                d2 += b * b;
            }
            let corr = num / (d1 * d2).sqrt().max(1e-12);
            if corr > best_corr {
                best_corr = corr;
                best_lag = lag;
            }
        }
        f64::from(rate) / best_lag as f64
    }

    #[test]
    fn source_selection_changes_timbre_while_preserving_notes_timing_and_out_of_range_cues() {
        let sources = source_catalog();
        assert_eq!(sources.len(), 138);
        let cello = sources
            .iter()
            .find(|s| s.name == "Cello")
            .expect("Cello present in 138-source catalog");
        let soprano = sources
            .iter()
            .find(|s| s.name == "Female Soprano Voice")
            .expect("Female Soprano Voice present in 138-source catalog");

        // Verify Cello has real published range & spectral regions
        assert_eq!(cello.published_range_hz, Some((65.0, 880.0)));
        assert!(cello.spectral_regions.body_warmth_hz.is_some());
        assert!(!cello.source_references.is_empty());
        assert!(!cello.uncertainties.is_empty());

        // Construct a 3-note performance containing:
        // Note 0: 220 Hz (A3 — inside Cello range, BELOW Female Soprano published 262–1047 Hz!)
        // Note 1: 440 Hz (A4 — inside both)
        // Note 2: 1400 Hz (F6 — ABOVE Cello published 65–880 Hz AND above Soprano 1047 Hz!)
        let rate = 48_000_u32;
        let hop_s = 0.005_f64;
        let make_note = |start: f64, dur: f64, hz: f64| Note {
            string_id: None,
            re_picks: None,
            start,
            end: start + dur,
            hz: vec![hz; (dur / hop_s).round() as usize + 1],
            level_steps: vec![0.28; (dur / hop_s).round() as usize + 1],
            _accent: None,
            shape: Shape::default(),
        };
        let perf = Performance {
            rate,
            samples: (rate as usize) * 2,
            hop_s,
            notes: vec![
                make_note(0.05, 0.50, 220.0),
                make_note(0.65, 0.50, 440.0),
                make_note(1.25, 0.50, 1400.0),
            ],
            frame_evidence: vec![],
        };

        let y_cello = perf
            .synthesize_dry_for_source(cello)
            .expect("cello dry render");
        let y_soprano = perf
            .synthesize_dry_for_source(soprano)
            .expect("soprano dry render");

        assert_eq!(y_cello.len(), perf.samples);
        assert_eq!(y_soprano.len(), perf.samples);
        assert_ne!(
            y_cello, y_soprano,
            "different source selections must produce different timbre waveforms"
        );

        // 1. Confirm timbre / spectral character shifted clearly (Soprano brighter / higher centroid than Cello)
        let s_cello = measure_spectral_summary(&y_cello, rate);
        let s_soprano = measure_spectral_summary(&y_soprano, rate);
        assert!(
            s_soprano.spectral_centroid_hz > s_cello.spectral_centroid_hz + 150.0,
            "expected Soprano centroid ({:.1} Hz) > Cello centroid ({:.1} Hz)",
            s_soprano.spectral_centroid_hz,
            s_cello.spectral_centroid_hz
        );
        assert!(
            s_cello.low_band_pct > s_soprano.low_band_pct + 5.0,
            "expected Cello low-band warmth ({:.1}%) > Soprano ({:.1}%)",
            s_cello.low_band_pct,
            s_soprano.low_band_pct
        );

        // 2. Confirm EVERY note (including out-of-range 220 Hz on Soprano & 1400 Hz on Cello!)
        //    preserves exact timing and exact fundamental pitch (never clamped or transposed).
        let windows = [
            (0.18, 0.35, 220.0, 180.0, 270.0),
            (0.78, 0.95, 440.0, 380.0, 520.0),
            (1.38, 1.55, 1400.0, 1200.0, 1650.0),
        ];
        for (t0, t1, expected_hz, lo_hz, hi_hz) in windows {
            let a = (t0 * rate as f64) as usize;
            let b = (t1 * rate as f64) as usize;
            let rms_c = (y_cello[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt();
            let rms_s =
                (y_soprano[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt();
            assert!(
                rms_c > 0.01 && rms_s > 0.01,
                "note at {expected_hz} Hz must have audible energy in both Cello ({rms_c}) and Soprano ({rms_s})"
            );
            let f0_c = estimate_f0_autocorr(&y_cello[a..b], rate, lo_hz, hi_hz);
            let f0_s = estimate_f0_autocorr(&y_soprano[a..b], rate, lo_hz, hi_hz);
            let cents_c = (1200.0 * (f0_c / expected_hz).log2()).abs();
            let cents_s = (1200.0 * (f0_s / expected_hz).log2()).abs();
            assert!(
                cents_c < 25.0 && cents_s < 25.0,
                "pitch at {expected_hz} Hz drifted! cello={f0_c:.1} Hz ({cents_c:.1}c), soprano={f0_s:.1} Hz ({cents_s:.1}c)"
            );
        }
    }

    #[test]
    fn shape_prompt_end_to_end_synthesizes_from_cues_plus_capture_without_original_samples() {
        use crate::{capture, studio_catalog};
        let rate = 48_000_u32;
        let sources = studio_catalog::source_catalog();
        let mics = studio_catalog::mic_catalog();
        let surfaces = studio_catalog::surface_catalog();

        let cello_idx = sources
            .iter()
            .position(|s| s.name == "Cello")
            .expect("Cello present in source catalog");
        let soprano_idx = sources
            .iter()
            .position(|s| s.name == "Female Soprano Voice")
            .expect("Female Soprano Voice present in source catalog");
        let mkh_idx = mics
            .iter()
            .position(|m| m.model.contains("MKH 50"))
            .unwrap_or(1);

        // Fixed Performance shape (no original WAV passed to synthesis or capture)
        let perf = Performance {
            rate,
            samples: rate as usize,
            hop_s: 0.005,
            notes: vec![Note {
                string_id: None,
                re_picks: None,
                start: 0.08,
                end: 0.72,
                hz: vec![220.0; 129],
                level_steps: vec![0.40; 129],
                _accent: None,
                shape: Shape::default(),
            }],
            frame_evidence: vec![],
        };

        let render_prompt = |rig: &studio_catalog::StudioRig| -> Vec<f32> {
            let dry = perf
                .synthesize_dry_for_source(&sources[rig.source_index])
                .expect("shape-guided dry synthesis");
            capture::render_studio_rig(&dry, rate, rig, &sources, &mics, &surfaces)
        };

        let mut base_rig = studio_catalog::StudioRig::default();
        base_rig.source_index = cello_idx;
        let out_base = render_prompt(&base_rig);

        // 1. Changing Source updates prompt output
        let mut src_rig = base_rig.clone();
        src_rig.source_index = soprano_idx;
        assert_ne!(
            out_base,
            render_prompt(&src_rig),
            "Source change must update signal"
        );

        // 2. Changing Mic updates prompt output
        let mut mic_rig = base_rig.clone();
        mic_rig.mics[0].catalog_index = mkh_idx;
        assert_ne!(
            out_base,
            render_prompt(&mic_rig),
            "Mic change must update signal"
        );

        // 3. Changing Materials updates prompt output
        let mut mat_rig = base_rig.clone();
        mat_rig.surface_faces[0] = 28; // Sheet steel
        assert_ne!(
            out_base,
            render_prompt(&mat_rig),
            "Material change must update signal"
        );

        // 4. Changing Room dimensions updates prompt output
        let mut room_rig = base_rig.clone();
        room_rig.dimensions_m = [8.0, 7.0, 4.0];
        assert_ne!(
            out_base,
            render_prompt(&room_rig),
            "Room change must update signal"
        );

        // 5. Display-only unit & overlay changes leave prompt output bit-identical
        let mut unit_rig = base_rig.clone();
        unit_rig.unit = studio_catalog::Unit::Centimetres;
        unit_rig.show_patterns = !base_rig.show_patterns;
        assert_eq!(
            out_base,
            render_prompt(&unit_rig),
            "Display-only changes must keep audio bit-identical"
        );
    }

    #[test]
    fn source_physics_constrains_voicing_without_overriding_bends_atypical_decay_or_uncertainty() {
        let sources = source_catalog();
        let piano = sources
            .iter()
            .find(|s| s.name == "Concert Grand Piano (Lid Open)")
            .expect("Concert Grand Piano (Lid Open) present");
        let bass = sources
            .iter()
            .find(|s| s.name == "Bass Guitar (4-String)")
            .expect("Bass Guitar (4-String) present");
        let soprano = sources
            .iter()
            .find(|s| s.name == "Female Soprano Voice")
            .expect("Female Soprano Voice present");
        let percussion = sources
            .iter()
            .find(|s| {
                s.regen_model.family_kind == crate::studio_catalog::SourceRegenKind::Percussion
            })
            .expect("Percussion source present");

        // Confirm physics constraint metadata is consistent & non-empty across families
        assert_eq!(
            bass.regen_model
                .family_kind
                .physics_constraint()
                .harmonic_ceiling_hz,
            Some(3800.0)
        );
        assert!(
            soprano
                .regen_model
                .family_kind
                .physics_constraint()
                .breath_texture_amount
                > 0.0
        );

        let rate = 48_000_u32;
        let hop_s = 0.005_f64;
        let dur = 1.0_f64;
        let frames = (dur / hop_s).round() as usize + 1;

        // Note 0 (0.05..1.05 s): atypical continuous +3 semitone bend (220 Hz -> 261.63 Hz)
        // AND atypical crescendo / swelling sustain envelope (0.12 -> 0.48),
        // followed by 0.65 s of uncertain/silent span (1.15..1.80 s) with NO supported note.
        let hz: Vec<f64> = (0..frames)
            .map(|i| {
                let u = i as f64 / (frames - 1) as f64;
                220.0 * 2.0_f64.powf(3.0 * u / 12.0)
            })
            .collect();
        let level_steps: Vec<f64> = (0..frames)
            .map(|i| {
                let u = i as f64 / (frames - 1) as f64;
                0.12 + 0.36 * u
            })
            .collect();

        let total_samples = (rate as usize) * 2;
        let total_ev_frames = (2.0 / hop_s).round() as usize;
        let frame_evidence: Vec<FrameEvidence> = (0..total_ev_frames)
            .map(|i| {
                let t = i as f64 * hop_s;
                let status = if (0.05..=1.05).contains(&t) {
                    EvidenceStatus::Supported
                } else if (1.20..=1.65).contains(&t) {
                    EvidenceStatus::Uncertain
                } else {
                    EvidenceStatus::Silence
                };
                FrameEvidence {
                    time_s: t,
                    level_rms: if status == EvidenceStatus::Supported {
                        0.25
                    } else {
                        0.01
                    },
                    status,
                    selected_hz: if status == EvidenceStatus::Supported {
                        Some(220.0)
                    } else {
                        None
                    },
                    tonal_fraction: if status == EvidenceStatus::Supported {
                        0.88
                    } else {
                        0.15
                    },
                    hypotheses: vec![],
                }
            })
            .collect();

        let perf = Performance {
            rate,
            samples: total_samples,
            hop_s,
            notes: vec![Note {
                string_id: None,
                re_picks: None,
                start: 0.05,
                end: 1.05,
                hz,
                level_steps,
                _accent: None,
                shape: Shape::default(),
            }],
            frame_evidence,
        };

        let y_piano = perf.synthesize_dry_for_source(piano).expect("piano render");
        let y_bass = perf.synthesize_dry_for_source(bass).expect("bass render");
        let y_soprano = perf
            .synthesize_dry_for_source(soprano)
            .expect("soprano render");
        let y_perc = perf
            .synthesize_dry_for_source(percussion)
            .expect("percussion render");

        // 1. Implausible-band constraint: Bass (3.8 kHz lowpass) suppresses high-band energy vs Soprano
        let s_bass = measure_spectral_summary(&y_bass, rate);
        let s_soprano = measure_spectral_summary(&y_soprano, rate);
        assert!(
            s_bass.high_band_pct < s_soprano.high_band_pct,
            "Bass should suppress high-band energy ({:.2}%) vs Soprano ({:.2}%)",
            s_bass.high_band_pct,
            s_soprano.high_band_pct
        );

        // 2. Cue precedence over textbook physics: Acoustic Piano STILL follows the +3 st cue bend!
        let f0_start = estimate_f0_autocorr(
            &y_piano[(0.12 * rate as f64) as usize..(0.24 * rate as f64) as usize],
            rate,
            180.0,
            320.0,
        );
        let f0_end = estimate_f0_autocorr(
            &y_piano[(0.84 * rate as f64) as usize..(0.96 * rate as f64) as usize],
            rate,
            180.0,
            320.0,
        );
        let bend_cents = 1200.0 * (f0_end / f0_start).log2();
        assert!(
            bend_cents > 160.0 && bend_cents < 320.0,
            "Piano must still follow cue bend instead of snapping to equal temperament (got {bend_cents:.1}c: {f0_start:.1} -> {f0_end:.1} Hz)"
        );

        // 3. Atypical swelling sustain on Percussion still follows cue envelope growth rather than dying immediately
        let rms_window = |buf: &[f32], t0: f64, t1: f64| -> f32 {
            let a = (t0 * rate as f64) as usize;
            let b = (t1 * rate as f64) as usize;
            (buf[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt()
        };
        let perc_early = rms_window(&y_perc, 0.12, 0.25);
        let perc_late = rms_window(&y_perc, 0.78, 0.92);
        assert!(
            perc_late > perc_early * 0.85 && perc_late > 0.02,
            "Percussion must respect atypical swelling cue sustain (early={perc_early:.4}, late={perc_late:.4})"
        );

        // 4. Uncertain & silent spans NEVER invent synthetic notes across any source model
        let u0 = (1.25 * rate as f64) as usize;
        let u1 = (1.60 * rate as f64) as usize;
        for (label, buf) in [
            ("piano", &y_piano),
            ("bass", &y_bass),
            ("soprano", &y_soprano),
            ("percussion", &y_perc),
        ] {
            let peak = buf[u0..u1].iter().fold(0.0_f32, |m, &v| m.max(v.abs()));
            assert!(
                peak < 1e-4,
                "{label} must not invent tones during uncertain/unsupported frames (peak={peak})"
            );
        }
    }

    #[test]
    fn ddsp_voicing_character_path_and_bend_preserving_straightening() {
        let sources = source_catalog();
        let violin = sources
            .iter()
            .find(|s| s.name.contains("Violin"))
            .unwrap_or(&sources[1]);
        let rate = 48_000_u32;
        let hop_s = 0.005_f64;
        let frames = 121;
        // Note 0: wobbly steady pitch around 440 Hz (+/- 20 cents jitter)
        let wobbly_hz: Vec<f64> = (0..frames)
            .map(|i| 440.0 * if i % 2 == 0 { 1.012 } else { 0.988 })
            .collect();
        // Note 1: intentional +3 semitone bend (330 -> 392.4 Hz)
        let bend_hz: Vec<f64> = (0..frames)
            .map(|i| 330.0 * 2.0_f64.powf(3.0 * (i as f64 / (frames - 1) as f64) / 12.0))
            .collect();
        let perf = Performance {
            rate,
            samples: (rate as usize) * 2,
            hop_s,
            notes: vec![
                Note {
                    string_id: None,
                    re_picks: None,
                    start: 0.05,
                    end: 0.65,
                    hz: wobbly_hz.clone(),
                    level_steps: vec![0.35; frames],
                    _accent: None,
                    shape: Shape::default(),
                },
                Note {
                    string_id: None,
                    re_picks: None,
                    start: 0.95,
                    end: 1.55,
                    hz: bend_hz.clone(),
                    level_steps: vec![0.35; frames],
                    _accent: None,
                    shape: Shape::default(),
                },
            ],
            frame_evidence: vec![],
        };

        // 1. Character targets produce distinct waveforms (Natural vs Analog vs Digital)
        let y_nat = perf
            .synthesize_dry_for_source_with_voicing(
                violin,
                &VoicingControls {
                    character_target: CharacterTarget::Natural,
                    ..VoicingControls::default()
                },
            )
            .unwrap();
        let y_ana = perf
            .synthesize_dry_for_source_with_voicing(
                violin,
                &VoicingControls {
                    character_target: CharacterTarget::Analog,
                    ..VoicingControls::default()
                },
            )
            .unwrap();
        let y_dig = perf
            .synthesize_dry_for_source_with_voicing(
                violin,
                &VoicingControls {
                    character_target: CharacterTarget::Digital,
                    ..VoicingControls::default()
                },
            )
            .unwrap();
        assert_ne!(y_nat, y_ana);
        assert_ne!(y_ana, y_dig);
        assert_ne!(y_nat, y_dig);

        // 2. Mild stabilization reduces steady-note wobble while preserving intentional +3 st bend!
        let stab_wobble = stabilize_note_pitch_mild(&wobbly_hz, StraightenMode::Moderate, true);
        let raw_std =
            (wobbly_hz.iter().map(|&f| (f - 440.0).powi(2)).sum::<f64>() / frames as f64).sqrt();
        let stab_std = (stab_wobble
            .iter()
            .map(|&f| (f - 440.0).powi(2))
            .sum::<f64>()
            / frames as f64)
            .sqrt();
        assert!(
            stab_std < raw_std * 0.5,
            "stabilization must reduce steady-note wobble ({stab_std:.2} vs {raw_std:.2})"
        );

        let stab_bend = stabilize_note_pitch_mild(&bend_hz, StraightenMode::Moderate, true);
        let bend_span_st = 12.0 * (stab_bend[frames - 1] / stab_bend[0]).log2();
        assert!(
            (bend_span_st - 3.0).abs() < 0.15,
            "intentional bend must be preserved under bend-preserving straightening (got {bend_span_st:.2} st)"
        );
    }

    #[test]
    fn controlled_single_string_pure_bend_vs_additive_repick_and_polyphony() {
        let rate = 48_000_u32;
        let hop_s = 0.005_f64;
        let frames = 161; // 0.80 s
        let bend_hz: Vec<f64> = (0..frames)
            .map(|i| 220.0 * 2.0_f64.powf(2.0 * (i as f64 / (frames - 1) as f64) / 12.0))
            .collect();

        let pure_bend = Performance {
            rate,
            samples: rate as usize,
            hop_s,
            notes: vec![Note {
                string_id: Some(0),
                re_picks: None,
                start: 0.10,
                end: 0.90,
                hz: bend_hz.clone(),
                level_steps: vec![0.18; frames],
                _accent: None,
                shape: Shape::default(),
            }],
            frame_evidence: vec![],
        };
        let repicked_bend = Performance {
            rate,
            samples: rate as usize,
            hop_s,
            notes: vec![Note {
                string_id: Some(0),
                re_picks: Some(vec![Strike {
                    offset_s: 0.30,
                    strength: 0.55,
                }]),
                start: 0.10,
                end: 0.90,
                hz: bend_hz,
                level_steps: vec![0.18; frames],
                _accent: None,
                shape: Shape::default(),
            }],
            frame_evidence: vec![],
        };

        let y_bend = pure_bend.synthesize_dry().unwrap();
        let y_repick = repicked_bend.synthesize_dry().unwrap();
        let strike_sample = ((0.10 + 0.30) * f64::from(rate)).round() as usize; // 19200
        assert_eq!(y_bend[..=strike_sample], y_repick[..=strike_sample]);
        assert!(y_bend[..4800].iter().all(|&v| v == 0.0));
        assert!(y_bend[43200..].iter().all(|&v| v == 0.0));
        let e_bend: f64 = y_bend[strike_sample + 1..strike_sample + 4800]
            .iter()
            .map(|&v| f64::from(v) * f64::from(v))
            .sum();
        let e_repick: f64 = y_repick[strike_sample + 1..strike_sample + 4800]
            .iter()
            .map(|&v| f64::from(v) * f64::from(v))
            .sum();
        assert!(e_repick > e_bend * 1.15);

        // Basic polyphony: different string_ids render independently and sum linearly.
        let note_s0 = Note {
            string_id: Some(0),
            re_picks: None,
            start: 0.10,
            end: 0.60,
            hz: vec![220.0; 101],
            level_steps: vec![0.14; 101],
            _accent: None,
            shape: Shape::default(),
        };
        let note_s1 = Note {
            string_id: Some(1),
            re_picks: None,
            start: 0.25,
            end: 0.75,
            hz: vec![329.63; 101],
            level_steps: vec![0.14; 101],
            _accent: None,
            shape: Shape::default(),
        };
        let p0 = Performance {
            rate,
            samples: rate as usize,
            hop_s,
            notes: vec![note_s0.clone()],
            frame_evidence: vec![],
        }
        .synthesize_dry()
        .unwrap();
        let p1 = Performance {
            rate,
            samples: rate as usize,
            hop_s,
            notes: vec![note_s1.clone()],
            frame_evidence: vec![],
        }
        .synthesize_dry()
        .unwrap();
        let poly = Performance {
            rate,
            samples: rate as usize,
            hop_s,
            notes: vec![note_s0.clone(), note_s1],
            frame_evidence: vec![],
        }
        .synthesize_dry()
        .unwrap();
        for i in 0..poly.len() {
            assert!((poly[i] - (p0[i] + p1[i])).abs() < 1e-6);
        }

        // Shared string_id carries persistent vibration state across sequential notes without reset.
        let shared_second = Note {
            string_id: Some(0),
            re_picks: None,
            start: 0.60,
            end: 0.90,
            hz: vec![246.94; 61],
            level_steps: vec![0.14; 61],
            _accent: None,
            shape: Shape::default(),
        };
        let shared_out = Performance {
            rate,
            samples: rate as usize,
            hop_s,
            notes: vec![note_s0, shared_second.clone()],
            frame_evidence: vec![],
        }
        .synthesize_dry()
        .unwrap();
        let fresh_second_out = Performance {
            rate,
            samples: rate as usize,
            hop_s,
            notes: vec![shared_second],
            frame_evidence: vec![],
        }
        .synthesize_dry()
        .unwrap();
        assert_ne!(shared_out[28_800..33_600], fresh_second_out[28_800..33_600]);

        // Optional simple body resonance produces finite colored dry output.
        let body_out =
            pure_bend
                .synthesize_dry_with_body_resonance(
                    &crate::string_state::BodyResonanceConfig::default(),
                )
                .unwrap();
        assert_eq!(body_out.len(), y_bend.len());
        assert!(body_out.iter().all(|v| v.is_finite()));
        assert_ne!(body_out, y_bend);
    }

    #[test]
    fn procedural_noise_is_centered_deterministic_and_finite() {
        let mut s1 = 0x51a7_9911_u64;
        let mut s2 = 0x51a7_9911_u64;
        let n = 100_000_usize;
        let mut pos = 0usize;
        let mut neg = 0usize;
        let mut sum = 0.0_f64;
        for _ in 0..n {
            let a = next_centered_white_noise(&mut s1);
            let b = next_centered_white_noise(&mut s2);
            assert_eq!(a, b, "noise generator must be deterministic for same seed");
            assert!(a.is_finite() && (-1.0..=1.0).contains(&a), "out of bounds: {a}");
            if a > 0.0 {
                pos += 1;
            } else if a < 0.0 {
                neg += 1;
            }
            sum += a;
        }
        let mean = sum / n as f64;
        assert!(pos > 45_000 && neg > 45_000, "expected both polarities, pos={pos} neg={neg}");
        assert!(mean.abs() < 0.015, "expected zero-centered mean, got {mean}");
    }
}

/// Deterministic zero-centered white noise sample in `[-1.0, +1.0]` using the upper 32 bits
/// of a 64-bit LCG (fixes 31-bit shift negative-only bias).
#[inline]
pub(crate) fn next_centered_white_noise(noise_state: &mut u64) -> f64 {
    *noise_state = noise_state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let u = (*noise_state >> 32) as u32;
    (f64::from(u) / f64::from(u32::MAX)) * 2.0 - 1.0
}

