//! Prefix-only modal release prediction. Hidden future samples never enter fit.
use crate::{dsp, Result};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mode {
    pub hz: f64,
    pub amplitude_at_cut: f64,
    pub decay_time_s: f64,
    pub fit_db: f64,
}
pub fn extract(prefix: &[f32], rate: u32) -> Result<Vec<Mode>> {
    if !(16000..=96000).contains(&rate) || prefix.iter().any(|v| !v.is_finite()) {
        return Err("Invalid continuation prefix".into());
    }
    let l = (0.064 * rate as f64).round() as usize;
    let w = (0.016 * rate as f64).round() as usize;
    if prefix.len() < 2 * l {
        return Ok(vec![]);
    }
    let prev: Vec<_> = prefix[prefix.len() - l..]
        .iter()
        .map(|v| *v as f64)
        .collect();
    if prev.iter().map(|v| v.abs()).fold(0., f64::max) < 2e-5
        || dsp::rms(&prev[l - w..]) > 0.85 * dsp::rms(&prev[..w])
    {
        return Ok(vec![]);
    }
    let window = dsp::hann(l);
    let sum: f64 = window.iter().sum();
    let fft = 4 * l.next_power_of_two();
    let mut spectrum = dsp::spectrum(&prev, &window, fft);
    for v in &mut spectrum {
        *v *= 2. / sum;
    }
    let mut peaks: Vec<_> = dsp::peaks(&spectrum)
        .into_iter()
        .filter(|&k| (125. ..=12000.).contains(&(k as f64 * rate as f64 / fft as f64)))
        .collect();
    peaks.sort_by(|&i, &j| spectrum[j].total_cmp(&spectrum[i]));
    if peaks.is_empty() {
        return Ok(vec![]);
    }
    let taper = dsp::hann(w);
    let taper_sum: f64 = taper.iter().sum();
    let times: Vec<_> = (0..9).map(|i| -0.048 + 0.005 * i as f64).collect();
    let mut modes: Vec<Mode> = Vec::new();
    for &k in &peaks {
        if modes.len() >= 10 || spectrum[k] < 2.5e-5_f64.max(spectrum[peaks[0]] * 0.018) {
            break;
        }
        if spectrum[k]
            < 4. * dsp::median(&spectrum[k.saturating_sub(75)..(k + 76).min(spectrum.len())])
        {
            continue;
        }
        let hz = dsp::refined_bin(&spectrum, k) * rate as f64 / fft as f64;
        if modes.iter().any(|m| (hz - m.hz).abs() < 150.) {
            continue;
        }
        let mut amps = Vec::new();
        for &time in &times {
            let end =
                (prefix.len() as isize + (time * rate as f64).round() as isize + w as isize / 2)
                    .min(prefix.len() as isize) as usize;
            let (re, im) = (0..w).fold((0., 0.), |(re, im), i| {
                let phase = TAU * hz * i as f64 / rate as f64;
                let v = prefix[end - w + i] as f64 * taper[i];
                (re + v * phase.cos(), im - v * phase.sin())
            });
            amps.push(re.hypot(im) * 2. / taper_sum);
        }
        let max = amps.iter().copied().fold(0., f64::max);
        let good: Vec<_> = (0..amps.len())
            .filter(|&i| amps[i] > (max * 0.03).max(1.5e-5))
            .collect();
        if good.len() < 6 || !good.contains(&(amps.len() - 1)) {
            continue;
        }
        let levels: Vec<_> = amps.iter().map(|v| 20. * v.max(1e-12).log10()).collect();
        let mut slopes = Vec::new();
        for (a, &i) in good.iter().enumerate() {
            for &j in good.iter().skip(a + 1) {
                slopes.push((levels[j] - levels[i]) / (times[j] - times[i]));
            }
        }
        let slope = dsp::median(&slopes);
        let offset = dsp::median(
            &good
                .iter()
                .map(|&i| levels[i] - slope * times[i])
                .collect::<Vec<_>>(),
        );
        let err = (good
            .iter()
            .map(|&i| (levels[i] - offset - slope * times[i]).powi(2))
            .sum::<f64>()
            / good.len() as f64)
            .sqrt();
        if !(-1900. ..-32.).contains(&slope)
            || err > 5.5
            || slope * (times.last().unwrap() - times[0]) > -3.
        {
            continue;
        }
        let amplitude = 10_f64.powf(offset / 20.);
        if amplitude > 1.45 * amps.last().unwrap() {
            continue;
        }
        modes.push(Mode {
            hz,
            amplitude_at_cut: amplitude,
            decay_time_s: -20. / (slope * 10_f64.ln()),
            fit_db: err,
        });
    }
    Ok(modes)
}
pub fn generate(
    modes: &[Mode],
    rate: u32,
    seconds: f64,
    seed: u64,
    decay: bool,
) -> Result<Vec<f32>> {
    if !(16000..=96000).contains(&rate)
        || !seconds.is_finite()
        || !(0. ..=1.).contains(&seconds)
        || modes.len() > 10
        || modes.iter().any(|m| {
            !m.hz.is_finite()
                || !(0. ..rate as f64 / 2.).contains(&m.hz)
                || !m.amplitude_at_cut.is_finite()
                || !(0. ..=4.).contains(&m.amplitude_at_cut)
                || !m.decay_time_s.is_finite()
                || !(0.0001..=10.).contains(&m.decay_time_s)
        })
    {
        return Err("Invalid continuation model".into());
    }
    let mut rng = dsp::ReferenceRandom::new(seed);
    let mut y = vec![0.; (seconds * rate as f64).round() as usize];
    for mode in modes {
        let phase = rng.uniform() * TAU;
        for (i, v) in y.iter_mut().enumerate() {
            let t = i as f64 / rate as f64;
            let amp = mode.amplitude_at_cut
                * if decay {
                    (-t / mode.decay_time_s).exp()
                } else {
                    1.
                };
            *v += (amp * (TAU * mode.hz * t + phase).sin()) as f32;
        }
    }
    Ok(y)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn damped_modes_are_fitted_from_prefix_and_sustain_is_declined() {
        let rate = 44100;
        let x: Vec<_> = (0..rate)
            .map(|i| {
                let t = i as f64 / rate as f64;
                let env = if t >= 0.5 {
                    (-(t - 0.5) / 0.045).exp()
                } else {
                    1.
                };
                (env * (0.12 * (TAU * 560. * t).sin() + 0.07 * (TAU * 1110. * t + 0.2).cos()))
                    as f32
            })
            .collect();
        assert!(extract(&x[..(0.4 * rate as f64) as usize], rate)
            .unwrap()
            .is_empty());
        let modes = extract(&x[..(0.58 * rate as f64) as usize], rate).unwrap();
        assert_eq!(modes.len(), 2);
        assert!(modes.iter().all(|m| (m.decay_time_s - 0.045).abs() < 0.006));
        let a = generate(&modes, rate, 0.12, 714, true).unwrap();
        let b = generate(&modes, rate, 0.12, 911, true).unwrap();
        assert_ne!(a, b);
        assert_eq!(a, generate(&modes, rate, 0.12, 714, true).unwrap());
        assert!(a.iter().all(|v| v.is_finite()));
        let cut = (0.58 * rate as f64) as usize;
        let score = crate::qa::continuation_score(&x[cut..cut + a.len()], &a, rate).unwrap();
        assert!(
            score["magnitude_error_relative_to_silence_db"]
                .as_f64()
                .unwrap()
                < -20.
        );
    }
}
