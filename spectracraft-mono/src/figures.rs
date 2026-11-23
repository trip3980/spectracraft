//! Standalone SVG reports written entirely in Rust with shared numeric scales.
use crate::{metrics, qa, Result};
use std::{fmt::Write, path::Path};
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
pub fn heatmap(
    panels: &[(&str, &[f32], f64, f64)],
    rate: u32,
    range: (f64, f64),
    windows: &[(f64, f64)],
) -> Result<String> {
    let height = 100 + panels.len() * 195;
    let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"{height}\" viewBox=\"0 0 1000 {height}\"><rect width=\"100%\" height=\"100%\" fill=\"#f5f7fb\"/><g font-family=\"sans-serif\" fill=\"#162437\"><text x=\"70\" y=\"30\" font-size=\"22\">SpectraCraft · shared-scale comparison</text><text x=\"70\" y=\"55\">Amplitude dBFS: {} to {} · 0–8 kHz · seconds</text>",range.0,range.1);
    for (p, (name, x, lo, hi)) in panels.iter().enumerate() {
        let (f, t, z) = metrics::stft(x, rate, 1024, 128);
        let top = 100 + p * 195;
        write!(
            svg,
            "<text x=\"70\" y=\"{}\" font-size=\"17\">{}</text>",
            top - 12,
            escape(name)
        )?;
        for col in 0..560 {
            let time = lo + (hi - lo) * (col as f64 + 0.5) / 560.;
            let frame = t.partition_point(|v| *v < time).min(t.len() - 1);
            for row in 0..64 {
                let hz = (row as f64 + 0.5) * 8000. / 64.;
                let bin = f.partition_point(|v| *v < hz).min(f.len() - 1);
                let db = 20. * (2. * z[frame][bin]).max(1e-9).log10();
                let u = ((db - range.0) / (range.1 - range.0)).clamp(0., 1.);
                let r = (255. * u.sqrt()) as u8;
                let g = (230. * u.powi(2)) as u8;
                let b = (30. + 90. * (std::f64::consts::PI * u).sin()) as u8;
                write!(svg,"<rect x=\"{:.2}\" y=\"{:.2}\" width=\"1.51\" height=\"2.27\" fill=\"#{r:02x}{g:02x}{b:02x}\"/>",70.+col as f64*1.5,top as f64+(63-row) as f64*2.25)?;
            }
        }
        for &(center, width) in windows {
            for edge in [center - width, center + width] {
                if edge >= *lo && edge <= *hi {
                    let x = 70. + 840. * (edge - lo) / (hi - lo);
                    write!(svg,"<path d=\"M{x:.2},{top}v144\" stroke=\"#a1dcf7\" stroke-dasharray=\"4 3\"/>")?;
                }
            }
        }
        for i in 0..=5 {
            write!(
                svg,
                "<text x=\"{}\" y=\"{}\" font-size=\"12\">{:.2}</text>",
                70 + i * 168,
                top + 163,
                lo + (hi - lo) * i as f64 / 5.
            )?;
        }
        write!(
            svg,
            "<text x=\"15\" y=\"{}\">8 kHz</text><text x=\"22\" y=\"{}\">0</text>",
            top + 10,
            top + 143
        )?;
    }
    svg.push_str("</g></svg>");
    Ok(svg)
}
pub fn rms_plot(
    signals: &[(&str, &[f32])],
    rate: u32,
    views: &[(f64, f64)],
    marker: f64,
) -> Result<String> {
    let height = 90 + views.len() * 240;
    let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"{height}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><g font-family=\"sans-serif\"><text x=\"70\" y=\"30\" font-size=\"22\">Matched capture · 10 ms RMS, dBFS</text>");
    for (p, (lo, hi)) in views.iter().enumerate() {
        let top = 70 + p * 240;
        for db in [-80, -60, -40, -20, 0] {
            let y = top as f64 - db as f64 / 80. * 180.;
            write!(
                svg,
                "<path d=\"M70,{y}h840\" stroke=\"#ddd\"/><text x=\"20\" y=\"{y}\">{db}</text>"
            )?;
        }
        for (i, (name, x)) in signals.iter().enumerate() {
            let color = ["#114e7d", "#892c53", "#006d62"][i % 3];
            let e = metrics::envelope(x, (rate as f64 * 0.01).round() as usize);
            let mut path = String::new();
            for (k, v) in e.iter().enumerate() {
                let t = (k as f64 + 0.5) * 0.01;
                if t < *lo || t > *hi {
                    continue;
                }
                write!(
                    path,
                    "{}{:0.2},{:0.2}",
                    if path.is_empty() { "M" } else { "L" },
                    70. + 840. * (t - lo) / (hi - lo),
                    top as f64 - 20. * v.max(1e-4).log10() / 80. * 180.
                )?;
            }
            write!(svg,"<path d=\"{path}\" stroke=\"{color}\" fill=\"none\" stroke-width=\"1.5\"/><text x=\"{}\" y=\"{}\" fill=\"{color}\">{}</text>",70+i*270,top+220,escape(name))?;
        }
        if marker >= *lo && marker <= *hi {
            let x = 70. + 840. * (marker - lo) / (hi - lo);
            write!(
                svg,
                "<path d=\"M{x},{top}v180\" stroke=\"#555\" stroke-dasharray=\"3 3\"/>"
            )?;
        }
        write!(
            svg,
            "<text x=\"70\" y=\"{}\">{lo:.2} seconds</text><text x=\"830\" y=\"{}\">{hi:.2}</text>",
            top + 200,
            top + 200
        )?;
    }
    svg.push_str("</g></svg>");
    Ok(svg)
}
pub fn accent(input: &Path, out: &Path) -> Result<serde_json::Value> {
    let (s, x) = qa::read_audio(input)?;
    if s.sample_rate != 44100 || x.len() != 557892 {
        return Err("Unexpected flute".into());
    }
    let reference = &x[..256896];
    let p = crate::edge::analyze(
        reference,
        44100,
        crate::edge::Options {
            profile: crate::harmonic::Profile::FluteReference,
            ..Default::default()
        },
    )?;
    let r = crate::workspace::render(&p, 734)?;
    let plain: Vec<_> = (0..reference.len())
        .map(|i| r.stems[0][i] + r.stems[1][i] + r.stems[2][i])
        .collect();
    let accented = r.mix(&Default::default())?;
    let added: Vec<_> = accented.iter().zip(&plain).map(|(a, b)| a - b).collect();
    let windows: Vec<_> = r
        .breath_onsets
        .iter()
        .map(|v| (*v, 0.105 + 1. / 44100.))
        .collect();
    let outside = added
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            !windows
                .iter()
                .any(|(c, w)| (*i as f64 / 44100. - c).abs() <= *w)
        })
        .map(|(_, v)| v.abs())
        .fold(0., f32::max);
    if outside > 1. / 32768. || added.iter().all(|v| *v == 0.) {
        return Err("Accent difference window gate failed".into());
    }
    let n = reference.len() as f64 / 44100.;
    let svg = heatmap(
        &[
            ("Original", reference, 0., n),
            ("Plain rebuild", &plain, 0., n),
            ("Breath accent", &accented, 0., n),
            ("Added audio: accent minus plain", &added, 0., n),
            ("First breath: added audio", &added, 0.43, 0.70),
        ],
        44100,
        (-104., -29.),
        &windows,
    )?;
    qa::write_new(out, svg.as_bytes())?;
    Ok(
        serde_json::json!({"panels":5,"shared_scale_dbfs":[-104,-29],"outside_max":outside,"difference_is_audio_subtraction":true}),
    )
}

