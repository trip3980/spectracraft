//! Native studio catalogs and 6-band virtual capture rig engine for SpectraCraft.
//!
//! Offline `capture-reference` datasets embedded at compile time (zero runtime network access):
//! - 138 real acoustic source entries (Unknown, Instrument families, Human Male/Female/Other-Ensemble)
//! - 972 real microphones across 88 brands recovered from Mic Archive (`capture-reference/mic-archive-972.json`,
//!   939 with published frequency-response curves, 667 with published off-axis polar tables; ZERO invented
//!   specs; every entry separates published manufacturer data from the renderer's modeled polar + parametric class)
//! - 32 acoustic surface material profiles (`capture-reference/acoustic-materials-32.json`) with raw published
//!   6-band octave absorption (125–4000 Hz), clamped passive effective absorption `alpha_eff`, reflectance
//!   `sqrt(1 - alpha_eff)`, assembly/standard metadata, and explicit unfabricated Biot-JCA parameter slots
//! - Exact 6-band image-source acoustics across 6 independent room faces (`[-X, +X, -Y, +Y, floor, ceiling]`)
//!   using complementary 1-pole crossover splitting (unity sum when band gains are equal)
//! - Single SI metre coordinate system with lossless ruler unit conversion, chain-linkable room
//!   dimensions, multi-mic slots, and full 3D aim controls.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const FEET_TO_METRES: f32 = 0.3048;

/// Reference microphone-to-source distance (0.40 m) where an on-axis unity omni microphone
/// has calibrated 0 dB direct sensitivity (inverse-distance 1/r attenuation is computed relative
/// to this reference distance in `Timing::Physical` capture mode).
pub const DIRECT_REF_DISTANCE_M: f64 = 0.40;

/// Octave-band centre frequencies (Hz) for 6-band surface reflection and DRR reporting.
pub const BANDS_HZ: [f64; 6] = [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0];

/// Ideal-gas speed of sound in dry air (m/s) anchored at standard 343.0 m/s at 20 °C.
pub fn speed_of_sound_celsius(celsius: f64) -> f64 {
    let t = celsius.clamp(-40.0, 60.0);
    343.0 * ((t + 273.15) / 293.15).sqrt()
}

/// Convert MIDI note number to equal-temperament frequency in Hz (A4 = 69 = 440 Hz).
pub fn midi_to_hz(midi: f64) -> f64 {
    440.0 * 2.0_f64.powf((midi - 69.0) / 12.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    Millimetres,
    Centimetres,
    Metres,
    Inches,
    Feet,
    Yards,
}

impl Unit {
    pub const ALL: [Self; 6] = [
        Self::Millimetres,
        Self::Centimetres,
        Self::Metres,
        Self::Inches,
        Self::Feet,
        Self::Yards,
    ];
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Millimetres => "mm",
            Self::Centimetres => "cm",
            Self::Metres => "m",
            Self::Inches => "in",
            Self::Feet => "ft",
            Self::Yards => "yd",
        }
    }
    pub fn metres_per_unit(self) -> f32 {
        match self {
            Self::Millimetres => 0.001,
            Self::Centimetres => 0.01,
            Self::Metres => 1.0,
            Self::Inches => 0.0254,
            Self::Feet => FEET_TO_METRES,
            Self::Yards => 0.9144,
        }
    }
    pub fn from_metres(self, m: f32) -> f32 {
        m / self.metres_per_unit()
    }
    pub fn to_metres(self, v: f32) -> f32 {
        v * self.metres_per_unit()
    }
    pub fn decimals(self) -> usize {
        match self {
            Self::Millimetres => 0,
            Self::Centimetres | Self::Inches => 1,
            Self::Metres | Self::Feet | Self::Yards => 2,
        }
    }
    /// Clean human-friendly grid square size in metres for a given visible span.
    pub fn nice_grid_step_m(self, visible_span_m: f32) -> f32 {
        let span_u = self.from_metres(visible_span_m.max(0.1));
        let raw = span_u / 8.0;
        let mag = 10_f32.powf(raw.max(1e-4).log10().floor());
        let norm = raw / mag;
        let step_u = if norm <= 1.5 {
            1.0 * mag
        } else if norm <= 3.0 {
            2.0 * mag
        } else if norm <= 7.0 {
            5.0 * mag
        } else {
            10.0 * mag
        };
        self.to_metres(step_u.max(0.01))
    }
}

/// First-order analytical microphone polar patterns (including Hypercardioid = 0.25).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pattern {
    Omni,
    WideCardioid,
    Cardioid,
    Supercardioid,
    Hypercardioid,
    Figure8,
}

impl Pattern {
    pub const ALL: [Self; 6] = [
        Self::Omni,
        Self::WideCardioid,
        Self::Cardioid,
        Self::Supercardioid,
        Self::Hypercardioid,
        Self::Figure8,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Omni => "Omni",
            Self::WideCardioid => "Wide cardioid",
            Self::Cardioid => "Cardioid",
            Self::Supercardioid => "Supercardioid",
            Self::Hypercardioid => "Hypercardioid",
            Self::Figure8 => "Figure-8",
        }
    }
    /// First-order pattern coefficient `a` in `p(cos θ) = a + (1 - a) * cos θ`.
    pub fn alpha(self) -> f64 {
        match self {
            Self::Omni => 1.0,
            Self::WideCardioid => 0.70,
            Self::Cardioid => 0.50,
            Self::Supercardioid => 0.37,
            Self::Hypercardioid => 0.25,
            Self::Figure8 => 0.0,
        }
    }
    pub fn pressure(self, cos_theta: f64) -> f64 {
        let a = self.alpha();
        a + (1.0 - a) * cos_theta.clamp(-1.0, 1.0)
    }
}

/// Optional 1D measured polar sample at `angle_deg` (0° = on-axis, 180° = rear).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolarSample {
    pub angle_deg: f64,
    pub pressure: f64,
}

/// Directivity model for a receiver capsule: either ideal analytical or measured 1D samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Directivity {
    Ideal(Pattern),
    Measured1d(&'static [PolarSample]),
}

