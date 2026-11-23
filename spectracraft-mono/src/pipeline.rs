//! Clean Sequential Post-Dry Pipeline (Stages 3, 4, 5) + Crash-Safe Atomic WAV Export.
//!
//! Stage 1 (Dry Regeneration) and Stage 2 (Surgical Region Edits) produce the cached dry take.
//! This module executes the optional, independently bypassable downstream stages strictly from
//! that dry take (zero original audio ever opened or blended):
//!   - Stage 3: Optional Microphone Stage (OFF by default)
//!   - Stage 4: Optional Space / Room Stage (OFF by default; defaults to 10x10x10 ft padded room)
//!   - Stage 5: Optional Plugin / Processing Slot before final output (OFF / empty by default)
//!
//! Crash-safe file output uses vendored `craft_atomic::atomic_write` from `storytold/craft-libs`
//! (MIT OR Apache-2.0, pinned rev b24eba64758f1b9135e7a3afab1af88260f9323b).

use crate::{
    capture, digest,
    region_edit::{self, DryRegionEdit, RegionEditOutput},
    reperform::{self, Performance, VoicingControls},
    studio_catalog::{
        mic_catalog, source_catalog, surface_catalog, MicEntry, Pattern, RoomPreset, SourceEntry,
        StudioRig, SurfaceEntry,
    },
    Result,
};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginId {
    None,
    GentleEq,
    SoftDynamics,
    NoiseControl,
    PhaseLimiter,
}

impl Default for PluginId {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginSlotConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub plugin_id: PluginId,
    /// Low-shelf / body gentle trim in dB (-4..+4 dB) for GentleEq.
    #[serde(default)]
    pub eq_low_db: f64,
    /// High-shelf / air gentle trim in dB (-4..+4 dB) for GentleEq.
    #[serde(default)]
    pub eq_high_db: f64,
    /// Processing intensity (0.0..=1.0) for SoftDynamics / NoiseControl / PhaseLimiter.
    #[serde(default = "default_plugin_amount")]
    pub amount: f64,
    /// Limiter output ceiling in dBFS (default -1.0 dBFS).
    #[serde(default = "default_ceiling_dbfs")]
    pub ceiling_dbfs: f64,
}

fn default_plugin_amount() -> f64 {
    0.45
}
fn default_ceiling_dbfs() -> f64 {
    -1.0
}

impl Default for PluginSlotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            plugin_id: PluginId::None,
            eq_low_db: 0.0,
            eq_high_db: 0.0,
            amount: 0.45,
            ceiling_dbfs: -1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PostDryPipelineConfig {
    /// Stage 3: Optional Microphone Stage (OFF by default).
    pub mic_enabled: bool,
    /// Stage 4: Optional Space / Room Stage (OFF by default; defaults to 10x10x10 ft padded room).
    pub room_enabled: bool,
    /// Full 3D rig parameters (mic model/position/aim + room size/materials/source position).
    pub rig: StudioRig,
    /// Stage 5: Optional Plugin / Processing Slot before final output (OFF / empty by default).
    pub plugin_slot: PluginSlotConfig,
}

