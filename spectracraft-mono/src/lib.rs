pub mod acoustic_math;
pub mod acoustics_physics;
pub mod app_jobs;
pub mod app_session;
pub mod attack_guard;
#[cfg(feature = "bench")]
pub mod bench;
pub mod capture;
pub mod continuation;
pub mod cue_passes;
pub mod daw_bridge;
pub mod digest;
pub mod dsp;
pub mod edge;
pub mod figures;
pub mod gap;
pub mod harmonic;
pub mod layers;
pub mod masked;
pub mod metrics;
pub mod migration;
pub mod pipeline;
pub mod platform;
pub mod preflight;
pub mod qa;
pub mod reference_profiles;
pub mod region_edit;
pub mod reperform;
pub mod report_export;
pub mod research_diagnostics;
#[cfg(feature = "bench")]
pub mod room_bench;
pub mod settings_store;
pub mod shape_profile;
pub mod source_qualification;
pub mod string_state;
pub mod studio_catalog;
pub mod technical_report;
pub mod translation_layer;
pub mod update_manager;
pub mod visualization;
pub mod workspace;
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
use std::{f64::consts::TAU, path::Path};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const MAX_SECONDS: usize = 30;
const MAX_TRACKS: usize = 20;
const VERSION: u32 = 1;
const TEXTURE_EDGES: [f32; 9] = [
    500., 800., 1250., 2000., 3150., 5000., 8000., 12500., 20000.,
];
type TextureFrame = [f32; TEXTURE_EDGES.len() - 1];
fn band(hz: f32) -> Option<usize> {
    TEXTURE_EDGES
        .windows(2)
        .position(|x| x[0] <= hz && hz < x[1])
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Partial {
    hz: f32,
    amplitude: f32,
}
type Frame = [Partial; MAX_TRACKS];
const OFF: Partial = Partial {
    hz: 0.,
    amplitude: 0.,
};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    version: u32,
    sample_rate: u32,
    frames: usize,
    hop: usize,
    partials: Vec<Frame>,
    #[serde(default)]
    texture: Vec<TextureFrame>,
}

fn valid_rate(rate: u32) -> bool {
    (8_000..=96_000).contains(&rate)
}

/// Validate a WAV header before decoding and return a safe capped initial `Vec` capacity
/// so crafted headers with huge declared durations or invalid bit depths cannot overflow or OOM.
pub fn validate_wav_spec(
    spec: hound::WavSpec,
    duration_frames: u32,
    max_seconds: u32,
    allow_stereo: bool,
) -> Result<usize> {
    let channel_ok = if allow_stereo {
        (1..=2).contains(&spec.channels)
    } else {
        spec.channels == 1
    };
    if !channel_ok || !valid_rate(spec.sample_rate) {
        return Err(if allow_stereo {
            "Expected a mono or stereo WAV at 8–96 kHz".into()
        } else {
            "Expected mono WAV at 8–96 kHz".into()
        });
    }
    let bits_ok = match spec.sample_format {
        hound::SampleFormat::Float => spec.bits_per_sample == 32,
        hound::SampleFormat::Int => matches!(spec.bits_per_sample, 8 | 16 | 24 | 32),
    };
    if !bits_ok {
        return Err(format!(
            "Unsupported WAV sample format ({:?}, {}-bit)",
            spec.sample_format, spec.bits_per_sample
        )
        .into());
    }
    let max_frames = u64::from(spec.sample_rate) * u64::from(max_seconds);
    if u64::from(duration_frames) > max_frames {
        return Err(format!("WAV exceeds maximum duration of {max_seconds} seconds").into());
    }
    // Cap upfront allocation to <= 10 seconds of frames so a truncated/crafted header cannot force huge RAM allocation before EOF.
    let cap_frames = u64::from(duration_frames).min(u64::from(spec.sample_rate) * 10);
    Ok(cap_frames as usize)
}

