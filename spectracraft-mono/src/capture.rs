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
    let initial_cap = crate::validate_wav_spec(s, wav.duration(), 30, true)?;
    let channels = s.channels as usize;
    let mut l = Vec::with_capacity(initial_cap);
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

/// Explicit human listening review state for a dry candidate.
/// No numerical score can promote `NotReviewed` to `ApprovedByHuman`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DryReviewStatus {
    #[default]
    NotReviewed,
    ApprovedByHuman,
}

impl DryReviewStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotReviewed => "Not reviewed",
            Self::ApprovedByHuman => "Approved by human listener",
        }
    }
}

/// Post-dry-review guitar capture variants.
/// Each rendered variant is unmastered with editing headroom; `Original` is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExportVariant {
    Original,
    Natural,
    Digital,
    Analog,
    Expressive,
}

impl ExportVariant {
    pub const ALL: [Self; 5] = [
        Self::Original,
        Self::Natural,
        Self::Digital,
        Self::Analog,
        Self::Expressive,
    ];
    pub fn slug(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Natural => "natural",
            Self::Digital => "digital",
            Self::Analog => "analog",
            Self::Expressive => "expressive",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "Original (unchanged)",
            Self::Natural => {
                "Natural (clean amp-to-mic, SM7-class approx @ ~2 ft, padded 10x10 ft)"
            }
            Self::Digital => "Digital (clean direct/modeled capture path)",
            Self::Analog => "Analog (modeled nonlinear triode/transformer circuit & booth capture)",
            Self::Expressive => {
                "Expressive (pre-capture performance gestures + Natural amp-to-mic)"
            }
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "original" => Some(Self::Original),
            "natural" => Some(Self::Natural),
            "digital" => Some(Self::Digital),
            "analog" => Some(Self::Analog),
            "expressive" => Some(Self::Expressive),
            _ => None,
        }
    }
    /// Parse `"all"` or a comma-separated list of individual variant names.
    pub fn parse_selection(spec: &str) -> Result<Vec<Self>> {
        let trimmed = spec.trim();
        if trimmed.eq_ignore_ascii_case("all") {
            return Ok(Self::ALL.to_vec());
        }
        let mut out = Vec::new();
        for part in trimmed.split(',') {
            let v = Self::parse(part).ok_or_else(|| {
                format!(
                    "Unknown export variant '{}'; expected all or comma-separated original,natural,digital,analog,expressive",
                    part.trim()
                )
            })?;
            if !out.contains(&v) {
                out.push(v);
            }
        }
        if out.is_empty() {
            return Err("At least one export variant must be selected".into());
        }
        Ok(out)
    }
}

/// Standard second-order biquad filter (Direct Form II Transposed).
#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}
impl Biquad {
    fn from_norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.,
            z2: 0.,
        }
    }
    fn highpass(rate: f64, hz: f64, q: f64) -> Self {
        let w = std::f64::consts::TAU * hz.clamp(10., rate * 0.45) / rate;
        let (s, c) = w.sin_cos();
        let alpha = s / (2. * q);
        Self::from_norm(
            (1. + c) * 0.5,
            -(1. + c),
            (1. + c) * 0.5,
            1. + alpha,
            -2. * c,
            1. - alpha,
        )
    }
    fn lowpass(rate: f64, hz: f64, q: f64) -> Self {
        let w = std::f64::consts::TAU * hz.clamp(20., rate * 0.46) / rate;
        let (s, c) = w.sin_cos();
        let alpha = s / (2. * q);
        Self::from_norm(
            (1. - c) * 0.5,
            1. - c,
            (1. - c) * 0.5,
            1. + alpha,
            -2. * c,
            1. - alpha,
        )
    }
    fn peaking(rate: f64, hz: f64, q: f64, gain_db: f64) -> Self {
        let w = std::f64::consts::TAU * hz.clamp(20., rate * 0.45) / rate;
        let (s, c) = w.sin_cos();
        let a = 10_f64.powf(gain_db / 40.);
        let alpha = s / (2. * q);
        Self::from_norm(
            1. + alpha * a,
            -2. * c,
            1. - alpha * a,
            1. + alpha / a,
            -2. * c,
            1. - alpha / a,
        )
    }
    fn high_shelf(rate: f64, hz: f64, q: f64, gain_db: f64) -> Self {
        let w = std::f64::consts::TAU * hz.clamp(20., rate * 0.45) / rate;
        let (s, c) = w.sin_cos();
        let a = 10_f64.powf(gain_db / 40.);
        let beta = (a.sqrt() / q) * s;
        Self::from_norm(
            a * ((a + 1.) + (a - 1.) * c + beta),
            -2. * a * ((a - 1.) + (a + 1.) * c),
            a * ((a + 1.) + (a - 1.) * c - beta),
            (a + 1.) - (a - 1.) * c + beta,
            2. * ((a - 1.) - (a + 1.) * c),
            (a + 1.) - (a - 1.) * c - beta,
        )
    }
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        let z1 = self.b1 * x - self.a1 * y + self.z2;
        let z2 = self.b2 * x - self.a2 * y;
        self.z1 = if z1.abs() < 1e-30 { 0.0 } else { z1 };
        self.z2 = if z2.abs() < 1e-30 { 0.0 } else { z2 };
        if y.abs() < 1e-30 {
            0.0
        } else {
            y
        }
    }
}

