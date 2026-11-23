//! Native editable layer workspace: immutable source-free packet, four generated
//! stems, fixed audition headroom, and independent view/mute/solo/gain controls.
use crate::{
    dsp, edge,
    harmonic::{self, BANDS, HOP},
    Result,
};
use serde::{Deserialize, Serialize};
pub const NAMES: [&str; 4] = ["Pitch", "Faint ringing", "Texture", "Breath accent"];
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Controls {
    pub gain_db: [f32; 4],
    pub muted: [bool; 4],
    pub solo: Option<usize>,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            gain_db: [0.; 4],
            muted: [false; 4],
            solo: None,
        }
    }
}
impl Controls {
    pub fn gains(&self) -> Result<[f32; 4]> {
        if self.solo.is_some_and(|v| v >= 4)
            || self
                .gain_db
                .iter()
                .any(|v| !v.is_finite() || !(-60. ..=6.).contains(v))
        {
            return Err("Invalid layer controls".into());
        }
        Ok(std::array::from_fn(|i| {
            if self.muted[i] || self.solo.is_some_and(|s| s != i) {
                0.
            } else {
                10_f32.powf(self.gain_db[i] / 20.)
            }
        }))
    }
}
pub struct Rendered {
    pub stems: [Vec<f32>; 4],
    pub breath_onsets: Vec<f64>,
    pub spans: [Vec<[f64; 2]>; 4],
}
impl Rendered {
    pub fn mix(&self, c: &Controls) -> Result<Vec<f32>> {
        let gains = c.gains()?;
        let n = self.stems[0].len();
        if self
            .stems
            .iter()
            .any(|s| s.len() != n || s.iter().any(|v| !v.is_finite()))
        {
            return Err("Invalid generated stems".into());
        }
        Ok((0..n)
            .map(|i| (0..4).map(|j| gains[j] * self.stems[j][i]).sum())
            .collect())
    }
    pub fn headroom(&self, reference: &[f32]) -> f32 {
        let ref_peak = reference.iter().map(|v| v.abs()).fold(0., f32::max);
        let stem_peak = (0..self.stems[0].len())
            .map(|i| self.stems.iter().map(|s| s[i].abs()).sum::<f32>() * 10_f32.powf(6. / 20.))
            .fold(0., f32::max);
        0.95 / ref_peak.max(stem_peak).max(0.95)
    }
    /// Sample-local edit with zero effect outside the requested exclusive range.
    pub fn apply_region_gain(
        &mut self,
        layer: usize,
        start: usize,
        end: usize,
        db: f32,
        fade: usize,
    ) -> Result<()> {
        if layer >= 4
            || start >= end
            || end > self.stems[layer].len()
            || !db.is_finite()
            || !(-60. ..=6.).contains(&db)
            || fade == 0
            || fade > (end - start) / 2
        {
            return Err("Invalid layer edit range".into());
        }
        let gain = 10_f32.powf(db / 20.);
        for i in start..end {
            let u = ((i - start).min(end - 1 - i) as f64 / fade as f64).min(1.);
            let w = (0.5 - 0.5 * (std::f64::consts::PI * u).cos()) as f32;
            self.stems[layer][i] *= 1. + (gain - 1.) * w;
        }
        Ok(())
    }
}
fn reflected(i: isize, n: usize) -> usize {
    let n = n as isize;
    let k = i.rem_euclid(n * 2);
    if k < n {
        k as usize
    } else {
        (2 * n - 1 - k) as usize
    }
}
pub fn breath_control(p: &harmonic::Packet) -> (Vec<f64>, Vec<f64>) {
    let n = p.pitch.len();
    let stable: Vec<_> = (0..n)
        .map(|i| {
            (-13..=13)
                .filter(|&d| p.voiced[reflected(i as isize + d, n)])
                .count() as f64
                / 27.
                > 0.81
        })
        .collect();
    let entrances: Vec<_> = (0..n)
        .filter(|&i| stable[i] && (i == 0 || !stable[i - 1]))
        .map(|i| (i * HOP) as f64 / p.rate as f64)
        .collect();
    let weights: [f64; 10] =
        std::array::from_fn(|i| (BANDS[i + 1].min(8000.) - BANDS[i].max(1000.)).max(0.));
    let energy: Vec<_> = p
        .texture
        .iter()
        .map(|tex| tex.iter().zip(weights).map(|(p, w)| p * w).sum::<f64>())
        .collect();
    let mut control = vec![0_f64; n];
    let mut active = Vec::new();
    for onset in entrances {
        let reference: Vec<_> = (0..n)
            .filter(|&i| {
                let t = (i * HOP) as f64 / p.rate as f64;
                t >= onset - 0.30 && t < onset - 0.135
            })
            .map(|i| energy[i])
            .collect();
        let gate: Vec<_> = (0..n)
            .filter(|&i| {
                let t = (i * HOP) as f64 / p.rate as f64;
                t > onset - 0.105 && t < onset + 0.105
            })
            .collect();
        if reference.is_empty() || gate.is_empty() {
            continue;
        }
        let floor = dsp::median(&reference);
        let amp: Vec<_> = energy.iter().map(|v| (v - floor).max(0.).sqrt()).collect();
        let peak = gate.iter().map(|&i| amp[i]).fold(0., f64::max);
        if peak < 1e-9 || peak * peak < 3. * floor.max(1e-18) {
            continue;
        }
        for i in gate {
            let t = (i * HOP) as f64 / p.rate as f64;
            let u = ((t - onset) / 0.105).clamp(-1., 1.);
            let shape =
                (amp[i] / peak).clamp(0., 1.) * (0.5 + 0.5 * (std::f64::consts::PI * u).cos());
            control[i] = control[i].max(shape);
        }
        active.push(onset);
    }
    (control, active)
}
fn spans(flag: &[bool], rate: u32) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let mut start = None;
    for i in 0..=flag.len() {
        if i < flag.len() && flag[i] {
            start.get_or_insert(i);
        } else if let Some(a) = start.take() {
            if i - a > 4 {
                out.push([
                    (a * HOP) as f64 / rate as f64,
                    (i.min(flag.len() - 1) * HOP) as f64 / rate as f64,
                ]);
            }
        }
    }
    out
}
pub fn render(p: &edge::Packet, seed: u64) -> Result<Rendered> {
    let stems = edge::render(p, seed, true, true)?;
    let (control, onsets) = breath_control(&p.base);
    let noise = harmonic::render_inner(&p.base, seed, false, false, false)?.texture;
    let input: Vec<_> = noise.iter().map(|v| *v as f64).collect();
    let breath = dsp::bandpass(
        &input,
        p.base.rate,
        1000.,
        8000_f64.min(p.base.rate as f64 * 0.49),
        3,
    );
    let accent: Vec<_> = breath
        .iter()
        .enumerate()
        .map(|(i, v)| {
            ((10_f64.powf(7. * dsp::interp(&control, i as f64 / HOP as f64) / 20.) - 1.) * v) as f32
        })
        .collect();
    let pitch_spans = spans(&p.base.voiced, p.base.rate);
    let ring_spans = p
        .tracks
        .iter()
        .map(|t| {
            [
                (t.frames[0] * HOP) as f64 / p.base.rate as f64,
                (t.frames.last().unwrap() * HOP) as f64 / p.base.rate as f64,
            ]
        })
        .collect();
    let accent_spans = spans(
        &control.iter().map(|v| *v > 0.).collect::<Vec<_>>(),
        p.base.rate,
    );
    Ok(Rendered {
        stems: [stems.tone, stems.ring, stems.texture, accent],
        breath_onsets: onsets,
        spans: [pitch_spans, ring_spans, Vec::new(), accent_spans],
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_and_region_edits_cannot_touch_other_layers_or_reference() {
        let mut w = Rendered {
            stems: std::array::from_fn(|i| vec![(i + 1) as f32 * 0.01; 100]),
            breath_onsets: vec![],
            spans: std::array::from_fn(|_| vec![]),
        };
        let before = w.stems.clone();
        let original = vec![0.1; 100];
        let fixed = w.headroom(&original);
        for i in 0..4 {
            let c = Controls {
                solo: Some(i),
                ..Controls::default()
            };
            assert_eq!(w.mix(&c).unwrap(), w.stems[i]);
        }
        assert!(w
            .mix(&Controls {
                muted: [true; 4],
                ..Controls::default()
            })
            .unwrap()
            .iter()
            .all(|v| *v == 0.));
        w.apply_region_gain(2, 20, 70, 6., 5).unwrap();
        for i in [0, 1, 3] {
            assert_eq!(w.stems[i], before[i]);
        }
        assert_eq!(w.stems[2][..=20], before[2][..=20]);
        assert_eq!(w.stems[2][69..], before[2][69..]);
        assert!(w.stems[2][40] > before[2][40]);
        assert!(fixed > 0.);
        assert_eq!(original, vec![0.1; 100]);
    }
}
