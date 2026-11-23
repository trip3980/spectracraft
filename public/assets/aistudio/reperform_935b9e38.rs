//! Dry rendering from performance cues, separate from legacy FFT-partial resynthesis.
use crate::{valid_rate, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    f64::consts::{PI, TAU},
    path::Path,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Performance {
    rate: u32,
    samples: usize,
    hop_s: f64,
    notes: Vec<Note>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Note {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    re_picks: Option<Vec<Strike>>,
    start: f64,
    end: f64,
    hz: Vec<f64>,
    level_steps: Vec<f64>,
    #[serde(default, rename = "accent")]
    _accent: Option<f64>,
    #[serde(default)]
    shape: Shape,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Strike {
    offset_s: f64,
    strength: f64,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
struct Shape {
    mode_weights: BTreeMap<usize, f64>,
    decay_per_second: BTreeMap<usize, f64>,
}

fn interp(x: &[f64], t: f64) -> f64 {
    let i = t as usize;
    if i >= x.len() - 1 {
        return x[x.len() - 1];
    }
    let a = t - i as f64;
    x[i] + a * (x[i + 1] - x[i])
}

fn smooth(x: &[f64]) -> Vec<f64> {
    let kernel: Vec<_> = (-8_i32..=8)
        .map(|k| (-f64::from(k * k) / 8.).exp())
        .collect();
    let sum = kernel.iter().sum::<f64>();
    let len = x.len() as isize;
    (0..x.len())
        .map(|i| {
            kernel
                .iter()
                .enumerate()
                .map(|(j, w)| {
                    let k = (i as isize + j as isize - 8).rem_euclid(2 * len);
                    w * x[if k < len { k } else { 2 * len - 1 - k } as usize]
                })
                .sum::<f64>()
                / sum
        })
        .collect()
}

impl Performance {
    pub fn synthesize_dry(&self) -> Result<Vec<f32>> {
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
        let mut total = 0;
        for note in &self.notes {
            if !(0. ..=self.samples as f64 / rate).contains(&note.start)
                || !note.end.is_finite()
                || note.end <= note.start
                || note.end > self.samples as f64 / rate + self.hop_s
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
                return Err("Invalid dry guitar event".into());
            }
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
            let pitches: Vec<_> = (0..count)
                .map(|i| interp(&note.hz, (i as f64 / rate) / self.hop_s))
                .collect();
            let mut sorted = pitches.clone();
            sorted.sort_by(f64::total_cmp);
            let n = pitches.len();
            let median = (sorted[n / 2] + sorted[(n - 1) / 2]) * 0.5;
            let fret = (median / 330.).clamp(1., 2.7);
            let pick = (0.175 * fret).clamp(0.16, 0.36);
            let pickup = (0.145 * fret).clamp(0.11, 0.275);
            let levels = smooth(&note.level_steps);
            let modes: Vec<_> = (1..20)
                .map(|k| {
                    let h = k as f64;
                    let sinc = PI * 0.029 * h;
                    let w = (PI * h * pick).sin() * (PI * h * pickup).sin() * sinc.sin()
                        / sinc
                        / h.powf(0.91)
                        * note.shape.mode_weights.get(&k).copied().unwrap_or(1.);
                    let d = note
                        .shape
                        .decay_per_second
                        .get(&k)
                        .copied()
                        .unwrap_or(0.71 + 0.39 * h.powf(0.84));
                    (h, w, d)
                })
                .collect();
            let mut modes_state: Vec<_> = modes
                .iter()
                .map(|&(h, w, d)| crate::string_state::Mode::new(h, w, d, rate))
                .collect();
            let mut kicks = Vec::new();
            if let Some(strikes) = &note.re_picks {
                if strikes.len() > 1024 {
                    return Err("Too many re-picks in one event".into());
                }
                kicks = vec![0.; count];
                kicks[0] = 1.;
                for strike in strikes {
                    if !(0. ..=(count - 1) as f64 / rate).contains(&strike.offset_s)
                        || !(0. ..=1.5).contains(&strike.strength)
                    {
                        return Err("Invalid re-pick".into());
                    }
                    kicks[(strike.offset_s * rate).round() as usize] += strike.strength;
                }
            }
            let mut cycles = 0.;
            for (i, sample) in y[start..end].iter_mut().enumerate() {
                let t = i as f64 / rate;
                let f = pitches[i];
                cycles += f;
                let phase = TAU * cycles / rate;
                let envelope =
                    (t / 0.008).min(1.) * (((count - 1 - i) as f64 / rate) / 0.018).min(1.);
                let modal_sum = if kicks.is_empty() {
                    modes
                        .iter()
                        .filter(|(h, _, _)| h * f < rate / 2.)
                        .map(|(h, w, d)| w * (h * phase).sin() * (-d * t).exp())
                        .sum::<f64>()
                } else {
                    modes_state
                        .iter_mut()
                        .map(|m| m.advance(f, rate, kicks[i]))
                        .sum::<f64>()
                };
                *sample += 1.53 * interp(&levels, t / self.hop_s) * envelope * modal_sum;
            }
        }
        Ok(y.into_iter().map(|x| x as f32).collect())
    }

    pub fn write_dry_wav(&self, path: &Path) -> Result<()> {
        let y = self.synthesize_dry()?;
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
                sample_rate: self.rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for sample in y {
            out.write_sample(sample)?;
        }
        out.finalize()?;
        Ok(())
    }
}

/// Source-audio analysis for a predominantly monophonic guitar performance.
/// Cue packets contain only time, pitch and relative intensity, not source PCM.
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
    const LAG: usize = 156;
    let mut s = Vec::with_capacity((input.len() * 12000 / rate as usize) + 1);
    let ratio = f64::from(rate) / FS;
    let cutoff = 0.42 / ratio.max(1.);
    let mut smooth_dc = 0.;
    let hp = 1. - (-TAU * 85. / FS).exp();
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
    let mut levels = Vec::with_capacity(frame_count);
    let mut options = Vec::with_capacity(frame_count);
    for i in 0..frame_count {
        let c = i * H;
        let get = |j: isize| s.get((c as isize + j) as usize).copied().unwrap_or(0.);
        levels.push(((-60..60).map(|j| get(j).powi(2)).sum::<f64>() / 120.).sqrt());
        let mut frame = [0.; WIN];
        for (j, x) in frame.iter_mut().enumerate() {
            *x = get(j as isize - WIN as isize / 2);
        }
        let mean = frame.iter().sum::<f64>() / WIN as f64;
        for x in &mut frame {
            *x -= mean;
        }
        let mut prefix = [0.; WIN + 1];
        for (j, x) in frame.iter().enumerate() {
            prefix[j + 1] = prefix[j] + x * x;
        }
        let mut score = [0.; LAG + 1];
        if prefix[WIN] > 1e-9 {
            for lag in 8..=LAG {
                let cross = frame[..WIN - lag]
                    .iter()
                    .zip(&frame[lag..])
                    .map(|(a, b)| a * b)
                    .sum::<f64>();
                score[lag] =
                    2. * cross / (prefix[WIN - lag] + prefix[WIN] - prefix[lag]).max(1e-12);
            }
        }
        let best = score.iter().copied().fold(0., f64::max);
        let candidates: Vec<_> = (10..LAG)
            .filter(|&p| {
                score[p] > 0.57_f64.max(best * 0.88)
                    && score[p] > score[p - 1]
                    && score[p] >= score[p + 1]
            })
            .map(|p| {
                let (a, b, c) = (score[p - 1], score[p], score[p + 1]);
                let d = (a - 2. * b + c).min(-1e-9);
                let f = FS / (p as f64 + (0.5 * (a - c) / d).clamp(-0.5, 0.5));
                (f, b, p)
            })
            .collect();
        options.push(candidates);
    }
    let max = levels.iter().copied().fold(0., f64::max);
    let floor = (max * 0.052).max(0.004);
    let mut f = vec![0.; frame_count];
    let mut prev: f64 = 0.;
    for i in 0..frame_count {
        if levels[i] < floor || options[i].is_empty() {
            prev = 0.;
            continue;
        }
        let cost = |(hz, q, p): (f64, f64, usize)| {
            let distance = if prev > 0. {
                (hz / prev).log2().abs()
            } else {
                0.
            };
            (1. - q) + 0.0012 * p as f64 + 0.21 * distance.min(1.)
        };
        let c = options[i]
            .iter()
            .copied()
            .min_by(|&a, &b| cost(a).total_cmp(&cost(b)))
            .unwrap();
        f[i] = c.0;
        prev = c.0;
    }
    // Fill only brief gaps within a continuing note. Never bridge a rest.
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
        if a > 0 && j < frame_count && j - a <= 4 && (f[a - 1] / f[j]).log2().abs() < 0.13 {
            let (l, r) = (f[a - 1], f[j]);
            for k in a..j {
                f[k] = l + (r - l) * (k - a + 1) as f64 / (j - a + 1) as f64;
            }
        }
    }
    let mut notes = Vec::new();
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
        if i - start < 8 {
            continue;
        }
        let mut cuts = vec![start];
        let mean = |x: &[f64]| x.iter().sum::<f64>() / x.len() as f64;
        for k in start + 12..i.saturating_sub(10) {
            if k - cuts.last().unwrap() < 14 {
                continue;
            }
            let before = mean(&levels[k - 8..k - 2]);
            let after = mean(&levels[k + 2..k + 8]);
            let p = mean(&f[k - 7..k - 2]);
            let q = mean(&f[k + 2..k + 7]);
            let rise = after / before.max(floor * 0.5);
            let step = (p / q).log2().abs();
            if (rise > 1.65 && step > 0.078) || rise > 2.25 {
                cuts.push(k);
            }
        }
        cuts.push(i);
        for bounds in cuts.windows(2) {
            let (a, b) = (bounds[0], bounds[1]);
            let mut l = levels[a..b].to_vec();
            l.sort_by(f64::total_cmp);
            let accent = l[((l.len() - 1) * 73) / 100];
            notes.push(Note {
                re_picks: None,
                start: a as f64 * 0.005,
                end: b as f64 * 0.005,
                hz: f[a..b].to_vec(),
                level_steps: vec![accent; b - a],
                _accent: Some(accent),
                shape: Shape::default(),
            });
        }
    }
    Ok(Performance {
        rate,
        samples: input.len(),
        hop_s: 0.005,
        notes,
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
    use crate::dsp;
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
    })
}
