//! Rust-only engineering measurements. These do not certify perceived quality.
use crate::Result;
use rustfft::{num_complex::Complex, FftPlanner};
use serde::Serialize;
use std::{io::Write, path::Path};
#[derive(Serialize)]
pub struct StereoMetrics {
    pub frames: usize,
    pub sample_peak_dbfs: Option<f64>,
    pub lr_normalized_dot: Option<f64>,
    pub mono_vs_pooled_channel_power_db: Option<f64>,
}
pub fn stereo(x: &[f32]) -> Result<StereoMetrics> {
    if x.is_empty() || x.len() % 2 != 0 || x.iter().any(|v| !v.is_finite()) {
        return Err("Expected finite interleaved stereo".into());
    }
    let (mut l, mut r, mut cross, mut mono, mut peak) = (0., 0., 0., 0., 0_f64);
    for pair in x.chunks_exact(2) {
        let (a, b) = (pair[0] as f64, pair[1] as f64);
        l += a * a;
        r += b * b;
        cross += a * b;
        mono += ((a + b) * 0.5).powi(2);
        peak = peak.max(a.abs()).max(b.abs());
    }
    Ok(StereoMetrics {
        frames: x.len() / 2,
        sample_peak_dbfs: (peak > 0.).then(|| 20. * peak.log10()),
        lr_normalized_dot: (l * r > 0.).then(|| cross / (l * r).sqrt()),
        mono_vs_pooled_channel_power_db: (mono > 0. && l + r > 0.)
            .then(|| 10. * (2. * mono / (l + r)).log10()),
    })
}
pub fn pre_onset_ratio(x: &[f32], onset: usize, pre: usize, attack: usize) -> Result<Option<f64>> {
    if onset < pre
        || attack == 0
        || pre == 0
        || onset.checked_add(attack).is_none_or(|n| n > x.len())
        || x.iter().any(|v| !v.is_finite())
    {
        return Err("Invalid onset window".into());
    }
    let energy = |v: &[f32]| v.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
    let (a, b) = (
        energy(&x[onset - pre..onset]),
        energy(&x[onset..onset + attack]),
    );
    Ok((a > 0. && b > 0.).then(|| 10. * (a / b).log10()))
}
#[derive(Debug, Serialize)]
pub struct ImpulseMetrics {
    pub direct_to_reflected_db: Option<f64>,
    pub first_reflection_ms: Option<f64>,
}
pub fn impulse(h: &[f32], rate: u32) -> Result<ImpulseMetrics> {
    if h.is_empty() || rate == 0 || h.iter().any(|v| !v.is_finite()) {
        return Err("Invalid impulse response".into());
    }
    let energy = h[1..].iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
    Ok(ImpulseMetrics {
        direct_to_reflected_db: (energy > 0. && h[0] != 0.)
            .then(|| 10. * ((h[0] as f64).powi(2) / energy).log10()),
        first_reflection_ms: h[1..]
            .iter()
            .position(|v| *v != 0.)
            .map(|i| (i + 1) as f64 * 1000. / rate as f64),
    })
}
pub fn read_audio(path: &Path) -> Result<(hound::WavSpec, Vec<f32>)> {
    let mut w = hound::WavReader::open(path)?;
    let s = w.spec();
    if !(1..=2).contains(&s.channels)
        || !(8000..=96000).contains(&s.sample_rate)
        || w.duration() > s.sample_rate * 600
    {
        return Err("Expected mono/stereo 8–96 kHz, at most 600 seconds".into());
    }
    let x = match s.sample_format {
        hound::SampleFormat::Float => w
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 2_f32.powi(s.bits_per_sample as i32 - 1);
            w.samples::<i32>()
                .map(|v| v.map(|v| v as f32 / scale))
                .collect::<std::result::Result<Vec<_>, _>>()?
        }
    };
    if x.is_empty() || x.len() % s.channels as usize != 0 || x.iter().any(|v| !v.is_finite()) {
        return Err("Invalid audio samples".into());
    }
    Ok((s, x))
}
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut w = std::io::BufWriter::new(f);
    w.write_all(bytes)?;
    w.flush()?;
    Ok(())
}
pub fn six_note() -> (Vec<i16>, serde_json::Value) {
    let fs = 48000;
    let mut out = vec![0i16; 4 * fs];
    let mut notes = Vec::new();
    for (i, (midi, velocity)) in [60, 62, 64, 65, 67, 69]
        .into_iter()
        .zip([0.50, 0.75, 0.62, 0.90, 0.67, 0.80])
        .enumerate()
    {
        let start = fs / 4 + i * fs / 2;
        let duration = (fs as f64 * if i < 5 { 0.42 } else { 1.10 }) as usize;
        let hz = 440. * 2_f64.powf((midi as f64 - 69.) / 12.);
        notes.push(serde_json::json!({"midi":midi,"frequency_hz":hz,"onset_frame":start,"relative_strength":velocity}));
        for n in 0..duration {
            let t = n as f64 / fs as f64;
            let env =
                1_f64.min(n as f64 / 160.).min((duration - n) as f64 / 480.) * (-5. * t).exp();
            let partials = [0.64, 0.21, 0.10, 0.05]
                .into_iter()
                .enumerate()
                .map(|(k, a)| {
                    let k = (k + 1) as f64;
                    a * (std::f64::consts::TAU * hz * k * (1. + 0.00015 * k * k) * t).sin()
                        * (-k * t).exp()
                })
                .sum::<f64>();
            out[start + n] = (24000. * velocity * env * partials).round_ties_even() as i16;
        }
    }
    let meta = serde_json::json!({"sample_rate":fs,"channels":1,"frames":out.len(),"peak_pcm":out.iter().map(|v|v.abs()).max(),"notes":notes});
    (out, meta)
}
/// A bounded spectrogram with one fixed amplitude scale (-104 to -29 dBFS).
/// Rust produces SVG directly; no plotting interpreter or browser code is used.
pub fn spectrogram_svg(x: &[f32], rate: u32) -> Result<String> {
    if !(8000..=96000).contains(&rate) || x.is_empty() || x.iter().any(|v| !v.is_finite()) {
        return Err("Invalid spectrogram input".into());
    }
    let n = 1024;
    let columns = x.len().div_ceil(128).min(800);
    let rows = 128;
    let fft = FftPlanner::new().plan_fft_forward(n);
    let win: Vec<_> = (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (n - 1) as f64).cos())
        .collect();
    let sum: f64 = win.iter().sum();
    let max_hz = 8000_f64.min(rate as f64 / 2.);
    let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"440\" viewBox=\"0 0 960 440\"><title>Mono spectrogram</title><rect width=\"960\" height=\"440\" fill=\"#101522\"/><g fill=\"#ffffff\" font-family=\"sans-serif\" font-size=\"16\"><text x=\"60\" y=\"28\">Spectrogram · fixed scale −104 to −29 dBFS</text><text x=\"60\" y=\"420\">0 to {:.3} seconds · 0 to {:.0} Hz (bottom to top)</text></g>",x.len() as f64/rate as f64,max_hz);
    for col in 0..columns {
        let center = col * x.len() / columns;
        let mut buf: Vec<_> = (0..n)
            .map(|i| {
                Complex::new(
                    (center + i)
                        .checked_sub(n / 2)
                        .and_then(|j| x.get(j))
                        .copied()
                        .unwrap_or(0.) as f64
                        * win[i],
                    0.,
                )
            })
            .collect();
        fft.process(&mut buf);
        for row in 0..rows {
            let lo = ((row as f64 / rows as f64 * max_hz * n as f64 / rate as f64).floor()
                as usize)
                .min(n / 2);
            let hi = (((row + 1) as f64 / rows as f64 * max_hz * n as f64 / rate as f64).ceil()
                as usize)
                .min(n / 2)
                .max(lo);
            let amp = buf[lo..=hi]
                .iter()
                .map(|v| 2. * v.norm() / sum)
                .fold(0., f64::max);
            let u = ((20. * amp.max(1e-12).log10() + 104.) / 75.).clamp(0., 1.);
            let (r, g, b) = (
                (12. + 243. * u) as u8,
                (15. + 220. * u * u) as u8,
                (26. + 143. * u) as u8,
            );
            svg.push_str(&format!("<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"2.7\" fill=\"#{r:02x}{g:02x}{b:02x}\"/>",60.+col as f64*840./columns as f64,48.+(rows-1-row) as f64*2.65,840./columns as f64+0.01));
        }
    }
    svg.push_str("</svg>");
    Ok(svg)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn planted_pre_echo_is_gain_invariant() {
        let mut x = vec![0.; 96000];
        for i in 0..480 {
            let v = (std::f64::consts::TAU * 3000. * i as f64 / 48000.).sin() as f32;
            x[48000 + i] = v;
            x[47040 + i] = v * 0.1;
        }
        let a = pre_onset_ratio(&x, 48000, 960, 480).unwrap().unwrap();
        assert!((a + 20.).abs() < 1e-6);
        for v in &mut x {
            *v *= 0.5;
        }
        assert!((pre_onset_ratio(&x, 48000, 960, 480).unwrap().unwrap() - a).abs() < 1e-10);
        x[..48000].fill(0.);
        assert_eq!(pre_onset_ratio(&x, 48000, 960, 480).unwrap(), None);
    }
    #[test]
    fn stereo_and_silence_have_explicit_undefined_metrics() {
        let m = stereo(&[1., -1., -1., 1.]).unwrap();
        assert_eq!(m.lr_normalized_dot, Some(-1.));
        assert_eq!(m.mono_vs_pooled_channel_power_db, None);
        let s = stereo(&[0.; 8]).unwrap();
        assert_eq!(s.sample_peak_dbfs, None);
        assert!(serde_json::to_string(&s).is_ok());
        assert!(stereo(&[f32::NAN, 0.]).is_err());
    }
    #[test]
    fn known_ir_level_and_delay_are_independent() {
        for delay in [960, 1920] {
            for db in [0., 12.] {
                let mut h = vec![0.; 24000];
                h[0] = 1.;
                h[delay] = 10_f32.powf(-db / 20.);
                for g in [0.1, 0.7, 1.] {
                    let v: Vec<_> = h.iter().map(|v| v * g).collect();
                    let m = impulse(&v, 48000).unwrap();
                    assert!((m.direct_to_reflected_db.unwrap() - db as f64).abs() < 1e-5);
                    assert_eq!(m.first_reflection_ms, Some(delay as f64 / 48.));
                }
            }
        }
        assert_eq!(
            impulse(&[1., 0.], 48000).unwrap().direct_to_reflected_db,
            None
        );
    }
    #[test]
    fn electrical_response_matches_analytic_control() {
        let mut h = vec![Complex::new(0_f64, 0.); 8];
        h[0].re = 0.75;
        h[1].re = 0.25;
        FftPlanner::new().plan_fft_forward(8).process(&mut h);
        for k in [0, 1, 2, 4] {
            let expected = (0.75_f64.powi(2)
                + 0.25_f64.powi(2)
                + 2. * 0.75 * 0.25 * (std::f64::consts::TAU * k as f64 / 8.).cos())
            .sqrt();
            assert!((h[k].norm() - expected).abs() < 1e-12);
        }
    }
    #[test]
    fn fixture_matches_original_recipe_and_silence_bounds() {
        let (x, m) = six_note();
        assert_eq!(x.len(), 192000);
        assert!(x[..12000].iter().all(|v| *v == 0));
        assert!(x[184800..].iter().all(|v| *v == 0));
        assert_eq!(m["notes"].as_array().unwrap().len(), 6);
        assert_eq!(six_note().0, x);
    }
}

