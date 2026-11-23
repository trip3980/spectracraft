//! Explicit capture experiments, separate from performance synthesis. These
//! operate on the archived generated take and make no hardware-identification claim.
use crate::{dsp, metrics, qa, Result};
use rustfft::{num_complex::Complex, FftPlanner};
use std::path::Path;
fn norm(v: [f64; 3]) -> f64 {
    v.iter().map(|v| v * v).sum::<f64>().sqrt()
}
fn geometry() -> ([f64; 3], [f64; 3], [f64; 3]) {
    (
        [15. * 0.3048, 14. * 0.3048, 5. * 0.3048],
        [15. * 0.3048, 20. * 0.3048, 4. * 0.3048],
        [30. * 0.3048; 3],
    )
}
pub fn convolve(x: &[f64], h: &[f64]) -> Vec<f64> {
    let len = x.len() + h.len() - 1;
    let n = len.next_power_of_two();
    let mut a = vec![Complex::new(0., 0.); n];
    let mut b = a.clone();
    for (a, x) in a.iter_mut().zip(x) {
        a.re = *x;
    }
    for (b, h) in b.iter_mut().zip(h) {
        b.re = *h;
    }
    let mut planner = FftPlanner::new();
    let forward = planner.plan_fft_forward(n);
    forward.process(&mut a);
    forward.process(&mut b);
    for (a, b) in a.iter_mut().zip(b) {
        *a *= b;
    }
    planner.plan_fft_inverse(n).process(&mut a);
    a[..len].iter().map(|v| v.re / n as f64).collect()
}
pub fn meter(x: &[f64], rate: u32) -> Result<(f64, f64)> {
    let mut m = ebur128::EbuR128::new(1, rate, ebur128::Mode::I | ebur128::Mode::TRUE_PEAK)?;
    m.add_frames_f64(x)?;
    let l = m.loudness_global()?;
    let peak = 20. * m.true_peak(0)?.max(1e-20).log10();
    if !l.is_finite() {
        return Err("No gated loudness for silent/short input".into());
    }
    Ok((l, peak))
}
pub fn first_returns(mic: [f64; 3], strength: f64, omni: bool) -> (Vec<f64>, Vec<f64>) {
    let (source, _, room) = geometry();
    let direct = norm(std::array::from_fn(|i| source[i] - mic[i]));
    let aim: [f64; 3] = std::array::from_fn(|i| (source[i] - mic[i]) / direct);
    let mut h = vec![0.; 1 + (norm(room.map(|v| v * 2.)) * 44100. / 343.).ceil() as usize];
    h[0] = 1.;
    let mut delays = Vec::new();
    for axis in 0..3 {
        for face in [0., 1.] {
            let mut image = source;
            image[axis] = 2. * face * room[axis] - source[axis];
            let path: [f64; 3] = std::array::from_fn(|i| image[i] - mic[i]);
            let length = norm(path);
            let delay = (length - direct) * 44100. / 343.;
            let k = delay as usize;
            let f = delay - k as f64;
            let g = strength * direct / length
                * if omni {
                    1.
                } else {
                    0.5 + 0.5
                        * aim
                            .iter()
                            .zip(path)
                            .map(|(a, b)| a * b / length)
                            .sum::<f64>()
                };
            h[k] += g * (1. - f);
            h[k + 1] += g * f;
            delays.push(delay / 44.1);
        }
    }
    (h, delays)
}
pub fn metal_ir(wall: f64) -> Result<Vec<f64>> {
    if !(0. ..1.).contains(&wall) {
        return Err("Wall pressure gain must be below one".into());
    }
    let (s, m, l) = geometry();
    let direct = norm(std::array::from_fn(|i| s[i] - m[i]));
    let aim: [f64; 3] = std::array::from_fn(|i| (s[i] - m[i]) / direct);
    let rmax = direct + 343. * 1.5;
    let bound = (rmax / l[0]).ceil() as i32 + 2;
    let mut h = vec![0.; 66152];
    h[0] = 1.;
    for ix in -bound..=bound {
        for iy in -bound..=bound {
            for iz in -bound..=bound {
                let index = [ix, iy, iz];
                let bounces = index.iter().map(|v| v.abs()).sum::<i32>();
                if bounces == 0 {
                    continue;
                }
                let path: [f64; 3] = std::array::from_fn(|i| {
                    index[i] as f64 * l[i] + if index[i] % 2 == 0 { s[i] } else { l[i] - s[i] }
                        - m[i]
                });
                let r = norm(path);
                if r > rmax {
                    continue;
                }
                let delay = (r - direct) * 44100. / 343.;
                let k = delay as usize;
                if k + 1 >= h.len() {
                    continue;
                }
                let fraction = delay - k as f64;
                let pickup = 0.5 + 0.5 * path.iter().zip(aim).map(|(a, b)| a * b / r).sum::<f64>();
                let gain = direct / r
                    * pickup
                    * wall.powi(bounces)
                    * ((1.5 - delay / 44100.) / 0.2).clamp(0., 1.);
                h[k] += gain * (1. - fraction);
                h[k + 1] += gain * fraction;
            }
        }
    }
    Ok(h)
}
/// Frequency-sampled Type-I Hamming FIR with the same 1025 taps/2049 grid.
pub fn panel_ir(inches: usize, omni: bool) -> Result<(Vec<f64>, Vec<f64>)> {
    let alpha = match inches {
        1 => [0.11, 0.11, 0.28, 0.68, 0.90, 0.93, 0.96, 0.96],
        2 => [0.17, 0.17, 0.86, 1.14, 1.07, 1.02, 0.98, 0.98],
        _ => return Err("Panel must be one or two inches".into()),
    };
    let bands = [0., 125., 250., 500., 1000., 2000., 4000., 22050.];
    let gains = alpha.map(|v: f64| (1. - v.clamp(0., 0.99)).sqrt());
    let n = 4096;
    let mut z = vec![Complex::new(0., 0.); n];
    for k in 0..=n / 2 {
        let hz = k as f64 * 44100. / n as f64;
        let i = bands.partition_point(|v| *v <= hz).saturating_sub(1).min(6);
        let gain =
            gains[i] + (gains[i + 1] - gains[i]) * (hz - bands[i]) / (bands[i + 1] - bands[i]);
        z[k] = Complex::from_polar(gain, -std::f64::consts::TAU * k as f64 * 512. / n as f64);
        if k > 0 && k < n / 2 {
            z[n - k] = z[k].conj();
        }
    }
    FftPlanner::new().plan_fft_inverse(n).process(&mut z);
    let wall: Vec<_> = (0..1025)
        .map(|i| {
            z[i].re / n as f64 * (0.54 - 0.46 * (std::f64::consts::TAU * i as f64 / 1024.).cos())
        })
        .collect();
    let (_, mic, _) = geometry();
    let (impulse, delays) = first_returns(mic, 1., omni);
    let mut h = vec![0.; 4096];
    h[0] = 1.;
    for (k, &gain) in impulse.iter().enumerate().skip(1) {
        if gain == 0. {
            continue;
        }
        for j in 0..513 {
            h[k + j] += gain * wall[512 + j] * if j == 0 { 1. } else { 2. };
        }
    }
    Ok((h, delays))
}
fn fixture(path: &Path) -> Result<(Vec<f64>, Vec<f64>)> {
    let (s, x) = qa::read_audio(path)?;
    if s.sample_rate != 44100
        || s.channels != 1
        || x.len() != 557892
        || x[256896..300996].iter().any(|v| *v != 0.)
    {
        return Err("Unexpected capture fixture".into());
    }
    Ok((
        x[..256896].iter().map(|v| *v as f64).collect(),
        x[300996..].iter().map(|v| *v as f64).collect(),
    ))
}
fn write_wav(path: &Path, x: &[f64]) -> Result<()> {
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut w = hound::WavWriter::new(
        f,
        hound::WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for &v in x {
        w.write_sample(v as f32)?;
    }
    w.finalize()?;
    Ok(())
}
fn matched_export(
    folder: &Path,
    names: &[&str],
    waves: &[Vec<f64>],
    margin: f64,
) -> Result<serde_json::Value> {
    let meters = waves
        .iter()
        .map(|x| meter(x, 44100))
        .collect::<Result<Vec<_>>>()?;
    let target = meters
        .iter()
        .map(|(l, p)| l - p - margin)
        .fold(f64::INFINITY, f64::min);
    let mut clips = Vec::new();
    let mut rows = Vec::new();
    for ((name, wave), (l, p)) in names.iter().zip(waves).zip(meters) {
        let gain = 10_f64.powf((target - l) / 20.);
        let y: Vec<_> = wave.iter().map(|v| v * gain).collect();
        let (l2, p2) = meter(&y, 44100)?;
        if (l2 - target).abs() > 0.11 || p2 > -margin + 0.02 {
            return Err("Loudness/true-peak matching gate failed".into());
        }
        write_wav(&folder.join(format!("{name}-raw.wav")), wave)?;
        write_wav(&folder.join(format!("{name}-audition.wav")), &y)?;
        if !clips.is_empty() {
            clips.extend(vec![0.; 44100]);
        }
        clips.extend(y);
        rows.push(serde_json::json!({"name":name,"raw_lufs":l,"raw_dbtp":p,"audition_lufs":l2,"audition_dbtp":p2,"gain":gain}));
    }
    write_wav(&folder.join("comparison.wav"), &clips)?;
    Ok(
        serde_json::json!({"target_lufs":target,"clips":rows,"meter":"ebur128 0.1.10 Rust; no external executable"}),
    )
}
pub fn capture_experiments(input: &Path, out: &Path) -> Result<serde_json::Value> {
    let bytes = std::fs::read(input)?;
    let (_, take) = fixture(input)?;
    std::fs::create_dir(out)?;
    let mut report = serde_json::Map::new();
    let first = 882;
    let mut rng = dsp::Random::new(25);
    let mut h = vec![0.; 22050];
    h[0] = 1.;
    for (i, v) in h.iter_mut().enumerate().skip(first) {
        let t = (i - first) as f64 / 44100.;
        *v = rng.normal()
            * (-t / 0.070).exp()
            * ((0.48 - t) / 0.035).clamp(0., 1.)
            * (t / 0.006).clamp(0., 1.);
    }
    h[first] = 0.2;
    let norm = h[first..].iter().map(|v| v * v).sum::<f64>().sqrt();
    for v in &mut h[first..] {
        *v *= 10_f64.powf(-1. / 20.) / norm;
    }
    let room = convolve(&take, &h);
    let mut dry = take.clone();
    dry.resize(room.len(), 0.);
    if dry[..first]
        .iter()
        .zip(&room[..first])
        .any(|(a, b)| (a - b).abs() > 1e-12)
    {
        return Err("Direct arrival changed".into());
    }
    let dir = out.join("virtual");
    std::fs::create_dir(&dir)?;
    report.insert(
        "virtual".into(),
        matched_export(&dir, &["dry", "room"], &[dry, room], 2.)?,
    );
    let dir = out.join("metal");
    std::fs::create_dir(&dir)?;
    let mut waves = Vec::new();
    let mut energies = Vec::new();
    for wall in [0.15, 0.96] {
        let h = metal_ir(wall)?;
        energies.push(10. * h[1..].iter().map(|v| v * v).sum::<f64>().log10());
        waves.push(convolve(&take, &h));
    }
    report.insert("metal".into(),serde_json::json!({"reflection_energy_db":energies,"audition":matched_export(&dir,&["treated","metal"],&waves,3.)?}));
    let dir = out.join("panels");
    std::fs::create_dir(&dir)?;
    let mut waves = Vec::new();
    let mut delays = Vec::new();
    for (inches, omni) in [(1, false), (1, true), (2, false)] {
        let (h, d) = panel_ir(inches, omni)?;
        delays.push(d);
        waves.push(convolve(&take, &h));
    }
    if delays[0] != delays[1] || delays[0] != delays[2] {
        return Err("Panel treatment moved reflections".into());
    }
    report.insert("panels".into(),serde_json::json!({"fixed_delays_ms":delays[0],"audition":matched_export(&dir,&["cardioid-1in","omni-1in","cardioid-2in"],&waves,3.)?}));
    if bytes != std::fs::read(input)? {
        return Err("Source changed".into());
    }
    report.insert("source_unchanged".into(), true.into());
    Ok(report.into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fir_and_loudness_controls() {
        let a = vec![1., 2., 3.];
        let b = vec![0.5, -0.25];
        let y = convolve(&a, &b);
        for (a, b) in y.iter().zip([0.5, 0.75, 1., -0.75]) {
            assert!((a - b).abs() < 1e-12);
        }
        let (h, a) = panel_ir(1, false).unwrap();
        let (g, b) = panel_ir(2, true).unwrap();
        assert_eq!(a, b);
        assert_eq!(h[0], 1.);
        assert_eq!(g[0], 1.);
        assert_ne!(h, g);
        let x: Vec<_> = (0..132300)
            .map(|i| 0.1 * (std::f64::consts::TAU * 1000. * i as f64 / 44100.).sin())
            .collect();
        let (l, p) = meter(&x, 44100).unwrap();
        let (m, q) = meter(&x.iter().map(|v| v * 0.5).collect::<Vec<_>>(), 44100).unwrap();
        assert!((l - m - 6.0205999).abs() < 1e-5);
        assert!((p - q - 6.0205999).abs() < 1e-5);
    }
}

pub fn reference_match(input: &Path, out: &Path) -> Result<serde_json::Value> {
    let bytes = std::fs::read(input)?;
    let (reference, take) = fixture(input)?;
    let n = take.len();
    let floats = |x: &[f64]| x.iter().map(|v| *v as f32).collect::<Vec<_>>();
    let (f, t, a) = metrics::stft(&floats(&reference), 44100, 2048, 256);
    let (_, _, b) = metrics::stft(&floats(&take), 44100, 2048, 256);
    let db = |v: f64| 20. * v.max(1e-4).log10();
    let mask: Vec<Vec<_>> = a
        .iter()
        .zip(&b)
        .map(|(a, b)| {
            a.iter()
                .zip(b)
                .enumerate()
                .map(|(k, (a, b))| f[k] >= 80. && f[k] <= 14000. && db(*a).max(db(*b)) > -62.)
                .collect()
        })
        .collect();
    let lo = (0.55 * 44100.) as usize;
    let hi = (2.48 * 44100.) as usize;
    let target = reference[lo..hi].iter().map(|v| v * v).sum::<f64>().sqrt();
    let fit = |mut y: Vec<f64>| {
        y.truncate(n);
        let gain = target / y[lo..hi].iter().map(|v| v * v).sum::<f64>().sqrt();
        for v in &mut y {
            *v *= gain;
        }
        y
    };
    let score = |y: &[f64], start: f64, end: f64| {
        let (_, _, s) = metrics::stft(&floats(y), 44100, 2048, 256);
        let (mut error, mut count) = (0., 0);
        for i in 0..t.len() {
            if t[i] < start || t[i] >= end {
                continue;
            }
            for k in 0..f.len() {
                if mask[i][k] {
                    error += (db(s[i][k]) - db(a[i][k])).abs();
                    count += 1;
                }
            }
        }
        error / count as f64
    };
    let baseline = fit(take.clone());
    let baseline_score = score(&baseline, 0.55, 2.48);
    let mut candidates = vec![(baseline_score, None, baseline, Vec::new())];
    let (source, _, _) = geometry();
    for dx in [-3., 0., 3.] {
        for dy in [1., 2., 4., 6.] {
            for dz in [-1., 0., 2.] {
                for strength in [0.05, 0.15] {
                    let mic = std::array::from_fn(|i| source[i] + [dx, dy, dz][i] * 0.3048);
                    let (h, delays) = first_returns(mic, strength, false);
                    let y = fit(convolve(&take, &h));
                    let training = score(&y, 0.55, 2.48);
                    candidates.push((training, Some([dx, dy, dz, strength]), y, delays));
                }
            }
        }
    }
    // Select by training score only. Hidden second-note values are evaluated below.
    let best = (0..candidates.len())
        .min_by(|&i, &j| candidates[i].0.total_cmp(&candidates[j].0))
        .unwrap();
    let reflected = (1..candidates.len())
        .min_by(|&i, &j| candidates[i].0.total_cmp(&candidates[j].0))
        .unwrap();
    let correlation = |a: &[f64], b: &[f64]| {
        let ma = a.iter().sum::<f64>() / a.len() as f64;
        let mb = b.iter().sum::<f64>() / b.len() as f64;
        let num = a
            .iter()
            .zip(b)
            .map(|(a, b)| (a - ma) * (b - mb))
            .sum::<f64>();
        num / (a.iter().map(|v| (v - ma).powi(2)).sum::<f64>()
            * b.iter().map(|v| (v - mb).powi(2)).sum::<f64>())
        .sqrt()
    };
    let row = |i: usize| {
        let q = &candidates[i];
        let lo = (3.48 * 44100.) as usize;
        let hi = (5.53 * 44100.) as usize;
        let levels = |x: &[f64]| x.chunks_exact(882).map(dsp::rms).collect::<Vec<_>>();
        serde_json::json!({"training_error_db":q.0,"heldout_error_db":score(&q.2,3.48,5.53),"relative_mic_xyz_ft_and_assumed_wall_amplitude":q.1,"heldout_sample_correlation":correlation(&reference[lo..hi],&q.2[lo..hi]),"heldout_20ms_level_correlation":correlation(&levels(&reference[lo..hi]),&levels(&q.2[lo..hi])),"reflection_delay_ms":q.3})
    };
    let [dx, dy, dz, strength] = candidates[reflected].1.unwrap();
    let (_, _, base_spec) = metrics::stft(&floats(&candidates[reflected].2), 44100, 2048, 256);
    let mut changes = serde_json::Map::new();
    for (name, offset, g, omni) in [
        ("wall_return_0.15_to_0.05", [dx, dy, dz], 0.05, false),
        ("mic_height_up_1ft", [dx, dy, dz + 1.], strength, false),
        (
            "mic_distance_6ft_to_2ft_forward",
            [dx, 2., dz],
            strength,
            false,
        ),
        ("ideal_cardioid_to_omni", [dx, dy, dz], strength, true),
    ] {
        let (h, delays) = first_returns(
            std::array::from_fn(|i| source[i] + offset[i] * 0.3048),
            g,
            omni,
        );
        let y = fit(convolve(&take, &h));
        let (_, _, s) = metrics::stft(&floats(&y), 44100, 2048, 256);
        let (mut delta, mut count) = (0., 0);
        for i in 0..t.len() {
            if t[i] >= 3.48 && t[i] < 5.53 {
                for k in 0..f.len() {
                    if mask[i][k] {
                        delta += (db(s[i][k]) - db(base_spec[i][k])).abs();
                        count += 1;
                    }
                }
            }
        }
        changes.insert(name.into(),serde_json::json!({"heldout_error_to_original_db":score(&y,3.48,5.53),"spectral_change_vs_local_reference_db":delta/count as f64,"floor_delay_ms":delays[4],"ceiling_delay_ms":delays[5]}));
    }
    std::fs::create_dir(out)?;
    write_wav(&out.join("matched.wav"), &candidates[best].2)?;
    let result = serde_json::json!({"candidate_count":candidates.len(),"baseline":row(0),"best_training_candidate":row(best),"best_reflection_candidate":row(reflected),"training_time":[0.55,2.48],"holdout_time":[3.48,5.53],"one_factor_changes":changes,"measured_km84_response_used":false,"source_unchanged":std::fs::read(input)?==bytes});
    qa::write_new(
        &out.join("metrics.json"),
        serde_json::to_string_pretty(&result)?.as_bytes(),
    )?;
    Ok(result)
}

pub fn capture_figures(root: &Path) -> Result<serde_json::Value> {
    let mut files = Vec::new();
    for (group, names, views) in [
        (
            "virtual",
            vec!["dry", "room"],
            vec![(0., 6.32), (5.23, 6.32)],
        ),
        (
            "metal",
            vec!["treated", "metal"],
            vec![(0., 7.33), (5., 7.25)],
        ),
        (
            "panels",
            vec!["cardioid-1in", "omni-1in", "cardioid-2in"],
            vec![(0., 5.92)],
        ),
    ] {
        let mut waves = Vec::new();
        for name in &names {
            waves.push(qa::read_audio(&root.join(group).join(format!("{name}-audition.wav")))?.1);
        }
        let signals: Vec<_> = names
            .iter()
            .zip(&waves)
            .map(|(n, v)| (*n, &v[..]))
            .collect();
        let output = root.join(group).join("rms-comparison.svg");
        qa::write_new(
            &output,
            crate::figures::rms_plot(&signals, 44100, &views, 256896. / 44100.)?.as_bytes(),
        )?;
        files.push(output.display().to_string());
        if group == "metal" {
            let panels: Vec<_> = signals.iter().map(|(n, x)| (*n, *x, 5., 7.25)).collect();
            let output = root.join(group).join("spectral-tail.svg");
            qa::write_new(
                &output,
                crate::figures::heatmap(&panels, 44100, (-75., -20.), &[])?.as_bytes(),
            )?;
            files.push(output.display().to_string());
        }
    }
    // Published absorption and measured modeled response share numeric data;
    // export CSV alongside the SVG-free plots for exact reproducibility.
    let mut csv = String::from("frequency_hz,cardioid_1in_db,omni_1in_db,cardioid_2in_db\n");
    let responses = [
        panel_ir(1, false)?.0,
        panel_ir(1, true)?.0,
        panel_ir(2, false)?.0,
    ];
    use std::fmt::Write;
    for hz in (80..=1500).step_by(2) {
        write!(csv, "{hz}")?;
        for h in &responses {
            let z = h
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    Complex::from_polar(*v, -std::f64::consts::TAU * hz as f64 * i as f64 / 44100.)
                })
                .sum::<Complex<f64>>();
            write!(csv, ",{:.9}", 20. * z.norm().max(1e-20).log10())?;
        }
        csv.push('\n');
    }
    qa::write_new(&root.join("panels/response.csv"), csv.as_bytes())?;
    Ok(serde_json::json!({"figures":files,"response_data":"panels/response.csv"}))
}