pub fn load_wav(path: &Path) -> Result<(u32, Vec<f32>)> {
    let mut wav = hound::WavReader::open(path)?;
    let s = wav.spec();
    let initial_cap = validate_wav_spec(s, wav.duration(), MAX_SECONDS as u32, false)?;
    let mut x = Vec::with_capacity(initial_cap);
    match s.sample_format {
        hound::SampleFormat::Float => {
            for sample in wav.samples::<f32>() {
                x.push(sample?);
            }
        }
        hound::SampleFormat::Int => {
            let scale = 2_f32.powi(i32::from(s.bits_per_sample) - 1);
            for sample in wav.samples::<i32>() {
                x.push(sample? as f32 / scale);
            }
        }
    }
    if x.is_empty() || x.iter().any(|v| !v.is_finite() || v.abs() > 4.) {
        return Err("Invalid or unsupported mono audio".into());
    }
    Ok((s.sample_rate, x))
}

pub fn analyze_wav(path: &Path) -> Result<Model> {
    let (rate, x) = load_wav(path)?;
    analyze(&x, rate)
}

pub fn analyze(x: &[f32], rate: u32) -> Result<Model> {
    analyze_with_texture(x, rate, false)
}

pub fn analyze_with_texture(x: &[f32], rate: u32, include_texture: bool) -> Result<Model> {
    if !valid_rate(rate)
        || x.is_empty()
        || x.len() > rate as usize * MAX_SECONDS
        || x.iter().any(|x| !x.is_finite() || x.abs() > 4.)
    {
        return Err("Invalid or unsupported mono audio".into());
    }
    let hop = rate as usize / 200;
    let n = (rate as usize / 24).next_power_of_two();
    let win: Vec<_> = (0..n)
        .map(|i| (0.5 - 0.5 * (TAU * i as f64 / n as f64).cos()) as f32)
        .collect();
    let window_sum: f32 = win.iter().sum();
    let stream_fft = FftPlanner::new().plan_fft_forward(n);
    let mut buf = vec![Complex::<f32>::new(0., 0.); n];
    let mut mags = vec![0_f32; n / 2 + 1];
    let mut last = [OFF; MAX_TRACKS];
    let mut frames = Vec::new();
    let mut texture = Vec::new();
    for center in (0..=x.len().div_ceil(hop)).map(|i| i * hop) {
        for (i, z) in buf.iter_mut().enumerate() {
            let idx = (center + i).checked_sub(n / 2);
            *z = Complex::new(
                idx.and_then(|j| x.get(j)).copied().unwrap_or(0.) * win[i],
                0.,
            );
        }
        stream_fft.process(&mut buf);
        for (m, z) in mags.iter_mut().zip(&buf) {
            *m = z.norm();
        }
        let strongest = mags[1..].iter().copied().fold(0_f32, f32::max);
        let floor = (strongest * 0.007).max(window_sum * 0.00004);
        let mut peaks = Vec::new();
        let mut quiet_peaks = Vec::new();
        let bin_hz = rate as f32 / n as f32;
        for k in 1..mags.len() - 1 {
            let m = mags[k];
            if m < (0.6 * floor).max(window_sum * 0.00004) || m <= mags[k - 1] || m < mags[k + 1] {
                continue;
            }
            let a = mags[k - 1].max(1e-20).ln();
            let b = m.ln();
            let c = mags[k + 1].max(1e-20).ln();
            let offset = (0.5 * (a - c) / (a - 2. * b + c).min(-1e-8)).clamp(-0.5, 0.5);
            let hz = (k as f32 + offset) * bin_hz;
            if !(35. ..=12_000.).contains(&hz) {
                continue;
            }
            let amp = 2. * (b - 0.25 * (a - c) * offset).exp() / window_sum;
            let partial = Partial { hz, amplitude: amp };
            if m >= floor {
                peaks.push(partial);
            } else {
                quiet_peaks.push(partial);
            }
        }
        peaks.sort_unstable_by(|a, b| b.amplitude.total_cmp(&a.amplitude));
        peaks.truncate(MAX_TRACKS);
        let mut pairs = Vec::new();
        for (slot, old) in last.iter().enumerate().filter(|(_, p)| p.hz > 0.) {
            for (i, p) in peaks.iter().enumerate() {
                let d = (old.hz - p.hz).abs();
                if d <= (0.8 * bin_hz).max(old.hz * 0.018).min(70.) {
                    pairs.push((d, slot, i));
                }
            }
        }
        pairs.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut next = [OFF; MAX_TRACKS];
        let mut used = [false; MAX_TRACKS];
        for (_, slot, i) in pairs {
            if next[slot].hz == 0. && !used[i] {
                next[slot] = peaks[i];
                used[i] = true;
            }
        }
        for (_, p) in peaks.into_iter().enumerate().filter(|(i, _)| !used[*i]) {
            if let Some(slot) = (0..MAX_TRACKS).find(|&s| last[s].hz == 0. && next[s].hz == 0.) {
                next[slot] = p;
            }
        }
        // Keep the original selection and birth behavior. Only a preexisting
        // unmatched oscillator may use an observed quieter local peak.
        let mut continuations = Vec::new();
        for (slot, old) in last
            .iter()
            .enumerate()
            .filter(|(s, p)| p.hz > 0. && next[*s].hz == 0.)
        {
            for (i, p) in quiet_peaks.iter().enumerate() {
                let d = (old.hz - p.hz).abs();
                if d <= (0.8 * bin_hz).max(old.hz * 0.018).min(70.) {
                    continuations.push((d, slot, i));
                }
            }
        }
        continuations.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut used_quiet = vec![false; quiet_peaks.len()];
        for (_, slot, i) in continuations {
            if next[slot].hz == 0. && !used_quiet[i] {
                next[slot] = quiet_peaks[i];
                used_quiet[i] = true;
            }
        }
        if include_texture {
            let mut bins: [Vec<f32>; TEXTURE_EDGES.len() - 1] = std::array::from_fn(|_| Vec::new());
            for (k, &amp) in mags.iter().enumerate().take(n / 2).skip(1) {
                let Some(b) = band(k as f32 * bin_hz) else {
                    continue;
                };
                if next
                    .iter()
                    .any(|p| p.hz > 0. && (p.hz - k as f32 * bin_hz).abs() < 3.0 * bin_hz)
                {
                    continue;
                }
                bins[b].push(amp * amp);
            }
            let mut broad = [0.; TEXTURE_EDGES.len() - 1];
            for (b, v) in bins.iter_mut().enumerate() {
                if v.len() < 5 {
                    continue;
                }
                v.sort_unstable_by(f32::total_cmp);
                let plateau = v[(v.len() * 3) / 4];
                // Limit the effect of missed narrow tonal peaks. This layer is
                // synthesized noise, not copied residual PCM or source phase.
                broad[b] =
                    (v.iter().map(|power| power.min(plateau)).sum::<f32>() / v.len() as f32).sqrt();
            }
            texture.push(broad);
        }
        frames.push(next);
        last = next;
    }
    Ok(Model {
        version: VERSION,
        sample_rate: rate,
        frames: x.len(),
        hop,
        partials: frames,
        texture,
    })
}