impl Directivity {
    /// Evaluate signed pressure sensitivity for `cos_theta`.
    /// For `Measured1d`, returns `None` if samples are empty or angle falls outside supplied range.
    pub fn pressure(self, cos_theta: f64) -> Option<f64> {
        let c = cos_theta.clamp(-1.0, 1.0);
        match self {
            Self::Ideal(p) => Some(p.pressure(c)),
            Self::Measured1d(samples) => {
                if samples.is_empty() {
                    return None;
                }
                let deg = c.acos().to_degrees();
                if deg < samples.first()?.angle_deg || deg > samples.last()?.angle_deg {
                    return None;
                }
                for pair in samples.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    if deg >= a.angle_deg && deg <= b.angle_deg {
                        let span = (b.angle_deg - a.angle_deg).max(1e-9);
                        let t = (deg - a.angle_deg) / span;
                        return Some(a.pressure + (b.pressure - a.pressure) * t);
                    }
                }
                Some(samples.last()?.pressure)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCategory {
    Unknown,
    Instrument,
    Human,
}

impl SourceCategory {
    pub const ALL: [Self; 3] = [Self::Unknown, Self::Instrument, Self::Human];
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Instrument => "Instrument",
            Self::Human => "Human",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRadiation {
    Omni,
    ForwardWide,
    ForwardCardioid,
    DipoleOpenBack,
    DirectionalBell,
}

impl SourceRadiation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Omni => "Omnidirectional-like",
            Self::ForwardWide => "Forward-biased wide",
            Self::ForwardCardioid => "Forward cardioid-like",
            Self::DipoleOpenBack => "Dipole / open-back",
            Self::DirectionalBell => "Directional forward bell",
        }
    }
    /// Renderer outgoing amplitude gain as a function of cosine between forward (+Y) and ray.
    pub fn gain(self, cos_forward: f64) -> f64 {
        let c = cos_forward.clamp(-1.0, 1.0);
        match self {
            Self::Omni => 1.0,
            Self::ForwardWide => 0.78 + 0.22 * c,
            Self::ForwardCardioid => 0.58 + 0.42 * c,
            Self::DipoleOpenBack => 0.30 + 0.70 * c.abs(),
            Self::DirectionalBell => 0.42 + 0.58 * c.max(-0.5),
        }
    }
    pub fn directivity_alpha(self) -> f32 {
        match self {
            Self::Omni => 0.0,
            Self::ForwardWide => 0.22,
            Self::ForwardCardioid => 0.42,
            Self::DipoleOpenBack => 0.70,
            Self::DirectionalBell => 0.58,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceSymbol {
    Unknown,
    Guitar,
    Amp,
    Bowed,
    Wind,
    Brass,
    Percussion,
    Keyboard,
    Voice,
    Ensemble,
}

/// Broad published spectral character regions (reference only — never clamps cues).
#[derive(Clone, Debug, PartialEq)]
pub struct SpectralRegions {
    pub body_warmth_hz: Option<(f32, f32)>,
    pub presence_attack_hz: Option<(f32, f32)>,
    pub air_high_hz: Option<(f32, f32)>,
    pub formants_and_resonances_note: String,
}

/// Explicit parametric regeneration synthesis family used by `reperform::Performance::synthesize_dry_for_source`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRegenKind {
    Neutral,
    PluckedElectric,
    PluckedAcoustic,
    PluckedBass,
    BowedLow,
    BowedHigh,
    WoodwindFlute,
    WoodwindReed,
    BrassBright,
    BrassWarm,
    VoiceMale,
    VoiceFemale,
    VoiceEnsemble,
    KeyboardStruck,
    KeyboardOrgan,
    Percussion,
    AmpCabinet,
}

/// Exact physical voicing constraints actively applied by `reperform::Performance::synthesize_dry_for_source`.
/// Separated cleanly from published reference data: informs plausible voicing & spectral contour
/// while keeping extracted performance cues (pitch, bends, slides, timing, atypical decay) authoritative.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourcePhysicsConstraint {
    pub attack_ms: f64,
    pub release_ms: f64,
    pub breath_texture_amount: f64,
    pub dynamic_brightness_amount: f64,
    pub harmonic_ceiling_hz: Option<f64>,
    pub resonance_contour_summary: &'static str,
    pub texture_mechanism: &'static str,
    pub plausibility_note: &'static str,
}

impl SourceRegenKind {
    /// Single source of truth for both Rust DSP synthesis and UI disclosure.
    pub const fn physics_constraint(self) -> SourcePhysicsConstraint {
        match self {
            Self::Neutral | Self::PluckedElectric => SourcePhysicsConstraint {
                attack_ms: 8.0,
                release_ms: 18.0,
                breath_texture_amount: 0.0,
                dynamic_brightness_amount: 0.0,
                harmonic_ceiling_hz: None,
                resonance_contour_summary: "Neutral cue-calibrated modal string partials (no added body EQ)",
                texture_mechanism: "Pure cue-driven modal string excitation (no added breath/bow noise)",
                plausibility_note: "Follows cue mode weights and decay directly; never clamps pitch or bends.",
            },
            Self::PluckedAcoustic => SourcePhysicsConstraint {
                attack_ms: 5.0,
                release_ms: 18.0,
                breath_texture_amount: 0.004,
                dynamic_brightness_amount: 0.15,
                harmonic_ceiling_hz: None,
                resonance_contour_summary: "Soundboard body +3.2 dB @ 185 Hz, box dip -2.4 dB @ 460 Hz, pick air +3.5 dB @ 3.6 kHz",
                texture_mechanism: "Subtle pick/string transient friction coupled to reference tonal fraction",
                plausibility_note: "Favors acoustic body & pick attack while preserving cue bends and atypical sustain.",
            },
            Self::PluckedBass => SourcePhysicsConstraint {
                attack_ms: 7.0,
                release_ms: 22.0,
                breath_texture_amount: 0.0,
                dynamic_brightness_amount: 0.08,
                harmonic_ceiling_hz: Some(3800.0),
                resonance_contour_summary: "Low fundamental body +4.5 dB @ 115 Hz, scoop -3.5 dB @ 1.4 kHz, lowpass 3.8 kHz",
                texture_mechanism: "Heavy-string modal decay (no added breath noise)",
                plausibility_note: "Suppresses implausible >3.8 kHz fizz while keeping performed pitch/slides exact.",
            },
            Self::BowedLow => SourcePhysicsConstraint {
                attack_ms: 26.0,
                release_ms: 32.0,
                breath_texture_amount: 0.014,
                dynamic_brightness_amount: 0.20,
                harmonic_ceiling_hz: Some(6200.0),
                resonance_contour_summary: "Wood cavity +5.2 dB @ 175 Hz, hollow dip -2.8 dB @ 650 Hz, bridge +2.4 dB @ 2.1 kHz, LP 6.2 kHz",
                texture_mechanism: "Bow-rosin friction band-limited texture coupled to reference tonal fraction",
                plausibility_note: "Bowed onset and cello/viola body contour; follows cue slides, vibrato, and decay.",
            },
            Self::BowedHigh => SourcePhysicsConstraint {
                attack_ms: 22.0,
                release_ms: 28.0,
                breath_texture_amount: 0.016,
                dynamic_brightness_amount: 0.22,
                harmonic_ceiling_hz: None,
                resonance_contour_summary: "Body +2.6 dB @ 320 Hz, bridge hill +4.8 dB @ 2.8 kHz, bow sheen +2.2 dB @ 6 kHz",
                texture_mechanism: "Bow-hair friction coupled to reference tonal fraction",
                plausibility_note: "Upper bowed-string projection without overriding unusual cue dynamics or pitch.",
            },
            Self::WoodwindFlute => SourcePhysicsConstraint {
                attack_ms: 24.0,
                release_ms: 26.0,
                breath_texture_amount: 0.024,
                dynamic_brightness_amount: 0.10,
                harmonic_ceiling_hz: Some(7500.0),
                resonance_contour_summary: "Air-column warmth +2.0 dB @ 480 Hz, embouchure breath +2.8 dB @ 4.8 kHz, LP 7.5 kHz",
                texture_mechanism: "Air-jet breath aspiration coupled to reference tonal fraction",
                plausibility_note: "Smooth air-column partials + breath jet; never forces notes into flute compass.",
            },
            Self::WoodwindReed => SourcePhysicsConstraint {
                attack_ms: 18.0,
                release_ms: 24.0,
                breath_texture_amount: 0.012,
                dynamic_brightness_amount: 0.18,
                harmonic_ceiling_hz: Some(6500.0),
                resonance_contour_summary: "Bore +2.2 dB @ 260 Hz, reed formant +4.6 dB @ 1.65 kHz, lowpass 6.5 kHz",
                texture_mechanism: "Reed air-column turbulence coupled to reference tonal fraction",
                plausibility_note: "Reed formant emphasis with upper rolloff; cue phrasing and pitch remain authoritative.",
            },
            Self::BrassBright => SourcePhysicsConstraint {
                attack_ms: 12.0,
                release_ms: 22.0,
                breath_texture_amount: 0.006,
                dynamic_brightness_amount: 0.35,
                harmonic_ceiling_hz: None,
                resonance_contour_summary: "Tight low -1.8 dB @ 240 Hz, bell brilliance +5.2 dB @ 2.2 kHz & +3.0 dB @ 4.2 kHz",
                texture_mechanism: "Dynamic brass harmonic bloom + subtle lip/air onset",
                plausibility_note: "Louder cues open brighter brass partials while preserving exact cue envelope.",
            },
            Self::BrassWarm => SourcePhysicsConstraint {
                attack_ms: 18.0,
                release_ms: 26.0,
                breath_texture_amount: 0.005,
                dynamic_brightness_amount: 0.20,
                harmonic_ceiling_hz: Some(4200.0),
                resonance_contour_summary: "Horn warmth +4.0 dB @ 280 Hz, mid +2.0 dB @ 1.3 kHz, smooth bell lowpass 4.2 kHz",
                texture_mechanism: "Warm brass dynamic harmonic bloom + mild breath onset",
                plausibility_note: "Rolls off implausible harsh >4.2 kHz harmonics while keeping cues intact.",
            },
            Self::VoiceMale => SourcePhysicsConstraint {
                attack_ms: 20.0,
                release_ms: 30.0,
                breath_texture_amount: 0.016,
                dynamic_brightness_amount: 0.12,
                harmonic_ceiling_hz: Some(8000.0),
                resonance_contour_summary: "Chest +3.2 dB @ 160 Hz; vocal formants 500 / 1350 / 2700 Hz; lowpass 8.0 kHz",
                texture_mechanism: "Glottal breath aspiration coupled to reference tonal fraction",
                plausibility_note: "Male vocal formant contour + breath; never forces pitch or erases vocal quirks.",
            },
            Self::VoiceFemale => SourcePhysicsConstraint {
                attack_ms: 22.0,
                release_ms: 30.0,
                breath_texture_amount: 0.018,
                dynamic_brightness_amount: 0.12,
                harmonic_ceiling_hz: None,
                resonance_contour_summary: "Low trim -5.5 dB @ 160 Hz; vocal formants 680 / 1650 / 3150 Hz + 6 kHz air",
                texture_mechanism: "Glottal breath aspiration coupled to reference tonal fraction",
                plausibility_note: "Upper vocal formant contour; preserves low/out-of-range cues as performed.",
            },
            Self::VoiceEnsemble => SourcePhysicsConstraint {
                attack_ms: 24.0,
                release_ms: 34.0,
                breath_texture_amount: 0.015,
                dynamic_brightness_amount: 0.10,
                harmonic_ceiling_hz: Some(8500.0),
                resonance_contour_summary: "Blended choir formants 580 / 1500 / 3000 Hz, smooth air lowpass 8.5 kHz",
                texture_mechanism: "Soft choral breath texture coupled to reference tonal fraction",
                plausibility_note: "Broad ensemble formant contour while strictly following mono performance cues.",
            },
            Self::KeyboardStruck => SourcePhysicsConstraint {
                attack_ms: 6.0,
                release_ms: 22.0,
                breath_texture_amount: 0.0,
                dynamic_brightness_amount: 0.18,
                harmonic_ceiling_hz: None,
                resonance_contour_summary: "Soundboard +2.6 dB @ 220 Hz, hammer presence +2.8 dB @ 2.5 kHz",
                texture_mechanism: "Hammer-struck string modal partials (no added breath)",
                plausibility_note: "Struck-string voicing that STILL follows cue bends/slides/sustain if performed.",
            },
            Self::KeyboardOrgan => SourcePhysicsConstraint {
                attack_ms: 10.0,
                release_ms: 18.0,
                breath_texture_amount: 0.004,
                dynamic_brightness_amount: 0.05,
                harmonic_ceiling_hz: Some(6000.0),
                resonance_contour_summary: "Pipe/drawbar mid +3.4 dB @ 900 Hz, smooth lowpass 6.0 kHz",
                texture_mechanism: "Subtle pipe-chiff air texture coupled to reference tonal fraction",
                plausibility_note: "Steady organ harmonic contour without overriding cue dynamics or bends.",
            },
            Self::Percussion => SourcePhysicsConstraint {
                attack_ms: 3.0,
                release_ms: 16.0,
                breath_texture_amount: 0.012,
                dynamic_brightness_amount: 0.25,
                harmonic_ceiling_hz: None,
                resonance_contour_summary: "Shell fundamental +3.2 dB @ 190 Hz, stick transient +4.2 dB @ 3.6 kHz",
                texture_mechanism: "Fast stick/mallet transient texture coupled to reference tonal fraction",
                plausibility_note: "Crisp strike voicing while still following cue pitch and atypical sustain envelopes.",
            },
            Self::AmpCabinet => SourcePhysicsConstraint {
                attack_ms: 7.0,
                release_ms: 18.0,
                breath_texture_amount: 0.0,
                dynamic_brightness_amount: 0.16,
                harmonic_ceiling_hz: Some(4700.0),
                resonance_contour_summary: "Cabinet thump +3.2 dB @ 130 Hz, speaker midrange +4.4 dB @ 1.75 kHz, cone rolloff 4.7 kHz",
                texture_mechanism: "Electric string + guitar-speaker cone filtering (no added breath)",
                plausibility_note: "Suppresses implausible >4.7 kHz fizz typical of raw DI while keeping cue expression.",
            },
        }
    }
}

impl SourceRegenKind {
    /// High-level synthesis model type hint (string, bowed, voice, wind, brass, membrane, keyboard, amp, neutral).
    pub const fn model_type_hint(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::PluckedElectric | Self::PluckedAcoustic | Self::PluckedBass => "string",
            Self::BowedLow | Self::BowedHigh => "bowed",
            Self::WoodwindFlute | Self::WoodwindReed => "wind",
            Self::BrassBright | Self::BrassWarm => "brass",
            Self::VoiceMale | Self::VoiceFemale | Self::VoiceEnsemble => "voice",
            Self::KeyboardStruck | Self::KeyboardOrgan => "keyboard",
            Self::Percussion => "membrane",
            Self::AmpCabinet => "amp",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceRegenModel {
    pub model_id: String,
    pub model_label: String,
    pub family_kind: SourceRegenKind,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceEntry {
    pub id: String,
    pub name: String,
    pub category: SourceCategory,
    pub family: String,
    pub symbol: SourceSymbol,
    /// High-level synthesis model type hint ("string", "bowed", "voice", "wind", "brass", "membrane", "keyboard", "amp", "neutral").
    pub synthesis_model_hint: String,
    /// Published reference fundamental range (Hz). Informational only; NEVER filters or clamps cues.
    pub published_range_hz: Option<(f32, f32)>,
    /// Musical note compass corresponding to published fundamental range when pitched (e.g. "C2–A5").
    pub published_fundamental_notes: Option<String>,
    /// Published practical playing / overtone range (Hz). Informational only.
    pub published_practical_range_hz: Option<(f32, f32)>,
    pub spectral_regions: SpectralRegions,
    pub published_notes: String,
    pub source_references: Vec<String>,
    pub uncertainties: Vec<String>,
    pub radiation: SourceRadiation,
    pub regen_model: SourceRegenModel,
}

#[derive(Deserialize)]
struct RawSourceProfilesFile {
    sources: Vec<RawSourceProfileEntry>,
}

#[derive(Default, Deserialize)]
struct RawSpectralRegions {
    #[serde(default)]
    body_warmth_hz: Option<[f32; 2]>,
    #[serde(default)]
    presence_attack_hz: Option<[f32; 2]>,
    #[serde(default)]
    air_high_hz: Option<[f32; 2]>,
    #[serde(default)]
    formants_and_resonances_note: String,
}

#[derive(Deserialize)]
struct RawSourceRegenModel {
    model_id: String,
    model_label: String,
    family_kind: String,
    description: String,
}

#[derive(Deserialize)]
struct RawSourceProfileEntry {
    id: String,
    name: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    family: String,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    radiation: String,
    #[serde(default)]
    synthesis_model_hint: Option<String>,
    #[serde(default)]
    published_range_hz: Option<[f32; 2]>,
    #[serde(default)]
    published_fundamental_notes: Option<String>,
    #[serde(default)]
    published_practical_range_hz: Option<[f32; 2]>,
    #[serde(default)]
    spectral_regions: RawSpectralRegions,
    #[serde(default)]
    published_notes: String,
    #[serde(default)]
    source_references: Vec<String>,
    #[serde(default)]
    uncertainties: Vec<String>,
    #[serde(default)]
    regen_model: Option<RawSourceRegenModel>,
}

fn parse_source_category(s: &str) -> SourceCategory {
    match s {
        "Instrument" => SourceCategory::Instrument,
        "Human" => SourceCategory::Human,
        _ => SourceCategory::Unknown,
    }
}

fn parse_source_symbol(s: &str) -> SourceSymbol {
    match s {
        "guitar" => SourceSymbol::Guitar,
        "amp" => SourceSymbol::Amp,
        "bowed" => SourceSymbol::Bowed,
        "wind" => SourceSymbol::Wind,
        "brass" => SourceSymbol::Brass,
        "percussion" => SourceSymbol::Percussion,
        "keyboard" => SourceSymbol::Keyboard,
        "voice" => SourceSymbol::Voice,
        "ensemble" => SourceSymbol::Ensemble,
        _ => SourceSymbol::Unknown,
    }
}

fn parse_source_radiation(s: &str) -> SourceRadiation {
    match s {
        "forward_wide" => SourceRadiation::ForwardWide,
        "forward_cardioid" => SourceRadiation::ForwardCardioid,
        "dipole_open_back" => SourceRadiation::DipoleOpenBack,
        "directional_bell" => SourceRadiation::DirectionalBell,
        _ => SourceRadiation::Omni,
    }
}

fn parse_regen_kind(s: &str) -> SourceRegenKind {
    match s {
        "plucked_electric" => SourceRegenKind::PluckedElectric,
        "plucked_acoustic" | "string" => SourceRegenKind::PluckedAcoustic,
        "plucked_bass" | "bass" => SourceRegenKind::PluckedBass,
        "bowed_low" | "bowed" => SourceRegenKind::BowedLow,
        "bowed_high" => SourceRegenKind::BowedHigh,
        "woodwind_flute" | "flute" => SourceRegenKind::WoodwindFlute,
        "woodwind_reed" | "wind" | "reed" => SourceRegenKind::WoodwindReed,
        "brass_bright" | "brass" => SourceRegenKind::BrassBright,
        "brass_warm" => SourceRegenKind::BrassWarm,
        "voice_male" => SourceRegenKind::VoiceMale,
        "voice_female" | "voice" => SourceRegenKind::VoiceFemale,
        "voice_ensemble" => SourceRegenKind::VoiceEnsemble,
        "keyboard_struck" | "keyboard" => SourceRegenKind::KeyboardStruck,
        "keyboard_organ" => SourceRegenKind::KeyboardOrgan,
        "percussion" | "membrane" => SourceRegenKind::Percussion,
        "amp_cabinet" | "amp" => SourceRegenKind::AmpCabinet,
        _ => SourceRegenKind::Neutral,
    }
}

static SOURCE_PROFILES_JSON: &str = include_str!("../capture-reference/source-profiles-138.json");
static SOURCE_CATALOG_CACHE: OnceLock<Vec<SourceEntry>> = OnceLock::new();

/// Parse an extensible `capture-reference` source catalog JSON string into `Vec<SourceEntry>`.
/// Adding or correcting entries requires zero engine code changes beyond reloading the catalog.
pub fn parse_source_catalog_json(json: &str) -> Result<Vec<SourceEntry>, String> {
    let parsed: RawSourceProfilesFile =
        serde_json::from_str(json).map_err(|e| format!("Invalid source catalog JSON: {e}"))?;
    Ok(parsed
        .sources
        .into_iter()
        .map(|r| {
            let (regen_model, hint_from_kind) = match r.regen_model {
                Some(rm) => {
                    let fk = parse_regen_kind(&rm.family_kind);
                    (
                        SourceRegenModel {
                            model_id: rm.model_id,
                            model_label: rm.model_label,
                            family_kind: fk,
                            description: rm.description,
                        },
                        fk.model_type_hint().to_string(),
                    )
                }
                None => {
                    let hint_str = r
                        .synthesis_model_hint
                        .as_deref()
                        .unwrap_or("neutral")
                        .to_ascii_lowercase();
                    let fk = parse_regen_kind(&hint_str);
                    (
                        SourceRegenModel {
                            model_id: format!("hint_{hint_str}"),
                            model_label: format!("Guided {} Model", fk.model_type_hint()),
                            family_kind: fk,
                            description: fk.physics_constraint().plausibility_note.to_string(),
                        },
                        fk.model_type_hint().to_string(),
                    )
                }
            };
            let synthesis_model_hint = r
                .synthesis_model_hint
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(hint_from_kind);
            SourceEntry {
                id: r.id,
                name: r.name,
                category: parse_source_category(&r.category),
                family: if r.family.is_empty() {
                    "Uncategorized".to_string()
                } else {
                    r.family
                },
                symbol: parse_source_symbol(&r.symbol),
                synthesis_model_hint,
                published_range_hz: r.published_range_hz.map(|[a, b]| (a, b)),
                published_fundamental_notes: r.published_fundamental_notes,
                published_practical_range_hz: r.published_practical_range_hz.map(|[a, b]| (a, b)),
                spectral_regions: SpectralRegions {
                    body_warmth_hz: r.spectral_regions.body_warmth_hz.map(|[a, b]| (a, b)),
                    presence_attack_hz: r.spectral_regions.presence_attack_hz.map(|[a, b]| (a, b)),
                    air_high_hz: r.spectral_regions.air_high_hz.map(|[a, b]| (a, b)),
                    formants_and_resonances_note: if r
                        .spectral_regions
                        .formants_and_resonances_note
                        .is_empty()
                    {
                        "No narrow resonance peaks assumed; missing data left explicit.".into()
                    } else {
                        r.spectral_regions.formants_and_resonances_note
                    },
                },
                published_notes: r.published_notes,
                source_references: r.source_references,
                uncertainties: r.uncertainties,
                radiation: parse_source_radiation(&r.radiation),
                regen_model,
            }
        })
        .collect())
}

/// Reloadable offline catalog loader: checks `SPECTRAL_SOURCE_CATALOG_PATH` (or local
/// `capture-reference/source-profiles-138.json`) so adding/editing entries requires no recompile,
/// falling back cleanly to the compile-time embedded catalog with zero network calls.
pub fn load_source_catalog_from_disk_or_embedded() -> Vec<SourceEntry> {
    if let Ok(path) = std::env::var("SPECTRAL_SOURCE_CATALOG_PATH") {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(entries) = parse_source_catalog_json(&text) {
                if !entries.is_empty() {
                    return entries;
                }
            }
        }
    }
    for rel in [
        "spectracraft-mono/capture-reference/source-profiles-138.json",
        "capture-reference/source-profiles-138.json",
    ] {
        if let Ok(text) = std::fs::read_to_string(rel) {
            if let Ok(entries) = parse_source_catalog_json(&text) {
                if !entries.is_empty() {
                    return entries;
                }
            }
        }
    }
    source_catalog()
}

/// Full offline 138-source frequency & resonance catalog from `capture-reference/source-profiles-138.json`.
/// Zero runtime network access; published ranges are reference only and never clamp cues.
pub fn source_catalog() -> Vec<SourceEntry> {
    SOURCE_CATALOG_CACHE
        .get_or_init(|| parse_source_catalog_json(SOURCE_PROFILES_JSON).unwrap_or_default())
        .clone()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transducer {
    Dynamic,
    SmallCondenser,
    LargeCondenser,
    Ribbon,
    BoundaryOmni,
}

impl Transducer {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dynamic => "Dynamic",
            Self::SmallCondenser => "Small-Diaphragm Condenser",
            Self::LargeCondenser => "Large-Diaphragm Condenser",
            Self::Ribbon => "Ribbon",
            Self::BoundaryOmni => "Measurement / Boundary Omni",
        }
    }
}

/// Underlying parametric renderer voicing shared by catalog families.
/// Explicitly NOT a measured unit impulse response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicRendererProfile {
    FlatIdeal,
    DynamicSm7Class,
    DynamicPresence57Class,
    SmallCondenserNeutral,
    LargeCondenserWarm,
    RibbonSmooth,
    BoundaryOmniFlat,
}

impl MicRendererProfile {
    pub fn id(self) -> &'static str {
        match self {
            Self::FlatIdeal => "ideal_flat_analytical",
            Self::DynamicSm7Class => "dynamic_sm7_class_approx",
            Self::DynamicPresence57Class => "dynamic_presence_approx",
            Self::SmallCondenserNeutral => "sd_condenser_neutral_approx",
            Self::LargeCondenserWarm => "ld_condenser_warm_approx",
            Self::RibbonSmooth => "ribbon_smooth_approx",
            Self::BoundaryOmniFlat => "omni_measurement_flat",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::FlatIdeal => "Flat on-axis response with ideal analytical polar equation.",
            Self::DynamicSm7Class => {
                "Parametric SM7-class approximation (55 Hz HPF, +1.2 dB @ 200 Hz, +1.9 dB @ 3.4 kHz, 10.5 kHz LPF)."
            }
            Self::DynamicPresence57Class => {
                "Parametric instrument dynamic approximation (70 Hz HPF, +2.8 dB @ 4.8 kHz presence, 11 kHz LPF)."
            }
            Self::SmallCondenserNeutral => {
                "Parametric neutral SDC approximation (30 Hz HPF, +0.7 dB @ 8 kHz air shelf)."
            }
            Self::LargeCondenserWarm => {
                "Parametric warm LDC approximation (35 Hz HPF, +1.1 dB @ 160 Hz, +1.5 dB @ 5.5 kHz)."
            }
            Self::RibbonSmooth => {
                "Parametric ribbon approximation (40 Hz HPF, +1.6 dB @ 180 Hz proximity warmth, 7.5 kHz gentle LPF)."
            }
            Self::BoundaryOmniFlat => {
                "Flat analytical omni pressure model (20 Hz HPF, flat to Nyquist)."
            }
        }
    }
}

/// Published off-axis polar attenuation summary recovered from Mic Archive spec-sheet plots.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PublishedOffaxisTable {
    pub freqs_hz: Vec<f32>,
    pub atten_90deg_db: Vec<f32>,
    pub atten_180deg_db: Vec<f32>,
}