/// Replays the retired local-gap corpus and fixed stress grids without decoding
/// through external tools. Targets are erased before fitting; originals score only.
pub fn gap_corpus(root: &Path) -> Result<serde_json::Value> {
    let flute_path = root.join("Iowa flute - original then regenerated.wav");
    let vocal_path = root.join("vocals.wav");
    let flute_bytes = std::fs::read(&flute_path)?;
    let vocal_bytes = std::fs::read(&vocal_path)?;
    let (sf, f) = read_audio(&flute_path)?;
    let (sv, v) = read_audio(&vocal_path)?;
    if sf.sample_rate != 44100
        || sf.channels != 1
        || f.len() < 256896
        || sv.channels != 2
        || sv.sample_rate != 48000
        || v.len() < 60 * 48000 * 2
    {
        return Err("Retired corpus source layout does not match expected fixtures".into());
    }
    let f = &f[..256896];
    let vocal = &v[40 * 48000 * 2..50 * 48000 * 2];
    let stress_v = &v[50 * 48000 * 2..60 * 48000 * 2];
    let mut rows = Vec::new();
    let mut measure = |name: &str,
                       x: &[f32],
                       rate: u32,
                       ch: usize,
                       time: f64,
                       ms: f64,
                       mode: &str|
     -> Result<()> {
        let a = (time * rate as f64).round_ties_even() as usize;
        let b = a + (ms * rate as f64 / 1000.).round_ties_even() as usize;
        if b > x.len() / ch {
            return Err("Corpus window exceeds source".into());
        }
        let mut broken = x.to_vec();
        if mode != "intact" {
            broken[a * ch..b * ch].fill(0.);
        }
        let (y, status) = crate::gap::fill(&broken, rate, ch, a, b, mode != "unmarked");
        if y[..a * ch] != broken[..a * ch]
            || y[b * ch..] != broken[b * ch..]
            || (status != "filled" && y != broken)
        {
            return Err("Gap preservation regression".into());
        }
        let energy = |z: &[f32]| z.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
        let before = energy(
            &broken[a * ch..b * ch]
                .iter()
                .zip(&x[a * ch..b * ch])
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>(),
        );
        let after = energy(
            &y[a * ch..b * ch]
                .iter()
                .zip(&x[a * ch..b * ch])
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>(),
        );
        rows.push(serde_json::json!({"case":name,"time_s":time,"ms":ms,"channels":ch,"decision":status,"error_reduction_db":(before>1e-16).then(||10.*(before/after.max(1e-16)).log10())}));
        Ok(())
    };
    for t in [0.45, 0.8, 1.2, 1.55, 3.2, 4.2] {
        for ms in [0.5, 2., 4.] {
            measure("Iowa flute vibrato", f, 44100, 1, t, ms, "gap")?;
        }
    }
    for t in [0.98, 1.49, 2.19, 3.47, 4.4, 5.16, 6.63, 8.2] {
        for ms in [0.5, 2.] {
            measure("stereo vocal excerpt", vocal, 48000, 2, t, ms, "gap")?;
        }
    }
    measure("intact flute control", f, 44100, 1, 0.8, 2., "intact")?;
    measure("unmarked flute control", f, 44100, 1, 0.8, 2., "unmarked")?;
    // Fixed synthetic transient; deterministic Rust PRNG, not NumPy stream equivalence.
    let mut seed = 457u64;
    let drum: Vec<_> = (0..48000)
        .map(|i| {
            let t = i as f64 / 48000.;
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let noise = (seed >> 11) as f64 / (1u64 << 53) as f64 * 2. - 1.;
            let d = (t - 0.35).max(0.);
            if t < 0.35 {
                0.
            } else {
                (noise * (-15. * d).exp() * 0.0866
                    + 0.23 * (-22. * d).exp() * (std::f64::consts::TAU * 183. * d).sin())
                    as f32
            }
        })
        .collect();
    for ms in [12., 2.] {
        measure(
            "synthetic noisy transient",
            &drum,
            48000,
            1,
            0.35,
            ms,
            "gap",
        )?;
    }
    for i in 0..32 {
        for ms in [0.5, 2., 4.] {
            measure(
                "stress flute",
                f,
                44100,
                1,
                0.17 + i as f64 * 0.17,
                ms,
                "gap",
            )?;
        }
    }
    for i in 0..36 {
        for ms in [0.5, 2., 4.] {
            measure(
                "stress stereo vocal",
                stress_v,
                48000,
                2,
                0.19 + i as f64 * 0.265,
                ms,
                "gap",
            )?;
        }
    }
    if std::fs::read(&flute_path)? != flute_bytes || std::fs::read(&vocal_path)? != vocal_bytes {
        return Err("Source files changed during corpus run".into());
    }
    let filled = rows.iter().filter(|r| r["decision"] == "filled").count();
    let worse = rows
        .iter()
        .filter(|r| {
            r["decision"] == "filled" && r["error_reduction_db"].as_f64().is_some_and(|v| v < 0.)
        })
        .count();
    if worse > 0
        || rows.iter().any(|r| {
            r["decision"] == "filled"
                && (r["case"].as_str().unwrap_or("").ends_with("control")
                    || r["case"].as_str().unwrap_or("").starts_with("synthetic")
                    || r["error_reduction_db"].as_f64().is_none_or(|v| v <= 0.))
        })
    {
        return Err("Frozen gap corpus acceptance failed".into());
    }
    Ok(
        serde_json::json!({"flute_sha256":crate::digest::sha256(&flute_bytes),"vocals_sha256":crate::digest::sha256(&vocal_bytes),"sources_unchanged":true,"total":rows.len(),"filled":filled,"worse_accepted":worse,"synthetic_rng":"Rust uniform, not NumPy Gaussian stream","rows":rows}),
    )
}

