//! Framework-neutral SpectraCraft desktop session model and artifact provenance boundary.
//!
//! Shared by both the existing `egui` desktop (`spectracraft-desktop`) and the experimental
//! native GPUI frontend (`apps/spectracraft-gpui`). Contains zero UI framework types.
//!
//! Enforces the canonical SpectraCraft invariants:
//! 1. Single-source five-stage pipeline: Dry -> Surgical (dry only) -> Mic -> Room -> Plugin.
//! 2. Source-free synthesis: Stages 1–5 never open or blend original PCM; stage bypass never
//!    substitutes original PCM.
//! 3. Immutable `Arc`-backed audio artifacts carrying sample rate, channel count, frame count,
//!    origin stage, SHA-256 digest, parent artifact ID, and effective config digest.
//! 4. Stale-completion rejection: async job results are validated against session ID, epoch,
//!    request sequence, expected parent artifact ID, and effective config before publication.
//! 5. Revision-bound dry listening review (`SC-GOV-04`): approval is bound to the exact dry
//!    SHA-256 and never carries over across source swaps, dry regenerations, region edits, or
//!    settings-only preset loads.
//! 6. Display-only state is strictly isolated from audio buffers and transport playback clocks.

use crate::capture::DryReviewStatus;
use crate::digest;
use crate::pipeline::{
    self, AtomicWavReceipt, PipelineRenderOutput, PipelineStageSummary, PluginSlotConfig,
    PostDryPipelineConfig,
};
use crate::region_edit::{DryRegionEdit, RegionEditOutput, RegionFixAction};
use crate::reperform::{Performance, VoicingControls};
use crate::studio_catalog::{
    mic_catalog, source_catalog, surface_catalog, MicEntry, RoomPreset, SourceEntry, StudioRig,
    SurfaceEntry, Unit,
};
use crate::visualization::FrequencyScale;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type SessionId = u64;
pub type SessionEpoch = u64;
pub type ArtifactId = u64;

/// Maximum WAV duration permitted for interactive desktop import (matches `desktop.rs`).
pub const MAX_IMPORT_SECONDS: u32 = 600;
/// Maximum excerpt duration processed by Stage 1 dry regeneration (matches `lib.rs` / `desktop.rs`).
pub const MAX_REGEN_SECONDS: f64 = 30.0;
const MAX_HISTORY_DEPTH: usize = 64;
const MAX_HISTORY_UNIQUE_DRY_BUFFERS: usize = 8;
const MAX_HISTORY_AUDIO_BYTES: usize = 128 * 1024 * 1024;

/// Compute deterministic SHA-256 hex digest of an `f32` PCM slice (little-endian IEEE 754).
pub fn sha256_f32_slice(samples: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(samples.len().saturating_mul(4));
    for &s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    digest::sha256(&bytes)
}

/// Compute a deterministic digest for any serializable configuration value.
pub fn config_digest<T: Serialize>(value: &T) -> String {
    match serde_json::to_vec(value) {
        Ok(bytes) => digest::sha256(&bytes),
        Err(_) => "unserializable-config".to_owned(),
    }
}

/// Origin stage for an immutable audio artifact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioOrigin {
    OriginalSource,
    Stage1Dry,
    Stage2SurgicalDry,
    PostDryCapture,
}

impl AudioOrigin {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OriginalSource => "Original Reference",
            Self::Stage1Dry => "Stage 1 Dry",
            Self::Stage2SurgicalDry => "Stage 2 Surgical Dry",
            Self::PostDryCapture => "Post-Dry Capture (Stages 3-5)",
        }
    }
}

/// Immutable `Arc`-backed audio buffer with explicit provenance and content identity.
#[derive(Clone, Debug)]
pub struct AudioArtifact {
    pub id: ArtifactId,
    pub origin: AudioOrigin,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: usize,
    pub sha256: String,
    pub parent_id: Option<ArtifactId>,
    pub config_digest: String,
    pub samples: Arc<[f32]>,
}

impl AudioArtifact {
    /// Construct a validated mono `AudioArtifact`.
    pub fn new_mono(
        id: ArtifactId,
        origin: AudioOrigin,
        sample_rate: u32,
        samples: Vec<f32>,
        parent_id: Option<ArtifactId>,
        config_digest: String,
    ) -> Result<Self, String> {
        if !(8_000..=96_000).contains(&sample_rate) {
            return Err(format!(
                "Unsupported sample rate {sample_rate} Hz (expected 8,000-96,000 Hz)"
            ));
        }
        if samples.is_empty() {
            return Err("Audio artifact cannot be empty".to_owned());
        }
        if samples.iter().any(|v| !v.is_finite()) {
            return Err("Audio artifact contains non-finite samples".to_owned());
        }
        let frames = samples.len();
        let sha256 = sha256_f32_slice(&samples);
        Ok(Self {
            id,
            origin,
            sample_rate,
            channels: 1,
            frames,
            sha256,
            parent_id,
            config_digest,
            samples: Arc::from(samples),
        })
    }

    pub fn duration_s(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate.max(1))
    }
}

