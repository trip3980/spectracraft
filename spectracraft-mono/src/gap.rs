//! Explicit, bounded loss repair. This copies unaffected PCM and is deliberately
//! separate from fresh performance synthesis. All channels accept or decline together.
use crate::Result;
use std::path::Path;
fn power(x: &[f64]) -> f64 {
    x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64
}
fn solve(mut a: Vec<Vec<f64>>, b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    // Cholesky for the symmetric positive definite regularized normal equations.
    for i in 0..n {
        for j in 0..=i {
            let s = a[i][j] - (0..j).map(|k| a[i][k] * a[j][k]).sum::<f64>();
            if i == j {
                if !s.is_finite() || s <= 0. {
                    return None;
                }
                a[i][j] = s.sqrt();
            } else {
                a[i][j] = s / a[j][j];
            }
        }
    }
    let mut x = b.to_vec();
    for i in 0..n {
        x[i] = (x[i] - (0..i).map(|j| a[i][j] * x[j]).sum::<f64>()) / a[i][i];
    }
    for i in (0..n).rev() {
        x[i] = (x[i] - (i + 1..n).map(|j| a[j][i] * x[j]).sum::<f64>()) / a[i][i];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}
fn fit(left: &[f64], right: &[f64], p: usize) -> Option<Vec<f64>> {
    if left.len() <= p || right.len() <= p {
        return None;
    }
    let mut c = vec![0.; p + 1];
    for (lag, v) in c.iter_mut().enumerate() {
        for side in [left, right] {
            *v += side[lag..]
                .iter()
                .zip(side)
                .map(|(a, b)| a * b)
                .sum::<f64>();
        }
    }
    if c[0] < 1e-16 {
        return None;
    }
    c[0] *= 1.00003;
    let matrix = (0..p)
        .map(|i| (0..p).map(|j| c[i.abs_diff(j)]).collect())
        .collect();
    let w = solve(matrix, &c[1..])?;
    Some(
        std::iter::once(1.)
            .chain(w.into_iter().map(|v| -v))
            .collect(),
    )
}
fn interpolate(left: &[f64], right: &[f64], a: &[f64], n: usize) -> Option<Vec<f64>> {
    let p = a.len() - 1;
    if left.len() < p || right.len() < p {
        return None;
    }
    let base: Vec<_> = left[left.len() - p..]
        .iter()
        .copied()
        .chain(vec![0.; n])
        .chain(right[..p].iter().copied())
        .collect();
    let mut rhs = vec![0.; n];
    let mut normal = vec![vec![0.; n]; n];
    for row in 0..n + p {
        let t = p + row;
        let residual = (0..=p)
            .filter_map(|k| base.get(t - k).map(|v| a[k] * v))
            .sum::<f64>();
        let lo = row.saturating_sub(p);
        let hi = row.min(n - 1);
        for i in lo..=hi {
            let v = a[row - i];
            rhs[i] -= v * residual;
            for j in lo..=hi {
                normal[i][j] += v * a[row - j];
            }
        }
    }
    for (i, row) in normal.iter_mut().enumerate() {
        row[i] += 1e-8;
    }
    solve(normal, &rhs)
}
/// Interleaved mono/stereo, frame indices, exclusive end. Declines return an exact copy.
pub fn fill(
    audio: &[f32],
    rate: u32,
    channels: usize,
    start: usize,
    end: usize,
    known_loss: bool,
) -> (Vec<f32>, &'static str) {
    let decline = |why| (audio.to_vec(), why);
    if !(8000..=96000).contains(&rate)
        || !(1..=2).contains(&channels)
        || audio.len() % channels != 0
    {
        return decline("requires interleaved mono/stereo at 8–96 kHz");
    }
    let n = end.saturating_sub(start);
    let fs = rate as f64;
    if !known_loss
        || end <= start
        || n < (0.00025 * fs).round() as usize
        || n > (0.004 * fs).round() as usize
    {
        return decline("needs an explicitly identified 0.25–4 ms loss");
    }
    let p = ((0.0015 * fs).round() as usize).min(96);
    let context = (0.048 * fs).round() as usize;
    if start < context || end > audio.len() / channels || context > audio.len() / channels - end {
        return decline("insufficient context");
    }
    if audio[(start - context) * channels..(end + context) * channels]
        .iter()
        .any(|v| !v.is_finite())
    {
        return decline("nonfinite input");
    }
    let mut repairs = Vec::new();
    for ch in 0..channels {
        let take = |a: usize, b: usize| {
            (a..b)
                .map(|i| audio[i * channels + ch] as f64)
                .collect::<Vec<_>>()
        };
        let l = take(start - context, start);
        let r = take(end, end + context);
        let inside = take(start, end);
        let (lp, rp) = (power(&l), power(&r));
        if lp.max(rp) < 1e-8 || lp.min(rp) < 0.16 * lp.max(rp) {
            return decline("the sides disagree in level or are quiet");
        }
        if power(&inside) > 0.06 * lp.min(rp) {
            return decline("audio is already present in selected patch");
        }
        let Some(a) = fit(&l, &r, p) else {
            return decline("no usable neighboring signal");
        };
        let mut errs = Vec::new();
        for side in [&l, &r] {
            for frac in [0.35, 0.66] {
                let at = (side.len() as f64 * frac).round() as usize;
                let Some(test) = fit(&side[..at], &side[at + n..], p) else {
                    return decline("weak adjacent landmarks");
                };
                let Some(z) = interpolate(&side[..at], &side[at + n..], &test, n) else {
                    return decline("unsolved adjacent patch");
                };
                let err: Vec<_> = z
                    .iter()
                    .zip(&side[at..at + n])
                    .map(|(a, b)| a - b)
                    .collect();
                errs.push(power(&err) / power(&side[at..at + n]).max(1e-16));
            }
        }
        errs.sort_by(f64::total_cmp);
        if errs[3] > 0.65 || (errs[1] + errs[2]) * 0.5 > 0.30 {
            return decline("adjacent patches are not predictable");
        }
        let Some(z) = interpolate(&l, &r, &a, n) else {
            return decline("unsolved target patch");
        };
        let peak = l.iter().chain(&r).map(|v| v.abs()).fold(0., f64::max);
        if z.iter()
            .any(|v| !v.is_finite() || v.abs() > 0.98_f64.min(1.08 * peak))
        {
            return decline("predicted peak exceeds local headroom");
        }
        repairs.push(z);
    }
    let mut out = audio.to_vec();
    for (ch, z) in repairs.iter().enumerate() {
        for (i, &v) in z.iter().enumerate() {
            out[(start + i) * channels + ch] = v as f32;
        }
    }
    (out, "filled")
}
pub fn repair_wav(input: &Path, output: &Path, start: usize, end: usize) -> Result<()> {
    let mut reader = hound::WavReader::open(input)?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Float
        || spec.bits_per_sample != 32
        || !(1..=2).contains(&spec.channels)
        || reader.duration() > spec.sample_rate.saturating_mul(600)
    {
        return Err("Expected float32 mono/stereo WAV, at most 600 seconds".into());
    }
    let x = reader
        .samples::<f32>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let (y, status) = fill(
        &x,
        spec.sample_rate,
        spec.channels as usize,
        start,
        end,
        true,
    );
    if status != "filled" {
        return Err(format!("No output: {status}").into());
    }
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut w = hound::WavWriter::new(std::io::BufWriter::new(f), spec)?;
    for v in y {
        w.write_sample(v)?;
    }
    w.finalize()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stereo_repair_preserves_outside_and_improves_missing_tones() {
        for rate in [8000, 44100, 48000, 96000] {
            let clean: Vec<_> = (0..rate)
                .flat_map(|i| {
                    [
                        0.21 * (std::f64::consts::TAU * 1300. * i as f64 / rate as f64).sin()
                            as f32,
                        0.125
                            * (std::f64::consts::TAU * 1700. * i as f64 / rate as f64 + 0.45).sin()
                                as f32,
                    ]
                })
                .collect();
            let a = rate as usize / 2;
            let b = a + (rate as f64 * 0.002).round() as usize;
            let mut broken = clean.clone();
            broken[a * 2..b * 2].fill(0.);
            let (y, status) = fill(&broken, rate, 2, a, b, true);
            assert_eq!(status, "filled", "{rate}");
            assert_eq!(&y[..a * 2], &broken[..a * 2]);
            assert_eq!(&y[b * 2..], &broken[b * 2..]);
            let err: f64 = y[a * 2..b * 2]
                .iter()
                .zip(&clean[a * 2..b * 2])
                .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
                .sum();
            let before: f64 = clean[a * 2..b * 2]
                .iter()
                .map(|v| (*v as f64).powi(2))
                .sum();
            assert!(err < before * 0.05, "{rate}: {err}/{before}");
            assert_eq!(fill(&broken, rate, 2, a, b, false).0, broken);
            assert_eq!(fill(&clean, rate, 2, a, b, true).0, clean);
            broken[a * 2 - 1] = 0.;
            broken.iter_mut().skip(1).step_by(2).for_each(|v| *v = 0.);
            assert_eq!(fill(&broken, rate, 2, a, b, true).0, broken);
        }
    }
    #[test]
    fn two_boundary_solution_satisfies_normal_equations() {
        let a = [1., -0.7, 0.12];
        let l = [0.2, 0.5];
        let r = [-0.3, 0.1];
        let z = interpolate(&l, &r, &a, 5).unwrap();
        let x: Vec<_> = l.into_iter().chain(z.iter().copied()).chain(r).collect();
        let residual: Vec<_> = (2..9)
            .map(|t| (0..3).map(|k| a[k] * x[t - k]).sum::<f64>())
            .collect();
        for i in 0..5 {
            let grad = (0..3).map(|k| a[k] * residual[i + k]).sum::<f64>() + 1e-8 * z[i];
            assert!(grad.abs() < 1e-12);
        }
    }
    #[test]
    fn malformed_ranges_decline_without_panicking() {
        for (s, e) in [(0, 1), (usize::MAX, 0), (1, usize::MAX)] {
            assert_eq!(fill(&[0.; 16], 48000, 1, s, e, true).0, vec![0.; 16]);
        }
        assert_eq!(fill(&[0.; 16], 48000, 0, 1, 2, true).0, vec![0.; 16]);
    }
}

#[cfg(test)]
mod independent_oracle {
    use super::*;
    #[test]
    fn cholesky_patch_matches_independent_householder_least_squares() {
        for n in [6, 17, 48] {
            let p = 4;
            let left: Vec<_> = (0..80).map(|i| (i as f64 * 0.17).sin()).collect();
            let right: Vec<_> = (80 + n..160 + n).map(|i| (i as f64 * 0.17).sin()).collect();
            let coefficients = fit(&left, &right, p).unwrap();
            let actual = interpolate(&left, &right, &coefficients, n).unwrap();
            let base: Vec<_> = left[left.len() - p..]
                .iter()
                .copied()
                .chain(vec![0.; n])
                .chain(right[..p].iter().copied())
                .collect();
            let mut a = vec![vec![0.; n]; n + p + n];
            let mut b = vec![0.; a.len()];
            for row in 0..n + p {
                for k in 0..=p {
                    let index = p + row - k;
                    if (p..p + n).contains(&index) {
                        a[row][index - p] = coefficients[k];
                    } else {
                        b[row] -= coefficients[k] * base[index];
                    }
                }
            }
            for j in 0..n {
                a[n + p + j][j] = 1e-4;
            }
            // Householder QR on the original augmented design, no A^T A.
            for k in 0..n {
                let norm = a[k..].iter().map(|r| r[k] * r[k]).sum::<f64>().sqrt();
                let mut v: Vec<_> = a[k..].iter().map(|r| r[k]).collect();
                v[0] += norm.copysign(v[0]);
                let scale = v.iter().map(|v| v * v).sum::<f64>();
                for j in k..n {
                    let projection = 2.
                        * v.iter()
                            .enumerate()
                            .map(|(i, v)| v * a[k + i][j])
                            .sum::<f64>()
                        / scale;
                    for (i, v) in v.iter().enumerate() {
                        a[k + i][j] -= projection * v;
                    }
                }
                let projection =
                    2. * v.iter().enumerate().map(|(i, v)| v * b[k + i]).sum::<f64>() / scale;
                for (i, v) in v.iter().enumerate() {
                    b[k + i] -= projection * v;
                }
            }
            let mut expected = vec![0.; n];
            for i in (0..n).rev() {
                expected[i] =
                    (b[i] - (i + 1..n).map(|j| a[i][j] * expected[j]).sum::<f64>()) / a[i][i];
            }
            assert!(actual
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 1e-8));
        }
    }
}