pub fn engineering_checks(path: &Path) -> Result<serde_json::Value> {
    let (s, x) = read_audio(path)?;
    if s.channels != 2 || s.sample_rate != 48000 || x.len() != 15947292 * 2 {
        return Err("Expected original 48 kHz, 15947292-frame stereo vocal fixture".into());
    }
    let mut clean = vec![0.; 96000];
    for i in 0..480 {
        clean[48000 + i] = ((std::f64::consts::TAU * 3000. * i as f64 / 48000.).sin()
            * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 479.).cos()))
            as f32;
    }
    let mut planted = clean.clone();
    for i in 0..480 {
        planted[47040 + i] = 0.1 * clean[48000 + i];
    }
    let ratio = pre_onset_ratio(&planted, 48000, 960, 480)?.ok_or("Missing planted energy")?;
    let scaled: Vec<_> = planted.iter().map(|v| v * 0.5).collect();
    let gain_invariant =
        pre_onset_ratio(&scaled, 48000, 960, 480)?.is_some_and(|v| (v - ratio).abs() < 1e-6);
    if (ratio + 20.).abs() > 1e-5
        || !gain_invariant
        || planted[48000..] != clean[48000..]
        || pre_onset_ratio(&clean, 48000, 960, 480)?.is_some()
    {
        return Err("Pre-onset control failed".into());
    }
    Ok(
        serde_json::json!({"sample_rate":s.sample_rate,"channels":s.channels,"full":stereo(&x)?,"excerpt_40_to_50":stereo(&x[40*48000*2..50*48000*2])?,"synthetic_pre_onset_control":{"clean":"zero pre-onset energy","planted_db_relative_to_attack":ratio,"unchanged_attack":true,"same_ratio_after_gain_change":gain_invariant}}),
    )
}

pub fn flute_layers(input: &Path) -> Result<serde_json::Value> {
    let before = std::fs::read(input)?;
    let (spec, all) = read_audio(input)?;
    let n = 256896;
    if spec.sample_rate != 44100
        || spec.channels != 1
        || all.len() != 2 * n + 44100
        || all[n..n + 44100].iter().any(|v| *v != 0.)
    {
        return Err("Unexpected original/regenerated flute layout".into());
    }
    let x = &all[..n];
    let base = crate::harmonic::analyze(x, 44100, crate::harmonic::Profile::FluteReference)?;
    let baseline = crate::harmonic::render(&base, 734, false, false)?.mix();
    let packet = crate::edge::analyze(
        x,
        44100,
        crate::edge::Options {
            profile: crate::harmonic::Profile::FluteReference,
            ..Default::default()
        },
    )?;
    let mut seeds = serde_json::Map::new();
    for seed in [734, 927, 1229] {
        eprintln!("Flute resonance acceptance seed {seed}");
        let y = crate::edge::render(&packet, seed, false, false)?.mix();
        seeds.insert(seed.to_string(), crate::metrics::criteria(x, &y, &base)?);
    }
    let rendered = crate::workspace::render(&packet, 734)?;
    let normal: Vec<_> = (0..n)
        .map(|i| rendered.stems[0][i] + rendered.stems[1][i] + rendered.stems[2][i])
        .collect();
    let accent = rendered.mix(&Default::default())?;
    let (control, onsets) = crate::workspace::breath_control(&packet.base);
    let outside_equal = (0..n)
        .filter(|&i| crate::dsp::interp(&control, i as f64 / 128.) == 0.)
        .all(|i| accent[i] == normal[i]);
    if std::fs::read(input)? != before {
        return Err("Flute source changed".into());
    }
    Ok(
        serde_json::json!({"source_sha256":crate::digest::sha256(&before),"source_unchanged":true,"tracks":packet.tracks.len(),"before":crate::metrics::criteria(x,&baseline,&base)?,"seeds":seeds,"breath_onsets":onsets,"accent_unchanged_outside_gesture":outside_equal,"accent_full_excerpt_rms_delta_db":20.*((accent.iter().map(|v|(*v as f64).powi(2)).sum::<f64>()/normal.iter().map(|v|(*v as f64).powi(2)).sum::<f64>()).sqrt()).log10()}),
    )
}

pub fn guitar_boundaries(root: &Path) -> Result<serde_json::Value> {
    use crate::reperform::{BoundaryPatch, Performance};
    let source_path = root.join("original.wav");
    let source_bytes = std::fs::read(&source_path)?;
    let (spec, source) = read_audio(&source_path)?;
    if spec.channels != 1 {
        return Err("Expected mono guitar source".into());
    }
    let cue_path = root.join("work/new_performance.json");
    let cues: Performance = serde_json::from_reader(std::fs::File::open(&cue_path)?)?;
    let before = serde_json::to_value(&cues)?;
    let restored = cues.restore_subharmonic_ranges(
        &source,
        spec.sample_rate,
        &[[6.105, 6.185], [13.270, 13.355], [25.690, 25.790]],
    )?;
    let ranges = [
        (5.500, 5.510, 873.),
        (5.965, 5.975, 977.),
        (11.385, 11.395, 589.),
        (12.750, 12.785, 660.),
        (31.500, 31.515, 738.),
    ];
    let patches: Vec<_> = ranges
        .iter()
        .map(|&(start_s, end_s, approximate_hz)| BoundaryPatch {
            start_s,
            end_s,
            approximate_hz,
        })
        .collect();
    let fixed = restored.patch_boundaries(&source, spec.sample_rate, &patches)?;
    let after = serde_json::to_value(&fixed)?;
    let archived: serde_json::Value = serde_json::from_reader(std::fs::File::open(
        root.join("work/dry-candidate/new_performance.json"),
    )?)?;
    let mut unchanged = true;
    let mut max_pitch_delta = 0_f64;
    let a = after["notes"].as_array().ok_or("Missing notes")?;
    let b = before["notes"].as_array().ok_or("Missing original notes")?;
    let expected = archived["notes"]
        .as_array()
        .ok_or("Missing archived notes")?;
    if a.len() != b.len() || a.len() != expected.len() {
        return Err("Boundary event count changed".into());
    }
    for ((new, old), reference) in a.iter().zip(b).zip(expected) {
        for key in ["start", "end", "level_steps", "shape", "re_picks"] {
            unchanged &= new[key] == old[key];
        }
        let pitches = new["hz"].as_array().unwrap();
        let expected = reference["hz"]
            .as_array()
            .ok_or("Missing archived pitches")?;
        if pitches.len() != expected.len() {
            return Err("Pitch trajectory length changed".into());
        }
        for (p, q) in pitches.iter().zip(expected) {
            max_pitch_delta =
                max_pitch_delta.max((p.as_f64().unwrap() - q.as_f64().unwrap()).abs());
        }
    }
    let unchanged_source = std::fs::read(&source_path)? == source_bytes;
    Ok(
        serde_json::json!({"source_sha256":crate::digest::sha256(&source_bytes),"source_unchanged":unchanged_source,"non_pitch_cues_unchanged":unchanged,"max_pitch_difference_vs_archived_hz":max_pitch_delta,"before_jumps":cues.boundary_evidence(&source,spec.sample_rate)?.len(),"restored_jumps":restored.boundary_evidence(&source,spec.sample_rate)?.len(),"after_jumps":fixed.boundary_evidence(&source,spec.sample_rate)?.len(),"passed":unchanged&&unchanged_source&&max_pitch_delta<1e-8}),
    )
}

