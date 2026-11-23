//! Frozen numerical acceptance measurements; these are not listening verdicts.
use crate::{dsp, Result};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::Serialize;
pub fn welch(x: &[f32], rate: u32, segment: usize) -> Result<(Vec<f64>, Vec<f64>)> {
    if rate == 0 || x.is_empty() || x.iter().any(|v| !v.is_finite()) || segment < 2 {
        return Err("Invalid Welch input".into());
    }
    let n = segment.min(x.len());
    let window: Vec<_> = (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())
        .collect();
    let sum: f64 = window.iter().sum();
    let fft = FftPlanner::new().plan_fft_forward(n);
    let mut power = vec![0.; n / 2 + 1];
    let mut count = 0;
    for start in (0..=x.len() - n).step_by((n / 2).max(1)) {
        let mean = x[start..start + n].iter().map(|v| *v as f64).sum::<f64>() / n as f64;
        let mut z: Vec<_> = x[start..start + n]
            .iter()
            .zip(&window)
            .map(|(v, w)| Complex::new((*v as f64 - mean) * w, 0.))
            .collect();
        fft.process(&mut z);
        for (k, p) in power.iter_mut().enumerate() {
            *p += z[k].norm_sqr() / sum.powi(2)
                * if k == 0 || n % 2 == 0 && k == n / 2 {
                    1.
                } else {
                    2.
                };
        }
        count += 1;
    }
    for p in &mut power {
        *p /= count as f64;
    }
    Ok((
        (0..power.len())
            .map(|k| k as f64 * rate as f64 / n as f64)
            .collect(),
        power,
    ))
}
pub fn envelope(x: &[f32], h: usize) -> Vec<f64> {
    x.chunks_exact(h)
        .map(|w| (w.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / h as f64).sqrt())
        .collect()
}
fn onset(e: &[f64], threshold: f64) -> Option<usize> {
    let max = e.iter().copied().fold(0., f64::max);
    if max == 0. {
        None
    } else {
        e.iter().position(|v| *v >= threshold * max)
    }
}
fn db(a: f64, b: f64) -> f64 {
    10. * (a.max(1e-20) / b.max(1e-20)).log10()
}
#[derive(Debug, Serialize)]
pub struct FrozenScore {
    pub metrics: std::collections::BTreeMap<String, f64>,
    pub failed: Vec<String>,
}
pub fn frozen_score(x: &[f32], y: &[f32], rate: u32, tonal: bool) -> Result<FrozenScore> {
    if x.len() != y.len()
        || !(16000..=96000).contains(&rate)
        || x.iter().chain(y).any(|v| !v.is_finite())
    {
        return Err("Invalid comparison signals".into());
    }
    let h = (0.001 * rate as f64).round_ties_even() as usize;
    let (e, r) = (envelope(x, h), envelope(y, h));
    let mut metrics = std::collections::BTreeMap::new();
    let mut failed = Vec::new();
    let Some(on) = onset(&e, 0.05) else {
        return Ok(FrozenScore {
            metrics,
            failed: if onset(&r, 0.05).is_none() {
                vec![]
            } else {
                vec!["silent_reference".into()]
            },
        });
    };
    let Some(on_y) = onset(&r, 0.05) else {
        return Ok(FrozenScore {
            metrics,
            failed: vec!["missing_render".into()],
        });
    };
    metrics.insert("attack_ms".into(), on_y as f64 - on as f64);
    let a = on + 30;
    let b = e.len();
    if a + 2 > b {
        return Err("Too little post-attack data for frozen tail score".into());
    }
    let mid = (a + b) / 2;
    let ratio = |z: &[f64]| {
        db(
            z[mid..b].iter().map(|v| v * v).sum::<f64>() / (b - mid) as f64,
            z[a..mid].iter().map(|v| v * v).sum::<f64>() / (mid - a) as f64,
        )
    };
    metrics.insert("tail_shape_db".into(), ratio(&r) - ratio(&e));
    if tonal {
        let n = 8192.min(x.len() - on * h);
        let (f, a) = welch(&x[on * h..], rate, n)?;
        let (_, b) = welch(&y[on * h..], rate, n)?;
        let k = (0..f.len())
            .filter(|&i| f[i] >= 40. && f[i] < 5000.)
            .max_by(|&i, &j| a[i].total_cmp(&a[j]))
            .ok_or("No peak-score bins")?;
        let width = 9_f64.max(1.5 * f[1]);
        let sum_a: f64 = a.iter().sum();
        let sum_b: f64 = b.iter().sum();
        let band_a: f64 = (0..f.len())
            .filter(|&i| (f[i] - f[k]).abs() <= width)
            .map(|i| a[i])
            .sum();
        let band_b: f64 = (0..f.len())
            .filter(|&i| (f[i] - f[k]).abs() <= width)
            .map(|i| b[i])
            .sum();
        metrics.insert(
            "peak_share_db".into(),
            db(band_b / sum_b.max(1e-20), band_a / sum_a.max(1e-20)),
        );
    }
    for (name, value) in &metrics {
        let limit = if name == "attack_ms" { 5. } else { 3. };
        if value.abs() > limit {
            failed.push(name.clone());
        }
    }
    Ok(FrozenScore { metrics, failed })
}
pub fn peaks(x: &[f32], rate: u32) -> Result<serde_json::Value> {
    let (f, p) = welch(x, rate, 8192.min(x.len()))?;
    let mut indices = dsp::peaks(&p);
    indices.sort_by(|&i, &j| p[j].total_cmp(&p[i]));
    indices.truncate(5);
    Ok(
        serde_json::json!({"rms":(x.iter().map(|v|(*v as f64).powi(2)).sum::<f64>()/x.len() as f64).sqrt(),"peaks_hz_dbfs":indices.into_iter().map(|i|[f[i],10.*p[i].max(1e-24).log10()]).collect::<Vec<_>>()}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_gates_detect_timing_and_tonal_mismatch() {
        let rate = 48000;
        let x: Vec<_> = (0..48000)
            .map(|i| {
                if i < 4800 {
                    0.
                } else {
                    (0.2 * (-((i - 4800) as f64) / 18000.).exp()
                        * (std::f64::consts::TAU * 440. * i as f64 / rate as f64).sin())
                        as f32
                }
            })
            .collect();
        assert!(frozen_score(&x, &x, rate, true).unwrap().failed.is_empty());
        let mut delayed = vec![0.; x.len()];
        delayed[480..].copy_from_slice(&x[..x.len() - 480]);
        assert!(frozen_score(&x, &delayed, rate, true)
            .unwrap()
            .failed
            .contains(&"attack_ms".into()));
        let y: Vec<_> = x
            .iter()
            .enumerate()
            .map(|(i, v)| v * (std::f64::consts::TAU * 800. * i as f64 / rate as f64).cos() as f32)
            .collect();
        assert!(frozen_score(&x, &y, rate, true)
            .unwrap()
            .failed
            .contains(&"peak_share_db".into()));
    }
}

pub fn landmarks(x: &[f32], rate: u32) -> std::collections::BTreeMap<String, Option<f64>> {
    let h = (0.002 * rate as f64).round_ties_even() as usize;
    let e = envelope(x, h);
    let mut out = std::collections::BTreeMap::new();
    for (note, (a, b)) in [(0.40, 2.62), (3.23, 5.65)].into_iter().enumerate() {
        let ids: Vec<_> = (0..e.len())
            .filter(|&i| {
                let t = (i as f64 + 0.5) * h as f64 / rate as f64;
                t >= a && t <= b
            })
            .collect();
        if ids.is_empty() {
            continue;
        }
        let smooth: Vec<_> = (0..ids.len())
            .map(|j| {
                (-2..=2)
                    .filter_map(|d| {
                        (j as isize + d)
                            .try_into()
                            .ok()
                            .and_then(|k: usize| ids.get(k))
                            .map(|&i| e[i])
                    })
                    .sum::<f64>()
                    / 5.
            })
            .collect();
        let peak = smooth.iter().copied().fold(0., f64::max);
        for pct in [10, 50] {
            let active: Vec<_> = (0..ids.len())
                .filter(|&j| peak > 0. && smooth[j] >= peak * pct as f64 / 100.)
                .collect();
            let time = |j: usize| 1000. * (ids[j] as f64 + 0.5) * h as f64 / rate as f64;
            out.insert(
                format!("note_{}_entry_{}pct_ms", note + 1, pct),
                active.first().map(|&j| time(j)),
            );
            if pct == 10 {
                out.insert(
                    format!("note_{}_release_10pct_ms", note + 1),
                    active.last().map(|&j| time(j)),
                );
            }
        }
    }
    out
}
pub fn stft(x: &[f32], rate: u32, n: usize, hop: usize) -> (Vec<f64>, Vec<f64>, Vec<Vec<f64>>) {
    let window: Vec<_> = (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())
        .collect();
    let sum: f64 = window.iter().sum();
    let fft = FftPlanner::new().plan_fft_forward(n);
    let starts: Vec<_> = (0..=x.len().div_ceil(hop)).map(|i| i * hop).collect();
    let frames = starts
        .iter()
        .map(|&s| {
            let mut z: Vec<_> = dsp::centered(x, s, n)
                .iter()
                .zip(&window)
                .map(|(v, w)| Complex::new(v * w, 0.))
                .collect();
            fft.process(&mut z);
            z[..n / 2 + 1].iter().map(|v| v.norm() / sum).collect()
        })
        .collect();
    (
        (0..n / 2 + 1)
            .map(|k| k as f64 * rate as f64 / n as f64)
            .collect(),
        starts.iter().map(|&s| s as f64 / rate as f64).collect(),
        frames,
    )
}
pub fn criteria(
    x: &[f32],
    y: &[f32],
    packet: &crate::harmonic::Packet,
) -> Result<serde_json::Value> {
    if x.len() != y.len() || x.len() != packet.samples {
        return Err("Criteria length mismatch".into());
    }
    let rate = packet.rate;
    let fitted = crate::harmonic::analyze(y, rate, packet.profile)?;
    let cents: Vec<_> = (0..packet.pitch.len())
        .filter(|&i| packet.voiced[i] && fitted.voiced[i])
        .map(|i| (1200. * (fitted.pitch[i] / packet.pitch[i]).log2()).abs())
        .collect();
    let mut sorted = cents.clone();
    sorted.sort_by(f64::total_cmp);
    let p95 = if sorted.is_empty() {
        None
    } else {
        Some(dsp::interp(&sorted, (sorted.len() - 1) as f64 * 0.95))
    };
    let voiced = packet.voiced.iter().filter(|v| **v).count();
    let (f, t, a) = stft(x, rate, 1024, 128);
    let (_, _, b) = stft(y, rate, 1024, 128);
    let mut portions = serde_json::Map::new();
    for (name, start, end) in [
        ("first held note", 0.8, 2.45),
        ("second held note", 3.7, 5.35),
        ("breath and first entry", 0.39, 0.75),
        ("gap and second entry", 2.7, 3.55),
        ("tail", 5.48, 5.8),
    ] {
        let mut source = 0.;
        let mut render = 0.;
        let mut error = 0.;
        let mut texture_source = 0.;
        let mut texture_render = 0.;
        let mut ea = Vec::new();
        let mut eb = Vec::new();
        for (i, &time) in t.iter().enumerate() {
            if time < start || time >= end {
                continue;
            }
            let pitch = dsp::interp(&packet.pitch, time * rate as f64 / 128.);
            let v = packet.voiced[i.min(packet.voiced.len() - 1)];
            let (mut sa, mut sb) = (0., 0.);
            for (k, &hz) in f.iter().enumerate() {
                let (x, y) = (a[i][k], b[i][k]);
                sa += x * x;
                sb += y * y;
                error += (y - x).powi(2);
                if (3000. ..16000.).contains(&hz)
                    && (!v || (1..10).all(|h| (hz - h as f64 * pitch).abs() > 180.))
                {
                    texture_source += x * x;
                    texture_render += y * y;
                }
            }
            source += sa;
            render += sb;
            ea.push(sa.sqrt());
            eb.push(sb.sqrt());
        }
        let correlation = if ea.len() > 1 {
            let ma = ea.iter().sum::<f64>() / ea.len() as f64;
            let mb = eb.iter().sum::<f64>() / eb.len() as f64;
            let num = ea
                .iter()
                .zip(&eb)
                .map(|(a, b)| (a - ma) * (b - mb))
                .sum::<f64>();
            let den = (ea.iter().map(|v| (v - ma).powi(2)).sum::<f64>()
                * eb.iter().map(|v| (v - mb).powi(2)).sum::<f64>())
            .sqrt();
            (den > 0.).then(|| num / den)
        } else {
            None
        };
        portions.insert(name.into(),serde_json::json!({"level_db":db(render,source),"off_harmonic_3_16_khz_level_db":db(texture_render,texture_source),"magnitude_error_db_relative_to_reference":db(error,source),"envelope_correlation":correlation}));
    }
    let source = landmarks(x, rate);
    let rendered = landmarks(y, rate);
    let delta: std::collections::BTreeMap<_, _> = source
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value
                    .zip(rendered.get(key).copied().flatten())
                    .map(|(a, b)| b - a),
            )
        })
        .collect();
    Ok(
        serde_json::json!({"timing_marker_error_ms":delta,"pitch_cents_median_abs":(!cents.is_empty()).then(||dsp::median(&cents)),"pitch_cents_p95_abs":p95,"voiced_coverage_pct":(voiced>0).then(||100.*cents.len() as f64/voiced as f64),"portions":portions,"peak":y.iter().map(|v|v.abs()).fold(0.,f32::max)}),
    )
}

