//! Preset-mapped archive benchmarks. Archive contents are read in Rust, never
//! extracted, and hidden references are used only to score completed renders.
use crate::{edge, metrics, Result};
use std::{
    collections::BTreeSet,
    io::{Cursor, Read},
    path::Path,
};
pub struct Asset {
    pub name: String,
    pub rate: u32,
    pub samples: Vec<f32>,
}
fn wav(bytes: Vec<u8>) -> Result<(u32, Vec<f32>)> {
    let mut r = hound::WavReader::new(Cursor::new(bytes))?;
    let s = r.spec();
    if !(1..=2).contains(&s.channels)
        || !(16000..=96000).contains(&s.sample_rate)
        || r.duration() > s.sample_rate * 120
    {
        return Err("Unsupported benchmark WAV".into());
    }
    let samples = match s.sample_format {
        hound::SampleFormat::Float => r
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 2_f32.powi(s.bits_per_sample as i32 - 1);
            r.samples::<i32>()
                .map(|v| v.map(|v| v as f32 / scale))
                .collect::<std::result::Result<Vec<_>, _>>()?
        }
    };
    if samples.iter().any(|v| !v.is_finite()) {
        return Err("Nonfinite benchmark sample".into());
    }
    Ok((
        s.sample_rate,
        samples
            .chunks_exact(s.channels as usize)
            .map(|v| v.iter().sum::<f32>() / s.channels as f32)
            .collect(),
    ))
}
fn read_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>> {
    let mut file = archive.by_name(name)?;
    if file.size() > 64 * 1024 * 1024 {
        return Err("Benchmark entry exceeds 64 MiB".into());
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}
pub fn assets(root: &Path, pack: &str, scope: &str, needles: &[&str]) -> Result<Vec<Asset>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(root.join(pack))?)?;
    let names: Vec<_> = zip
        .file_names()
        .filter(|n| n.contains(scope) && !n.contains("__MACOSX"))
        .map(str::to_owned)
        .collect();
    let mut refs = BTreeSet::new();
    for preset in names.iter().filter(|n| n.ends_with(".dspreset")) {
        let bytes = read_entry(&mut zip, preset)?;
        let text = std::str::from_utf8(&bytes)?
            .trim_start_matches('\u{feff}')
            .trim()
            .replace("Strings <-> Resonance", "Strings &lt;-&gt; Resonance");
        let mut reader = quick_xml::Reader::from_str(&text);
        loop {
            match reader.read_event()? {
                quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                    if e.name().as_ref() == b"sample" =>
                {
                    for attribute in e.attributes() {
                        let a = attribute?;
                        if a.key.as_ref() == b"path" {
                            let path = a
                                .decoded_and_normalized_value(
                                    quick_xml::XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )?
                                .replace('\\', "/");
                            refs.insert(path.rsplit('/').next().unwrap_or("").to_owned());
                        }
                    }
                }
                quick_xml::events::Event::Eof => break,
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for needle in needles {
        let mut found: Vec<_> = names
            .iter()
            .filter(|n| {
                n.contains(needle)
                    && n.to_lowercase().ends_with(".wav")
                    && refs.contains(n.rsplit('/').next().unwrap_or(""))
            })
            .collect();
        found.sort();
        let name = found
            .first()
            .ok_or_else(|| format!("No preset-mapped sample for {pack}/{scope}/{needle}"))?;
        let (rate, samples) = wav(read_entry(&mut zip, name)?)?;
        out.push(Asset {
            name: (*name).clone(),
            rate,
            samples,
        });
    }
    Ok(out)
}
pub fn prepare(x: &[f32], rate: u32) -> Result<Vec<f32>> {
    let h = (rate as f64 * 0.001).round_ties_even() as usize;
    let e = metrics::envelope(x, h);
    let peak = e.iter().copied().fold(0., f64::max);
    if peak == 0. {
        return Err("Silent benchmark asset".into());
    }
    let first = e.iter().position(|v| *v >= 0.05 * peak).unwrap() * h;
    let start = first.saturating_sub((0.15 * rate as f64).round_ties_even() as usize);
    let end = (start + (1.25 * rate as f64).round_ties_even() as usize).min(x.len());
    let segment = &x[start..end];
    let peak = segment.iter().map(|v| v.abs()).fold(0., f32::max);
    let mut out = vec![0.; (0.08 * rate as f64).round_ties_even() as usize];
    out.extend(segment.iter().map(|v| v * 0.65 / peak));
    Ok(out)
}
pub fn round3(root: &Path) -> Result<serde_json::Value> {
    let groups = [
        (
            "prior",
            vec![
                (
                    "clav.zip",
                    "Clavichord",
                    vec![
                        "/lupe/69_A4_00_00",
                        "/lupe/76_E5_01_00",
                        "/lupe/72_C5_01_00",
                        "/lupe/60_C4_01_00",
                    ],
                ),
                (
                    "ds-example.zip",
                    "example-001-boilerplate",
                    vec!["Wave-C1-", "Wave-C5-"],
                ),
                (
                    "ds-example.zip",
                    "example-002-how-to-make-buttons",
                    vec!["One.wav", "Quatre.wav"],
                ),
            ],
        ),
        (
            "holdout",
            vec![
                (
                    "clav.zip",
                    "Clavichord",
                    vec![
                        "/lupe/65_F4_01_00",
                        "/lupe/67_G4_01_00",
                        "/lupe/55_G3_01_00",
                        "/lupe/81_A5_01_00",
                    ],
                ),
                (
                    "ds-example.zip",
                    "example-001-boilerplate",
                    vec!["Wave-C2-", "Wave-C6-"],
                ),
                (
                    "ds-example.zip",
                    "example-002-how-to-make-buttons",
                    vec!["Three.wav", "Deux.wav"],
                ),
            ],
        ),
    ];
    let mut report = serde_json::Map::new();
    for (group, packs) in groups {
        let mut rows = serde_json::Map::new();
        for (pack, scope, needles) in packs {
            for asset in assets(root, pack, scope, &needles)? {
                eprintln!("Scoring {group}: {}", asset.name);
                let x = prepare(&asset.samples, asset.rate)?;
                let packet = edge::analyze(&x, asset.rate, edge::Options::default())?;
                let mut seeds = serde_json::Map::new();
                let tonal = !asset.name.contains("example-002");
                for seed in [734, 927, 1229] {
                    let y = edge::render(&packet, seed, true, true)?.mix();
                    seeds.insert(
                        seed.to_string(),
                        serde_json::to_value(metrics::frozen_score(&x, &y, asset.rate, tonal)?)?,
                    );
                }
                let before = edge::render(&packet, 734, true, false)?.mix();
                rows.insert(asset.name.clone(),serde_json::json!({"rate":asset.rate,"before":metrics::frozen_score(&x,&before,asset.rate,tonal)?,"seeds":seeds}));
            }
        }
        report.insert(group.into(), rows.into());
    }
    Ok(report.into())
}
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to {
        return x.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let cutoff = 0.46 / ratio.max(1.);
    (0..(x.len() as f64 / ratio).ceil() as usize)
        .map(|i| {
            let t = i as f64 * ratio;
            let center = t.floor() as isize;
            let (mut out, mut sum) = (0., 0.);
            for k in -48..=48 {
                let at = center + k;
                let d = at as f64 - t;
                let window = 0.5 + 0.5 * (std::f64::consts::PI * d / 49.).cos();
                let sinc = if d.abs() < 1e-12 {
                    2. * cutoff
                } else {
                    (std::f64::consts::TAU * cutoff * d).sin() / (std::f64::consts::PI * d)
                };
                let w = sinc * window;
                if at >= 0 && at < x.len() as isize {
                    out += x[at as usize] as f64 * w;
                    sum += w;
                }
            }
            if sum.abs() > 1e-12 {
                (out / sum) as f32
            } else {
                0.
            }
        })
        .collect()
}

pub fn six_samples(root: &Path) -> Result<serde_json::Value> {
    let mut report = serde_json::Map::new();
    for (group, pack, scope, needles) in [
        (
            "bottles",
            "bottles.zip",
            "Bottles",
            vec!["L_RR1_60_C3.wav", "L_RR1_67_G3.wav"],
        ),
        (
            "hurdy",
            "hurdy.zip",
            "Hurdy",
            vec!["67_G4_00_00.wav", "72_C5_00_00.wav"],
        ),
        (
            "glass",
            "glass.zip",
            "Glasharmonica",
            vec!["69_A4_00_00.wav", "72_C5_00_00.wav"],
        ),
    ] {
        let mut rows = Vec::new();
        for asset in assets(root, pack, scope, &needles)? {
            eprintln!("Native/44.1 kHz regression: {}", asset.name);
            let h = (0.005 * asset.rate as f64).round_ties_even() as usize;
            let env = metrics::envelope(&asset.samples, h);
            let peak = env.iter().copied().fold(0., f64::max);
            let first = env
                .iter()
                .position(|v| *v >= 0.12 * peak)
                .ok_or("Silent regression asset")?;
            let start =
                (first * h).saturating_sub((0.12 * asset.rate as f64).round_ties_even() as usize);
            let end = (start + (2.4 * asset.rate as f64).round_ties_even() as usize)
                .min(asset.samples.len());
            let slice = &asset.samples[start..end];
            let peak = slice.iter().map(|v| v.abs()).fold(0., f32::max);
            let native: Vec<_> = slice.iter().map(|v| v * 0.7 / peak).collect();
            let mut scores = serde_json::Map::new();
            for (rate, x) in [
                (asset.rate, native.clone()),
                (44100, resample(&native, asset.rate, 44100)),
            ] {
                let packet = edge::analyze(&x, rate, edge::Options::default())?;
                let y = crate::workspace::render(&packet, 734)?.mix(&Default::default())?;
                let width = (0.005 * rate as f64).round_ties_even() as usize;
                let onset = |v: &[f32]| {
                    let e = metrics::envelope(v, width);
                    let peak = e.iter().copied().fold(0., f64::max);
                    e.iter().position(|v| *v >= 0.1 * peak).unwrap_or(0) * width
                };
                let attack = (onset(&y) as f64 - onset(&x) as f64) * 1000. / rate as f64;
                let (f, a) = metrics::welch(&x, rate, 8192.min(x.len()))?;
                let (_, b) = metrics::welch(&y, rate, 8192.min(y.len()))?;
                let k = (0..f.len())
                    .filter(|&i| f[i] > 85. && f[i] < 1880.)
                    .max_by(|&i, &j| a[i].total_cmp(&a[j]))
                    .ok_or("No reference peak")?;
                let nw = (13. / f[1]).round_ties_even().max(1.) as usize;
                let lo = k.saturating_sub(nw);
                let hi = (k + nw + 1).min(a.len());
                let share = 10.
                    * ((b[lo..hi].iter().sum::<f64>() / b.iter().sum::<f64>()).max(1e-20)
                        / (a[lo..hi].iter().sum::<f64>() / a.iter().sum::<f64>()))
                    .log10();
                let passed = attack.abs() <= 5. && share.abs() <= 2.;
                scores.insert(rate.to_string(),serde_json::json!({"attack_10pct_error_ms":attack,"rebuilt_power_change_at_reference_peak_db_relative_to_total":share,"passed":passed}));
            }
            rows.push(
                serde_json::json!({"source":asset.name,"native_rate":asset.rate,"checks":scores}),
            );
        }
        report.insert(group.into(), rows.into());
    }
    Ok(report.into())
}
pub fn independent(root: &Path) -> Result<serde_json::Value> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(root.join("mecate.dslibrary"))?)?;
    let names: Vec<_> = zip
        .file_names()
        .filter(|n| n.ends_with(".dspreset"))
        .map(str::to_owned)
        .collect();
    let mut refs = BTreeSet::new();
    for name in names {
        let bytes = read_entry(&mut zip, &name)?;
        let text = std::str::from_utf8(&bytes)?
            .trim_start_matches('\u{feff}')
            .trim();
        let mut reader = quick_xml::Reader::from_str(text);
        loop {
            match reader.read_event()? {
                quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                    if e.name().as_ref() == b"sample" =>
                {
                    for a in e.attributes() {
                        let a = a?;
                        if a.key.as_ref() == b"path" {
                            refs.insert(format!(
                                "Mecate/{}",
                                a.decoded_and_normalized_value(
                                    quick_xml::XmlVersion::Implicit1_0,
                                    reader.decoder()
                                )?
                            ));
                        }
                    }
                }
                quick_xml::events::Event::Eof => break,
                _ => {}
            }
        }
    }
    let mut rows = serde_json::Map::new();
    for (voice, note) in [
        ("Bass", "A1"),
        ("Cello", "C3"),
        ("Brass", "C4"),
        ("Piano", "C4"),
    ] {
        let name = format!(
            "Mecate/Samples/{voice}/mecate-{}_{note}.flac",
            voice.to_lowercase()
        );
        if !refs.contains(&name) {
            return Err(format!("FLAC sample not mapped by preset: {name}").into());
        }
        eprintln!("Independent FLAC: {voice} {note}");
        let bytes = read_entry(&mut zip, &name)?;
        let mut reader = claxon::FlacReader::new(Cursor::new(bytes))?;
        let info = reader.streaminfo();
        if !(1..=2).contains(&info.channels)
            || !(16000..=96000).contains(&info.sample_rate)
            || info
                .samples
                .is_some_and(|v| v > info.sample_rate as u64 * 120)
        {
            return Err("Unsupported FLAC asset".into());
        }
        let scale = 2_f32.powi(info.bits_per_sample as i32 - 1);
        let samples = reader
            .samples()
            .map(|v| v.map(|v| v as f32 / scale))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mono: Vec<_> = samples
            .chunks_exact(info.channels as usize)
            .map(|v| v.iter().sum::<f32>() / info.channels as f32)
            .collect();
        let x = prepare(&mono, info.sample_rate)?;
        let packet = edge::analyze(&x, info.sample_rate, edge::Options::default())?;
        let mut seeds = serde_json::Map::new();
        for seed in [734, 927, 1229] {
            let y = edge::render(&packet, seed, true, true)?.mix();
            seeds.insert(
                seed.to_string(),
                serde_json::to_value(metrics::frozen_score(&x, &y, info.sample_rate, true)?)?,
            );
        }
        rows.insert(
            format!("{voice} {note}"),
            serde_json::json!({"rate":info.sample_rate,"seeds":seeds}),
        );
    }
    Ok(rows.into())
}