fn flute_fixture(input: &Path) -> Result<Vec<f32>> {
    let (s, x) = read_audio(input)?;
    if s.sample_rate != 44100 || s.channels != 1 || x.len() != 557892 {
        return Err("Unexpected flute fixture".into());
    }
    Ok(x[..256896].to_vec())
}
/// One periodic-Hann Welch segment zero-padded to 65536, matching ridge QA.
fn ridge_contrast(x: &[f32], a: f64, b: f64, hz: f64) -> f64 {
    let x = &x[(a * 44100.).round() as usize..(b * 44100.).round() as usize];
    let n = x.len();
    let mean = x.iter().map(|v| *v as f64).sum::<f64>() / n as f64;
    let w: Vec<_> = (0..n)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())
        .collect();
    let centered: Vec<_> = x.iter().map(|v| *v as f64 - mean).collect();
    let p = crate::dsp::spectrum(&centered, &w, 65536);
    let mut background = Vec::new();
    let mut peak = 0_f64;
    for (k, v) in p.iter().enumerate() {
        let d = (k as f64 * 44100. / 65536. - hz).abs();
        if d < 18. {
            peak = peak.max(v * v);
        }
        if d > 28. && d < 120. {
            background.push(v * v);
        }
    }
    10. * (peak / crate::dsp::median(&background).max(1e-20)).log10()
}
pub fn flute_faint(input: &Path) -> Result<serde_json::Value> {
    use crate::{edge, harmonic, metrics};
    let bytes = std::fs::read(input)?;
    let x = flute_fixture(input)?;
    let options = edge::Options {
        profile: harmonic::Profile::FluteReference,
        ..Default::default()
    };
    let base = edge::analyze(&x, 44100, options)?;
    let enhanced = edge::analyze(
        &x,
        44100,
        edge::Options {
            faint: true,
            ..options
        },
    )?;
    let mut seeds = Vec::new();
    let mut passed = true;
    let mut comparisons = Vec::new();
    for seed in [734, 927, 1229] {
        let before = edge::render(&base, seed, false, false)?.mix();
        let after = edge::render(&enhanced, seed, false, false)?.mix();
        let markers = metrics::landmarks(&before, 44100);
        let new_markers = metrics::landmarks(&after, 44100);
        let max = markers
            .iter()
            .filter_map(|(k, a)| a.zip(new_markers[k]).map(|(a, b)| (a - b).abs()))
            .fold(0., f64::max);
        let tail = before[(5.49 * 44100.) as usize..] == after[(5.49 * 44100.) as usize..];
        passed &= tail && max <= 4.1;
        seeds.push(
            serde_json::json!({"seed":seed,"last_tail_identical":tail,"max_timing_delta_ms":max}),
        );
        if seed == 734 {
            for (a, b, hz) in [(0.80, 1.02, 1552.), (1.35, 1.57, 1539.), (2., 2.22, 1550.)] {
                let old = ridge_contrast(&before, a, b, hz);
                let new = ridge_contrast(&after, a, b, hz);
                if a == 1.35 {
                    passed &= new - old > 3.;
                }
                comparisons.push(serde_json::json!({"span_s":[a,b],"hz":hz,"original_db":ridge_contrast(&x,a,b,hz),"before_db":old,"after_db":new}));
            }
        }
    }
    passed &= std::fs::read(input)? == bytes;
    Ok(
        serde_json::json!({"passed":passed,"source_unchanged":std::fs::read(input)?==bytes,"new_tracks":enhanced.tracks.len()-base.tracks.len(),"comparisons":comparisons,"seeds":seeds}),
    )
}
pub fn continuation_score(
    reference: &[f32],
    generated: &[f32],
    rate: u32,
) -> Result<serde_json::Value> {
    if reference.len() != generated.len() || reference.len() < 512 {
        return Err("Invalid continuation score".into());
    }
    let window: Vec<_> = (0..512)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 512.).cos())
        .collect();
    let (mut error, mut power) = (0., 0.);
    for start in (0..=reference.len() - 512).step_by(128) {
        let spectrum = |x: &[f32]| {
            crate::dsp::spectrum(
                &x[start..start + 512]
                    .iter()
                    .map(|v| *v as f64)
                    .collect::<Vec<_>>(),
                &window,
                512,
            )
        };
        let a = spectrum(reference);
        let b = spectrum(generated);
        for k in 0..a.len() {
            let hz = k as f64 * rate as f64 / 512.;
            if (125. ..=12000.).contains(&hz) {
                error += (a[k] - b[k]).powi(2);
                power += a[k] * a[k];
            }
        }
    }
    let energy = |x: &[f32]| x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
    Ok(
        serde_json::json!({"magnitude_error_relative_to_silence_db":10.*(error.max(1e-24)/power.max(1e-24)).log10(),"level_delta_db":10.*(energy(generated).max(1e-24)/energy(reference).max(1e-24)).log10()}),
    )
}
pub fn flute_cutoffs(input: &Path) -> Result<serde_json::Value> {
    use crate::continuation::{extract, generate};
    let bytes = std::fs::read(input)?;
    let x = flute_fixture(input)?;
    let mut rows = Vec::new();
    for cut in [5.40_f64, 5.44, 5.47, 5.49, 5.51, 5.53, 5.55, 5.57] {
        let i = (cut * 44100.).round() as usize;
        let modes = extract(&x[..i], 44100)?;
        let y = generate(&modes, 44100, 0.12, 714, true)?;
        let held = generate(&modes, 44100, 0.12, 714, false)?;
        let r = &x[i..i + y.len()];
        let mut seeds = Vec::new();
        for seed in [714, 911, 1703] {
            seeds.push(continuation_score(
                r,
                &generate(&modes, 44100, 0.12, seed, true)?,
                44100,
            )?);
        }
        rows.push(serde_json::json!({"cut_seconds":cut,"modes":modes,"continuation":continuation_score(r,&y,44100)?,"constant_hold":continuation_score(r,&held,44100)?,"first_30ms":continuation_score(&r[..1323],&y[..1323],44100)?,"seed_controls":seeds}));
    }
    if bytes != std::fs::read(input)? {
        return Err("Source changed".into());
    }
    Ok(serde_json::json!({"source_unchanged":true,"cuts":rows}))
}
pub fn flute_trace(input: &Path) -> Result<serde_json::Value> {
    let x = flute_fixture(input)?;
    let p = crate::edge::analyze(
        &x,
        44100,
        crate::edge::Options {
            profile: crate::harmonic::Profile::FluteReference,
            ..Default::default()
        },
    )?;
    let s = crate::edge::render(&p, 734, false, false)?;
    let mix = s.mix();
    let mut rows = Vec::new();
    for (a, b) in [
        (0.40, 0.75),
        (0.8, 2.45),
        (2.7, 3.55),
        (3.7, 5.35),
        (5.48, 5.8),
    ] {
        let lo = (a * 44100.) as usize;
        let hi = (b * 44100.) as usize;
        rows.push(serde_json::json!({"span_s":[a,b],"reference":crate::metrics::peaks(&x[lo..hi],44100)?,"mix":crate::metrics::peaks(&mix[lo..hi],44100)?,"pitch":crate::metrics::peaks(&s.tone[lo..hi],44100)?,"ring":crate::metrics::peaks(&s.ring[lo..hi],44100)?,"texture":crate::metrics::peaks(&s.texture[lo..hi],44100)?}));
    }
    Ok(serde_json::json!(rows))
}