fn initial_phase(seed: u64, frame: usize, slot: usize) -> f64 {
    let mut z =
        seed.wrapping_add(((frame * MAX_TRACKS + slot) as u64).wrapping_mul(0x9e3779b97f4a7c15));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((z ^ (z >> 31)) >> 11) as f64 / ((1u64 << 53) as f64) * TAU
}

pub fn synthesize(model: &Model, seed: u64) -> Result<Vec<f32>> {
    let rate = model.sample_rate;
    if model.version != VERSION
        || !valid_rate(rate)
        || model.hop != rate as usize / 200
        || model.frames == 0
        || model.frames > rate as usize * MAX_SECONDS
        || model.partials.len() != model.frames.div_ceil(model.hop) + 1
        || model.partials.iter().flatten().any(|p| {
            !p.hz.is_finite()
                || !p.amplitude.is_finite()
                || p.hz < 0.
                || p.hz >= rate as f32 / 2.
                || !(0. ..=20.).contains(&p.amplitude)
                || (p.hz == 0. && p.amplitude != 0.)
        })
    {
        return Err("Invalid feature model".into());
    }
    if !model.texture.is_empty()
        && (model.texture.len() != model.partials.len()
            || model
                .texture
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0. ..=32768.).contains(v)))
    {
        return Err("Invalid texture model".into());
    }
    let mut phase = [0.0; MAX_TRACKS];
    let mut y = vec![0.; model.frames];
    for (i, (block, pair)) in y
        .chunks_mut(model.hop)
        .zip(model.partials.windows(2))
        .enumerate()
    {
        for s in 0..MAX_TRACKS {
            let (a, b) = (pair[0][s], pair[1][s]);
            if a.hz == 0. && b.hz == 0. {
                continue;
            }
            if i == 0 || a.hz == 0. {
                phase[s] = initial_phase(seed, i, s);
            }
            let start = if a.hz == 0. { b.hz } else { a.hz };
            let end = if b.hz == 0. { a.hz } else { b.hz };
            for (j, v) in block.iter_mut().enumerate() {
                let t = j as f32 / model.hop as f32;
                let hz = start + (end - start) * t;
                let amp = a.amplitude + (b.amplitude - a.amplitude) * t;
                *v += amp * phase[s].sin() as f32;
                phase[s] = (phase[s] + TAU * hz as f64 / rate as f64) % TAU;
            }
        }
    }
    if !model.texture.is_empty() {
        add_texture(&mut y, model, seed);
    }
    Ok(y)
}