impl Default for PostDryPipelineConfig {
    fn default() -> Self {
        Self {
            mic_enabled: false,
            room_enabled: false,
            rig: StudioRig::sm7b_foam_reference(), // 10x10x10 ft padded room starting point
            plugin_slot: PluginSlotConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PipelineStageSummary {
    pub dry_sha256: String,
    pub output_sha256: String,
    pub mic_stage_active: bool,
    pub room_stage_active: bool,
    pub plugin_slot_active: bool,
    pub active_plugin: PluginId,
    pub bypassed_stages: Vec<String>,
    pub bit_identical_to_dry: bool,
    pub original_audio_opened: bool,
}

pub struct PipelineRenderOutput {
    pub samples: Vec<f32>,
    pub summary: PipelineStageSummary,
}

fn hash_f32_samples(samples: &[f32]) -> String {
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    digest::sha256(&bytes)
}

/// Write a 32-bit float mono WAV crash-safely using vendored `craft_atomic::atomic_write`.
/// Refuses non-finite (`NaN`/`Inf`) samples rather than silently coercing them to zero.
pub fn write_wav_f32_atomic(path: &Path, rate: u32, samples: &[f32]) -> Result<()> {
    if let Some((idx, &bad)) = samples.iter().enumerate().find(|(_, s)| !s.is_finite()) {
        return Err(format!("Refusing to write non-finite sample ({bad}) at index {idx}").into());
    }
    let mut cursor = Cursor::new(Vec::<u8>::with_capacity(samples.len() * 4 + 64));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for &s in samples {
            writer.write_sample(s)?;
        }
        writer.finalize()?;
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    craft_atomic::atomic_write(path, cursor.get_ref())?;
    Ok(())
}

/// Write UTF-8 JSON bytes crash-safely using vendored `craft_atomic::atomic_write`.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    craft_atomic::atomic_write(path, bytes)?;
    Ok(())
}

/// Execute Stages 3 (Optional Mic), 4 (Optional Room), and 5 (Optional Plugin Slot)
/// sequentially from the cached dry take. Any bypassed stage is a strict bit-identical pass-through.
pub fn run_post_dry_pipeline(
    dry_samples: &[f32],
    rate: u32,
    cfg: &PostDryPipelineConfig,
    sources: &[SourceEntry],
    mics: &[MicEntry],
    surfaces: &[SurfaceEntry],
) -> PipelineRenderOutput {
    let dry_sha256 = hash_f32_samples(dry_samples);
    let mut bypassed_stages = Vec::new();

    // Stages 3 & 4: Optional Microphone + Optional Space/Room
    let mut signal: Vec<f32> = if !cfg.mic_enabled && !cfg.room_enabled {
        bypassed_stages.push("microphone".into());
        bypassed_stages.push("space_room".into());
        dry_samples.to_vec()
    } else {
        let mut effective_rig = cfg.rig.clone();

        if !cfg.room_enabled {
            // Stage 3 ONLY: Apply microphone profile, polar pattern, distance, and 3D aim
            // with NO room walls/reflections (RoomPreset::Direct).
            bypassed_stages.push("space_room".into());
            effective_rig.room_preset = RoomPreset::Direct;
        }
        if !cfg.mic_enabled {
            // Stage 4 ONLY: Apply room size, 6-face materials, and free 3D source+mic geometry
            // using a strictly flat ideal omnidirectional pickup (zero mic capsule EQ coloration).
            bypassed_stages.push("microphone".into());
            for slot in &mut effective_rig.mics {
                slot.pattern_override = Some(Pattern::Omni);
            }
        }

        let active_indices: Vec<usize> = effective_rig
            .mics
            .iter()
            .enumerate()
            .filter(|(_, m)| m.enabled)
            .map(|(i, _)| i)
            .collect();
        let indices = if active_indices.is_empty() {
            vec![effective_rig
                .selected_mic
                .min(effective_rig.mics.len().saturating_sub(1))]
        } else {
            active_indices
        };

        let mut mixed = vec![0.0_f32; dry_samples.len()];
        let inv_count = 1.0 / (indices.len() as f32);
        for &mi in &indices {
            let mut slot_rig = effective_rig.clone();
            slot_rig.selected_mic = mi;
            let rendered = capture::render_studio_rig_with_mic_eq(
                dry_samples,
                rate,
                &slot_rig,
                sources,
                mics,
                surfaces,
                cfg.mic_enabled,
            );
            for (dst, &v) in mixed.iter_mut().zip(rendered.iter()) {
                *dst += v * inv_count;
            }
        }
        mixed
    };

    // Stage 5: Optional Plugin / Processing Slot (before final output)
    let plugin_active = cfg.plugin_slot.enabled && cfg.plugin_slot.plugin_id != PluginId::None;
    if !plugin_active {
        bypassed_stages.push("plugin_slot".into());
    } else {
        apply_plugin_slot(&mut signal, rate, &cfg.plugin_slot);
    }

    let output_sha256 = hash_f32_samples(&signal);
    let bit_identical_to_dry = signal == dry_samples;

    PipelineRenderOutput {
        samples: signal,
        summary: PipelineStageSummary {
            dry_sha256,
            output_sha256,
            mic_stage_active: cfg.mic_enabled,
            room_stage_active: cfg.room_enabled,
            plugin_slot_active: plugin_active,
            active_plugin: if plugin_active {
                cfg.plugin_slot.plugin_id
            } else {
                PluginId::None
            },
            bypassed_stages,
            bit_identical_to_dry,
            original_audio_opened: false,
        },
    }
}

fn apply_biquad_shelf(buf: &mut [f32], rate: f64, f0: f64, gain_db: f64, high_shelf: bool) {
    if gain_db.abs() < 1e-3 || f0 <= 20.0 || f0 >= rate * 0.45 {
        return;
    }
    let a = 10.0_f64.powf(gain_db / 40.0);
    let w0 = TAU * f0 / rate;
    let cos_w = w0.cos();
    let alpha = w0.sin() * 0.5 * 2.0_f64.sqrt();
    let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
    let (b0, b1, b2, a0, a1, a2) = if high_shelf {
        (
            a * ((a + 1.0) + (a - 1.0) * cos_w + two_sqrt_a_alpha),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w),
            a * ((a + 1.0) + (a - 1.0) * cos_w - two_sqrt_a_alpha),
            (a + 1.0) - (a - 1.0) * cos_w + two_sqrt_a_alpha,
            2.0 * ((a - 1.0) - (a + 1.0) * cos_w),
            (a + 1.0) - (a - 1.0) * cos_w - two_sqrt_a_alpha,
        )
    } else {
        (
            a * ((a + 1.0) - (a - 1.0) * cos_w + two_sqrt_a_alpha),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w),
            a * ((a + 1.0) - (a - 1.0) * cos_w - two_sqrt_a_alpha),
            (a + 1.0) + (a - 1.0) * cos_w + two_sqrt_a_alpha,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos_w),
            (a + 1.0) + (a - 1.0) * cos_w - two_sqrt_a_alpha,
        )
    };
    let (b0, b1, b2, a1, a2) = (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for s in buf.iter_mut() {
        let x0 = f64::from(*s);
        let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
        *s = y0 as f32;
    }
}