/// Microphone catalog entry separating **Published Manufacturer / Mic Archive Reference Data**
/// from **Actual Renderer Model Parameters** (`pattern`, `renderer_profile`).
#[derive(Clone, Debug, PartialEq)]
pub struct MicEntry {
    pub id: String,
    pub brand: String,
    pub model: String,
    pub source_url: String,
    pub transducer: Transducer,
    /// Primary analytical polar pattern used by the renderer unless overridden by the user.
    pub pattern: Pattern,
    pub published_type: Option<String>,
    pub published_patterns: Option<String>,
    pub published_curves: Vec<String>,
    pub published_primary_curve: Option<String>,
    /// Digitized published on-axis frequency response points `[[hz, db], ...]` (reference display ONLY).
    pub published_fr_points: Vec<[f32; 2]>,
    pub published_offaxis: Option<PublishedOffaxisTable>,
    pub published_range_text: Option<String>,
    pub published_range_hz: Option<(f32, f32)>,
    pub published_sensitivity_dbv: Option<f32>,
    pub published_self_noise_dba: Option<f32>,
    pub published_max_spl_db: Option<f32>,
    pub published_impedance_ohm: Option<f32>,
    pub published_released_year: Option<u16>,
    pub published_measurement_dist_cm: Option<f32>,
    pub published_power: Option<String>,
    pub source_document: String,
    pub uncertainties: Vec<String>,
    pub renderer_profile: MicRendererProfile,
    pub curve_provenance: String,
    pub published_notes: String,
}

impl MicEntry {
    pub fn display_name(&self) -> String {
        format!("{} {} ({})", self.brand, self.model, self.pattern.label())
    }
    pub fn shared_model_explanation(&self) -> String {
        format!(
            "Modeled renderer profile '{}': {}. Published curves/specs are manufacturer reference only (not a measured hardware IR). Any entries sharing this renderer profile and '{}' polar pattern produce identical DSP unless room/position/aim differ.",
            self.renderer_profile.id(),
            self.renderer_profile.description(),
            self.pattern.label()
        )
    }
}

#[derive(Deserialize)]
struct RawMicArchiveFile {
    mics: Vec<RawMicArchiveEntry>,
}

#[derive(Deserialize)]
struct RawMicArchiveEntry {
    id: String,
    brand: String,
    model: String,
    source_url: String,
    summary: String,
    transducer: String,
    pattern: String,
    renderer_profile: String,
    published_type: Option<String>,
    published_patterns: Option<String>,
    #[serde(default)]
    published_curves: Vec<String>,
    published_primary_curve: Option<String>,
    #[serde(default)]
    published_fr_points: Vec<[f32; 2]>,
    published_offaxis: Option<PublishedOffaxisTable>,
    published_range_text: Option<String>,
    published_range_hz: Option<[f32; 2]>,
    published_sensitivity_dbv: Option<f32>,
    published_self_noise_dba: Option<f32>,
    published_max_spl_db: Option<f32>,
    published_impedance_ohm: Option<f32>,
    published_released_year: Option<u16>,
    published_measurement_dist_cm: Option<f32>,
    published_power: Option<String>,
    source_document: String,
    #[serde(default)]
    uncertainties: Vec<String>,
}

fn parse_transducer_key(s: &str) -> Transducer {
    match s {
        "dynamic" => Transducer::Dynamic,
        "ribbon" => Transducer::Ribbon,
        "large_condenser" => Transducer::LargeCondenser,
        "small_condenser" => Transducer::SmallCondenser,
        "boundary_omni" => Transducer::BoundaryOmni,
        _ => Transducer::SmallCondenser,
    }
}

fn parse_pattern_key(s: &str) -> Pattern {
    match s {
        "omni" => Pattern::Omni,
        "wide_cardioid" => Pattern::WideCardioid,
        "cardioid" => Pattern::Cardioid,
        "supercardioid" => Pattern::Supercardioid,
        "hypercardioid" => Pattern::Hypercardioid,
        "figure8" => Pattern::Figure8,
        _ => Pattern::Cardioid,
    }
}

fn parse_renderer_profile_key(s: &str) -> MicRendererProfile {
    match s {
        "flat_ideal" => MicRendererProfile::FlatIdeal,
        "dynamic_sm7_class" => MicRendererProfile::DynamicSm7Class,
        "dynamic_presence_57_class" => MicRendererProfile::DynamicPresence57Class,
        "ribbon_smooth" => MicRendererProfile::RibbonSmooth,
        "large_condenser_warm" => MicRendererProfile::LargeCondenserWarm,
        "small_condenser_neutral" => MicRendererProfile::SmallCondenserNeutral,
        "boundary_omni_flat" => MicRendererProfile::BoundaryOmniFlat,
        _ => MicRendererProfile::SmallCondenserNeutral,
    }
}

static MIC_ARCHIVE_JSON: &str = include_str!("../capture-reference/mic-archive-972.json");
static MIC_CATALOG_CACHE: OnceLock<Vec<MicEntry>> = OnceLock::new();

/// Full offline 972-microphone Mic Archive catalog from `capture-reference/mic-archive-972.json`.
/// Zero runtime network access; index 0 is preserved as the Shure SM7B repeatable baseline.
pub fn mic_catalog() -> Vec<MicEntry> {
    MIC_CATALOG_CACHE
        .get_or_init(|| {
            const PROV: &str = "Published specs/curves from Mic Archive (reference only — not a measured hardware IR); renderer applies modeled analytical polar + parametric class";
            let parsed: RawMicArchiveFile = match serde_json::from_str(MIC_ARCHIVE_JSON) {
                Ok(v) => v,
                Err(_) => return Vec::new(),
            };
            parsed
                .mics
                .into_iter()
                .map(|r| MicEntry {
                    id: r.id,
                    brand: r.brand,
                    model: r.model,
                    source_url: r.source_url,
                    transducer: parse_transducer_key(&r.transducer),
                    pattern: parse_pattern_key(&r.pattern),
                    published_type: r.published_type,
                    published_patterns: r.published_patterns,
                    published_curves: r.published_curves,
                    published_primary_curve: r.published_primary_curve,
                    published_fr_points: r.published_fr_points,
                    published_offaxis: r.published_offaxis,
                    published_range_text: r.published_range_text,
                    published_range_hz: r.published_range_hz.map(|a| (a[0], a[1])),
                    published_sensitivity_dbv: r.published_sensitivity_dbv,
                    published_self_noise_dba: r.published_self_noise_dba,
                    published_max_spl_db: r.published_max_spl_db,
                    published_impedance_ohm: r.published_impedance_ohm,
                    published_released_year: r.published_released_year,
                    published_measurement_dist_cm: r.published_measurement_dist_cm,
                    published_power: r.published_power,
                    source_document: r.source_document,
                    uncertainties: r.uncertainties,
                    renderer_profile: parse_renderer_profile_key(&r.renderer_profile),
                    curve_provenance: PROV.to_owned(),
                    published_notes: r.summary,
                })
                .collect()
        })
        .clone()
}

/// Biot-JCA porous characterization parameters (porosity, airflow resistivity, tortuosity,
/// viscous/thermal characteristic lengths). Unmeasured fields remain explicitly `None`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BiotParameters {
    pub porosity: Option<f32>,
    pub airflow_resistivity_pa_s_m2: Option<f32>,
    pub tortuosity: Option<f32>,
    pub vcl_um: Option<f32>,
    pub tcl_um: Option<f32>,
    pub standard_ref: String,
    pub status: String,
}

impl Default for BiotParameters {
    fn default() -> Self {
        Self {
            porosity: None,
            airflow_resistivity_pa_s_m2: None,
            tortuosity: None,
            vcl_um: None,
            tcl_um: None,
            standard_ref: "ASTM E1050 / ISO 10534-2".to_owned(),
            status: "Not published — values not invented.".to_owned(),
        }
    }
}