fn apply_biquad_chain(input: &[f32], filters: &mut [Biquad]) -> Vec<f32> {
    input
        .iter()
        .map(|&s| {
            let mut v = f64::from(s);
            for f in filters.iter_mut() {
                v = f.process(v);
            }
            v as f32
        })
        .collect()
}

/// Keep unmastered editing headroom (no brickwall limiting; constant scale only if peak > ceiling).
pub fn enforce_editing_headroom(samples: &[f32], peak_ceiling: f32) -> (Vec<f32>, f32) {
    let peak = samples.iter().map(|v| v.abs()).fold(0_f32, f32::max);
    let gain = if peak > peak_ceiling && peak > 0. {
        peak_ceiling / peak
    } else {
        1.0
    };
    (samples.iter().map(|&v| v * gain).collect(), gain)
}

/// Load a mono or stereo WAV up to 10 minutes (600 s) for full-timeline studio navigation.
/// Excerpt regeneration/layer synthesis in desktop remains bounded to <= 30 s selections.
pub fn load_wav_for_studio(path: &Path) -> Result<(u32, Vec<f32>, Recording)> {
    let mut wav = hound::WavReader::open(path)?;
    let s = wav.spec();
    let initial_cap = crate::validate_wav_spec(s, wav.duration(), 600, true)?;
    let channels = s.channels as usize;
    let mut l = Vec::with_capacity(initial_cap);
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

#[derive(Clone, Copy, Debug)]
pub struct StudioPrediction {
    pub speed_of_sound_mps: f64,
    pub distance_m: f32,
    pub direct_pickup_gain: f32,
    pub direct_arrival_ms: f32,
    pub first_reflection_ms: Option<f32>,
    /// Broadband 1 kHz / 6-band mean reflected-to-direct path energy ratio (dB).
    /// Path proxy only; not a measured acoustic DRR.
    pub reflected_path_db: Option<f32>,
    /// Direct-to-Reflected Ratio (DRR) path proxy per octave band [125, 250, 500, 1000, 2000, 4000] Hz.
    pub six_band_drr_db: [Option<f64>; 6],
    pub last_path_ms: f32,
    pub reflection_count: usize,
}

fn plan_to_prediction(plan: &crate::studio_catalog::CapturePlan6) -> StudioPrediction {
    // Convert 6-band DRR (direct/reflected dB) to mean reflected-to-direct proxy (-DRR)
    let mut sum_inv = 0.0_f64;
    let mut count = 0usize;
    for d in plan.six_band_drr_db.into_iter().flatten() {
        sum_inv += 10.0_f64.powf(-d / 10.0);
        count += 1;
    }
    let reflected_path_db =
        (count > 0 && sum_inv > 0.0).then(|| (10.0 * (sum_inv / count as f64).log10()) as f32);
    StudioPrediction {
        speed_of_sound_mps: plan.speed_of_sound_mps,
        distance_m: plan.distance_m as f32,
        direct_pickup_gain: plan.direct_pickup_gain as f32,
        direct_arrival_ms: (plan.direct_arrival_s * 1000.0) as f32,
        first_reflection_ms: plan.first_reflection_s.map(|s| (s * 1000.0) as f32),
        reflected_path_db,
        six_band_drr_db: plan.six_band_drr_db,
        last_path_ms: (plan.last_path_s * 1000.0) as f32,
        reflection_count: plan.paths.len().saturating_sub(1),
    }
}

pub fn predict_studio_rig(
    rig: &crate::studio_catalog::StudioRig,
    sources: &[crate::studio_catalog::SourceEntry],
    mics: &[crate::studio_catalog::MicEntry],
    surfaces: &[crate::studio_catalog::SurfaceEntry],
) -> StudioPrediction {
    let plan = crate::studio_catalog::capture_paths_6band(
        rig,
        sources,
        mics,
        surfaces,
        4,
        crate::studio_catalog::Timing::AlignDirect,
    );
    plan_to_prediction(&plan)
}

fn apply_mic_profile(
    input: &[f32],
    rate: u32,
    profile: crate::studio_catalog::MicRendererProfile,
) -> Vec<f32> {
    use crate::studio_catalog::MicRendererProfile;
    let fs = f64::from(rate);
    let mut filters = match profile {
        MicRendererProfile::FlatIdeal => return input.to_vec(),
        MicRendererProfile::DynamicSm7Class => vec![
            Biquad::highpass(fs, 55., 0.707),
            Biquad::peaking(fs, 200., 0.9, 1.2),
            Biquad::peaking(fs, 3400., 1.1, 1.9),
            Biquad::lowpass(fs, (10_500_f64).min(fs * 0.45), 0.707),
        ],
        MicRendererProfile::DynamicPresence57Class => vec![
            Biquad::highpass(fs, 70., 0.707),
            Biquad::peaking(fs, 4800., 1.2, 2.8),
            Biquad::lowpass(fs, (11_000_f64).min(fs * 0.45), 0.707),
        ],
        MicRendererProfile::SmallCondenserNeutral => vec![
            Biquad::highpass(fs, 30., 0.707),
            Biquad::high_shelf(fs, (8000_f64).min(fs * 0.44), 0.75, 0.7),
        ],
        MicRendererProfile::LargeCondenserWarm => vec![
            Biquad::highpass(fs, 35., 0.707),
            Biquad::peaking(fs, 160., 0.85, 1.1),
            Biquad::peaking(fs, 5500., 1.0, 1.5),
        ],
        MicRendererProfile::RibbonSmooth => vec![
            Biquad::highpass(fs, 40., 0.707),
            Biquad::peaking(fs, 180., 0.85, 1.6),
            Biquad::lowpass(fs, (7500_f64).min(fs * 0.45), 0.65),
        ],
        MicRendererProfile::BoundaryOmniFlat => vec![Biquad::highpass(fs, 20., 0.707)],
    };
    apply_biquad_chain(input, &mut filters)
}

fn build_closed_room_fdn(
    rate: u32,
    rig: &crate::studio_catalog::StudioRig,
    surfaces: &[crate::studio_catalog::SurfaceEntry],
    plan: &crate::studio_catalog::CapturePlan6,
) -> Option<crate::acoustics_physics::LateRoomFdn> {
    use crate::studio_catalog::RoomPreset;
    if matches!(rig.room_preset, RoomPreset::Direct | RoomPreset::Open) {
        return None;
    }
    let dims_m = [
        f64::from(rig.dimensions_m[0]),
        f64::from(rig.dimensions_m[1]),
        f64::from(rig.dimensions_m[2]),
    ];
    let faces_eff_alpha: [[f64; 6]; 6] = std::array::from_fn(|f| {
        surfaces
            .get(rig.surface_faces[f])
            .map(|s| s.effective_absorption())
            .unwrap_or([1.0; 6])
    });
    crate::acoustics_physics::LateRoomFdn::from_room_physics(
        rate,
        dims_m,
        &faces_eff_alpha,
        plan.speed_of_sound_mps,
        plan.distance_m,
        plan.last_path_s.max(0.015),
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct StereoAudio {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

fn apply_stage3_mic_response(
    signal: &[f32],
    rate: u32,
    rig: &crate::studio_catalog::StudioRig,
    mics: &[crate::studio_catalog::MicEntry],
    plan: &crate::studio_catalog::CapturePlan6,
    preamp_drive: f64,
) -> Vec<f32> {
    let slot = rig.active_mic();
    let eff_pattern = slot.effective_pattern(mics);
    let mut prox = crate::acoustics_physics::ProximityFilter::new(
        rate,
        eff_pattern,
        plan.distance_m,
        plan.speed_of_sound_mps,
    );
    let with_prox = prox.apply_slice(signal);

    let with_fr = if let Some(entry) = mics.get(slot.catalog_index) {
        if let Some(fir) = crate::acoustics_physics::minimum_phase_fir_from_fr_points(
            &entry.published_fr_points,
            rate,
            128,
        ) {
            let fir_out = crate::acoustics_physics::apply_fir(&with_prox, &fir);
            if entry.renderer_profile != crate::studio_catalog::MicRendererProfile::FlatIdeal {
                // A 128-tap FIR at 48 kHz lacks sub-30 Hz resolution; add the physical
                // capsule/transformer subsonic rolloff (20 Hz Butterworth) so proximity boost
                // cannot accumulate DC or subsonic runaway.
                let mut dc_hp = [Biquad::highpass(
                    f64::from(rate.max(8_000)),
                    20.0,
                    std::f64::consts::FRAC_1_SQRT_2,
                )];
                apply_biquad_chain(&fir_out, &mut dc_hp)
            } else {
                fir_out
            }
        } else {
            apply_mic_profile(&with_prox, rate, entry.renderer_profile)
        }
    } else {
        with_prox
    };

    if preamp_drive.is_finite() && preamp_drive > 1e-6 {
        let mut preamp = crate::acoustics_physics::PreampStage::new(rate, preamp_drive);
        preamp.apply_slice(&with_fr)
    } else {
        with_fr
    }
}

/// Maximum offline room decay tail rendered beyond the dry input + early reflections (seconds).
pub const MAX_OFFLINE_TAIL_SECONDS: f64 = 10.0;

/// Target decay threshold for dynamic offline room-tail preservation (dB below impulse peak).
pub const TAIL_DECAY_THRESHOLD_DB: f64 = 90.0;

fn trim_silent_tail(buf: &mut Vec<f32>, min_len: usize, threshold_lin: f32) {
    if buf.len() <= min_len {
        return;
    }
    let mut keep = buf.len();
    while keep > min_len && buf[keep - 1].abs() < threshold_lin {
        keep -= 1;
    }
    // Preserve a small 16-sample safety margin after the last audible tail sample.
    let final_len = (keep + 16).min(buf.len()).max(min_len);
    buf.truncate(final_len);
}

fn trim_silent_tail_stereo(left: &mut Vec<f32>, right: &mut Vec<f32>, min_len: usize, threshold_lin: f32) {
    let n = left.len().min(right.len());
    if n <= min_len {
        return;
    }
    let mut keep = n;
    while keep > min_len
        && left[keep - 1].abs() < threshold_lin
        && right[keep - 1].abs() < threshold_lin
    {
        keep -= 1;
    }
    let final_len = (keep + 16).min(n).max(min_len);
    left.truncate(final_len);
    right.truncate(final_len);
}

/// Render a mono dry performance through the active `StudioRig` with explicit timing mode
/// (`Timing::Physical` for canonical calibrated distance attenuation & propagation delay, or
/// `Timing::AlignDirect` for aligned/level-matched monitoring), Stage 3 mic EQ toggle,
/// oversampled preamp drive, and dynamic un-truncated room decay tail preservation.
#[allow(clippy::too_many_arguments)]
pub fn render_studio_rig_with_timing(
    input: &[f32],
    rate: u32,
    rig: &crate::studio_catalog::StudioRig,
    sources: &[crate::studio_catalog::SourceEntry],
    mics: &[crate::studio_catalog::MicEntry],
    surfaces: &[crate::studio_catalog::SurfaceEntry],
    timing: crate::studio_catalog::Timing,
    apply_mic_eq: bool,
    preamp_drive: f64,
    preserve_tail: bool,
) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    let plan = crate::studio_catalog::capture_paths_6band(rig, sources, mics, surfaces, 4, timing);
    let fdn_opt = build_closed_room_fdn(rate, rig, surfaces, &plan);
    let fs = f64::from(rate.max(1));
    let min_physical_len = input.len() + ((plan.last_path_s.max(0.0) * fs).ceil() as usize);
    let mut roomed = if preserve_tail {
        let extra_tail_s = fdn_opt
            .as_ref()
            .map(|f| f.recommended_tail_seconds(TAIL_DECAY_THRESHOLD_DB, MAX_OFFLINE_TAIL_SECONDS))
            .unwrap_or(0.0);
        crate::studio_catalog::render_capture_6band_with_tail(input, rate, &plan, extra_tail_s)
    } else {
        crate::studio_catalog::render_capture_6band(input, rate, &plan)
    };
    let fdn_scale = match timing {
        crate::studio_catalog::Timing::Physical => {
            (crate::studio_catalog::DIRECT_REF_DISTANCE_M / plan.distance_m.max(0.20)).clamp(0.0, 2.0)
        }
        crate::studio_catalog::Timing::AlignDirect => 1.0,
    };
    if let Some(mut fdn) = fdn_opt {
        for (i, dst) in roomed.iter_mut().enumerate() {
            let dry_s = input.get(i).copied().unwrap_or(0.0);
            let clean = if dry_s.is_finite() {
                f64::from(dry_s)
            } else {
                0.0
            };
            let late = fdn.tick(clean) * fdn_scale;
            if late.is_finite() {
                *dst += late as f32;
            }
        }
    }
    let mut out = if !apply_mic_eq {
        roomed
    } else {
        apply_stage3_mic_response(&roomed, rate, rig, mics, &plan, preamp_drive)
    };
    if preserve_tail {
        // Trim only truly inaudible trailing silence (< -96 dBFS) beyond dry + early reflections.
        let min_len = min_physical_len.min(out.len());
        trim_silent_tail(&mut out, min_len, 1.58e-5);
    }
    out
}

/// Render a mono dry performance through the active `StudioRig`.
/// When `preserve_tail` is true (offline capture/export), preserves canonical physical propagation
/// delay, calibrated inverse-distance gain, and complete dynamic room decay. When false (fixed-block
/// auditioning), uses aligned direct timing matching `input.len()`.
#[allow(clippy::too_many_arguments)]
pub fn render_studio_rig_with_options(
    input: &[f32],
    rate: u32,
    rig: &crate::studio_catalog::StudioRig,
    sources: &[crate::studio_catalog::SourceEntry],
    mics: &[crate::studio_catalog::MicEntry],
    surfaces: &[crate::studio_catalog::SurfaceEntry],
    apply_mic_eq: bool,
    preamp_drive: f64,
    preserve_tail: bool,
) -> Vec<f32> {
    let timing = if preserve_tail {
        crate::studio_catalog::Timing::Physical
    } else {
        crate::studio_catalog::Timing::AlignDirect
    };
    render_studio_rig_with_timing(
        input,
        rate,
        rig,
        sources,
        mics,
        surfaces,
        timing,
        apply_mic_eq,
        preamp_drive,
        preserve_tail,
    )
}

/// Render a dry performance into true stereo `[left, right]` with explicit timing mode,
/// decorrelated Householder FDN stereo late-room bloom, and dynamic un-truncated room decay.
#[allow(clippy::too_many_arguments)]
pub fn render_studio_rig_stereo_with_timing(
    input: &[f32],
    rate: u32,
    rig: &crate::studio_catalog::StudioRig,
    sources: &[crate::studio_catalog::SourceEntry],
    mics: &[crate::studio_catalog::MicEntry],
    surfaces: &[crate::studio_catalog::SurfaceEntry],
    timing: crate::studio_catalog::Timing,
    apply_mic_eq: bool,
    preamp_drive: f64,
    preserve_tail: bool,
) -> StereoAudio {
    if input.is_empty() {
        return StereoAudio {
            left: Vec::new(),
            right: Vec::new(),
        };
    }
    let plan = crate::studio_catalog::capture_paths_6band(rig, sources, mics, surfaces, 4, timing);
    let fdn_opt = build_closed_room_fdn(rate, rig, surfaces, &plan);
    let fs = f64::from(rate.max(1));
    let min_physical_len = input.len() + ((plan.last_path_s.max(0.0) * fs).ceil() as usize);
    let early = if preserve_tail {
        let extra_tail_s = fdn_opt
            .as_ref()
            .map(|f| f.recommended_tail_seconds(TAIL_DECAY_THRESHOLD_DB, MAX_OFFLINE_TAIL_SECONDS))
            .unwrap_or(0.0);
        crate::studio_catalog::render_capture_6band_with_tail(input, rate, &plan, extra_tail_s)
    } else {
        crate::studio_catalog::render_capture_6band(input, rate, &plan)
    };
    let fdn_scale = match timing {
        crate::studio_catalog::Timing::Physical => {
            (crate::studio_catalog::DIRECT_REF_DISTANCE_M / plan.distance_m.max(0.20)).clamp(0.0, 2.0)
        }
        crate::studio_catalog::Timing::AlignDirect => 1.0,
    };
    let mut left = early.clone();
    let mut right = early;
    if let Some(mut fdn) = fdn_opt {
        for i in 0..left.len() {
            let dry_s = input.get(i).copied().unwrap_or(0.0);
            let clean = if dry_s.is_finite() {
                f64::from(dry_s)
            } else {
                0.0
            };
            let [late_l, late_r] = fdn.tick_stereo(clean);
            if late_l.is_finite() {
                left[i] += (late_l * fdn_scale) as f32;
            }
            if late_r.is_finite() {
                right[i] += (late_r * fdn_scale) as f32;
            }
        }
    }
    let (mut out_l, mut out_r) = if !apply_mic_eq {
        (left, right)
    } else {
        (
            apply_stage3_mic_response(&left, rate, rig, mics, &plan, preamp_drive),
            apply_stage3_mic_response(&right, rate, rig, mics, &plan, preamp_drive),
        )
    };
    if preserve_tail {
        let min_len = min_physical_len.min(out_l.len());
        trim_silent_tail_stereo(&mut out_l, &mut out_r, min_len, 1.58e-5);
    }
    StereoAudio {
        left: out_l,
        right: out_r,
    }
}

/// Render a dry performance into true stereo `[left, right]` with canonical physical timing when
/// `preserve_tail` is true or aligned fixed-block monitoring when `preserve_tail` is false.
#[allow(clippy::too_many_arguments)]
pub fn render_studio_rig_stereo_with_options(
    input: &[f32],
    rate: u32,
    rig: &crate::studio_catalog::StudioRig,
    sources: &[crate::studio_catalog::SourceEntry],
    mics: &[crate::studio_catalog::MicEntry],
    surfaces: &[crate::studio_catalog::SurfaceEntry],
    apply_mic_eq: bool,
    preamp_drive: f64,
    preserve_tail: bool,
) -> StereoAudio {
    let timing = if preserve_tail {
        crate::studio_catalog::Timing::Physical
    } else {
        crate::studio_catalog::Timing::AlignDirect
    };
    render_studio_rig_stereo_with_timing(
        input,
        rate,
        rig,
        sources,
        mics,
        surfaces,
        timing,
        apply_mic_eq,
        preamp_drive,
        preserve_tail,
    )
}

/// Render a dry performance through the active `StudioRig` for fixed-block aligned monitoring
/// (direct arrival sample-aligned and level-matched, matching `input.len()`).
pub fn render_studio_rig_with_mic_eq(
    input: &[f32],
    rate: u32,
    rig: &crate::studio_catalog::StudioRig,
    sources: &[crate::studio_catalog::SourceEntry],
    mics: &[crate::studio_catalog::MicEntry],
    surfaces: &[crate::studio_catalog::SurfaceEntry],
    apply_mic_eq: bool,
) -> Vec<f32> {
    render_studio_rig_with_timing(
        input,
        rate,
        rig,
        sources,
        mics,
        surfaces,
        crate::studio_catalog::Timing::AlignDirect,
        apply_mic_eq,
        0.0,
        false,
    )
}

/// Render a dry performance through the active `StudioRig` using canonical physical propagation
/// delay, calibrated inverse-distance direct attenuation, and complete dynamic room decay tail
/// (for un-truncated offline WAV capture/export).
pub fn render_studio_rig_with_tail(
    input: &[f32],
    rate: u32,
    rig: &crate::studio_catalog::StudioRig,
    sources: &[crate::studio_catalog::SourceEntry],
    mics: &[crate::studio_catalog::MicEntry],
    surfaces: &[crate::studio_catalog::SurfaceEntry],
    apply_mic_eq: bool,
) -> Vec<f32> {
    render_studio_rig_with_timing(
        input,
        rate,
        rig,
        sources,
        mics,
        surfaces,
        crate::studio_catalog::Timing::Physical,
        apply_mic_eq,
        0.0,
        true,
    )
}

/// Render a dry performance through the active `StudioRig` with the selected mic profile applied.
pub fn render_studio_rig(
    input: &[f32],
    rate: u32,
    rig: &crate::studio_catalog::StudioRig,
    sources: &[crate::studio_catalog::SourceEntry],
    mics: &[crate::studio_catalog::MicEntry],
    surfaces: &[crate::studio_catalog::SurfaceEntry],
) -> Vec<f32> {
    render_studio_rig_with_mic_eq(input, rate, rig, sources, mics, surfaces, true)
}

/// Natural amp-to-mic capture setup: labeled approximation of an SM7-class dynamic
/// cardioid microphone at ~2 ft (0.6096 m) in a foam-treated 10 x 10 x 10 ft room.
pub fn natural_sm7_room_setup() -> Setup {
    Setup {
        room: Room::Studio,
        mic: Mic::Cardioid,
        // 10 ft x 10 ft x 10 ft foam-treated room in metres.
        custom_dimensions: Some([3.048, 3.048, 3.048]),
        // Heavy acoustic foam on walls and ceiling, treated floor pad.
        custom_materials: Some([
            Material::HeavyTreatment,
            Material::Treatment,
            Material::HeavyTreatment,
        ]),
        // 2 ft = 0.6096 m forward from forward-facing source.
        offset: [0.0, 0.6096, 0.0],
    }
}

/// Natural: clean combo amp voicing -> padded 10x10x10 ft room at ~2 ft -> SM7-class
/// dynamic microphone approximation (labeled model, not a measured unit IR).
pub fn render_natural_amp_mic(dry: &[f32], rate: u32) -> Vec<f32> {
    let fs = f64::from(rate);
    // Clean 1x12 open-back guitar combo speaker voicing (linear, clean headroom).
    let mut amp_voicing = vec![
        Biquad::highpass(fs, 75., 0.72),
        Biquad::peaking(fs, 145., 1.1, 1.8),
        Biquad::peaking(fs, 480., 1.0, -1.5),
        Biquad::peaking(fs, 2200., 1.2, 2.4),
        Biquad::lowpass(fs, 5400., 0.78),
    ];
    let amped = apply_biquad_chain(dry, &mut amp_voicing);
    let in_room = render(&amped, rate, natural_sm7_room_setup());
    // Labeled approximation of SM7-class dynamic capsule response (smooth sub-55 Hz rolloff,
    // mild 200 Hz body, gentle 3.4 kHz presence rise, smooth >10 kHz air rolloff).
    let mut sm7_approx = vec![
        Biquad::highpass(fs, 55., 0.707),
        Biquad::peaking(fs, 200., 0.9, 1.2),
        Biquad::peaking(fs, 3400., 1.1, 1.9),
        Biquad::lowpass(fs, (10_500_f64).min(fs * 0.45), 0.707),
    ];
    let mic = apply_biquad_chain(&in_room, &mut sm7_approx);
    enforce_editing_headroom(&mic, 0.707).0
}

/// Digital: clean direct/modeled capture path (linear high-Z buffer, crisp articulation,
/// zero room reflections, unmastered editing headroom).
pub fn render_digital_direct(dry: &[f32], rate: u32) -> Vec<f32> {
    let fs = f64::from(rate);
    let mut direct_path = vec![
        Biquad::highpass(fs, 42., 0.707),
        Biquad::peaking(fs, 2900., 0.85, 1.4),
        Biquad::high_shelf(fs, (7500_f64).min(fs * 0.44), 0.75, 0.8),
    ];
    let out = apply_biquad_chain(dry, &mut direct_path);
    enforce_editing_headroom(&out, 0.707).0
}

/// Analog: distinct modeled nonlinear circuit and close-miked capture path.
/// Uses 2x oversampled asymmetric triode/transformer soft saturation, dynamic
/// power-supply sag on transients, passive tone stack & output-transformer shaping,
/// and tight damped booth reflection. Adds ZERO synthetic 50/60 Hz hum or noise.
pub fn render_analog_circuit(dry: &[f32], rate: u32) -> Vec<f32> {
    let fs = f64::from(rate);
    let os_fs = fs * 2.0;
    // Input high-Z / grid-stopper pre-emphasis.
    let mut pre = vec![
        Biquad::highpass(fs, 62., 0.707),
        Biquad::peaking(fs, 1100., 0.9, 2.0),
    ];
    let pre_sig = apply_biquad_chain(dry, &mut pre);
    let pre_rms = (pre_sig
        .iter()
        .map(|&v| f64::from(v) * f64::from(v))
        .sum::<f64>()
        / pre_sig.len().max(1) as f64)
        .sqrt();
    // Calibrate drive against signal RMS so silence stays exact 0 and normal notes sit
    // in the warm asymmetric knee without hard clipping.
    let drive = if pre_rms > 1e-6 {
        (0.28 / pre_rms).clamp(1.2, 5.5)
    } else {
        1.0
    };
    let dc_bias = 0.11_f64;
    let bias_offset = (dc_bias).tanh();

    // 2x oversampled asymmetric triode stage + dynamic B+ supply sag + output transformer.
    let mut aa_up = Biquad::lowpass(os_fs, fs * 0.43, 0.707);
    let mut aa_dn = Biquad::lowpass(os_fs, fs * 0.43, 0.707);
    let mut dc_block = Biquad::highpass(fs, 28., 0.707);
    let attack_a = (-1.0 / (0.012 * os_fs)).exp();
    let release_a = (-1.0 / (0.140 * os_fs)).exp();
    let mut sag_env = 0.0_f64;
    let mut prev_x = 0.0_f64;
    let mut sat = Vec::with_capacity(pre_sig.len());

    for &sample in &pre_sig {
        let cur_x = f64::from(sample);
        let pair = [0.5 * (prev_x + cur_x), cur_x];
        prev_x = cur_x;
        let mut out_s = 0.0;
        for &x_os in &pair {
            let x_filt = aa_up.process(x_os);
            let abs_x = (x_filt * drive).abs();
            let coeff = if abs_x > sag_env { attack_a } else { release_a };
            sag_env = coeff * sag_env + (1.0 - coeff) * abs_x;
            if sag_env < 1e-30 {
                sag_env = 0.0;
            }
            // Supply sag softly pulls gain down on strong transients (up to ~22%).
            let sag_gain = 1.0 / (1.0 + 0.32 * sag_env);
            let driven = x_filt * drive * sag_gain;
            // Asymmetric triode transfer: exact zero at driven == 0 (zero DC/hum on silence).
            let triode = ((driven + dc_bias).tanh() - bias_offset) / drive;
            // Subtle core hysteresis / transformer even-odd warmth.
            let xformer = triode * (1.0 - 0.10 * (triode * drive).abs().min(1.0));
            out_s = aa_dn.process(xformer);
        }
        sat.push(dc_block.process(out_s) as f32);
    }

    // Passive tone stack (slight mid scoop + transformer low-mid warmth + 4.4 kHz speaker rolloff).
    let mut post = vec![
        Biquad::peaking(fs, 175., 0.9, 2.2),
        Biquad::peaking(fs, 620., 1.1, -2.4),
        Biquad::peaking(fs, 2500., 1.3, 1.6),
        Biquad::lowpass(fs, (4600_f64).min(fs * 0.44), 0.78),
    ];
    let shaped = apply_biquad_chain(&sat, &mut post);

    // Close-miked damped booth capture (~0.32 m forward from speaker baffle).
    let booth = Setup {
        room: Room::Booth,
        mic: Mic::Cardioid,
        custom_dimensions: None,
        custom_materials: Some([Material::HeavyTreatment; 3]),
        offset: [0.04, 0.32, -0.02],
    };
    let captured = render(&shaped, rate, booth);
    enforce_editing_headroom(&captured, 0.707).0
}

/// Enforce explicit human dry-review approval before rendering a capture variant.
pub fn render_guitar_variant_checked(
    review: DryReviewStatus,
    variant: ExportVariant,
    original: &[f32],
    cues: &crate::reperform::Performance,
) -> Result<Vec<f32>> {
    if review != DryReviewStatus::ApprovedByHuman {
        return Err(
            "Variant export blocked: dry performance is Not reviewed. No numerical score can approve the dry take; human listening review is required."
                .into(),
        );
    }
    render_guitar_variant(variant, original, cues)
}

/// Render a selected variant from the unchanged original and source-free cue packet.
/// `Expressive` alters performance cues BEFORE dry synthesis and Natural amp-to-mic capture.
pub fn render_guitar_variant(
    variant: ExportVariant,
    original: &[f32],
    cues: &crate::reperform::Performance,
) -> Result<Vec<f32>> {
    let rate = cues.rate();
    match variant {
        ExportVariant::Original => Ok(original.to_vec()),
        ExportVariant::Natural => {
            let dry = cues.synthesize_dry()?;
            Ok(render_natural_amp_mic(&dry, rate))
        }
        ExportVariant::Digital => {
            let dry = cues.synthesize_dry()?;
            Ok(render_digital_direct(&dry, rate))
        }
        ExportVariant::Analog => {
            let dry = cues.synthesize_dry()?;
            Ok(render_analog_circuit(&dry, rate))
        }
        ExportVariant::Expressive => {
            let expressive_cues = cues.apply_expressive_gestures()?;
            let dry = expressive_cues.synthesize_dry()?;
            Ok(render_natural_amp_mic(&dry, rate))
        }
    }
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
    #[test]
    fn studio_rig_single_variable_changes_produce_real_acoustic_differences() {
        use crate::studio_catalog::*;
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        let rate = 48000;
        let mut impulse = vec![0.0_f32; 4800];
        impulse[40] = 0.8;

        let base = StudioRig::default();
        let y_base = render_studio_rig(&impulse, rate, &base, &sources, &mics, &surfaces);
        let p_base = predict_studio_rig(&base, &sources, &mics, &surfaces);

        // 1. Visual-only toggles (unit + Patterns overlay) MUST NOT change audio.
        let mut visual_only = base.clone();
        visual_only.unit = Unit::Millimetres;
        visual_only.show_patterns = true;
        assert_eq!(
            render_studio_rig(&impulse, rate, &visual_only, &sources, &mics, &surfaces),
            y_base
        );

        // 2. Geometry change (Small -> Large) lengthens wall reflection paths & changes tail audio
        // (floor bounce at identical source/mic Z height remains the earliest bounce).
        let mut large = base.clone();
        large.apply_preset(RoomPreset::Large);
        let p_large = predict_studio_rig(&large, &sources, &mics, &surfaces);
        assert!(p_large.first_reflection_ms.unwrap() >= p_base.first_reflection_ms.unwrap());
        assert!(p_large.last_path_ms > p_base.last_path_ms);
        assert_ne!(
            render_studio_rig(&impulse, rate, &large, &sources, &mics, &surfaces),
            y_base
        );

        // 3. Material change (2" Wedge Foam -> Oak Wood -> Rough Concrete -> Sheet Steel) monotonically raises reflection energy.
        let mut prev_db = p_base.reflected_path_db.unwrap();
        for mat_idx in [15usize, 21usize, 28usize] {
            let mut mrig = base.clone();
            mrig.surface_faces = [mat_idx; 6];
            let pm = predict_studio_rig(&mrig, &sources, &mics, &surfaces);
            assert!(
                pm.reflected_path_db.unwrap() > prev_db,
                "expected material {mat_idx} reflection dB {} > {prev_db}",
                pm.reflected_path_db.unwrap()
            );
            prev_db = pm.reflected_path_db.unwrap();
            assert_ne!(
                render_studio_rig(&impulse, rate, &mrig, &sources, &mics, &surfaces),
                y_base
            );
        }

        // 4. Aim change (0 deg toward source -> 90 deg off-axis) changes pickup without moving mic!
        let mut aimed = base.clone();
        aimed.active_mic_mut().aim.follow_source = false;
        aimed.active_mic_mut().aim.heading_z_deg = 90.0;
        let p_90 = predict_studio_rig(&aimed, &sources, &mics, &surfaces);
        assert_eq!(aimed.active_mic().offset_m, base.active_mic().offset_m);
        assert!(p_90.direct_pickup_gain < p_base.direct_pickup_gain - 0.3);
        let y_90 = render_studio_rig(&impulse, rate, &aimed, &sources, &mics, &surfaces);
        assert_ne!(y_90, y_base);

        // 5. Open field has ground reflection only (reflection_count == 1).
        let mut open = base.clone();
        open.apply_preset(RoomPreset::Open);
        let p_open = predict_studio_rig(&open, &sources, &mics, &surfaces);
        assert_eq!(p_open.reflection_count, 1);

        // 6. 6-band DRR path proxy reflects 2-inch wedge foam strong HF absorption vs LF reflection
        assert!(p_base.six_band_drr_db[0].is_some());
        assert!(
            p_base.six_band_drr_db[3].is_none(),
            "2-inch foam 1 kHz absorption > 1.0 clamps reflectance to 0 -> no 1 kHz reflection energy"
        );
    }
}