fn apply_plugin_slot(buf: &mut [f32], rate: u32, slot: &PluginSlotConfig) {
    let fs = f64::from(rate.max(1));
    let amount = slot.amount.clamp(0.0, 1.0);
    match slot.plugin_id {
        PluginId::None => {}
        PluginId::GentleEq => {
            let low = slot.eq_low_db.clamp(-6.0, 6.0);
            let high = slot.eq_high_db.clamp(-6.0, 6.0);
            apply_biquad_shelf(buf, fs, 160.0, low, false);
            apply_biquad_shelf(buf, fs, 4500.0, high, true);
        }
        PluginId::SoftDynamics => {
            // Smooth RMS feed-forward soft compressor (ratio ~2:1 to 3.5:1)
            let thresh = 10.0_f64.powf((-20.0 + (1.0 - amount) * 8.0) / 20.0);
            let ratio = 1.5 + 2.0 * amount;
            let atk = (-1.0 / (0.012 * fs)).exp();
            let rel = (-1.0 / (0.120 * fs)).exp();
            let mut env = 0.0_f64;
            let makeup = 1.0 + 0.18 * amount;
            for s in buf.iter_mut() {
                let x = f64::from(*s).abs();
                env = if x > env {
                    atk * env + (1.0 - atk) * x
                } else {
                    rel * env + (1.0 - rel) * x
                };
                let g = if env > thresh {
                    let over = env / thresh;
                    over.powf(1.0 / ratio - 1.0)
                } else {
                    1.0
                };
                *s = ((f64::from(*s) * g * makeup).clamp(-0.98, 0.98)) as f32;
            }
        }
        PluginId::NoiseControl => {
            // Smooth downward expander below noise threshold to gently clean quiet pauses
            let thresh = 10.0_f64.powf((-54.0 + amount * 14.0) / 20.0);
            let floor_gain = (1.0 - 0.85 * amount).clamp(0.1, 1.0);
            let atk = (-1.0 / (0.005 * fs)).exp();
            let rel = (-1.0 / (0.080 * fs)).exp();
            let mut env = 0.0_f64;
            for s in buf.iter_mut() {
                let x = f64::from(*s).abs();
                env = if x > env {
                    atk * env + (1.0 - atk) * x
                } else {
                    rel * env + (1.0 - rel) * x
                };
                let g = if env >= thresh {
                    1.0
                } else {
                    let t = (env / thresh.max(1e-9)).clamp(0.0, 1.0);
                    floor_gain + (1.0 - floor_gain) * t * t
                };
                *s = (f64::from(*s) * g) as f32;
            }
        }
        PluginId::PhaseLimiter => {
            // Lookahead transparent true-peak limiter (inspired by phaselimiter-style clean limiting)
            let ceiling = 10.0_f64.powf(slot.ceiling_dbfs.clamp(-6.0, -0.1) / 20.0);
            let drive = 10.0_f64.powf((3.5 * amount) / 20.0);
            let lookahead = ((0.003 * fs).round() as usize).clamp(16, 256);
            let n = buf.len();
            if n == 0 {
                return;
            }
            let driven: Vec<f64> = buf.iter().map(|&s| f64::from(s) * drive).collect();
            let mut gain_curve = vec![1.0_f64; n];
            for i in 0..n {
                let end = (i + lookahead).min(n);
                let mut local_peak = 0.0_f64;
                for &v in &driven[i..end] {
                    local_peak = local_peak.max(v.abs());
                }
                if local_peak > ceiling {
                    gain_curve[i] = ceiling / local_peak;
                }
            }
            // Smooth gain release so limiting stays click-free
            let rel = (-1.0 / (0.060 * fs)).exp();
            let mut g = 1.0_f64;
            for i in 0..n {
                if gain_curve[i] < g {
                    g = gain_curve[i];
                } else {
                    g = rel * g + (1.0 - rel) * gain_curve[i];
                }
                buf[i] = ((driven[i] * g).clamp(-ceiling, ceiling)) as f32;
            }
        }
    }
}