pub fn masked_holdout(root: &Path) -> Result<serde_json::Value> {
    use crate::masked;
    let (_, wood) = wav(std::fs::read(
        root.parent()
            .ok_or("Missing parent")?
            .join("iowa-woodblock-test.wav"),
    )?)?;
    let (wood_rate, _) = wav(std::fs::read(
        root.parent().unwrap().join("iowa-woodblock-test.wav"),
    )?)?;
    let wood = resample(&wood, wood_rate, 44100);
    let peak = wood.iter().map(|v| v.abs()).fold(0., f32::max);
    let first = wood
        .iter()
        .position(|v| v.abs() > 0.07 * peak)
        .ok_or("Silent woodblock")?;
    let w = &wood[first..(first + 3749).min(wood.len())];
    let mut results = serde_json::Map::new();
    for (pack, scope, notes) in [
        ("bottles.zip", "Bottles", vec!["L_RR1_67_G3.wav"]),
        ("hurdy.zip", "Hurdy", vec!["67_G4_00_00.wav"]),
        (
            "glass.zip",
            "Glasharmonica",
            vec!["69_A4_00_00.wav", "72_C5_00_00.wav"],
        ),
        (
            "ds-example.zip",
            "example-001-boilerplate",
            vec!["Wave-C5-"],
        ),
    ] {
        for asset in assets(root, pack, scope, &notes)? {
            let mut bed = resample(&prepare(&asset.samples, asset.rate)?, asset.rate, 44100);
            bed.truncate(37485);
            let mut rows = Vec::new();
            for injection in std::iter::once(None).chain([0.419, 0.571].into_iter().flat_map(|t| {
                [-20., -26., -32.]
                    .into_iter()
                    .map(move |level| Some((t, level)))
            })) {
                let mut x = bed.clone();
                if let Some((time, level)) = injection {
                    let at = (time * 44100.) as usize;
                    let energy = |v: &[f32]| v.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
                    let gain = 10_f64.powf(level / 20.)
                        * (energy(&x[at..at + w.len()]) / energy(w)).sqrt();
                    for (i, v) in w.iter().enumerate() {
                        x[at + i] += (*v as f64 * gain) as f32;
                    }
                }
                let before = x.clone();
                let p = edge::analyze(&x, 44100, edge::Options::default())?;
                let layers = edge::render(&p, 734, true, true)?;
                let baseline = masked::detect(&layers.mix(), 44100, true)?;
                let hits = |e: &[masked::Event]| -> serde_json::Value {
                    if let Some((t, _)) = injection {
                        e.iter()
                            .any(|e| (e.frame as f64 * e.hop as f64 / 44100. - t).abs() <= 0.006)
                            .into()
                    } else {
                        e.len().into()
                    }
                };
                let mut row = serde_json::json!({"time":injection.map(|v|v.0),"level":injection.map(|v|v.1),"baseline_render":hits(&baseline)});
                for physical in [false, true] {
                    let events = masked::detect(&x, 44100, physical)?;
                    let y = masked::render(&layers.texture, &events, 44100, 736)?;
                    let error = y.energy_error.iter().map(|v| v.abs()).fold(0., f64::max);
                    if error >= 1e-10 {
                        return Err("Attack energy budget violated".into());
                    }
                    let mix: Vec<_> = (0..x.len())
                        .map(|i| {
                            (layers.tone[i] as f64
                                + layers.ring[i] as f64
                                + y.retained[i]
                                + y.attack[i]) as f32
                        })
                        .collect();
                    let recovered = masked::detect(&mix, 44100, true)?;
                    row[if physical { "raw" } else { "whitened" }] = serde_json::json!({"input":hits(&events),"render":hits(&recovered),"off_target":events.iter().filter(|e|injection.is_none_or(|(t,_)|(e.frame as f64*e.hop as f64/44100.-t).abs()>0.006)).count(),"max_energy_error":error});
                }
                if before != x {
                    return Err("Holdout source mutated".into());
                }
                rows.push(row);
            }
            eprintln!("Masked holdout: {}", asset.name);
            results.insert(asset.name, rows.into());
        }
    }
    Ok(results.into())
}
pub fn smoke(root: &Path) -> Result<serde_json::Value> {
    smoke_to(root, None)
}
pub fn smoke_export(root: &Path, out: &Path) -> Result<serde_json::Value> {
    std::fs::create_dir(out)?;
    smoke_to(root, Some(out))
}
fn smoke_to(root: &Path, out: Option<&Path>) -> Result<serde_json::Value> {
    let mut rows = serde_json::Map::new();
    for name in ["violin", "guitar", "woodblock"] {
        let (rate, samples) = wav(std::fs::read(root.join(format!("iowa-{name}-test.wav")))?)?;
        let peak = samples.iter().map(|v| v.abs()).fold(0., f32::max);
        let first = samples
            .iter()
            .position(|v| v.abs() >= 0.035 * peak)
            .ok_or("Silent smoke asset")?;
        let start = first.saturating_sub((0.12 * rate as f64).round() as usize);
        let end = (start + (1.5 * rate as f64).round() as usize).min(samples.len());
        let peak = samples[start..end]
            .iter()
            .map(|v| v.abs())
            .fold(0., f32::max);
        let x: Vec<_> = samples[start..end].iter().map(|v| v * 0.7 / peak).collect();
        let p = crate::harmonic::analyze(&x, rate, Default::default())?;
        let s = crate::harmonic::render(&p, 734, false, false)?;
        let y = s.mix();
        if let Some(out) = out {
            crate::write_wav(
                rate,
                &y,
                &out.join(format!("iowa-{name}-smoke-rebuild.wav")),
            )?;
        }
        let loud = p.envelope.iter().copied().fold(0., f64::max) * 0.1;
        let active: Vec<_> = p
            .envelope
            .iter()
            .enumerate()
            .filter(|(_, v)| **v > loud)
            .map(|(i, _)| i)
            .collect();
        let pitch: Vec<_> = p
            .pitch
            .iter()
            .zip(&p.voiced)
            .filter_map(|(p, v)| v.then_some(*p))
            .collect();
        let energy = |v: &[f32]| v.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
        rows.insert(name.into(),serde_json::json!({"source_clip_offset_sec":start as f64/rate as f64,"render_seconds":x.len() as f64/rate as f64,"recognized_as_tonal_pct_of_loud_frames":100.*active.iter().filter(|&&i|p.voiced[i]).count() as f64/active.len().max(1) as f64,"tracked_frequency_if_active_median_hz":(!pitch.is_empty()).then(||crate::dsp::median(&pitch)),"harmonic_layer_power_relative_to_full_db":10.*(energy(&s.tone).max(1e-15)/energy(&y)).log10(),"input_peak":0.7,"output_peak":y.iter().map(|v|v.abs()).fold(0.,f32::max)}));
    }
    Ok(rows.into())
}