/// Upper byte bound for a single imported WAV file (600 s * 96 kHz * stereo * f32 = ~461 MiB + header headroom).
const MAX_IMPORT_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// Load and validate a local WAV file into an immutable `AudioArtifact` without modifying disk.
/// Supports mono or stereo (downmixed to mono with equal 0.5 weights) at 8-96 kHz up to 600 s.
pub fn load_validated_wav_artifact(
    path: &Path,
    artifact_id: ArtifactId,
) -> Result<(AudioArtifact, String), String> {
    if path.to_string_lossy().chars().any(|c| c.is_control()) {
        return Err("Invalid WAV path containing control characters".to_owned());
    }
    let meta =
        std::fs::metadata(path).map_err(|e| format!("Failed to inspect WAV file metadata: {e}"))?;
    if !meta.is_file() {
        return Err("WAV path must refer to a regular file".to_owned());
    }
    if meta.len() < 44 || meta.len() > MAX_IMPORT_FILE_BYTES {
        return Err("WAV file size is outside supported bounds (44 bytes to 512 MiB)".to_owned());
    }

    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("Failed to open WAV: {e}"))?;
    let spec = reader.spec();
    let initial_cap = crate::validate_wav_spec(spec, reader.duration(), MAX_IMPORT_SECONDS, true)
        .map_err(|e| format!("{e}"))?;
    let max_frames = spec.sample_rate as usize * MAX_IMPORT_SECONDS as usize;
    let channels = spec.channels as usize;
    let mut mono = Vec::with_capacity(initial_cap.min(max_frames));

    match spec.sample_format {
        hound::SampleFormat::Float => {
            let mut iter = reader.samples::<f32>();
            if channels == 1 {
                for s in iter {
                    if mono.len() >= max_frames {
                        return Err(format!(
                            "WAV exceeds maximum duration of {MAX_IMPORT_SECONDS} seconds"
                        ));
                    }
                    mono.push(s.map_err(|e| format!("WAV decode error: {e}"))?);
                }
            } else {
                while let Some(l) = iter.next() {
                    if mono.len() >= max_frames {
                        return Err(format!(
                            "WAV exceeds maximum duration of {MAX_IMPORT_SECONDS} seconds"
                        ));
                    }
                    let l = l.map_err(|e| format!("WAV decode error: {e}"))?;
                    let r = iter
                        .next()
                        .transpose()
                        .map_err(|e| format!("WAV decode error: {e}"))?
                        .unwrap_or(l);
                    mono.push(0.5 * (l + r));
                }
            }
        }
        hound::SampleFormat::Int => {
            let scale = 2_f32.powi(i32::from(spec.bits_per_sample) - 1);
            let mut iter = reader.samples::<i32>();
            if channels == 1 {
                for s in iter {
                    if mono.len() >= max_frames {
                        return Err(format!(
                            "WAV exceeds maximum duration of {MAX_IMPORT_SECONDS} seconds"
                        ));
                    }
                    let v = s.map_err(|e| format!("WAV decode error: {e}"))?;
                    mono.push(v as f32 / scale);
                }
            } else {
                while let Some(l) = iter.next() {
                    if mono.len() >= max_frames {
                        return Err(format!(
                            "WAV exceeds maximum duration of {MAX_IMPORT_SECONDS} seconds"
                        ));
                    }
                    let lv = l.map_err(|e| format!("WAV decode error: {e}"))? as f32 / scale;
                    let rv = iter
                        .next()
                        .transpose()
                        .map_err(|e| format!("WAV decode error: {e}"))?
                        .map(|v| v as f32 / scale)
                        .unwrap_or(lv);
                    mono.push(0.5 * (lv + rv));
                }
            }
        }
    }

    if mono.is_empty() || mono.iter().any(|v| !v.is_finite() || v.abs() > 4.0) {
        return Err("Invalid or out-of-range WAV samples".to_owned());
    }

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("source.wav")
        .to_owned();
    let cfg_hash = digest::sha256(
        format!("import:{}:{}ch:{}hz", name, spec.channels, spec.sample_rate).as_bytes(),
    );
    let artifact = AudioArtifact::new_mono(
        artifact_id,
        AudioOrigin::OriginalSource,
        spec.sample_rate,
        mono,
        None,
        cfg_hash,
    )?;
    Ok((artifact, name))
}

/// Five-criterion human listening checklist for Stage 1/2 dry review (`SC-GOV-04`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryReviewChecklist {
    pub rhythm: bool,
    pub phrasing: bool,
    pub dynamics: bool,
    pub sustain: bool,
    pub artifact_free: bool,
}

impl DryReviewChecklist {
    pub fn passed_count(&self) -> usize {
        [
            self.rhythm,
            self.phrasing,
            self.dynamics,
            self.sustain,
            self.artifact_free,
        ]
        .into_iter()
        .filter(|&v| v)
        .count()
    }

    pub fn all_passed(&self) -> bool {
        self.passed_count() == 5
    }
}

/// Revision-bound dry listening review record (`SC-GOV-04`).
/// Approval is valid ONLY when `approved_dry_sha256` matches the active dry artifact's SHA-256.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryReviewRecord {
    pub checklist: DryReviewChecklist,
    pub approved_dry_sha256: Option<String>,
}

impl DryReviewRecord {
    pub fn status_for(&self, current_dry_sha256: Option<&str>) -> DryReviewStatus {
        match (current_dry_sha256, self.approved_dry_sha256.as_deref()) {
            (Some(cur), Some(approved)) if cur == approved && self.checklist.all_passed() => {
                DryReviewStatus::ApprovedByHuman
            }
            _ => DryReviewStatus::NotReviewed,
        }
    }

    pub fn is_candidate_ready(&self, current_dry_sha256: Option<&str>) -> bool {
        self.status_for(current_dry_sha256) == DryReviewStatus::ApprovedByHuman
    }

    pub fn update_checklist(
        &mut self,
        checklist: DryReviewChecklist,
        current_dry_sha256: Option<&str>,
    ) {
        self.checklist = checklist;
        self.approved_dry_sha256 = if checklist.all_passed() {
            current_dry_sha256.map(ToOwned::to_owned)
        } else {
            None
        };
    }

    pub fn clear(&mut self) {
        self.checklist = DryReviewChecklist::default();
        self.approved_dry_sha256 = None;
    }
}

/// Explicit target selection for atomic WAV export.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportTarget {
    Dry,
    CurrentCapture,
}

/// Distinguishes settings-only presets from full sessions with verified audio provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveKind {
    SettingsPreset,
    FullSession,
}

/// Versioned settings/session JSON saved atomically through `report_export::atomic_write_bytes`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionPresetFile {
    pub version: u32,
    pub kind: SaveKind,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub source_sha256: Option<String>,
    #[serde(default)]
    pub dry_sha256: Option<String>,
    #[serde(default)]
    pub approved_dry_sha256: Option<String>,
    pub selected_source_id: String,
    pub voicing: VoicingControls,
    pub mic_enabled: bool,
    #[serde(default)]
    pub preamp_drive: f64,
    pub room_enabled: bool,
    pub rig: StudioRig,
    pub plugin_slot: PluginSlotConfig,
    pub selection: Option<[f64; 2]>,
    pub marquee_hz: Option<[f64; 2]>,
    pub region_edit_draft: DryRegionEdit,
    pub dry_checklist: DryReviewChecklist,
}

