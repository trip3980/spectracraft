//! Experimental masked-onset measurements. Whitening only detects events;
//! synthesis reallocates existing texture energy into fresh raw-band attacks.
use crate::{dsp, Result};
use serde::{Deserialize, Serialize};
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub frame: usize,
    pub hop: usize,
    pub energies: Vec<[f64; 6]>,
}
fn bank(x: &[f64], rate: u32) -> Vec<Vec<f64>> {
    let mut edges: Vec<_> = [120., 360., 850., 1800., 3800., 7600., 14500.]
        .into_iter()
        .filter(|v| *v < 0.46 * rate as f64)
        .collect();
    edges.push(0.47 * rate as f64);
    edges
        .windows(2)
        .map(|v| dsp::bandpass(x, rate, v[0], v[1], 2))
        .collect()
}
fn frames(x: &[Vec<f64>], h: usize) -> Vec<Vec<f64>> {
    x.iter()
        .map(|b| {
            b.chunks_exact(h)
                .map(|v| v.iter().map(|v| v * v).sum::<f64>() / h as f64)
                .collect()
        })
        .collect()
}
fn gates(x: &[Vec<f64>], j: usize, threshold: f64) -> (Vec<f64>, Vec<f64>) {
    let floor: Vec<_> = x.iter().map(|v| dsp::median(&v[j - 38..j - 5])).collect();
    let gate = x
        .iter()
        .zip(&floor)
        .map(|(v, f)| {
            let mad = dsp::median(
                &v[j - 38..j - 5]
                    .iter()
                    .map(|v| (v - f).abs())
                    .collect::<Vec<_>>(),
            );
            (3. * f).max(f + 8. * mad).max(threshold)
        })
        .collect();
    (floor, gate)
}
fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for i in 0..n {
        let k = (i..n).max_by(|&j, &k| a[j][i].abs().total_cmp(&a[k][i].abs()))?;
        a.swap(i, k);
        b.swap(i, k);
        if a[i][i].abs() < 1e-25 {
            return None;
        }
        for j in i + 1..n {
            let factor = a[j][i] / a[i][i];
            for k in i..n {
                a[j][k] -= factor * a[i][k];
            }
            b[j] -= factor * b[i];
        }
    }
    for i in (0..n).rev() {
        for j in i + 1..n {
            b[i] -= a[i][j] * b[j];
        }
        b[i] /= a[i][i];
    }
    Some(b)
}
pub fn detect(x: &[f32], rate: u32, physical: bool) -> Result<Vec<Event>> {
    if !(16000..=96000).contains(&rate)
        || x.len() > rate as usize * 30
        || x.iter().any(|v| !v.is_finite())
    {
        return Err("Invalid masked-onset audio".into());
    }
    let h = (0.002 * rate as f64).round_ties_even() as usize;
    let input: Vec<_> = x.iter().map(|v| *v as f64).collect();
    let raw = frames(&bank(&input, rate), h);
    let mut events: Vec<Event> = Vec::new();
    if raw[0].len() < 54 {
        return Ok(events);
    }
    let peak = raw.iter().flatten().copied().fold(0., f64::max);
    let threshold = 1e-8 * peak.max(1e-14);
    let order = ((0.0007 * rate as f64).round_ties_even() as usize).min(32);
    for j in 47..raw[0].len() - 6 {
        if events.last().is_some_and(|e| j - e.frame < 12) {
            continue;
        }
        let (fr, gr) = gates(&raw, j, threshold);
        let strong: Vec<_> = raw
            .iter()
            .zip(&gr)
            .map(|(v, g)| v[j].max(v[j + 1]) > *g)
            .collect();
        if strong.iter().filter(|v| **v).count() < if physical { 2 } else { 1 } {
            continue;
        }
        let train = &input[(j - 45) * h..(j - 5) * h];
        if train.iter().map(|v| v * v).sum::<f64>() / (train.len() as f64) < 1e-5 * peak {
            continue;
        }
        let ac: Vec<_> = (0..=order)
            .map(|lag| {
                train[..train.len() - lag]
                    .iter()
                    .zip(&train[lag..])
                    .map(|(a, b)| a * b)
                    .sum::<f64>()
            })
            .collect();
        if ac[0] < 1e-20 {
            continue;
        }
        let matrix = (0..order)
            .map(|i| {
                (0..order)
                    .map(|k| {
                        if i == k {
                            ac[0] * 1.0005
                        } else {
                            ac[i.abs_diff(k)]
                        }
                    })
                    .collect()
            })
            .collect();
        let Some(ar) = solve(matrix, ac[1..].to_vec()) else {
            continue;
        };
        let clip = &input[(j - 45) * h..(j + 6) * h];
        let residual: Vec<_> = clip
            .iter()
            .enumerate()
            .map(|(i, v)| {
                v - ar
                    .iter()
                    .enumerate()
                    .filter_map(|(k, a)| i.checked_sub(k + 1).map(|at| a * clip[at]))
                    .sum::<f64>()
            })
            .collect();
        let white = frames(&bank(&residual, rate), h);
        let (fw, gw) = gates(&white, 45, threshold);
        let whiten: Vec<_> = white
            .iter()
            .zip(&gw)
            .map(|(v, g)| v[45].max(v[46]) > *g)
            .collect();
        let used = if physical { &strong } else { &whiten };
        if whiten.iter().filter(|v| **v).count() < 2
            || used.iter().filter(|v| **v).count() < 2
            || !strong.iter().zip(&whiten).any(|(a, b)| *a && *b)
        {
            continue;
        }
        let (power, base, at) = if physical {
            (&raw, &fr, j)
        } else {
            (&white, &fw, 45)
        };
        let energies: Vec<[f64; 6]> = (0..power.len())
            .map(|b| {
                std::array::from_fn(|i| {
                    if used[b] {
                        (power[b][at + i] - base[b]).max(0.)
                    } else {
                        0.
                    }
                })
            })
            .collect();
        let first = energies.iter().map(|e| e[0]).sum::<f64>();
        let early = energies.iter().map(|e| e[0] + e[1]).sum::<f64>();
        if first < 0.025 * early {
            continue;
        }
        events.push(Event {
            frame: j,
            hop: h,
            energies,
        });
    }
    Ok(events)
}
pub struct Reallocation {
    pub retained: Vec<f64>,
    pub attack: Vec<f64>,
    pub energy_error: Vec<f64>,
}
pub fn render(texture: &[f32], events: &[Event], rate: u32, seed: u64) -> Result<Reallocation> {
    if !(16000..=96000).contains(&rate) || texture.iter().any(|v| !v.is_finite()) {
        return Err("Invalid attack texture".into());
    }
    let h = (0.002 * rate as f64).round_ties_even() as usize;
    let mut end = 0;
    let count = bank(&[], rate).len();
    for e in events {
        let start = e.frame.checked_mul(h).ok_or("Event overflow")?;
        if e.hop != h
            || start < end
            || start.checked_add(6 * h).is_none_or(|v| v > texture.len())
            || e.energies.len() != count
            || e.energies
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0. ..=16.).contains(v))
        {
            return Err("Invalid attack packet".into());
        }
        end = start + 6 * h;
    }
    let mut rng = dsp::Random::new(seed);
    let mut retained: Vec<_> = texture.iter().map(|v| *v as f64).collect();
    let mut attack = vec![0.; texture.len()];
    let mut ledger = Vec::new();
    for e in events {
        let width = 6 * h;
        let start = e.frame * h;
        let noise: Vec<_> = (0..width + 4 * h).map(|_| rng.normal()).collect();
        let bands = bank(&noise, rate);
        let window: Vec<_> = (0..width)
            .map(|i| {
                (std::f64::consts::PI * (i as f64 + 0.5) / width as f64)
                    .sin()
                    .powi(2)
            })
            .collect();
        let mut proposed = vec![0.; width];
        for (b, p) in bands.iter().zip(&e.energies) {
            let b = &b[4 * h..];
            let norm = dsp::rms(b).max(1e-15);
            for i in 0..width {
                let gain = dsp::interp(p, i as f64 / h as f64 - 0.5).sqrt();
                proposed[i] += gain * b[i] / norm;
            }
        }
        for i in 0..width {
            proposed[i] *= window[i];
        }
        let source = &retained[start..start + width];
        let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
        let anchor: Vec<_> = source.iter().zip(&window).map(|(a, b)| a * b).collect();
        let denom = dot(source, &anchor);
        if denom <= 1e-24 {
            continue;
        }
        let projection = dot(&proposed, source) / denom;
        for i in 0..width {
            proposed[i] -= anchor[i] * projection;
        }
        let maxdebit = source
            .iter()
            .zip(&window)
            .map(|(v, w)| v * v * (1.4 * w - (0.7 * w).powi(2)))
            .sum::<f64>();
        let power = dot(&proposed, &proposed);
        if power <= 1e-24 {
            continue;
        }
        let want = power.min(maxdebit);
        let d = dot(&anchor, &anchor);
        let gain = want / (denom + (denom * denom - d * want).max(0.).sqrt());
        let new: Vec<_> = source
            .iter()
            .zip(&window)
            .map(|(v, w)| v * (1. - gain * w))
            .collect();
        let projection = dot(&proposed, &new) / dot(&anchor, &new);
        for i in 0..width {
            proposed[i] -= anchor[i] * projection;
        }
        let norm = dot(&proposed, &proposed);
        if norm <= 1e-24 {
            continue;
        }
        for v in &mut proposed {
            *v *= (want / norm).sqrt();
        }
        let mix: Vec<_> = new.iter().zip(&proposed).map(|(a, b)| a + b).collect();
        ledger.push(dot(&mix, &mix) - dot(source, source));
        retained[start..start + width].copy_from_slice(&new);
        attack[start..start + width].copy_from_slice(&proposed);
    }
    Ok(Reallocation {
        retained,
        attack,
        energy_error: ledger,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn energy_debit_and_outside_preservation() {
        let x: Vec<_> = (0..48000)
            .map(|i| (0.02 * (i as f64 * 0.061).sin()) as f32)
            .collect();
        let e = Event {
            frame: 150,
            hop: 96,
            energies: vec![[0.003; 6]; bank(&[], 48000).len()],
        };
        let y = render(&x, &[e], 48000, 736).unwrap();
        assert_eq!(y.energy_error.len(), 1);
        assert!(y.energy_error[0].abs() < 1e-10);
        for i in (0..14400).chain(14976..48000) {
            assert_eq!(y.retained[i], x[i] as f64);
            assert_eq!(y.attack[i], 0.);
        }
        assert!(detect(&vec![0.; 48000], 48000, true).unwrap().is_empty());
    }
}