fn attack_metrics(x: &[f32], y: &[f32], rate: u32) -> Result<serde_json::Value> {
    let h = (0.001 * rate as f64).round_ties_even() as usize;
    let e = metrics::envelope(x, h);
    let r = metrics::envelope(y, h);
    let onset = |v: &[f64]| {
        let peak = v.iter().copied().fold(0., f64::max);
        v.iter().position(|v| *v >= 0.05 * peak).unwrap_or(0)
    };
    let on = onset(&e);
    let pre = |v: &[f64]| {
        10. * (v[on.saturating_sub(20)..on]
            .iter()
            .map(|v| v * v)
            .sum::<f64>()
            .max(1e-20)
            / v[on..(on + 25).min(v.len())]
                .iter()
                .map(|v| v * v)
                .sum::<f64>()
                .max(1e-20))
        .log10()
    };
    let (f, a) = metrics::welch(&x[on * h..], rate, 8192.min(x.len() - on * h))?;
    let (_, b) = metrics::welch(&y[on * h..], rate, 8192.min(y.len() - on * h))?;
    let k = (0..f.len())
        .filter(|&i| f[i] >= 15. && f[i] < 5000.)
        .max_by(|&i, &j| a[i].total_cmp(&a[j]))
        .ok_or("No reference peak")?;
    let band = |v: &[f64]| {
        (0..f.len())
            .filter(|&i| (f[i] - f[k]).abs() <= 9_f64.max(1.5 * f[1]))
            .map(|i| v[i])
            .sum::<f64>()
            / v.iter().sum::<f64>().max(1e-20)
    };
    Ok(
        serde_json::json!({"ref_onset_ms":on,"rebuilt_onset_delta_ms":onset(&r) as isize-on as isize,"reference_pre_db":pre(&e),"rebuilt_pre_db":pre(&r),"ref_peak_hz":f[k],"rebuilt_peak_share_delta_db":10.*(band(&b).max(1e-20)/band(&a).max(1e-20)).log10()}),
    )
}
pub fn round2(root: &Path) -> Result<serde_json::Value> {
    let mut cases = Vec::new();
    for (pack, scope, notes) in [
        (
            "clav.zip",
            "Clavichord",
            vec![
                "/lupe/69_A4_00_00",
                "/lupe/76_E5_01_00",
                "/lupe/72_C5_01_00",
                "/lupe/60_C4_01_00",
            ],
        ),
        (
            "ds-example.zip",
            "example-001-boilerplate",
            vec!["Wave-C1-", "Wave-C5-"],
        ),
        (
            "ds-example.zip",
            "example-002-how-to-make-buttons",
            vec!["One.wav", "Quatre.wav"],
        ),
    ] {
        cases.extend(assets(root, pack, scope, &notes)?);
    }
    let (rate, samples) = wav(std::fs::read(
        root.parent()
            .ok_or("No parent")?
            .join("iowa-woodblock-test.wav"),
    )?)?;
    cases.push(Asset {
        name: "Iowa woodblock (separate control)".into(),
        rate,
        samples,
    });
    let mut hashes = BTreeSet::new();
    let mut rows = serde_json::Map::new();
    for asset in cases {
        let hash = crate::digest::sha256(
            &asset
                .samples
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        );
        if !hashes.insert(hash) {
            return Err("Duplicate benchmark source".into());
        }
        let x = prepare(&asset.samples, asset.rate)?;
        let p = edge::analyze(&x, asset.rate, Default::default())?;
        let protected = edge::render(&p, 734, true, false)?.mix();
        let plain = edge::render(&p, 734, false, false)?.mix();
        rows.insert(asset.name,serde_json::json!({"rate":asset.rate,"guard_events":p.base.attacks.len(),"protected":attack_metrics(&x,&protected,asset.rate)?,"guard_off_current":attack_metrics(&x,&plain,asset.rate)?}));
    }
    Ok(rows.into())
}
pub fn sampler_run(root: &Path, out: &Path, mode: &str) -> Result<serde_json::Value> {
    let (guard, shape) = match mode {
        "plain" => (false, false),
        "guard" => (true, false),
        "shape" => (true, true),
        _ => return Err("Mode must be plain, guard, or shape".into()),
    };
    std::fs::create_dir(out)?;
    let mut rows = Vec::new();
    for (group, pack, scope, notes) in [
        (
            "bottles",
            "bottles.zip",
            "Bottles",
            vec!["L_RR1_60_C3.wav", "L_RR1_67_G3.wav"],
        ),
        (
            "hurdy",
            "hurdy.zip",
            "Hurdy",
            vec!["67_G4_00_00.wav", "72_C5_00_00.wav"],
        ),
        (
            "glass",
            "glass.zip",
            "Glasharmonica",
            vec!["69_A4_00_00.wav", "72_C5_00_00.wav"],
        ),
    ] {
        for (index, asset) in assets(root, pack, scope, &notes)?.into_iter().enumerate() {
            let width = (0.005 * asset.rate as f64).round_ties_even() as usize;
            let e = metrics::envelope(&asset.samples, width);
            let peak = e.iter().copied().fold(0., f64::max);
            let first = e
                .iter()
                .position(|v| *v >= 0.12 * peak)
                .ok_or("Silent sampler clip")?
                * width;
            let start = first.saturating_sub((0.12 * asset.rate as f64) as usize);
            let end = (start + (2.4 * asset.rate as f64).round() as usize).min(asset.samples.len());
            let peak = asset.samples[start..end]
                .iter()
                .map(|v| v.abs())
                .fold(0., f32::max);
            let native: Vec<_> = asset.samples[start..end]
                .iter()
                .map(|v| v * 0.7 / peak)
                .collect();
            for (label, rate, x) in [
                ("native", asset.rate, native.clone()),
                ("44100", 44100, resample(&native, asset.rate, 44100)),
            ] {
                let p = edge::analyze(&x, rate, Default::default())?;
                let stems = edge::render(&p, 734, guard, shape)?;
                let accent = crate::workspace::render(&p, 734)?.stems[3].clone();
                let layers = [stems.tone, stems.ring, stems.texture, accent];
                let mix: Vec<_> = (0..x.len())
                    .map(|i| layers.iter().map(|v| v[i] as f64).sum::<f64>())
                    .collect();
                for omit in 0..4 {
                    for i in 0..mix.len() {
                        let sum = layers
                            .iter()
                            .enumerate()
                            .filter(|(j, _)| *j != omit)
                            .map(|(_, v)| v[i] as f64)
                            .sum::<f64>();
                        if (mix[i] - layers[omit][i] as f64 - sum).abs() > 1e-12 {
                            return Err("Layer ablation failed".into());
                        }
                    }
                }
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(out.join(format!("{group}-{}-{label}-{mode}.wav", index + 1)))?;
                let mut w = hound::WavWriter::new(
                    file,
                    hound::WavSpec {
                        channels: 1,
                        sample_rate: rate,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    },
                )?;
                for v in &mix {
                    w.write_sample(*v as f32)?;
                }
                w.finalize()?;
                let energy = |v: &[f32]| v.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
                rows.push(serde_json::json!({"source":asset.name,"source_sample_rate":asset.rate,"render_rate":rate,"mode":mode,"layer_rms_db_relative_to_reference":layers.iter().map(|v|10.*(energy(v).max(1e-20)/energy(&x)).log10()).collect::<Vec<_>>(),"tentative_ring_tracks":p.tracks.len(),"breath_accent_events":crate::workspace::breath_control(&p.base).1.len(),"metrics":attack_metrics(&x,&mix.iter().map(|v|*v as f32).collect::<Vec<_>>(),rate)?,"exact_ablations":true}));
            }
        }
    }
    Ok(rows.into())
}