/// Receipt returned after an atomic WAV export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportReceiptInfo {
    pub target: ExportTarget,
    pub artifact_id: ArtifactId,
    pub artifact_sha256: String,
    pub wav: AtomicWavReceipt,
    pub path: PathBuf,
}

/// Summary of the most recent Stage 2 surgical region edit.
#[derive(Clone, Debug)]
pub struct RegionEditSummary {
    pub start_sample: usize,
    pub end_sample: usize,
    pub affected_notes: usize,
    pub effective_action: RegionFixAction,
    pub unresolved_note: Option<String>,
    pub outside_bit_identical: bool,
}

/// Guard binding every asynchronous job completion to the exact session/epoch/parent/config
/// that initiated it. Validated at the publication boundary before mutating `AppSession`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionGuard {
    pub session_id: SessionId,
    pub epoch: SessionEpoch,
    pub request_seq: u64,
    pub expected_parent_id: Option<ArtifactId>,
    pub config_digest: String,
}

/// Reason a stale or mismatched async completion was rejected prior to publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaleCompletionError {
    SessionMismatch {
        expected: SessionId,
        got: SessionId,
    },
    EpochMismatch {
        expected: SessionEpoch,
        got: SessionEpoch,
    },
    SupersededRequest {
        latest: u64,
        got: u64,
    },
    ParentMismatch {
        expected: Option<ArtifactId>,
        got: Option<ArtifactId>,
    },
    ConfigChanged {
        expected: String,
        got: String,
    },
    InvalidArtifact(String),
}

impl std::fmt::Display for StaleCompletionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionMismatch { expected, got } => {
                write!(f, "Stale result from session {got} (active {expected})")
            }
            Self::EpochMismatch { expected, got } => {
                write!(f, "Stale result from epoch {got} (active {expected})")
            }
            Self::SupersededRequest { latest, got } => {
                write!(f, "Superseded job sequence {got} (latest {latest})")
            }
            Self::ParentMismatch { expected, got } => {
                write!(
                    f,
                    "Parent artifact changed (expected {expected:?}, got {got:?})"
                )
            }
            Self::ConfigChanged { .. } => {
                write!(f, "Stage configuration changed before completion")
            }
            Self::InvalidArtifact(msg) => write!(f, "Invalid audio result: {msg}"),
        }
    }
}

impl std::error::Error for StaleCompletionError {}

/// Immutable snapshot of all inputs required to execute Stage 1 dry regeneration off-thread.
#[derive(Clone)]
pub struct DryJobSpec {
    pub guard: CompletionGuard,
    pub source_samples: Arc<[f32]>,
    pub sample_rate: u32,
    pub excerpt_span: [usize; 2],
    pub source_entry: SourceEntry,
    pub voicing: VoicingControls,
}

/// Immutable snapshot of all inputs required to execute Stage 2 surgical region editing off-thread.
/// Note: strictly receives the cached dry artifact and performance cues, NEVER original PCM.
#[derive(Clone)]
pub struct RegionEditJobSpec {
    pub guard: CompletionGuard,
    pub dry_samples: Arc<[f32]>,
    pub performance: Arc<Performance>,
    pub source_entry: SourceEntry,
    pub voicing: VoicingControls,
    pub edit: DryRegionEdit,
}

/// Immutable snapshot of all inputs required to execute Stages 3-5 post-dry capture off-thread.
/// Strictly receives the cached dry artifact, NEVER original PCM.
#[derive(Clone)]
pub struct PostDryJobSpec {
    pub guard: CompletionGuard,
    pub dry_samples: Arc<[f32]>,
    pub sample_rate: u32,
    pub config: PostDryPipelineConfig,
    pub sources: Arc<[SourceEntry]>,
    pub mics: Arc<[MicEntry]>,
    pub surfaces: Arc<[SurfaceEntry]>,
}

/// Which buffer is currently selected for display/auditioning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayTake {
    Original,
    Dry,
    Capture,
}

impl DisplayTake {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Dry => "Dry",
            Self::Capture => "Capture",
        }
    }
}

/// Display-only UI state strictly isolated from audio buffers and transport clocks.
/// Changing any field here NEVER alters audio artifacts, `SessionEpoch`, or playback time.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayState {
    pub take: DisplayTake,
    pub view_range: [f64; 2],
    pub display_max_hz: f32,
    pub show_waveform: bool,
    pub show_spectrogram: bool,
    pub unit: Unit,
    pub scale: FrequencyScale,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            take: DisplayTake::Original,
            view_range: [0.0, 1.0],
            display_max_hz: 12_000.0,
            show_waveform: true,
            show_spectrogram: true,
            unit: Unit::Feet,
            scale: FrequencyScale::Linear,
        }
    }
}

/// Copy-on-write reversible snapshot for actual-audio Undo/Redo.
#[derive(Clone)]
pub struct SessionSnapshot {
    pub selected_source_idx: usize,
    pub voicing: VoicingControls,
    pub mic_enabled: bool,
    pub preamp_drive: f64,
    pub room_enabled: bool,
    pub rig: StudioRig,
    pub plugin_slot: PluginSlotConfig,
    pub selection: Option<[f64; 2]>,
    pub marquee_hz: Option<[f64; 2]>,
    pub region_edit_draft: DryRegionEdit,
    pub dry: Option<Arc<AudioArtifact>>,
    pub performance: Option<Arc<Performance>>,
    pub capture: Option<Arc<AudioArtifact>>,
    pub capture_summary: Option<PipelineStageSummary>,
    pub capture_stale: bool,
    pub dry_review: DryReviewRecord,
}

impl PartialEq for SessionSnapshot {
    fn eq(&self, other: &Self) -> bool {
        let dry_same = match (&self.dry, &other.dry) {
            (None, None) => true,
            (Some(a), Some(b)) => a.id == b.id && a.sha256 == b.sha256,
            _ => false,
        };
        let cap_same = match (&self.capture, &other.capture) {
            (None, None) => true,
            (Some(a), Some(b)) => a.id == b.id && a.sha256 == b.sha256,
            _ => false,
        };
        self.selected_source_idx == other.selected_source_idx
            && self.voicing == other.voicing
            && self.mic_enabled == other.mic_enabled
            && (self.preamp_drive - other.preamp_drive).abs() < 1e-9
            && self.room_enabled == other.room_enabled
            && self.rig == other.rig
            && self.plugin_slot == other.plugin_slot
            && self.selection == other.selection
            && self.marquee_hz == other.marquee_hz
            && self.region_edit_draft == other.region_edit_draft
            && dry_same
            && cap_same
            && self.capture_stale == other.capture_stale
            && self.dry_review == other.dry_review
    }
}