fn add_texture(y: &mut [f32], m: &Model, seed: u64) {
    let rate = m.sample_rate as usize;
    let n = (rate / 24).next_power_of_two();
    let hop = n / 4;
    let fft = FftPlanner::new().plan_fft_inverse(n);
    let mut f = vec![Complex::<f32>::default(); n];
    let mut scratch = vec![Complex::<f32>::default(); fft.get_inplace_scratch_len()];
    let window: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
        .collect();
    let gain = 1.0 / (n as f32 * (3.0_f32 / 8.0).sqrt());
    let bands: Vec<_> = (0..n / 2)
        .map(|i| band(i as f32 * rate as f32 / n as f32))
        .collect();
    let mut noise = vec![0.; y.len()];
    let mut coverage = vec![0.; y.len()];
    for (frame, center) in (0..=y.len().div_ceil(hop)).map(|i| (i, i * hop)) {
        let i = center / m.hop;
        let Some(a) = m.texture.get(i) else {
            break;
        };
        let b = m.texture.get(i + 1).unwrap_or(a);
        let t = (center % m.hop) as f32 / m.hop as f32;
        f.fill(Complex::<f32>::default());
        for k in 1..n / 2 {
            let Some(j) = bands[k] else {
                continue;
            };
            let amp = a[j] + (b[j] - a[j]) * t;
            // Separate deterministic, uncorrelated random phases per block and bin.
            let p =
                initial_phase(seed ^ 0xa0761d6478bd642f, frame * (n / MAX_TRACKS + 1), k) as f32;
            f[k] = Complex::from_polar(amp, p);
            f[n - k] = f[k].conj();
        }
        fft.process_with_scratch(&mut f, &mut scratch);
        for (k, v) in f.iter().enumerate() {
            if let Some(j) = (center + k).checked_sub(n / 2).filter(|j| *j < y.len()) {
                noise[j] += v.re * window[k] * gain;
                coverage[j] += window[k] * window[k];
            }
        }
    }
    for i in 0..y.len() {
        if coverage[i] > 0.0 {
            y[i] += noise[i] / coverage[i].sqrt();
        }
    }
}

pub fn render_wav(model: &Model, seed: u64, path: &Path) -> Result<f32> {
    let y = synthesize(model, seed)?;
    write_wav(model.sample_rate, &y, path)
}