pub fn guitar_legacy(root: &Path) -> Result<serde_json::Value> {
    let source = root.join("original.wav");
    let bytes = std::fs::read(&source)?;
    let (s, x) = read_audio(&source)?;
    let p = crate::reperform::analyze_guitar_legacy(&x, s.sample_rate)?;
    let new = serde_json::to_value(&p)?;
    let old: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(root.join("work/new_performance.json"))?)?;
    let a = new["notes"].as_array().unwrap();
    let b = old["notes"].as_array().ok_or("Missing archived notes")?;
    let mut boundary = a.len() == b.len();
    let mut pitch = 0_f64;
    let mut level = 0_f64;
    for (a, b) in a.iter().zip(b) {
        boundary &= a["start"] == b["start"] && a["end"] == b["end"];
        let aa = a["hz"].as_array().unwrap();
        let bb = b["hz"].as_array().unwrap();
        boundary &= aa.len() == bb.len();
        for (a, b) in aa.iter().zip(bb) {
            pitch = pitch.max((a.as_f64().unwrap() - b.as_f64().unwrap()).abs());
        }
        level = level.max((a["accent"].as_f64().unwrap() - b["accent"].as_f64().unwrap()).abs());
    }
    let rendered = p.synthesize_dry()?;
    let unchanged = bytes == std::fs::read(&source)?;
    Ok(
        serde_json::json!({"archived_events":b.len(),"rust_events":a.len(),"boundaries_identical":boundary,"max_pitch_difference_hz":pitch,"max_accent_difference":level,"source_unchanged":unchanged,"fresh_render_finite":rendered.iter().all(|v|v.is_finite()),"passed":boundary&&pitch<=0.01&&level<=0.0001&&unchanged}),
    )
}
pub fn cue_passes(input: &Path, out: &Path) -> Result<serde_json::Value> {
    let before = std::fs::read(input)?;
    let (s, x) = read_audio(input)?;
    if s.sample_rate != 48000 || s.channels != 2 || x.len() < 50 * 48000 * 2 {
        return Err("Expected original stereo vocal source".into());
    }
    let mono: Vec<_> = x[40 * 48000 * 2..50 * 48000 * 2]
        .chunks_exact(2)
        .map(|v| (v[0] + v[1]) * 0.5)
        .collect();
    let packet = crate::cue_passes::analyze(&mono, 48000)?;
    let second = crate::cue_passes::analyze(&mono, 48000)?;
    if packet.piles != second.piles || packet.pitch != second.pitch {
        return Err("Nondeterministic cue analysis".into());
    }
    let stems = crate::cue_passes::render(&packet)?;
    let full: Vec<_> = (0..mono.len())
        .map(|i| stems.iter().map(|s| s[i]).sum::<f64>())
        .collect();
    let rms = |v: &[f64]| crate::dsp::rms(v);
    let target = rms(&full);
    let mut waves = vec![
        ("all-cues", full.clone()),
        ("anchors-only", stems[0].clone()),
    ];
    for (i, name) in [
        (1, "without-supported"),
        (2, "without-faint"),
        (3, "without-unresolved"),
    ] {
        waves.push((
            name,
            full.iter().zip(&stems[i]).map(|(a, b)| a - b).collect(),
        ));
    }
    let mut gainrows = serde_json::Map::new();
    for (name, y) in &mut waves {
        let gain = target / rms(y).max(1e-15);
        gainrows.insert((*name).into(), (20. * gain.log10()).into());
        for v in y {
            *v *= gain;
        }
    }
    let headroom = (0.90
        / waves
            .iter()
            .flat_map(|(_, y)| y)
            .map(|v| v.abs())
            .fold(0., f64::max))
    .min(1.);
    std::fs::create_dir(out)?;
    for (name, y) in waves {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out.join(format!("{name}.wav")))?;
        let mut w = hound::WavWriter::new(
            f,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for v in y {
            w.write_sample((v * headroom) as f32)?;
        }
        w.finalize()?;
    }
    write_new(
        &out.join("analysis.json"),
        serde_json::to_string(&packet)?.as_bytes(),
    )?;
    let mut piles = serde_json::Map::new();
    for (pile, name) in ["anchors", "supported", "faint", "unresolved"]
        .iter()
        .enumerate()
    {
        let mut total = 0.;
        let mut selected = 0.;
        let mut ratios = Vec::new();
        for (lo, hi) in [
            (0., 2000.),
            (2000., 4000.),
            (4000., 10000.),
            (10000., 20000.),
        ] {
            let (mut sum, mut part) = (0., 0.);
            for (i, row) in packet.magnitudes.iter().enumerate() {
                for (k, v) in row.iter().enumerate() {
                    let hz = k as f64 * 48000. / 2048.;
                    if hz >= lo && hz < hi {
                        sum += v * v;
                        if packet.piles[i][k] as usize == pile {
                            part += v * v;
                        }
                    }
                }
            }
            ratios.push(100. * part / sum.max(1e-15));
        }
        for (i, row) in packet.magnitudes.iter().enumerate() {
            for (k, v) in row.iter().enumerate() {
                total += v * v;
                if packet.piles[i][k] as usize == pile {
                    selected += v * v;
                }
            }
        }
        piles.insert((*name).into(),serde_json::json!({"spectral_energy_pct":100.*selected/total,"share_of_band_energy_pct":ratios,"difference_if_omitted_db_relative_to_full":20.*(rms(&stems[pile]).max(1e-15)/target).log10()}));
    }
    if before != std::fs::read(input)? {
        return Err("Vocal source changed".into());
    }
    Ok(
        serde_json::json!({"clip_s":[40,50],"sample_rate":48000,"channel_mode":"explicit stereo mean","piles":piles,"rms_adjustment_db":gainrows,"listening_peak_safety_gain":headroom,"source_unchanged":true,"shared_phase_exact_ablations":true}),
    )
}