/// Bounded copy-on-write Undo/Redo stack capping both unique dry buffers and memory bytes.
pub struct SessionHistory {
    past: Vec<SessionSnapshot>,
    future: Vec<SessionSnapshot>,
    max_depth: usize,
    max_unique_dry: usize,
    max_audio_bytes: usize,
}

impl Default for SessionHistory {
    fn default() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            max_depth: MAX_HISTORY_DEPTH,
            max_unique_dry: MAX_HISTORY_UNIQUE_DRY_BUFFERS,
            max_audio_bytes: MAX_HISTORY_AUDIO_BYTES,
        }
    }
}

impl SessionHistory {
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn past_len(&self) -> usize {
        self.past.len()
    }

    pub fn future_len(&self) -> usize {
        self.future.len()
    }

    fn unique_dry_usage(&self) -> (usize, usize) {
        let mut seen: Vec<(usize, usize)> = Vec::new();
        for snap in self.past.iter().chain(self.future.iter()) {
            if let Some(art) = &snap.dry {
                let ptr = art.samples.as_ptr() as usize;
                let len = art.samples.len();
                if !seen.iter().any(|&(p, l)| p == ptr && l == len) {
                    seen.push((ptr, len));
                }
            }
        }
        let bytes = seen
            .iter()
            .map(|&(_, len)| len.saturating_mul(std::mem::size_of::<f32>()))
            .sum();
        (seen.len(), bytes)
    }

    fn enforce_budget(&mut self) {
        while self.past.len() > self.max_depth {
            self.past.remove(0);
        }
        while self.future.len() > self.max_depth {
            self.future.remove(0);
        }
        loop {
            let (unique_count, total_bytes) = self.unique_dry_usage();
            if unique_count <= self.max_unique_dry && total_bytes <= self.max_audio_bytes {
                break;
            }
            if self.past.len() > 1 {
                self.past.remove(0);
            } else if !self.future.is_empty() {
                self.future.remove(0);
            } else {
                break;
            }
        }
    }

    pub fn push(&mut self, before: SessionSnapshot) {
        if self.past.last() == Some(&before) {
            return;
        }
        self.past.push(before);
        self.future.clear();
        self.enforce_budget();
    }

    pub fn undo(&mut self, current: SessionSnapshot) -> Option<SessionSnapshot> {
        let prev = self.past.pop()?;
        self.future.push(current);
        self.enforce_budget();
        Some(prev)
    }

    pub fn redo(&mut self, current: SessionSnapshot) -> Option<SessionSnapshot> {
        let next = self.future.pop()?;
        self.past.push(current);
        self.enforce_budget();
        Some(next)
    }
}

/// Authoritative framework-neutral SpectraCraft desktop session.
pub struct AppSession {
    session_id: SessionId,
    epoch: SessionEpoch,
    next_artifact_id: ArtifactId,
    next_request_seq: u64,
    latest_dry_seq: u64,
    latest_region_seq: u64,
    latest_post_dry_seq: u64,

    pub sources: Arc<[SourceEntry]>,
    pub mics: Arc<[MicEntry]>,
    pub surfaces: Arc<[SurfaceEntry]>,

    pub source_path: Option<PathBuf>,
    pub source_name: Option<String>,
    pub source: Option<Arc<AudioArtifact>>,
    pub performance: Option<Arc<Performance>>,
    pub dry: Option<Arc<AudioArtifact>>,
    pub capture: Option<Arc<AudioArtifact>>,
    pub capture_summary: Option<PipelineStageSummary>,
    pub capture_stale: bool,

    pub selected_source_idx: usize,
    pub voicing: VoicingControls,
    pub mic_enabled: bool,
    pub preamp_drive: f64,
    pub room_enabled: bool,
    pub rig: StudioRig,
    pub plugin_slot: PluginSlotConfig,
    pub selection: Option<[f64; 2]>,
    pub marquee_hz: Option<[f64; 2]>,
    pub region_edit_draft: DryRegionEdit,
    pub dry_review: DryReviewRecord,
    pub last_region_edit: Option<RegionEditSummary>,

    pub history: SessionHistory,
    pub display: DisplayState,
}

