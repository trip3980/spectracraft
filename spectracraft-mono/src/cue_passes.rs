//! Exclusive spectral evidence piles with shared fresh phase reconstruction.
//! The packet contains magnitudes and labels, never original complex phase.
use crate::{dsp, Result};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
const WIN: usize = 2048;
const HOP: usize = 240;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub rate: u32,
    pub samples: usize,
    pub magnitudes: Vec<Vec<f64>>,
    pub piles: Vec<Vec<u8>>,
    pub pitch: Vec<f64>,
    pub voiced: Vec<bool>,
}
fn transform(x: &[f64], n: usize, frames: usize) -> Vec<Vec<Complex<f64>>> {
    let fft = FftPlanner::new().plan_fft_forward(n);
    let window: Vec<_> = (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())
        .collect();
    (0..frames)
        .map(|frame| {
            let mut z: Vec<_> = (0..n)
                .map(|i| {
                    Complex::new(
                        (frame * HOP + i)
                            .checked_sub(n / 2)
                            .and_then(|k| x.get(k))
                            .copied()
                            .unwrap_or(0.)
                            * window[i],
                        0.,
                    )
                })
                .collect();
            fft.process(&mut z);
            z.truncate(n / 2 + 1);
            for v in &mut z {
                *v /= n as f64 / 2.;
            }
            z
        })
        .collect()
}
fn invert(z: &[Vec<Complex<f64>>], samples: usize) -> Vec<f64> {
    let fft = FftPlanner::new().plan_fft_inverse(WIN);
    let w: Vec<_> = (0..WIN)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / WIN as f64).cos())
        .collect();
    let mut y = vec![0.; z.len() * HOP + WIN];
    let mut den = y.clone();
    for (frame, row) in z.iter().enumerate() {
        let mut a = vec![Complex::new(0., 0.); WIN];
        a[..WIN / 2 + 1].copy_from_slice(row);
        for k in 1..WIN / 2 {
            a[WIN - k] = a[k].conj();
        }
        fft.process(&mut a);
        for i in 0..WIN {
            y[frame * HOP + i] += a[i].re * 0.5 * w[i];
            den[frame * HOP + i] += w[i] * w[i];
        }
    }
    (0..samples)
        .map(|i| y[i + WIN / 2] / den[i + WIN / 2].max(1e-10))
        .collect()
}
fn reflect(i: isize, n: usize) -> usize {
    let k = i.rem_euclid(2 * n as isize);
    if k < n as isize {
        k as usize
    } else {
        2 * n - 1 - k as usize
    }
}
pub fn analyze(x: &[f32], rate: u32) -> Result<Packet> {
    if rate != 48000 || x.is_empty() || x.len() > 480000 || x.iter().any(|v| !v.is_finite()) {
        return Err("Cue experiment expects finite 48 kHz mono, up to ten seconds".into());
    }
    let frames = x.len().div_ceil(HOP) + 1;
    let input: Vec<_> = x.iter().map(|v| *v as f64).collect();
    let a: Vec<Vec<_>> = transform(&input, WIN, frames)
        .iter()
        .map(|r| r.iter().map(|v| v.norm()).collect())
        .collect();
    let short = transform(&input, 960, frames);
    let df = rate as f64 / WIN as f64;
    let core = (4200. / df).ceil() as usize;
    let grid: Vec<_> = (0..93).map(|i| 82. * 2_f64.powf(i as f64 / 32.)).collect();
    let mut ratios = vec![vec![0.; core]; frames];
    let mut salience = vec![vec![0.; grid.len()]; frames];
    for frame in 0..frames {
        for k in 0..core {
            let local = dsp::median(
                &(-4..=4)
                    .map(|d| a[frame][(k as isize + d).clamp(0, core as isize - 1) as usize])
                    .collect::<Vec<_>>(),
            );
            let peak = a[frame][k.saturating_sub(1)..=(k + 1).min(core - 1)]
                .iter()
                .copied()
                .fold(0., f64::max);
            ratios[frame][k] = ((peak / (local + 0.00012)).ln_1p() - 2_f64.ln()).max(0.);
        }
        for (g, hz) in grid.iter().enumerate() {
            salience[frame][g] = (1..=8)
                .map(|h| {
                    ratios[frame][((hz * h as f64 / df).round_ties_even() as usize).min(core - 1)]
                        / (h as f64).sqrt()
                })
                .sum();
        }
    }
    let mut links = vec![vec![0; grid.len()]; frames];
    let mut score = salience[0].clone();
    for frame in 1..frames {
        let mut next = vec![0.; grid.len()];
        for j in 0..grid.len() {
            let best = (0..grid.len())
                .max_by(|&a, &b| {
                    (score[a] - 0.72 * a.abs_diff(j).min(18) as f64)
                        .total_cmp(&(score[b] - 0.72 * b.abs_diff(j).min(18) as f64))
                })
                .unwrap();
            links[frame][j] = best;
            next[j] = salience[frame][j] + score[best] - 0.72 * best.abs_diff(j).min(18) as f64;
        }
        score = next;
    }
    let mut path = vec![0; frames];
    path[frames - 1] = (0..grid.len())
        .max_by(|&a, &b| score[a].total_cmp(&score[b]))
        .unwrap();
    for i in (1..frames).rev() {
        path[i - 1] = links[i][path[i]];
    }
    let pitch: Vec<_> = path.iter().map(|&i| grid[i]).collect();
    let voiced: Vec<_> = (0..frames)
        .map(|i| {
            (1..=8)
                .filter(|&h| {
                    ratios[i][((pitch[i] * h as f64 / df).round_ties_even() as usize).min(core - 1)]
                        > 0.85
                })
                .count()
                >= 3
                && a[i].iter().copied().fold(0., f64::max) > 0.0025
        })
        .collect();
    let bands = [0., 250., 500., 1000., 2000., 4000., 8000., 16000., 24001.];
    let mut energy = vec![[0.; 8]; frames];
    for i in 0..frames {
        for (k, z) in short[i].iter().enumerate() {
            let b = bands[1..].partition_point(|&f| k as f64 * 50. >= f).min(7);
            energy[i][b] += z.norm_sqr();
        }
    }
    let mut relative = vec![vec![0.; WIN / 2 + 1]; frames];
    let mut local = vec![vec![false; WIN / 2 + 1]; frames];
    for i in 0..frames {
        for k in 0..=WIN / 2 {
            let floor = dsp::median(
                &(-7..=7)
                    .map(|d| a[i][(k as isize + d).clamp(0, WIN as isize / 2) as usize])
                    .collect::<Vec<_>>(),
            );
            relative[i][k] = 20. * ((a[i][k] + 1e-15) / (floor + 1e-15)).log10();
            let max = a[i][k.saturating_sub(1)..=(k + 1).min(WIN / 2)]
                .iter()
                .copied()
                .fold(0., f64::max);
            local[i][k] = a[i][k] >= max * 0.72 && relative[i][k] >= 7.;
        }
    }
    let mut piles = vec![vec![3; WIN / 2 + 1]; frames];
    for i in 0..frames {
        let nearby = (-8..=8).any(|d| voiced[reflect(i as isize + d, frames)]);
        for k in 0..=WIN / 2 {
            let hz = k as f64 * df;
            let h = (hz / pitch[i]).round_ties_even();
            let matched = h >= 1.
                && hz < 10500.
                && (hz - h * pitch[i]).abs() < (0.30 * pitch[i]).min((df * 1.20).max(0.007 * hz));
            let strength = 20. * a[i][k].max(1e-15).log10();
            let anchor = matched && voiced[i] && relative[i][k] >= 4. && strength >= -45.;
            let b = bands[1..].partition_point(|&f| hz >= f).min(7);
            let baseline = (-17..=17)
                .map(|d| energy[reflect(i as isize + d, frames)][b])
                .sum::<f64>()
                / 35.;
            let pulsed = nearby && energy[i][b] > 2.5 * baseline.max(1e-15);
            let supported = !anchor
                && strength >= -43.
                && relative[i][k] >= 7.
                && ((matched && nearby) || pulsed);
            let persistence = (-2..=2)
                .filter(|&d| local[reflect(i as isize + d, frames)][k])
                .count() as f64
                / 5.
                > 0.59;
            let faint =
                !anchor && !supported && (-72. ..-43.).contains(&strength) && persistence && nearby;
            piles[i][k] = if anchor {
                0
            } else if supported {
                1
            } else if faint {
                2
            } else {
                3
            };
        }
    }
    Ok(Packet {
        rate,
        samples: x.len(),
        magnitudes: a,
        piles,
        pitch,
        voiced,
    })
}
pub fn render(p: &Packet) -> Result<[Vec<f64>; 4]> {
    let frames = p.samples.div_ceil(HOP) + 1;
    if p.rate != 48000
        || p.samples == 0
        || p.samples > 480000
        || p.magnitudes.len() != frames
        || p.piles.len() != frames
        || p.magnitudes
            .iter()
            .any(|r| r.len() != 1025 || r.iter().any(|v| !v.is_finite() || !(0. ..=4.).contains(v)))
        || p.piles
            .iter()
            .any(|r| r.len() != 1025 || r.iter().any(|v| *v > 3))
    {
        return Err("Invalid spectral cue packet".into());
    }
    let mut rng = dsp::ReferenceRandom::new(1859);
    let mut phase = vec![vec![Complex::new(0., 0.); 1025]; frames];
    for k in 0..1025 {
        for row in &mut phase {
            row[k] = Complex::from_polar(1., std::f64::consts::TAU * rng.uniform());
        }
    }
    let compose = |phase: &[Vec<Complex<f64>>]| {
        phase
            .iter()
            .zip(&p.magnitudes)
            .map(|(r, a)| r.iter().zip(a).map(|(p, a)| p * a).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    };
    for _ in 0..40 {
        let y = invert(&compose(&phase), (frames - 1) * HOP);
        let s = transform(&y, WIN, frames);
        for i in 0..frames {
            for k in 0..1025 {
                phase[i][k] = Complex::from_polar(1., s[i][k].arg());
            }
        }
    }
    let full = compose(&phase);
    let stems = std::array::from_fn(|pile| {
        let masked: Vec<Vec<_>> = full
            .iter()
            .enumerate()
            .map(|(i, row)| {
                row.iter()
                    .enumerate()
                    .map(|(k, z)| {
                        if p.piles[i][k] as usize == pile {
                            *z
                        } else {
                            Complex::new(0., 0.)
                        }
                    })
                    .collect()
            })
            .collect();
        invert(&masked, p.samples)
    });
    let reference = invert(&full, p.samples);
    if (0..p.samples)
        .any(|i| (stems.iter().map(|s| s[i]).sum::<f64>() - reference[i]).abs() > 1e-11)
    {
        return Err("Shared-phase ablation sum failed".into());
    }
    Ok(stems)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn harmonic_control_and_exclusive_shared_phase() {
        let x: Vec<_> = (0..48000)
            .map(|i| {
                (1..7)
                    .map(|k| {
                        (0.12 / k as f64)
                            * (std::f64::consts::TAU * 187. * k as f64 * i as f64 / 48000.).sin()
                    })
                    .sum::<f64>() as f32
            })
            .collect();
        let p = analyze(&x, 48000).unwrap();
        let q = analyze(&x, 48000).unwrap();
        assert_eq!(p.piles, q.piles);
        assert_eq!(p.pitch, q.pitch);
        let ids: Vec<_> = (13..p.voiced.len() - 12).filter(|&i| p.voiced[i]).collect();
        assert!(ids.len() > 70);
        assert!(
            dsp::median(
                &ids.iter()
                    .map(|&i| (p.pitch[i] - 187.).abs())
                    .collect::<Vec<_>>()
            ) < 7.
        );
        let (mut all, mut anchor) = (0., 0.);
        for i in ids {
            for k in 0..1025 {
                let power = p.magnitudes[i][k].powi(2);
                all += power;
                if p.piles[i][k] == 0 {
                    anchor += power;
                }
            }
        }
        assert!(anchor / all > 0.75);
        let y = render(&p).unwrap();
        assert_ne!(y[0], x.iter().map(|v| *v as f64).collect::<Vec<_>>());
        assert!(y.iter().flatten().all(|v| v.is_finite()));
    }
}