/// Acoustic surface profile with raw 6-band absorption coefficients (125–4000 Hz),
/// effective passive absorption `alpha_eff = raw.clamp(0, 1)`, passive pressure
/// reflectance `sqrt(1 - alpha_eff)`, standards/assembly metadata, and Biot parameter slots.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceEntry {
    pub id: String,
    pub name: String,
    pub category: String,
    pub material_type: String,
    pub thickness_mm: Option<f32>,
    pub assembly: String,
    pub incidence: String,
    pub standard: String,
    pub response_kind: String,
    /// Raw octave-band absorption coefficients at [125, 250, 500, 1000, 2000, 4000] Hz.
    /// Published ASTM C423 random-incidence values may exceed 1.0 (diffraction edge effects);
    /// `pressure_reflectance` clamps each band to `[0.0, 1.0]` before computing `sqrt(1 - a)`.
    pub absorption_6band: [f32; 6],
    pub biot: BiotParameters,
    pub provenance: String,
    pub notes: String,
    pub uncertainties: Vec<String>,
}

impl SurfaceEntry {
    /// Effective absorption clamped to passive `[0.0, 1.0]` per octave band.
    /// Raw published ASTM C423 values remain intact in `absorption_6band`.
    pub fn effective_absorption(&self) -> [f64; 6] {
        self.absorption_6band.map(|a| {
            let v = f64::from(a);
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                1.0
            }
        })
    }
    /// Passive pressure reflectance per octave band `[125, 250, 500, 1000, 2000, 4000]` Hz:
    /// `sqrt(1.0 - effective_absorption)`.
    pub fn pressure_reflectance(&self) -> [f64; 6] {
        self.effective_absorption().map(|a| (1.0 - a).sqrt())
    }
    /// Oblique-incidence pressure reflectance per octave band `[125, 250, 500, 1000, 2000, 4000]` Hz.
    /// Uses Johnson-Champoux-Allard (JCA) porous impedance physics strictly when all required
    /// Biot parameters and thickness are published; falls back cleanly to published ASTM C423
    /// `pressure_reflectance()` otherwise (never invents unpublished parameters).
    pub fn oblique_pressure_reflectance(&self, incidence_rad: f64) -> [f64; 6] {
        let fallback = self.pressure_reflectance();
        std::array::from_fn(|b| {
            crate::acoustics_physics::jca_reflection_magnitude(
                BANDS_HZ[b],
                incidence_rad,
                self.thickness_mm,
                &self.biot,
            )
            .unwrap_or(fallback[b])
        })
    }
    /// Mean pressure reflectance across the 6 octave bands (for compact list display).
    pub fn mean_reflectance(&self) -> f64 {
        let r = self.pressure_reflectance();
        r.iter().sum::<f64>() / 6.0
    }
}

#[derive(Deserialize)]
struct RawMaterialsFile {
    materials: Vec<RawMaterialEntry>,
}

#[derive(Deserialize)]
struct RawMaterialEntry {
    id: String,
    name: String,
    category: String,
    material_type: String,
    thickness_mm: Option<f32>,
    assembly: String,
    incidence: String,
    standard: String,
    response_kind: String,
    raw_absorption_6band: [f32; 6],
    biot: BiotParameters,
    provenance: String,
    notes: String,
    #[serde(default)]
    uncertainties: Vec<String>,
}

static ACOUSTIC_MATERIALS_JSON: &str =
    include_str!("../capture-reference/acoustic-materials-32.json");
static SURFACE_CATALOG_CACHE: OnceLock<Vec<SurfaceEntry>> = OnceLock::new();

/// 32 searchable room/field surface profiles loaded offline from `capture-reference/acoustic-materials-32.json`.
/// Zero runtime network access; preserves exact index order (`0..31`) for all room presets.
pub fn surface_catalog() -> Vec<SurfaceEntry> {
    SURFACE_CATALOG_CACHE
        .get_or_init(|| {
            let parsed: RawMaterialsFile = match serde_json::from_str(ACOUSTIC_MATERIALS_JSON) {
                Ok(v) => v,
                Err(_) => return Vec::new(),
            };
            parsed
                .materials
                .into_iter()
                .map(|m| SurfaceEntry {
                    id: m.id,
                    name: m.name,
                    category: m.category,
                    material_type: m.material_type,
                    thickness_mm: m.thickness_mm,
                    assembly: m.assembly,
                    incidence: m.incidence,
                    standard: m.standard,
                    response_kind: m.response_kind,
                    absorption_6band: m.raw_absorption_6band,
                    biot: m.biot,
                    provenance: m.provenance,
                    notes: m.notes,
                    uncertainties: m.uncertainties,
                })
                .collect()
        })
        .clone()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomPreset {
    Small,
    Medium,
    Large,
    Open,
    Direct,
}

impl RoomPreset {
    pub const ALL: [Self; 5] = [
        Self::Small,
        Self::Medium,
        Self::Large,
        Self::Open,
        Self::Direct,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small (10 × 10 × 10 ft)",
            Self::Medium => "Medium (20 × 20 × 10 ft)",
            Self::Large => "Large (50 × 50 × 10 ft)",
            Self::Open => "Open (Field · Ground Only)",
            Self::Direct => "Direct (No Added Room)",
        }
    }
    pub fn dimensions_m(self) -> [f32; 3] {
        match self {
            Self::Small => [
                10. * FEET_TO_METRES,
                10. * FEET_TO_METRES,
                10. * FEET_TO_METRES,
            ],
            Self::Medium => [
                20. * FEET_TO_METRES,
                20. * FEET_TO_METRES,
                10. * FEET_TO_METRES,
            ],
            Self::Large => [
                50. * FEET_TO_METRES,
                50. * FEET_TO_METRES,
                10. * FEET_TO_METRES,
            ],
            Self::Open => [
                50. * FEET_TO_METRES,
                50. * FEET_TO_METRES,
                50. * FEET_TO_METRES,
            ],
            Self::Direct => [
                10. * FEET_TO_METRES,
                10. * FEET_TO_METRES,
                10. * FEET_TO_METRES,
            ],
        }
    }
    /// Default 6-face surface indices `[-X, +X, -Y, +Y, Floor/Ground, Ceiling]`.
    pub fn default_surface_faces(self) -> [usize; 6] {
        match self {
            // SM7B reference baseline: 2-inch wedge foam (index 1) across all 6 faces
            Self::Small => [1, 1, 1, 1, 1, 1],
            // Medium studio: pine paneling walls (14), oak floor (15), acoustic tile ceiling (6)
            Self::Medium => [14, 14, 14, 14, 15, 6],
            // Large live room: lime plaster walls & ceiling (18), oak floor (15)
            Self::Large => [18, 18, 18, 18, 15, 18],
            // Open field: open boundaries (0) + grass over soil ground (29)
            Self::Open => [0, 0, 0, 0, 29, 0],
            // Direct: no reflections
            Self::Direct => [0; 6],
        }
    }
}