impl AppSession {
    pub fn new(session_id: SessionId) -> Self {
        let sources: Arc<[SourceEntry]> = Arc::from(source_catalog());
        let mics: Arc<[MicEntry]> = Arc::from(mic_catalog());
        let surfaces: Arc<[SurfaceEntry]> = Arc::from(surface_catalog());
        let selected_source_idx = sources
            .iter()
            .position(|s| s.id.contains("guitar"))
            .unwrap_or(0);
        let mut rig = StudioRig::sm7b_foam_reference();
        rig.source_index = selected_source_idx;

        Self {
            session_id,
            epoch: 1,
            next_artifact_id: 1,
            next_request_seq: 1,
            latest_dry_seq: 0,
            latest_region_seq: 0,
            latest_post_dry_seq: 0,
            sources,
            mics,
            surfaces,
            source_path: None,
            source_name: None,
            source: None,
            performance: None,
            dry: None,
            capture: None,
            capture_summary: None,
            capture_stale: false,
            selected_source_idx,
            voicing: VoicingControls::default(),
            mic_enabled: false,
            preamp_drive: 0.0,
            room_enabled: false,
            rig,
            plugin_slot: PluginSlotConfig::default(),
            selection: None,
            marquee_hz: None,
            region_edit_draft: DryRegionEdit {
                id: "region-1".into(),
                kind: "range".into(),
                start_s: 0.0,
                end_s: 0.5,
                min_hz: None,
                max_hz: None,
                note: String::new(),
                action: RegionFixAction::Stabilize,
                strength: 0.55,
                crossfade_ms: 12.0,
            },
            dry_review: DryReviewRecord::default(),
            last_region_edit: None,
            history: SessionHistory::default(),
            display: DisplayState::default(),
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn epoch(&self) -> SessionEpoch {
        self.epoch
    }

    pub fn allocate_artifact_id(&mut self) -> ArtifactId {
        let id = self.next_artifact_id;
        self.next_artifact_id = self.next_artifact_id.wrapping_add(1).max(1);
        id
    }

    fn allocate_request_seq(&mut self) -> u64 {
        let seq = self.next_request_seq;
        self.next_request_seq = self.next_request_seq.wrapping_add(1).max(1);
        seq
    }

    pub fn current_source_entry(&self) -> &SourceEntry {
        &self.sources[self
            .selected_source_idx
            .min(self.sources.len().saturating_sub(1))]
    }

    pub fn post_dry_config(&self) -> PostDryPipelineConfig {
        let mut rig = self.rig.clone();
        rig.source_index = self.selected_source_idx;
        PostDryPipelineConfig {
            mic_enabled: self.mic_enabled,
            preamp_drive: self.preamp_drive.clamp(0.0, 1.0),
            room_enabled: self.room_enabled,
            rig,
            plugin_slot: self.plugin_slot.clone(),
        }
    }

    pub fn post_dry_config_digest(&self) -> String {
        config_digest(&self.post_dry_config())
    }

    pub fn dry_config_digest(&self) -> String {
        #[derive(Serialize)]
        struct DryCfg<'a> {
            source_id: &'a str,
            voicing: &'a VoicingControls,
        }
        config_digest(&DryCfg {
            source_id: &self.current_source_entry().id,
            voicing: &self.voicing,
        })
    }

    pub fn dry_review_status(&self) -> DryReviewStatus {
        self.dry_review
            .status_for(self.dry.as_ref().map(|d| d.sha256.as_str()))
    }

    /// Returns the currently displayed artifact for visualization and auditioning.
    /// Never substitutes Original when Dry or Capture is requested and unavailable.
    pub fn artifact_for_take(&self, take: DisplayTake) -> Option<Arc<AudioArtifact>> {
        match take {
            DisplayTake::Original => self.source.clone(),
            DisplayTake::Dry => self.dry.clone(),
            DisplayTake::Capture => self.capture.clone(),
        }
    }

    pub fn active_display_artifact(&self) -> Option<Arc<AudioArtifact>> {
        self.artifact_for_take(self.display.take)
    }

    /// Load a validated source WAV artifact, bumping `SessionEpoch` so all prior in-flight
    /// jobs from the previous source are rejected at the publication boundary.
    pub fn load_source_artifact(&mut self, path: PathBuf, name: String, artifact: AudioArtifact) {
        self.epoch = self.epoch.wrapping_add(1).max(1);
        let dur = artifact.duration_s().max(0.01);
        self.source_path = Some(path);
        self.source_name = Some(name);
        self.source = Some(Arc::new(artifact));
        self.performance = None;
        self.dry = None;
        self.capture = None;
        self.capture_summary = None;
        self.capture_stale = false;
        self.selection = None;
        self.marquee_hz = None;
        self.last_region_edit = None;
        self.dry_review.clear();
        self.history.clear();
        self.display.take = DisplayTake::Original;
        self.display.view_range = [0.0, dur];
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            selected_source_idx: self.selected_source_idx,
            voicing: self.voicing.clone(),
            mic_enabled: self.mic_enabled,
            preamp_drive: self.preamp_drive,
            room_enabled: self.room_enabled,
            rig: self.rig.clone(),
            plugin_slot: self.plugin_slot.clone(),
            selection: self.selection,
            marquee_hz: self.marquee_hz,
            region_edit_draft: self.region_edit_draft.clone(),
            dry: self.dry.clone(),
            performance: self.performance.clone(),
            capture: self.capture.clone(),
            capture_summary: self.capture_summary.clone(),
            capture_stale: self.capture_stale,
            dry_review: self.dry_review.clone(),
        }
    }

    fn restore_snapshot(&mut self, snap: SessionSnapshot) {
        self.selected_source_idx = snap
            .selected_source_idx
            .min(self.sources.len().saturating_sub(1));
        self.voicing = snap.voicing;
        self.mic_enabled = snap.mic_enabled;
        self.preamp_drive = snap.preamp_drive;
        self.room_enabled = snap.room_enabled;
        self.rig = snap.rig;
        self.plugin_slot = snap.plugin_slot;
        self.selection = snap.selection;
        self.marquee_hz = snap.marquee_hz;
        self.region_edit_draft = snap.region_edit_draft;
        self.dry = snap.dry;
        self.performance = snap.performance;
        self.capture = snap.capture;
        self.capture_summary = snap.capture_summary;
        self.capture_stale = snap.capture_stale;
        self.dry_review = snap.dry_review;
        self.recompute_capture_staleness();
    }

    fn recompute_capture_staleness(&mut self) {
        let Some(cap) = &self.capture else {
            self.capture_stale = false;
            return;
        };
        let dry_matches = self
            .dry
            .as_ref()
            .map(|d| cap.parent_id == Some(d.id))
            .unwrap_or(false);
        let cfg_matches = cap.config_digest == self.post_dry_config_digest();
        self.capture_stale = !(dry_matches && cfg_matches);
    }

