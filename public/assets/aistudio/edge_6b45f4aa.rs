//! Independent quiet/faint resonance tracks and adaptive harmonic analysis.
use crate::{
    attack_guard, dsp,
    harmonic::{self, Profile, BANDS, HOP, WIN},
    Result,
};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub frames: Vec<usize>,
    pub hz: Vec<f64>,
    pub amplitude: Vec<f64>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub profile: Profile,
    pub resonance: bool,
    pub entry: bool,
    pub chirp: bool,
    pub faint: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            profile: Profile::General,
            resonance: true,
            entry: false,
            chirp: false,
            faint: false,
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub base: harmonic::Packet,
    pub tracks: Vec<Track>,
    pub options: Options,
}
pub struct Stems {
    pub tone: Vec<f32>,
    pub texture: Vec<f32>,
    pub ring: Vec<f32>,
}
impl Stems {
    pub fn mix(&self) -> Vec<f32> {
        (0..self.tone.len())
            .map(|i| self.tone[i] + self.texture[i] + self.ring[i])
            .collect()
    }
}
fn add_observation(
    tracks: &mut Vec<Track>,
    slot: Option<usize>,
    frame: usize,
    hz: f64,
    amp: f64,
) -> usize {
    let j = slot.unwrap_or_else(|| {
        tracks.push(Track {
            frames: Vec::new(),
            hz: Vec::new(),
            amplitude: Vec::new(),
        });
        tracks.len() - 1
    });
    tracks[j].frames.push(frame);
    tracks[j].hz.push(hz);
    tracks[j].amplitude.push(amp);
    j
}
fn quiet_tracks(x: &[f32], p: &harmonic::Packet) -> Vec<Track> {
    let window = dsp::hann(2048);
    let sum: f64 = window.iter().sum();
    let fft = FftPlanner::new().plan_fft_forward(8192);
    let mut z = vec![Complex::new(0., 0.); 8192];
    let df = p.rate as f64 / 8192.;
    let mut tracks: Vec<Track> = Vec::new();
    for frame in 0..p.pitch.len() {
        if !(0.00035..0.020).contains(&p.envelope[frame]) {
            continue;
        }
        z.fill(Complex::new(0., 0.));
        for (i, v) in dsp::centered(x, frame * HOP, 2048).iter().enumerate() {
            z[i].re = v * window[i];
        }
        fft.process(&mut z);
        let a: Vec<_> = z[..4097].iter().map(|v| v.norm() * 2. / sum).collect();
        let mut peaks: Vec<_> = dsp::peaks(&a)
            .into_iter()
            .filter(|&k| {
                k as f64 * df > 125.
                    && k as f64 * df < 1950.
                    && a[k] > 0.00016
                    && a[k] > 2.8 * dsp::median(&a[k.saturating_sub(25)..(k + 26).min(a.len())])
            })
            .collect();
        peaks.sort_by(|&a1, &b| a[b].total_cmp(&a[a1]));
        peaks.truncate(6);
        let mut used = Vec::new();
        for k in peaks {
            let hz = dsp::refined_bin(&a, k) * df;
            let slot = tracks
                .iter()
                .enumerate()
                .filter(|(j, t)| {
                    !used.contains(j)
                        && frame - t.frames.last().unwrap() <= 2
                        && (hz - t.hz.last().unwrap()).abs() < 29.
                })
                .min_by(|(_, a), (_, b)| {
                    (hz - a.hz.last().unwrap())
                        .abs()
                        .total_cmp(&(hz - b.hz.last().unwrap()).abs())
                })
                .map(|(j, _)| j);
            used.push(add_observation(&mut tracks, slot, frame, hz, a[k]));
        }
    }
    tracks
        .into_iter()
        .filter(|t| {
            t.frames.len() >= 8
                && t.hz.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                    - t.hz.iter().copied().fold(f64::INFINITY, f64::min)
                    <= 50.
        })
        .collect()
}
fn faint_tracks(x: &[f32], p: &harmonic::Packet) -> Vec<Track> {
    let n = 2 * (0.032 * p.rate as f64).round() as usize;
    let window: Vec<_> = (0..n)
        .map(|i| {
            let a = TAU * i as f64 / (n - 1) as f64;
            0.35875 - 0.48829 * a.cos() + 0.14128 * (2. * a).cos() - 0.01168 * (3. * a).cos()
        })
        .collect();
    let sum: f64 = window.iter().sum();
    let fft_len = (4 * n).next_power_of_two();
    let df = p.rate as f64 / fft_len as f64;
    let lobe = 4. * p.rate as f64 / n as f64;
    let fft = FftPlanner::new().plan_fft_forward(fft_len);
    let mut z = vec![Complex::new(0., 0.); fft_len];
    let mut tracks: Vec<Track> = Vec::new();
    let mut live: Vec<usize> = Vec::new();
    for frame in (0..p.pitch.len()).step_by(4) {
        live.retain(|&j| frame - tracks[j].frames.last().unwrap() <= 4);
        if !p.voiced[frame] || p.envelope[frame] < 0.006 {
            continue;
        }
        z.fill(Complex::new(0., 0.));
        for (i, v) in dsp::centered(x, frame * HOP, n).iter().enumerate() {
            z[i].re = v * window[i];
        }
        fft.process(&mut z);
        let a: Vec<_> = z[..fft_len / 2 + 1]
            .iter()
            .map(|v| v.norm() * 2. / sum)
            .collect();
        let dominant = a.iter().copied().fold(0., f64::max);
        if dominant <= 0. {
            continue;
        }
        let peaks: Vec<_> = dsp::peaks(&a)
            .into_iter()
            .filter(|&k| {
                a[k] >= dominant * 0.00065
                    && k as f64 * df >= 75.
                    && k as f64 * df <= 5000_f64.min(p.rate as f64 * 0.42)
            })
            .collect();
        let mut strongest = peaks.clone();
        strongest.sort_by(|&i, &j| a[j].total_cmp(&a[i]));
        strongest.truncate(36);
        let mut candidates = Vec::new();
        for k in strongest {
            if a[k] > dominant * 0.23
                || (1..10).any(|h| (k as f64 * df - h as f64 * p.pitch[frame]).abs() < lobe)
            {
                continue;
            }
            let radius = (180. / df).round() as usize;
            let nearby: Vec<_> = (k.saturating_sub(radius)..(k + radius + 1).min(a.len()))
                .filter(|&i| i.abs_diff(k) > (lobe / df).round() as usize)
                .map(|i| a[i])
                .collect();
            if a[k] < 2.8 * dsp::median(&nearby)
                || peaks
                    .iter()
                    .any(|&j| j.abs_diff(k) as f64 * df < lobe && a[j] > 4. * a[k])
            {
                continue;
            }
            candidates.push((dsp::refined_bin(&a, k) * df, a[k]));
        }
        let mut pairs = Vec::new();
        for &j in &live {
            let t = &tracks[j];
            let mut predicted = *t.hz.last().unwrap();
            if t.hz.len() > 1 {
                predicted += (t.hz[t.hz.len() - 1] - t.hz[t.hz.len() - 2]).clamp(-18., 18.);
            }
            for (k, (hz, _)) in candidates.iter().enumerate() {
                let d = (hz - predicted).abs();
                if d <= 12_f64.max(1800. * HOP as f64 * 4. / p.rate as f64) {
                    pairs.push((d, j, k));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut used_tracks = Vec::new();
        let mut used_candidates = Vec::new();
        for (_, j, k) in pairs {
            if used_tracks.contains(&j) || used_candidates.contains(&k) {
                continue;
            }
            add_observation(
                &mut tracks,
                Some(j),
                frame,
                candidates[k].0,
                candidates[k].1,
            );
            used_tracks.push(j);
            used_candidates.push(k);
        }
        for (k, (hz, amp)) in candidates.into_iter().enumerate() {
            if !used_candidates.contains(&k) {
                live.push(add_observation(&mut tracks, None, frame, hz, amp));
            }
        }
    }
    let mut eligible: Vec<_> = tracks
        .into_iter()
        .filter(|t| {
            (t.frames.last().unwrap() - t.frames[0]) as f64 * HOP as f64 / p.rate as f64 >= 0.10
        })
        .collect();
    eligible.sort_by(|a, b| {
        b.amplitude
            .iter()
            .map(|v| v * v)
            .sum::<f64>()
            .total_cmp(&a.amplitude.iter().map(|v| v * v).sum::<f64>())
    });
    eligible.truncate(24);
    eligible
        .into_iter()
        .map(|t| {
            let frames: Vec<_> = (t.frames[0]..=*t.frames.last().unwrap()).collect();
            let mut hz = Vec::new();
            let mut amp = Vec::new();
            let mut j = 0;
            for &i in &frames {
                while j + 1 < t.frames.len() && t.frames[j + 1] < i {
                    j += 1;
                }
                let k = (j + 1).min(t.frames.len() - 1);
                let u = if j == k {
                    0.
                } else {
                    (i - t.frames[j]) as f64 / (t.frames[k] - t.frames[j]) as f64
                };
                hz.push(t.hz[j] + u * (t.hz[k] - t.hz[j]));
                amp.push(t.amplitude[j] + u * (t.amplitude[k] - t.amplitude[j]));
            }
            let m = (frames.len() / 2)
                .min(((0.018 * p.rate as f64 / HOP as f64).round() as usize).max(1));
            for i in 0..m {
                let fade = (std::f64::consts::FRAC_PI_2 * i as f64 / (m - 1).max(1) as f64)
                    .sin()
                    .powi(2);
                amp[i] *= fade;
                let k = amp.len() - 1 - i;
                amp[k] *= fade;
            }
            Track {
                frames,
                hz,
                amplitude: amp,
            }
        })
        .collect()
}
pub fn analyze(x: &[f32], rate: u32, options: Options) -> Result<Packet> {
    let mut base = harmonic::analyze(x, rate, options.profile)?;
    let mut tracks = if options.resonance {
        quiet_tracks(x, &base)
    } else {
        Vec::new()
    };
    let quiet_count = tracks.len();
    if options.faint {
        tracks.extend(faint_tracks(x, &base));
    }
    if tracks.is_empty() && !options.entry && !options.chirp {
        return Ok(Packet {
            base,
            tracks,
            options,
        });
    }
    let window = dsp::hann(WIN);
    let sum: f64 = window.iter().sum();
    let short = dsp::hann(WIN / 2);
    let short_sum: f64 = short.iter().sum();
    let fft = FftPlanner::new().plan_fft_forward(WIN);
    let chirp = options.chirp && base.voiced.iter().any(|v| *v);
    let mut angle = Vec::new();
    if chirp {
        let mut phase = 0.;
        for i in 0..base.pitch.len() * HOP + WIN {
            let t = (i as f64 - WIN as f64 / 2.) / HOP as f64;
            phase += TAU * dsp::interp(&base.pitch, t) / rate as f64;
            angle.push(phase);
        }
    }
    for frame in 0..base.pitch.len() {
        let input = dsp::centered(x, frame * HOP, WIN);
        let mut residual = input.clone();
        let mut distant = vec![true; WIN / 2 + 1];
        let before = base.envelope[frame.saturating_sub(2)];
        let after = base.envelope[(frame + 2).min(base.envelope.len() - 1)];
        let blend = if options.entry {
            (((after - before) / base.envelope[frame].max(0.0015) - 0.15).clamp(0., 0.65) / 0.65)
                * ((0.14 - base.envelope[frame]) / 0.05).clamp(0., 1.)
        } else {
            0.
        };
        for h in 1..10 {
            let rotation: Vec<_> = (0..WIN)
                .map(|i| {
                    let phase = if chirp {
                        h as f64 * (angle[frame * HOP + i] - angle[frame * HOP + WIN / 2])
                    } else {
                        TAU * h as f64 * base.pitch[frame] * (i as f64 - WIN as f64 / 2.)
                            / rate as f64
                    };
                    Complex::from_polar(1., phase)
                })
                .collect();
            let mut coefficient = input
                .iter()
                .zip(&window)
                .zip(&rotation)
                .map(|((v, w), r)| v * w * r.conj())
                .sum::<Complex<f64>>()
                * (2. / sum);
            if options.entry {
                let quick = (WIN / 4..3 * WIN / 4)
                    .map(|i| input[i] * short[i - WIN / 4] * rotation[i].conj())
                    .sum::<Complex<f64>>()
                    * (2. / short_sum);
                coefficient = coefficient * (1. - blend) + quick * blend;
            }
            if h as f64 * base.pitch[frame] >= rate as f64 * 0.48 || !base.voiced[frame] {
                coefficient = Complex::new(0., 0.);
            }
            if chirp || options.entry {
                base.amplitudes[frame][h - 1] = coefficient.norm();
            }
            for (i, v) in residual.iter_mut().enumerate() {
                *v -= (coefficient * rotation[i]).re;
            }
            for (k, d) in distant.iter_mut().enumerate() {
                *d &= !base.voiced[frame]
                    || (k as f64 * rate as f64 / WIN as f64 - h as f64 * base.pitch[frame]).abs()
                        > 130.;
            }
        }
        for (j, track) in tracks.iter().enumerate() {
            if let Ok(at) = track.frames.binary_search(&frame) {
                let hz = track.hz[at];
                let rotation: Vec<_> = (0..WIN)
                    .map(|i| {
                        Complex::from_polar(
                            1.,
                            TAU * hz * (i as f64 - WIN as f64 / 2.) / rate as f64,
                        )
                    })
                    .collect();
                let mut coefficient = residual
                    .iter()
                    .zip(&window)
                    .zip(&rotation)
                    .map(|((v, w), r)| v * w * r.conj())
                    .sum::<Complex<f64>>()
                    * (2. / sum);
                if j >= quiet_count {
                    coefficient *= (track.amplitude[at] / coefficient.norm().max(1e-12)).min(1.);
                }
                for (i, v) in residual.iter_mut().enumerate() {
                    *v -= (coefficient * rotation[i]).re;
                }
                if j < quiet_count {
                    for (k, d) in distant.iter_mut().enumerate() {
                        *d &= (k as f64 * rate as f64 / WIN as f64 - hz).abs() > 95.;
                    }
                }
            }
        }
        let mut z: Vec<_> = residual
            .iter()
            .zip(&window)
            .map(|(v, w)| Complex::new(v * w, 0.))
            .collect();
        fft.process(&mut z);
        let mut tex = [0.; 10];
        let mut bins = [0usize; 10];
        for (k, v) in z.iter().enumerate().take(WIN / 2 + 1) {
            let hz = k as f64 * rate as f64 / WIN as f64;
            let b = BANDS[1..10].partition_point(|&f| hz >= f);
            bins[b] += 1;
            if distant[k] {
                tex[b] += v.norm_sqr();
            }
        }
        for b in 0..10 {
            if bins[b] > 0 {
                tex[b] /= bins[b] as f64;
            }
        }
        base.texture[frame] = tex;
    }
    Ok(Packet {
        base,
        tracks,
        options,
    })
}
pub fn render(p: &Packet, seed: u64, guard: bool, entry_shape: bool) -> Result<Stems> {
    p.base.validate()?;
    let n = p.base.samples;
    let rate = p.base.rate as f64;
    if p.tracks.len() > 4096 {
        return Err("Too many resonance tracks".into());
    }
    for t in &p.tracks {
        if t.frames.is_empty()
            || t.frames.len() != t.hz.len()
            || t.frames.len() != t.amplitude.len()
            || t.frames.windows(2).any(|w| w[0] >= w[1])
            || *t.frames.last().unwrap() >= p.base.pitch.len()
            || t.hz
                .iter()
                .any(|v| !v.is_finite() || !(0. ..rate / 2.).contains(v))
            || t.amplitude
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=4.).contains(v))
        {
            return Err("Invalid resonance track".into());
        }
    }
    let raw = harmonic::render_inner(&p.base, seed, false, false, false)?;
    let mut ring = vec![0_f32; n];
    let mut rng = dsp::ReferenceRandom::new(seed.wrapping_add(101));
    for t in &p.tracks {
        let first = t.frames[0] * HOP;
        let last = t.frames.last().unwrap() * HOP;
        let start = first.saturating_sub(HOP);
        let end = (last + HOP + 1).min(n);
        let mut phase = rng.uniform() * TAU;
        let mut j = 0;
        for (i, v) in ring.iter_mut().enumerate().take(end).skip(start) {
            while j + 1 < t.frames.len() && t.frames[j + 1] * HOP < i {
                j += 1;
            }
            let k = (j + 1).min(t.frames.len() - 1);
            let u = if k == j {
                0.
            } else {
                ((i as f64 - (t.frames[j] * HOP) as f64)
                    / ((t.frames[k] - t.frames[j]) * HOP) as f64)
                    .clamp(0., 1.)
            };
            let hz = t.hz[j] + u * (t.hz[k] - t.hz[j]);
            let mut amp = t.amplitude[j] + u * (t.amplitude[k] - t.amplitude[j]);
            if i < first {
                amp = t.amplitude[0] * (1. - (first - i) as f64 / HOP as f64);
            }
            if i > last {
                amp = *t.amplitude.last().unwrap() * (1. - (i - last) as f64 / HOP as f64);
            }
            phase += TAU * hz / rate;
            *v += (amp * phase.cos()) as f32;
        }
    }
    let mut out = Stems {
        tone: raw.tone,
        texture: raw.texture,
        ring,
    };
    let mixed = out.mix();
    let mut gain = vec![1.; p.base.envelope.len()];
    for (i, g) in gain.iter_mut().enumerate() {
        let f = dsp::centered(&mixed, i * HOP, WIN);
        let current = dsp::rms(&f[WIN / 2 - 144..WIN / 2 + 144]);
        if p.base.envelope[i] > 0.008 && current > 0.004 {
            *g = (p.base.envelope[i] / current).clamp(0.8, 1.25);
        }
    }
    for i in 0..n {
        let g = dsp::interp(&gain, i as f64 / HOP as f64) as f32;
        out.tone[i] *= g;
        out.texture[i] *= g;
        out.ring[i] *= g;
    }
    if guard {
        let gain = attack_guard::pre_attack_gain(&out.mix(), &p.base.attacks)?;
        for i in 0..n {
            out.tone[i] *= gain[i];
            out.texture[i] *= gain[i];
            out.ring[i] *= gain[i];
        }
    }
    if entry_shape {
        let gain = attack_guard::quiet_entry_gain(&out.mix(), &p.base.attacks)?;
        for i in 0..n {
            out.tone[i] *= gain[i];
            out.texture[i] *= gain[i];
            out.ring[i] *= gain[i];
        }
    }
    Ok(out)
}