pub fn line_panels(panels: &[(&str, Vec<(String, Vec<[f64; 2]>)>, [f64; 4])]) -> Result<String> {
    let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"{}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><g font-family=\"sans-serif\">",panels.len()*300);
    for (i, (title, series, range)) in panels.iter().enumerate() {
        let top = 45 + i * 300;
        write!(
            svg,
            "<text x=\"70\" y=\"{}\" font-size=\"20\">{}</text>",
            top - 20,
            escape(title)
        )?;
        for j in 0..=4 {
            let y = top + 200 - j * 50;
            write!(
                svg,
                "<path d=\"M70,{y}h840\" stroke=\"#ddd\"/><text x=\"8\" y=\"{y}\">{:.2}</text>",
                range[2] + (range[3] - range[2]) * j as f64 / 4.
            )?;
        }
        for (j, (name, points)) in series.iter().enumerate() {
            let color = ["#114e7d", "#892c53", "#006d62"][j % 3];
            let mut path = String::new();
            for &[x, y] in points {
                if x < range[0] || x > range[1] {
                    continue;
                }
                write!(
                    path,
                    "{}{:.2},{:.2}",
                    if path.is_empty() { "M" } else { "L" },
                    70. + 840. * (x - range[0]) / (range[1] - range[0]),
                    top as f64 + 200. - 200. * (y - range[2]) / (range[3] - range[2])
                )?;
            }
            write!(svg,"<path d=\"{path}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.5\"/><text x=\"{}\" y=\"{}\" fill=\"{color}\">{}</text>",70+j*280,top+250,escape(name))?;
        }
        write!(
            svg,
            "<text x=\"70\" y=\"{}\">{:.0} Hz</text><text x=\"840\" y=\"{}\">{:.0} Hz</text>",
            top + 220,
            range[0],
            top + 220,
            range[1]
        )?;
    }
    svg.push_str("</g></svg>");
    Ok(svg)
}