    /// Undo to the previous state, restoring actual `Arc<AudioArtifact>` dry audio.
    /// Does not modify the original source or display/transport clocks.
    pub fn undo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(prev) = self.history.undo(current) else {
            return false;
        };
        self.restore_snapshot(prev);
        true
    }

    /// Redo to the next state, restoring actual `Arc<AudioArtifact>` dry audio.
    pub fn redo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(next) = self.history.redo(current) else {
            return false;
        };
        self.restore_snapshot(next);
        true
    }

    // ---------- Stage configuration mutators (push undo snapshot & mark stale capture) ----------

    pub fn set_source_index(&mut self, idx: usize) {
        let clamped = idx.min(self.sources.len().saturating_sub(1));
        if clamped == self.selected_source_idx {
            return;
        }
        self.history.push(self.snapshot());
        self.selected_source_idx = clamped;
        self.rig.source_index = clamped;
        self.recompute_capture_staleness();
    }

    pub fn set_voicing(&mut self, voicing: VoicingControls) {
        if self.voicing == voicing {
            return;
        }
        self.history.push(self.snapshot());
        self.voicing = voicing;
    }

    pub fn set_mic_enabled(&mut self, enabled: bool) {
        if self.mic_enabled == enabled {
            return;
        }
        self.history.push(self.snapshot());
        self.mic_enabled = enabled;
        self.recompute_capture_staleness();
    }

    pub fn set_preamp_drive(&mut self, drive: f64) {
        let clamped = drive.clamp(0.0, 1.0);
        if (self.preamp_drive - clamped).abs() < 1e-6 {
            return;
        }
        self.history.push(self.snapshot());
        self.preamp_drive = clamped;
        self.recompute_capture_staleness();
    }

    pub fn set_room_enabled(&mut self, enabled: bool) {
        if self.room_enabled == enabled {
            return;
        }
        self.history.push(self.snapshot());
        self.room_enabled = enabled;
        self.recompute_capture_staleness();
    }

    pub fn set_room_preset(&mut self, preset: RoomPreset) {
        if self.rig.room_preset == preset {
            return;
        }
        self.history.push(self.snapshot());
        self.rig.apply_preset(preset);
        self.recompute_capture_staleness();
    }

    pub fn set_mic_catalog_index(&mut self, mic_idx: usize) {
        let clamped = mic_idx.min(self.mics.len().saturating_sub(1));
        let slot_idx = self
            .rig
            .selected_mic
            .min(self.rig.mics.len().saturating_sub(1));
        if self.rig.mics.get(slot_idx).map(|m| m.catalog_index) == Some(clamped) {
            return;
        }
        self.history.push(self.snapshot());
        if let Some(slot) = self.rig.mics.get_mut(slot_idx) {
            slot.catalog_index = clamped;
        }
        self.recompute_capture_staleness();
    }

    pub fn set_plugin_slot(&mut self, plugin_slot: PluginSlotConfig) {
        if self.plugin_slot == plugin_slot {
            return;
        }
        self.history.push(self.snapshot());
        self.plugin_slot = plugin_slot;
        self.recompute_capture_staleness();
    }

    pub fn set_selection(&mut self, selection: Option<[f64; 2]>) {
        self.selection = selection.map(|[a, b]| {
            let lo = a.min(b).max(0.0);
            let hi = a.max(b).max(lo);
            [lo, hi]
        });
        if let Some([s0, s1]) = self.selection {
            self.region_edit_draft.start_s = s0;
            self.region_edit_draft.end_s = s1;
        }
    }

    pub fn set_marquee_hz(&mut self, marquee_hz: Option<[f64; 2]>) {
        self.marquee_hz = marquee_hz.map(|[a, b]| [a.min(b).max(0.0), a.max(b).max(1.0)]);
        match self.marquee_hz {
            Some([lo, hi]) => {
                self.region_edit_draft.kind = "marquee".into();
                self.region_edit_draft.min_hz = Some(lo);
                self.region_edit_draft.max_hz = Some(hi);
            }
            None => {
                self.region_edit_draft.kind = "range".into();
                self.region_edit_draft.min_hz = None;
                self.region_edit_draft.max_hz = None;
            }
        }
    }

    pub fn set_region_edit_draft(&mut self, draft: DryRegionEdit) {
        self.region_edit_draft = draft;
    }

    pub fn set_dry_checklist(&mut self, checklist: DryReviewChecklist) {
        let dry_hash = self.dry.as_ref().map(|d| d.sha256.clone());
        self.dry_review
            .update_checklist(checklist, dry_hash.as_deref());
    }

    // ---------- Async job request & validated publication boundary ----------

    /// Prepare a Stage 1 dry regeneration job bound to the active source and session epoch.
    pub fn begin_dry_request(&mut self) -> Result<DryJobSpec, String> {
        let source = self
            .source
            .clone()
            .ok_or_else(|| "Import a WAV file before regenerating Stage 1 dry audio".to_owned())?;
        let seq = self.allocate_request_seq();
        self.latest_dry_seq = seq;

        let max_frames = (f64::from(source.sample_rate) * MAX_REGEN_SECONDS).round() as usize;
        let end = source.frames.min(max_frames.max(1));

        let guard = CompletionGuard {
            session_id: self.session_id,
            epoch: self.epoch,
            request_seq: seq,
            expected_parent_id: Some(source.id),
            config_digest: self.dry_config_digest(),
        };

        Ok(DryJobSpec {
            guard,
            source_samples: source.samples.clone(),
            sample_rate: source.sample_rate,
            excerpt_span: [0, end],
            source_entry: self.current_source_entry().clone(),
            voicing: self.voicing.clone(),
        })
    }

    /// Validate guard and publish Stage 1 dry audio.
    pub fn publish_dry_result(
        &mut self,
        guard: &CompletionGuard,
        performance: Performance,
        samples: Vec<f32>,
    ) -> Result<Arc<AudioArtifact>, StaleCompletionError> {
        if guard.session_id != self.session_id {
            return Err(StaleCompletionError::SessionMismatch {
                expected: self.session_id,
                got: guard.session_id,
            });
        }
        if guard.epoch != self.epoch {
            return Err(StaleCompletionError::EpochMismatch {
                expected: self.epoch,
                got: guard.epoch,
            });
        }
        if guard.request_seq < self.latest_dry_seq {
            return Err(StaleCompletionError::SupersededRequest {
                latest: self.latest_dry_seq,
                got: guard.request_seq,
            });
        }
        let expected_parent = self.source.as_ref().map(|s| s.id);
        if guard.expected_parent_id != expected_parent {
            return Err(StaleCompletionError::ParentMismatch {
                expected: expected_parent,
                got: guard.expected_parent_id,
            });
        }
        let cur_cfg = self.dry_config_digest();
        if guard.config_digest != cur_cfg {
            return Err(StaleCompletionError::ConfigChanged {
                expected: cur_cfg,
                got: guard.config_digest.clone(),
            });
        }

        let id = self.allocate_artifact_id();
        let artifact = AudioArtifact::new_mono(
            id,
            AudioOrigin::Stage1Dry,
            performance.rate(),
            samples,
            expected_parent,
            guard.config_digest.clone(),
        )
        .map_err(StaleCompletionError::InvalidArtifact)?;
        let arc = Arc::new(artifact);

        self.history.push(self.snapshot());
        self.performance = Some(Arc::new(performance));
        self.dry = Some(arc.clone());
        self.last_region_edit = None;
        // Revision-bound approval: new dry revision invalidates prior checklist approval.
        self.dry_review.clear();
        self.recompute_capture_staleness();
        self.display.take = DisplayTake::Dry;
        Ok(arc)
    }

    /// Prepare a Stage 2 surgical region edit job. Strictly requires cached Stage 1/2 dry audio.
    pub fn begin_region_edit_request(&mut self) -> Result<RegionEditJobSpec, String> {
        let dry = self.dry.clone().ok_or_else(|| {
            "Regenerate Stage 1 dry audio before applying Stage 2 edits".to_owned()
        })?;
        let perf = self
            .performance
            .clone()
            .ok_or_else(|| "Performance cues missing for Stage 2 region edit".to_owned())?;
        let mut edit = self.region_edit_draft.clone();
        if let Some([s0, s1]) = self.selection {
            edit.start_s = s0;
            edit.end_s = s1;
        }
        if edit.end_s <= edit.start_s {
            return Err(
                "Select a valid region (end_s > start_s) before applying Stage 2 edit".to_owned(),
            );
        }

        let seq = self.allocate_request_seq();
        self.latest_region_seq = seq;

        let guard = CompletionGuard {
            session_id: self.session_id,
            epoch: self.epoch,
            request_seq: seq,
            expected_parent_id: Some(dry.id),
            config_digest: config_digest(&edit),
        };

        Ok(RegionEditJobSpec {
            guard,
            dry_samples: dry.samples.clone(),
            performance: perf,
            source_entry: self.current_source_entry().clone(),
            voicing: self.voicing.clone(),
            edit,
        })
    }

    /// Validate guard and publish Stage 2 surgical region edit onto the dry take.
    pub fn publish_region_edit_result(
        &mut self,
        guard: &CompletionGuard,
        output: RegionEditOutput,
    ) -> Result<Arc<AudioArtifact>, StaleCompletionError> {
        if guard.session_id != self.session_id {
            return Err(StaleCompletionError::SessionMismatch {
                expected: self.session_id,
                got: guard.session_id,
            });
        }
        if guard.epoch != self.epoch {
            return Err(StaleCompletionError::EpochMismatch {
                expected: self.epoch,
                got: guard.epoch,
            });
        }
        if guard.request_seq < self.latest_region_seq {
            return Err(StaleCompletionError::SupersededRequest {
                latest: self.latest_region_seq,
                got: guard.request_seq,
            });
        }
        let expected_parent = self.dry.as_ref().map(|d| d.id);
        if guard.expected_parent_id != expected_parent {
            return Err(StaleCompletionError::ParentMismatch {
                expected: expected_parent,
                got: guard.expected_parent_id,
            });
        }
        if !output.outside_bit_identical {
            return Err(StaleCompletionError::InvalidArtifact(
                "Stage 2 invariant violation: samples outside edit region were modified".to_owned(),
            ));
        }

        let rate = self.dry.as_ref().map(|d| d.sample_rate).ok_or_else(|| {
            StaleCompletionError::InvalidArtifact("Dry buffer missing".to_owned())
        })?;
        let id = self.allocate_artifact_id();
        let artifact = AudioArtifact::new_mono(
            id,
            AudioOrigin::Stage2SurgicalDry,
            rate,
            output.edited_dry,
            expected_parent,
            guard.config_digest.clone(),
        )
        .map_err(StaleCompletionError::InvalidArtifact)?;
        let arc = Arc::new(artifact);

        self.history.push(self.snapshot());
        self.dry = Some(arc.clone());
        self.last_region_edit = Some(RegionEditSummary {
            start_sample: output.start_sample,
            end_sample: output.end_sample,
            affected_notes: output.affected_notes,
            effective_action: output.effective_action,
            unresolved_note: output.unresolved_note,
            outside_bit_identical: output.outside_bit_identical,
        });
        // Revision-bound approval: edited dry audio requires fresh human review.
        self.dry_review.clear();
        self.recompute_capture_staleness();
        self.display.take = DisplayTake::Dry;
        Ok(arc)
    }

    /// Prepare a Stages 3-5 post-dry capture job strictly from the current cached dry take.
    pub fn begin_post_dry_request(&mut self) -> Result<PostDryJobSpec, String> {
        let dry = self
            .dry
            .clone()
            .ok_or_else(|| "Regenerate Stage 1 dry audio before running Stages 3-5".to_owned())?;
        let cfg = self.post_dry_config();
        let cfg_hash = config_digest(&cfg);
        let seq = self.allocate_request_seq();
        self.latest_post_dry_seq = seq;

        let guard = CompletionGuard {
            session_id: self.session_id,
            epoch: self.epoch,
            request_seq: seq,
            expected_parent_id: Some(dry.id),
            config_digest: cfg_hash,
        };

        Ok(PostDryJobSpec {
            guard,
            dry_samples: dry.samples.clone(),
            sample_rate: dry.sample_rate,
            config: cfg,
            sources: self.sources.clone(),
            mics: self.mics.clone(),
            surfaces: self.surfaces.clone(),
        })
    }

    /// Validate guard and publish Stages 3-5 post-dry capture result.
    pub fn publish_post_dry_result(
        &mut self,
        guard: &CompletionGuard,
        output: PipelineRenderOutput,
    ) -> Result<Arc<AudioArtifact>, StaleCompletionError> {
        if guard.session_id != self.session_id {
            return Err(StaleCompletionError::SessionMismatch {
                expected: self.session_id,
                got: guard.session_id,
            });
        }
        if guard.epoch != self.epoch {
            return Err(StaleCompletionError::EpochMismatch {
                expected: self.epoch,
                got: guard.epoch,
            });
        }
        if guard.request_seq < self.latest_post_dry_seq {
            return Err(StaleCompletionError::SupersededRequest {
                latest: self.latest_post_dry_seq,
                got: guard.request_seq,
            });
        }
        let expected_parent = self.dry.as_ref().map(|d| d.id);
        if guard.expected_parent_id != expected_parent {
            return Err(StaleCompletionError::ParentMismatch {
                expected: expected_parent,
                got: guard.expected_parent_id,
            });
        }
        let cur_cfg = self.post_dry_config_digest();
        if guard.config_digest != cur_cfg {
            return Err(StaleCompletionError::ConfigChanged {
                expected: cur_cfg,
                got: guard.config_digest.clone(),
            });
        }
        if output.summary.original_audio_opened {
            return Err(StaleCompletionError::InvalidArtifact(
                "Source-free invariant violation: original audio was opened in Stages 3-5"
                    .to_owned(),
            ));
        }

        let rate = self.dry.as_ref().map(|d| d.sample_rate).ok_or_else(|| {
            StaleCompletionError::InvalidArtifact("Dry buffer missing".to_owned())
        })?;
        let id = self.allocate_artifact_id();
        let artifact = AudioArtifact::new_mono(
            id,
            AudioOrigin::PostDryCapture,
            rate,
            output.samples,
            expected_parent,
            guard.config_digest.clone(),
        )
        .map_err(StaleCompletionError::InvalidArtifact)?;
        let arc = Arc::new(artifact);

        self.capture = Some(arc.clone());
        self.capture_summary = Some(output.summary);
        self.capture_stale = false;
        Ok(arc)
    }

    // ---------- Atomic export & honest preset persistence ----------

    /// Atomically export the explicitly selected current artifact via `craft-atomic`
    /// (`pipeline::write_wav_f32_atomic_with_hash`).
    /// Blocks stale capture exports and refuses overwriting the loaded original WAV.
    pub fn export_artifact_atomic(
        &self,
        target: ExportTarget,
        out_path: &Path,
    ) -> Result<ExportReceiptInfo, String> {
        if let Some(src_path) = &self.source_path {
            if paths_match(src_path, out_path) {
                return Err(
                    "Refusing to overwrite the original source recording; choose a new export path"
                        .to_owned(),
                );
            }
        }
        let artifact = match target {
            ExportTarget::Dry => self
                .dry
                .as_ref()
                .ok_or_else(|| "No dry artifact available to export".to_owned())?,
            ExportTarget::CurrentCapture => {
                if self.capture_stale {
                    return Err(
                        "Post-dry capture is stale after recent edits or stage changes; re-render Stages 3-5 before exporting capture"
                            .to_owned(),
                    );
                }
                self.capture
                    .as_ref()
                    .ok_or_else(|| "No current post-dry capture available to export".to_owned())?
            }
        };

        let wav = pipeline::write_wav_f32_atomic_with_hash(
            out_path,
            artifact.sample_rate,
            &artifact.samples,
        )
        .map_err(|e| format!("Atomic WAV export failed: {e}"))?;

        Ok(ExportReceiptInfo {
            target,
            artifact_id: artifact.id,
            artifact_sha256: artifact.sha256.clone(),
            wav,
            path: out_path.to_path_buf(),
        })
    }

    /// Save current settings atomically, explicitly labeled as `SaveKind::SettingsPreset`
    /// (since no raw audio PCM is embedded in the JSON file).
    pub fn save_preset_atomic(&self, out_path: &Path) -> Result<(), String> {
        let doc = SessionPresetFile {
            version: 2,
            kind: SaveKind::SettingsPreset,
            source_path: self.source_path.as_ref().map(|p| p.display().to_string()),
            source_sha256: self.source.as_ref().map(|s| s.sha256.clone()),
            dry_sha256: self.dry.as_ref().map(|d| d.sha256.clone()),
            approved_dry_sha256: self.dry_review.approved_dry_sha256.clone(),
            selected_source_id: self.current_source_entry().id.clone(),
            voicing: self.voicing.clone(),
            mic_enabled: self.mic_enabled,
            preamp_drive: self.preamp_drive,
            room_enabled: self.room_enabled,
            rig: self.rig.clone(),
            plugin_slot: self.plugin_slot.clone(),
            selection: self.selection,
            marquee_hz: self.marquee_hz,
            region_edit_draft: self.region_edit_draft.clone(),
            dry_checklist: self.dry_review.checklist,
        };
        let bytes =
            serde_json::to_vec_pretty(&doc).map_err(|e| format!("Preset serialize error: {e}"))?;
        pipeline::write_bytes_atomic(out_path, &bytes)
            .map_err(|e| format!("Atomic preset save failed: {e}"))?;
        Ok(())
    }

    /// Load a settings preset file without faking audio or carrying over unverified approval.
    pub fn load_preset_file(&mut self, path: &Path) -> Result<SaveKind, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| format!("Preset read error: {e}"))?;
        let doc: SessionPresetFile =
            serde_json::from_str(&raw).map_err(|e| format!("Preset parse error: {e}"))?;
        if doc.version != 2 {
            return Err(format!("Unsupported preset schema version {}", doc.version));
        }

        self.history.push(self.snapshot());
        if let Some(idx) = self
            .sources
            .iter()
            .position(|s| s.id == doc.selected_source_id)
        {
            self.selected_source_idx = idx;
        }
        self.voicing = doc.voicing;
        self.mic_enabled = doc.mic_enabled;
        self.preamp_drive = doc.preamp_drive.clamp(0.0, 1.0);
        self.room_enabled = doc.room_enabled;
        self.rig = doc.rig;
        self.rig.source_index = self.selected_source_idx;
        self.plugin_slot = doc.plugin_slot;
        self.selection = doc.selection;
        self.marquee_hz = doc.marquee_hz;
        self.region_edit_draft = doc.region_edit_draft;

        // Only retain approval if the loaded preset matches the EXACT active dry SHA-256 already in memory.
        let current_dry_hash = self.dry.as_ref().map(|d| d.sha256.as_str());
        if current_dry_hash.is_some()
            && current_dry_hash == doc.approved_dry_sha256.as_deref()
            && doc.dry_checklist.all_passed()
        {
            self.dry_review = DryReviewRecord {
                checklist: doc.dry_checklist,
                approved_dry_sha256: doc.approved_dry_sha256,
            };
        } else {
            self.dry_review.clear();
        }
        self.recompute_capture_staleness();
        Ok(doc.kind)
    }
}

fn paths_match(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}