pub fn gesture(input: &Path, out: &Path) -> Result<serde_json::Value> {
    let bytes = std::fs::read(input)?;
    let x = flute_fixture(input)?;
    let p = crate::edge::analyze(
        &x,
        44100,
        crate::edge::Options {
            profile: crate::harmonic::Profile::FluteReference,
            ..Default::default()
        },
    )?;
    let rendered = crate::workspace::render(&p, 734)?;
    let normal: Vec<_> = (0..x.len())
        .map(|i| rendered.stems[0][i] + rendered.stems[1][i] + rendered.stems[2][i])
        .collect();
    let accent = rendered.mix(&Default::default())?;
    let (control, onsets) = crate::workspace::breath_control(&p.base);
    let noise = crate::harmonic::render_inner(&p.base, 734, false, false, false)?.texture;
    let breath = crate::dsp::bandpass(
        &noise.iter().map(|v| *v as f64).collect::<Vec<_>>(),
        44100,
        1000.,
        8000.,
        3,
    );
    let marks = crate::metrics::landmarks(&normal, 44100);
    let changed = crate::metrics::landmarks(&accent, 44100);
    let max = marks
        .iter()
        .filter_map(|(k, v)| v.zip(changed[k]).map(|(a, b)| (a - b).abs()))
        .fold(0., f64::max);
    let mut rows = Vec::new();
    for onset in &onsets {
        let lo = ((onset - 0.075) * 44100.) as usize;
        let hi = ((onset + 0.075) * 44100.) as usize;
        let delta: Vec<_> = rendered.stems[3][lo..hi]
            .iter()
            .map(|v| *v as f64)
            .collect();
        let baseline: Vec<_> = normal[lo..hi].iter().map(|v| *v as f64).collect();
        let added: Vec<_> = breath[lo..hi]
            .iter()
            .zip(&delta)
            .map(|(a, b)| a + b)
            .collect();
        let db = |a: f64, b: f64| 20. * (a.max(1e-20) / b.max(1e-20)).log10();
        rows.push(serde_json::json!({"entrance_seconds":onset,"added_breath_relative_to_local_baseline_db":db(crate::dsp::rms(&delta),crate::dsp::rms(&baseline)),"generated_breath_carrier_rms_change_db":db(crate::dsp::rms(&added),crate::dsp::rms(&breath[lo..hi])),"peak_local_gain_command_db":7.*(lo..hi).map(|i|crate::dsp::interp(&control,i as f64/128.)).fold(0.,f64::max)}));
    }
    let energy = |v: &[f32]| v.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
    let rms = 10. * (energy(&accent) / energy(&normal)).log10();
    let unchanged = (0..x.len())
        .filter(|&i| crate::dsp::interp(&control, i as f64 / 128.) == 0.)
        .all(|i| normal[i] == accent[i]);
    let passed = onsets.len() == 2
        && control.iter().copied().fold(0., f64::max) > 0.20
        && unchanged
        && max <= 4.1
        && rms.abs() < 0.15
        && bytes == std::fs::read(input)?;
    if !passed {
        return Err("Frozen gesture gates failed".into());
    }
    let common = (0.88
        / normal
            .iter()
            .chain(&accent)
            .map(|v| v.abs())
            .fold(0., f32::max))
    .min(1.);
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?;
    let mut w = hound::WavWriter::new(
        f,
        hound::WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for v in normal
        .iter()
        .copied()
        .chain(std::iter::repeat_n(0., 37485))
        .chain(accent.iter().copied())
    {
        w.write_sample((v * common * 32767.).round_ties_even() as i16)?;
    }
    w.finalize()?;
    let (_, pcm) = read_audio(out)?;
    if (0..x.len())
        .filter(|&i| crate::dsp::interp(&control, i as f64 / 128.) == 0.)
        .any(|i| pcm[i] != pcm[x.len() + 37485 + i])
    {
        return Err("Quantized gesture comparison changed outside gate".into());
    }
    Ok(
        serde_json::json!({"passed":true,"entrances":rows,"max_timing_delta_ms":max,"full_excerpt_rms_delta_db":rms,"same_outside_gesture":true,"source_unchanged":true,"common_gain":common,"second_take_start_sec":(x.len()+37485) as f64/44100.,"listening_verdict":"not assessed"}),
    )
}

pub fn cutoff_export(input: &Path, out: &Path) -> Result<serde_json::Value> {
    let report = flute_cutoffs(input)?;
    std::fs::create_dir(out)?;
    for row in report["cuts"].as_array().ok_or("Missing cut report")? {
        let modes: Vec<crate::continuation::Mode> = serde_json::from_value(row["modes"].clone())?;
        if !modes.is_empty() {
            let y = crate::continuation::generate(&modes, 44100, 0.12, 714, true)?;
            crate::write_wav(
                44100,
                &y,
                &out.join(format!(
                    "predicted-tail-{:.2}.wav",
                    row["cut_seconds"].as_f64().unwrap()
                )),
            )?;
        }
    }
    write_new(
        &out.join("cutoff-results.json"),
        serde_json::to_string_pretty(&report)?.as_bytes(),
    )?;
    Ok(report)
}
pub fn faint_export(input: &Path, out: &Path) -> Result<serde_json::Value> {
    let report = flute_faint(input)?;
    if report["passed"] != true {
        return Err("Frozen faint acceptance failed".into());
    }
    let x = flute_fixture(input)?;
    let base = crate::edge::Options {
        profile: crate::harmonic::Profile::FluteReference,
        ..Default::default()
    };
    let before =
        crate::edge::render(&crate::edge::analyze(&x, 44100, base)?, 734, false, false)?.mix();
    let after = crate::edge::render(
        &crate::edge::analyze(
            &x,
            44100,
            crate::edge::Options {
                faint: true,
                ..base
            },
        )?,
        734,
        false,
        false,
    )?
    .mix();
    let energy = |x: &[f32]| x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
    let target = energy(&before);
    let mut combo = Vec::new();
    for wave in [&x, &before, &after] {
        if !combo.is_empty() {
            combo.extend(std::iter::repeat_n(0., 35280));
        }
        let gain = (target / energy(wave)).sqrt();
        combo.extend(wave.iter().map(|v| (*v as f64 * gain) as f32));
    }
    let gain = (0.97 / combo.iter().map(|v| v.abs()).fold(0., f32::max)).min(1.);
    for v in &mut combo {
        *v *= gain;
    }
    crate::write_wav(44100, &combo, out)?;
    Ok(report)
}

fn write_float_wav_unscaled(rate: u32, samples: &[f32], path: &Path) -> Result<()> {
    if samples.is_empty() || samples.iter().any(|v| !v.is_finite()) {
        return Err("Invalid float WAV samples".into());
    }
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut w = hound::WavWriter::new(
        std::io::BufWriter::new(f),
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for &s in samples {
        w.write_sample(s)?;
    }
    w.finalize()?;
    Ok(())
}

fn signal_stats(samples: &[f32]) -> (f64, Option<f64>, Option<f64>) {
    let peak = samples.iter().map(|v| v.abs() as f64).fold(0_f64, f64::max);
    let rms = (samples.iter().map(|v| (*v as f64).powi(2)).sum::<f64>()
        / samples.len().max(1) as f64)
        .sqrt();
    let peak_dbfs = (peak > 0.).then(|| 20. * peak.log10());
    let rms_dbfs = (rms > 0.).then(|| 20. * rms.log10());
    (peak, peak_dbfs, rms_dbfs)
}

/// Compare two mono WAVs of equal rate and length with the independent pitch checker.
pub fn guitar_pitch_check(
    reference_path: &Path,
    candidate_path: &Path,
    start_s: f64,
    end_s: f64,
) -> Result<serde_json::Value> {
    let (sr, r) = read_audio(reference_path)?;
    let (sc, c) = read_audio(candidate_path)?;
    if sr.channels != 1
        || sc.channels != 1
        || sr.sample_rate != sc.sample_rate
        || r.len() != c.len()
    {
        return Err("Expected matching mono WAVs of identical sample rate and length".into());
    }
    let report = crate::reperform::independent_pitch_check(&r, &c, sr.sample_rate, start_s, end_s)?;
    Ok(serde_json::to_value(report)?)
}

/// Full-track guitar verification harness: verifies source SHA-256 unchanged,
/// extracts source-free cues, verifies bit-identical cue-only rerun, runs the
/// independent pitch & envelope check (full track + 20-27 s + 25-26 s evidence),
/// writes level-matched A/B WAV and same-scale waveforms/spectrograms + report.md.
pub fn guitar_verify(input: &Path, out: &Path) -> Result<serde_json::Value> {
    use crate::reperform::{independent_pitch_check, EvidenceStatus, Performance};
    use std::fmt::Write as _;

    let before_bytes = std::fs::read(input)?;
    let source_sha256 = crate::digest::sha256(&before_bytes);
    let (spec, source) = read_audio(input)?;
    if spec.channels != 1 {
        return Err("Expected mono WAV for guitar verification".into());
    }
    let rate = spec.sample_rate;
    let duration_s = source.len() as f64 / f64::from(rate);

    let cues = crate::reperform::analyze_guitar(&source, rate)?;
    std::fs::create_dir(out)?;
    let cues_path = out.join("cues.json");
    write_new(&cues_path, &serde_json::to_vec_pretty(&cues)?)?;

    // Strict source-free cue-only rerun check from disk.
    let reloaded: Performance = serde_json::from_reader(std::fs::File::open(&cues_path)?)?;
    let dry_a = reloaded.synthesize_dry()?;
    let dry_b = reloaded.synthesize_dry()?;
    let cue_only_rerun_identical = dry_a == dry_b;
    let full_duration_match = dry_a.len() == source.len();
    let all_finite = dry_a.iter().all(|v| v.is_finite());
    let (dry_peak, dry_peak_dbfs, dry_rms_dbfs) = signal_stats(&dry_a);
    let (src_peak, src_peak_dbfs, src_rms_dbfs) = signal_stats(&source);
    let headroom_ok = dry_peak < 1.0;

    if !cue_only_rerun_identical || !full_duration_match || !all_finite || !headroom_ok {
        return Err(
            "Dry candidate failed determinism, duration, finiteness, or headroom check".into(),
        );
    }

    let dry_path = out.join("dry-candidate.wav");
    reloaded.write_dry_wav(&dry_path)?;
    let dry_sha256 = crate::digest::sha256(&std::fs::read(&dry_path)?);

    // Independent pitch and envelope checks (never folds octave errors or counts unresolved as pass).
    let full_check = independent_pitch_check(&source, &dry_a, rate, 0.0, duration_s)?;
    let window_20_27 = if duration_s >= 27.0 {
        Some(independent_pitch_check(&source, &dry_a, rate, 20.0, 27.0)?)
    } else {
        None
    };

    // Inspect 25.0-26.0 s evidence without modifying the cue packet.
    let transition_25_26 = if duration_s >= 26.0 {
        let frames: Vec<_> = reloaded
            .frame_evidence()
            .iter()
            .filter(|f| (25.0..=26.0).contains(&f.time_s))
            .collect();
        let sup = frames
            .iter()
            .filter(|f| f.status == EvidenceStatus::Supported)
            .count();
        let unc = frames
            .iter()
            .filter(|f| f.status == EvidenceStatus::Uncertain)
            .count();
        let oot = frames
            .iter()
            .filter(|f| f.status == EvidenceStatus::OutOfTarget)
            .count();
        let sil = frames
            .iter()
            .filter(|f| f.status == EvidenceStatus::Silence)
            .count();
        let multi_hyp = frames.iter().filter(|f| f.hypotheses.len() >= 2).count();
        Some(serde_json::json!({
            "window_s": [25.0, 26.0],
            "frames": frames.len(),
            "supported": sup,
            "uncertain": unc,
            "out_of_target": oot,
            "silence": sil,
            "frames_with_competing_hypotheses": multi_hyp
        }))
    } else {
        None
    };

    // Level-matched A/B WAV: Original + 0.8 s silence + RMS-matched Dry candidate.
    let rms_of = |x: &[f32]| {
        (x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    };
    let src_rms = rms_of(&source);
    let dry_rms = rms_of(&dry_a);
    let dry_match_gain = if dry_rms > 1e-12 && src_rms > 1e-12 {
        (src_rms / dry_rms).clamp(0.125, 8.0) as f32
    } else {
        1.0
    };
    let ab_peak = (src_peak as f32).max(dry_peak as f32 * dry_match_gain);
    let ab_headroom = if ab_peak > 0.89 { 0.89 / ab_peak } else { 1.0 };
    let gap = (rate as f64 * 0.8).round() as usize;
    let mut ab = Vec::with_capacity(source.len() * 2 + gap);
    ab.extend(source.iter().map(|&v| v * ab_headroom));
    ab.extend(std::iter::repeat_n(0_f32, gap));
    ab.extend(dry_a.iter().map(|&v| v * dry_match_gain * ab_headroom));
    write_float_wav_unscaled(rate, &ab, &out.join("ab-original-vs-dry-candidate.wav"))?;

    // Same-scale waveforms and spectrograms.
    let shared_peak = (src_peak as f32).max(dry_peak as f32).max(0.05);
    let wf_full = crate::figures::waveform_panels(
        &[
            ("Original (reference)", &source, 0.0, duration_s),
            (
                "Dry candidate (cue-only synthesis)",
                &dry_a,
                0.0,
                duration_s,
            ),
        ],
        rate,
        shared_peak,
    )?;
    write_new(&out.join("waveforms-full.svg"), wf_full.as_bytes())?;
    let sp_full = crate::figures::heatmap(
        &[
            ("Original (reference)", &source, 0.0, duration_s),
            (
                "Dry candidate (cue-only synthesis)",
                &dry_a,
                0.0,
                duration_s,
            ),
        ],
        rate,
        (-104., -29.),
        &[],
    )?;
    write_new(&out.join("spectrograms-full.svg"), sp_full.as_bytes())?;

    if duration_s >= 27.0 {
        let wf_20_27 = crate::figures::waveform_panels(
            &[
                ("Original (20–27 s)", &source, 20.0, 27.0),
                ("Dry candidate (20–27 s)", &dry_a, 20.0, 27.0),
            ],
            rate,
            shared_peak,
        )?;
        write_new(&out.join("waveforms-20-27s.svg"), wf_20_27.as_bytes())?;
        let sp_20_27 = crate::figures::heatmap(
            &[
                ("Original (20–27 s)", &source, 20.0, 27.0),
                ("Dry candidate (20–27 s)", &dry_a, 20.0, 27.0),
            ],
            rate,
            (-104., -29.),
            &[],
        )?;
        write_new(&out.join("spectrograms-20-27s.svg"), sp_20_27.as_bytes())?;
        let sp_25_26 = crate::figures::heatmap(
            &[
                (
                    "Original (25.0–26.0 s transition detail)",
                    &source,
                    25.0,
                    26.0,
                ),
                (
                    "Dry candidate (25.0–26.0 s transition detail)",
                    &dry_a,
                    25.0,
                    26.0,
                ),
            ],
            rate,
            (-104., -29.),
            &[],
        )?;
        write_new(&out.join("spectrograms-25-26s.svg"), sp_25_26.as_bytes())?;
    }

    let source_unchanged = std::fs::read(input)? == before_bytes;
    if !source_unchanged {
        return Err("Source file changed during guitar verification".into());
    }

    let summary = reloaded.summary();
    let full_agreed_of_resolved_pct = if full_check.both_resolved_frames > 0 {
        Some(
            100.0 * full_check.agreed_within_100_cents as f64
                / full_check.both_resolved_frames as f64,
        )
    } else {
        None
    };
    let full_agreed_of_total_pct = if full_check.total_frames > 0 {
        Some(100.0 * full_check.agreed_within_100_cents as f64 / full_check.total_frames as f64)
    } else {
        None
    };

    let result = serde_json::json!({
        "audio_status": "candidate only (pending human listening review)",
        "dry_review_status": crate::capture::DryReviewStatus::NotReviewed.label(),
        "listening_verdict": "Not reviewed — automated environment cannot listen",
        "source_path": input.display().to_string(),
        "source_sha256": source_sha256,
        "source_unchanged": source_unchanged,
        "sample_rate": rate,
        "samples": source.len(),
        "duration_s": duration_s,
        "dry_candidate_sha256": dry_sha256,
        "cue_only_rerun_identical": cue_only_rerun_identical,
        "full_duration_match": full_duration_match,
        "headroom_ok": headroom_ok,
        "source_peak_dbfs": src_peak_dbfs,
        "source_rms_dbfs": src_rms_dbfs,
        "dry_peak_dbfs": dry_peak_dbfs,
        "dry_rms_dbfs": dry_rms_dbfs,
        "ab_dry_match_gain_db": 20. * f64::from(dry_match_gain).log10(),
        "ab_common_headroom_gain_db": 20. * f64::from(ab_headroom).log10(),
        "cue_summary": summary,
        "independent_pitch_full": full_check,
        "independent_pitch_full_agreed_of_resolved_pct": full_agreed_of_resolved_pct,
        "independent_pitch_full_agreed_of_total_pct": full_agreed_of_total_pct,
        "independent_pitch_20_27s": window_20_27,
        "transition_25_26s": transition_25_26,
    });

    write_new(
        &out.join("verification.json"),
        serde_json::to_string_pretty(&result)?.as_bytes(),
    )?;

    let mut md = String::new();
    writeln!(md, "# SpectraCraft — Dry Guitar Verification Report")?;
    writeln!(md)?;
    writeln!(
        md,
        "**Audio status:** Candidate only. No human listening was performed in this run; objective measurements do not certify perceived phrasing, tone, or musical quality."
    )?;
    writeln!(md)?;
    writeln!(md, "## Integrity & Determinism")?;
    writeln!(
        md,
        "- Source SHA-256: `{}` (verified unchanged)",
        source_sha256
    )?;
    writeln!(
        md,
        "- Sample rate / duration: {} Hz, {} samples ({:.3} s)",
        rate,
        source.len(),
        duration_s
    )?;
    writeln!(
        md,
        "- Cue-only rerun bit-identical: `{}`",
        cue_only_rerun_identical
    )?;
    writeln!(
        md,
        "- Dry candidate peak / RMS: {} / {} (headroom < 0 dBFS: `{}`)",
        dry_peak_dbfs.map_or("silence".into(), |v| format!("{v:+.2} dBFS")),
        dry_rms_dbfs.map_or("silence".into(), |v| format!("{v:+.2} dBFS")),
        headroom_ok
    )?;
    writeln!(md)?;
    writeln!(md, "## Cue & Evidence Separation")?;
    writeln!(
        md,
        "- Notes: {}, auto-detected re-picks: {}, overlapping-ringing notes: {}",
        summary.note_count, summary.total_repicks, summary.overlapping_ring_notes
    )?;
    writeln!(
        md,
        "- 5 ms frame evidence: {} supported, {} uncertain (abstained), {} out-of-target, {} silence",
        summary.supported_frames,
        summary.uncertain_frames,
        summary.out_of_target_frames,
        summary.silence_frames
    )?;
    writeln!(md)?;
    writeln!(
        md,
        "## Independent Pitch & Envelope Check (10 ms grid; no octave folding)"
    )?;
    writeln!(
        md,
        "- Full track (0–{:.3} s): {} agreed <=100 cents out of {} jointly resolved ({}), out of {} total evaluated ({}), {} unresolved (ref-only {}, dry-only {}, both {}), {} octave errors, {} non-octave disagreements, envelope corr = {}",
        duration_s,
        full_check.agreed_within_100_cents,
        full_check.both_resolved_frames,
        full_agreed_of_resolved_pct.map_or("n/a".into(), |v| format!("{v:.1}%")),
        full_check.total_frames,
        full_agreed_of_total_pct.map_or("n/a".into(), |v| format!("{v:.1}%")),
        full_check.unresolved_frames,
        full_check.unresolved_reference,
        full_check.unresolved_dry,
        full_check.unresolved_both,
        full_check.octave_errors,
        full_check.disagreed_non_octave,
        full_check
            .envelope_correlation
            .map_or("n/a".into(), |v| format!("{v:.4}"))
    )?;
    if let Some(w) = &window_20_27 {
        writeln!(
            md,
            "- 20–27 s window: {}/{} both resolved, {} agreed <=100 cents, {} octave errors, {} non-octave disagreements, {} unresolved (ref-only {}, dry-only {}, both {}), envelope corr = {}",
            w.both_resolved_frames,
            w.total_frames,
            w.agreed_within_100_cents,
            w.octave_errors,
            w.disagreed_non_octave,
            w.unresolved_frames,
            w.unresolved_reference,
            w.unresolved_dry,
            w.unresolved_both,
            w.envelope_correlation
                .map_or("n/a".into(), |v| format!("{v:.4}"))
        )?;
    }
    writeln!(md)?;
    writeln!(md, "## What These Comparisons Show & Cannot Establish")?;
    writeln!(
        md,
        "- **Shown:** Deterministic source-free cue rendering, full-track duration and headroom, separated evidence categories, shared-scale waveform/envelope alignment, and independent frame-level pitch agreement vs. explicit octave errors and unresolved frames."
    )?;
    writeln!(
        md,
        "- **Not established:** Perceived guitar realism, pick feel, vibrato naturalness, or whether abstained transitions (such as near 25.5 s) sound musically acceptable. Human auditioning of `ab-original-vs-dry-candidate.wav` and `dry-candidate.wav` is required before approving the dry performance."
    )?;
    write_new(&out.join("report.md"), md.as_bytes())?;

    Ok(result)
}

/// Export selected post-dry-review guitar variants (`all` or comma-separated subset of
/// `original,natural,digital,analog,expressive`) with matching waveforms, spectrograms,
/// and a technical comparison report. Never overwrites the source audio.
pub fn export_guitar_variants(
    input: &Path,
    cues_path: &Path,
    out: &Path,
    selection: &str,
    review: crate::capture::DryReviewStatus,
) -> Result<serde_json::Value> {
    use crate::capture::{render_guitar_variant_checked, DryReviewStatus, ExportVariant};
    use crate::reperform::Performance;
    use std::fmt::Write as _;

    if review != DryReviewStatus::ApprovedByHuman {
        return Err(
            "Variant export blocked: dry performance is Not reviewed. No numerical score can approve the dry take; explicit human review is required."
                .into(),
        );
    }

    let variants = ExportVariant::parse_selection(selection)?;
    let before_bytes = std::fs::read(input)?;
    let source_sha256 = crate::digest::sha256(&before_bytes);
    let (spec, source) = read_audio(input)?;
    if spec.channels != 1 {
        return Err("Expected mono WAV for guitar variant export".into());
    }
    let cues: Performance = serde_json::from_reader(std::fs::File::open(cues_path)?)?;
    if cues.rate() != spec.sample_rate || cues.samples() != source.len() {
        return Err("Cue packet rate and sample count must match the original WAV".into());
    }
    // Verify cue-only dry render is deterministic before exporting capture variants.
    let dry_a = cues.synthesize_dry()?;
    let dry_b = cues.synthesize_dry()?;
    if dry_a != dry_b {
        return Err("Cue-only dry render was not deterministic".into());
    }

    std::fs::create_dir(out)?;
    let rate = spec.sample_rate;
    let duration_s = source.len() as f64 / f64::from(rate);

    let mut rendered: Vec<(ExportVariant, Vec<f32>, serde_json::Value)> = Vec::new();
    for variant in variants {
        let filename = format!("{}.wav", variant.slug());
        let file_path = out.join(&filename);
        let wave = render_guitar_variant_checked(review, variant, &source, &cues)?;
        if variant == ExportVariant::Original {
            // Preserve the exact original WAV bytes untouched.
            write_new(&file_path, &before_bytes)?;
        } else {
            write_float_wav_unscaled(rate, &wave, &file_path)?;
        }
        let sha256 = crate::digest::sha256(&std::fs::read(&file_path)?);
        let (peak, peak_dbfs, rms_dbfs) = signal_stats(&wave);
        let headroom_db = (peak > 0.).then(|| -20. * peak.log10());
        let entry = serde_json::json!({
            "variant": variant.slug(),
            "label": variant.label(),
            "file": filename,
            "sha256": sha256,
            "samples": wave.len(),
            "sample_rate": rate,
            "unmastered": true,
            "peak_linear": peak,
            "peak_dbfs": peak_dbfs,
            "rms_dbfs": rms_dbfs,
            "headroom_below_0dbfs_db": headroom_db,
            "unchanged_copy_of_original": variant == ExportVariant::Original && sha256 == source_sha256,
        });
        rendered.push((variant, wave, entry));
    }

    // Build shared-scale full-track and detail waveforms & spectrograms across selected variants.
    let shared_peak = rendered
        .iter()
        .flat_map(|(_, w, _)| w.iter())
        .map(|v| v.abs())
        .fold(0.05_f32, f32::max);
    let (detail_lo, detail_hi) = if duration_s >= 27.0 {
        (20.0, 27.0)
    } else {
        (duration_s * 0.25, duration_s * 0.75)
    };
    let full_panels: Vec<(&str, &[f32], f64, f64)> = rendered
        .iter()
        .map(|(v, w, _)| (v.label(), w.as_slice(), 0.0, duration_s))
        .collect();
    let detail_panels: Vec<(&str, &[f32], f64, f64)> = rendered
        .iter()
        .map(|(v, w, _)| (v.label(), w.as_slice(), detail_lo, detail_hi))
        .collect();

    let wf_full = crate::figures::waveform_panels(&full_panels, rate, shared_peak)?;
    write_new(&out.join("waveforms-full.svg"), wf_full.as_bytes())?;
    let wf_detail = crate::figures::waveform_panels(&detail_panels, rate, shared_peak)?;
    write_new(&out.join("waveforms-detail.svg"), wf_detail.as_bytes())?;
    let sp_full = crate::figures::heatmap(&full_panels, rate, (-104., -29.), &[])?;
    write_new(&out.join("spectrograms-full.svg"), sp_full.as_bytes())?;
    let sp_detail = crate::figures::heatmap(&detail_panels, rate, (-104., -29.), &[])?;
    write_new(&out.join("spectrograms-detail.svg"), sp_detail.as_bytes())?;

    let source_unchanged = std::fs::read(input)? == before_bytes;
    if !source_unchanged {
        return Err("Source file changed during variant export".into());
    }

    let entries: Vec<serde_json::Value> = rendered.iter().map(|(_, _, e)| e.clone()).collect();
    let manifest = serde_json::json!({
        "audio_status": "candidate exports (pending human dry-review & listening)",
        "selection": selection,
        "source_sha256": source_sha256,
        "source_unchanged": source_unchanged,
        "detail_window_s": [detail_lo, detail_hi],
        "shared_waveform_scale_linear": shared_peak,
        "shared_spectrogram_scale_dbfs": [-104, -29],
        "exports": entries,
    });
    write_new(
        &out.join("variants-manifest.json"),
        serde_json::to_string_pretty(&manifest)?.as_bytes(),
    )?;

    let mut md = String::new();
    writeln!(md, "# SpectraCraft — Guitar Variant Comparison Report")?;
    writeln!(md)?;
    writeln!(
        md,
        "**Status:** Unmastered candidate exports. Automated builds cannot audition audio; dry-performance listening review is required before treating any capture variant as approved."
    )?;
    writeln!(md)?;
    writeln!(md, "## Exported Versions (Selection: `{selection}`)")?;
    for (_, _, e) in &rendered {
        writeln!(
            md,
            "- **{}** (`{}`): peak {}, RMS {}, SHA-256 `{}`",
            e["label"].as_str().unwrap_or(""),
            e["file"].as_str().unwrap_or(""),
            e["peak_dbfs"]
                .as_f64()
                .map_or("silence".into(), |v| format!("{v:+.2} dBFS")),
            e["rms_dbfs"]
                .as_f64()
                .map_or("silence".into(), |v| format!("{v:+.2} dBFS")),
            e["sha256"].as_str().unwrap_or("")
        )?;
    }
    writeln!(md)?;
    writeln!(md, "## Signal Paths")?;
    writeln!(
        md,
        "- **Original**: Byte-for-byte unchanged copy of the input WAV."
    )?;
    writeln!(
        md,
        "- **Natural**: Clean open-back combo voicing into a padded 10 × 10 × 9 ft image-source room (heavy treatment on walls/ceiling, treated floor) captured by a labeled SM7-class cardioid dynamic approximation at ~2 ft (0.61 m). Not a measured unit impulse response."
    )?;
    writeln!(
        md,
        "- **Digital**: Clean direct/modeled linear buffer path with crisp high-frequency articulation and zero added room reflections."
    )?;
    writeln!(
        md,
        "- **Analog**: 2× oversampled asymmetric triode/transformer soft saturation, dynamic supply-sag envelope response on transients, passive tone stack, and close-miked damped booth capture. Adds zero synthetic 50/60 Hz hum or noise."
    )?;
    writeln!(
        md,
        "- **Expressive**: Mutates selected performance cues (dynamic phrasing contrast, accented pick harmonics, stronger re-picks, subtle delayed sustain vibrato on steady notes > 280 ms while leaving slides/bends untouched) *before* dry string synthesis and Natural amp-to-mic capture."
    )?;
    writeln!(md)?;
    writeln!(md, "## What Each Comparison Shows & Cannot Establish")?;
    writeln!(
        md,
        "- **Waveforms (`waveforms-full.svg`, `waveforms-detail.svg`)** share one linear amplitude scale (`±{shared_peak:.3}`) so relative peak headroom, transient envelopes, supply-sag compression (Analog), and expanded phrasing dynamics (Expressive) are directly visible without per-panel auto-scaling."
    )?;
    writeln!(
        md,
        "- **Spectrograms (`spectrograms-full.svg`, `spectrograms-detail.svg`)** share one fixed dBFS color scale (`-104` to `-29 dBFS`, `0–8 kHz`) so cabinet/mic rolloff (Natural), extended clean top end (Digital), nonlinear harmonic generation (Analog), and pre-capture vibrato/accent modulation (Expressive) can be compared directly against Original."
    )?;
    writeln!(
        md,
        "- **What these plots and metrics cannot establish:** Perceptual realism of the amp/mic approximation, preference between Analog and Natural warmth, or whether Expressive phrasing feels musical. Those require human listening on calibrated playback."
    )?;
    write_new(&out.join("technical-report.md"), md.as_bytes())?;

    Ok(manifest)
}