pub fn verify_corpus_reports(
    prior: &serde_json::Value,
    independent: &serde_json::Value,
    six: &serde_json::Value,
) -> Result<serde_json::Value> {
    let mut checked = 0;
    let mut failures = Vec::new();
    let mut check_seeds = |label: &str, row: &serde_json::Value| -> Result<()> {
        let seeds = row["seeds"].as_object().ok_or("Missing seed metrics")?;
        if seeds.len() != 3 {
            return Err("Expected exactly three render seeds".into());
        }
        for seed in ["734", "927", "1229"] {
            let r = seeds.get(seed).ok_or("Missing frozen seed")?;
            let m = r["metrics"].as_object().ok_or("Missing metric map")?;
            for key in ["attack_ms", "tail_shape_db"] {
                if !m.contains_key(key) {
                    return Err("Missing frozen acceptance metric".into());
                }
            }
            if !label.contains("example-002") && !m.contains_key("peak_share_db") {
                return Err("Missing tonal peak-share metric".into());
            }
            for (key, value) in m {
                let v = value.as_f64().ok_or("Invalid metric number")?;
                let limit = match key.as_str() {
                    "attack_ms" => 5.,
                    "tail_shape_db" | "peak_share_db" => 3.,
                    _ => return Err("Unexpected frozen metric".into()),
                };
                if v.abs() > limit {
                    failures.push(format!("{label}/{seed}/{key}: {v}"));
                }
            }
            let recorded = r["failed"].as_array().ok_or("Missing failed-gate list")?;
            if !recorded.is_empty() {
                failures.push(format!("{label}/{seed}: recorded failure {recorded:?}"));
            }
            checked += 1;
        }
        Ok(())
    };
    for group in ["prior", "holdout"] {
        let rows = prior[group].as_object().ok_or("Missing benchmark group")?;
        if rows.len() != 8 {
            return Err("Expected eight clips per prior/holdout group".into());
        }
        for (label, row) in rows {
            check_seeds(label, row)?;
        }
    }
    let rows = independent
        .as_object()
        .ok_or("Missing independent report")?;
    if rows.len() != 4 {
        return Err("Expected four independent clips".into());
    }
    for label in ["Bass A1", "Cello C3", "Brass C4", "Piano C4"] {
        check_seeds(label, rows.get(label).ok_or("Missing independent voice")?)?;
    }
    let mut regressions = 0;
    for group in ["bottles", "hurdy", "glass"] {
        let rows = six[group]
            .as_array()
            .ok_or("Missing native regression group")?;
        if rows.len() != 2 {
            return Err("Expected two samples per native regression group".into());
        }
        for row in rows {
            let rate = row["native_rate"]
                .as_u64()
                .ok_or("Missing native rate")?
                .to_string();
            let checks = row["checks"].as_object().ok_or("Missing native checks")?;
            for key in [rate.as_str(), "44100"] {
                let q = checks.get(key).ok_or("Missing native/44.1 kHz check")?;
                for (field, limit) in [
                    ("attack_10pct_error_ms", 5.),
                    (
                        "rebuilt_power_change_at_reference_peak_db_relative_to_total",
                        2.,
                    ),
                ] {
                    let v = q[field].as_f64().ok_or("Missing regression metric")?;
                    if v.abs() > limit {
                        failures.push(format!("{group}/{key}/{field}: {v}"));
                    }
                }
                if q["passed"] != true {
                    failures.push(format!("{group}/{key}: recorded regression"));
                }
            }
            regressions += 1;
        }
    }
    Ok(
        serde_json::json!({"renders":checked,"clips":20,"native_regressions":regressions,"failures":failures,"passed":failures.is_empty()}),
    )
}
pub fn verify_flute_report(report: &serde_json::Value) -> Result<serde_json::Value> {
    let mut failures = Vec::new();
    let metric = |row: &serde_json::Value, part: &str| -> Result<f64> {
        row["portions"][part]["magnitude_error_db_relative_to_reference"]
            .as_f64()
            .ok_or("Missing flute magnitude score".into())
    };
    let before = &report["before"];
    for seed in ["734", "927", "1229"] {
        let row = &report["seeds"][seed];
        if metric(row, "tail")? >= metric(before, "tail")? - 3. {
            failures.push(format!("{seed}: tail improvement below 3 dB"));
        }
        for part in ["first held note", "second held note"] {
            if (metric(row, part)? - metric(before, part)?).abs() >= 0.1 {
                failures.push(format!("{seed}: held-note drift {part}"));
            }
        }
        for (key, value) in row["timing_marker_error_ms"]
            .as_object()
            .ok_or("Missing timing markers")?
        {
            if value.as_f64().ok_or("Missing flute timing value")?.abs() > 4.1 {
                failures.push(format!("{seed}: timing {key}"));
            }
        }
    }
    if report["breath_onsets"]
        .as_array()
        .ok_or("Missing breath evidence")?
        .len()
        != 2
    {
        failures.push("Expected two breath gestures".into());
    }
    if report["accent_unchanged_outside_gesture"] != true {
        failures.push("Accent changed samples outside gesture".into());
    }
    if report["accent_full_excerpt_rms_delta_db"]
        .as_f64()
        .ok_or("Missing accent level")?
        .abs()
        >= 0.15
    {
        failures.push("Accent whole-clip level drift".into());
    }
    if report["source_unchanged"] != true {
        failures.push("Source preservation not verified".into());
    }
    Ok(serde_json::json!({"passed":failures.is_empty(),"failures":failures}))
}