/// Standalone Rust SVG waveform comparison on one shared numeric amplitude scale.
pub fn waveform_panels(
    panels: &[(&str, &[f32], f64, f64)],
    rate: u32,
    shared_peak: f32,
) -> Result<String> {
    if panels.is_empty() || rate == 0 || !shared_peak.is_finite() || shared_peak <= 0. {
        return Err("Invalid waveform panel specification".into());
    }
    let scale = shared_peak.max(1e-4) as f64;
    let height = 95 + panels.len() * 170;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1000\" height=\"{height}\" viewBox=\"0 0 1000 {height}\"><rect width=\"100%\" height=\"100%\" fill=\"#f5f7fb\"/><g font-family=\"sans-serif\" fill=\"#162437\"><text x=\"70\" y=\"30\" font-size=\"22\">SpectraCraft · same-scale waveforms</text><text x=\"70\" y=\"53\" font-size=\"13\">Shared vertical scale: \u{00b1}{scale:.3} full-scale linear · time in seconds</text>"
    );
    let palette = ["#114e7d", "#006d62", "#892c53", "#8a4b08", "#4c3575"];
    for (p, (name, x, lo, hi)) in panels.iter().enumerate() {
        let top = 85 + p * 170;
        let mid = top as f64 + 62.;
        let half_h = 54.;
        let color = palette[p % palette.len()];
        let start_idx = ((*lo * rate as f64).floor().max(0.) as usize).min(x.len());
        let end_idx = ((*hi * rate as f64).ceil().max(0.) as usize).min(x.len());
        let slice = &x[start_idx..end_idx.max(start_idx)];
        let peak_val = slice.iter().map(|v| v.abs()).fold(0_f32, f32::max);
        let peak_str = if peak_val > 0. {
            format!("{:+.1} dBFS peak", 20. * (peak_val as f64).log10())
        } else {
            "silence".to_owned()
        };
        write!(
            svg,
            "<text x=\"70\" y=\"{}\" font-size=\"16\">{}</text><text x=\"770\" y=\"{}\" font-size=\"12\" fill=\"#4b5b70\">{}</text>",
            top - 8,
            escape(name),
            top - 8,
            peak_str
        )?;
        write!(
            svg,
            "<rect x=\"70\" y=\"{top}\" width=\"840\" height=\"124\" fill=\"#ffffff\" stroke=\"#d0d9e6\"/><path d=\"M70,{mid:.1}h840\" stroke=\"#d8e1ec\"/>"
        )?;
        write!(
            svg,
            "<text x=\"10\" y=\"{}\" font-size=\"11\">+{scale:.2}</text><text x=\"32\" y=\"{:.1}\" font-size=\"11\">0</text><text x=\"10\" y=\"{}\" font-size=\"11\">-{scale:.2}</text>",
            top + 12,
            mid + 4.,
            top + 120
        )?;
        if !slice.is_empty() {
            let cols = 840usize;
            let mut bars = String::new();
            for col in 0..cols {
                let a = col * slice.len() / cols;
                let b = ((col + 1) * slice.len() / cols).max(a + 1).min(slice.len());
                let (lo_v, hi_v) = slice[a..b]
                    .iter()
                    .fold((0_f32, 0_f32), |(mn, mx), &v| (mn.min(v), mx.max(v)));
                let y1 = mid - (hi_v as f64 / scale).clamp(-1., 1.) * half_h;
                let y2 = mid - (lo_v as f64 / scale).clamp(-1., 1.) * half_h;
                let xx = 70. + col as f64 + 0.5;
                write!(bars, "M{xx:.1},{y1:.2}V{y2:.2}")?;
            }
            write!(
                svg,
                "<path d=\"{bars}\" stroke=\"{color}\" stroke-width=\"1\"/>"
            )?;
        }
        for i in 0..=5 {
            write!(
                svg,
                "<text x=\"{}\" y=\"{}\" font-size=\"12\">{:.2}</text>",
                70 + i * 168,
                top + 142,
                lo + (hi - lo) * i as f64 / 5.
            )?;
        }
    }
    svg.push_str("</g></svg>");
    Ok(svg)
}