pub fn response_figure(out: &Path) -> Result<()> {
    let alpha = [
        vec![0.11, 0.28, 0.68, 0.90, 0.93, 0.96],
        vec![0.17, 0.86, 1.14, 1.07, 1.02, 0.98],
    ];
    let absorption = alpha
        .iter()
        .enumerate()
        .map(|(i, a)| {
            (
                format!("{} inch published absorption", i + 1),
                [125., 250., 500., 1000., 2000., 4000.]
                    .into_iter()
                    .zip(a)
                    .map(|(x, y)| [x, *y])
                    .collect(),
            )
        })
        .collect();
    let mut responses = Vec::new();
    for (name, inches, omni) in [
        ("Cardioid 1 inch", 1, false),
        ("Omni 1 inch", 1, true),
        ("Cardioid 2 inch", 2, false),
    ] {
        let (h, _) = panel_ir(inches, omni)?;
        let points = (80..=1500)
            .step_by(2)
            .map(|hz| {
                let z = h
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        Complex::from_polar(
                            *v,
                            -std::f64::consts::TAU * hz as f64 * i as f64 / 44100.,
                        )
                    })
                    .sum::<Complex<f64>>();
                [hz as f64, 20. * z.norm().log10()]
            })
            .collect();
        responses.push((name.into(), points));
    }
    let svg = crate::figures::line_panels(&[
        (
            "Published absorption (unclamped source data)",
            absorption,
            [125., 4000., 0., 1.2],
        ),
        (
            "Modeled capture response relative to direct, dB",
            responses,
            [80., 1500., -8., 8.],
        ),
    ])?;
    qa::write_new(out, svg.as_bytes())
}
pub fn reference_figures(input: &Path, folder: &Path) -> Result<()> {
    let (reference, _) = fixture(input)?;
    let reference: Vec<_> = reference.iter().map(|v| *v as f32).collect();
    let (_, matched) = qa::read_audio(&folder.join("matched.wav"))?;
    let svg = crate::figures::heatmap(
        &[
            ("Original recording", &reference, 0., 256896. / 44100.),
            (
                "Best training capture candidate",
                &matched,
                0.,
                256896. / 44100.,
            ),
        ],
        44100,
        (-75., -18.),
        &[],
    )?;
    qa::write_new(&folder.join("spectral-comparison.svg"), svg.as_bytes())?;
    let svg = crate::figures::rms_plot(
        &[
            ("Original", &reference),
            ("Training-selected match", &matched),
        ],
        44100,
        &[(0., 256896. / 44100.)],
        3.48,
    )?;
    qa::write_new(&folder.join("levels.svg"), svg.as_bytes())
}