pub const FACE_LABELS: [&str; 6] = [
    "-X Left wall",
    "+X Right wall",
    "-Y Front wall",
    "+Y Back wall",
    "Floor / Ground",
    "Ceiling",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridDirection {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl GridDirection {
    pub const ALL: [Self; 6] = [
        Self::PosX,
        Self::NegX,
        Self::PosY,
        Self::NegY,
        Self::PosZ,
        Self::NegZ,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::PosX => "+X (Right)",
            Self::NegX => "-X (Left)",
            Self::PosY => "+Y (Back)",
            Self::NegY => "-Y (Front)",
            Self::PosZ => "+Z (Up)",
            Self::NegZ => "-Z (Down)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MicAim {
    /// Pitch / tilt in degrees (-90 = straight down -Z, 0 = level, +90 = straight up +Z).
    pub tilt_x_deg: f32,
    /// Roll around microphone longitudinal axis in degrees (-180..=180).
    pub roll_y_deg: f32,
    /// Heading / azimuth in degrees in Top plan (0° = -Y toward source when mic is in front at +Y,
    /// +90° = +X right, -90° = -X left, ±180° = +Y away from source).
    pub heading_z_deg: f32,
    /// When true, the mic persistently rotates to point at the source whenever moved.
    pub follow_source: bool,
}

impl Default for MicAim {
    fn default() -> Self {
        Self {
            tilt_x_deg: 0.0,
            roll_y_deg: 0.0,
            heading_z_deg: 0.0,
            follow_source: true,
        }
    }
}

impl MicAim {
    /// Unit direction vector in world metres `[x, y, z]` where the microphone capsule points.
    pub fn direction(&self) -> [f32; 3] {
        let tilt = self.tilt_x_deg.clamp(-90., 90.).to_radians();
        let heading = self.heading_z_deg.to_radians();
        let horiz = tilt.cos();
        let x = horiz * heading.sin();
        let y = -horiz * heading.cos();
        let z = tilt.sin();
        let len = (x * x + y * y + z * z).sqrt().max(1e-6);
        [x / len, y / len, z / len]
    }
    /// Set aim angles to point from `mic_offset` (relative to source at origin) toward source `[0,0,0]`.
    /// Never modifies the microphone position.
    pub fn point_at_source(&mut self, mic_offset: [f32; 3]) {
        let to_src = [-mic_offset[0], -mic_offset[1], -mic_offset[2]];
        let horiz = (to_src[0] * to_src[0] + to_src[1] * to_src[1]).sqrt();
        let len = (horiz * horiz + to_src[2] * to_src[2]).sqrt();
        if len < 1e-6 {
            return;
        }
        self.tilt_x_deg = to_src[2].atan2(horiz).to_degrees().clamp(-90., 90.);
        if horiz > 1e-6 {
            self.heading_z_deg = to_src[0].atan2(-to_src[1]).to_degrees();
        }
    }
    /// Apply one of the six axis-aligned grid directions (disables follow_source, keeps position).
    pub fn set_grid_direction(&mut self, dir: GridDirection) {
        self.follow_source = false;
        match dir {
            GridDirection::PosX => {
                self.tilt_x_deg = 0.;
                self.heading_z_deg = 90.;
            }
            GridDirection::NegX => {
                self.tilt_x_deg = 0.;
                self.heading_z_deg = -90.;
            }
            GridDirection::PosY => {
                self.tilt_x_deg = 0.;
                self.heading_z_deg = 180.;
            }
            GridDirection::NegY => {
                self.tilt_x_deg = 0.;
                self.heading_z_deg = 0.;
            }
            GridDirection::PosZ => {
                self.tilt_x_deg = 90.;
            }
            GridDirection::NegZ => {
                self.tilt_x_deg = -90.;
            }
        }
    }
    pub fn snap_angles(&mut self, step_deg: f32) {
        if step_deg > 0. {
            self.tilt_x_deg = ((self.tilt_x_deg / step_deg).round() * step_deg).clamp(-90., 90.);
            self.roll_y_deg = ((self.roll_y_deg / step_deg).round() * step_deg).clamp(-180., 180.);
            self.heading_z_deg =
                ((self.heading_z_deg / step_deg).round() * step_deg).clamp(-180., 180.);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MicSlot {
    pub label: String,
    pub catalog_index: usize,
    /// Independent pickup pattern override; when `None`, uses catalog entry's default pattern.
    pub pattern_override: Option<Pattern>,
    /// Offset from room source origin in metres: `[left/right X, forward/back Y, up/down Z]`.
    pub offset_m: [f32; 3],
    pub aim: MicAim,
    pub enabled: bool,
}

impl MicSlot {
    /// Default SM7B repeatable baseline: 2 ft (0.6096 m) directly in front (+Y) of source.
    pub fn sm7_baseline() -> Self {
        Self {
            label: "Mic 1 (SM7B Baseline)".into(),
            catalog_index: 0,
            pattern_override: None,
            offset_m: [0.0, 2.0 * FEET_TO_METRES, 0.0],
            aim: MicAim::default(),
            enabled: true,
        }
    }
    /// Effective pickup pattern actually used by the renderer (independent override or catalog default).
    pub fn effective_pattern(&self, mics: &[MicEntry]) -> Pattern {
        self.pattern_override.unwrap_or_else(|| {
            mics.get(self.catalog_index)
                .map_or(Pattern::Cardioid, |m| m.pattern)
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudioRig {
    pub source_index: usize,
    /// Optional user offset from the baseline centered-at-5-ft source origin in metres.
    /// Zero by default so the source stays centered and exactly 5 ft high as the room changes.
    pub source_offset_m: [f32; 3],
    /// Optional source lock (default false): lock source X/Y to footprint center.
    pub source_lock_center_xy: bool,
    /// Optional source lock (default false): lock source height Z to 5 ft (1.524 m).
    pub source_lock_height_5ft: bool,
    pub room_preset: RoomPreset,
    /// Physical room dimensions `[width X, depth Y, height Z]` in metres.
    pub dimensions_m: [f32; 3],
    /// Chain-link flags for `[Width, Depth, Height]`. Changing a linked axis scales other linked axes.
    pub dim_linked: [bool; 3],
    /// 6-face surface catalog indices `[-X, +X, -Y, +Y, Floor/Ground, Ceiling]`.
    pub surface_faces: [usize; 6],
    /// Room air temperature in degrees Celsius (default 20.0 °C -> 343.42 m/s).
    pub temperature_c: f32,
    pub mics: Vec<MicSlot>,
    pub selected_mic: usize,
    pub unit: Unit,
    /// Visual-only toggle for large polar pattern overlays on plans; NEVER changes audio.
    pub show_patterns: bool,
}

impl Default for StudioRig {
    fn default() -> Self {
        Self {
            source_index: 0,
            source_offset_m: [0.0, 0.0, 0.0],
            source_lock_center_xy: false,
            source_lock_height_5ft: false,
            room_preset: RoomPreset::Small,
            dimensions_m: RoomPreset::Small.dimensions_m(),
            dim_linked: [true, true, false],
            surface_faces: RoomPreset::Small.default_surface_faces(),
            temperature_c: 20.0,
            mics: vec![MicSlot::sm7_baseline()],
            selected_mic: 0,
            unit: Unit::Feet,
            show_patterns: false,
        }
    }
}

impl StudioRig {
    /// Repeatable SM7B + 2-inch wedge foam 10×10×10 ft baseline.
    pub fn sm7b_foam_reference() -> Self {
        Self::default()
    }
    /// Defined baseline source position: horizontally centered, 5 ft (1.524 m) above floor
    /// plus any explicit user adjustment `source_offset_m` (clamped safely inside room bounds).
    pub fn source_pos_m(&self) -> [f32; 3] {
        let [w, d, h] = self.dimensions_m;
        let base_z = (5.0 * FEET_TO_METRES).min((h - 0.25).max(0.25));
        let ox = if self.source_lock_center_xy {
            0.0
        } else {
            self.source_offset_m[0]
        };
        let oy = if self.source_lock_center_xy {
            0.0
        } else {
            self.source_offset_m[1]
        };
        let oz = if self.source_lock_height_5ft {
            0.0
        } else {
            self.source_offset_m[2]
        };
        let x = (w * 0.5 + if ox.is_finite() { ox } else { 0.0 }).clamp(0.15, (w - 0.15).max(0.15));
        let y = (d * 0.5 + if oy.is_finite() { oy } else { 0.0 }).clamp(0.15, (d - 0.15).max(0.15));
        let z = (base_z + if oz.is_finite() { oz } else { 0.0 }).clamp(0.15, (h - 0.15).max(0.15));
        [x, y, z]
    }
    /// Move the source to an absolute room position `[x, y, z]` in metres WITHOUT moving any
    /// microphones in world coordinates; re-aims mics that have `follow_source` enabled.
    pub fn set_source_pos_m(&mut self, target_m: [f32; 3]) {
        let old_src = self.source_pos_m();
        let [w, d, h] = self.dimensions_m;
        let base_z = (5.0 * FEET_TO_METRES).min((h - 0.25).max(0.25));
        let safe = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        let next_x = if self.source_lock_center_xy {
            w * 0.5
        } else {
            safe(target_m[0], old_src[0]).clamp(0.15, (w - 0.15).max(0.15))
        };
        let next_y = if self.source_lock_center_xy {
            d * 0.5
        } else {
            safe(target_m[1], old_src[1]).clamp(0.15, (d - 0.15).max(0.15))
        };
        let next_z = if self.source_lock_height_5ft {
            base_z
        } else {
            safe(target_m[2], old_src[2]).clamp(0.15, (h - 0.15).max(0.15))
        };
        self.source_offset_m = [next_x - w * 0.5, next_y - d * 0.5, next_z - base_z];
        let new_src = self.source_pos_m();
        let delta = [
            new_src[0] - old_src[0],
            new_src[1] - old_src[1],
            new_src[2] - old_src[2],
        ];
        // Keep every microphone stationary in room world coordinates
        for mic in &mut self.mics {
            mic.offset_m[0] -= delta[0];
            mic.offset_m[1] -= delta[1];
            mic.offset_m[2] -= delta[2];
        }
        self.clamp_all();
    }
    /// Reset source to centered footprint + 5 ft height without changing mic model, world position, or materials.
    pub fn reset_source_center_5ft(&mut self) {
        let [w, d, h] = self.dimensions_m;
        let base_z = (5.0 * FEET_TO_METRES).min((h - 0.25).max(0.25));
        let lock_xy = self.source_lock_center_xy;
        let lock_z = self.source_lock_height_5ft;
        self.source_lock_center_xy = false;
        self.source_lock_height_5ft = false;
        self.set_source_pos_m([w * 0.5, d * 0.5, base_z]);
        self.source_lock_center_xy = lock_xy;
        self.source_lock_height_5ft = lock_z;
    }
    /// World coordinates `[x, y, z]` in metres of microphone slot `idx`.
    pub fn mic_world_pos_m(&self, idx: usize) -> [f32; 3] {
        let src = self.source_pos_m();
        let off = self
            .mics
            .get(idx)
            .map_or([0.0, 2.0 * FEET_TO_METRES, 0.0], |m| m.offset_m);
        [src[0] + off[0], src[1] + off[1], src[2] + off[2]]
    }
    pub fn link_all(&mut self) {
        self.dim_linked = [true, true, true];
    }
    pub fn unlink_all(&mut self) {
        self.dim_linked = [false, false, false];
    }
    pub fn apply_preset(&mut self, preset: RoomPreset) {
        self.room_preset = preset;
        self.dimensions_m = preset.dimensions_m();
        self.surface_faces = preset.default_surface_faces();
        self.clamp_all();
    }
    pub fn set_all_walls(&mut self, surface_idx: usize) {
        self.surface_faces[0] = surface_idx;
        self.surface_faces[1] = surface_idx;
        self.surface_faces[2] = surface_idx;
        self.surface_faces[3] = surface_idx;
    }
    /// Edit one physical dimension in metres, propagating proportionally to any other linked axes.
    pub fn set_dimension_m(&mut self, axis: usize, new_val_m: f32) {
        if axis >= 3 || !new_val_m.is_finite() {
            return;
        }
        let clamped = new_val_m.clamp(1.5, 40.0);
        let old = self.dimensions_m[axis].max(0.1);
        let ratio = clamped / old;
        self.dimensions_m[axis] = clamped;
        if self.dim_linked[axis] {
            for other in 0..3 {
                if other != axis && self.dim_linked[other] {
                    self.dimensions_m[other] = (self.dimensions_m[other] * ratio).clamp(1.5, 40.0);
                }
            }
        }
        self.clamp_all();
    }
    /// Maximum supported simultaneous microphone slots per rig.
    pub const MAX_MIC_SLOTS: usize = 4;

    /// Validate a `StudioRig` at session, engine-state, CLI, or IPC boundaries.
    /// Rejects empty or oversized microphone lists, invalid indices, non-finite parameters,
    /// and out-of-bounds geometry without mutating or silently substituting defaults.
    pub fn validate(
        &self,
        sources_len: usize,
        mics_len: usize,
        surfaces_len: usize,
    ) -> crate::Result<()> {
        if self.mics.is_empty() {
            return Err("StudioRig must contain at least one microphone slot".into());
        }
        if self.mics.len() > Self::MAX_MIC_SLOTS {
            return Err(format!(
                "StudioRig supports at most {} microphone slots (got {})",
                Self::MAX_MIC_SLOTS,
                self.mics.len()
            )
            .into());
        }
        if self.selected_mic >= self.mics.len() {
            return Err(format!(
                "selected_mic index {} out of bounds for {} microphone slot(s)",
                self.selected_mic,
                self.mics.len()
            )
            .into());
        }
        if sources_len > 0 && self.source_index >= sources_len {
            return Err(format!(
                "source_index {} out of bounds (catalog size {})",
                self.source_index, sources_len
            )
            .into());
        }
        if !self.temperature_c.is_finite() || !(-20.0..=50.0).contains(&self.temperature_c) {
            return Err("temperature_c must be finite and within [-20, 50] C".into());
        }
        for (axis, &d) in self.dimensions_m.iter().enumerate() {
            if !d.is_finite() || !(1.5..=40.0).contains(&d) {
                return Err(format!(
                    "Room dimension axis {axis} ({d}) must be finite and within [1.5, 40.0] m"
                )
                .into());
            }
        }
        for (axis, &off) in self.source_offset_m.iter().enumerate() {
            if !off.is_finite() {
                return Err(format!("source_offset_m[{axis}] must be finite").into());
            }
        }
        if surfaces_len > 0 {
            for (face, &idx) in self.surface_faces.iter().enumerate() {
                if idx >= surfaces_len {
                    return Err(format!(
                        "surface_faces[{face}] index {idx} out of bounds (catalog size {surfaces_len})"
                    )
                    .into());
                }
            }
        }
        for (i, slot) in self.mics.iter().enumerate() {
            if mics_len > 0 && slot.catalog_index >= mics_len {
                return Err(format!(
                    "mics[{i}].catalog_index {} out of bounds (catalog size {mics_len})",
                    slot.catalog_index
                )
                .into());
            }
            if !slot.offset_m.iter().all(|v| v.is_finite()) {
                return Err(format!("mics[{i}].offset_m contains non-finite coordinates").into());
            }
            if !slot.aim.heading_z_deg.is_finite() || !slot.aim.tilt_x_deg.is_finite() {
                return Err(format!("mics[{i}].aim contains non-finite angles").into());
            }
        }
        Ok(())
    }

    pub fn try_active_mic(&self) -> Option<&MicSlot> {
        if self.mics.is_empty() {
            return None;
        }
        let idx = self.selected_mic.min(self.mics.len() - 1);
        self.mics.get(idx)
    }

    pub fn active_mic(&self) -> &MicSlot {
        if let Some(slot) = self.try_active_mic() {
            return slot;
        }
        static FALLBACK_MIC: std::sync::OnceLock<MicSlot> = std::sync::OnceLock::new();
        FALLBACK_MIC.get_or_init(MicSlot::sm7_baseline)
    }
    pub fn active_mic_mut(&mut self) -> &mut MicSlot {
        if self.mics.is_empty() {
            self.mics.push(MicSlot::sm7_baseline());
        }
        let idx = self.selected_mic.min(self.mics.len() - 1);
        self.selected_mic = idx;
        &mut self.mics[idx]
    }
    pub fn add_mic_slot(&mut self) {
        if self.mics.len() >= Self::MAX_MIC_SLOTS {
            return;
        }
        let n = self.mics.len() + 1;
        let offset_x = if n % 2 == 0 { 0.45 } else { -0.45 };
        let mut slot = MicSlot {
            label: format!("Mic {n}"),
            catalog_index: 10, // Neumann KM 184 approx
            pattern_override: None,
            offset_m: [offset_x, 3.0 * FEET_TO_METRES, 0.10],
            aim: MicAim::default(),
            enabled: true,
        };
        slot.aim.point_at_source(slot.offset_m);
        self.mics.push(slot);
        self.selected_mic = self.mics.len() - 1;
        self.clamp_all();
    }
    pub fn remove_selected_mic(&mut self) {
        if self.mics.len() <= 1 {
            return;
        }
        let idx = self.selected_mic.min(self.mics.len() - 1);
        self.mics.remove(idx);
        self.selected_mic = idx.min(self.mics.len() - 1);
    }
    /// Swap a microphone slot's catalog model WITHOUT moving its position or resetting its aim.
    pub fn swap_mic_model(&mut self, slot_idx: usize, catalog_index: usize) {
        let mics_len = mic_catalog().len();
        if mics_len == 0 || slot_idx >= self.mics.len() {
            return;
        }
        self.mics[slot_idx].catalog_index = catalog_index.min(mics_len - 1);
    }
    pub fn clamp_all(&mut self) {
        if self.mics.is_empty() {
            self.mics.push(MicSlot::sm7_baseline());
        }
        if self.mics.len() > Self::MAX_MIC_SLOTS {
            self.mics.truncate(Self::MAX_MIC_SLOTS);
        }
        self.selected_mic = self.selected_mic.min(self.mics.len() - 1);
        if !self.temperature_c.is_finite() {
            self.temperature_c = 20.0;
        }
        self.temperature_c = self.temperature_c.clamp(-20.0, 50.0);
        for d in &mut self.dimensions_m {
            *d = if d.is_finite() {
                d.clamp(1.5, 40.0)
            } else {
                3.048
            };
        }
        let size = self.dimensions_m;
        let src = self.source_pos_m();
        for mic in &mut self.mics {
            for i in 0..3 {
                let v = if mic.offset_m[i].is_finite() {
                    mic.offset_m[i]
                } else {
                    0.0
                };
                mic.offset_m[i] = v.clamp(0.10 - src[i], size[i] - src[i] - 0.10);
            }
            let dist =
                (mic.offset_m[0].powi(2) + mic.offset_m[1].powi(2) + mic.offset_m[2].powi(2))
                    .sqrt();
            if dist < 0.20 {
                mic.offset_m = [0.0, 0.20, 0.0];
            }
            if mic.aim.follow_source {
                mic.aim.point_at_source(mic.offset_m);
            }
        }
    }
    /// Acoustic key that excludes visual-only fields (`unit`, `show_patterns`, camera, layout).
    pub fn acoustic_fingerprint(&self) -> AcousticFingerprint {
        let q = |v: f32| (v * 10000.0).round() as i32 as u32;
        (
            self.source_index,
            [
                q(self.source_offset_m[0]),
                q(self.source_offset_m[1]),
                q(self.source_offset_m[2]),
            ],
            self.room_preset,
            [
                q(self.dimensions_m[0]),
                q(self.dimensions_m[1]),
                q(self.dimensions_m[2]),
            ],
            self.surface_faces,
            q(self.temperature_c),
            self.selected_mic,
            self.mics
                .iter()
                .map(|m| {
                    let d = m.aim.direction();
                    (
                        m.catalog_index,
                        m.pattern_override,
                        [q(m.offset_m[0]), q(m.offset_m[1]), q(m.offset_m[2])],
                        [q(d[0]), q(d[1]), q(d[2])],
                        m.enabled,
                    )
                })
                .collect(),
        )
    }
}

/// Quantized per-slot acoustic identity (catalog model, pattern override, position, aim, enabled).
pub type MicSlotFingerprint = (usize, Option<Pattern>, [u32; 3], [u32; 3], bool);

/// Quantized acoustic state of a `StudioRig`, excluding all display-only/layout/camera state.
pub type AcousticFingerprint = (
    usize,
    [u32; 3],
    RoomPreset,
    [u32; 3],
    [usize; 6],
    u32,
    usize,
    Vec<MicSlotFingerprint>,
);

// ======================= 6-BAND IMAGE-SOURCE ENGINE =======================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timing {
    /// Align direct arrival to t = 0 for clean A/B comparison.
    AlignDirect,
    /// Preserve absolute physical propagation delay `dist / c`.
    Physical,
}

#[derive(Clone, Copy, Debug)]
pub struct CapturePath6 {
    pub order: u32,
    pub delay_s: f64,
    pub band_gains: [f64; 6],
}

#[derive(Clone, Debug)]
pub struct CapturePlan6 {
    pub speed_of_sound_mps: f64,
    pub distance_m: f64,
    pub direct_arrival_s: f64,
    pub direct_pickup_gain: f64,
    pub first_reflection_s: Option<f64>,
    pub last_path_s: f64,
    pub six_band_drr_db: [Option<f64>; 6],
    pub paths: Vec<CapturePath6>,
}

fn axis_image_f64(p: f64, l: f64, n: i32) -> f64 {
    if n % 2 == 0 {
        f64::from(n) * l + p
    } else {
        f64::from(n + 1) * l - p
    }
}

/// Count hits on negative vs positive face along one axis for image index `n`.
fn axis_hits(n: i32) -> (i32, i32) {
    let a = n.abs();
    let half = a / 2;
    let neg = half + if n < 0 { a % 2 } else { 0 };
    let pos = a - neg;
    (neg, pos)
}

/// Compute exact 6-band image-source paths for a `StudioRig` (up to `max_order` reflections).
pub fn capture_paths_6band(
    rig: &StudioRig,
    sources: &[SourceEntry],
    mics: &[MicEntry],
    surfaces: &[SurfaceEntry],
    max_order: i32,
    timing: Timing,
) -> CapturePlan6 {
    let mut r = rig.clone();
    r.clamp_all();
    let c = speed_of_sound_celsius(f64::from(r.temperature_c));
    let src_rad = sources
        .get(r.source_index)
        .map_or(SourceRadiation::Omni, |s| s.radiation);
    let slot = r.active_mic();
    let mic_pat = slot.effective_pattern(mics);
    let directivity = Directivity::Ideal(mic_pat);

    let src = r.source_pos_m().map(f64::from);
    let off = slot.offset_m.map(f64::from);
    let at = [src[0] + off[0], src[1] + off[1], src[2] + off[2]];
    let size = r.dimensions_m.map(f64::from);
    let dist = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2])
        .sqrt()
        .max(0.20);
    let aim = slot.aim.direction().map(f64::from);
    let direct_arrival_s = dist / c;

    // Direct ray: source -> mic.
    // Under canonical `Timing::Physical`, direct pressure obeys calibrated 1/r spherical
    // spreading referenced to `DIRECT_REF_DISTANCE_M` (0.40 m -> 0.0 dB, 0.80 m -> -6.0206 dB)
    // and preserves physical acoustic propagation delay `dist / c`.
    // Under explicit `Timing::AlignDirect` monitoring, direct arrival is sample-aligned and
    // level-matched for instant audition comparisons.
    let dir_s2m = [off[0] / dist, off[1] / dist, off[2] / dist];
    let src_direct = src_rad.gain(dir_s2m[1]);
    let cos_mic_direct = -(dir_s2m[0] * aim[0] + dir_s2m[1] * aim[1] + dir_s2m[2] * aim[2]);
    let mic_direct = directivity.pressure(cos_mic_direct).unwrap_or(1.0);
    let direct_distance_scale = match timing {
        Timing::Physical => (DIRECT_REF_DISTANCE_M / dist).clamp(0.0, 2.0),
        Timing::AlignDirect => 1.0,
    };
    let direct_gain = (src_direct * mic_direct * direct_distance_scale).clamp(-2.0, 2.0);

    let direct_delay = match timing {
        Timing::AlignDirect => 0.0,
        Timing::Physical => direct_arrival_s,
    };

    let mut paths = vec![CapturePath6 {
        order: 0,
        delay_s: direct_delay,
        band_gains: [direct_gain; 6],
    }];

    // Fetch 6-face pressure reflectances [6 faces][6 bands]
    let face_r = |face_idx: usize| -> [f64; 6] {
        if r.room_preset == RoomPreset::Direct {
            return [0.0; 6];
        }
        if r.room_preset == RoomPreset::Open && face_idx != 4 {
            return [0.0; 6];
        }
        surfaces
            .get(r.surface_faces[face_idx])
            .map(SurfaceEntry::pressure_reflectance)
            .unwrap_or([0.0; 6])
    };
    let faces_r: [[f64; 6]; 6] = std::array::from_fn(face_r);
    let any_reflective = faces_r.iter().any(|bands| bands.iter().any(|&v| v > 1e-6));

    let mut refl_energy = [0.0_f64; 6];
    let mut first_refl = f64::INFINITY;
    let mut last_path = direct_delay;
    let order_lim = max_order.clamp(0, 5);

    let active_mic_entry = mics.get(slot.catalog_index);
    let published_offaxis = if slot.pattern_override.is_none() {
        active_mic_entry.and_then(|m| m.published_offaxis.as_ref())
    } else {
        None
    };

    if any_reflective && order_lim > 0 {
        for ix in -order_lim..=order_lim {
            for iy in -order_lim..=order_lim {
                for iz in -order_lim..=order_lim {
                    let order = ix.abs() + iy.abs() + iz.abs();
                    if !(1..=order_lim).contains(&order) {
                        continue;
                    }
                    if r.room_preset == RoomPreset::Open && (ix != 0 || iy != 0 || iz != -1) {
                        continue;
                    }
                    let img = [
                        axis_image_f64(src[0], size[0], ix),
                        axis_image_f64(src[1], size[1], iy),
                        axis_image_f64(src[2], size[2], iz),
                    ];
                    // Vector from mic toward image source
                    let to_img = [img[0] - at[0], img[1] - at[1], img[2] - at[2]];
                    let travel =
                        (to_img[0] * to_img[0] + to_img[1] * to_img[1] + to_img[2] * to_img[2])
                            .sqrt();
                    if travel <= dist + 1e-5 {
                        continue;
                    }
                    let delay_s = match timing {
                        Timing::AlignDirect => (travel - dist) / c,
                        Timing::Physical => travel / c,
                    };
                    if delay_s <= 0.0 && timing == Timing::AlignDirect {
                        continue;
                    }
                    let cos_mic =
                        (to_img[0] * aim[0] + to_img[1] * aim[1] + to_img[2] * aim[2]) / travel;
                    let mic_g_6band = published_offaxis
                        .and_then(|tbl| {
                            crate::acoustics_physics::offaxis_6band_pressure(tbl, cos_mic, mic_pat)
                        })
                        .unwrap_or_else(|| [directivity.pressure(cos_mic).unwrap_or(0.0); 6]);
                    // Reflection parity on source forward axis (+Y)
                    let y_parity = if iy % 2 == 0 { 1.0 } else { -1.0 };
                    let cos_src = ((-to_img[1]) / travel) * y_parity;
                    let src_g = src_rad.gain(cos_src);
                    let spread = match timing {
                        Timing::Physical => (DIRECT_REF_DISTANCE_M / travel).clamp(0.0, 2.0),
                        Timing::AlignDirect => dist / travel,
                    };
                    let base_scalar = spread * src_g;
                    if base_scalar.abs() < 1e-5 {
                        continue;
                    }
                    let air_6band = crate::acoustics_physics::air_absorption_amplitude_6band(
                        (travel - dist).max(0.0),
                        f64::from(r.temperature_c),
                        50.0,
                    );
                    let (nx, px) = axis_hits(ix);
                    let (ny, py) = axis_hits(iy);
                    let (nz, pz) = axis_hits(iz);
                    // Per-axis oblique incidence angles against wall normals (X, Y, Z) for JCA porous surfaces
                    let theta_x = (to_img[0].abs() / travel).clamp(0.05, 1.0).acos();
                    let theta_y = (to_img[1].abs() / travel).clamp(0.05, 1.0).acos();
                    let theta_z = (to_img[2].abs() / travel).clamp(0.05, 1.0).acos();
                    let face_oblique = |face_idx: usize, theta: f64| -> [f64; 6] {
                        if r.room_preset == RoomPreset::Direct
                            || (r.room_preset == RoomPreset::Open && face_idx != 4)
                        {
                            return [0.0; 6];
                        }
                        surfaces
                            .get(r.surface_faces[face_idx])
                            .map(|s| s.oblique_pressure_reflectance(theta))
                            .unwrap_or(faces_r[face_idx])
                    };
                    let rx_neg = if nx > 0 {
                        face_oblique(0, theta_x)
                    } else {
                        [1.0; 6]
                    };
                    let rx_pos = if px > 0 {
                        face_oblique(1, theta_x)
                    } else {
                        [1.0; 6]
                    };
                    let ry_neg = if ny > 0 {
                        face_oblique(2, theta_y)
                    } else {
                        [1.0; 6]
                    };
                    let ry_pos = if py > 0 {
                        face_oblique(3, theta_y)
                    } else {
                        [1.0; 6]
                    };
                    let rz_neg = if nz > 0 {
                        face_oblique(4, theta_z)
                    } else {
                        [1.0; 6]
                    };
                    let rz_pos = if pz > 0 {
                        face_oblique(5, theta_z)
                    } else {
                        [1.0; 6]
                    };

                    let mut band_gains = [0.0_f64; 6];
                    let mut max_abs = 0.0_f64;
                    for b in 0..6 {
                        let refl = rx_neg[b].powi(nx)
                            * rx_pos[b].powi(px)
                            * ry_neg[b].powi(ny)
                            * ry_pos[b].powi(py)
                            * rz_neg[b].powi(nz)
                            * rz_pos[b].powi(pz);
                        let g = base_scalar * mic_g_6band[b] * refl * air_6band[b];
                        band_gains[b] = g;
                        max_abs = max_abs.max(g.abs());
                    }
                    if max_abs < 1e-4 {
                        continue;
                    }
                    for b in 0..6 {
                        refl_energy[b] += band_gains[b] * band_gains[b];
                    }
                    let rel_s = (travel - dist) / c;
                    first_refl = first_refl.min(rel_s);
                    last_path = last_path.max(delay_s);
                    paths.push(CapturePath6 {
                        order: order as u32,
                        delay_s,
                        band_gains,
                    });
                }
            }
        }
    }

    let direct_sq = direct_gain.abs().max(1e-6).powi(2);
    let six_band_drr_db = std::array::from_fn(|b| {
        (refl_energy[b] > 1e-18).then(|| 10.0 * (direct_sq / refl_energy[b]).log10())
    });

    CapturePlan6 {
        speed_of_sound_mps: c,
        distance_m: dist,
        direct_arrival_s,
        direct_pickup_gain: direct_gain,
        first_reflection_s: first_refl.is_finite().then_some(first_refl),
        last_path_s: last_path,
        six_band_drr_db,
        paths,
    }
}

/// Exact complementary 6-band 1-pole crossover splitter:
/// `band[0] = lp(c0)`, `band[k] = lp(ck) - lp(c_{k-1})`, `band[5] = x - lp(c4)`.
/// Summing all 6 bands equals `x` identically at every sample!
#[cfg(test)]
struct SixBandSplit {
    cutoffs_hz: [f64; 5],
    alphas: [f64; 5],
    state: [f64; 5],
}

#[cfg(test)]
impl SixBandSplit {
    fn new(sample_rate: u32) -> Self {
        // Geometric midpoints between [125, 250, 500, 1000, 2000, 4000] Hz
        let cutoffs_hz = [
            (125.0_f64 * 250.0).sqrt(),
            (250.0_f64 * 500.0).sqrt(),
            (500.0_f64 * 1000.0).sqrt(),
            (1000.0_f64 * 2000.0).sqrt(),
            (2000.0_f64 * 4000.0).sqrt(),
        ];
        let fs = f64::from(sample_rate.max(1));
        let alphas = cutoffs_hz.map(|fc| {
            let clamped = fc.min(fs * 0.45);
            1.0 - (-std::f64::consts::TAU * clamped / fs).exp()
        });
        Self {
            cutoffs_hz,
            alphas,
            state: [0.0; 5],
        }
    }
    #[inline]
    fn split(&mut self, x: f64) -> [f64; 6] {
        let mut lp = [0.0_f64; 5];
        for i in 0..5 {
            self.state[i] += self.alphas[i] * (x - self.state[i]);
            lp[i] = self.state[i];
        }
        let _ = self.cutoffs_hz;
        [
            lp[0],
            lp[1] - lp[0],
            lp[2] - lp[1],
            lp[3] - lp[2],
            lp[4] - lp[3],
            x - lp[4],
        ]
    }
}

/// Render a mono dry signal through a 6-band `CapturePlan6` with 4-tap cubic Lagrange
/// fractional-delay taps (Laakso et al. 1996) and exact complementary 6-band splitting.
/// Output matches `input.len()` for real-time/in-place buffer compatibility.
pub fn render_capture_6band(input: &[f32], rate: u32, plan: &CapturePlan6) -> Vec<f32> {
    render_capture_6band_len(input, rate, plan, input.len())
}

/// Render a mono dry signal through a 6-band `CapturePlan6` preserving the complete
/// geometric reflection tail plus optional `extra_tail_s` (for offline WAV export).
pub fn render_capture_6band_with_tail(
    input: &[f32],
    rate: u32,
    plan: &CapturePlan6,
    extra_tail_s: f64,
) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    let fs = f64::from(rate.max(1));
    let tail_samples = ((plan.last_path_s.max(0.0) + extra_tail_s.max(0.0)) * fs).ceil() as usize;
    let out_len = input.len() + tail_samples.min(rate.max(1) as usize * 12);
    render_capture_6band_len(input, rate, plan, out_len)
}

fn render_capture_6band_len(
    input: &[f32],
    rate: u32,
    plan: &CapturePlan6,
    out_len: usize,
) -> Vec<f32> {
    if input.is_empty() || out_len == 0 {
        return Vec::new();
    }
    let n = input.len();
    let safe_rate = rate.max(1);
    let clean_in: Vec<f64> = input
        .iter()
        .map(|&s| if s.is_finite() { f64::from(s) } else { 0.0 })
        .collect();
    // 24 dB/oct Linkwitz-Riley 4th-order crossover isolates octave-band room reflections cleanly,
    // while order==0 direct arrival bypasses crossover splitting for zero-lag sample alignment.
    let mut splitter = crate::acoustics_physics::Lr4SixBandSplit::new(safe_rate);
    let mut bands = vec![[0.0_f64; 6]; n];
    for (i, &clean) in clean_in.iter().enumerate() {
        bands[i] = splitter.split(clean);
    }

    let mut out = vec![0.0_f64; out_len];
    let fs = f64::from(safe_rate);
    for path in &plan.paths {
        let d_samp = path.delay_s * fs;
        if !d_samp.is_finite() || d_samp < 0.0 {
            continue;
        }
        let d0 = d_samp.floor() as usize;
        if d0 >= out_len {
            continue;
        }
        let g = path.band_gains;
        if !g.iter().all(|v| v.is_finite()) {
            continue;
        }
        let frac = d_samp - d0 as f64;
        let direct_flat = path.order == 0;
        let sinc16 = if direct_flat && frac > 1e-12 {
            Some(crate::acoustics_physics::sinc16_weights_phase(frac))
        } else {
            None
        };
        let lag = crate::acoustics_physics::lagrange4_weights(frac);
        let max_src = n.min(out_len - d0);
        for src_i in 0..max_src {
            let v = if direct_flat {
                clean_in[src_i] * g[0]
            } else {
                let b = bands[src_i];
                b[0] * g[0] + b[1] * g[1] + b[2] * g[2] + b[3] * g[3] + b[4] * g[4] + b[5] * g[5]
            };
            let dst = src_i + d0;
            if frac <= 1e-12 {
                out[dst] += v;
            } else if let (Some(w16), true) = (sinc16, dst >= 7 && dst + 8 < out_len) {
                for (t, &wt) in w16.iter().enumerate() {
                    out[dst + t - 7] += v * wt;
                }
            } else if dst >= 1 && dst + 2 < out_len {
                out[dst - 1] += v * lag[0];
                out[dst] += v * lag[1];
                out[dst + 1] += v * lag[2];
                out[dst + 2] += v * lag[3];
            } else {
                out[dst] += v * (1.0 - frac);
                if dst + 1 < out_len {
                    out[dst + 1] += v * frac;
                }
            }
        }
    }
    out.into_iter()
        .map(|v| if v.is_finite() { v as f32 } else { 0.0 })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_are_factual_and_honest_with_sm7b_baseline_at_zero() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        assert_eq!(sources.len(), 138);
        assert_eq!(surfaces.len(), 32);
        assert!(mics.len() >= 60);
        // Zero synthetic "Studio LDC Mk" models
        assert!(mics.iter().all(|m| !m.model.contains("Studio LDC Mk")));
        // SM7B baseline at index 0
        assert_eq!(mics[0].id, "shure-sm7b");
        assert_eq!(mics[0].brand, "Shure");
        assert_eq!(mics[0].model, "SM7B");
        assert!(mics[0]
            .curve_provenance
            .contains("not a measured hardware IR"));
        // 2-inch wedge foam ASTM C423 1 kHz value > 1.0 clamps to passive reflectance == 0.0
        assert_eq!(surfaces[1].id, "wedge_foam_2in");
        assert!(surfaces[1].absorption_6band[3] > 1.0);
        assert_eq!(surfaces[1].pressure_reflectance()[3], 0.0);
    }

    #[test]
    fn six_band_complementary_splitter_sums_to_exact_unity() {
        let mut s = SixBandSplit::new(48_000);
        for i in 0..2000 {
            let x = (i as f64 * 0.137).sin() + 0.4 * (i as f64 * 0.51).cos();
            let bands = s.split(x);
            let sum: f64 = bands.iter().sum();
            assert!((sum - x).abs() < 1e-12, "unity mismatch: {sum} vs {x}");
        }
    }

    #[test]
    fn hypercardioid_and_measured_1d_directivity_behave_honestly() {
        assert!((Pattern::Hypercardioid.pressure(1.0) - 1.0).abs() < 1e-12);
        assert!((Pattern::Hypercardioid.pressure(0.0) - 0.25).abs() < 1e-12);
        assert!((Pattern::Hypercardioid.pressure(-1.0) + 0.50).abs() < 1e-12);
        static PARTIAL: [PolarSample; 2] = [
            PolarSample {
                angle_deg: 0.0,
                pressure: 1.0,
            },
            PolarSample {
                angle_deg: 90.0,
                pressure: 0.5,
            },
        ];
        let d = Directivity::Measured1d(&PARTIAL);
        assert!((d.pressure(1.0).unwrap() - 1.0).abs() < 1e-9);
        assert!((d.pressure(0.0).unwrap() - 0.5).abs() < 1e-9);
        // Rejects angles outside supplied measured range (180 deg > 90 deg)
        assert!(d.pressure(-1.0).is_none());
    }

    #[test]
    fn unit_changes_never_resize_physical_room_or_mic() {
        let mut rig = StudioRig::sm7b_foam_reference();
        let before_dims = rig.dimensions_m;
        let before_mic = rig.active_mic().offset_m;
        let before_fp = rig.acoustic_fingerprint();
        for u in Unit::ALL {
            rig.unit = u;
            assert_eq!(rig.dimensions_m, before_dims);
            assert_eq!(rig.active_mic().offset_m, before_mic);
            assert_eq!(rig.acoustic_fingerprint(), before_fp);
            let roundtrip = u.to_metres(u.from_metres(before_dims[0]));
            assert!((roundtrip - before_dims[0]).abs() < 1e-4);
        }
    }

    #[test]
    fn chain_link_scales_linked_dimensions_and_leaves_unlocked_alone() {
        let mut rig = StudioRig {
            dim_linked: [true, true, false],
            ..StudioRig::default()
        };
        let h_before = rig.dimensions_m[2];
        rig.set_dimension_m(0, rig.dimensions_m[0] * 2.0);
        assert!((rig.dimensions_m[0] - 20.0 * FEET_TO_METRES).abs() < 1e-4);
        assert!((rig.dimensions_m[1] - 20.0 * FEET_TO_METRES).abs() < 1e-4);
        assert!((rig.dimensions_m[2] - h_before).abs() < 1e-6);
    }

    #[test]
    fn rotating_and_aiming_mic_never_moves_mic_position() {
        let mut rig = StudioRig::default();
        let pos_before = rig.active_mic().offset_m;
        for dir in GridDirection::ALL {
            rig.active_mic_mut().aim.set_grid_direction(dir);
            assert_eq!(rig.active_mic().offset_m, pos_before);
        }
        rig.active_mic_mut().aim.tilt_x_deg = 22.5;
        rig.active_mic_mut().aim.roll_y_deg = 30.0;
        rig.active_mic_mut().aim.heading_z_deg = 77.0;
        rig.active_mic_mut().aim.snap_angles(15.0);
        assert_eq!(rig.active_mic().offset_m, pos_before);
        assert_eq!(rig.active_mic().aim.heading_z_deg, 75.0);
        let offset = rig.active_mic().offset_m;
        rig.active_mic_mut().aim.point_at_source(offset);
        assert_eq!(rig.active_mic().offset_m, pos_before);
        let d = rig.active_mic().aim.direction();
        assert!(
            (d[1] + 1.0).abs() < 1e-4,
            "expected pointing -Y toward source, got {d:?}"
        );
    }

    #[test]
    fn capture_lab_single_variable_audio_and_unit_invariance() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        // Deterministic rich broadband pulse train (1200 samples @ 48 kHz = 25 ms)
        let rate = 48_000_u32;
        let dry: Vec<f32> = (0..1800)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let env = (-t * 140.0).exp();
                (env * ((2.0 * std::f64::consts::PI * 220.0 * t).sin()
                    + 0.5 * (2.0 * std::f64::consts::PI * 880.0 * t).sin()
                    + 0.3 * (2.0 * std::f64::consts::PI * 2640.0 * t).sin())) as f32
            })
            .collect();

        // Start in Medium room (reflective wood/plaster surfaces) with source centered at 5 ft
        let mut base = StudioRig::default();
        base.apply_preset(RoomPreset::Medium);
        assert!((base.source_pos_m()[2] - 5.0 * FEET_TO_METRES).abs() < 1e-5);
        assert_eq!(
            RoomPreset::Open.dimensions_m(),
            [
                50. * FEET_TO_METRES,
                50. * FEET_TO_METRES,
                50. * FEET_TO_METRES
            ]
        );

        let render = |rig: &StudioRig| {
            let plan = capture_paths_6band(rig, &sources, &mics, &surfaces, 2, Timing::AlignDirect);
            render_capture_6band(&dry, rate, &plan)
        };
        let rms_diff = |a: &[f32], b: &[f32]| -> f64 {
            let sq: f64 = a
                .iter()
                .zip(b)
                .map(|(&x, &y)| (f64::from(x) - f64::from(y)).powi(2))
                .sum();
            (sq / a.len().max(1) as f64).sqrt()
        };

        let y_base = render(&base);

        // 1. Move mic -> real audio changes
        let mut moved = base.clone();
        moved.active_mic_mut().offset_m[0] += 0.9;
        assert!(rms_diff(&y_base, &render(&moved)) > 1e-4);

        // 2. Change ONE single wall material (-X left wall only) -> real audio changes; ceiling != floor
        let mut wall_changed = base.clone();
        wall_changed.surface_faces[0] = 0; // open boundary on -X only
        assert_eq!(wall_changed.surface_faces[4], base.surface_faces[4]);
        assert_eq!(wall_changed.surface_faces[5], base.surface_faces[5]);
        assert!(rms_diff(&y_base, &render(&wall_changed)) > 1e-4);

        // 3. Resize room (24 ft with Length+Width linked -> both 24 ft, Height stays 10 ft, source stays centered & 5 ft)
        let mut resized = base.clone();
        resized.dim_linked = [true, true, false];
        resized.set_dimension_m(0, 24.0 * FEET_TO_METRES);
        assert!((resized.dimensions_m[0] - 24.0 * FEET_TO_METRES).abs() < 1e-4);
        assert!((resized.dimensions_m[1] - 24.0 * FEET_TO_METRES).abs() < 1e-4);
        assert!((resized.dimensions_m[2] - 10.0 * FEET_TO_METRES).abs() < 1e-4);
        assert!((resized.source_pos_m()[0] - 12.0 * FEET_TO_METRES).abs() < 1e-4);
        assert!((resized.source_pos_m()[1] - 12.0 * FEET_TO_METRES).abs() < 1e-4);
        assert!((resized.source_pos_m()[2] - 5.0 * FEET_TO_METRES).abs() < 1e-5);
        assert!(rms_diff(&y_base, &render(&resized)) > 1e-4);

        // 4. Independent pickup pattern override -> real audio changes without changing mic model
        let mut pat_changed = base.clone();
        pat_changed.active_mic_mut().pattern_override = Some(Pattern::Figure8);
        assert_eq!(
            pat_changed.active_mic().catalog_index,
            base.active_mic().catalog_index
        );
        assert!(rms_diff(&y_base, &render(&pat_changed)) > 1e-4);

        // 5. Switch ONLY display units & visual-only pattern overlay -> BIT-IDENTICAL audio!
        for u in Unit::ALL {
            let mut unit_only = base.clone();
            unit_only.unit = u;
            unit_only.show_patterns = !base.show_patterns;
            assert_eq!(unit_only.dimensions_m, base.dimensions_m);
            assert_eq!(render(&unit_only), y_base);
        }
    }

    #[test]
    fn acceptance_3d_capture_lab_all_six_checks() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        let rate = 48_000_u32;
        let dry: Vec<f32> = (0..2400)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let env = (-t * 110.0).exp();
                (env * ((2.0 * std::f64::consts::PI * 196.0 * t).sin()
                    + 0.45 * (2.0 * std::f64::consts::PI * 588.0 * t).sin()
                    + 0.25 * (2.0 * std::f64::consts::PI * 1764.0 * t).sin()))
                    as f32
            })
            .collect();
        let render = |rig: &StudioRig| {
            let plan = capture_paths_6band(rig, &sources, &mics, &surfaces, 2, Timing::AlignDirect);
            render_capture_6band(&dry, rate, &plan)
        };
        let rms_diff = |a: &[f32], b: &[f32]| -> f64 {
            let sq: f64 = a
                .iter()
                .zip(b)
                .map(|(&x, &y)| (f64::from(x) - f64::from(y)).powi(2))
                .sum();
            (sq / a.len().max(1) as f64).sqrt()
        };

        // Start: 10×10×10 ft treated room, Source at center + 5 ft, SM7B at ~2 ft aimed at source
        let mut rig = StudioRig::sm7b_foam_reference();
        rig.surface_faces = [14, 14, 14, 14, 15, 6]; // reflective wood/oak/tile for strong reflections
        let y0 = render(&rig);
        let mic_world_0 = rig.mic_world_pos_m(0);

        // 1. Move source off-center and change height -> mic world stays fixed, capture changes
        let mut s1 = rig.clone();
        s1.set_source_pos_m([1.0, 1.1, 1.0]);
        assert!((s1.source_pos_m()[0] - 1.0).abs() < 1e-4);
        assert!((s1.source_pos_m()[2] - 1.0).abs() < 1e-4);
        assert!((s1.mic_world_pos_m(0)[0] - mic_world_0[0]).abs() < 1e-4);
        assert!(rms_diff(&y0, &render(&s1)) > 1e-4);

        // 2. Move mic in Top (X/Y) and Front (X/Z) -> coordinates update, capture changes
        let mut s2 = rig.clone();
        s2.active_mic_mut().offset_m[0] += 0.55;
        s2.active_mic_mut().offset_m[2] -= 0.35;
        assert!(rms_diff(&y0, &render(&s2)) > 1e-4);

        // 3. Follow Source enabled: moving source re-aims mic without moving mic in world coords
        let mut s3 = rig.clone();
        s3.active_mic_mut().aim.follow_source = true;
        let world_before = s3.mic_world_pos_m(0);
        s3.set_source_pos_m([2.2, 1.2, 1.1]);
        let world_after = s3.mic_world_pos_m(0);
        for i in 0..3 {
            assert!((world_after[i] - world_before[i]).abs() < 1e-4);
        }
        let aim_dir = s3.active_mic().aim.direction();
        let off = s3.active_mic().offset_m;
        let len = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
        let expected_aim = [-off[0] / len, -off[1] / len, -off[2] / len];
        for i in 0..3 {
            assert!((aim_dir[i] - expected_aim[i]).abs() < 1e-3);
        }

        // 4. Swap one wall material -> reflected audio changes
        let mut s4 = rig.clone();
        s4.surface_faces[1] = 0; // +X wall -> open boundary
        assert!(rms_diff(&y0, &render(&s4)) > 1e-4);

        // 5. Visual-only / ruler-unit changes -> audio remains bit-identical
        let mut s5 = rig.clone();
        s5.unit = Unit::Millimetres;
        s5.show_patterns = !rig.show_patterns;
        assert_eq!(render(&s5), y0);

        // 6. Reset Source to Default returns center + 5 ft without affecting mic model or materials
        let mut s6 = s1.clone();
        s6.reset_source_center_5ft();
        assert!((s6.source_pos_m()[0] - rig.dimensions_m[0] * 0.5).abs() < 1e-4);
        assert!((s6.source_pos_m()[1] - rig.dimensions_m[1] * 0.5).abs() < 1e-4);
        assert!((s6.source_pos_m()[2] - 5.0 * FEET_TO_METRES).abs() < 1e-4);
        assert_eq!(s6.active_mic().catalog_index, s1.active_mic().catalog_index);
        assert_eq!(s6.surface_faces, s1.surface_faces);
    }

    #[test]
    fn edge_cases_never_panic_or_propagate_nan() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();

        // Zero, negative, huge, and NaN room dimensions + mic exactly on source + out-of-range catalog indices
        let mut rig = StudioRig {
            dimensions_m: [0.0, -10.0, f32::NAN],
            temperature_c: f32::INFINITY,
            source_index: 99_999,
            surface_faces: [99_999; 6],
            ..StudioRig::default()
        };
        rig.active_mic_mut().catalog_index = 99_999;
        rig.active_mic_mut().offset_m = [0.0, 0.0, 0.0];
        rig.set_source_pos_m([999.0, -999.0, f32::NAN]);
        rig.clamp_all();
        assert!(rig
            .dimensions_m
            .iter()
            .all(|d| d.is_finite() && *d >= 1.5 && *d <= 40.0));
        let off = rig.active_mic().offset_m;
        let dist = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
        assert!(
            dist >= 0.199,
            "mic on source must clamp to >= 0.20 m separation"
        );

        // Out-of-range and NaN surface absorption
        let weird_surf = SurfaceEntry {
            id: "weird".into(),
            name: "Weird".into(),
            category: "Test".into(),
            material_type: "Test".into(),
            thickness_mm: None,
            assembly: "test".into(),
            incidence: "test".into(),
            standard: "test".into(),
            response_kind: "Analytical model".into(),
            absorption_6band: [-0.5, 0.0, 0.5, 1.25, f32::NAN, f32::INFINITY],
            biot: BiotParameters::default(),
            provenance: "test".into(),
            notes: "test".into(),
            uncertainties: Vec::new(),
        };
        for r in weird_surf.pressure_reflectance() {
            assert!(r.is_finite() && (0.0..=1.0).contains(&r));
        }

        // NaN/Inf in input audio & zero sample rate must not panic or output NaN
        let plan = capture_paths_6band(&rig, &sources, &mics, &surfaces, 2, Timing::AlignDirect);
        let dirty = vec![0.5, f32::NAN, -0.5, f32::INFINITY, f32::NEG_INFINITY, 0.25];
        let out = render_capture_6band(&dirty, 0, &plan);
        assert_eq!(out.len(), dirty.len());
        assert!(out.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn refactor_guard_delay_from_distance_and_absorption_multipliers() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();

        // Physical timing must equal exact distance / speed_of_sound
        let rig = StudioRig::default();
        let plan_phys = capture_paths_6band(&rig, &sources, &mics, &surfaces, 1, Timing::Physical);
        let expected_c = speed_of_sound_celsius(f64::from(rig.temperature_c));
        let expected_delay = plan_phys.distance_m / expected_c;
        assert!(
            (plan_phys.direct_arrival_s - expected_delay).abs() < 1e-9,
            "direct arrival must equal distance / c"
        );
        assert!(
            (plan_phys.paths[0].delay_s - expected_delay).abs() < 1e-9,
            "order-0 physical path delay must equal distance / c"
        );

        // Passive reflectance must equal sqrt(1 - clamp(alpha, 0, 1)) across all 32 catalog surfaces
        for surf in &surfaces {
            let eff = surf.effective_absorption();
            let refl = surf.pressure_reflectance();
            for b in 0..6 {
                let expected = (1.0 - eff[b]).clamp(0.0, 1.0).sqrt();
                assert!(
                    (refl[b] - expected).abs() < 1e-9,
                    "surface {} band {} reflectance mismatch",
                    surf.id,
                    b
                );
            }
        }
    }

    #[test]
    fn refactor_guard_catalog_reachability_and_mic_swap_preserves_placement() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        assert_eq!(sources.len(), 138);
        assert_eq!(mics.len(), 972);
        assert_eq!(surfaces.len(), 32);

        // Verify late-catalog Sennheiser & DPA Mic Archive entries carry real published data
        let mkh50_idx = mics
            .iter()
            .position(|m| m.id == "sennheiser-mkh-50")
            .expect("Sennheiser MKH 50 present in 972-mic catalog");
        assert!(mkh50_idx > 700, "Sennheiser MKH 50 must be late in catalog");
        assert!(!mics[mkh50_idx].published_fr_points.is_empty());
        assert!(mics[mkh50_idx].published_sensitivity_dbv.is_some());
        assert!(mics[mkh50_idx].published_self_noise_dba.is_some());

        let dpa_idx = mics
            .iter()
            .position(|m| m.id == "dpa-4011a")
            .expect("DPA 4011A present in 972-mic catalog");
        assert!(!mics[dpa_idx].published_fr_points.is_empty());

        let mut rig = StudioRig::default();
        rig.active_mic_mut().offset_m = [0.42, 1.15, 0.25];
        rig.active_mic_mut().aim = MicAim {
            tilt_x_deg: -12.0,
            roll_y_deg: 5.0,
            heading_z_deg: -18.0,
            follow_source: false,
        };
        let pos_before = rig.active_mic().offset_m;
        let aim_before = rig.active_mic().aim;

        // Swap to late-catalog Sennheiser MKH 50 — position and aim must NOT change
        rig.swap_mic_model(0, mkh50_idx);
        assert_eq!(rig.active_mic().catalog_index, mkh50_idx);
        assert_eq!(
            rig.active_mic().offset_m,
            pos_before,
            "swapping mic model must never move position"
        );
        assert_eq!(
            rig.active_mic().aim,
            aim_before,
            "swapping mic model must never reset aim"
        );

        // Swapping to the very last catalog entry (index 971) also preserves placement
        let last_idx = mics.len() - 1;
        rig.swap_mic_model(0, last_idx);
        assert_eq!(rig.active_mic().catalog_index, last_idx);
        assert_eq!(rig.active_mic().offset_m, pos_before);
        assert_eq!(rig.active_mic().aim, aim_before);

        // Selecting a late-catalog figure-8 or supercardioid mic changes the actual rendered capture
        let dry: Vec<f32> = (0..1200)
            .map(|i| ((i as f32) * 0.08).sin() * (-((i as f32) / 600.0)).exp())
            .collect();
        let dry_copy = dry.clone();

        let mut rig_a = StudioRig::default();
        rig_a.swap_mic_model(0, 0); // SM7B cardioid
        let mut rig_b = rig_a.clone();
        rig_b.swap_mic_model(0, mkh50_idx); // Sennheiser MKH 50 supercardioid
        assert_eq!(rig_a.active_mic().offset_m, rig_b.active_mic().offset_m);
        assert_eq!(rig_a.active_mic().aim, rig_b.active_mic().aim);

        let out_a = render_capture_6band(
            &dry,
            48_000,
            &capture_paths_6band(&rig_a, &sources, &mics, &surfaces, 2, Timing::AlignDirect),
        );
        let out_b = render_capture_6band(
            &dry,
            48_000,
            &capture_paths_6band(&rig_b, &sources, &mics, &surfaces, 2, Timing::AlignDirect),
        );
        assert_eq!(dry, dry_copy, "dry buffer must never be mutated in place");
        assert_ne!(
            out_a, out_b,
            "late-catalog Sennheiser MKH 50 must change rendered capture relative to SM7B"
        );

        // Open preset uses ONLY ground surface; changing a wall face in Open mode leaves audio identical
        let mut open_a = StudioRig::default();
        open_a.apply_preset(RoomPreset::Open);
        let mut open_b = open_a.clone();
        open_b.surface_faces[0] = 28; // sheet steel on -X wall (ignored in Open mode)
        let open_plan_a =
            capture_paths_6band(&open_a, &sources, &mics, &surfaces, 2, Timing::AlignDirect);
        let open_plan_b =
            capture_paths_6band(&open_b, &sources, &mics, &surfaces, 2, Timing::AlignDirect);
        assert_eq!(
            open_plan_a.paths.len(),
            2,
            "Open preset has direct + single ground bounce"
        );
        assert_eq!(
            render_capture_6band(&dry, 48_000, &open_plan_a),
            render_capture_6band(&dry, 48_000, &open_plan_b),
            "wall changes must not affect Open mode"
        );
        // Changing ground material in Open mode MUST change reflected capture
        let mut open_c = open_a.clone();
        open_c.surface_faces[4] = 31; // asphalt instead of grass over soil
        let open_plan_c =
            capture_paths_6band(&open_c, &sources, &mics, &surfaces, 2, Timing::AlignDirect);
        assert_ne!(
            render_capture_6band(&dry, 48_000, &open_plan_a),
            render_capture_6band(&dry, 48_000, &open_plan_c),
            "changing ground material in Open mode must alter capture audio"
        );
    }

    #[test]
    fn refactor_guard_display_only_actions_keep_audio_bit_identical() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        let dry: Vec<f32> = (0..960).map(|i| ((i as f32) * 0.11).sin() * 0.4).collect();

        let base = StudioRig::default();
        let base_fp = base.acoustic_fingerprint();
        let base_out = render_capture_6band(
            &dry,
            48_000,
            &capture_paths_6band(&base, &sources, &mics, &surfaces, 2, Timing::AlignDirect),
        );

        for unit in Unit::ALL {
            for show_patterns in [false, true] {
                let mut mutated = base.clone();
                mutated.unit = unit;
                mutated.show_patterns = show_patterns;
                assert_eq!(mutated.acoustic_fingerprint(), base_fp);
                let out = render_capture_6band(
                    &dry,
                    48_000,
                    &capture_paths_6band(
                        &mutated,
                        &sources,
                        &mics,
                        &surfaces,
                        2,
                        Timing::AlignDirect,
                    ),
                );
                assert_eq!(
                    out, base_out,
                    "display unit {unit:?} / patterns={show_patterns} must leave audio bit-identical"
                );
            }
        }
    }

    #[test]
    fn extensible_source_catalog_accepts_new_and_corrected_entries_without_engine_changes() {
        let custom_json = r#"{
            "sources": [
                {
                    "id": "custom_hurdy_gurdy",
                    "name": "Hurdy-Gurdy (Custom Entry)",
                    "category": "Instrument",
                    "family": "Strings — Wheel Bowed",
                    "symbol": "bowed",
                    "synthesis_model_hint": "bowed",
                    "published_range_hz": [196, 1046],
                    "spectral_regions": {
                        "body_warmth_hz": [180, 420],
                        "presence_attack_hz": [1400, 3200],
                        "air_high_hz": null,
                        "formants_and_resonances_note": "Wheel-bowed drone & chanter; upper air peak intentionally omitted."
                    },
                    "source_references": ["User custom capture-reference entry"],
                    "uncertainties": ["Air band missing in reference data — left null without invention."]
                }
            ]
        }"#;
        let parsed = parse_source_catalog_json(custom_json).expect("custom catalog parses");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].synthesis_model_hint, "bowed");
        assert_eq!(parsed[0].regen_model.family_kind, SourceRegenKind::BowedLow);
        assert!(parsed[0].spectral_regions.air_high_hz.is_none());

        // Immediately synthesizable with zero Rust engine changes
        let perf = crate::reperform::Performance {
            rate: 48_000,
            samples: 24_000,
            hop_s: 0.005,
            notes: vec![crate::reperform::Note {
                string_id: None,
                re_picks: None,
                start: 0.05,
                end: 0.40,
                hz: vec![293.66; 71],
                level_steps: vec![0.35; 71],
                _accent: None,
                shape: crate::reperform::Shape::default(),
            }],
            frame_evidence: vec![],
        };
        let dry = perf
            .synthesize_dry_for_source(&parsed[0])
            .expect("custom catalog entry synthesizes directly");
        assert_eq!(dry.len(), 24_000);
        assert!(dry.iter().any(|&v| v.abs() > 0.01));
    }
}