fn write_wav_atomic_no_overwrite(rate: u32, y: &[f32], gain: f32, path: &Path) -> Result<f32> {
    if !valid_rate(rate) || y.is_empty() || !gain.is_finite() || y.iter().any(|v| !v.is_finite()) {
        return Err("Invalid output signal".into());
    }
    if path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Destination WAV already exists",
        )
        .into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("export.wav");
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(".{stem}.tmp-{pid}-{nanos}"));

    let write_res: Result<()> = (|| {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        let mut out = hound::WavWriter::new(
            std::io::BufWriter::new(file),
            hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for &x in y {
            out.write_sample(x * gain)?;
        }
        out.finalize()?;
        // Sync temp file before publishing so interrupted writes never leave a partial WAV.
        let sync_file = std::fs::File::open(&tmp_path)?;
        let _ = sync_file.sync_all();
        // Atomic no-overwrite publish via hard_link (fails cleanly with AlreadyExists if raced).
        match std::fs::hard_link(&tmp_path, path) {
            Ok(()) => {
                let _ = std::fs::remove_file(&tmp_path);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e.into()),
            Err(_) => {
                // Fallback for filesystems without hard links (e.g. FAT32): re-check then rename.
                if path.exists() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "Destination WAV already exists",
                    )
                    .into());
                }
                std::fs::rename(&tmp_path, path)?;
                Ok(())
            }
        }
    })();

    if let Err(err) = write_res {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(gain)
}

/// Write mono 32-bit IEEE float WAV preserving exact samples (gain = 1.0) with atomic
/// no-overwrite publication so interrupted writes never leave partial files on disk.
pub fn write_wav(rate: u32, y: &[f32], path: &Path) -> Result<f32> {
    write_wav_atomic_no_overwrite(rate, y, 1.0, path)
}

