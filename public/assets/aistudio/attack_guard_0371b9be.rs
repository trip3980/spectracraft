//! Short-time energy evidence, never reference samples, for quiet entrances.
use crate::{valid_rate, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttackEvidence {
    start_sample: usize,
    samples_per_block: usize,
    pre_rms: Vec<f64>,
    entry_rms: Vec<f64>,
    peak_rms: f64,
}

fn sq(x: f32) -> f64 {
    f64::from(x).powi(2)
}
fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|&v| sq(v)).sum::<f64>() / x.len() as f64).sqrt()
}
fn peak(x: &[f64]) -> f64 {
    x.iter().copied().fold(0., f64::max)
}
fn norm2(x: &[f64]) -> f64 {
    x.iter().map(|v| v * v).sum()
}

pub fn measure_quiet_attacks(x: &[f32], rate: u32) -> Result<Vec<AttackEvidence>> {
    if !valid_rate(rate) || x.iter().any(|v| !v.is_finite()) {
        return Err("Invalid attack measurement input".into());
    }
    let h = (f64::from(rate) * 0.001).round_ties_even() as usize;
    let e: Vec<_> = x.chunks_exact(h).map(rms).collect();
    let mut out: Vec<AttackEvidence> = Vec::new();
    let global = peak(&e);
    if e.len() < 48 || global == 0. {
        return Ok(out);
    }
    for i in 24..e.len() - 24 {
        if out.last().is_some_and(|a| i * h - a.start_sample < 48 * h) {
            continue;
        }
        let now = peak(&e[i..i + 25]);
        if now < 0.015 * global || !(e[i - 1] < 0.05 * now && e[i] >= 0.05 * now) {
            continue;
        }
        if norm2(&e[i - 20..i]) > 0.16_f64.powi(2) * norm2(&e[i..i + 25])
            || peak(&e[i - 6..i]) > 0.06 * now
        {
            continue;
        }
        out.push(AttackEvidence {
            start_sample: i * h,
            samples_per_block: h,
            pre_rms: e[i - 20..i].to_vec(),
            entry_rms: e[i..(i + 48).min(e.len())].to_vec(),
            peak_rms: global,
        });
    }
    Ok(out)
}

fn validate(input: &[f32], events: &[AttackEvidence]) -> Result<()> {
    let h = events.first().map(|ev| ev.samples_per_block);
    if input.iter().any(|v| !v.is_finite())
        || events.iter().any(|ev| {
            let b = ev.samples_per_block;
            !(8..=96).contains(&b)
                || Some(b) != h
                || ev.start_sample > input.len()
                || ev.start_sample % b != 0
                || ev.pre_rms.len() != 20
                || ev.pre_rms.len() * b > ev.start_sample
                || ev.entry_rms.len() > 48
                || !ev.peak_rms.is_finite()
                || ev.peak_rms < 0.
                || ev
                    .pre_rms
                    .iter()
                    .chain(&ev.entry_rms)
                    .any(|v| !v.is_finite() || *v < 0.)
        })
    {
        return Err("Invalid quiet-attack evidence".into());
    }
    Ok(())
}

/// Returns the same gain curve for every synthesized layer; gain never exceeds unity.
pub fn pre_attack_gain(mix: &[f32], events: &[AttackEvidence]) -> Result<Vec<f32>> {
    validate(mix, events)?;
    let mut gain = vec![1_f32; mix.len()];
    for ev in events {
        let h = ev.samples_per_block;
        let len = ev.pre_rms.len() * h;
        let start = ev.start_sample - len;
        let knots: Vec<_> = mix[start..ev.start_sample]
            .chunks_exact(h)
            .zip(&ev.pre_rms)
            .enumerate()
            .map(|(k, (x, c))| {
                (
                    (k as f64 + 0.5) * h as f64,
                    ((1.4 * c + 1e-7) / rms(x).max(1e-12)).min(1.),
                )
            })
            .collect();
        let ramp = len as f64 - (0.25 * h as f64).min((0.15 * h as f64).max(1.));
        let mut points = vec![(0., 1.)];
        points.extend_from_slice(&knots);
        points.push((ramp, knots.last().unwrap().1));
        points.push((len as f64, 1.));
        let mut k = 0;
        for (i, g) in gain[start..ev.start_sample].iter_mut().enumerate() {
            let t = i as f64;
            while t > points[k + 1].0 {
                k += 1;
            }
            let (a, x) = points[k];
            let (b, y) = points[k + 1];
            *g = g.min((x + (y - x) * (t - a) / (b - a)) as f32);
        }
    }
    Ok(gain)
}

