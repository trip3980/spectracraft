//! General/flute-reference harmonic and stochastic texture analysis, ported from
//! the retired rebuild experiment. Rendering accepts measurements only.
use crate::{
    attack_guard::{self, AttackEvidence},
    dsp, Result,
};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;
pub const WIN: usize = 1024;
pub const HOP: usize = 128;
pub const BANDS: [f64; 11] = [
    0., 500., 1000., 1750., 3000., 5000., 8000., 12000., 16000., 20500., 22500.,
];
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    #[default]
    General,
    FluteReference,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub version: u32,
    pub rate: u32,
    pub samples: usize,
    pub profile: Profile,
    pub pitch: Vec<f64>,
    pub voiced: Vec<bool>,
    pub amplitudes: Vec<[f64; 9]>,
    pub texture: Vec<[f64; 10]>,
    pub envelope: Vec<f64>,
    pub attacks: Vec<AttackEvidence>,
}
#[derive(Debug)]
pub struct Stems {
    pub tone: Vec<f32>,
    pub texture: Vec<f32>,
}
impl Stems {
    pub fn mix(&self) -> Vec<f32> {
        self.tone
            .iter()
            .zip(&self.texture)
            .map(|(a, b)| a + b)
            .collect()
    }
}
fn band(hz: f64) -> usize {
    BANDS[1..10].partition_point(|&edge| hz >= edge)
}
fn sample_spectrum(spec: &[f64], hz: f64, df: f64) -> f64 {
    dsp::interp(spec, hz / df)
}
pub fn analyze(x: &[f32], rate: u32, profile: Profile) -> Result<Packet> {
    if !(16000..=96000).contains(&rate)
        || x.is_empty()
        || x.len() > rate as usize * 30
        || x.iter().any(|v| !v.is_finite() || v.abs() > 4.)
    {
        return Err("Expected finite mono 16–96 kHz audio, up to 30 seconds".into());
    }
    if profile == Profile::FluteReference && rate != 44100 {
        return Err("Flute reference profile requires 44.1 kHz".into());
    }
    let count = x.len().div_ceil(HOP) + 1;
    let long = if profile == Profile::General {
        2 * ((0.02322 * rate as f64).round() as usize).max(1024)
    } else {
        WIN
    };
    let nfft = if profile == Profile::General {
        (8 * long).next_power_of_two()
    } else {
        4096
    };
    let df = rate as f64 / nfft as f64;
    let window = dsp::hann(long);
    let sum: f64 = window.iter().sum();
    let fft = FftPlanner::new().plan_fft_forward(nfft);
    let mut buf = vec![Complex::new(0., 0.); nfft];
    let mut pitch = vec![120.; count];
    let mut voiced = vec![false; count];
    for frame in 0..count {
        buf.fill(Complex::new(0., 0.));
        for (i, v) in dsp::centered(x, frame * HOP, long).iter().enumerate() {
            buf[i].re = v * window[i];
        }
        fft.process(&mut buf);
        let spec: Vec<_> = buf[..nfft / 2 + 1]
            .iter()
            .map(|v| v.norm() * 2. / sum)
            .collect();
        if profile == Profile::FluteReference {
            let region: Vec<_> = (1..spec.len() - 1)
                .filter(|&k| (2000. ..=2400.).contains(&(k as f64 * df)))
                .collect();
            let k = *region
                .iter()
                .max_by(|&&a, &&b| spec[a].total_cmp(&spec[b]))
                .unwrap();
            let floor =
                (region.iter().map(|&k| spec[k].powi(2)).sum::<f64>() / region.len() as f64).sqrt();
            pitch[frame] = dsp::refined_bin(&spec, k) * df;
            voiced[frame] = spec[k] > 0.002 && spec[k] > 1.65 * floor;
            continue;
        }
        let region: Vec<_> = (1..spec.len() - 1)
            .filter(|&k| (40. ..=3500_f64.min(rate as f64 * 0.4)).contains(&(k as f64 * df)))
            .collect();
        let floor =
            (region.iter().map(|&k| spec[k].powi(2)).sum::<f64>() / region.len() as f64).sqrt();
        let mut candidates: Vec<_> = region
            .into_iter()
            .filter(|&k| spec[k] > spec[k - 1] && spec[k] >= spec[k + 1])
            .collect();
        candidates.sort_by(|&a, &b| spec[b].total_cmp(&spec[a]));
        candidates.truncate(12);
        let mut best = -1.;
        for k in candidates {
            let hz = dsp::refined_bin(&spec, k) * df;
            let values: Vec<_> = (2..10)
                .filter(|&h| hz * (h as f64) < rate as f64 * 0.46)
                .map(|h| (h, sample_spectrum(&spec, hz * h as f64, df)))
                .collect();
            let stronger = values.iter().map(|(_, v)| *v).fold(0., f64::max);
            if spec[k] < 0.002_f64.max(3.8 * floor).max(0.055 * stronger) {
                continue;
            }
            let score = spec[k]
                + 1.15
                    * values
                        .iter()
                        .filter(|(_, v)| *v > 2.5 * floor)
                        .map(|(h, v)| v / (*h as f64).powf(0.28))
                        .sum::<f64>();
            if score > best {
                best = score;
                pitch[frame] = hz;
                voiced[frame] = true;
            }
        }
    }
    let good: Vec<_> = (0..count).filter(|&i| voiced[i]).collect();
    if profile == Profile::General && !good.is_empty() {
        let mut k = 0;
        for i in 0..count {
            while k + 1 < good.len() && good[k + 1] < i {
                k += 1;
            }
            let a = good[k];
            let b = good[(k + 1).min(good.len() - 1)];
            if !voiced[i] {
                pitch[i] = if i <= good[0] {
                    pitch[good[0]]
                } else if i >= *good.last().unwrap() {
                    pitch[*good.last().unwrap()]
                } else {
                    pitch[a] + (pitch[b] - pitch[a]) * (i - a) as f64 / (b - a) as f64
                };
            }
        }
    }
    let original = pitch.clone();
    for i in 0..count {
        pitch[i] = dsp::median(&[
            original[i.saturating_sub(1)],
            original[i],
            original[(i + 1).min(count - 1)],
        ]);
    }
    let window = dsp::hann(WIN);
    let sum: f64 = window.iter().sum();
    let fft = FftPlanner::new().plan_fft_forward(WIN);
    let mut amplitudes = Vec::with_capacity(count);
    let mut texture = Vec::with_capacity(count);
    let mut envelope = Vec::with_capacity(count);
    for frame in 0..count {
        let input = dsp::centered(x, frame * HOP, WIN);
        envelope.push(dsp::rms(&input[WIN / 2 - 144..WIN / 2 + 144]));
        let mut residual = input.clone();
        let mut amps = [0.; 9];
        for h in 1..10 {
            if !voiced[frame] || h as f64 * pitch[frame] >= rate as f64 * 0.48 {
                continue;
            }
            let step = TAU * h as f64 * pitch[frame] / rate as f64;
            let coefficient = input
                .iter()
                .zip(&window)
                .enumerate()
                .map(|(i, (v, w))| Complex::from_polar(v * w, (WIN as f64 / 2. - i as f64) * step))
                .sum::<Complex<f64>>()
                * (2. / sum);
            amps[h - 1] = coefficient.norm();
            for (i, r) in residual.iter_mut().enumerate() {
                *r -=
                    (coefficient * Complex::from_polar(1., (i as f64 - WIN as f64 / 2.) * step)).re;
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
            let b = band(hz);
            bins[b] += 1;
            let distant =
                !voiced[frame] || (1..10).all(|h| (hz - h as f64 * pitch[frame]).abs() > 130.);
            if distant {
                tex[b] += v.norm_sqr();
            }
        }
        for b in 0..10 {
            if bins[b] > 0 {
                tex[b] /= bins[b] as f64;
            }
        }
        amplitudes.push(amps);
        texture.push(tex);
    }
    Ok(Packet {
        version: 1,
        rate,
        samples: x.len(),
        profile,
        pitch,
        voiced,
        amplitudes,
        texture,
        envelope,
        attacks: attack_guard::measure_quiet_attacks(x, rate)?,
    })
}
impl Packet {
    pub fn validate(&self) -> Result<()> {
        let n = self.samples.div_ceil(HOP) + 1;
        if self.version != 1
            || !(16000..=96000).contains(&self.rate)
            || self.samples == 0
            || self.samples > self.rate as usize * 30
            || self.pitch.len() != n
            || self.voiced.len() != n
            || self.amplitudes.len() != n
            || self.texture.len() != n
            || self.envelope.len() != n
            || self
                .pitch
                .iter()
                .any(|v| !v.is_finite() || !(20. ..self.rate as f64 / 2.).contains(v))
            || self
                .amplitudes
                .iter()
                .flatten()
                .chain(self.texture.iter().flatten())
                .chain(&self.envelope)
                .any(|v| !v.is_finite() || !(0. ..=1e6).contains(v))
        {
            return Err("Invalid harmonic packet".into());
        }
        Ok(())
    }
}
pub fn render(packet: &Packet, seed: u64, guard: bool, entry_shape: bool) -> Result<Stems> {
    render_inner(packet, seed, guard, entry_shape, true)
}
pub(crate) fn render_inner(
    packet: &Packet,
    seed: u64,
    guard: bool,
    entry_shape: bool,
    trim: bool,
) -> Result<Stems> {
    packet.validate()?;
    let n = packet.samples;
    let rate = packet.rate as f64;
    let mut rng = dsp::ReferenceRandom::new(seed);
    let mut phase = vec![0.; n];
    let mut angle = 0.;
    for (i, v) in phase.iter_mut().enumerate() {
        angle += TAU * dsp::interp(&packet.pitch, i as f64 / HOP as f64) / rate;
        *v = angle;
    }
    let mut tone = vec![0_f64; n];
    for h in 1..10 {
        let offset = rng.uniform() * TAU;
        let amplitudes: Vec<_> = packet.amplitudes.iter().map(|a| a[h - 1]).collect();
        for (i, v) in tone.iter_mut().enumerate() {
            if h as f64 * dsp::interp(&packet.pitch, i as f64 / HOP as f64) < rate / 2. {
                *v += dsp::interp(&amplitudes, i as f64 / HOP as f64)
                    * (h as f64 * phase[i] + offset).cos();
            }
        }
    }
    let window = dsp::hann(WIN);
    let window_power: f64 = window.iter().map(|v| v * v).sum();
    let fft = FftPlanner::new().plan_fft_inverse(WIN);
    let mut noise = vec![0.; n + WIN + HOP];
    let mut den = vec![0.; noise.len()];
    for (frame, tex) in packet.texture.iter().enumerate() {
        let start = frame * HOP;
        let mut z = vec![Complex::new(0., 0.); WIN];
        for k in 0..=WIN / 2 {
            let hz = k as f64 * rate / WIN as f64;
            z[k] = Complex::from_polar(tex[band(hz)].sqrt(), rng.uniform() * TAU);
            if k == 0 {
                z[k] = Complex::new(0., 0.);
            } else if k == WIN / 2 {
                z[k].im = 0.;
            } else {
                z[WIN - k] = z[k].conj();
            }
        }
        fft.process(&mut z);
        for i in 0..WIN {
            noise[start + i] += z[i].re / WIN as f64 * window[i];
            den[start + i] += window[i] * window[i];
        }
    }
    let mut texture: Vec<_> = (0..n)
        .map(|i| {
            noise[i + WIN / 2] / den[i + WIN / 2].max(1e-8).sqrt()
                * (WIN as f64 / window_power).sqrt()
        })
        .collect();
    let raw: Vec<_> = tone
        .iter()
        .zip(&texture)
        .map(|(a, b)| (a + b) as f32)
        .collect();
    let mut gain = vec![1.; packet.envelope.len()];
    for (i, g) in gain.iter_mut().enumerate() {
        let f = dsp::centered(&raw, i * HOP, WIN);
        let measured = dsp::rms(&f[WIN / 2 - 144..WIN / 2 + 144]);
        if trim && packet.envelope[i] > 0.008 && measured > 0.004 {
            *g = (packet.envelope[i] / measured).clamp(0.8, 1.25);
        }
    }
    for i in 0..n {
        let g = dsp::interp(&gain, i as f64 / HOP as f64);
        tone[i] *= g;
        texture[i] *= g;
    }
    let mut stems = Stems {
        tone: tone.into_iter().map(|v| v as f32).collect(),
        texture: texture.into_iter().map(|v| v as f32).collect(),
    };
    if guard {
        let g = attack_guard::pre_attack_gain(&stems.mix(), &packet.attacks)?;
        for i in 0..n {
            stems.tone[i] *= g[i];
            stems.texture[i] *= g[i];
        }
    }
    if entry_shape {
        let g = attack_guard::quiet_entry_gain(&stems.mix(), &packet.attacks)?;
        for i in 0..n {
            stems.tone[i] *= g[i];
            stems.texture[i] *= g[i];
        }
    }
    Ok(stems)
}