/// Explicitly normalize peaks above `target_peak` before writing a mono 32-bit float WAV
/// with atomic no-overwrite publication.
pub fn write_wav_normalized(rate: u32, y: &[f32], path: &Path, target_peak: f32) -> Result<f32> {
    if !target_peak.is_finite() || target_peak <= 0.0 || target_peak > 1.0 {
        return Err("target_peak must be in (0.0, 1.0]".into());
    }
    let peak = y.iter().map(|x| x.abs()).fold(0_f32, f32::max);
    let gain = if peak > target_peak {
        target_peak / peak
    } else {
        1.0
    };
    write_wav_atomic_no_overwrite(rate, y, gain, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_tone_matches_level_and_varies_with_synthesis_seed() {
        let rate = 48000;
        let x: Vec<_> = (0..rate / 4)
            .map(|i| (0.2 * (TAU * 440. * i as f64 / rate as f64).sin()) as f32)
            .collect();
        let m = analyze(&x, rate).unwrap();
        let y = synthesize(&m, 12345).unwrap();
        assert_eq!(y, synthesize(&m, 12345).unwrap());
        assert_ne!(y, synthesize(&m, 6789).unwrap());
        assert_eq!(x.len(), y.len());
        let rms = |a: &[f32]| (a.iter().map(|x| x * x).sum::<f32>() / a.len() as f32).sqrt();
        let a = rms(&y[2400..9600]);
        assert!((a / rms(&x[2400..9600]) - 1.).abs() < 0.08, "{a}");
        let crossings = y[2400..9600]
            .windows(2)
            .filter(|p| p[0] <= 0. && p[1] > 0.)
            .count();
        assert!((65..=67).contains(&crossings), "{crossings}");
    }

    #[test]
    fn quieter_evidenced_partial_continues_without_birthing_or_bridging_silence() {
        let rate = 48_000;
        let x: Vec<_> = (0..rate * 2)
            .map(|i| {
                let t = i as f64 / rate as f64;
                if (1.1..1.5).contains(&t) {
                    return 0.0;
                }
                let level = if (0.3..0.7).contains(&t) {
                    0.012
                } else {
                    0.003
                };
                let elapsed = if t < 1.5 { t } else { t - 1.5 };
                let fade =
                    0.5 - 0.5 * (std::f64::consts::PI * (elapsed / 0.08).clamp(0.0, 1.0)).cos();
                (fade * (0.55 * (TAU * 1_000. * t).sin() + level * (TAU * 3_000. * t).sin())) as f32
            })
            .collect();
        let m = analyze(&x, rate).unwrap();
        let slot = |t: f64| {
            m.partials[(t * rate as f64 / m.hop as f64).round() as usize]
                .iter()
                .position(|p| (p.hz - 3_000.).abs() < 10.)
        };
        assert_eq!(slot(0.15), None);
        assert!(slot(0.55).is_some());
        assert_eq!(slot(0.55), slot(0.95));
        assert_eq!(slot(1.3), None);
        assert_eq!(slot(1.8), None);
        let y = synthesize(&m, 12345).unwrap();
        assert!(y[57_600..67_200].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn texture_model_adds_seeded_sound_and_keeps_silence() {
        let x = vec![0.; 9600];
        let mut m = analyze_with_texture(&x, 48000, true).unwrap();
        assert!(m.texture.iter().flatten().all(|&v| v == 0.));
        assert_eq!(synthesize(&m, 1).unwrap(), x);
        for row in &mut m.texture {
            row[5] = 1.0;
        }
        let a = synthesize(&m, 1).unwrap();
        assert_eq!(a, synthesize(&m, 1).unwrap());
        assert_ne!(a, synthesize(&m, 2).unwrap());
        assert!(a.iter().all(|v| v.is_finite()));
        assert!(a.iter().any(|&v| v.abs() > 0.001));
    }

    #[test]
    fn silence_stays_silent_and_malformed_inputs_fail() {
        let x = vec![0.; 997];
        let mut m = analyze(&x, 44100).unwrap();
        assert_eq!(synthesize(&m, 9).unwrap(), x);
        m.hop = 0;
        assert!(synthesize(&m, 9).is_err());
        assert!(analyze(&[f32::NAN], 48000).is_err());
        assert!(analyze(&[0.], 200).is_err());
    }

    #[test]
    fn validate_wav_spec_rejects_bad_bit_depths_and_caps_initial_allocation() {
        let bad_zero = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 0,
            sample_format: hound::SampleFormat::Int,
        };
        assert!(validate_wav_spec(bad_zero, 48_000, 30, true).is_err());

        let bad_float = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 64,
            sample_format: hound::SampleFormat::Float,
        };
        assert!(validate_wav_spec(bad_float, 48_000, 30, true).is_err());

        let valid_long = hound::WavSpec {
            channels: 2,
            sample_rate: 96_000,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        // 600 s accepted, initial pre-allocation capped to 10 s of frames (960_000)
        assert_eq!(
            validate_wav_spec(valid_long, 96_000 * 600, 600, true).unwrap(),
            960_000
        );
        // 601 s rejected without u32 overflow
        assert!(validate_wav_spec(valid_long, 96_000 * 601, 600, true).is_err());
    }

    #[test]
    fn write_wav_preserves_float32_samples_atomically_without_auto_normalization() {
        let dir = std::env::temp_dir().join(format!(
            "spectracraft-lib-wav-test-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let raw_path = dir.join("exact-float32.wav");
        let norm_path = dir.join("normalized-float32.wav");
        let _ = std::fs::remove_file(&raw_path);
        let _ = std::fs::remove_file(&norm_path);

        let samples = vec![0.0_f32, 0.95, -0.98, 1.02, -0.50, 0.25];
        let gain = write_wav(48_000, &samples, &raw_path).unwrap();
        assert_eq!(gain, 1.0, "ordinary float32 write_wav must not rescale");

        let mut reader = hound::WavReader::open(&raw_path).unwrap();
        let read_back: Vec<f32> = reader.samples::<f32>().map(|s| s.unwrap()).collect();
        assert_eq!(read_back, samples, "written float32 samples must match input bit-for-bit");

        // No-overwrite guarantee
        assert!(
            write_wav(48_000, &samples, &raw_path).is_err(),
            "write_wav must refuse to overwrite an existing WAV"
        );

        // Explicit opt-in normalization rescales peaks above target_peak
        let norm_gain = write_wav_normalized(48_000, &samples, &norm_path, 0.89).unwrap();
        assert!(norm_gain < 1.0);
        let mut norm_reader = hound::WavReader::open(&norm_path).unwrap();
        let norm_peak = norm_reader
            .samples::<f32>()
            .map(|s| s.unwrap().abs())
            .fold(0.0_f32, f32::max);
        assert!((norm_peak - 0.89).abs() < 1e-4, "expected 0.89 peak, got {norm_peak}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