/// Restores only low-level entry detail with one gain curve shared by all layers.
pub fn quiet_entry_gain(mix: &[f32], events: &[AttackEvidence]) -> Result<Vec<f32>> {
    validate(mix, events)?;
    let mut gain = vec![1_f32; mix.len()];
    let Some(first) = events.first() else {
        return Ok(gain);
    };
    let h = first.samples_per_block;
    let scale = mix.chunks_exact(h).map(rms).fold(0., f64::max);
    let ramp = (0.12 * h as f64).round_ties_even().max(1.) as usize;
    let ramp_position = |i: usize| (i as f64 / ramp as f64).min(1.);
    for ev in events {
        let n = ev.entry_rms.len();
        if n == 0 || ev.peak_rms == 0. || n * h > mix.len() - ev.start_sample {
            continue;
        }
        let tail = n.min(8);
        let mut prev = 1.;
        for (k, &reference) in ev.entry_rms.iter().enumerate() {
            let start = ev.start_sample + k * h;
            let block = &mix[start..start + h];
            let frac = reference / ev.peak_rms;
            let mut strength = ((0.20 - frac) / 0.08).clamp(0., 1.);
            if k >= n - tail && tail > 1 {
                strength *= (n - 1 - k) as f64 / (tail - 1) as f64;
            }
            let desired = h as f64
                * ((1. - strength) * rms(block).powi(2) + strength * (frac * scale).powi(2));
            let (mut a, mut b, mut c) = (0., 0., 0.);
            for (i, &y) in block.iter().enumerate() {
                let u = ramp_position(i);
                let p = (1. - u) * prev;
                a += sq(y) * u * u;
                b += sq(y) * u * p;
                c += sq(y) * p * p;
            }
            let end = if a < 1e-14 || strength == 0. {
                1.
            } else {
                (((b * b + a * (desired - c)).max(0.).sqrt() - b) / a).clamp(0.22, 5.)
            };
            for (j, g) in gain[start..start + h].iter_mut().enumerate() {
                let u = ramp_position(j);
                *g = (prev * (1. - u) + end * u) as f32;
            }
            prev = end;
        }
    }
    Ok(gain)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_attacks_guard_pre_echo_without_modifying_entrance() {
        let mut reference = vec![0.; 48_000];
        reference[4800..9600].fill(0.3);
        reference[24000..27000].fill(0.2);
        let evidence = measure_quiet_attacks(&reference, 48_000).unwrap();
        assert_eq!(
            evidence.iter().map(|e| e.start_sample).collect::<Vec<_>>(),
            [4800, 24000]
        );
        let mut render = reference;
        render[4000..4800].fill(0.1);
        let gain = pre_attack_gain(&render, &evidence).unwrap();
        assert!(gain[4200] < 0.001);
        assert!(gain[4800..9600].iter().all(|&g| g == 1.));
        assert!(gain.iter().all(|&g| (0. ..=1.).contains(&g)));
    }
    #[test]
    fn quiet_entry_is_shaped_without_touching_full_note() {
        let mut reference = vec![0.; 12_000];
        reference[4800..5184].fill(0.04);
        reference[5184..9000].fill(0.3);
        let evidence = measure_quiet_attacks(&reference, 48_000).unwrap();
        assert_eq!(evidence.len(), 1);
        let mut render = reference;
        render[4800..5184].fill(0.015);
        let gain = quiet_entry_gain(&render, &evidence).unwrap();
        assert!(gain[5100] > 2.);
        assert!(gain[6000..].iter().all(|&g| g == 1.));
    }
}
