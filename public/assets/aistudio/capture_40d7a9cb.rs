//! Bounded image-source experiment. It predicts an editable virtual setup;
//! it does not reconstruct the historical microphone/room from a recording.
use crate::Result;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Room {
    Direct,
    Studio,
    Hall,
    Booth,
    Chamber,
    Open,
}
impl Room {
    pub const CATALOG: [Self; 6] = [
        Self::Direct,
        Self::Booth,
        Self::Studio,
        Self::Chamber,
        Self::Hall,
        Self::Open,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Direct => "No added room",
            Self::Studio => "Treated studio",
            Self::Hall => "Hard live room",
            Self::Booth => "Small treated booth",
            Self::Chamber => "Wood chamber",
            Self::Open => "Open air · ground only",
        }
    }
    pub fn dimensions(self) -> [f32; 3] {
        match self {
            Self::Direct | Self::Studio => [4., 3., 2.7],
            Self::Hall => [12., 9., 5.5],
            Self::Booth => [2.4, 1.9, 2.3],
            Self::Chamber => [7.5, 5.4, 3.4],
            Self::Open => [20., 20., 10.],
        }
    }
    pub fn materials(self) -> [Material; 3] {
        use Material::*;
        match self {
            Self::Direct => [OpenBoundary; 3],
            Self::Booth => [HeavyTreatment; 3],
            Self::Studio => [Treatment; 3],
            Self::Chamber => [WoodLike, WoodLike, Treatment],
            Self::Hall => [PlasterLike; 3],
            Self::Open => [OpenBoundary, HeavyTreatment, OpenBoundary],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Material {
    OpenBoundary,
    HeavyTreatment,
    Treatment,
    WoodLike,
    PlasterLike,
    ConcreteLike,
    MetalLike,
}
impl Material {
    pub const CATALOG: [Self; 7] = [
        Self::OpenBoundary,
        Self::HeavyTreatment,
        Self::Treatment,
        Self::WoodLike,
        Self::PlasterLike,
        Self::ConcreteLike,
        Self::MetalLike,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenBoundary => "No reflection",
            Self::HeavyTreatment => "Heavy treatment",
            Self::Treatment => "Acoustic treatment",
            Self::WoodLike => "Wood-like",
            Self::PlasterLike => "Hard plaster-like",
            Self::ConcreteLike => "Concrete-like",
            Self::MetalLike => "Metal-like",
        }
    }
    /// Illustrative broadband pressure reflectance; not a measured absorption coefficient.
    pub fn coefficient(self) -> f32 {
        match self {
            Self::OpenBoundary => 0.,
            Self::HeavyTreatment => 0.18,
            Self::Treatment => 0.38,
            Self::WoodLike => 0.65,
            Self::PlasterLike => 0.82,
            Self::ConcreteLike => 0.90,
            Self::MetalLike => 0.96,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mic {
    Cardioid,
    Omni,
    Wide,
    Super,
    FigureEight,
}
impl Mic {
    pub const CATALOG: [Self; 5] = [
        Self::Omni,
        Self::Wide,
        Self::Cardioid,
        Self::Super,
        Self::FigureEight,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Omni => "Omni",
            Self::Wide => "Wide cardioid",
            Self::Cardioid => "Cardioid",
            Self::Super => "Supercardioid",
            Self::FigureEight => "Figure-eight",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "omni" => Some(Self::Omni),
            "wide" => Some(Self::Wide),
            "cardioid" => Some(Self::Cardioid),
            "super" => Some(Self::Super),
            "figure8" => Some(Self::FigureEight),
            _ => None,
        }
    }
    pub fn pressure(self, cosine: f32) -> f32 {
        let c = cosine.clamp(-1., 1.);
        match self {
            Self::Omni => 1.,
            Self::Wide => 0.70 + 0.30 * c,
            Self::Cardioid => 0.5 * (1. + c),
            Self::Super => 0.37 + 0.63 * c,
            Self::FigureEight => c,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setup {
    pub room: Room,
    pub mic: Mic,
    pub custom_dimensions: Option<[f32; 3]>,
    /// One choice for vertical walls, then floor and ceiling.
    pub custom_materials: Option<[Material; 3]>,
    /// Microphone offsets [left/right, forward, up] in metres.
    pub offset: [f32; 3],
}
impl Default for Setup {
    fn default() -> Self {
        Self {
            room: Room::Studio,
            mic: Mic::Cardioid,
            custom_dimensions: None,
            custom_materials: None,
            offset: [0., 1., 0.15],
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Prediction {
    pub distance_m: f32,
    pub first_reflection_ms: Option<f32>,
    /// Ratio of the squared individual modeled reflected path gains to direct.
    /// Not a measured acoustic DRR; coherent overlapping paths may add.
    pub reflected_path_db: Option<f32>,
    pub last_path_ms: f32,
}
impl Setup {
    pub fn dimensions(self) -> [f32; 3] {
        let base = self.room.dimensions();
        if let Some(custom) = self.custom_dimensions {
            std::array::from_fn(|i| {
                if custom[i].is_finite() {
                    custom[i].clamp(1.5, 40.)
                } else {
                    base[i]
                }
            })
        } else {
            base
        }
    }
    pub fn materials(self) -> [Material; 3] {
        let m = self
            .custom_materials
            .unwrap_or_else(|| self.room.materials());
        match self.room {
            Room::Direct => [Material::OpenBoundary; 3],
            Room::Open => [Material::OpenBoundary, m[1], Material::OpenBoundary],
            _ => m,
        }
    }

    pub fn source(self) -> [f32; 3] {
        let [w, d, _] = self.dimensions();
        [w / 2., d * 0.27, 1.05]
    }
    pub fn clamp(self) -> Self {
        let mut s = self;
        let size = self.dimensions();
        let src = self.source();
        for i in 0..3 {
            let v = if self.offset[i].is_finite() {
                self.offset[i]
            } else {
                0.
            };
            s.offset[i] = v.clamp(0.10 - src[i], size[i] - src[i] - 0.10);
        }
        // Avoid point-source singularity; enforce a small minimum separation.
        if norm(s.offset) < 0.2 {
            s.offset = [0., 0.2, 0.];
        }
        s
    }
}
fn norm(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn axis_image(p: f32, l: f32, n: i32) -> f32 {
    if n % 2 == 0 {
        n as f32 * l + p
    } else {
        (n + 1) as f32 * l - p
    }
}

fn paths(setup: Setup) -> (Prediction, Vec<(f32, f32)>) {
    let s = setup.clamp();
    let src = s.source();
    let at = std::array::from_fn::<_, 3, _>(|i| src[i] + s.offset[i]);
    let size = s.dimensions();
    let dist = norm(s.offset);
    let [wall, floor, ceiling] = s.materials().map(Material::coefficient);
    let mut paths = vec![(0., 1.)]; // align the direct arrival for alternatives
    let mut energy = 0.;
    let mut first = f32::INFINITY;
    let mut last = 0_f32;
    if wall > 0. || floor > 0. || ceiling > 0. {
        for x in -6i32..=6 {
            for y in -6i32..=6 {
                for z in -6i32..=6 {
                    let order = x.abs() + y.abs() + z.abs();
                    if !(1..=6).contains(&order) {
                        continue;
                    }
                    if s.room == Room::Open && (x != 0 || y != 0 || z != -1) {
                        continue;
                    }
                    let pos = [x, y, z];
                    let v = std::array::from_fn::<_, 3, _>(|i| {
                        axis_image(src[i], size[i], pos[i]) - at[i]
                    });
                    let travel = norm(v);
                    let sec = (travel - dist) / 343.;
                    let cosine = (0..3).map(|i| v[i] * -s.offset[i]).sum::<f32>() / (travel * dist);
                    // The sign of the first image determines the first surface on
                    // that axis. Odd orders have one extra hit on that surface.
                    let half = z.abs() / 2;
                    let down = half + if z < 0 { z.abs() % 2 } else { 0 };
                    let up = z.abs() - down;
                    let reflect =
                        wall.powi(x.abs() + y.abs()) * floor.powi(down) * ceiling.powi(up);
                    let g = reflect * dist / travel * s.mic.pressure(cosine);
                    if g.abs() < 0.0001 || sec <= 0. {
                        continue;
                    }
                    paths.push((sec, g));
                    energy += g * g;
                    first = first.min(sec);
                    last = last.max(sec);
                }
            }
        }
    }
    (
        Prediction {
            distance_m: dist,
            first_reflection_ms: first.is_finite().then_some(first * 1000.),
            reflected_path_db: (energy > 0.).then(|| 10. * energy.log10()),
            last_path_ms: last * 1000.,
        },
        paths,
    )
}
pub fn predict(s: Setup) -> Prediction {
    paths(s).0
}

pub fn render(input: &[f32], rate: u32, s: Setup) -> Vec<f32> {
    render_length(input, rate, s, false)
}

/// Export the complete modeled room tail rather than truncating at clip end.
pub fn render_with_tail(input: &[f32], rate: u32, s: Setup) -> Vec<f32> {
    render_length(input, rate, s, true)
}

fn render_length(input: &[f32], rate: u32, s: Setup, tail: bool) -> Vec<f32> {
    if input.is_empty() || s.room == Room::Direct {
        return input.to_vec();
    }
    let (p, taps) = paths(s);
    if taps.len() == 1 {
        return input.to_vec();
    }
    let ir_len = (p.last_path_ms * rate as f32 / 1000.).ceil() as usize + 2;
    let n = (input.len() + ir_len - 1).next_power_of_two();
    let mut a = vec![Complex32::default(); n];
    let mut b = vec![Complex32::default(); n];
    for (i, v) in input.iter().copied().enumerate() {
        a[i].re = v;
    }
    for (delay, gain) in taps {
        let f = delay * rate as f32;
        let i = f.floor() as usize;
        let frac = f - i as f32;
        b[i].re += (1. - frac) * gain;
        b[i + 1].re += frac * gain;
    }
    let mut plan = FftPlanner::new();
    let forward = plan.plan_fft_forward(n);
    forward.process(&mut a);
    forward.process(&mut b);
    for (a, b) in a.iter_mut().zip(b) {
        *a *= b;
    }
    plan.plan_fft_inverse(n).process(&mut a);
    a[..if tail {
        input.len() + ir_len - 1
    } else {
        input.len()
    }]
        .iter()
        .map(|x| x.re / n as f32)
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub struct Recording {
    pub channels: u16,
    pub left_minus_right_db: Option<f64>,
    pub lr_similarity: Option<f64>,
}
pub fn load_wav_for_capture(path: &Path) -> Result<(u32, Vec<f32>, Recording)> {
    let mut wav = hound::WavReader::open(path)?;
    let s = wav.spec();
    if !(1..=2).contains(&s.channels) || !(8_000..=96_000).contains(&s.sample_rate) {
        return Err("Expected a mono or stereo WAV at 8–96 kHz".into());
    }
    if wav.duration() > s.sample_rate * 30 {
        return Err("This preview supports WAV excerpts up to 30 seconds".into());
    }
    let channels = s.channels as usize;
    let mut l = Vec::with_capacity(wav.duration() as usize);
    let (mut ll, mut rr, mut lr, mut sl, mut sr) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    let mut last = 0.0_f64;
    let scale = 2_f32.powi(i32::from(s.bits_per_sample) - 1);
    let mut it: Box<dyn Iterator<Item = std::result::Result<f32, hound::Error>> + '_> =
        match s.sample_format {
            hound::SampleFormat::Float => Box::new(wav.samples::<f32>()),
            hound::SampleFormat::Int => {
                Box::new(wav.samples::<i32>().map(|r| r.map(|x| x as f32 / scale)))
            }
        };
    for (i, x) in it.by_ref().enumerate() {
        let x = x?;
        if !x.is_finite() || x.abs() > 4. {
            return Err("Invalid WAV sample".into());
        }
        if i % channels == 0 {
            l.push(x);
            last = x as f64;
        } else {
            let r = x as f64;
            ll += last * last;
            rr += r * r;
            lr += last * r;
            sl += last;
            sr += r;
        }
    }
    if l.is_empty() {
        return Err("Empty WAV".into());
    }
    let n = l.len() as f64;
    let den = ((ll - sl * sl / n).max(0.) * (rr - sr * sr / n).max(0.)).sqrt();
    Ok((
        s.sample_rate,
        l,
        Recording {
            channels: s.channels,
            left_minus_right_db: (ll > 0. && rr > 0.).then(|| 10. * (ll / rr).log10()),
            lr_similarity: (den > 0.).then(|| (lr - sl * sr / n) / den),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn virtual_mic_and_room_change_paths_not_direct_timing() {
        let mut s = Setup::default();
        let a = predict(s);
        let x = vec![0.0; 4000];
        assert_eq!(render(&x, 48000, s), x);
        let mut impulse = x;
        impulse[30] = 1.0;
        let y = render(&impulse, 48000, s);
        assert_eq!(y.len(), impulse.len());
        assert!((y[30] - 1.).abs() < 1e-5);
        assert!(y[31..].iter().any(|v| v.abs() > 0.001));
        s.room = Room::Hall;
        assert!(predict(s).last_path_ms > a.last_path_ms);
        let b = predict(s);
        s.mic = Mic::Omni;
        assert!(predict(s).reflected_path_db.unwrap() > b.reflected_path_db.unwrap());
        s.room = Room::Direct;
        assert_eq!(render(&impulse, 48000, s), impulse);
    }
    #[test]
    fn catalog_surfaces_size_and_signed_capsule_are_independent() {
        let base = Setup::default();
        let metal = Setup {
            custom_materials: Some([Material::MetalLike; 3]),
            ..base
        };
        let a = predict(base);
        let b = predict(metal);
        assert_eq!(a.distance_m, b.distance_m);
        assert_eq!(a.first_reflection_ms, b.first_reflection_ms);
        assert!(b.reflected_path_db.unwrap() > a.reflected_path_db.unwrap() + 6.);
        let big = Setup {
            custom_dimensions: Some([12., 9., 5.5]),
            ..base
        };
        assert!(predict(big).last_path_ms > predict(base).last_path_ms);
        assert_eq!(Mic::FigureEight.pressure(-1.), -1.);
        assert_eq!(Mic::Cardioid.pressure(-1.), 0.);
        let open = Setup {
            room: Room::Open,
            ..base
        };
        assert_eq!(paths(open).1.len(), 2);
    }
    #[test]
    fn stereo_cues_do_not_replace_left_channel() {
        let file = std::env::temp_dir().join(format!("spectral-cues-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&file, spec).unwrap();
        for x in [0.5f32, -0.5, 0.25, -0.25] {
            w.write_sample(x).unwrap();
            w.write_sample(x * 0.5).unwrap();
        }
        w.finalize().unwrap();
        let (_, l, r) = load_wav_for_capture(&file).unwrap();
        std::fs::remove_file(file).unwrap();
        assert_eq!(l, [0.5, -0.5, 0.25, -0.25]);
        assert!((r.left_minus_right_db.unwrap() - 6.0206).abs() < 1e-4);
        assert!((r.lr_similarity.unwrap() - 1.).abs() < 1e-6);
    }
}