// ======================= PLUGIN-READY UI-FREE AUDIO ENGINE =======================

/// Serializable parameter/state chunk for DAW host preset save/recall
/// (VST3 `getState`/`setState`, Audio Unit `ClassInfo`, AAX chunk).
/// Contains zero UI or layout fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineStateSnapshot {
    pub version: u32,
    pub voicing: VoicingControls,
    pub post_dry: PostDryPipelineConfig,
}

impl Default for EngineStateSnapshot {
    fn default() -> Self {
        Self {
            version: 1,
            voicing: VoicingControls::default(),
            post_dry: PostDryPipelineConfig::default(),
        }
    }
}

/// UI-free, host-agnostic SpectraCraft audio processor encapsulating the authoritative
/// five-stage pipeline (`SPECTRACRAFT_SPEC.md`):
///
/// 1. Stage 1: Dry Regeneration (`regenerate_dry_from_reference` / `resynthesize_dry_from_cues`)
/// 2. Stage 2: Surgical Region Editing on the dry take only (`apply_dry_region_edit`)
/// 3. Stage 3: Optional Microphone (`post_dry.mic_enabled`, OFF by default)
/// 4. Stage 4: Optional Space / Room (`post_dry.room_enabled`, OFF by default)
/// 5. Stage 5: Optional Plugin Slot (`post_dry.plugin_slot`, OFF by default)
///
/// Designed so future VST3, AU, or AAX wrappers can drive the exact same processing path
/// via clean buffer-in / buffer-out entry points without pulling in `egui`, `eframe`,
/// file dialogs, or audio-device crates. Original reference samples are never stored or blended.
pub struct AudioPipelineEngine {
    sample_rate: u32,
    sources: Arc<[SourceEntry]>,
    mics: Arc<[MicEntry]>,
    surfaces: Arc<[SurfaceEntry]>,
    voicing: VoicingControls,
    post_dry: PostDryPipelineConfig,
    cues: Option<Performance>,
    dry_buffer: Option<Vec<f32>>,
}

impl AudioPipelineEngine {
    /// Create a new engine prepared for `sample_rate` (8–96 kHz) using the built-in catalogs.
    pub fn new(sample_rate: u32) -> Result<Self> {
        Self::with_catalogs(
            sample_rate,
            Arc::from(source_catalog()),
            Arc::from(mic_catalog()),
            Arc::from(surface_catalog()),
        )
    }

    /// Create an engine sharing pre-loaded immutable catalogs (zero per-instance catalog rebuild).
    pub fn with_catalogs(
        sample_rate: u32,
        sources: Arc<[SourceEntry]>,
        mics: Arc<[MicEntry]>,
        surfaces: Arc<[SurfaceEntry]>,
    ) -> Result<Self> {
        if !crate::valid_rate(sample_rate) {
            return Err(format!("Unsupported sample rate {sample_rate} Hz (expected 8000..=96000)").into());
        }
        if sources.is_empty() || mics.is_empty() || surfaces.is_empty() {
            return Err("AudioPipelineEngine requires non-empty source, mic, and surface catalogs".into());
        }
        Ok(Self {
            sample_rate,
            sources,
            mics,
            surfaces,
            voicing: VoicingControls::default(),
            post_dry: PostDryPipelineConfig::default(),
            cues: None,
            dry_buffer: None,
        })
    }

    /// Reconfigure sample rate (mirrors VST3 `setupProcessing` / AU `Initialize`).
    /// Invalidates cached Stage 1 cues and dry buffer when the rate changes.
    pub fn prepare(&mut self, sample_rate: u32) -> Result<()> {
        if !crate::valid_rate(sample_rate) {
            return Err(format!("Unsupported sample rate {sample_rate} Hz (expected 8000..=96000)").into());
        }
        if self.sample_rate != sample_rate {
            self.sample_rate = sample_rate;
            self.cues = None;
            self.dry_buffer = None;
        }
        Ok(())
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn voicing(&self) -> &VoicingControls {
        &self.voicing
    }

    pub fn set_voicing(&mut self, voicing: VoicingControls) {
        self.voicing = voicing;
    }

    pub fn post_dry_config(&self) -> &PostDryPipelineConfig {
        &self.post_dry
    }

    pub fn post_dry_config_mut(&mut self) -> &mut PostDryPipelineConfig {
        &mut self.post_dry
    }

    pub fn set_post_dry_config(&mut self, mut cfg: PostDryPipelineConfig) {
        cfg.rig.clamp_all();
        self.post_dry = cfg;
    }

    pub fn cues(&self) -> Option<&Performance> {
        self.cues.as_ref()
    }

    pub fn dry_buffer(&self) -> Option<&[f32]> {
        self.dry_buffer.as_deref()
    }

    fn active_source(&self) -> &SourceEntry {
        let idx = self
            .post_dry
            .rig
            .source_index
            .min(self.sources.len().saturating_sub(1));
        &self.sources[idx]
    }

    /// Stage 1: Analyze `reference_mono` into performance cues and synthesize a fresh dry take.
    /// `reference_mono` is used strictly as shape reference and is never stored or blended.
    pub fn regenerate_dry_from_reference(&mut self, reference_mono: &[f32]) -> Result<&[f32]> {
        if reference_mono.is_empty() {
            return Err("Reference buffer must not be empty".into());
        }
        if let Some((idx, &bad)) = reference_mono.iter().enumerate().find(|(_, s)| !s.is_finite()) {
            return Err(format!("Reference buffer contains non-finite sample ({bad}) at index {idx}").into());
        }
        let cues = reperform::analyze_guitar(reference_mono, self.sample_rate)?;
        let dry = cues.synthesize_dry_for_source_with_voicing(self.active_source(), &self.voicing)?;
        self.cues = Some(cues);
        self.dry_buffer = Some(dry);
        Ok(self.dry_buffer.as_deref().unwrap_or(&[]))
    }

    /// Stage 1 re-synthesis from already-extracted cues (e.g., after changing Source or Voicing).
    pub fn resynthesize_dry_from_cues(&mut self) -> Result<&[f32]> {
        let cues = self
            .cues
            .as_ref()
            .ok_or("No extracted performance cues available; run Stage 1 first")?;
        let dry = cues.synthesize_dry_for_source_with_voicing(self.active_source(), &self.voicing)?;
        self.dry_buffer = Some(dry);
        Ok(self.dry_buffer.as_deref().unwrap_or(&[]))
    }

    /// Stage 2: Apply a surgical region edit strictly to the cached dry take.
    /// Samples outside `[edit.start_s, edit.end_s]` remain bit-identical.
    pub fn apply_dry_region_edit(&mut self, edit: &DryRegionEdit) -> Result<RegionEditOutput> {
        let cues = self
            .cues
            .as_ref()
            .ok_or("No extracted performance cues available; run Stage 1 first")?;
        let dry = self
            .dry_buffer
            .as_deref()
            .ok_or("No cached dry take available; run Stage 1 first")?;
        let source = self.active_source();
        let output = region_edit::apply_dry_region_edit(dry, cues, source, &self.voicing, edit)?;
        self.dry_buffer = Some(output.edited_dry.clone());
        Ok(output)
    }

    /// Stages 3–5 buffer-in / buffer-out entry point.
    ///
    /// Reads mono dry audio from `dry_in`, executes only enabled post-dry stages
    /// (Stage 3 Mic -> Stage 4 Room -> Stage 5 Plugin Slot), and writes to `out_buf`.
    /// When all post-dry stages are bypassed, `out_buf` is bit-identical to `dry_in`.
    pub fn process_post_dry_buffer(
        &self,
        dry_in: &[f32],
        out_buf: &mut [f32],
    ) -> Result<PipelineStageSummary> {
        if dry_in.len() != out_buf.len() {
            return Err(format!(
                "Buffer length mismatch: dry_in has {} samples, out_buf has {} samples",
                dry_in.len(),
                out_buf.len()
            )
            .into());
        }
        if let Some((idx, &bad)) = dry_in.iter().enumerate().find(|(_, s)| !s.is_finite()) {
            return Err(format!("Dry input buffer contains non-finite sample ({bad}) at index {idx}").into());
        }
        let mut cfg = self.post_dry.clone();
        cfg.rig.clamp_all();
        let rendered = run_post_dry_pipeline(
            dry_in,
            self.sample_rate,
            &cfg,
            &self.sources,
            &self.mics,
            &self.surfaces,
        );
        if rendered.samples.len() != out_buf.len() {
            return Err("Post-dry pipeline output length differed from input length".into());
        }
        out_buf.copy_from_slice(&rendered.samples);
        Ok(rendered.summary)
    }

    /// Render Stages 3–5 from the engine's cached dry take into caller-supplied `out_buf`.
    pub fn process_cached_dry_into(&self, out_buf: &mut [f32]) -> Result<PipelineStageSummary> {
        let dry = self
            .dry_buffer
            .as_deref()
            .ok_or("No cached dry take available; run Stage 1 first")?;
        self.process_post_dry_buffer(dry, out_buf)
    }

    /// Execute the full five-stage pipeline (`Dry -> Surgical Edits -> Mic -> Room -> Plugin`)
    /// from `reference_mono` into `out_buf` in strict stage order.
    /// Original reference samples are never blended into `out_buf`.
    pub fn process_full_pipeline_buffer(
        &mut self,
        reference_mono: &[f32],
        region_edits: &[DryRegionEdit],
        out_buf: &mut [f32],
    ) -> Result<PipelineStageSummary> {
        if reference_mono.len() != out_buf.len() {
            return Err(format!(
                "Buffer length mismatch: reference_mono has {} samples, out_buf has {} samples",
                reference_mono.len(),
                out_buf.len()
            )
            .into());
        }
        self.regenerate_dry_from_reference(reference_mono)?;
        for edit in region_edits {
            self.apply_dry_region_edit(edit)?;
        }
        self.process_cached_dry_into(out_buf)
    }

    /// Capture serializable plugin parameter state (no UI layout fields).
    pub fn snapshot_state(&self) -> EngineStateSnapshot {
        let mut post_dry = self.post_dry.clone();
        post_dry.rig.clamp_all();
        EngineStateSnapshot {
            version: 1,
            voicing: self.voicing.clone(),
            post_dry,
        }
    }

    /// Restore validated plugin parameter state from a snapshot.
    pub fn apply_state(&mut self, mut state: EngineStateSnapshot) -> Result<()> {
        if state.version != 1 {
            return Err(format!("Unsupported engine state version {}", state.version).into());
        }
        state.post_dry.rig.clamp_all();
        state.post_dry.rig.source_index = state
            .post_dry
            .rig
            .source_index
            .min(self.sources.len().saturating_sub(1));
        self.voicing = state.voicing;
        self.post_dry = state.post_dry;
        Ok(())
    }

    pub fn export_state_json(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&self.snapshot_state())?)
    }

    pub fn import_state_json(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("Engine state chunk exceeds 2 MiB limit".into());
        }
        let state: EngineStateSnapshot = serde_json::from_slice(bytes)?;
        self.apply_state(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio_catalog::{mic_catalog, source_catalog, surface_catalog};

    #[test]
    fn post_dry_pipeline_stages_are_off_by_default_and_independently_bypassable() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        let rate = 48_000_u32;
        let dry: Vec<f32> = (0..4800)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                (0.45 * (TAU * 330.0 * t).sin() + 0.15 * (TAU * 990.0 * t).sin()) as f32
            })
            .collect();

        // 1. Default config (Mic OFF, Room OFF, Plugin OFF) -> BIT-IDENTICAL to dry take!
        let def_cfg = PostDryPipelineConfig::default();
        assert!(!def_cfg.mic_enabled);
        assert!(!def_cfg.room_enabled);
        assert!(!def_cfg.plugin_slot.enabled);
        let out_def = run_post_dry_pipeline(&dry, rate, &def_cfg, &sources, &mics, &surfaces);
        assert!(out_def.summary.bit_identical_to_dry);
        assert_eq!(out_def.samples, dry);

        // 2. Stage 3 ONLY (Mic ON, Room OFF) alters signal
        let mut mic_only = def_cfg.clone();
        mic_only.mic_enabled = true;
        let out_mic = run_post_dry_pipeline(&dry, rate, &mic_only, &sources, &mics, &surfaces);
        assert!(out_mic.summary.mic_stage_active);
        assert!(!out_mic.summary.room_stage_active);
        assert_ne!(out_mic.samples, dry);

        // 3. Stage 4 ONLY (Mic OFF, Room ON: 10x10x10 padded room) alters signal independently
        let mut room_only = def_cfg.clone();
        room_only.room_enabled = true;
        let out_room = run_post_dry_pipeline(&dry, rate, &room_only, &sources, &mics, &surfaces);
        assert!(!out_room.summary.mic_stage_active);
        assert!(out_room.summary.room_stage_active);
        assert_ne!(out_room.samples, dry);
        assert_ne!(out_room.samples, out_mic.samples);

        // 4. Stage 5 ONLY (PhaseLimiter ON, Mic/Room OFF) respects ceiling
        let mut plug_only = def_cfg.clone();
        plug_only.plugin_slot.enabled = true;
        plug_only.plugin_slot.plugin_id = PluginId::PhaseLimiter;
        plug_only.plugin_slot.amount = 0.8;
        plug_only.plugin_slot.ceiling_dbfs = -3.0;
        let out_plug = run_post_dry_pipeline(&dry, rate, &plug_only, &sources, &mics, &surfaces);
        assert!(out_plug.summary.plugin_slot_active);
        let peak = out_plug.samples.iter().fold(0.0_f32, |m, &v| m.max(v.abs()));
        let ceiling_lin = 10.0_f32.powf(-3.0 / 20.0);
        assert!(peak <= ceiling_lin + 1e-4, "limiter peak {peak} exceeded ceiling {ceiling_lin}");

        // 5. Verify craft-atomic crash-safe WAV write produces readable WAV with no temp residue
        let tmp = std::env::temp_dir().join(format!("spectracraft-atomic-test-{}.wav", std::process::id()));
        write_wav_f32_atomic(&tmp, rate, &out_plug.samples).expect("atomic wav write");
        let reader = hound::WavReader::open(&tmp).expect("read back atomic wav");
        assert_eq!(reader.len() as usize, dry.len());
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn mic_bypass_with_room_enabled_has_zero_mic_eq_coloration() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        let rate = 48_000_u32;
        let dry: Vec<f32> = (0..4800)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                (0.35 * (TAU * 40.0 * t).sin() + 0.35 * (TAU * 9000.0 * t).sin()) as f32
            })
            .collect();

        // Compare two different colored mic catalog entries with Stage 3 bypassed and Stage 4 enabled:
        // both MUST produce bit-identical flat omni room-only output regardless of catalog_index.
        let mut cfg_a = PostDryPipelineConfig::default();
        cfg_a.mic_enabled = false;
        cfg_a.room_enabled = true;
        cfg_a.rig.mics[0].catalog_index = 0; // SM7B dynamic
        let out_a = run_post_dry_pipeline(&dry, rate, &cfg_a, &sources, &mics, &surfaces);

        let mut cfg_b = cfg_a.clone();
        cfg_b.rig.mics[0].catalog_index = mics.len().saturating_sub(1);
        let out_b = run_post_dry_pipeline(&dry, rate, &cfg_b, &sources, &mics, &surfaces);

        assert_eq!(
            out_a.samples, out_b.samples,
            "room-only render with bypassed mic must not depend on mic catalog EQ"
        );

        // Also match direct 6-band omni room rendering with zero mic EQ applied.
        let mut omni_rig = cfg_a.rig.clone();
        omni_rig.mics[0].pattern_override = Some(Pattern::Omni);
        let expected = capture::render_studio_rig_with_mic_eq(
            &dry, rate, &omni_rig, &sources, &mics, &surfaces, false,
        );
        assert_eq!(out_a.samples, expected);
    }

    #[test]
    fn gentle_eq_at_zero_db_is_bit_identical_pass_through() {
        let sources = source_catalog();
        let mics = mic_catalog();
        let surfaces = surface_catalog();
        let rate = 48_000_u32;
        let dry: Vec<f32> = (0..2400)
            .map(|i| (0.4 * (TAU * 440.0 * i as f64 / f64::from(rate)).sin()) as f32)
            .collect();

        let mut cfg = PostDryPipelineConfig::default();
        cfg.plugin_slot.enabled = true;
        cfg.plugin_slot.plugin_id = PluginId::GentleEq;
        cfg.plugin_slot.eq_low_db = 0.0;
        cfg.plugin_slot.eq_high_db = 0.0;
        cfg.plugin_slot.amount = 1.0;

        let out = run_post_dry_pipeline(&dry, rate, &cfg, &sources, &mics, &surfaces);
        assert!(
            out.summary.bit_identical_to_dry,
            "GentleEq at 0.0 / 0.0 dB must not apply a hidden boost"
        );
        assert_eq!(out.samples, dry);
    }

    #[test]
    fn write_wav_f32_atomic_rejects_non_finite_samples() {
        let tmp = std::env::temp_dir().join(format!("spectracraft-nan-test-{}.wav", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        assert!(write_wav_f32_atomic(&tmp, 48_000, &[0.0, f32::NAN, 0.1]).is_err());
        assert!(write_wav_f32_atomic(&tmp, 48_000, &[0.0, f32::INFINITY, 0.1]).is_err());
        assert!(!tmp.exists(), "failed non-finite WAV write must not leave a file behind");
    }

    #[test]
    fn audio_pipeline_engine_buffer_in_out_preserves_order_and_bypass_identity() {
        let rate = 48_000_u32;
        let dry: Vec<f32> = (0..4800)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                (0.4 * (TAU * 330.0 * t).sin() + 0.12 * (TAU * 660.0 * t).sin()) as f32
            })
            .collect();

        let mut engine = AudioPipelineEngine::new(rate).expect("engine init");
        assert_eq!(engine.sample_rate(), rate);

        // Default state: all post-dry stages bypassed -> bit-identical pass-through
        let mut out_bypass = vec![0.0_f32; dry.len()];
        let summary_bypass = engine
            .process_post_dry_buffer(&dry, &mut out_bypass)
            .expect("bypass render");
        assert!(summary_bypass.bit_identical_to_dry);
        assert!(!summary_bypass.original_audio_opened);
        assert_eq!(out_bypass, dry);

        // Enable Stage 3 Mic only -> alters signal cleanly without Room or Plugin
        engine.post_dry_config_mut().mic_enabled = true;
        let mut out_mic = vec![0.0_f32; dry.len()];
        let summary_mic = engine
            .process_post_dry_buffer(&dry, &mut out_mic)
            .expect("mic render");
        assert!(summary_mic.mic_stage_active);
        assert!(!summary_mic.room_stage_active);
        assert!(!summary_mic.plugin_slot_active);
        assert_ne!(out_mic, dry);

        // State chunk round-trip (DAW preset save/recall)
        let chunk = engine.export_state_json().expect("export state");
        let mut restored = AudioPipelineEngine::new(rate).expect("restored engine");
        restored.import_state_json(&chunk).expect("import state");
        assert_eq!(restored.snapshot_state(), engine.snapshot_state());

        let mut out_restored = vec![0.0_f32; dry.len()];
        restored
            .process_post_dry_buffer(&dry, &mut out_restored)
            .expect("restored render");
        assert_eq!(out_restored, out_mic);
    }

    #[test]
    fn audio_pipeline_engine_rejects_mismatched_buffers_and_non_finite_input() {
        let mut engine = AudioPipelineEngine::new(48_000).expect("engine init");
        let mut short_out = vec![0.0_f32; 4];
        assert!(engine.process_post_dry_buffer(&[0.0; 8], &mut short_out).is_err());
        assert!(engine
            .process_post_dry_buffer(&[0.0, f32::NAN, 0.1, 0.2], &mut short_out)
            .is_err());
        assert!(engine.prepare(200).is_err());
    }
}
