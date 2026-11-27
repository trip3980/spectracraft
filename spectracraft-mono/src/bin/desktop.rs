use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use spectracraft_mono::app_session::sha256_f32_slice;
use spectracraft_mono::capture::{
    self, DryReviewStatus, ExportVariant, Recording, StudioPrediction,
};
use spectracraft_mono::pipeline::{self, PluginId, PluginSlotConfig, PostDryPipelineConfig};
use spectracraft_mono::platform::{self, AudioPlayback};
use spectracraft_mono::reference_profiles;
use spectracraft_mono::region_edit::{self, DryRegionEdit, RegionFixAction};
use spectracraft_mono::reperform::{
    self, CharacterTarget, SignalPathStyle, SourceCharacterVariant, StraightenMode, VoicingControls,
};
use spectracraft_mono::studio_catalog::{
    self, GridDirection, MicAim, MicEntry, Pattern, RoomPreset, SourceCategory, SourceEntry,
    SourceRadiation, SourceSymbol, StudioRig, SurfaceEntry, Unit, BANDS_HZ, FACE_LABELS,
};
use spectracraft_mono::visualization::{
    measure_peaks, spectrum_color_rgb, Peaks, Spectrum, Visual, SPECTRUM_BOTTOM, SPECTRUM_H,
};
use spectracraft_mono::{analyze_with_texture, digest, synthesize, write_wav};
use spectracraft_mono::{edge, workspace};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

const MAX_EXCERPT_SECS: f64 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Take {
    Original,
    Regenerated,
    Difference,
}
impl Take {
    fn name(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Regenerated => "Regenerated",
            Self::Difference => "Difference",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum CanvasMode {
    Spectrograms,
    Waveforms,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionTool {
    Range,
    Marquee,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegionPreviewMode {
    Off,
    Before,
    After,
}
impl RegionPreviewMode {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Full Track",
            Self::Before => "Region Before",
            Self::After => "Region After",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorTab {
    Source,
    DryEdit,
    Microphone,
    Materials,
    Room,
    Plugin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpatialViewMode {
    Plans,
    Grid3D,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewBitDepth {
    Float32,
    Int24,
    Int16,
}
impl PreviewBitDepth {
    const ALL: [Self; 3] = [Self::Float32, Self::Int24, Self::Int16];
    fn label(self) -> &'static str {
        match self {
            Self::Float32 => "32-bit float",
            Self::Int24 => "24-bit preview",
            Self::Int16 => "16-bit preview",
        }
    }
    fn quantize(self, x: f32) -> f32 {
        match self {
            Self::Float32 => x,
            Self::Int24 => {
                let s = 8_388_607.0_f32;
                (x.clamp(-1.0, 1.0) * s).round() / s
            }
            Self::Int16 => {
                let s = 32_767.0_f32;
                (x.clamp(-1.0, 1.0) * s).round() / s
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppState {
    Empty,
    Loading,
    Ready,
    Busy,
    Error,
}
impl AppState {
    fn label(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::Loading => "Loading",
            Self::Ready => "Ready",
            Self::Busy => "Busy",
            Self::Error => "Error",
        }
    }
    fn color(self) -> Color32 {
        match self {
            Self::Empty => Color32::from_rgb(130, 140, 155),
            Self::Loading | Self::Busy => Color32::from_rgb(235, 185, 75),
            Self::Ready => Color32::from_rgb(90, 200, 140),
            Self::Error => Color32::from_rgb(230, 95, 95),
        }
    }
}

fn spectrum_color(dbfs: f32) -> Color32 {
    let [r, g, b] = spectrum_color_rgb(dbfs);
    Color32::from_rgb(r, g, b)
}

trait SpectrumEguiExt {
    fn image(&self) -> egui::ColorImage;
}

impl SpectrumEguiExt for Spectrum {
    fn image(&self) -> egui::ColorImage {
        egui::ColorImage {
            size: [self.width, SPECTRUM_H],
            pixels: self.dbfs.iter().copied().map(spectrum_color).collect(),
        }
    }
}

struct LayerSession {
    packet: edge::Packet,
    rendered: workspace::Rendered,
    controls: workspace::Controls,
    headroom: f32,
    view: Option<usize>,
    /// Sample range in the full track corresponding to this layer session.
    span: [usize; 2],
}

struct Track {
    overview_peaks: Peaks,
    original_view: Visual,
    regenerated_view: Option<Visual>,
    difference_view: Option<Visual>,
    name: String,
    rate: u32,
    original: Arc<[f32]>,
    /// Sample span [start, end] of the current regenerated/dry excerpt inside `original`.
    rebuilt_span: Option<[usize; 2]>,
    rebuilt: Option<Vec<f32>>,
    dry: Option<Arc<[f32]>>,
    /// Plot-only buffer when inspecting a single layer via `View` (does NOT affect audio).
    layer_plot_only: Option<Vec<f32>>,
    recording: Recording,
    layers: Option<LayerSession>,
    guitar_cues: Option<reperform::Performance>,
}

fn aligned_excerpt_slice(
    span_start: usize,
    excerpt: &[f32],
    s0: usize,
    s1: usize,
) -> Cow<'_, [f32]> {
    if s1 <= s0 {
        return Cow::Borrowed(&[]);
    }
    let span_end = span_start.saturating_add(excerpt.len());
    if s0 >= span_start && s1 <= span_end {
        return Cow::Borrowed(&excerpt[s0 - span_start..s1 - span_start]);
    }
    let mut out = vec![0.0_f32; s1 - s0];
    let ov0 = s0.max(span_start);
    let ov1 = s1.min(span_end);
    if ov1 > ov0 {
        out[ov0 - s0..ov1 - s0].copy_from_slice(&excerpt[ov0 - span_start..ov1 - span_start]);
    }
    Cow::Owned(out)
}

impl Track {
    fn duration(&self) -> f64 {
        self.original.len() as f64 / self.rate as f64
    }
    fn rebuilt_span_secs(&self) -> Option<[f64; 2]> {
        self.rebuilt_span
            .map(|[a, b]| [a as f64 / self.rate as f64, b as f64 / self.rate as f64])
    }
    /// Viewport/playback-bounded aligned regenerated slice (zero full-track padded copy).
    fn aligned_rebuilt_slice(&self, s0: usize, s1: usize) -> Option<Cow<'_, [f32]>> {
        let rebuilt = self.rebuilt.as_deref()?;
        let (s0, s1) = platform::clamp_playback_slice(self.original.len(), s0, Some(s1));
        let [start, _] = self.rebuilt_span.unwrap_or([0, rebuilt.len()]);
        Some(aligned_excerpt_slice(start, rebuilt, s0, s1))
    }
    /// Viewport/playback-bounded sample-aligned difference: `rebuilt[i] - original[i]` on `rebuilt_span`, 0 outside.
    fn aligned_difference_slice(&self, s0: usize, s1: usize) -> Option<Vec<f32>> {
        let rebuilt = self.rebuilt.as_deref()?;
        let (s0, s1) = platform::clamp_playback_slice(self.original.len(), s0, Some(s1));
        let [start, _] = self.rebuilt_span.unwrap_or([0, rebuilt.len()]);
        let span_end = start.saturating_add(rebuilt.len()).min(self.original.len());
        let mut diff = vec![0.0_f32; s1 - s0];
        let ov0 = s0.max(start);
        let ov1 = s1.min(span_end);
        for i in ov0..ov1 {
            diff[i - s0] = rebuilt[i - start] - self.original[i];
        }
        Some(diff)
    }
    #[cfg(test)]
    fn aligned_difference(&self) -> Option<Vec<f32>> {
        self.aligned_difference_slice(0, self.original.len())
    }
    #[cfg(test)]
    fn audition(&self, which: Take) -> Option<Vec<f32>> {
        self.audition_with_options(which, true, PreviewBitDepth::Float32)
    }
    #[cfg(test)]
    fn audition_with_options(
        &self,
        which: Take,
        rms_match: bool,
        bit_depth: PreviewBitDepth,
    ) -> Option<Vec<f32>> {
        self.audition_slice_with_options(which, rms_match, bit_depth, 0, None)
    }
    fn audition_slice_with_options(
        &self,
        which: Take,
        rms_match: bool,
        bit_depth: PreviewBitDepth,
        start_sample: usize,
        end_sample: Option<usize>,
    ) -> Option<Vec<f32>> {
        let (slice_start, slice_end) =
            platform::clamp_playback_slice(self.original.len(), start_sample, end_sample);
        let quant = |mut v: Vec<f32>| {
            if bit_depth != PreviewBitDepth::Float32 {
                for s in &mut v {
                    *s = bit_depth.quantize(*s);
                }
            }
            v
        };
        if let Some(layers) = &self.layers {
            let samples: Vec<f32> = match which {
                Take::Original => self.original[slice_start..slice_end]
                    .iter()
                    .map(|&v| v * layers.headroom)
                    .collect(),
                Take::Regenerated => self
                    .aligned_rebuilt_slice(slice_start, slice_end)?
                    .iter()
                    .map(|&v| v * layers.headroom)
                    .collect(),
                Take::Difference => self
                    .aligned_difference_slice(slice_start, slice_end)?
                    .into_iter()
                    .map(|v| v * layers.headroom)
                    .collect(),
            };
            return Some(quant(samples));
        }
        let Some(other_excerpt) = self.rebuilt.as_deref() else {
            return (which == Take::Original).then(|| {
                let peak = self.original.iter().fold(0_f32, |p, x| p.max(x.abs()));
                let scale = 0.89 / peak.max(0.89);
                quant(
                    self.original[slice_start..slice_end]
                        .iter()
                        .map(|&x| x * scale)
                        .collect(),
                )
            });
        };
        let [s0, s1] = self
            .rebuilt_span
            .unwrap_or([0, other_excerpt.len().min(self.original.len())]);
        let orig_excerpt = &self.original[s0..s1.min(self.original.len())];
        let rms = |s: &[f32]| {
            if s.is_empty() {
                0.0
            } else {
                (s.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
            }
        };
        let gain = if rms_match {
            (rms(orig_excerpt) / rms(other_excerpt).max(1e-10)).clamp(0.125, 8.) as f32
        } else {
            1.0
        };
        let peak = |s: &[f32]| s.iter().fold(0_f32, |p, x| p.max(x.abs()));
        let master = 0.89
            / 0.89_f32
                .max(peak(&self.original))
                .max(peak(other_excerpt) * gain);
        let out: Vec<f32> = match which {
            Take::Original => self.original[slice_start..slice_end]
                .iter()
                .map(|&v| v * master)
                .collect(),
            Take::Regenerated => self
                .aligned_rebuilt_slice(slice_start, slice_end)?
                .iter()
                .map(|&v| v * master * gain)
                .collect(),
            Take::Difference => {
                // Difference between level-matched regenerated and original on active span
                let mut diff = vec![0.0_f32; slice_end - slice_start];
                let span_end = s0
                    .saturating_add(other_excerpt.len())
                    .min(self.original.len());
                let ov0 = slice_start.max(s0);
                let ov1 = slice_end.min(span_end);
                for i in ov0..ov1 {
                    diff[i - slice_start] =
                        other_excerpt[i - s0] * master * gain - self.original[i] * master;
                }
                diff
            }
        };
        Some(quant(out))
    }
}

enum Finished {
    Layers(Result<LayerSession, String>),
    Load(Result<Track, String>),
    Render(Result<([usize; 2], Vec<f32>), String>),
    GuitarDry(Result<([usize; 2], reperform::Performance, Vec<f32>), String>),
    DryResynth(Result<([usize; 2], Vec<f32>), String>),
    Capture(
        PostDryPipelineConfig,
        [usize; 2],
        pipeline::PipelineRenderOutput,
    ),
}

#[derive(Clone, Copy, Debug)]
struct Camera3D {
    yaw_deg: f32,
    pitch_deg: f32,
    pan: [f32; 2],
    zoom: f32,
    perspective: bool,
}
impl Default for Camera3D {
    fn default() -> Self {
        Self {
            yaw_deg: -34.0,
            pitch_deg: 24.0,
            pan: [0.0, 0.0],
            zoom: 1.0,
            perspective: true,
        }
    }
}

/// Display-only docking placement for the Studio Inspector (never alters audio or undo history).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorDock {
    Left,
    Right,
    Floating,
    Hidden,
}

/// Display-only docking placement for the Lower Controls Dock (never alters audio or undo history).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LowerDock {
    Bottom,
    Floating,
    Hidden,
}

/// Direct manipulation mode when dragging on a waveform or spectrogram plot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionDragMode {
    Create,
    Move,
    ResizeLeft,
    ResizeRight,
    ResizeTop,
    ResizeBottom,
    ResizeTopLeft,
    ResizeTopRight,
    ResizeBottomLeft,
    ResizeBottomRight,
}

/// Centralized ArtCraft / Audition / Logic-style Pro studio dark theme tokens
/// (neutral charcoal surfaces, Spectrum Blue focus, emerald active stage, high-contrast ink).
struct StudioTheme;
impl StudioTheme {
    const BG_WINDOW: Color32 = Color32::from_rgb(22, 22, 26); // #16161a canvas chrome
    const BG_HEADER: Color32 = Color32::from_rgb(28, 28, 33); // #1c1c21 panel header strips
    const BG_PANEL: Color32 = Color32::from_rgb(31, 31, 36); // #1f1f24 docked panels
    const BG_SURFACE: Color32 = Color32::from_rgb(38, 38, 44); // #26262c section cards
    const BG_CANVAS: Color32 = Color32::from_rgb(18, 18, 22); // #121216 plots & spatial plans
    const BG_WIDGET: Color32 = Color32::from_rgb(46, 46, 54); // #2e2e36 flat buttons/inputs
    const BG_HOVER: Color32 = Color32::from_rgb(58, 58, 69); // #3a3a45 hover state
    const ACCENT_BLUE: Color32 = Color32::from_rgb(55, 142, 240); // #378ef0 Spectrum Blue
    const STAGE_ACTIVE: Color32 = Color32::from_rgb(52, 211, 153); // #34d399 active DSP stage
    const STAGE_BYPASS: Color32 = Color32::from_rgb(156, 163, 175); // #9ca3af bypassed stage
    const WARNING: Color32 = Color32::from_rgb(245, 158, 11); // #f59e0b review/warning amber
    const BORDER_SUBTLE: Color32 = Color32::from_rgb(58, 58, 68); // #3a3a44 1px hairline seams
    const TEXT_PRIMARY: Color32 = Color32::from_rgb(238, 238, 243); // #eeeef3 >= 12:1 contrast
    const TEXT_MUTED: Color32 = Color32::from_rgb(161, 161, 172); // #a1a1ac >= 5.2:1 contrast

    fn apply(ctx: &egui::Context) {
        let mut v = egui::Visuals::dark();
        v.panel_fill = Self::BG_PANEL;
        v.window_fill = Self::BG_WINDOW;
        v.window_stroke = Stroke::new(1.0, Self::BORDER_SUBTLE);
        v.extreme_bg_color = Self::BG_CANVAS;
        v.faint_bg_color = Self::BG_SURFACE;
        v.override_text_color = Some(Self::TEXT_PRIMARY);
        v.selection.bg_fill = Color32::from_rgba_unmultiplied(55, 142, 240, 88);
        v.selection.stroke = Stroke::new(1.0, Self::ACCENT_BLUE);
        v.widgets.inactive.bg_fill = Self::BG_WIDGET;
        v.widgets.inactive.weak_bg_fill = Self::BG_WIDGET;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, Self::BORDER_SUBTLE);
        v.widgets.inactive.corner_radius = egui::CornerRadius::same(3);
        v.widgets.hovered.bg_fill = Self::BG_HOVER;
        v.widgets.hovered.weak_bg_fill = Self::BG_HOVER;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, Self::ACCENT_BLUE);
        v.widgets.hovered.corner_radius = egui::CornerRadius::same(3);
        v.widgets.active.bg_fill = Color32::from_rgb(42, 92, 150);
        v.widgets.active.corner_radius = egui::CornerRadius::same(3);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Self::BORDER_SUBTLE);
        ctx.set_visuals(v);

        ctx.style_mut(|style| {
            style.spacing.item_spacing = Vec2::new(6.0, 5.0);
            style.spacing.button_padding = Vec2::new(7.0, 3.0);
            style.spacing.interact_size = Vec2::new(34.0, 21.0);
            style.spacing.window_margin = egui::Margin::same(8);
            style.spacing.slider_width = 128.0;
            style.spacing.combo_width = 144.0;
        });
    }

    fn section_frame<R>(
        ui: &mut egui::Ui,
        title: &str,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        egui::Frame::new()
            .fill(Self::BG_SURFACE)
            .stroke(Stroke::new(1.0, Self::BORDER_SUBTLE))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(title)
                        .strong()
                        .size(11.5)
                        .color(Self::TEXT_PRIMARY),
                );
                ui.add_space(3.0);
                add_contents(ui)
            })
            .inner
    }
}

/// Core Design Contract: 5-criterion human listening gate required before any post-processing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct DryListeningChecklist {
    rhythm: bool,
    phrasing: bool,
    dynamics: bool,
    sustain: bool,
    artifact_free: bool,
}

impl DryListeningChecklist {
    fn passed_count(&self) -> usize {
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

    fn all_passed(&self) -> bool {
        self.passed_count() == 5
    }

    fn sync_status(&self) -> DryReviewStatus {
        if self.all_passed() {
            DryReviewStatus::ApprovedByHuman
        } else {
            DryReviewStatus::NotReviewed
        }
    }
}

#[derive(Clone, Debug)]
struct RegionEditReport {
    start_sample: usize,
    end_sample: usize,
    affected_notes: usize,
    effective_action: RegionFixAction,
    unresolved_note: Option<String>,
    outside_bit_identical: bool,
    before_slice: Vec<f32>,
    after_slice: Vec<f32>,
}

/// Copy-on-write reversible studio snapshot for Undo / Redo (never mutates original audio or playhead).
#[derive(Clone, Debug)]
struct StudioSnapshot {
    rig: StudioRig,
    voicing: VoicingControls,
    mic_enabled: bool,
    room_enabled: bool,
    plugin_slot: PluginSlotConfig,
    selection: Option<[f64; 2]>,
    marquee_hz: Option<[f64; 2]>,
    region_edit_draft: DryRegionEdit,
    dry_audio: Option<Arc<[f32]>>,
    layer_controls: Option<workspace::Controls>,
}

impl PartialEq for StudioSnapshot {
    fn eq(&self, other: &Self) -> bool {
        let dry_same = match (&self.dry_audio, &other.dry_audio) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a.as_ref() == b.as_ref(),
            _ => false,
        };
        self.rig == other.rig
            && self.voicing == other.voicing
            && self.mic_enabled == other.mic_enabled
            && self.room_enabled == other.room_enabled
            && self.plugin_slot == other.plugin_slot
            && self.selection == other.selection
            && self.marquee_hz == other.marquee_hz
            && self.region_edit_draft == other.region_edit_draft
            && dry_same
            && self.layer_controls == other.layer_controls
    }
}


/// Serializable SpectraCraft studio session file (`.spectracraft.json`), written atomically.
/// Version 2 records source/dry provenance and revision-bound approval; Version 1 is loaded
/// explicitly as a settings-only preset without carrying over dry-review approval (Audit H6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StudioSessionFile {
    version: u32,
    #[serde(default)]
    source_path: Option<String>,
    #[serde(default)]
    source_sha256: Option<String>,
    #[serde(default)]
    dry_sha256: Option<String>,
    #[serde(default)]
    approved_dry_sha256: Option<String>,
    rig: StudioRig,
    voicing: VoicingControls,
    mic_enabled: bool,
    room_enabled: bool,
    plugin_slot: PluginSlotConfig,
    selection: Option<[f64; 2]>,
    marquee_hz: Option<[f64; 2]>,
    region_edit_draft: DryRegionEdit,
    dry_checklist: DryListeningChecklist,
    canvas_mode: CanvasMode,
    display_max_hz: f32,
}

/// SoundCraft / FigCraft-inspired structured command surface: panels, menus, shortcuts,
/// and automated tests all mutate authoritative engine state through this single dispatcher.
#[allow(dead_code)]
#[derive(Clone, Debug)]
enum StudioCommand {
    // Stage 1 — Dry Regeneration
    SelectSource(usize),
    SetVoicing(VoicingControls),
    RegenerateDry,
    AnalyzeGuitarDry,
    ResynthesizeDryFromCues,
    // Stage 2 — Surgical Region Editing (Dry only)
    SetSelection(Option<[f64; 2]>),
    SetMarqueeHz(Option<[f64; 2]>),
    SetRegionEditDraft(DryRegionEdit),
    ApplyRegionEdit,
    SetRegionPreview(RegionPreviewMode),
    // Stage 3 — Optional Microphone
    SetMicEnabled(bool),
    SelectMicSlot(usize),
    AddMicSlot,
    RemoveMicSlot,
    SetMicCatalog(usize),
    SetMicPatternOverride(Option<Pattern>),
    SetMicOffset([f32; 3]),
    SetMicAim(MicAim),
    // Stage 4 — Optional Space / Room & Materials
    SetRoomEnabled(bool),
    ApplyRoomPreset(RoomPreset),
    SetRoomDimension { axis: usize, metres: f32 },
    SetSurfaceFace { face: usize, surface_idx: usize },
    SetAllWalls(usize),
    SetTemperatureC(f32),
    // Stage 5 — Optional Plugin Slot
    SetPluginSlot(PluginSlotConfig),
    // Review Gate & Post-Dry Execution
    SetDryChecklist(DryListeningChecklist),
    RunPostDryPipeline,
    // Transport & Display-Only (MUST NEVER mutate audio)
    TogglePlay,
    Seek(f64),
    SwitchTake(Take),
    ZoomToSelection,
    ZoomToResult,
    FitRecording,
    ZoomTimeline { factor: f64, anchor_frac: f64 },
    SetDisplayUnit(Unit),
    SetCanvasMode(CanvasMode),
    SetDisplayMaxHz(f32),
    SetShowPatterns(bool),
    // History & Persistence
    Undo,
    Redo,
    SaveSession(PathBuf),
    LoadSession(PathBuf),
}

const MAX_HISTORY_AUDIO_BYTES: usize = 128 * 1024 * 1024;
const MAX_HISTORY_UNIQUE_DRY_BUFFERS: usize = 8;

struct StudioHistory {
    past: Vec<StudioSnapshot>,
    future: Vec<StudioSnapshot>,
    max_depth: usize,
    max_audio_bytes: usize,
    max_unique_dry_buffers: usize,
}

impl Default for StudioHistory {
    fn default() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            max_depth: 64,
            max_audio_bytes: MAX_HISTORY_AUDIO_BYTES,
            max_unique_dry_buffers: MAX_HISTORY_UNIQUE_DRY_BUFFERS,
        }
    }
}

impl StudioHistory {
    fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }

    fn unique_dry_usage(&self) -> (usize, usize) {
        let mut seen: Vec<(usize, usize)> = Vec::new();
        for snap in self.past.iter().chain(self.future.iter()) {
            if let Some(buf) = &snap.dry_audio {
                let ptr = buf.as_ptr() as usize;
                let len = buf.len();
                if !seen.iter().any(|&(p, l)| p == ptr && l == len) {
                    seen.push((ptr, len));
                }
            }
        }
        let total_bytes: usize = seen
            .iter()
            .map(|&(_, len)| len.saturating_mul(std::mem::size_of::<f32>()))
            .sum();
        (seen.len(), total_bytes)
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
            if unique_count <= self.max_unique_dry_buffers && total_bytes <= self.max_audio_bytes {
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

    fn push(&mut self, before: StudioSnapshot) {
        if self.past.last() == Some(&before) {
            return;
        }
        self.past.push(before);
        self.future.clear();
        self.enforce_budget();
    }
    fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }
    fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
    fn undo(&mut self, current: StudioSnapshot) -> Option<StudioSnapshot> {
        let prev = self.past.pop()?;
        self.future.push(current);
        self.enforce_budget();
        Some(prev)
    }
    fn redo(&mut self, current: StudioSnapshot) -> Option<StudioSnapshot> {
        let next = self.future.pop()?;
        self.past.push(current);
        self.enforce_budget();
        Some(next)
    }
}

struct Studio {
    // Catalogs (138 sources, curated factual mics, 32 6-band surfaces)
    sources: Arc<[SourceEntry]>,
    mics: Arc<[MicEntry]>,
    surfaces: Arc<[SurfaceEntry]>,
    theme_applied: bool,
    history: StudioHistory,

    // Loaded audio & background job
    track: Option<Track>,
    job: Option<(u64, mpsc::Receiver<Finished>)>,
    active_job_id: u64,
    job_cancel: Option<Arc<AtomicBool>>,
    app_state: AppState,
    play: Option<AudioPlayback>,
    take: Take,
    cursor: f64,
    status: String,
    smoke: Option<PathBuf>,

    // Open-by-path fallback UI
    show_path_bar: bool,
    path_input: String,
    jump_input: String,

    // Timeline view & selection (seconds + optional Hz marquee for Stage 2)
    view_range: [f64; 2],
    selection: Option<[f64; 2]>,
    selection_tool: SelectionTool,
    marquee_hz: Option<[f64; 2]>,
    drag_anchor: Option<f64>,
    drag_hz_anchor: Option<f64>,
    drag_mode: SelectionDragMode,
    drag_initial_sel: Option<[f64; 2]>,
    drag_initial_hz: Option<[f64; 2]>,
    canvas_mode: CanvasMode,
    display_max_hz: f32,
    preview_bit_depth: PreviewBitDepth,
    audition_rms_match: bool,
    include_texture: bool,

    // Stage 1 — Dry Regeneration voicing controls (always dry, never enables mic/room/plugin)
    voicing: VoicingControls,

    // Stage 2 — Surgical Region Editing on Dry Take
    region_edit_draft: DryRegionEdit,
    last_region_edit: Option<RegionEditReport>,
    region_preview: RegionPreviewMode,

    // Stages 3, 4, 5 — Optional Independently Bypassable Post-Dry Pipeline (ALL OFF by default)
    mic_enabled: bool,
    room_enabled: bool,
    plugin_slot: PluginSlotConfig,
    last_pipeline_summary: Option<pipeline::PipelineStageSummary>,

    // Session persistence & source provenance (Audit H6)
    session_path: Option<PathBuf>,
    source_path: Option<PathBuf>,
    approved_dry_sha256: Option<String>,

    // Background post-dry worker with monotonic revision coalescing (Audit M4)
    post_dry_job_rev: u64,
    post_dry_worker: Option<(
        u64,
        mpsc::Receiver<(PostDryPipelineConfig, [usize; 2], pipeline::PipelineRenderOutput)>,
    )>,

    // Viewport texture cache key: (view_start_ms, view_end_ms, display_max_hz_i32, rev)
    plot_cache_key: Option<(u64, u64, i32, u64)>,
    plot_revision: u64,
    original_texture: Option<egui::TextureHandle>,
    regenerated_texture: Option<egui::TextureHandle>,
    difference_texture: Option<egui::TextureHandle>,

    // Dockable / Undockable Studio Inspector & Spatial Views (display-only layout state)
    inspector_dock: InspectorDock,
    show_spatial_viewport: bool,
    inspector_tab: InspectorTab,
    spatial_mode: SpatialViewMode,
    rig: StudioRig,
    applied_rig: Option<StudioRig>,
    live_preview: bool,
    // Inspector search/filter state
    source_category_filter: SourceCategory,
    source_family_filter: String,
    source_search: String,
    mic_search: String,
    mic_pattern_filter: Option<Pattern>,
    aim_snap_step: f32,
    surface_search: String,
    // Linked Top + Front plan zoom & X pan (shared across both plans)
    plan_zoom: f32,
    plan_pan_x_m: f32,
    // Separate 3D Grid camera (never moves/rotates mics)
    cam_3d: Camera3D,

    // Dockable / Undockable Lower dock & dry review / variant export gating
    lower_dock: LowerDock,
    show_layers_panel: bool,
    dry_review: DryReviewStatus,
    dry_checklist: DryListeningChecklist,
    variant_selection: [bool; 5],
}

impl Studio {
    fn new() -> Self {
        Self {
            sources: Arc::from(studio_catalog::source_catalog()),
            mics: Arc::from(studio_catalog::mic_catalog()),
            surfaces: Arc::from(studio_catalog::surface_catalog()),
            theme_applied: false,
            history: StudioHistory::default(),
            track: None,
            job: None,
            active_job_id: 0,
            job_cancel: None,
            app_state: AppState::Empty,
            play: None,
            take: Take::Original,
            cursor: 0.,
            status:
                "No file loaded. Open a WAV file, drop one onto the window, or use Open by Path."
                    .into(),
            smoke: None,
            show_path_bar: false,
            path_input: String::new(),
            jump_input: String::new(),
            view_range: [0.0, 1.0],
            selection: None,
            selection_tool: SelectionTool::Range,
            marquee_hz: None,
            drag_anchor: None,
            drag_hz_anchor: None,
            drag_mode: SelectionDragMode::Create,
            drag_initial_sel: None,
            drag_initial_hz: None,
            canvas_mode: CanvasMode::Spectrograms,
            display_max_hz: 12_000.0,
            preview_bit_depth: PreviewBitDepth::Float32,
            audition_rms_match: true,
            include_texture: false,
            voicing: VoicingControls::default(),
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
            last_region_edit: None,
            region_preview: RegionPreviewMode::Off,
            mic_enabled: false,
            room_enabled: false,
            plugin_slot: PluginSlotConfig::default(),
            last_pipeline_summary: None,
            session_path: None,
            source_path: None,
            approved_dry_sha256: None,
            post_dry_job_rev: 0,
            post_dry_worker: None,
            plot_cache_key: None,
            plot_revision: 0,
            original_texture: None,
            regenerated_texture: None,
            difference_texture: None,
            inspector_dock: InspectorDock::Left,
            show_spatial_viewport: true,
            inspector_tab: InspectorTab::Source,
            spatial_mode: SpatialViewMode::Plans,
            rig: StudioRig::default(),
            applied_rig: None,
            live_preview: false,
            source_category_filter: SourceCategory::Unknown,
            source_family_filter: String::new(),
            source_search: String::new(),
            mic_search: String::new(),
            mic_pattern_filter: None,
            aim_snap_step: 15.0,
            surface_search: String::new(),
            plan_zoom: 1.0,
            plan_pan_x_m: 0.0,
            cam_3d: Camera3D::default(),
            lower_dock: LowerDock::Bottom,
            show_layers_panel: false,
            dry_review: DryReviewStatus::NotReviewed,
            dry_checklist: DryListeningChecklist::default(),
            variant_selection: [true; 5],
        }
    }

    /// Authoritative post-dry pipeline configuration (Stages 3, 4, 5).
    fn post_dry_config(&self) -> PostDryPipelineConfig {
        let mut rig = self.rig.clone();
        rig.clamp_all();
        PostDryPipelineConfig {
            mic_enabled: self.mic_enabled,
            room_enabled: self.room_enabled,
            rig,
            preamp_drive: 0.0,
            plugin_slot: self.plugin_slot.clone(),
        }
    }

    /// Effective acoustic rig reflecting Stage 3 (Mic) and Stage 4 (Room) bypass states,
    /// matching `pipeline::run_post_dry_pipeline` exactly for UI readouts & inspection.
    fn effective_rig_for_prediction(&self) -> StudioRig {
        let mut eff = self.rig.clone();
        eff.clamp_all();
        if !self.room_enabled {
            eff.room_preset = RoomPreset::Direct;
        }
        if !self.mic_enabled {
            for slot in &mut eff.mics {
                slot.pattern_override = Some(Pattern::Omni);
            }
        }
        eff
    }

    fn effective_prediction(&self) -> StudioPrediction {
        let eff = self.effective_rig_for_prediction();
        capture::predict_studio_rig(&eff, &self.sources, &self.mics, &self.surfaces)
    }

    /// Combined DSP fingerprint for Stages 3, 4, 5 (display-only actions NEVER change this).
    fn post_dry_fingerprint(
        &self,
    ) -> (
        studio_catalog::AcousticFingerprint,
        bool,
        bool,
        i32,
        bool,
        PluginId,
        i32,
        i32,
        i32,
        i32,
    ) {
        let cfg = self.post_dry_config();
        (
            self.rig.acoustic_fingerprint(),
            self.mic_enabled,
            self.room_enabled,
            (cfg.preamp_drive * 1000.0).round() as i32,
            self.plugin_slot.enabled,
            self.plugin_slot.plugin_id,
            (self.plugin_slot.eq_low_db * 100.0).round() as i32,
            (self.plugin_slot.eq_high_db * 100.0).round() as i32,
            (self.plugin_slot.amount * 1000.0).round() as i32,
            (self.plugin_slot.ceiling_dbfs * 100.0).round() as i32,
        )
    }

    fn capture_snapshot(&self) -> StudioSnapshot {
        StudioSnapshot {
            rig: self.rig.clone(),
            voicing: self.voicing.clone(),
            mic_enabled: self.mic_enabled,
            room_enabled: self.room_enabled,
            plugin_slot: self.plugin_slot.clone(),
            selection: self.selection,
            marquee_hz: self.marquee_hz,
            region_edit_draft: self.region_edit_draft.clone(),
            dry_audio: self.track.as_ref().and_then(|t| t.dry.clone()),
            layer_controls: self
                .track
                .as_ref()
                .and_then(|t| t.layers.as_ref())
                .map(|l| l.controls.clone()),
        }
    }

    fn apply_snapshot(&mut self, snap: StudioSnapshot, ctx: &egui::Context) {
        let dry_changed = self
            .track
            .as_ref()
            .and_then(|t| t.dry.as_ref())
            .map(|d| d.as_ref())
            != snap.dry_audio.as_ref().map(|d| d.as_ref());
        let post_changed = self.rig != snap.rig
            || self.mic_enabled != snap.mic_enabled
            || self.room_enabled != snap.room_enabled
            || self.plugin_slot != snap.plugin_slot;

        self.rig = snap.rig;
        self.rig.clamp_all();
        self.voicing = snap.voicing;
        self.mic_enabled = snap.mic_enabled;
        self.room_enabled = snap.room_enabled;
        self.plugin_slot = snap.plugin_slot;
        self.selection = snap.selection;
        self.marquee_hz = snap.marquee_hz;
        self.region_edit_draft = snap.region_edit_draft;

        if dry_changed {
            if let (Some(t), Some(dry)) = (self.track.as_mut(), snap.dry_audio) {
                t.dry = Some(dry);
            }
        }

        let mut remix = false;
        if let (Some(ctrls), Some(layers)) = (
            snap.layer_controls,
            self.track.as_mut().and_then(|t| t.layers.as_mut()),
        ) {
            if layers.controls != ctrls {
                layers.controls = ctrls;
                remix = true;
            }
        }
        if remix {
            self.remix_layers(ctx);
        } else if (dry_changed || post_changed)
            && self.track.as_ref().is_some_and(|t| t.dry.is_some())
        {
            self.sync_post_dry_immediate(ctx);
        }
    }

    fn undo(&mut self, ctx: &egui::Context) -> bool {
        let cur = self.capture_snapshot();
        if let Some(prev) = self.history.undo(cur) {
            self.apply_snapshot(prev, ctx);
            self.status =
                "Undid studio edit and synchronized audio (original & playhead unchanged).".into();
            true
        } else {
            false
        }
    }

    fn redo(&mut self, ctx: &egui::Context) -> bool {
        let cur = self.capture_snapshot();
        if let Some(next) = self.history.redo(cur) {
            self.apply_snapshot(next, ctx);
            self.status =
                "Redid studio edit and synchronized audio (original & playhead unchanged).".into();
            true
        } else {
            false
        }
    }

    /// Clear human dry-review approval whenever dry audio changes (Audit H5).
    fn invalidate_dry_review(&mut self) {
        self.dry_checklist = DryListeningChecklist::default();
        self.dry_review = DryReviewStatus::NotReviewed;
        self.approved_dry_sha256 = None;
        self.applied_rig = None;
        self.last_pipeline_summary = None;
    }

    /// Synchronize dry review status from checklist and bind approval strictly to current dry SHA-256.
    fn sync_dry_review_from_checklist(&mut self) {
        let next = self.dry_checklist.sync_status();
        self.dry_review = next;
        self.approved_dry_sha256 = if next == DryReviewStatus::ApprovedByHuman {
            self.track
                .as_ref()
                .and_then(|t| t.dry.as_deref())
                .map(sha256_f32_slice)
        } else {
            None
        };
    }

    fn save_session(&mut self, path: &Path) -> Result<(), String> {
        let source_sha256 = self.track.as_ref().map(|t| sha256_f32_slice(&t.original));
        let dry_sha256 = self
            .track
            .as_ref()
            .and_then(|t| t.dry.as_deref())
            .map(sha256_f32_slice);
        let approved_dry_sha256 = if self.dry_review == DryReviewStatus::ApprovedByHuman {
            dry_sha256.clone()
        } else {
            None
        };
        let session = StudioSessionFile {
            version: 2,
            source_path: self
                .source_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            source_sha256,
            dry_sha256,
            approved_dry_sha256,
            rig: self.rig.clone(),
            voicing: self.voicing.clone(),
            mic_enabled: self.mic_enabled,
            room_enabled: self.room_enabled,
            plugin_slot: self.plugin_slot.clone(),
            selection: self.selection,
            marquee_hz: self.marquee_hz,
            region_edit_draft: self.region_edit_draft.clone(),
            dry_checklist: self.dry_checklist,
            canvas_mode: self.canvas_mode,
            display_max_hz: self.display_max_hz,
        };
        let bytes = serde_json::to_vec_pretty(&session).map_err(|e| e.to_string())?;
        pipeline::write_bytes_atomic(path, &bytes).map_err(|e| e.to_string())?;
        self.session_path = Some(path.to_path_buf());
        self.status = format!(
            "Saved SpectraCraft v2 session with provenance atomically to {}.",
            path.display()
        );
        Ok(())
    }

    fn load_session(&mut self, path: &Path, ctx: &egui::Context) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("Session file exceeds 8 MiB limit.".into());
        }
        let session: StudioSessionFile =
            serde_json::from_slice(&bytes).map_err(|e| format!("Invalid session JSON: {e}"))?;
        if session.version != 1 && session.version != 2 {
            return Err(format!("Unsupported session version {}.", session.version));
        }
        let mut loaded_rig = session.rig;
        loaded_rig.clamp_all();
        loaded_rig
            .validate(self.sources.len(), self.mics.len(), self.surfaces.len())
            .map_err(|e| format!("Session rig validation failed: {e}"))?;

        let current_source_sha = self.track.as_ref().map(|t| sha256_f32_slice(&t.original));
        let current_dry_sha = self
            .track
            .as_ref()
            .and_then(|t| t.dry.as_deref())
            .map(sha256_f32_slice);
        let source_mismatch = session.version == 2
            && session.source_sha256.is_some()
            && current_source_sha.is_some()
            && session.source_sha256 != current_source_sha;

        let before = self.capture_snapshot();
        self.history.push(before);
        self.rig = loaded_rig;
        self.voicing = session.voicing;
        self.mic_enabled = session.mic_enabled;
        self.room_enabled = session.room_enabled;
        self.plugin_slot = session.plugin_slot;
        self.selection = session.selection;
        self.marquee_hz = session.marquee_hz;
        self.region_edit_draft = session.region_edit_draft;

        // Only restore dry approval if v2 provenance matches the exact loaded dry PCM SHA-256 (Audit H5/H6).
        let approval_matches_active_dry = session.version == 2
            && !source_mismatch
            && session.approved_dry_sha256.is_some()
            && session.approved_dry_sha256 == current_dry_sha;
        if approval_matches_active_dry {
            self.dry_checklist = session.dry_checklist;
            self.dry_review = self.dry_checklist.sync_status();
            self.approved_dry_sha256 = session.approved_dry_sha256;
        } else {
            self.invalidate_dry_review();
        }

        self.canvas_mode = session.canvas_mode;
        self.display_max_hz = session.display_max_hz.clamp(1_000.0, 48_000.0);
        self.session_path = Some(path.to_path_buf());
        self.plot_cache_key = None;
        if self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
            self.sync_post_dry_immediate(ctx);
        }
        self.status = if session.version == 1 {
            format!(
                "Loaded legacy v1 settings preset from {} (dry review reset; open/regenerate source audio explicitly).",
                path.display()
            )
        } else if source_mismatch {
            format!(
                "Loaded v2 rig/settings from {} with WARNING: source audio SHA-256 differs from session provenance (dry approval cleared).",
                path.display()
            )
        } else {
            format!("Loaded SpectraCraft v2 session from {}.", path.display())
        };
        Ok(())
    }

    /// Unified SoundCraft/FigCraft-style command dispatcher for all UI panels, menus, shortcuts, and tests.
    fn execute(&mut self, cmd: StudioCommand, ctx: &egui::Context) -> Result<(), String> {
        match cmd {
            StudioCommand::SelectSource(idx) => {
                if idx >= self.sources.len() {
                    return Err(format!("Source index {idx} out of range"));
                }
                if self.rig.source_index != idx {
                    let before = self.capture_snapshot();
                    self.rig.source_index = idx;
                    self.history.push(before);
                    if self.track.as_ref().is_some_and(|t| t.guitar_cues.is_some()) {
                        self.resynthesize_dry_from_cues(ctx);
                    }
                }
                Ok(())
            }
            StudioCommand::SetVoicing(v) => {
                if self.voicing != v {
                    let before = self.capture_snapshot();
                    self.voicing = v;
                    self.history.push(before);
                    if self.track.as_ref().is_some_and(|t| t.guitar_cues.is_some()) {
                        self.resynthesize_dry_from_cues(ctx);
                    }
                }
                Ok(())
            }
            StudioCommand::RegenerateDry => {
                self.regenerate(ctx);
                Ok(())
            }
            StudioCommand::AnalyzeGuitarDry => {
                self.analyze_guitar_dry(ctx);
                Ok(())
            }
            StudioCommand::ResynthesizeDryFromCues => {
                self.resynthesize_dry_from_cues(ctx);
                Ok(())
            }
            StudioCommand::SetSelection(sel) => {
                if self.selection != sel {
                    let before = self.capture_snapshot();
                    self.selection = sel;
                    if let Some([a, b]) = sel {
                        self.region_edit_draft.start_s = a.min(b);
                        self.region_edit_draft.end_s = a.max(b);
                    }
                    self.history.push(before);
                }
                Ok(())
            }
            StudioCommand::SetMarqueeHz(hz) => {
                if self.marquee_hz != hz {
                    let before = self.capture_snapshot();
                    self.marquee_hz = hz;
                    match hz {
                        Some([lo, hi]) => {
                            self.region_edit_draft.kind = "marquee".into();
                            self.region_edit_draft.min_hz = Some(lo.min(hi));
                            self.region_edit_draft.max_hz = Some(lo.max(hi));
                        }
                        None => {
                            self.region_edit_draft.kind = "range".into();
                            self.region_edit_draft.min_hz = None;
                            self.region_edit_draft.max_hz = None;
                        }
                    }
                    self.history.push(before);
                }
                Ok(())
            }
            StudioCommand::SetRegionEditDraft(draft) => {
                if self.region_edit_draft != draft {
                    let before = self.capture_snapshot();
                    self.region_edit_draft = draft;
                    self.history.push(before);
                }
                Ok(())
            }
            StudioCommand::ApplyRegionEdit => self.apply_region_edit(ctx),
            StudioCommand::SetRegionPreview(mode) => {
                self.region_preview = mode;
                Ok(())
            }
            StudioCommand::SetMicEnabled(on) => {
                if self.mic_enabled != on {
                    let before = self.capture_snapshot();
                    self.mic_enabled = on;
                    self.history.push(before);
                    if self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
                        self.sync_post_dry_immediate(ctx);
                    }
                }
                Ok(())
            }
            StudioCommand::SelectMicSlot(idx) => {
                if idx < self.rig.mics.len() {
                    let before = self.capture_snapshot();
                    self.rig.selected_mic = idx;
                    self.history.push(before);
                }
                Ok(())
            }
            StudioCommand::AddMicSlot => {
                if self.rig.mics.len() < 4 {
                    let before = self.capture_snapshot();
                    self.rig.add_mic_slot();
                    self.history.push(before);
                }
                Ok(())
            }
            StudioCommand::RemoveMicSlot => {
                if self.rig.mics.len() > 1 {
                    let before = self.capture_snapshot();
                    self.rig.remove_selected_mic();
                    self.history.push(before);
                }
                Ok(())
            }
            StudioCommand::SetMicCatalog(idx) => {
                if idx >= self.mics.len() {
                    return Err(format!("Mic catalog index {idx} out of range"));
                }
                let before = self.capture_snapshot();
                self.rig.active_mic_mut().catalog_index = idx;
                self.history.push(before);
                if self.mic_enabled && self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
                    self.sync_post_dry_immediate(ctx);
                }
                Ok(())
            }
            StudioCommand::SetMicPatternOverride(pat) => {
                let before = self.capture_snapshot();
                self.rig.active_mic_mut().pattern_override = pat;
                self.history.push(before);
                if self.mic_enabled && self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
                    self.sync_post_dry_immediate(ctx);
                }
                Ok(())
            }
            StudioCommand::SetMicOffset(offset) => {
                let before = self.capture_snapshot();
                self.rig.active_mic_mut().offset_m = offset;
                self.rig.clamp_all();
                self.history.push(before);
                if (self.mic_enabled || self.room_enabled)
                    && self.track.as_ref().is_some_and(|t| t.dry.is_some())
                {
                    self.sync_post_dry_immediate(ctx);
                }
                Ok(())
            }
            StudioCommand::SetMicAim(aim) => {
                let before = self.capture_snapshot();
                self.rig.active_mic_mut().aim = aim;
                self.history.push(before);
                if (self.mic_enabled || self.room_enabled)
                    && self.track.as_ref().is_some_and(|t| t.dry.is_some())
                {
                    self.sync_post_dry_immediate(ctx);
                }
                Ok(())
            }
            StudioCommand::SetRoomEnabled(on) => {
                if self.room_enabled != on {
                    let before = self.capture_snapshot();
                    self.room_enabled = on;
                    self.history.push(before);
                    if self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
                        self.sync_post_dry_immediate(ctx);
                    }
                }
                Ok(())
            }
            StudioCommand::ApplyRoomPreset(preset) => {
                let before = self.capture_snapshot();
                self.rig.apply_preset(preset);
                self.history.push(before);
                if self.room_enabled && self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
                    self.sync_post_dry_immediate(ctx);
                }
                Ok(())
            }
            StudioCommand::SetRoomDimension { axis, metres } => {
                if axis >= 3 {
                    return Err(format!("Invalid room axis {axis}"));
                }
                let before = self.capture_snapshot();
                self.rig.set_dimension_m(axis, metres);
                self.rig.clamp_all();
                self.history.push(before);
                if self.room_enabled && self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
                    self.sync_post_dry_immediate(ctx);
                }
                Ok(())
            }
            StudioCommand::SetSurfaceFace { face, surface_idx } => {
                if face >= 6 || surface_idx >= self.surfaces.len() {
                    return Err("Surface face or index out of range".into());
                }
                let before = self.capture_snapshot();
                self.rig.surface_faces[face] = surface_idx;
                self.history.push(before);
                if self.room_enabled && self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
                    self.sync_post_dry_immediate(ctx);
                }
                Ok(())
            }
            StudioCommand::SetAllWalls(surface_idx) => {
                if surface_idx >= self.surfaces.len() {
                    return Err("Surface index out of range".into());
                }
                let before = self.capture_snapshot();
                self.rig.set_all_walls(surface_idx);
                self.history.push(before);
                if self.room_enabled && self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
                    self.sync_post_dry_immediate(ctx);
                }
                Ok(())
            }
            StudioCommand::SetTemperatureC(temp_c) => {
                let before = self.capture_snapshot();
                self.rig.temperature_c = temp_c.clamp(-20.0, 50.0);
                self.history.push(before);
                if self.room_enabled && self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
                    self.sync_post_dry_immediate(ctx);
                }
                Ok(())
            }
            StudioCommand::SetPluginSlot(slot) => {
                if self.plugin_slot != slot {
                    let before = self.capture_snapshot();
                    self.plugin_slot = slot;
                    self.history.push(before);
                    if self.track.as_ref().is_some_and(|t| t.dry.is_some()) {
                        self.sync_post_dry_immediate(ctx);
                    }
                }
                Ok(())
            }
            StudioCommand::SetDryChecklist(checklist) => {
                self.dry_checklist = checklist;
                self.sync_dry_review_from_checklist();
                Ok(())
            }
            StudioCommand::RunPostDryPipeline => {
                self.capture(ctx);
                Ok(())
            }
            // Display-only & transport actions (NEVER push DSP state or mutate audio buffers!)
            StudioCommand::TogglePlay => {
                if self.play.is_some() {
                    self.stop();
                } else {
                    self.start();
                }
                Ok(())
            }
            StudioCommand::Seek(t) => {
                self.seek_to(t);
                Ok(())
            }
            StudioCommand::SwitchTake(t) => {
                self.switch(t);
                Ok(())
            }
            StudioCommand::ZoomToSelection => {
                self.zoom_to_selection();
                Ok(())
            }
            StudioCommand::ZoomToResult => {
                self.zoom_to_result();
                Ok(())
            }
            StudioCommand::FitRecording => {
                self.fit_recording();
                Ok(())
            }
            StudioCommand::ZoomTimeline {
                factor,
                anchor_frac,
            } => {
                self.zoom_timeline(factor, anchor_frac);
                Ok(())
            }
            StudioCommand::SetDisplayUnit(u) => {
                self.rig.unit = u;
                Ok(())
            }
            StudioCommand::SetCanvasMode(mode) => {
                self.canvas_mode = mode;
                Ok(())
            }
            StudioCommand::SetDisplayMaxHz(hz) => {
                self.display_max_hz = hz.clamp(1_000.0, 48_000.0);
                self.plot_cache_key = None;
                Ok(())
            }
            StudioCommand::SetShowPatterns(show) => {
                self.rig.show_patterns = show;
                Ok(())
            }
            StudioCommand::Undo => {
                self.undo(ctx);
                Ok(())
            }
            StudioCommand::Redo => {
                self.redo(ctx);
                Ok(())
            }
            StudioCommand::SaveSession(path) => self.save_session(&path),
            StudioCommand::LoadSession(path) => self.load_session(&path, ctx),
        }
    }

    /// Headless structured workspace inspection (inspired by SoundCraft's `ui.inspect`).
    #[cfg_attr(not(test), allow(dead_code))]
    fn inspect_state_json(&self) -> serde_json::Value {
        let pred = self.effective_prediction();
        let active_slot = self.rig.active_mic();
        let mic = self.mics.get(active_slot.catalog_index);
        let eff_pattern = active_slot.effective_pattern(&self.mics);
        let src = self.sources.get(self.rig.source_index);
        let plugin_active =
            self.plugin_slot.enabled && self.plugin_slot.plugin_id != PluginId::None;
        serde_json::json!({
            "app_state": self.app_state.label(),
            "status": self.status,
            "loaded_file": self.track.as_ref().map(|t| serde_json::json!({
                "name": t.name,
                "sample_rate": t.rate,
                "duration_s": t.duration(),
                "channels": t.recording.channels,
                "has_dry": t.dry.is_some(),
                "has_rebuilt": t.rebuilt.is_some(),
                "has_layers": t.layers.is_some(),
                "has_guitar_cues": t.guitar_cues.is_some(),
            })),
            "pipeline": {
                "stage_1_dry_voicing": &self.voicing,
                "stage_2_marquee_hz": self.marquee_hz,
                "stage_2_region_edit_draft": &self.region_edit_draft,
                "stage_2_last_edit": self.last_region_edit.as_ref().map(|r| serde_json::json!({
                    "start_sample": r.start_sample,
                    "end_sample": r.end_sample,
                    "affected_notes": r.affected_notes,
                    "effective_action": r.effective_action,
                    "unresolved_note": r.unresolved_note,
                    "outside_bit_identical": r.outside_bit_identical,
                })),
                "stage_3_mic_enabled": self.mic_enabled,
                "stage_4_room_enabled": self.room_enabled,
                "stage_5_plugin_active": plugin_active,
                "stage_5_plugin_slot": &self.plugin_slot,
                "last_summary": &self.last_pipeline_summary,
            },
            "transport": {
                "playing": self.play.is_some(),
                "take": self.take.name(),
                "region_preview": self.region_preview.label(),
                "cursor_s": self.cursor,
                "view_range_s": self.view_range,
                "selection_s": self.selection,
            },
            "dry_review": self.dry_review.label(),
            "dry_listening_checklist": {
                "rhythm": self.dry_checklist.rhythm,
                "phrasing": self.dry_checklist.phrasing,
                "dynamics": self.dry_checklist.dynamics,
                "sustain": self.dry_checklist.sustain,
                "artifact_free": self.dry_checklist.artifact_free,
                "passed_count": self.dry_checklist.passed_count(),
            },
            "history": {
                "can_undo": self.history.can_undo(),
                "can_redo": self.history.can_redo(),
            },
            "catalog_counts": {
                "sources": self.sources.len(),
                "mics_curated_factual": self.mics.len(),
                "surfaces_6band": self.surfaces.len(),
            },
            "rig": {
                "source_index": self.rig.source_index,
                "source": src.map(|s| s.name.as_str()),
                "room_preset": self.rig.room_preset.label(),
                "dimensions_m": self.rig.dimensions_m,
                "temperature_c": self.rig.temperature_c,
                "unit": self.rig.unit.suffix(),
                "surface_faces": self.rig.surface_faces.map(|i| self.surfaces.get(i).map_or("unknown", |s| s.id.as_str())),
                "active_mic": mic.map(|m| m.display_name()),
                "effective_pattern": eff_pattern.label(),
                "pattern_override": active_slot.pattern_override.map(|p| p.label()),
                "mic_offset_m": active_slot.offset_m,
                "mic_aim_dir": active_slot.aim.direction(),
                "show_patterns": self.rig.show_patterns,
            },
            "prediction": {
                "speed_of_sound_mps": pred.speed_of_sound_mps,
                "distance_m": pred.distance_m,
                "direct_arrival_ms": pred.direct_arrival_ms,
                "first_reflection_ms": pred.first_reflection_ms,
                "reflected_path_db": pred.reflected_path_db,
                "six_band_drr_db": pred.six_band_drr_db,
                "reflection_count": pred.reflection_count,
            }
        })
    }

    fn begin_job(
        &mut self,
        cancel_existing: bool,
    ) -> Option<(u64, Arc<AtomicBool>, mpsc::Sender<Finished>)> {
        if !cancel_existing && self.job.is_some() {
            self.status = "Another background job is already running; wait for it to finish.".into();
            return None;
        }
        if let Some(prev_cancel) = self.job_cancel.take() {
            prev_cancel.store(true, Ordering::Relaxed);
        }
        self.active_job_id = self.active_job_id.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        self.job_cancel = Some(Arc::clone(&cancel));
        self.job = Some((self.active_job_id, rx));
        Some((self.active_job_id, cancel, tx))
    }

    fn stop(&mut self) {
        if let Some(p) = self.play.take() {
            self.cursor = p.stop(self.cursor);
        }
    }

    fn start(&mut self) {
        // Always stop and release any active OS playback stream before opening a new one.
        self.stop();
        let Some(t) = &self.track else {
            return;
        };
        let (mut wave, started_at) = match (self.region_preview, &self.last_region_edit) {
            (RegionPreviewMode::Before, Some(rep)) if !rep.before_slice.is_empty() => (
                rep.before_slice.clone(),
                rep.start_sample as f64 / t.rate as f64,
            ),
            (RegionPreviewMode::After, Some(rep)) if !rep.after_slice.is_empty() => (
                rep.after_slice.clone(),
                rep.start_sample as f64 / t.rate as f64,
            ),
            _ => {
                let total = t.original.len();
                let start = (self.cursor * t.rate as f64)
                    .round()
                    .clamp(0., total as f64) as usize;
                let start = if start >= total { 0 } else { start };
                let Some(w) = t.audition_slice_with_options(
                    self.take,
                    self.audition_rms_match,
                    self.preview_bit_depth,
                    start,
                    None,
                ) else {
                    return;
                };
                (w, start as f64 / t.rate as f64)
            }
        };
        if wave.is_empty() {
            return;
        }
        if self.region_preview != RegionPreviewMode::Off {
            let peak = wave.iter().fold(0.89_f32, |p, &x| p.max(x.abs()));
            let g = 0.89 / peak;
            for s in &mut wave {
                *s *= g;
            }
        }
        match AudioPlayback::play_mono(t.rate, wave, started_at) {
            Ok(playback) => {
                self.play = Some(playback);
            }
            Err(e) => self.status = format!("Playback unavailable: {e}"),
        }
    }

    fn seek_to(&mut self, seconds: f64) {
        if !seconds.is_finite() {
            return;
        }
        let duration = self.track.as_ref().map_or(0.0, Track::duration);
        if duration <= 0.0 {
            return;
        }
        let target = seconds.clamp(0.0, duration);
        let playing = self.play.is_some();
        self.stop();
        self.cursor = target;
        if playing && target < duration {
            self.start();
        }
    }

    fn switch(&mut self, take: Take) {
        if take == self.take {
            return;
        }
        if take != Take::Original && self.track.as_ref().is_none_or(|t| t.rebuilt.is_none()) {
            return;
        }
        let playing = self.play.is_some();
        self.stop();
        self.take = take;
        if playing {
            self.start();
        }
    }

    /// Active excerpt sample span `[start, end]`, bounded to `<= 30 s` for desktop regeneration.
    fn target_excerpt_samples(&self) -> Result<[usize; 2], String> {
        let Some(t) = &self.track else {
            return Err("Open a WAV file first.".into());
        };
        if t.original.is_empty() {
            return Err("Loaded audio is empty.".into());
        }
        let (a_s, b_s) = match self.selection {
            Some([a, b]) if a.is_finite() && b.is_finite() && (b - a).abs() >= 0.05 => {
                (a.min(b), a.max(b))
            }
            _ => (0.0, t.duration().min(MAX_EXCERPT_SECS)),
        };
        let dur = b_s - a_s;
        if dur > MAX_EXCERPT_SECS + 1e-3 {
            return Err(format!(
                "Selected excerpt is {dur:.2} s. Desktop excerpt regeneration is bounded to <= {MAX_EXCERPT_SECS:.0} s; drag a shorter passage on the timeline."
            ));
        }
        let s0 = ((a_s * t.rate as f64).round() as usize).min(t.original.len().saturating_sub(1));
        let s1 = ((b_s * t.rate as f64).round() as usize).clamp(s0 + 1, t.original.len());
        Ok([s0, s1])
    }

    fn fit_recording(&mut self) {
        if let Some(t) = &self.track {
            self.view_range = [0.0, t.duration().max(0.01)];
            self.plot_cache_key = None;
        }
    }

    fn zoom_to_selection(&mut self) {
        if let (Some(t), Some([a, b])) = (&self.track, self.selection) {
            let lo = a.min(b).clamp(0.0, t.duration());
            let hi = b.max(a).clamp(0.0, t.duration());
            if hi - lo >= 0.02 {
                self.view_range = [lo, hi];
                self.plot_cache_key = None;
            }
        }
    }

    fn zoom_to_result(&mut self) {
        if let Some(span) = self.track.as_ref().and_then(Track::rebuilt_span_secs) {
            if span[1] - span[0] >= 0.02 {
                self.view_range = span;
                self.plot_cache_key = None;
            }
        }
    }

    /// Zoom timeline horizontally without moving `self.cursor` or altering audio.
    fn zoom_timeline(&mut self, factor: f64, anchor_frac: f64) {
        let Some(t) = &self.track else {
            return;
        };
        let dur = t.duration().max(0.01);
        let [lo, hi] = self.view_range;
        let span = (hi - lo).max(0.02);
        let new_span = (span * factor).clamp(0.05_f64.min(dur), dur);
        let anchor_t = lo + span * anchor_frac.clamp(0.0, 1.0);
        let mut new_lo = anchor_t - new_span * anchor_frac.clamp(0.0, 1.0);
        let mut new_hi = new_lo + new_span;
        if new_lo < 0.0 {
            new_hi = (new_hi - new_lo).min(dur);
            new_lo = 0.0;
        }
        if new_hi > dur {
            new_lo = (new_lo - (new_hi - dur)).max(0.0);
            new_hi = dur;
        }
        self.view_range = [new_lo, new_hi];
        self.plot_cache_key = None;
    }

    fn load(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.status = format!("Opening {}…", path.display());
        self.app_state = AppState::Loading;
        self.source_path = Some(path.clone());
        self.post_dry_job_rev = self.post_dry_job_rev.wrapping_add(1);
        self.post_dry_worker = None;
        self.stop();
        let Some((_, cancel, tx)) = self.begin_job(true) else {
            return;
        };
        let ctx = ctx.clone();
        let display_max_hz = self.display_max_hz;
        std::thread::spawn(move || {
            let result = capture::load_wav_for_studio(&path)
                .map(|(rate, original, recording)| {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("Load cancelled.".to_owned());
                    }
                    let overview_peaks = measure_peaks(&original);
                    let initial_view = Visual::measure_range(
                        &original,
                        rate,
                        display_max_hz.min(rate as f32 * 0.5),
                    );
                    Ok(Track {
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        rate,
                        overview_peaks,
                        original_view: initial_view,
                        regenerated_view: None,
                        difference_view: None,
                        original: original.into(),
                        rebuilt_span: None,
                        rebuilt: None,
                        dry: None,
                        layer_plot_only: None,
                        recording,
                        layers: None,
                        guitar_cues: None,
                    })
                })
                .map_err(|e| e.to_string())
                .and_then(|r| r);
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Finished::Load(result));
                ctx.request_repaint();
            }
        });
    }

    fn regenerate(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            self.status = "Wait for the current background job to finish before regenerating.".into();
            return;
        }
        let span = match self.target_excerpt_samples() {
            Ok(s) => s,
            Err(e) => {
                self.status = e;
                self.app_state = AppState::Error;
                return;
            }
        };
        let Some(t) = &self.track else {
            return;
        };
        let rate = t.rate;
        let excerpt: Vec<f32> = t.original[span[0]..span[1]].to_vec();
        let include_texture = self.include_texture;
        let Some((_, cancel, tx)) = self.begin_job(false) else {
            return;
        };
        self.app_state = AppState::Busy;
        self.status = if include_texture {
            format!(
                "Regenerating tonal + modeled texture ({:.2}–{:.2} s)…",
                span[0] as f64 / rate as f64,
                span[1] as f64 / rate as f64
            )
        } else {
            format!(
                "Regenerating tonal excerpt ({:.2}–{:.2} s)…",
                span[0] as f64 / rate as f64,
                span[1] as f64 / rate as f64
            )
        };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result =
                analyze_with_texture(&excerpt, rate, include_texture).map_err(|e| e.to_string());
            drop(excerpt);
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let result = result.and_then(|m| synthesize(&m, 12345).map_err(|e| e.to_string()));
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Finished::Render(result.map(|w| (span, w))));
                ctx.request_repaint();
            }
        });
    }

    fn analyze_guitar_dry(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            self.status = "Wait for the current background job to finish before running Stage 1.".into();
            return;
        }
        let span = match self.target_excerpt_samples() {
            Ok(s) => s,
            Err(e) => {
                self.status = e;
                self.app_state = AppState::Error;
                return;
            }
        };
        let Some(t) = &self.track else {
            return;
        };
        let rate = t.rate;
        let excerpt: Vec<f32> = t.original[span[0]..span[1]].to_vec();
        let source = self
            .sources
            .get(self.rig.source_index)
            .cloned()
            .unwrap_or_else(|| self.sources[0].clone());
        let voicing = self.voicing.clone();
        let Some((_, cancel, tx)) = self.begin_job(false) else {
            return;
        };
        self.app_state = AppState::Busy;
        self.status = format!(
            "Stage 1: extracting performance shape ({:.2}–{:.2} s) & synthesizing dry {} take…",
            span[0] as f64 / rate as f64,
            span[1] as f64 / rate as f64,
            source.name
        );
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> spectracraft_mono::Result<(
                [usize; 2],
                reperform::Performance,
                Vec<f32>,
            )> {
                let cues = reperform::analyze_guitar(&excerpt, rate)?;
                drop(excerpt);
                if cancel.load(Ordering::Relaxed) {
                    return Err("Stage 1 cancelled.".into());
                }
                let dry = cues.synthesize_dry_for_source_with_voicing(&source, &voicing)?;
                Ok((span, cues, dry))
            })()
            .map_err(|e| e.to_string());
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Finished::GuitarDry(result));
                ctx.request_repaint();
            }
        });
    }

    /// Re-synthesize Stage 1 dry audio directly from cached performance cues when Source
    /// or Stage-1 Voicing changes (never reopens original PCM).
    fn resynthesize_dry_from_cues(&mut self, ctx: &egui::Context) {
        let Some(t) = self.track.as_ref() else {
            return;
        };
        let Some(cues) = t.guitar_cues.as_ref() else {
            self.status = "Extract performance cues (Stage 1) before re-voicing dry take.".into();
            return;
        };
        let span = t.rebuilt_span.unwrap_or([0, cues.samples()]);
        let source = self
            .sources
            .get(self.rig.source_index)
            .cloned()
            .unwrap_or_else(|| self.sources[0].clone());
        let voicing = self.voicing.clone();
        match cues.synthesize_dry_for_source_with_voicing(&source, &voicing) {
            Ok(dry) => {
                if let Some(t) = self.track.as_mut() {
                    t.layers = None;
                    t.layer_plot_only = None;
                    t.rebuilt_span = Some(span);
                    t.dry = Some(Arc::from(dry));
                }
                self.last_region_edit = None;
                self.region_preview = RegionPreviewMode::Off;
                self.invalidate_dry_review();
                self.sync_post_dry_immediate(ctx);
                self.status = format!(
                    "Stage 1 dry take re-voiced for '{}' from cached shape (original untouched; dry review reset).",
                    source.name
                );
            }
            Err(e) => {
                self.app_state = AppState::Error;
                self.status = format!("Stage 1 dry re-voicing failed: {e}");
            }
        }
    }

    /// Stage 2 — Apply localized surgical edit strictly to the cached dry take before capture.
    fn apply_region_edit(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(t) = self.track.as_ref() else {
            return Err("Open a WAV file first.".into());
        };
        let Some(dry_arc) = t.dry.clone() else {
            return Err("Generate a Stage 1 dry take before surgical region editing.".into());
        };
        let Some(cues) = t.guitar_cues.as_ref() else {
            return Err(
                "Stage 2 surgical editing requires extracted Stage 1 performance cues (Analyze Dry)."
                    .into(),
            );
        };
        let source = self
            .sources
            .get(self.rig.source_index)
            .ok_or_else(|| "Selected source index invalid.".to_owned())?;
        let mut edit = self.region_edit_draft.clone();
        let span_offset_s = t
            .rebuilt_span
            .map_or(0.0, |[s0, _]| s0 as f64 / t.rate as f64);
        if let Some([a, b]) = self.selection {
            edit.start_s = (a.min(b) - span_offset_s).max(0.0);
            edit.end_s = (a.max(b) - span_offset_s).max(0.0);
        }
        if let Some([flo, fhi]) = self.marquee_hz {
            edit.kind = "marquee".into();
            edit.min_hz = Some(flo.min(fhi));
            edit.max_hz = Some(flo.max(fhi));
        } else {
            edit.kind = "range".into();
            edit.min_hz = None;
            edit.max_hz = None;
        }

        let out = region_edit::apply_dry_region_edit(&dry_arc, cues, source, &self.voicing, &edit)
            .map_err(|e| e.to_string())?;

        let report = RegionEditReport {
            start_sample: out.start_sample,
            end_sample: out.end_sample,
            affected_notes: out.affected_notes,
            effective_action: out.effective_action,
            unresolved_note: out.unresolved_note.clone(),
            outside_bit_identical: out.outside_bit_identical,
            before_slice: out.before_slice,
            after_slice: out.after_slice,
        };

        if let Some(msg) = out.unresolved_note {
            self.last_region_edit = Some(report);
            self.status = format!("Stage 2 no-op: {msg}");
            return Ok(());
        }

        let before = self.capture_snapshot();
        self.history.push(before);
        if let Some(t) = self.track.as_mut() {
            t.dry = Some(Arc::from(out.edited_dry));
        }
        self.last_region_edit = Some(report);
        self.invalidate_dry_review();
        self.sync_post_dry_immediate(ctx);
        self.status = format!(
            "Stage 2 surgical edit ({:?}) applied to dry take ({} note(s), outside bit-identical: {}; dry review reset).",
            out.effective_action, out.affected_notes, out.outside_bit_identical
        );
        Ok(())
    }

    /// Synchronize `t.rebuilt` from `t.dry` via Stages 3, 4, 5 (`pipeline::run_post_dry_pipeline`).
    /// When all post-dry stages are bypassed (or dry review is unapproved), updates immediately with
    /// zero DSP cost; when active post-dry DSP is enabled, dispatches to a background worker thread
    /// with monotonic revision coalescing so heavy FDN/mic rendering never blocks the egui thread (Audit M4).
    fn sync_post_dry_immediate(&mut self, ctx: &egui::Context) {
        let Some(t) = self.track.as_ref() else {
            return;
        };
        let Some(dry) = t.dry.clone() else {
            return;
        };
        let rate = t.rate;
        let span = t.rebuilt_span.unwrap_or([0, dry.len()]);
        let cfg = self.post_dry_config();
        let any_post_active = cfg.mic_enabled
            || cfg.room_enabled
            || (cfg.plugin_slot.enabled && cfg.plugin_slot.plugin_id != PluginId::None);

        let effective_cfg = if any_post_active
            && self.dry_review != DryReviewStatus::ApprovedByHuman
        {
            // Respect the 5-criterion human dry-review gate: keep output strictly dry until approved.
            let mut bypassed = cfg.clone();
            bypassed.mic_enabled = false;
            bypassed.room_enabled = false;
            bypassed.plugin_slot.enabled = false;
            self.status = format!(
                "Stage setting saved, but post-dry audio stays dry until dry listening check passes ({}/5).",
                self.dry_checklist.passed_count()
            );
            bypassed
        } else {
            cfg.clone()
        };

        let effective_active = effective_cfg.mic_enabled
            || effective_cfg.room_enabled
            || (effective_cfg.plugin_slot.enabled
                && effective_cfg.plugin_slot.plugin_id != PluginId::None);

        // Invalidate any in-flight background post-dry render before dispatching/applying
        self.post_dry_job_rev = self.post_dry_job_rev.wrapping_add(1);
        let rev = self.post_dry_job_rev;

        if effective_active {
            // Off-UI-thread background render with revision coalescing (Audit M4)
            let (tx, rx) = mpsc::channel();
            self.post_dry_worker = Some((rev, rx));
            let sources = self.sources.clone();
            let mics = self.mics.clone();
            let surfaces = self.surfaces.clone();
            let ctx_bg = ctx.clone();
            std::thread::spawn(move || {
                let out = pipeline::run_post_dry_pipeline(
                    &dry,
                    rate,
                    &effective_cfg,
                    &sources,
                    &mics,
                    &surfaces,
                );
                let _ = tx.send((effective_cfg, span, out));
                ctx_bg.request_repaint();
            });
            return;
        }

        // Fast bypass path: bit-identical copy of dry take, zero DSP blocking
        self.post_dry_worker = None;
        let out = pipeline::run_post_dry_pipeline(
            &dry,
            rate,
            &effective_cfg,
            &self.sources,
            &self.mics,
            &self.surfaces,
        );
        self.apply_post_dry_output(effective_cfg, span, out, any_post_active, ctx);
    }

    fn apply_post_dry_output(
        &mut self,
        effective_cfg: PostDryPipelineConfig,
        span: [usize; 2],
        out: pipeline::PipelineRenderOutput,
        any_post_active: bool,
        ctx: &egui::Context,
    ) {
        let was_playing = self.play.is_some();
        let keep_cursor = self.cursor;
        if was_playing && self.take != Take::Original {
            self.stop();
        }
        if let Some(t) = self.track.as_mut() {
            t.layer_plot_only = None;
            t.rebuilt_span = Some(span);
            t.rebuilt = Some(out.samples);
        }
        if !any_post_active || self.dry_review == DryReviewStatus::ApprovedByHuman {
            self.applied_rig = Some(effective_cfg.rig.clone());
        }
        self.last_pipeline_summary = Some(out.summary);
        self.cursor = keep_cursor;
        if was_playing && self.take != Take::Original {
            self.start();
        }
        self.plot_revision += 1;
        self.plot_cache_key = None;
        self.refresh_viewport_visuals(ctx);
    }

    fn capture(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            self.status = "Wait for the current background job to finish before running Stages 3–5.".into();
            return;
        }
        if self.dry_review != DryReviewStatus::ApprovedByHuman {
            self.status = format!(
                "Post-processing locked: complete all 5 human dry listening checks ({}/5) before running Stages 3–5.",
                self.dry_checklist.passed_count()
            );
            return;
        }
        let Some(t) = self.track.as_ref() else {
            return;
        };
        let Some(dry) = t.dry.clone() else {
            return;
        };
        let span = t.rebuilt_span.unwrap_or([0, dry.len()]);
        let rate = t.rate;
        let cfg = self.post_dry_config();
        let sources = self.sources.clone();
        let mics = self.mics.clone();
        let surfaces = self.surfaces.clone();
        let Some((_, cancel, tx)) = self.begin_job(false) else {
            return;
        };
        // Do NOT reset self.cursor! Preserve playhead across post-dry pipeline rerenders.
        self.app_state = AppState::Busy;
        self.status = format!(
            "Running post-dry pipeline (Mic: {}, Room: {}, Plugin: {}) from cached dry take…",
            if cfg.mic_enabled { "ON" } else { "BYPASS" },
            if cfg.room_enabled {
                cfg.rig.room_preset.label()
            } else {
                "BYPASS"
            },
            if cfg.plugin_slot.enabled && cfg.plugin_slot.plugin_id != PluginId::None {
                format!("{:?}", cfg.plugin_slot.plugin_id)
            } else {
                "BYPASS".into()
            }
        );
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let out = pipeline::run_post_dry_pipeline(&dry, rate, &cfg, &sources, &mics, &surfaces);
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Finished::Capture(cfg, span, out));
                ctx.request_repaint();
            }
        });
    }

    fn refresh_viewport_visuals(&mut self, ctx: &egui::Context) {
        let Some(t) = self.track.as_mut() else {
            return;
        };
        if t.original.is_empty() {
            return;
        }
        let dur = t.duration().max(0.01);
        let [raw0, raw1] = self.view_range;
        let a = if raw0.is_finite() { raw0 } else { 0.0 };
        let b = if raw1.is_finite() { raw1 } else { dur };
        let v0 = a.min(b).clamp(0.0, dur);
        let v1 = a.max(b).clamp(v0, dur);
        let key = (
            (v0 * 1000.0).round() as u64,
            (v1 * 1000.0).round() as u64,
            self.display_max_hz.round() as i32,
            self.plot_revision,
        );
        if self.plot_cache_key == Some(key) {
            return;
        }
        let s0 = ((v0 * t.rate as f64).floor() as usize).min(t.original.len().saturating_sub(1));
        let s1 = ((v1 * t.rate as f64).ceil() as usize).clamp(s0 + 1, t.original.len());
        let orig_vis = Visual::measure_range(&t.original[s0..s1], t.rate, self.display_max_hz);
        self.original_texture = Some(ctx.load_texture(
            "Original spectrum",
            orig_vis.spectrum.image(),
            egui::TextureOptions::NEAREST,
        ));
        t.original_view = orig_vis;

        let regen_slice: Option<Cow<'_, [f32]>> = if let Some(plot_only) = &t.layer_plot_only {
            let [r0, _] = t.rebuilt_span.unwrap_or([0, plot_only.len()]);
            Some(aligned_excerpt_slice(r0, plot_only, s0, s1))
        } else {
            t.aligned_rebuilt_slice(s0, s1)
        };
        if let Some(slice) = regen_slice {
            let reg_vis = Visual::measure_range(&slice, t.rate, self.display_max_hz);
            self.regenerated_texture = Some(ctx.load_texture(
                "Regenerated spectrum",
                reg_vis.spectrum.image(),
                egui::TextureOptions::NEAREST,
            ));
            t.regenerated_view = Some(reg_vis);
        } else {
            self.regenerated_texture = None;
            t.regenerated_view = None;
        }

        if let Some(diff_slice) = t.aligned_difference_slice(s0, s1) {
            let diff_vis = Visual::measure_range(&diff_slice, t.rate, self.display_max_hz);
            self.difference_texture = Some(ctx.load_texture(
                "Difference spectrum",
                diff_vis.spectrum.image(),
                egui::TextureOptions::NEAREST,
            ));
            t.difference_view = Some(diff_vis);
        } else {
            self.difference_texture = None;
            t.difference_view = None;
        }
        self.plot_cache_key = Some(key);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        // Drain background post-dry worker completions, discarding stale revisions (Audit M4).
        let completed_post = if let Some((rev, rx)) = &self.post_dry_worker {
            match rx.try_recv() {
                Ok(payload) => Some((*rev, Ok(payload))),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some((*rev, Err(()))),
            }
        } else {
            None
        };
        if let Some((rev, res)) = completed_post {
            self.post_dry_worker = None;
            if rev == self.post_dry_job_rev {
                if let Ok((cfg, span, out)) = res {
                    self.apply_post_dry_output(cfg, span, out, true, ctx);
                }
            }
        }

        let (job_id, result) = {
            let Some((id, rx)) = &self.job else {
                return;
            };
            let id = *id;
            match rx.try_recv() {
                Ok(r) => (id, r),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(_) => {
                    self.job = None;
                    self.job_cancel = None;
                    self.app_state = AppState::Error;
                    self.status = "Processing stopped unexpectedly.".into();
                    return;
                }
            }
        };
        self.job = None;
        self.job_cancel = None;
        if job_id != self.active_job_id {
            return;
        }
        match result {
            Finished::Load(Ok(t)) => {
                self.stop();
                let dur = t.duration();
                self.history.clear();
                self.display_max_hz = 12_000_f32.min(t.rate as f32 * 0.5);
                self.view_range = [0.0, dur.max(0.01)];
                self.selection = if dur > MAX_EXCERPT_SECS {
                    Some([0.0, MAX_EXCERPT_SECS])
                } else {
                    None
                };
                self.marquee_hz = None;
                self.last_region_edit = None;
                self.region_preview = RegionPreviewMode::Off;
                self.last_pipeline_summary = None;
                self.applied_rig = None;
                let channel_note = if t.recording.channels > 1 {
                    format!(
                        " [{} ch source folded equally to mono for analysis]",
                        t.recording.channels
                    )
                } else {
                    String::new()
                };
                self.invalidate_dry_review();
                self.track = Some(t);
                self.cursor = 0.;
                self.take = Take::Original;
                self.plot_revision += 1;
                self.plot_cache_key = None;
                self.app_state = AppState::Ready;
                self.status = if dur > MAX_EXCERPT_SECS {
                    format!(
                        "Loaded long recording ({dur:.2} s){channel_note}. Navigate full overview; drag any <= 30 s excerpt to regenerate."
                    )
                } else {
                    format!(
                        "Loaded{channel_note}. Ready for Stage 1 dry regeneration or editable layers."
                    )
                };
                self.refresh_viewport_visuals(ctx);
            }
            Finished::Render(Ok((span, w))) => {
                if let Some(t) = self.track.as_mut() {
                    t.layers = None;
                    t.layer_plot_only = None;
                    t.rebuilt_span = Some(span);
                    t.dry = Some(Arc::from(w.clone()));
                    t.rebuilt = Some(w);
                    self.applied_rig = None;
                    self.last_pipeline_summary = None;
                    self.last_region_edit = None;
                    self.dry_checklist = DryListeningChecklist::default();
                    self.dry_review = DryReviewStatus::NotReviewed;
                }
                self.plot_revision += 1;
                self.plot_cache_key = None;
                self.app_state = AppState::Ready;
                self.status =
                    "Stage 1 dry candidate ready (Not reviewed). Export keeps source rate and excerpt length."
                        .into();
                self.refresh_viewport_visuals(ctx);
            }
            Finished::Layers(Ok(session)) => {
                if let Some(t) = self.track.as_mut() {
                    t.rebuilt_span = Some(session.span);
                    t.layers = Some(session);
                }
                self.dry_checklist = DryListeningChecklist::default();
                self.dry_review = DryReviewStatus::NotReviewed;
                self.app_state = AppState::Ready;
                self.remix_layers(ctx);
            }
            Finished::GuitarDry(Ok((span, cues, w))) => {
                let summary = cues.summary();
                if let Some(t) = self.track.as_mut() {
                    t.layers = None;
                    t.layer_plot_only = None;
                    t.rebuilt_span = Some(span);
                    t.dry = Some(Arc::from(w.clone()));
                    t.rebuilt = Some(w);
                    t.guitar_cues = Some(cues);
                    self.applied_rig = None;
                    self.last_pipeline_summary = None;
                    self.last_region_edit = None;
                    self.dry_checklist = DryListeningChecklist::default();
                    self.dry_review = DryReviewStatus::NotReviewed;
                }
                self.plot_revision += 1;
                self.plot_cache_key = None;
                self.app_state = AppState::Ready;
                self.status = format!(
                    "Stage 1 dry take ({}) ready: {} notes ({} re-picks). Frames: {} supported, {} uncertain, {} out-of-target.",
                    self.dry_review.label(),
                    summary.note_count,
                    summary.total_repicks,
                    summary.supported_frames,
                    summary.uncertain_frames,
                    summary.out_of_target_frames
                );
                self.refresh_viewport_visuals(ctx);
            }
            Finished::DryResynth(Ok((span, w))) => {
                if let Some(t) = self.track.as_mut() {
                    t.layers = None;
                    t.layer_plot_only = None;
                    t.rebuilt_span = Some(span);
                    t.dry = Some(Arc::from(w));
                }
                self.invalidate_dry_review();
                self.app_state = AppState::Ready;
                self.sync_post_dry_immediate(ctx);
            }
            Finished::Capture(cfg, span, out) => {
                let was_playing = self.play.is_some();
                let keep_cursor = self.cursor;
                if was_playing && self.take != Take::Original {
                    self.stop();
                }
                if let Some(t) = self.track.as_mut() {
                    t.layer_plot_only = None;
                    t.rebuilt_span = Some(span);
                    t.rebuilt = Some(out.samples);
                    self.applied_rig = Some(cfg.rig.clone());
                }
                let bit_id = out.summary.bit_identical_to_dry;
                let mic_on = out.summary.mic_stage_active;
                let room_on = out.summary.room_stage_active;
                let plug_on = out.summary.plugin_slot_active;
                self.last_pipeline_summary = Some(out.summary);
                self.cursor = keep_cursor;
                if was_playing && self.take != Take::Original {
                    self.start();
                }
                self.plot_revision += 1;
                self.plot_cache_key = None;
                self.app_state = AppState::Ready;
                self.status = if bit_id {
                    "Post-dry pipeline complete: all optional stages bypassed (bit-identical to dry take).".into()
                } else {
                    format!(
                        "Post-dry pipeline complete (Mic: {}, Room: {}, Plugin: {}). Playhead preserved.",
                        if mic_on { "ON" } else { "BYPASS" },
                        if room_on { cfg.rig.room_preset.label() } else { "BYPASS" },
                        if plug_on { "ON" } else { "BYPASS" }
                    )
                };
                self.refresh_viewport_visuals(ctx);
            }
            Finished::Load(Err(e))
            | Finished::Render(Err(e))
            | Finished::Layers(Err(e))
            | Finished::GuitarDry(Err(e))
            | Finished::DryResynth(Err(e)) => {
                self.status = e;
                self.app_state = AppState::Error;
                if self.smoke.take().is_some() {
                    eprintln!("{}", self.status);
                    std::process::exit(1);
                }
            }
        }
    }

    fn export(&mut self, path: &Path) -> bool {
        let Some(t) = &self.track else {
            return false;
        };
        let Some(y) = t.rebuilt.as_deref() else {
            return false;
        };
        let result = write_wav(t.rate, y, path);
        let success = result.is_ok();
        self.status = match result {
            Ok(g) => format!(
                "Exported float32 mono WAV ({:.2} s, export gain {:.1} dB). Original untouched.",
                y.len() as f64 / t.rate as f64,
                20. * g.log10()
            ),
            Err(e) => {
                self.app_state = AppState::Error;
                format!("Export did not complete: {e}")
            }
        };
        success
    }

    fn export_selected_variants(&mut self, out_dir: &Path) -> bool {
        let Some(t) = &self.track else {
            return false;
        };
        let Some(cues) = &t.guitar_cues else {
            self.status = "Run Guitar Dry analysis before exporting capture variants.".into();
            return false;
        };
        if self.dry_review != DryReviewStatus::ApprovedByHuman {
            self.status =
                "Variant export blocked: dry performance is Not reviewed. Human listening approval is required."
                    .into();
            return false;
        }
        let selected: Vec<ExportVariant> = ExportVariant::ALL
            .into_iter()
            .zip(self.variant_selection)
            .filter_map(|(v, on)| on.then_some(v))
            .collect();
        if selected.is_empty() {
            self.status = "Select at least one variant (individual or all) to export.".into();
            return false;
        }
        let [s0, s1] = t.rebuilt_span.unwrap_or([0, t.original.len()]);
        let orig_slice = &t.original[s0..s1.min(t.original.len())];
        let review = self.dry_review;
        let result = (|| -> spectracraft_mono::Result<usize> {
            std::fs::create_dir(out_dir)?;
            for &variant in &selected {
                let wave =
                    capture::render_guitar_variant_checked(review, variant, orig_slice, cues)?;
                let path = out_dir.join(format!("{}.wav", variant.slug()));
                let f = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                let mut w = hound::WavWriter::new(
                    std::io::BufWriter::new(f),
                    hound::WavSpec {
                        channels: 1,
                        sample_rate: t.rate,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    },
                )?;
                for s in wave {
                    w.write_sample(s)?;
                }
                w.finalize()?;
            }
            let manifest = serde_json::json!({
                "audio_status": "candidate exports (unmastered, post human dry-review approval)",
                "dry_review_status": review.label(),
                "selected_variants": selected.iter().map(|v| v.slug()).collect::<Vec<_>>(),
            });
            spectracraft_mono::qa::write_new(
                &out_dir.join("variants-selection.json"),
                serde_json::to_string_pretty(&manifest)?.as_bytes(),
            )?;
            Ok(selected.len())
        })();
        match result {
            Ok(n) => {
                self.status = format!("Exported {n} unmastered variant WAV(s) to fresh directory.");
                true
            }
            Err(e) => {
                self.status = format!("Variant export did not complete: {e}");
                false
            }
        }
    }

    fn build_layers(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            self.status = "Wait for the current background job to finish before analyzing layers.".into();
            return;
        }
        let span = match self.target_excerpt_samples() {
            Ok(s) => s,
            Err(e) => {
                self.status = e;
                self.app_state = AppState::Error;
                return;
            }
        };
        let Some(t) = &self.track else {
            return;
        };
        let input: Vec<f32> = t.original[span[0]..span[1]].to_vec();
        let rate = t.rate;
        self.stop();
        let Some((_, cancel, tx)) = self.begin_job(false) else {
            return;
        };
        self.app_state = AppState::Busy;
        self.status = format!(
            "Analyzing editable Pitch, Faint ringing, Texture, and Breath accent ({:.2}–{:.2} s)…",
            span[0] as f64 / rate as f64,
            span[1] as f64 / rate as f64
        );
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> spectracraft_mono::Result<LayerSession> {
                let packet = edge::analyze(&input, rate, edge::Options::default())?;
                if cancel.load(Ordering::Relaxed) {
                    return Err("Layer analysis cancelled.".into());
                }
                let rendered = workspace::render(&packet, 734)?;
                let headroom = rendered.headroom(&input);
                Ok(LayerSession {
                    packet,
                    rendered,
                    controls: workspace::Controls::default(),
                    headroom,
                    view: None,
                    span,
                })
            })()
            .map_err(|e| e.to_string());
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Finished::Layers(result));
                ctx.request_repaint();
            }
        });
    }

    fn remix_layers(&mut self, ctx: &egui::Context) {
        let playing = self.play.is_some();
        let keep_cursor = self.cursor;
        if playing {
            self.stop();
        }
        if let Some(t) = self.track.as_mut() {
            if let Some(session) = &t.layers {
                match session.rendered.mix(&session.controls) {
                    Ok(mix) => {
                        // View changes ONLY the plot buffer; audio mix (`t.rebuilt` & `t.dry`) is strictly controlled by Mute/Solo/Gain.
                        t.layer_plot_only = session.view.map(|i| session.rendered.stems[i].clone());
                        t.rebuilt_span = Some(session.span);
                        t.dry = Some(Arc::from(mix.clone()));
                        t.rebuilt = Some(mix);
                        self.applied_rig = None;
                        self.status = if let Some(v) = session.view {
                            format!(
                                "Editable layers ready. Viewing {} plot only (audible mix unchanged).",
                                workspace::NAMES[v]
                            )
                        } else {
                            "Editable layers ready. Gain and isolation use fixed audition headroom."
                                .into()
                        };
                    }
                    Err(e) => self.status = e.to_string(),
                }
            }
        }
        self.plot_revision += 1;
        self.plot_cache_key = None;
        self.refresh_viewport_visuals(ctx);
        self.cursor = keep_cursor;
        if playing {
            self.start();
        }
    }

    fn load_layers(&mut self, path: &Path, ctx: &egui::Context) {
        if self.job.is_some() {
            self.status = "Wait for the current background job to finish before loading layers.".into();
            return;
        }
        let Some(track) = &self.track else {
            return;
        };
        let span = self
            .target_excerpt_samples()
            .unwrap_or([0, track.original.len()]);
        let source: Vec<f32> = track.original[span[0]..span[1]].to_vec();
        let rate = track.rate;
        let bytes = match std::fs::read(path) {
            Ok(v) if v.len() <= 64 * 1024 * 1024 => v,
            Ok(_) => {
                self.status = "Analysis exceeds 64 MiB.".into();
                return;
            }
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        self.stop();
        let Some((_, cancel, tx)) = self.begin_job(false) else {
            return;
        };
        self.app_state = AppState::Busy;
        self.status = "Loading editable analysis…".into();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> spectracraft_mono::Result<LayerSession> {
                let packet: edge::Packet = serde_json::from_slice(&bytes)?;
                if packet.base.rate != rate || packet.base.samples != source.len() {
                    return Err(
                        "Analysis rate and duration must match the loaded reference/selection."
                            .into(),
                    );
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err("Layer load cancelled.".into());
                }
                let rendered = workspace::render(&packet, 734)?;
                let headroom = rendered.headroom(&source);
                Ok(LayerSession {
                    packet,
                    rendered,
                    controls: Default::default(),
                    headroom,
                    view: None,
                    span,
                })
            })()
            .map_err(|e| e.to_string());
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(Finished::Layers(result));
                ctx.request_repaint();
            }
        });
    }
}

// ======================= DRAWING HELPERS (TIMELINE & SPATIAL) =======================

fn draw_source_symbol(
    p: &egui::Painter,
    center: Pos2,
    radius: f32,
    sym: SourceSymbol,
    col: Color32,
) {
    p.circle_stroke(center, radius, Stroke::new(1.5, col));
    let s = Stroke::new(1.6, col);
    match sym {
        SourceSymbol::Unknown => {
            p.text(
                center,
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(radius * 1.15),
                col,
            );
        }
        SourceSymbol::Guitar => {
            p.circle_stroke(center + Vec2::new(0., radius * 0.18), radius * 0.42, s);
            p.line_segment(
                [
                    center - Vec2::new(0., radius * 0.65),
                    center + Vec2::new(0., radius * 0.2),
                ],
                s,
            );
        }
        SourceSymbol::Amp => {
            let r = Rect::from_center_size(center, Vec2::splat(radius * 1.15));
            p.rect_stroke(r, 2., s, egui::StrokeKind::Middle);
            p.circle_stroke(center, radius * 0.34, s);
        }
        SourceSymbol::Bowed => {
            p.line_segment(
                [
                    center - Vec2::new(radius * 0.5, -radius * 0.5),
                    center + Vec2::new(radius * 0.5, -radius * 0.5),
                ],
                s,
            );
            p.circle_stroke(center, radius * 0.32, s);
        }
        SourceSymbol::Wind | SourceSymbol::Brass => {
            p.line_segment(
                [
                    center - Vec2::new(radius * 0.55, 0.),
                    center + Vec2::new(radius * 0.35, 0.),
                ],
                s,
            );
            p.line_segment(
                [
                    center + Vec2::new(radius * 0.35, 0.),
                    center + Vec2::new(radius * 0.6, -radius * 0.3),
                ],
                s,
            );
            p.line_segment(
                [
                    center + Vec2::new(radius * 0.35, 0.),
                    center + Vec2::new(radius * 0.6, radius * 0.3),
                ],
                s,
            );
        }
        SourceSymbol::Percussion => {
            p.circle_stroke(center, radius * 0.52, s);
            p.line_segment(
                [
                    center - Vec2::new(radius * 0.5, radius * 0.45),
                    center + Vec2::new(radius * 0.1, 0.),
                ],
                s,
            );
        }
        SourceSymbol::Keyboard => {
            let r = Rect::from_center_size(center, Vec2::new(radius * 1.2, radius * 0.72));
            p.rect_stroke(r, 1., s, egui::StrokeKind::Middle);
            for k in [-0.25, 0.0, 0.25] {
                let x = center.x + radius * k;
                p.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.center().y)], s);
            }
        }
        SourceSymbol::Voice => {
            p.circle_stroke(center - Vec2::new(0., radius * 0.22), radius * 0.28, s);
            p.line_segment(
                [
                    center + Vec2::new(-radius * 0.4, radius * 0.42),
                    center + Vec2::new(radius * 0.4, radius * 0.42),
                ],
                s,
            );
        }
        SourceSymbol::Ensemble => {
            for dx in [-radius * 0.32, 0.0, radius * 0.32] {
                p.circle_filled(center + Vec2::new(dx, 0.), radius * 0.16, col);
            }
        }
    }
}

fn draw_polar_mini(ui: &mut egui::Ui, pat: Pattern, size: f32) {
    let (resp, p) = ui.allocate_painter(Vec2::splat(size), egui::Sense::hover());
    let c = resp.rect.center();
    let r = size * 0.42;
    p.rect_filled(resp.rect, 4., StudioTheme::BG_CANVAS);
    p.rect_stroke(
        resp.rect,
        4.,
        Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
        egui::StrokeKind::Inside,
    );
    p.circle_stroke(c, r, Stroke::new(1.0, Color32::from_gray(65)));
    p.circle_stroke(c, r * 0.5, Stroke::new(0.7, Color32::from_gray(45)));
    p.line_segment(
        [Pos2::new(c.x - r, c.y), Pos2::new(c.x + r, c.y)],
        Stroke::new(0.7, Color32::from_gray(50)),
    );
    p.line_segment(
        [Pos2::new(c.x, c.y - r), Pos2::new(c.x, c.y + r)],
        Stroke::new(0.7, Color32::from_gray(50)),
    );
    let steps = 72;
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let ang = std::f32::consts::TAU * i as f32 / steps as f32;
        // 0° points up (on-axis)
        let gain = pat.pressure(f64::from(ang.cos())).abs() as f32;
        let px = c.x + gain * r * ang.sin();
        let py = c.y - gain * r * ang.cos();
        pts.push(Pos2::new(px, py));
    }
    p.add(egui::Shape::line(
        pts,
        Stroke::new(1.8, Color32::from_rgb(255, 196, 102)),
    ));
    p.text(
        Pos2::new(c.x, resp.rect.top() + 6.),
        egui::Align2::CENTER_CENTER,
        "0°",
        egui::FontId::proportional(9.),
        Color32::LIGHT_GRAY,
    );
}

fn draw_polar_overlay_on_plan(
    p: &egui::Painter,
    center: Pos2,
    aim_2d: Vec2,
    pat: Pattern,
    radius: f32,
) {
    let dir = if aim_2d.length() > 1e-4 {
        aim_2d.normalized()
    } else {
        Vec2::new(0.0, -1.0)
    };
    let perp = Vec2::new(-dir.y, dir.x);
    let steps = 64;
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let ang = std::f32::consts::TAU * i as f32 / steps as f32;
        let g = pat.pressure(f64::from(ang.cos())).abs() as f32;
        let local = dir * (g * radius * ang.cos()) + perp * (g * radius * ang.sin());
        pts.push(center + local);
    }
    p.add(egui::Shape::line(
        pts,
        Stroke::new(1.4, Color32::from_rgba_unmultiplied(255, 196, 102, 165)),
    ));
}

fn draw_source_radiation_ring(
    p: &egui::Painter,
    center: Pos2,
    forward_2d: Vec2,
    rad: SourceRadiation,
    radius: f32,
) {
    let dir = if forward_2d.length() > 1e-4 {
        forward_2d.normalized()
    } else {
        Vec2::new(0.0, 1.0)
    };
    let perp = Vec2::new(-dir.y, dir.x);
    let steps = 48;
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let ang = std::f32::consts::TAU * i as f32 / steps as f32;
        let g = rad.gain(f64::from(ang.cos())).clamp(0.05, 1.0) as f32;
        let local = dir * (g * radius * ang.cos()) + perp * (g * radius * ang.sin());
        pts.push(center + local);
    }
    p.add(egui::Shape::line(
        pts,
        Stroke::new(1.1, Color32::from_rgba_unmultiplied(110, 185, 255, 130)),
    ));
}

fn draw_overview_bar(
    ui: &mut egui::Ui,
    peaks: &[(f32, f32)],
    duration: f64,
    view: [f64; 2],
    selection: Option<[f64; 2]>,
    cursor: f64,
) -> Option<[f64; 2]> {
    let (resp, p) = ui.allocate_painter(
        Vec2::new(ui.available_width().max(60.), 36.),
        egui::Sense::click_and_drag(),
    );
    let r = resp.rect;
    p.rect_filled(r, 3., StudioTheme::BG_CANVAS);
    p.rect_stroke(
        r,
        3.,
        Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
        egui::StrokeKind::Inside,
    );
    if !peaks.is_empty() && duration > 0. {
        let scale = peaks
            .iter()
            .fold(0.1_f32, |m, &(lo, hi)| m.max(lo.abs()).max(hi.abs()));
        let n = (r.width() as usize).clamp(1, peaks.len());
        for i in 0..n {
            let a = peaks.len() * i / n;
            let b = (peaks.len() * (i + 1) / n).max(a + 1).min(peaks.len());
            let (lo, hi) = peaks[a..b]
                .iter()
                .fold((0_f32, 0_f32), |(x, y), &(l, h)| (x.min(l), y.max(h)));
            let x = r.left() + r.width() * (i as f32 + 0.5) / n as f32;
            let y0 = r.center().y - (hi / scale) * r.height() * 0.42;
            let y1 = r.center().y - (lo / scale) * r.height() * 0.42;
            p.line_segment(
                [Pos2::new(x, y0), Pos2::new(x, y1)],
                Stroke::new(1.0, Color32::from_rgb(95, 125, 165)),
            );
        }
        let tx = |t: f64| r.left() + r.width() * (t / duration).clamp(0., 1.) as f32;
        if let Some([sa, sb]) = selection {
            let sr = Rect::from_min_max(Pos2::new(tx(sa), r.top()), Pos2::new(tx(sb), r.bottom()));
            p.rect_filled(sr, 0., Color32::from_rgba_unmultiplied(100, 210, 175, 45));
        }
        let vr = Rect::from_min_max(
            Pos2::new(tx(view[0]), r.top()),
            Pos2::new(tx(view[1]), r.bottom()),
        );
        p.rect_filled(vr, 0., Color32::from_rgba_unmultiplied(255, 255, 255, 22));
        p.rect_stroke(
            vr,
            1.,
            Stroke::new(1.2, Color32::from_rgb(200, 220, 255)),
            egui::StrokeKind::Inside,
        );
        let cx = tx(cursor);
        p.line_segment(
            [Pos2::new(cx, r.top()), Pos2::new(cx, r.bottom())],
            Stroke::new(1.2, Color32::WHITE),
        );
        if (resp.clicked() || resp.dragged()) && r.width() > 1. {
            if let Some(pt) = resp.interact_pointer_pos() {
                let frac = ((pt.x - r.left()) / r.width()).clamp(0., 1.) as f64;
                let span = (view[1] - view[0]).max(0.05);
                let mut lo = (frac * duration - span * 0.5).clamp(0., (duration - span).max(0.));
                let hi = (lo + span).min(duration);
                lo = (hi - span).max(0.);
                return Some([lo, hi]);
            }
        }
    }
    None
}

/// Interactive Audition/Logic-style time ruler: dynamic non-overlapping ticks,
/// right-edge-safe alignment, minor subdivisions, selection highlight, and click/drag scrubbing.
fn draw_time_ruler(
    ui: &mut egui::Ui,
    view: [f64; 2],
    selection: Option<[f64; 2]>,
    cursor: f64,
) -> Option<f64> {
    let (resp, p) = ui.allocate_painter(
        Vec2::new(ui.available_width().max(80.), 24.),
        egui::Sense::click_and_drag(),
    );
    let r = resp.rect;
    p.rect_filled(r, 0., StudioTheme::BG_HEADER);
    p.line_segment(
        [
            Pos2::new(r.left(), r.bottom()),
            Pos2::new(r.right(), r.bottom()),
        ],
        Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
    );
    let plot = Rect::from_min_max(r.min + Vec2::new(64., 0.), r.max);
    p.text(
        Pos2::new(r.left() + 6., r.center().y),
        egui::Align2::LEFT_CENTER,
        "TIME",
        egui::FontId::proportional(9.5),
        StudioTheme::TEXT_MUTED,
    );
    let span = (view[1] - view[0]).max(1e-4);
    let ticks = ((plot.width() / 78.0).floor() as usize).clamp(3, 12);
    for i in 0..=ticks {
        let f = i as f64 / ticks as f64;
        let t = view[0] + span * f;
        let x = plot.left() + plot.width() * f as f32;
        p.line_segment(
            [Pos2::new(x, r.bottom() - 7.), Pos2::new(x, r.bottom())],
            Stroke::new(1.0, Color32::from_gray(120)),
        );
        if i < ticks {
            let mid_x = plot.left() + plot.width() * ((i as f32 + 0.5) / ticks as f32);
            p.line_segment(
                [
                    Pos2::new(mid_x, r.bottom() - 4.),
                    Pos2::new(mid_x, r.bottom()),
                ],
                Stroke::new(0.8, Color32::from_gray(75)),
            );
        }
        let (tx, align) = if i == 0 {
            (x + 2.0, egui::Align2::LEFT_CENTER)
        } else if i == ticks {
            (x - 2.0, egui::Align2::RIGHT_CENTER)
        } else {
            (x, egui::Align2::CENTER_CENTER)
        };
        p.text(
            Pos2::new(tx, r.top() + 8.),
            align,
            format!("{t:.2}s"),
            egui::FontId::monospace(9.5),
            StudioTheme::TEXT_MUTED,
        );
    }
    if let Some([sa, sb]) = selection {
        let a = ((sa - view[0]) / span).clamp(0., 1.) as f32;
        let b = ((sb - view[0]) / span).clamp(0., 1.) as f32;
        if b > a {
            let x0 = plot.left() + plot.width() * a;
            let x1 = plot.left() + plot.width() * b;
            p.rect_filled(
                Rect::from_min_max(Pos2::new(x0, r.bottom() - 4.), Pos2::new(x1, r.bottom())),
                0.,
                StudioTheme::STAGE_ACTIVE,
            );
            p.line_segment(
                [Pos2::new(x0, r.top() + 14.), Pos2::new(x0, r.bottom())],
                Stroke::new(1.5, StudioTheme::STAGE_ACTIVE),
            );
            p.line_segment(
                [Pos2::new(x1, r.top() + 14.), Pos2::new(x1, r.bottom())],
                Stroke::new(1.5, StudioTheme::STAGE_ACTIVE),
            );
        }
    }
    if cursor >= view[0] && cursor <= view[1] {
        let cx = plot.left() + plot.width() * ((cursor - view[0]) / span) as f32;
        p.line_segment(
            [Pos2::new(cx, r.top()), Pos2::new(cx, r.bottom())],
            Stroke::new(1.5, Color32::WHITE),
        );
        // Playhead top cap triangle
        p.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(cx - 4.5, r.top()),
                Pos2::new(cx + 4.5, r.top()),
                Pos2::new(cx, r.top() + 6.0),
            ],
            Color32::WHITE,
            Stroke::NONE,
        ));
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if (resp.clicked() || resp.dragged()) && plot.width() > 1.0 {
        if let Some(pt) = resp.interact_pointer_pos() {
            if pt.x >= plot.left() - 2.0 {
                let frac = ((pt.x - plot.left()) / plot.width()).clamp(0., 1.) as f64;
                return Some(view[0] + frac * span);
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug)]
enum TimelineInteraction {
    Seek(f64),
    DragStart {
        t: f64,
        hz: Option<f64>,
        mode: SelectionDragMode,
    },
    DragMove {
        t: f64,
        hz: Option<f64>,
    },
    DragEnd,
    WheelZoom(f64, f64),
}

fn selection_rect_on_plot(
    plot: Rect,
    view: [f64; 2],
    selection: Option<[f64; 2]>,
    marquee_hz: Option<[f64; 2]>,
    max_hz: Option<f32>,
) -> Option<Rect> {
    let [sa, sb] = selection?;
    let span = (view[1] - view[0]).max(1e-4);
    let a = ((sa.min(sb) - view[0]) / span).clamp(0., 1.) as f32;
    let b = ((sa.max(sb) - view[0]) / span).clamp(0., 1.) as f32;
    if b <= a {
        return None;
    }
    let (y_top, y_bot) = match (marquee_hz, max_hz) {
        (Some([flo, fhi]), Some(cap)) => {
            let cap_hz = f64::from(cap).max(1.0);
            let f0 = (flo.min(fhi) / cap_hz).clamp(0.0, 1.0) as f32;
            let f1 = (flo.max(fhi) / cap_hz).clamp(0.0, 1.0) as f32;
            (
                plot.bottom() - plot.height() * f1,
                plot.bottom() - plot.height() * f0,
            )
        }
        _ => (plot.top(), plot.bottom()),
    };
    Some(Rect::from_min_max(
        Pos2::new(plot.left() + plot.width() * a, y_top),
        Pos2::new(plot.left() + plot.width() * b, y_bot.max(y_top + 4.0)),
    ))
}

fn hit_test_selection(
    pt: Pos2,
    plot: Rect,
    view: [f64; 2],
    selection: Option<[f64; 2]>,
    marquee_hz: Option<[f64; 2]>,
    max_hz: Option<f32>,
) -> Option<SelectionDragMode> {
    let sr = selection_rect_on_plot(plot, view, selection, marquee_hz, max_hz)?;
    let tol = 7.0_f32;
    let expanded = sr.expand(tol);
    if !expanded.contains(pt) {
        return None;
    }
    let near_l = (pt.x - sr.left()).abs() <= tol;
    let near_r = (pt.x - sr.right()).abs() <= tol;
    let has_hz_box = marquee_hz.is_some() && max_hz.is_some();
    let near_t = has_hz_box && (pt.y - sr.top()).abs() <= tol;
    let near_b = has_hz_box && (pt.y - sr.bottom()).abs() <= tol;

    if near_l && near_t {
        Some(SelectionDragMode::ResizeTopLeft)
    } else if near_r && near_t {
        Some(SelectionDragMode::ResizeTopRight)
    } else if near_l && near_b {
        Some(SelectionDragMode::ResizeBottomLeft)
    } else if near_r && near_b {
        Some(SelectionDragMode::ResizeBottomRight)
    } else if near_l {
        Some(SelectionDragMode::ResizeLeft)
    } else if near_r {
        Some(SelectionDragMode::ResizeRight)
    } else if near_t {
        Some(SelectionDragMode::ResizeTop)
    } else if near_b {
        Some(SelectionDragMode::ResizeBottom)
    } else if sr.contains(pt) {
        Some(SelectionDragMode::Move)
    } else {
        None
    }
}

fn draw_selection_handles(
    painter: &egui::Painter,
    sr: Rect,
    stroke_col: Color32,
    is_marquee: bool,
    tag: Option<String>,
) {
    let h = 3.5_f32;
    if is_marquee {
        for corner in [
            sr.left_top(),
            sr.right_top(),
            sr.left_bottom(),
            sr.right_bottom(),
        ] {
            let hr = Rect::from_center_size(corner, Vec2::splat(h * 2.0));
            painter.rect_filled(hr, 1.0, stroke_col);
            painter.rect_stroke(
                hr,
                1.0,
                Stroke::new(0.8, StudioTheme::BG_CANVAS),
                egui::StrokeKind::Inside,
            );
        }
    } else {
        // Vertical left/right resize grips centered on selection edges
        for x in [sr.left(), sr.right()] {
            let grip = Rect::from_center_size(
                Pos2::new(x, sr.center().y),
                Vec2::new(4.0, (sr.height() * 0.28).clamp(12.0, 26.0)),
            );
            painter.rect_filled(grip, 1.5, stroke_col);
        }
    }
    if let Some(txt) = tag {
        if sr.width() >= 48.0 {
            let anchor = Pos2::new(sr.left() + 4.0, sr.top() + 3.0);
            let galley = painter.layout_no_wrap(
                txt,
                egui::FontId::monospace(9.5),
                StudioTheme::TEXT_PRIMARY,
            );
            let bg = Rect::from_min_size(
                anchor - Vec2::new(2.0, 1.0),
                galley.size() + Vec2::new(6.0, 2.0),
            );
            painter.rect_filled(bg, 2.0, Color32::from_rgba_unmultiplied(16, 16, 22, 205));
            painter.galley(anchor, galley, StudioTheme::TEXT_PRIMARY);
        }
    }
}

fn handle_plot_interaction(
    resp: &egui::Response,
    plot: Rect,
    view: [f64; 2],
    selection: Option<[f64; 2]>,
    marquee_hz: Option<[f64; 2]>,
    max_hz: Option<f32>,
    ctx: &egui::Context,
) -> Option<TimelineInteraction> {
    let span = (view[1] - view[0]).max(1e-4);
    let to_time =
        |x: f32| view[0] + ((x - plot.left()) / plot.width().max(1.)).clamp(0., 1.) as f64 * span;
    let to_hz = |y: f32| {
        max_hz.map(|cap| {
            let frac = ((plot.bottom() - y) / plot.height().max(1.)).clamp(0., 1.) as f64;
            frac * f64::from(cap)
        })
    };
    if resp.hovered() {
        if let Some(pt) = resp.hover_pos() {
            if plot.contains(pt) {
                let icon = match hit_test_selection(pt, plot, view, selection, marquee_hz, max_hz) {
                    Some(SelectionDragMode::ResizeLeft | SelectionDragMode::ResizeRight) => {
                        egui::CursorIcon::ResizeHorizontal
                    }
                    Some(SelectionDragMode::ResizeTop | SelectionDragMode::ResizeBottom) => {
                        egui::CursorIcon::ResizeVertical
                    }
                    Some(
                        SelectionDragMode::ResizeTopLeft | SelectionDragMode::ResizeBottomRight,
                    ) => egui::CursorIcon::ResizeNwSe,
                    Some(
                        SelectionDragMode::ResizeTopRight | SelectionDragMode::ResizeBottomLeft,
                    ) => egui::CursorIcon::ResizeNeSw,
                    Some(SelectionDragMode::Move) => egui::CursorIcon::Grab,
                    _ => egui::CursorIcon::Crosshair,
                };
                ctx.set_cursor_icon(icon);
            }
        }
        let scroll_y = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll_y.abs() > 1.0 {
            let anchor = resp
                .hover_pos()
                .map(|p| ((p.x - plot.left()) / plot.width().max(1.)).clamp(0., 1.) as f64)
                .unwrap_or(0.5);
            let factor = if scroll_y > 0. { 0.85 } else { 1.18 };
            return Some(TimelineInteraction::WheelZoom(factor, anchor));
        }
    }
    if resp.drag_started() {
        if let Some(pt) = resp.interact_pointer_pos() {
            if plot.contains(pt) {
                let mode = hit_test_selection(pt, plot, view, selection, marquee_hz, max_hz)
                    .unwrap_or(SelectionDragMode::Create);
                return Some(TimelineInteraction::DragStart {
                    t: to_time(pt.x),
                    hz: to_hz(pt.y),
                    mode,
                });
            }
        }
    }
    if resp.dragged() {
        if let Some(pt) = resp.interact_pointer_pos() {
            return Some(TimelineInteraction::DragMove {
                t: to_time(pt.x),
                hz: to_hz(pt.y),
            });
        }
    }
    if resp.drag_stopped() {
        return Some(TimelineInteraction::DragEnd);
    }
    if resp.clicked() {
        if let Some(pt) = resp.interact_pointer_pos() {
            if plot.contains(pt) {
                return Some(TimelineInteraction::Seek(to_time(pt.x)));
            }
        }
    }
    None
}

fn spectrum_plot_panel(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    label: &str,
    visual: Option<&Visual>,
    texture: Option<&egui::TextureHandle>,
    view: [f64; 2],
    selection: Option<[f64; 2]>,
    marquee_hz: Option<[f64; 2]>,
    cursor: f64,
    height: f32,
) -> Option<TimelineInteraction> {
    ui.horizontal(|ui| {
        ui.strong(label);
        if let Some(v) = visual {
            ui.colored_label(StudioTheme::TEXT_MUTED, v.peak_dbfs());
            if v.spectrum.max_hz > v.spectrum.nyquist_hz + 1.0 {
                ui.colored_label(
                    StudioTheme::WARNING,
                    format!(
                        "Nyquist {:.1} kHz — above Nyquist: No data",
                        v.spectrum.nyquist_hz / 1000.0
                    ),
                );
            }
        }
    });
    let (resp, painter) = ui.allocate_painter(
        Vec2::new(ui.available_width().max(80.), height),
        egui::Sense::click_and_drag(),
    );
    let r = resp.rect;
    painter.rect_filled(r, 3., spectrum_color(SPECTRUM_BOTTOM));
    painter.rect_stroke(
        r,
        3.,
        Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
        egui::StrokeKind::Inside,
    );
    let plot = Rect::from_min_max(r.min + Vec2::new(64., 0.), r.max);
    // Axis gutter separator
    painter.line_segment(
        [
            Pos2::new(plot.left(), r.top()),
            Pos2::new(plot.left(), r.bottom()),
        ],
        Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
    );
    let span = (view[1] - view[0]).max(1e-4);
    let mut plot_max_hz: Option<f32> = None;
    if let Some((tex, view_vis)) = texture.zip(visual) {
        plot_max_hz = Some(view_vis.spectrum.max_hz);
        painter.image(
            tex.id(),
            plot,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
        let cap = view_vis.spectrum.max_hz / 1000.;
        let nyq = view_vis.spectrum.nyquist_hz / 1000.;
        if cap > nyq + 0.01 {
            let nyq_y = plot.bottom() - (nyq / cap).clamp(0., 1.) * plot.height();
            let above = Rect::from_min_max(plot.min, Pos2::new(plot.right(), nyq_y));
            painter.rect_filled(above, 0., Color32::from_rgba_unmultiplied(45, 45, 55, 220));
            painter.line_segment(
                [
                    Pos2::new(plot.left(), nyq_y),
                    Pos2::new(plot.right(), nyq_y),
                ],
                Stroke::new(1.2, StudioTheme::WARNING),
            );
            painter.text(
                above.center(),
                egui::Align2::CENTER_CENTER,
                "No data (above Nyquist)",
                egui::FontId::proportional(11.),
                StudioTheme::WARNING,
            );
        }
        for (lbl, y) in [
            (format!("{cap:.1} kHz"), r.top() + 9.),
            (format!("{:.1} kHz", cap / 2.), r.center().y),
            ("0.0 kHz".to_string(), r.bottom() - 9.),
        ] {
            painter.text(
                Pos2::new(r.left() + 5., y),
                egui::Align2::LEFT_CENTER,
                lbl,
                egui::FontId::monospace(9.5),
                StudioTheme::TEXT_MUTED,
            );
        }
        if let Some(sr) = selection_rect_on_plot(plot, view, selection, marquee_hz, plot_max_hz) {
            let is_marquee = marquee_hz.is_some();
            let fill = if is_marquee {
                Color32::from_rgba_unmultiplied(55, 142, 240, 48)
            } else {
                Color32::from_rgba_unmultiplied(52, 211, 153, 35)
            };
            let stroke_col = if is_marquee {
                StudioTheme::ACCENT_BLUE
            } else {
                StudioTheme::STAGE_ACTIVE
            };
            painter.rect_filled(sr, 0., fill);
            painter.rect_stroke(
                sr,
                0.,
                Stroke::new(1.4, stroke_col),
                egui::StrokeKind::Inside,
            );
            let tag = selection.map(|[a, b]| match marquee_hz {
                Some([flo, fhi]) => {
                    format!(
                        "{:.2}s · {:.0}–{:.0} Hz",
                        (b - a).abs(),
                        flo.min(fhi),
                        flo.max(fhi)
                    )
                }
                None => format!("{:.2}s", (b - a).abs()),
            });
            draw_selection_handles(&painter, sr, stroke_col, is_marquee, tag);
        }
        if cursor >= view[0] && cursor <= view[1] {
            let x = plot.left() + plot.width() * ((cursor - view[0]) / span) as f32;
            let line = [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())];
            painter.line_segment(line, Stroke::new(3_f32, Color32::BLACK));
            painter.line_segment(line, Stroke::new(1_f32, Color32::WHITE));
        }
    } else {
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for rendered samples",
            egui::FontId::proportional(12.5),
            StudioTheme::TEXT_MUTED,
        );
    }
    if visual.is_some() {
        handle_plot_interaction(&resp, plot, view, selection, marquee_hz, plot_max_hz, ctx)
    } else {
        None
    }
}

fn wave_plot_panel(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    label: &str,
    wave: Option<&[(f32, f32)]>,
    scale: f32,
    view: [f64; 2],
    selection: Option<[f64; 2]>,
    cursor: f64,
    color: Color32,
    height: f32,
) -> Option<TimelineInteraction> {
    ui.horizontal(|ui| {
        ui.strong(label);
        ui.colored_label(StudioTheme::TEXT_MUTED, format!("±{scale:.2}"));
    });
    let (resp, paint) = ui.allocate_painter(
        Vec2::new(ui.available_width().max(80.), height),
        egui::Sense::click_and_drag(),
    );
    let r = resp.rect;
    paint.rect_filled(r, 3., StudioTheme::BG_CANVAS);
    paint.rect_stroke(
        r,
        3.,
        Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
        egui::StrokeKind::Inside,
    );
    let plot = Rect::from_min_max(r.min + Vec2::new(64., 0.), r.max);
    paint.line_segment(
        [
            Pos2::new(plot.left(), r.top()),
            Pos2::new(plot.left(), r.bottom()),
        ],
        Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
    );
    // Crisp 0.0 amplitude centerline
    paint.line_segment(
        [
            Pos2::new(plot.left(), plot.center().y),
            Pos2::new(plot.right(), plot.center().y),
        ],
        Stroke::new(0.8, Color32::from_gray(52)),
    );
    let span = (view[1] - view[0]).max(1e-4);
    for (lbl, y) in [
        (format!("+{scale:.2}"), r.top() + 9.),
        (" 0.00".to_string(), r.center().y),
        (format!("-{scale:.2}"), r.bottom() - 9.),
    ] {
        paint.text(
            Pos2::new(r.left() + 5., y),
            egui::Align2::LEFT_CENTER,
            lbl,
            egui::FontId::monospace(9.5),
            StudioTheme::TEXT_MUTED,
        );
    }
    if let Some(x) = wave {
        if !x.is_empty() {
            let n = (plot.width() as usize).clamp(1, x.len());
            for i in 0..n {
                let p = x.len() * i / n;
                let q = (x.len() * (i + 1) / n).max(p + 1).min(x.len());
                let (lo, hi) = x[p..q]
                    .iter()
                    .fold((0_f32, 0_f32), |(a, b), &(lo, hi)| (a.min(lo), b.max(hi)));
                let xx = plot.left() + plot.width() * (i as f32 + 0.5) / n as f32;
                let y = |v| plot.center().y - v / scale * plot.height() * 0.44;
                paint.line_segment(
                    [Pos2::new(xx, y(lo)), Pos2::new(xx, y(hi))],
                    Stroke::new(1_f32, color),
                );
            }
        }
        if let Some(sr) = selection_rect_on_plot(plot, view, selection, None, None) {
            paint.rect_filled(sr, 0., Color32::from_rgba_unmultiplied(52, 211, 153, 35));
            paint.rect_stroke(
                sr,
                0.,
                Stroke::new(1.4, StudioTheme::STAGE_ACTIVE),
                egui::StrokeKind::Inside,
            );
            let tag = selection.map(|[a, b]| format!("{:.2}s", (b - a).abs()));
            draw_selection_handles(&paint, sr, StudioTheme::STAGE_ACTIVE, false, tag);
        }
        if cursor >= view[0] && cursor <= view[1] {
            let cx = plot.left() + plot.width() * ((cursor - view[0]) / span) as f32;
            paint.line_segment(
                [Pos2::new(cx, plot.top()), Pos2::new(cx, plot.bottom())],
                Stroke::new(1.5, Color32::WHITE),
            );
        }
    } else {
        paint.text(
            plot.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for rendered samples",
            egui::FontId::proportional(12.5),
            StudioTheme::TEXT_MUTED,
        );
    }
    if wave.is_some() {
        handle_plot_interaction(&resp, plot, view, selection, None, None, ctx)
    } else {
        None
    }
}

fn spectrum_legend(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label("−90 dBFS");
        let (r, p) = ui.allocate_painter(Vec2::new(130., 9.), egui::Sense::hover());
        for i in 0..65 {
            let a = r.rect.left() + r.rect.width() * i as f32 / 65.;
            let b = r.rect.left() + r.rect.width() * (i + 1) as f32 / 65.;
            p.rect_filled(
                Rect::from_min_max(Pos2::new(a, r.rect.top()), Pos2::new(b, r.rect.bottom())),
                0.,
                spectrum_color(SPECTRUM_BOTTOM + i as f32 * 90. / 65.),
            );
        }
        ui.label("0 dBFS");
    });
}

// ======================= INSPECTOR & SPATIAL VIEWS =======================

impl Studio {
    fn draw_inspector(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let snap_before = self.capture_snapshot();
        let fp_before = self.post_dry_fingerprint();
        let rig_non_display_before = {
            let mut r = snap_before.rig.clone();
            r.unit = Unit::Feet;
            r.show_patterns = false;
            r
        };

        // Compact panel header with display-only Dock Left / Right / Float / Hide controls
        egui::Frame::new()
            .fill(StudioTheme::BG_HEADER)
            .corner_radius(3.0)
            .inner_margin(egui::Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Inspector");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text("Hide Inspector")
                            .clicked()
                        {
                            self.inspector_dock = InspectorDock::Hidden;
                        }
                        if ui
                            .selectable_label(
                                self.inspector_dock == InspectorDock::Floating,
                                "Float",
                            )
                            .on_hover_text("Undock Inspector into floating window")
                            .clicked()
                        {
                            self.inspector_dock = InspectorDock::Floating;
                        }
                        if ui
                            .selectable_label(self.inspector_dock == InspectorDock::Right, "Right")
                            .on_hover_text("Dock Inspector on right")
                            .clicked()
                        {
                            self.inspector_dock = InspectorDock::Right;
                        }
                        if ui
                            .selectable_label(self.inspector_dock == InspectorDock::Left, "Left")
                            .on_hover_text("Dock Inspector on left")
                            .clicked()
                        {
                            self.inspector_dock = InspectorDock::Left;
                        }
                    });
                });
            });
        ui.add_space(3.0);

        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.inspector_tab, InspectorTab::Source, "1. Source");
            ui.selectable_value(
                &mut self.inspector_tab,
                InspectorTab::DryEdit,
                "2. Dry Edit",
            );
            ui.selectable_value(&mut self.inspector_tab, InspectorTab::Microphone, "3. Mic");
            ui.selectable_value(
                &mut self.inspector_tab,
                InspectorTab::Materials,
                "Materials",
            );
            ui.selectable_value(&mut self.inspector_tab, InspectorTab::Room, "4. Room");
            ui.selectable_value(&mut self.inspector_tab, InspectorTab::Plugin, "5. Plugin");
        });
        ui.separator();

        // Dynamic vertical height budgeting so short windows (580px) and floating windows never clip
        let avail_h = ui.available_height().max(220.0);
        let capture_footer_h = 114.0_f32;
        let spatial_budget_h = if self.show_spatial_viewport {
            (avail_h * 0.34).clamp(138.0, 232.0)
        } else {
            24.0
        };
        let tab_h = (avail_h - spatial_budget_h - capture_footer_h - 32.0).clamp(120.0, 520.0);

        egui::ScrollArea::vertical()
            .id_salt("inspector_outer_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("inspector_tab_scroll")
                    .max_height(tab_h)
                    .show(ui, |ui| match self.inspector_tab {
                        InspectorTab::Source => self.draw_source_tab(ui, ctx),
                        InspectorTab::DryEdit => self.draw_dry_edit_tab(ui, ctx),
                        InspectorTab::Microphone => self.draw_microphone_tab(ui),
                        InspectorTab::Materials => self.draw_materials_tab(ui),
                        InspectorTab::Room => self.draw_room_tab(ui),
                        InspectorTab::Plugin => self.draw_plugin_tab(ui),
                    });

                ui.separator();

                // Collapsible Linked Top & Front Plans OR Separate 3D Grid
                ui.horizontal(|ui| {
                    let toggle_lbl = if self.show_spatial_viewport {
                        "▾ Spatial"
                    } else {
                        "▸ Spatial"
                    };
                    if ui.small_button(toggle_lbl).clicked() {
                        self.show_spatial_viewport = !self.show_spatial_viewport;
                    }
                    if self.show_spatial_viewport {
                        ui.selectable_value(
                            &mut self.spatial_mode,
                            SpatialViewMode::Plans,
                            "Top + Front",
                        );
                        ui.selectable_value(
                            &mut self.spatial_mode,
                            SpatialViewMode::Grid3D,
                            "3D Grid",
                        );
                        ui.checkbox(&mut self.rig.show_patterns, "Patterns");
                    }
                });
                if self.show_spatial_viewport {
                    match self.spatial_mode {
                        SpatialViewMode::Plans => {
                            self.draw_linked_top_front_plans(ui, ctx, spatial_budget_h)
                        }
                        SpatialViewMode::Grid3D => {
                            self.draw_3d_grid_view(ui, ctx, spatial_budget_h)
                        }
                    }
                }

                self.rig.clamp_all();
                let rig_non_display_after = {
                    let mut r = self.rig.clone();
                    r.unit = Unit::Feet;
                    r.show_patterns = false;
                    r
                };
                let editable_changed = rig_non_display_after != rig_non_display_before
                    || self.voicing != snap_before.voicing
                    || self.mic_enabled != snap_before.mic_enabled
                    || self.room_enabled != snap_before.room_enabled
                    || self.plugin_slot != snap_before.plugin_slot
                    || self.region_edit_draft != snap_before.region_edit_draft;
                if editable_changed {
                    self.history.push(snap_before);
                }
                let pred = self.effective_prediction();
                self.draw_capture_summary_and_actions(ui, ctx, pred, fp_before);
            });
    }

    fn draw_source_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            for cat in SourceCategory::ALL {
                if ui
                    .selectable_label(self.source_category_filter == cat, cat.label())
                    .clicked()
                {
                    self.source_category_filter = cat;
                    self.source_family_filter.clear();
                }
            }
            ui.label(format!("({} catalog)", self.sources.len()));
        });
        // Family drill-down for Instrument / Human
        let mut families: Vec<String> = self
            .sources
            .iter()
            .filter(|s| s.category == self.source_category_filter)
            .map(|s| s.family.clone())
            .collect();
        families.dedup();
        if families.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .selectable_label(self.source_family_filter.is_empty(), "All")
                    .clicked()
                {
                    self.source_family_filter.clear();
                }
                for fam in &families {
                    if ui
                        .selectable_label(self.source_family_filter == *fam, fam.as_str())
                        .clicked()
                    {
                        self.source_family_filter = fam.clone();
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.source_search);
        });
        let q = self.source_search.to_ascii_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("source_list")
            .max_height(88.)
            .show(ui, |ui| {
                for (idx, s) in self.sources.iter().enumerate() {
                    if s.category != self.source_category_filter && q.is_empty() {
                        continue;
                    }
                    if !self.source_family_filter.is_empty()
                        && s.family != self.source_family_filter
                        && q.is_empty()
                    {
                        continue;
                    }
                    if !q.is_empty()
                        && !s.name.to_ascii_lowercase().contains(&q)
                        && !s.family.to_ascii_lowercase().contains(&q)
                    {
                        continue;
                    }
                    if ui
                        .selectable_label(self.rig.source_index == idx, &s.name)
                        .clicked()
                    {
                        self.rig.source_index = idx;
                    }
                }
            });

        // ~Half-inch (40x40 px) source symbol + Info sheet
        if let Some(sel) = self.sources.get(self.rig.source_index) {
            ui.add_space(4.);
            ui.horizontal(|ui| {
                let (r, p) = ui.allocate_painter(Vec2::splat(40.), egui::Sense::hover());
                p.rect_filled(r.rect, 4., StudioTheme::BG_CANVAS);
                p.rect_stroke(
                    r.rect,
                    4.,
                    Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
                    egui::StrokeKind::Inside,
                );
                draw_source_symbol(
                    &p,
                    r.rect.center(),
                    15.,
                    sel.symbol,
                    StudioTheme::ACCENT_BLUE,
                );
                ui.vertical(|ui| {
                    ui.strong(&sel.name);
                    ui.small(format!("{} · {}", sel.category.label(), sel.family));
                    ui.small(format!(
                        "Radiation: {} (α {:.2})",
                        sel.radiation.label(),
                        sel.radiation.directivity_alpha()
                    ));
                });
            });
            if let Some((lo, hi)) = sel.published_range_hz {
                ui.small(format!(
                    "Published range (info only): {lo:.0}–{hi:.0} Hz. Never filters recording."
                ));
            }
        }

        ui.add_space(4.);
        StudioTheme::section_frame(ui, "Stage 1 — Dry Voicing & Loose Guidance", |ui| {
            let char_lbl = |c: CharacterTarget| match c {
                CharacterTarget::Keep => "Keep original balance",
                CharacterTarget::Natural => "Natural / acoustic",
                CharacterTarget::Analog => "Analog / warm",
                CharacterTarget::Digital => "Digital / crisp",
            };
            ui.horizontal(|ui| {
                ui.label("Character:");
                egui::ComboBox::from_id_salt("voicing_character")
                    .selected_text(char_lbl(self.voicing.character_target))
                    .show_ui(ui, |ui| {
                        for opt in [
                            CharacterTarget::Keep,
                            CharacterTarget::Natural,
                            CharacterTarget::Analog,
                            CharacterTarget::Digital,
                        ] {
                            ui.selectable_value(
                                &mut self.voicing.character_target,
                                opt,
                                char_lbl(opt),
                            );
                        }
                    });
            });

            let path_lbl = |p: SignalPathStyle| match p {
                SignalPathStyle::Default => "Default (neutral)",
                SignalPathStyle::Miced => "Miced acoustic",
                SignalPathStyle::PickupDirect => "Pickup / DI direct",
                SignalPathStyle::AnalogPreamp => "Analog preamp",
                SignalPathStyle::CleanDigital => "Clean digital",
            };
            ui.horizontal(|ui| {
                ui.label("Signal path:");
                egui::ComboBox::from_id_salt("voicing_path")
                    .selected_text(path_lbl(self.voicing.signal_path))
                    .show_ui(ui, |ui| {
                        for opt in [
                            SignalPathStyle::Default,
                            SignalPathStyle::Miced,
                            SignalPathStyle::PickupDirect,
                            SignalPathStyle::AnalogPreamp,
                            SignalPathStyle::CleanDigital,
                        ] {
                            ui.selectable_value(&mut self.voicing.signal_path, opt, path_lbl(opt));
                        }
                    });
            });

            let var_lbl = |v: SourceCharacterVariant| match v {
                SourceCharacterVariant::Standard => "Standard",
                SourceCharacterVariant::Rich => "Rich",
                SourceCharacterVariant::Basic => "Basic",
                SourceCharacterVariant::Warm => "Warm",
                SourceCharacterVariant::Bright => "Bright",
            };
            ui.horizontal(|ui| {
                ui.label("Voicing:");
                egui::ComboBox::from_id_salt("voicing_variant")
                    .selected_text(var_lbl(self.voicing.source_variant))
                    .show_ui(ui, |ui| {
                        for opt in [
                            SourceCharacterVariant::Standard,
                            SourceCharacterVariant::Rich,
                            SourceCharacterVariant::Basic,
                            SourceCharacterVariant::Warm,
                            SourceCharacterVariant::Bright,
                        ] {
                            ui.selectable_value(
                                &mut self.voicing.source_variant,
                                opt,
                                var_lbl(opt),
                            );
                        }
                    });
            });

            let str_lbl = |s: StraightenMode| match s {
                StraightenMode::Off => "Off",
                StraightenMode::Light => "Light",
                StraightenMode::Moderate => "Moderate",
            };
            ui.horizontal(|ui| {
                ui.label("Straighten:");
                egui::ComboBox::from_id_salt("voicing_straighten")
                    .selected_text(str_lbl(self.voicing.straighten_mode))
                    .show_ui(ui, |ui| {
                        for opt in [
                            StraightenMode::Off,
                            StraightenMode::Light,
                            StraightenMode::Moderate,
                        ] {
                            ui.selectable_value(
                                &mut self.voicing.straighten_mode,
                                opt,
                                str_lbl(opt),
                            );
                        }
                    });
                ui.checkbox(
                    &mut self.voicing.straighten_preserve_bends,
                    "Preserve bends",
                );
            });

            ui.horizontal(|ui| {
                ui.label("Loose guidance:");
                ui.add(
                    egui::Slider::new(&mut self.voicing.profile_guidance_strength, 0.0..=1.0)
                        .fixed_decimals(2),
                );
            });

            let lib = reference_profiles::loose_profile_library();
            let src_name = self
                .sources
                .get(self.rig.source_index)
                .map_or("Unspecified", |s| s.name.as_str());
            if let Some(p) = lib
                .instrument_families
                .iter()
                .find(|ip| {
                    src_name
                        .to_ascii_lowercase()
                        .contains(&ip.id.to_ascii_lowercase())
                })
                .or_else(|| lib.instrument_families.first())
            {
                ui.small(format!(
                    "Loose reference ({}): guides reshaping only — never replaces performance shape or blends original PCM.",
                    p.label
                ));
            }

            let has_track = self.track.is_some();
            let has_cues = self.track.as_ref().is_some_and(|t| t.guitar_cues.is_some());
            let idle = self.job.is_none();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        has_cues && idle,
                        egui::Button::new("Apply Source & Voicing to Dry"),
                    )
                    .clicked()
                {
                    let _ = self.execute(StudioCommand::ResynthesizeDryFromCues, ctx);
                }
                if !has_cues
                    && ui
                        .add_enabled(has_track && idle, egui::Button::new("Extract Dry Shape"))
                        .clicked()
                {
                    let _ = self.execute(StudioCommand::AnalyzeGuitarDry, ctx);
                }
            });
        });
    }

    fn draw_dry_edit_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        StudioTheme::section_frame(ui, "Stage 2 — Surgical Region Edit (Dry Only)", |ui| {
            ui.small(
                "Localized fixes operate ONLY on the Stage 1 dry buffer before optional Mic/Room/Plugin stages. Audio outside the selection stays bit-identical.",
            );
            ui.horizontal(|ui| {
                ui.label("Selection tool:");
                if ui
                    .selectable_label(self.selection_tool == SelectionTool::Range, "Range (Time)")
                    .clicked()
                {
                    self.selection_tool = SelectionTool::Range;
                    self.marquee_hz = None;
                    self.region_edit_draft.kind = "range".into();
                    self.region_edit_draft.min_hz = None;
                    self.region_edit_draft.max_hz = None;
                }
                if ui
                    .selectable_label(
                        self.selection_tool == SelectionTool::Marquee,
                        "Marquee (Time + Hz)",
                    )
                    .clicked()
                {
                    self.selection_tool = SelectionTool::Marquee;
                    self.region_edit_draft.kind = "marquee".into();
                }
            });

            let dur = self.track.as_ref().map_or(10.0, Track::duration).max(0.1);
            ui.horizontal(|ui| {
                let mut changed_t = false;
                changed_t |= ui
                    .add(
                        egui::DragValue::new(&mut self.region_edit_draft.start_s)
                            .prefix("In: ")
                            .suffix(" s")
                            .range(0.0..=dur)
                            .speed(0.01)
                            .fixed_decimals(3),
                    )
                    .changed();
                changed_t |= ui
                    .add(
                        egui::DragValue::new(&mut self.region_edit_draft.end_s)
                            .prefix("Out: ")
                            .suffix(" s")
                            .range(0.0..=dur)
                            .speed(0.01)
                            .fixed_decimals(3),
                    )
                    .changed();
                if changed_t {
                    let a = self
                        .region_edit_draft
                        .start_s
                        .min(self.region_edit_draft.end_s);
                    let b = self
                        .region_edit_draft
                        .start_s
                        .max(self.region_edit_draft.end_s);
                    self.region_edit_draft.start_s = a;
                    self.region_edit_draft.end_s = b;
                    if b - a >= 0.01 {
                        self.selection = Some([a, b]);
                    }
                }
            });

            if self.selection_tool == SelectionTool::Marquee || self.marquee_hz.is_some() {
                let [mut lo, mut hi] = self.marquee_hz.unwrap_or([200.0, 2400.0]);
                let mut changed_hz = false;
                ui.horizontal(|ui| {
                    changed_hz |= ui
                        .add(
                            egui::DragValue::new(&mut lo)
                                .prefix("Min: ")
                                .suffix(" Hz")
                                .range(20.0..=20_000.0)
                                .speed(10.0),
                        )
                        .changed();
                    changed_hz |= ui
                        .add(
                            egui::DragValue::new(&mut hi)
                                .prefix("Max: ")
                                .suffix(" Hz")
                                .range(20.0..=24_000.0)
                                .speed(10.0),
                        )
                        .changed();
                    if ui.small_button("Clear Hz").clicked() {
                        self.marquee_hz = None;
                        self.region_edit_draft.kind = "range".into();
                        self.region_edit_draft.min_hz = None;
                        self.region_edit_draft.max_hz = None;
                    }
                });
                if changed_hz {
                    let a = lo.min(hi);
                    let b = lo.max(hi);
                    self.marquee_hz = Some([a, b]);
                    self.region_edit_draft.kind = "marquee".into();
                    self.region_edit_draft.min_hz = Some(a);
                    self.region_edit_draft.max_hz = Some(b);
                }
            }

            let act_lbl = |a: RegionFixAction| match a {
                RegionFixAction::Straighten => "Straighten pitch",
                RegionFixAction::Smooth => "Smooth harshness",
                RegionFixAction::ReduceNoise => "Reduce region noise",
                RegionFixAction::Stabilize => "Stabilize level",
                RegionFixAction::NoteOnly => "Note only (interpret note)",
            };
            ui.horizontal(|ui| {
                ui.label("Fix action:");
                egui::ComboBox::from_id_salt("region_fix_action")
                    .selected_text(act_lbl(self.region_edit_draft.action))
                    .show_ui(ui, |ui| {
                        for opt in [
                            RegionFixAction::Stabilize,
                            RegionFixAction::Straighten,
                            RegionFixAction::Smooth,
                            RegionFixAction::ReduceNoise,
                            RegionFixAction::NoteOnly,
                        ] {
                            ui.selectable_value(
                                &mut self.region_edit_draft.action,
                                opt,
                                act_lbl(opt),
                            );
                        }
                    });
            });

            ui.horizontal(|ui| {
                ui.label("Note:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.region_edit_draft.note)
                        .hint_text("e.g. smooth bright edge, reduce hiss, stabilize"),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Strength:");
                ui.add(
                    egui::Slider::new(&mut self.region_edit_draft.strength, 0.0..=1.0)
                        .fixed_decimals(2),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Crossfade:");
                ui.add(
                    egui::Slider::new(&mut self.region_edit_draft.crossfade_ms, 4.0..=60.0)
                        .suffix(" ms")
                        .fixed_decimals(1),
                );
            });

            let can_apply = self
                .track
                .as_ref()
                .is_some_and(|t| t.dry.is_some() && t.guitar_cues.is_some())
                && self.job.is_none();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(can_apply, egui::Button::new("Apply Surgical Fix to Dry"))
                    .clicked()
                {
                    let _ = self.execute(StudioCommand::ApplyRegionEdit, ctx);
                }
            });

            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label("Region preview:");
                ui.selectable_value(
                    &mut self.region_preview,
                    RegionPreviewMode::Off,
                    "Full Track",
                );
                ui.selectable_value(
                    &mut self.region_preview,
                    RegionPreviewMode::Before,
                    "Region Before",
                );
                ui.selectable_value(
                    &mut self.region_preview,
                    RegionPreviewMode::After,
                    "Region After",
                );
            });

            if let Some(rep) = self.last_region_edit.as_ref() {
                ui.separator();
                ui.small(format!(
                    "Last edit: samples {}..{} · {} notes · action {:?} · outside unchanged: {}",
                    rep.start_sample,
                    rep.end_sample,
                    rep.affected_notes,
                    rep.effective_action,
                    if rep.outside_bit_identical {
                        "YES"
                    } else {
                        "NO"
                    }
                ));
                if let Some(msg) = rep.unresolved_note.as_ref() {
                    ui.colored_label(StudioTheme::WARNING, msg);
                }
            }
        });
    }

    fn draw_microphone_tab(&mut self, ui: &mut egui::Ui) {
        StudioTheme::section_frame(ui, "Stage 3 — Optional Microphone", |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.mic_enabled, "Enable Microphone Stage");
                if self.mic_enabled {
                    ui.colored_label(StudioTheme::STAGE_ACTIVE, "ACTIVE");
                } else {
                    ui.colored_label(StudioTheme::STAGE_BYPASS, "BYPASSED (Dry pass-through)");
                }
            });
        });
        ui.add_space(4.);

        // Multi-mic slot bar
        ui.horizontal_wrapped(|ui| {
            for idx in 0..self.rig.mics.len() {
                let lbl = self.rig.mics[idx].label.clone();
                if ui
                    .selectable_label(self.rig.selected_mic == idx, lbl)
                    .clicked()
                {
                    self.rig.selected_mic = idx;
                }
            }
            if self.rig.mics.len() < 4 && ui.button("+ Add Mic").clicked() {
                self.rig.add_mic_slot();
            }
            if self.rig.mics.len() > 1 && ui.button("Remove").clicked() {
                self.rig.remove_selected_mic();
            }
        });

        // Search + Pattern filter across curated factual microphones
        ui.horizontal(|ui| {
            ui.label(format!("Search ({} curated):", self.mics.len()));
            ui.text_edit_singleline(&mut self.mic_search);
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(self.mic_pattern_filter.is_none(), "All")
                .clicked()
            {
                self.mic_pattern_filter = None;
            }
            for pat in Pattern::ALL {
                if ui
                    .selectable_label(self.mic_pattern_filter == Some(pat), pat.label())
                    .clicked()
                {
                    self.mic_pattern_filter = Some(pat);
                }
            }
        });

        let q = self.mic_search.to_ascii_lowercase();
        let active_cat = self.rig.active_mic().catalog_index;
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt("mic_catalog_list")
            .max_height(84.)
            .show(ui, |ui| {
                for (idx, m) in self.mics.iter().enumerate() {
                    if let Some(pf) = self.mic_pattern_filter {
                        if m.pattern != pf {
                            continue;
                        }
                    }
                    if !q.is_empty()
                        && !m.brand.to_ascii_lowercase().contains(&q)
                        && !m.model.to_ascii_lowercase().contains(&q)
                        && !m.pattern.label().to_ascii_lowercase().contains(&q)
                    {
                        continue;
                    }
                    if ui
                        .selectable_label(active_cat == idx, m.display_name())
                        .clicked()
                    {
                        picked = Some(idx);
                    }
                }
            });
        if let Some(idx) = picked {
            self.rig.active_mic_mut().catalog_index = idx;
        }

        // Mic thumbnail + small polar chart side-by-side
        if let Some(entry) = self.mics.get(self.rig.active_mic().catalog_index) {
            ui.separator();
            ui.horizontal(|ui| {
                let (r, p) = ui.allocate_painter(Vec2::splat(48.), egui::Sense::hover());
                p.rect_filled(r.rect, 4., StudioTheme::BG_CANVAS);
                p.rect_stroke(
                    r.rect,
                    4.,
                    Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
                    egui::StrokeKind::Inside,
                );
                let c = r.rect.center();
                p.circle_stroke(
                    c - Vec2::new(0., 7.),
                    8.,
                    Stroke::new(1.5, Color32::from_rgb(255, 192, 106)),
                );
                p.rect_stroke(
                    Rect::from_center_size(c + Vec2::new(0., 9.), Vec2::new(10., 16.)),
                    2.,
                    Stroke::new(1.4, Color32::LIGHT_GRAY),
                    egui::StrokeKind::Middle,
                );
                let eff_pat = self.rig.active_mic().effective_pattern(&self.mics);
                draw_polar_mini(ui, eff_pat, 48.);
                ui.vertical(|ui| {
                    ui.strong(format!("{} {}", entry.brand, entry.model));
                    ui.small(format!(
                        "{} · Published: {} · Renderer: {}",
                        entry.transducer.label(),
                        entry.pattern.label(),
                        eff_pat.label()
                    ));
                    let band_txt = match entry.published_range_hz {
                        Some((lo, hi)) => format!("{lo:.0}–{hi:.0} Hz"),
                        None => "Band not listed".into(),
                    };
                    let sens_txt = match entry.published_sensitivity_dbv {
                        Some(dbv) => format!("{dbv:.1} dBV/Pa"),
                        None => "Sens. not listed".into(),
                    };
                    ui.small(format!("Published: {band_txt} · {sens_txt}"));
                });
            });
            ui.horizontal(|ui| {
                ui.label("Pickup pattern (independent):");
                let current_pat = self.rig.active_mic().effective_pattern(&self.mics);
                egui::ComboBox::from_id_salt("mic_independent_pattern")
                    .selected_text(current_pat.label())
                    .show_ui(ui, |ui| {
                        for p in Pattern::ALL {
                            if ui.selectable_label(current_pat == p, p.label()).clicked() {
                                self.rig.active_mic_mut().pattern_override = Some(p);
                            }
                        }
                    });
                if self.rig.active_mic().pattern_override.is_some()
                    && ui.small_button("Model default").clicked()
                {
                    self.rig.active_mic_mut().pattern_override = None;
                }
            });

            // Aim menu (X tilt, Y roll, Z heading, snap, 6 grid directions, Snap to source, Follow source)
            egui::CollapsingHeader::new("Aim & Rotation (rotates without moving)")
                .default_open(true)
                .show(ui, |ui| {
                    let offset = self.rig.active_mic().offset_m;
                    let slot = self.rig.active_mic_mut();
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut slot.aim.follow_source, "Follow source");
                        if ui.button("Snap to source").clicked() {
                            slot.aim.point_at_source(offset);
                        }
                    });
                    ui.horizontal(|ui| {
                        let mut changed_angle = false;
                        changed_angle |= ui
                            .add(
                                egui::DragValue::new(&mut slot.aim.tilt_x_deg)
                                    .prefix("X tilt: ")
                                    .suffix("°")
                                    .range(-90. ..=90.)
                                    .speed(0.5),
                            )
                            .changed();
                        changed_angle |= ui
                            .add(
                                egui::DragValue::new(&mut slot.aim.roll_y_deg)
                                    .prefix("Y roll: ")
                                    .suffix("°")
                                    .range(-180. ..=180.)
                                    .speed(0.5),
                            )
                            .changed();
                        changed_angle |= ui
                            .add(
                                egui::DragValue::new(&mut slot.aim.heading_z_deg)
                                    .prefix("Z head: ")
                                    .suffix("°")
                                    .range(-180. ..=180.)
                                    .speed(0.5),
                            )
                            .changed();
                        if changed_angle {
                            slot.aim.follow_source = false;
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.small("Snap:");
                        for step in [0.0_f32, 15.0, 45.0] {
                            let lbl = if step == 0.0 {
                                "Off".into()
                            } else {
                                format!("{step:.0}°")
                            };
                            if ui
                                .selectable_label((self.aim_snap_step - step).abs() < 0.1, lbl)
                                .clicked()
                            {
                                self.aim_snap_step = step;
                                if step > 0.0 {
                                    slot.aim.snap_angles(step);
                                }
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for dir in GridDirection::ALL {
                            if ui.small_button(dir.label()).clicked() {
                                slot.aim.set_grid_direction(dir);
                            }
                        }
                    });
                });

            ui.small(&entry.curve_provenance);
            ui.small(entry.shared_model_explanation());
            ui.small(format!(
                "Published: Noise={} · MaxSPL={} · Curves={}",
                entry
                    .published_self_noise_dba
                    .map_or_else(|| "N/A".to_owned(), |v| format!("{v:.1} dB(A)")),
                entry
                    .published_max_spl_db
                    .map_or_else(|| "N/A".to_owned(), |v| format!("{v:.0} dB")),
                entry.published_curves.len()
            ));
        }
    }

    fn draw_materials_tab(&mut self, ui: &mut egui::Ui) {
        StudioTheme::section_frame(ui, "Stage 4 — Space / Room Stage", |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.room_enabled, "Enable Space / Room Stage");
                if self.room_enabled {
                    ui.colored_label(StudioTheme::STAGE_ACTIVE, "ACTIVE");
                } else {
                    ui.colored_label(StudioTheme::STAGE_BYPASS, "BYPASSED (No room reflections)");
                }
            });
        });
        ui.add_space(4.);
        let is_open = self.rig.room_preset == RoomPreset::Open;
        let is_direct = self.rig.room_preset == RoomPreset::Direct;
        if is_direct {
            ui.small("Room preset is Direct (no added room reflections). Switch Room to Small/Medium/Large/Open to apply 6-band surface profiles.");
        }
        // Assigned 6-face surfaces (or Floor/Ground only for Open)
        for face in 0..6 {
            if is_open && face != 4 {
                continue;
            }
            let current = self.rig.surface_faces[face];
            let name = self
                .surfaces
                .get(current)
                .map_or("Unknown", |s| s.name.as_str());
            ui.horizontal(|ui| {
                ui.label(format!("{}:", FACE_LABELS[face]));
                egui::ComboBox::from_id_salt(("surface_combo", face))
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for (i, s) in self.surfaces.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.rig.surface_faces[face],
                                i,
                                s.name.as_str(),
                            );
                        }
                    });
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(format!("Search ({} 6-band):", self.surfaces.len()));
            ui.text_edit_singleline(&mut self.surface_search);
        });
        let q = self.surface_search.to_ascii_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("surface_list")
            .max_height(110.)
            .show(ui, |ui| {
                for (i, s) in self.surfaces.iter().enumerate() {
                    if !q.is_empty()
                        && !s.name.to_ascii_lowercase().contains(&q)
                        && !s.category.to_ascii_lowercase().contains(&q)
                    {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.small(format!("{} (mean R={:.2})", s.name, s.mean_reflectance()));
                        if !is_open
                            && ui
                                .small_button("Walls")
                                .on_hover_text("Assign to all 4 walls (-X, +X, -Y, +Y)")
                                .clicked()
                        {
                            self.rig.set_all_walls(i);
                        }
                        if ui
                            .small_button("Floor")
                            .on_hover_text("Assign to Floor / Ground")
                            .clicked()
                        {
                            self.rig.surface_faces[4] = i;
                        }
                        if !is_open
                            && ui
                                .small_button("Ceil")
                                .on_hover_text("Assign to Ceiling")
                                .clicked()
                        {
                            self.rig.surface_faces[5] = i;
                        }
                    });
                }
            });
        // Info sheet showing BOTH raw 6-band absorption AND clamped passive pressure reflectance
        let inspect_idx = if is_open {
            self.rig.surface_faces[4]
        } else {
            self.rig.surface_faces[0]
        };
        if let Some(surf) = self.surfaces.get(inspect_idx) {
            let r = surf.pressure_reflectance();
            let a = surf.absorption_6band;
            let eff = surf.effective_absorption();
            ui.separator();
            ui.small(format!("Inspected surface: {}", surf.name));
            ui.small("Bands: 125 / 250 / 500 / 1k / 2k / 4k Hz");
            ui.small(format!(
                "Published raw α: {:.2} / {:.2} / {:.2} / {:.2} / {:.2} / {:.2}",
                a[0], a[1], a[2], a[3], a[4], a[5]
            ));
            ui.small(format!(
                "Effective α (renderer): {:.2} / {:.2} / {:.2} / {:.2} / {:.2} / {:.2}",
                eff[0], eff[1], eff[2], eff[3], eff[4], eff[5]
            ));
            ui.small(format!(
                "Passive R=√(1−α_eff): {:.2} / {:.2} / {:.2} / {:.2} / {:.2} / {:.2}",
                r[0], r[1], r[2], r[3], r[4], r[5]
            ));
            ui.small(format!("Assembly: {}", surf.assembly));
            ui.small(format!(
                "Standard: {} ({})",
                surf.standard, surf.response_kind
            ));
            ui.small(format!("Biot-JCA: {}", surf.biot.status));
            ui.small(&surf.provenance);
            ui.small(&surf.notes);
        }
    }

    fn draw_room_tab(&mut self, ui: &mut egui::Ui) {
        StudioTheme::section_frame(ui, "Stage 4 — Optional Space / Room", |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.room_enabled, "Enable Space / Room Stage");
                if self.room_enabled {
                    ui.colored_label(StudioTheme::STAGE_ACTIVE, "ACTIVE");
                } else {
                    ui.colored_label(StudioTheme::STAGE_BYPASS, "BYPASSED (No room reflections)");
                }
            });
        });
        ui.add_space(4.);
        ui.horizontal_wrapped(|ui| {
            for preset in RoomPreset::ALL {
                if ui
                    .selectable_label(self.rig.room_preset == preset, preset.label())
                    .clicked()
                {
                    self.rig.apply_preset(preset);
                }
            }
        });
        ui.separator();
        // Lossless unit selector (never resizes physical room!)
        ui.horizontal_wrapped(|ui| {
            ui.label("Ruler units:");
            for u in Unit::ALL {
                ui.selectable_value(&mut self.rig.unit, u, u.suffix());
            }
        });

        ui.horizontal(|ui| {
            if ui.small_button("Link All").clicked() {
                self.rig.link_all();
            }
            if ui.small_button("Unlink All").clicked() {
                self.rig.unlink_all();
            }
            if ui.small_button("Reset Source (Center, 5 ft)").clicked() {
                self.rig.reset_source_center_5ft();
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.rig.source_lock_center_xy, "Lock center (X/Y)");
            ui.checkbox(&mut self.rig.source_lock_height_5ft, "Lock height (5 ft)");
        });
        let u = self.rig.unit;
        let dec = u.decimals();
        let axis_names = ["Width (X)", "Length / Depth (Y)", "Height (Z)"];
        for axis in 0..3 {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.rig.dim_linked[axis], "Link");
                let mut val_u = u.from_metres(self.rig.dimensions_m[axis]);
                let min_u = u.from_metres(1.5);
                let max_u = u.from_metres(40.0);
                let resp = ui.add(
                    egui::DragValue::new(&mut val_u)
                        .prefix(format!("{}: ", axis_names[axis]))
                        .suffix(format!(" {}", u.suffix()))
                        .range(min_u..=max_u)
                        .fixed_decimals(dec)
                        .speed((max_u - min_u) * 0.002),
                );
                if resp.changed() {
                    self.rig.set_dimension_m(axis, u.to_metres(val_u));
                }
            });
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.rig.temperature_c)
                    .prefix("Air temp: ")
                    .suffix(" °C")
                    .range(-20.0..=50.0)
                    .speed(0.2),
            );
            ui.small(format!(
                "c = {:.2} m/s",
                studio_catalog::speed_of_sound_celsius(f64::from(self.rig.temperature_c))
            ));
        });
        let src = self.rig.source_pos_m();
        let mic_off = self.rig.active_mic().offset_m;
        ui.small(format!(
            "Source origin (baseline): X={:.2}, Y={:.2}, Z={:.2} {} (horizontally centered, 5 ft height baseline)",
            u.from_metres(src[0]),
            u.from_metres(src[1]),
            u.from_metres(src[2]),
            u.suffix()
        ));
        ui.small(format!(
            "Selected Mic offset: ΔX={:+.2}, ΔY={:+.2}, ΔZ={:+.2} {}",
            u.from_metres(mic_off[0]),
            u.from_metres(mic_off[1]),
            u.from_metres(mic_off[2]),
            u.suffix()
        ));
        ui.small("Virtual capture choice — does not claim to recover the historical recording room or mic.");
    }

    fn draw_plugin_tab(&mut self, ui: &mut egui::Ui) {
        StudioTheme::section_frame(ui, "Stage 5 — Optional Plugin Slot", |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.plugin_slot.enabled, "Enable Plugin Slot");
                if self.plugin_slot.enabled && self.plugin_slot.plugin_id != PluginId::None {
                    ui.colored_label(StudioTheme::STAGE_ACTIVE, "ACTIVE");
                } else {
                    ui.colored_label(StudioTheme::STAGE_BYPASS, "BYPASSED (Clean pass-through)");
                }
            });
            ui.small(
                "Inserted strictly after Stage 4 and before final output. Bypassed by default.",
            );
            ui.add_space(4.);

            let pid_lbl = |id: PluginId| match id {
                PluginId::None => "None (Bypass)",
                PluginId::GentleEq => "Gentle EQ (Low / High Shelf)",
                PluginId::SoftDynamics => "Soft Dynamics",
                PluginId::NoiseControl => "Noise Control",
                PluginId::PhaseLimiter => "Phase-Safe Ceiling Limiter",
            };
            ui.horizontal(|ui| {
                ui.label("Plugin:");
                egui::ComboBox::from_id_salt("stage5_plugin_id")
                    .selected_text(pid_lbl(self.plugin_slot.plugin_id))
                    .show_ui(ui, |ui| {
                        for id in [
                            PluginId::None,
                            PluginId::GentleEq,
                            PluginId::SoftDynamics,
                            PluginId::NoiseControl,
                            PluginId::PhaseLimiter,
                        ] {
                            ui.selectable_value(&mut self.plugin_slot.plugin_id, id, pid_lbl(id));
                        }
                    });
            });

            match self.plugin_slot.plugin_id {
                PluginId::None => {
                    ui.small("Select a built-in DSP module above and enable Stage 5 to process.");
                }
                PluginId::GentleEq => {
                    ui.horizontal(|ui| {
                        ui.label("Low shelf:");
                        ui.add(
                            egui::Slider::new(&mut self.plugin_slot.eq_low_db, -4.0..=4.0)
                                .suffix(" dB")
                                .fixed_decimals(1),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("High shelf:");
                        ui.add(
                            egui::Slider::new(&mut self.plugin_slot.eq_high_db, -4.0..=4.0)
                                .suffix(" dB")
                                .fixed_decimals(1),
                        );
                    });
                }
                PluginId::SoftDynamics | PluginId::NoiseControl => {
                    ui.horizontal(|ui| {
                        ui.label("Amount:");
                        ui.add(
                            egui::Slider::new(&mut self.plugin_slot.amount, 0.0..=1.0)
                                .fixed_decimals(2),
                        );
                    });
                }
                PluginId::PhaseLimiter => {
                    ui.horizontal(|ui| {
                        ui.label("Ceiling:");
                        ui.add(
                            egui::Slider::new(&mut self.plugin_slot.ceiling_dbfs, -12.0..=-0.1)
                                .suffix(" dBFS")
                                .fixed_decimals(1),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Amount:");
                        ui.add(
                            egui::Slider::new(&mut self.plugin_slot.amount, 0.0..=1.0)
                                .fixed_decimals(2),
                        );
                    });
                }
            }
        });
    }

    fn draw_linked_top_front_plans(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        budget_h: f32,
    ) {
        let dim = self.rig.dimensions_m;
        let src = self.rig.source_pos_m();
        let u = self.rig.unit;
        let avail_w = ui.available_width().max(220.0);
        let plans_h = (budget_h - 46.0).max(112.0);
        let top_h = (plans_h * 0.55).clamp(64.0, 126.0);
        let front_h = (plans_h * 0.45).clamp(54.0, 104.0);

        // Shared X mapping across Top and Front plans!
        let vis_w = (dim[0] / self.plan_zoom).max(0.5);
        let step_m = u.nice_grid_step_m(vis_w);
        let step_u = u.from_metres(step_m);

        ui.horizontal(|ui| {
            ui.small(format!(
                "Scale: {:.2} {} / sq  |  Zoom {:.0}%",
                step_u,
                u.suffix(),
                self.plan_zoom * 100.0
            ));
            if ui.small_button("Fit plans").clicked() {
                self.plan_zoom = 1.0;
                self.plan_pan_x_m = 0.0;
            }
        });

        let x_center = dim[0] * 0.5 + self.plan_pan_x_m;
        let x_min = x_center - vis_w * 0.5;

        // Shared wheel / pinch zoom on either plan zooms BOTH together!
        let mut handle_zoom = |resp: &egui::Response| {
            if resp.hovered() {
                let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                let pinch = ctx.input(|i| i.zoom_delta());
                if (pinch - 1.0).abs() > 0.005 {
                    self.plan_zoom = (self.plan_zoom * pinch).clamp(0.5, 4.0);
                } else if scroll.abs() > 1.0 {
                    let f = if scroll > 0. { 1.12 } else { 0.89 };
                    self.plan_zoom = (self.plan_zoom * f).clamp(0.5, 4.0);
                }
            }
        };

        // ---- TOP PLAN (X = Width, Y = Depth) ----
        ui.small(format!(
            "Top Plan (Width {:.2} × Depth {:.2} {})",
            u.from_metres(dim[0]),
            u.from_metres(dim[1]),
            u.suffix()
        ));
        let (r_top, p_top) =
            ui.allocate_painter(Vec2::new(avail_w, top_h), egui::Sense::click_and_drag());
        handle_zoom(&r_top);
        p_top.rect_filled(r_top.rect, 4., StudioTheme::BG_CANVAS);
        p_top.rect_stroke(
            r_top.rect,
            4.,
            Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
            egui::StrokeKind::Inside,
        );
        if !self.mic_enabled {
            p_top.text(
                r_top.rect.right_top() + Vec2::new(-6., 4.),
                egui::Align2::RIGHT_TOP,
                "Mic Bypassed",
                egui::FontId::proportional(9.5),
                StudioTheme::TEXT_MUTED,
            );
        }
        let box_top = r_top.rect.shrink(6.);
        let px_per_m = box_top.width() / vis_w;
        let to_top = |wx: f32, wy: f32| {
            let sx = box_top.left() + (wx - x_min) * px_per_m;
            let sy = box_top.center().y + (wy - dim[1] * 0.5) * px_per_m;
            Pos2::new(sx, sy)
        };
        // Grid lines at exact `step_m` intervals
        let gx0 = (x_min / step_m).floor() as i32;
        let gx1 = ((x_min + vis_w) / step_m).ceil() as i32;
        for g in gx0..=gx1 {
            let x = box_top.left() + (g as f32 * step_m - x_min) * px_per_m;
            if x >= box_top.left() && x <= box_top.right() {
                p_top.line_segment(
                    [Pos2::new(x, box_top.top()), Pos2::new(x, box_top.bottom())],
                    Stroke::new(0.7, Color32::from_gray(42)),
                );
            }
        }
        let gy_max = (dim[1] / step_m).ceil() as i32 + 1;
        for g in -1..=gy_max {
            let y = to_top(0., g as f32 * step_m).y;
            if y >= box_top.top() && y <= box_top.bottom() {
                p_top.line_segment(
                    [Pos2::new(box_top.left(), y), Pos2::new(box_top.right(), y)],
                    Stroke::new(0.7, Color32::from_gray(42)),
                );
            }
        }
        // Room boundary (dashed/open for Open preset)
        let tl = to_top(0., 0.);
        let br = to_top(dim[0], dim[1]);
        let room_col = if self.rig.room_preset == RoomPreset::Open {
            Color32::from_rgb(95, 145, 115)
        } else {
            Color32::from_rgb(130, 160, 200)
        };
        p_top.rect_stroke(
            Rect::from_min_max(tl, br),
            0.,
            Stroke::new(1.3, room_col),
            egui::StrokeKind::Middle,
        );

        // Source at origin with forward arrow (+Y) & radiation envelope
        let src_pt = to_top(src[0], src[1]);
        let src_entry = self.sources.get(self.rig.source_index);
        if let Some(se) = src_entry {
            draw_source_radiation_ring(&p_top, src_pt, Vec2::new(0., 1.), se.radiation, 22.);
            draw_source_symbol(
                &p_top,
                src_pt,
                8.,
                se.symbol,
                Color32::from_rgb(123, 181, 255),
            );
        }
        p_top.line_segment(
            [src_pt, src_pt + Vec2::new(0., 16.)],
            Stroke::new(1.8, Color32::from_rgb(123, 181, 255)),
        );

        // Drag active mic in Top plan (X, Y)
        if (r_top.dragged() || r_top.clicked()) && !ctx.input(|i| i.modifiers.shift) {
            if let Some(pt) = r_top.interact_pointer_pos() {
                let wx = x_min + (pt.x - box_top.left()) / px_per_m;
                let wy = dim[1] * 0.5 + (pt.y - box_top.center().y) / px_per_m;
                let m = self.rig.active_mic_mut();
                m.offset_m[0] = wx - src[0];
                m.offset_m[1] = wy - src[1];
                self.rig.clamp_all();
            }
        } else if r_top.dragged() && ctx.input(|i| i.modifiers.shift) {
            self.plan_pan_x_m -= r_top.drag_delta().x / px_per_m;
        }

        // Draw all mics in Top plan
        for (idx, m) in self.rig.mics.iter().enumerate() {
            let mp = to_top(src[0] + m.offset_m[0], src[1] + m.offset_m[1]);
            let selected = idx == self.rig.selected_mic;
            let col = if selected {
                Color32::from_rgb(255, 192, 106)
            } else {
                Color32::from_rgb(165, 175, 190)
            };
            let aim = m.aim.direction();
            let aim_2d = Vec2::new(aim[0], aim[1]);
            if self.rig.show_patterns {
                let pat = m.effective_pattern(&self.mics);
                draw_polar_overlay_on_plan(&p_top, mp, aim_2d, pat, 26.);
            }
            p_top.line_segment([src_pt, mp], Stroke::new(0.8, Color32::from_gray(95)));
            p_top.circle_filled(mp, if selected { 5.0 } else { 3.5 }, col);
            let aim_dir2 = if aim_2d.length_sq() > 1e-6 {
                aim_2d.normalized()
            } else {
                Vec2::ZERO
            };
            p_top.line_segment([mp, mp + aim_dir2 * 14.], Stroke::new(1.6, col));
        }

        // ---- FRONT PLAN (X = Width linked identically to Top, Z = Height) ----
        ui.small(format!(
            "Front Plan (Width {:.2} × Height {:.2} {})",
            u.from_metres(dim[0]),
            u.from_metres(dim[2]),
            u.suffix()
        ));
        let (r_front, p_front) =
            ui.allocate_painter(Vec2::new(avail_w, front_h), egui::Sense::click_and_drag());
        handle_zoom(&r_front);
        p_front.rect_filled(r_front.rect, 4., StudioTheme::BG_CANVAS);
        p_front.rect_stroke(
            r_front.rect,
            4.,
            Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
            egui::StrokeKind::Inside,
        );
        let box_front = r_front.rect.shrink(6.);
        // Exact same px_per_m and x_min as Top plan so vertical grid lines align 1:1!
        let to_front = |wx: f32, wz: f32| {
            let sx = box_front.left() + (wx - x_min) * px_per_m;
            let sy = box_front.bottom() - wz * px_per_m;
            Pos2::new(sx, sy)
        };
        for g in gx0..=gx1 {
            let x = box_front.left() + (g as f32 * step_m - x_min) * px_per_m;
            if x >= box_front.left() && x <= box_front.right() {
                p_front.line_segment(
                    [
                        Pos2::new(x, box_front.top()),
                        Pos2::new(x, box_front.bottom()),
                    ],
                    Stroke::new(0.7, Color32::from_gray(42)),
                );
            }
        }
        let gz_max = (dim[2] / step_m).ceil() as i32 + 1;
        for g in 0..=gz_max {
            let y = to_front(0., g as f32 * step_m).y;
            if y >= box_front.top() && y <= box_front.bottom() {
                p_front.line_segment(
                    [
                        Pos2::new(box_front.left(), y),
                        Pos2::new(box_front.right(), y),
                    ],
                    Stroke::new(0.7, Color32::from_gray(42)),
                );
            }
        }
        let f_tl = to_front(0., dim[2]);
        let f_br = to_front(dim[0], 0.);
        if self.rig.room_preset == RoomPreset::Open {
            // Ground line only (not a closed cube)
            p_front.line_segment(
                [
                    Pos2::new(box_front.left(), f_br.y),
                    Pos2::new(box_front.right(), f_br.y),
                ],
                Stroke::new(2.0, Color32::from_rgb(95, 165, 115)),
            );
        } else {
            p_front.rect_stroke(
                Rect::from_min_max(f_tl, f_br),
                0.,
                Stroke::new(1.3, room_col),
                egui::StrokeKind::Middle,
            );
        }

        let src_f = to_front(src[0], src[2]);
        if let Some(se) = src_entry {
            draw_source_symbol(
                &p_front,
                src_f,
                7.,
                se.symbol,
                Color32::from_rgb(123, 181, 255),
            );
        }

        // Drag active mic in Front plan (X, Z)
        if (r_front.dragged() || r_front.clicked()) && !ctx.input(|i| i.modifiers.shift) {
            if let Some(pt) = r_front.interact_pointer_pos() {
                let wx = x_min + (pt.x - box_front.left()) / px_per_m;
                let wz = (box_front.bottom() - pt.y) / px_per_m;
                let m = self.rig.active_mic_mut();
                m.offset_m[0] = wx - src[0];
                m.offset_m[2] = wz - src[2];
                self.rig.clamp_all();
            }
        } else if r_front.dragged() && ctx.input(|i| i.modifiers.shift) {
            self.plan_pan_x_m -= r_front.drag_delta().x / px_per_m;
        }

        for (idx, m) in self.rig.mics.iter().enumerate() {
            let mp = to_front(src[0] + m.offset_m[0], src[2] + m.offset_m[2]);
            let selected = idx == self.rig.selected_mic;
            let col = if selected {
                Color32::from_rgb(255, 192, 106)
            } else {
                Color32::from_rgb(165, 175, 190)
            };
            let aim = m.aim.direction();
            let aim_f = Vec2::new(aim[0], -aim[2]);
            if self.rig.show_patterns {
                let pat = m.effective_pattern(&self.mics);
                draw_polar_overlay_on_plan(&p_front, mp, aim_f, pat, 22.);
            }
            p_front.circle_filled(mp, if selected { 5.0 } else { 3.5 }, col);
            let aim_dir_f = if aim_f.length_sq() > 1e-6 {
                aim_f.normalized()
            } else {
                Vec2::ZERO
            };
            p_front.line_segment([mp, mp + aim_dir_f * 13.], Stroke::new(1.6, col));
        }
    }

    fn draw_3d_grid_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, budget_h: f32) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.cam_3d.perspective, true, "Perspective");
            ui.selectable_value(&mut self.cam_3d.perspective, false, "Orthographic");
            if ui.small_button("Fit view").clicked() {
                self.cam_3d = Camera3D::default();
            }
            ui.small("Drag: orbit · Shift+drag: pan");
        });
        let avail_w = ui.available_width().max(220.0);
        let grid_h = (budget_h - 24.0).clamp(112.0, 224.0);
        let (resp, p) =
            ui.allocate_painter(Vec2::new(avail_w, grid_h), egui::Sense::click_and_drag());
        p.rect_filled(resp.rect, 4., StudioTheme::BG_CANVAS);
        p.rect_stroke(
            resp.rect,
            4.,
            Stroke::new(1.0, StudioTheme::BORDER_SUBTLE),
            egui::StrokeKind::Inside,
        );

        // Camera navigation ONLY modifies self.cam_3d; NEVER moves or rotates any mic!
        if resp.dragged() {
            let d = resp.drag_delta();
            if ctx.input(|i| i.modifiers.shift) {
                self.cam_3d.pan[0] += d.x;
                self.cam_3d.pan[1] += d.y;
            } else {
                self.cam_3d.yaw_deg = (self.cam_3d.yaw_deg + d.x * 0.6) % 360.0;
                self.cam_3d.pitch_deg = (self.cam_3d.pitch_deg + d.y * 0.5).clamp(-85.0, 85.0);
            }
        }
        if resp.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 1.0 {
                self.cam_3d.zoom =
                    (self.cam_3d.zoom * if scroll > 0. { 1.1 } else { 0.9 }).clamp(0.4, 3.5);
            }
        }

        let dim = self.rig.dimensions_m;
        let center3 = [dim[0] * 0.5, dim[1] * 0.5, dim[2] * 0.5];
        let radius = (dim[0].powi(2) + dim[1].powi(2) + dim[2].powi(2))
            .sqrt()
            .max(1.0);
        let yaw = self.cam_3d.yaw_deg.to_radians();
        let pitch = self.cam_3d.pitch_deg.to_radians();
        let (sy, cy) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let scale = (resp.rect.height().min(resp.rect.width()) * 0.62 / radius) * self.cam_3d.zoom;
        let origin = resp.rect.center() + Vec2::new(self.cam_3d.pan[0], self.cam_3d.pan[1]);

        let proj = |pt: [f32; 3]| -> Pos2 {
            let x = pt[0] - center3[0];
            let y = pt[1] - center3[1];
            let z = pt[2] - center3[2];
            // Rotate around Z (yaw), then X (pitch)
            let rx = x * cy - y * sy;
            let ry = x * sy + y * cy;
            let rz = z * cp - ry * sp;
            let depth = z * sp + ry * cp;
            let persp = if self.cam_3d.perspective {
                let d_cam = radius * 2.2;
                (d_cam / (d_cam - depth).max(0.5)).clamp(0.4, 2.4)
            } else {
                1.0
            };
            Pos2::new(origin.x + rx * scale * persp, origin.y - rz * scale * persp)
        };

        // Floor grid
        let steps = 4;
        for i in 0..=steps {
            let f = i as f32 / steps as f32;
            p.line_segment(
                [proj([f * dim[0], 0., 0.]), proj([f * dim[0], dim[1], 0.])],
                Stroke::new(0.8, Color32::from_gray(55)),
            );
            p.line_segment(
                [proj([0., f * dim[1], 0.]), proj([dim[0], f * dim[1], 0.])],
                Stroke::new(0.8, Color32::from_gray(55)),
            );
        }
        // Closed room wireframe for Small/Medium/Large (ground only for Open)
        if self.rig.room_preset != RoomPreset::Open {
            let c = [
                [0., 0., 0.],
                [dim[0], 0., 0.],
                [dim[0], dim[1], 0.],
                [0., dim[1], 0.],
                [0., 0., dim[2]],
                [dim[0], 0., dim[2]],
                [dim[0], dim[1], dim[2]],
                [0., dim[1], dim[2]],
            ];
            let edges = [
                (0, 1),
                (1, 2),
                (2, 3),
                (3, 0),
                (4, 5),
                (5, 6),
                (6, 7),
                (7, 4),
                (0, 4),
                (1, 5),
                (2, 6),
                (3, 7),
            ];
            for (a, b) in edges {
                p.line_segment(
                    [proj(c[a]), proj(c[b])],
                    Stroke::new(1.1, Color32::from_rgb(110, 145, 185)),
                );
            }
        }
        // Source & forward arrow
        let src = self.rig.source_pos_m();
        let sp2 = proj(src);
        let sf2 = proj([src[0], src[1] + 0.5, src[2]]);
        p.circle_filled(sp2, 5.5, Color32::from_rgb(123, 181, 255));
        p.line_segment(
            [sp2, sf2],
            Stroke::new(1.8, Color32::from_rgb(123, 181, 255)),
        );

        // Microphones & aim vectors
        for (idx, m) in self.rig.mics.iter().enumerate() {
            let pos = [
                src[0] + m.offset_m[0],
                src[1] + m.offset_m[1],
                src[2] + m.offset_m[2],
            ];
            let aim = m.aim.direction();
            let tip = [
                pos[0] + aim[0] * 0.45,
                pos[1] + aim[1] * 0.45,
                pos[2] + aim[2] * 0.45,
            ];
            let mp = proj(pos);
            let col = if idx == self.rig.selected_mic {
                Color32::from_rgb(255, 192, 106)
            } else {
                Color32::from_rgb(165, 175, 190)
            };
            p.line_segment([sp2, mp], Stroke::new(0.8, Color32::from_gray(95)));
            p.circle_filled(
                mp,
                if idx == self.rig.selected_mic {
                    5.0
                } else {
                    3.5
                },
                col,
            );
            p.line_segment([mp, proj(tip)], Stroke::new(1.8, col));
        }
    }

    fn draw_capture_summary_and_actions(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pred: StudioPrediction,
        fp_before: (
            studio_catalog::AcousticFingerprint,
            bool,
            bool,
            i32,
            bool,
            PluginId,
            i32,
            i32,
            i32,
            i32,
        ),
    ) {
        let u = self.rig.unit;
        ui.separator();
        ui.small(format!(
            "Pipeline: Mic {} · Room {} · Plugin {}",
            if self.mic_enabled { "ON" } else { "BYPASS" },
            if self.room_enabled { "ON" } else { "BYPASS" },
            if self.plugin_slot.enabled && self.plugin_slot.plugin_id != PluginId::None {
                "ON"
            } else {
                "BYPASS"
            }
        ));
        ui.small(format!(
            "Distance: {:.2} {} (direct {:.2} ms @ {:.1} m/s) · Direct pickup: {:+.2}",
            u.from_metres(pred.distance_m),
            u.suffix(),
            pred.direct_arrival_ms,
            pred.speed_of_sound_mps,
            pred.direct_pickup_gain
        ));
        if let (Some(ms), Some(db)) = (pred.first_reflection_ms, pred.reflected_path_db) {
            ui.small(format!(
                "Modeled reflections ({}): first {:.1} ms · {:+.1} dB vs direct · tail {:.0} ms",
                pred.reflection_count, ms, db, pred.last_path_ms
            ));
            let bands: Vec<String> = BANDS_HZ
                .iter()
                .zip(pred.six_band_drr_db)
                .map(|(&hz, drr)| {
                    let lbl = if hz >= 1000.0 {
                        format!("{}k", (hz / 1000.0) as u32)
                    } else {
                        format!("{}", hz as u32)
                    };
                    match drr {
                        Some(v) => format!("{lbl}:{v:+.0}dB"),
                        None => format!("{lbl}:∞"),
                    }
                })
                .collect();
            ui.small(format!("6-band DRR proxy: {}", bands.join(" · ")));
        } else if !self.room_enabled {
            ui.small("Stage 4 Room bypassed (dry direct signal).");
        } else {
            ui.small("No modeled room reflections added (Direct).");
        }

        let dry_approved = self.dry_review == DryReviewStatus::ApprovedByHuman;
        let ready = dry_approved
            && self.job.is_none()
            && self.track.as_ref().is_some_and(|t| t.dry.is_some());
        if !dry_approved {
            ui.colored_label(
                StudioTheme::WARNING,
                format!(
                    "Post-dry locked: dry listening check {}/5",
                    self.dry_checklist.passed_count()
                ),
            );
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ready, egui::Button::new("Rerender Post-Dry"))
                .clicked()
            {
                self.capture(ctx);
            }
            ui.add_enabled(
                dry_approved,
                egui::Checkbox::new(&mut self.live_preview, "Live Preview"),
            );
        });
        let fp_after = self.post_dry_fingerprint();
        if self.live_preview && ready && fp_after != fp_before {
            self.capture(ctx);
        }
    }

    fn draw_lower_dock(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.colored_label(
                self.app_state.color(),
                format!("[{}]", self.app_state.label()),
            );
            ui.label(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("×")
                    .on_hover_text("Hide Lower Dock")
                    .clicked()
                {
                    self.lower_dock = LowerDock::Hidden;
                }
                if ui
                    .selectable_label(self.lower_dock == LowerDock::Floating, "Float")
                    .on_hover_text("Undock Lower Dock into floating window")
                    .clicked()
                {
                    self.lower_dock = LowerDock::Floating;
                }
                if ui
                    .selectable_label(self.lower_dock == LowerDock::Bottom, "Dock")
                    .on_hover_text("Dock Lower Dock at bottom")
                    .clicked()
                {
                    self.lower_dock = LowerDock::Bottom;
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            let review_col = match self.dry_review {
                DryReviewStatus::NotReviewed => Color32::from_rgb(235, 185, 85),
                DryReviewStatus::ApprovedByHuman => Color32::from_rgb(100, 210, 150),
            };
            ui.colored_label(
                review_col,
                format!(
                    "Dry Listening Gate ({}/5): {}",
                    self.dry_checklist.passed_count(),
                    self.dry_review.label()
                ),
            );
            let has_rebuilt = self.track.as_ref().is_some_and(|t| t.rebuilt.is_some());
            let mut changed = false;
            changed |= ui
                .add_enabled(
                    has_rebuilt,
                    egui::Checkbox::new(&mut self.dry_checklist.rhythm, "Rhythm"),
                )
                .changed();
            changed |= ui
                .add_enabled(
                    has_rebuilt,
                    egui::Checkbox::new(&mut self.dry_checklist.phrasing, "Phrasing"),
                )
                .changed();
            changed |= ui
                .add_enabled(
                    has_rebuilt,
                    egui::Checkbox::new(&mut self.dry_checklist.dynamics, "Dynamics"),
                )
                .changed();
            changed |= ui
                .add_enabled(
                    has_rebuilt,
                    egui::Checkbox::new(&mut self.dry_checklist.sustain, "Sustain"),
                )
                .changed();
            changed |= ui
                .add_enabled(
                    has_rebuilt,
                    egui::Checkbox::new(&mut self.dry_checklist.artifact_free, "No artifacts"),
                )
                .changed();
            if changed {
                self.sync_dry_review_from_checklist();
            }
            ui.separator();
            ui.checkbox(&mut self.show_layers_panel, "Editable analysis layers");
            ui.small("Visuals are evidence only; human listening gates all post-processing.");
        });

        if self.show_layers_panel {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        self.track.is_some() && self.job.is_none(),
                        egui::Button::new("Build editable layers"),
                    )
                    .clicked()
                {
                    self.build_layers(ctx);
                }
                if ui
                    .add_enabled(
                        self.track.is_some() && self.job.is_none(),
                        egui::Button::new("Load editable analysis…"),
                    )
                    .clicked()
                {
                    if let Some(path) = platform::dialogs::pick_analysis_file() {
                        self.load_layers(&path, ctx);
                    }
                }
                if self.track.as_ref().is_some_and(|t| t.layers.is_some()) {
                    let mut changed = false;
                    let mut save_pkt = false;
                    if let Some(session) = self.track.as_mut().and_then(|t| t.layers.as_mut()) {
                        if ui.button("Reset layers").clicked() {
                            session.controls = workspace::Controls::default();
                            session.view = None;
                            changed = true;
                        }
                        if ui.button("View full mix").clicked() {
                            session.view = None;
                            changed = true;
                        }
                        save_pkt = ui.button("Save editable analysis…").clicked();
                    }
                    if changed {
                        self.remix_layers(ctx);
                    }
                    if save_pkt {
                        if let Some(path) = platform::dialogs::save_analysis_file("analysis.json") {
                            if let Some(session) =
                                self.track.as_ref().and_then(|t| t.layers.as_ref())
                            {
                                self.status = match serde_json::to_vec_pretty(&session.packet)
                                    .map_err(|e| e.to_string())
                                    .and_then(|bytes| {
                                        spectracraft_mono::qa::write_new(&path, &bytes)
                                            .map_err(|e| e.to_string())
                                    }) {
                                    Ok(()) => {
                                        "Saved editable analysis packet without source PCM.".into()
                                    }
                                    Err(e) => e,
                                };
                            }
                        }
                    }
                }
            });

            let mut changed = false;
            if let Some(session) = self.track.as_mut().and_then(|t| t.layers.as_mut()) {
                egui::Grid::new("layers_grid")
                    .spacing([8., 3.])
                    .show(ui, |ui| {
                        for i in 0..4 {
                            ui.label(workspace::NAMES[i]);
                            changed |= ui
                                .checkbox(&mut session.controls.muted[i], "Mute")
                                .changed();
                            if ui
                                .selectable_label(session.controls.solo == Some(i), "Solo")
                                .clicked()
                            {
                                session.controls.solo = if session.controls.solo == Some(i) {
                                    None
                                } else {
                                    Some(i)
                                };
                                changed = true;
                            }
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut session.controls.gain_db[i], -60. ..=6.)
                                        .suffix(" dB"),
                                )
                                .changed();
                            if ui
                                .selectable_label(session.view == Some(i), "View")
                                .clicked()
                            {
                                session.view = if session.view == Some(i) {
                                    None
                                } else {
                                    Some(i)
                                };
                                changed = true;
                            }
                            ui.small(format!("{} spans", session.rendered.spans[i].len()));
                            ui.end_row();
                        }
                    });
            }
            if changed {
                self.remix_layers(ctx);
            }
        }
    }

    fn sync_region_draft_from_selection(&mut self) {
        if let Some([a, b]) = self.selection {
            self.region_edit_draft.start_s = a.min(b);
            self.region_edit_draft.end_s = a.max(b);
        }
        match self.marquee_hz {
            Some([lo, hi]) => {
                self.region_edit_draft.kind = "marquee".into();
                self.region_edit_draft.min_hz = Some(lo.min(hi));
                self.region_edit_draft.max_hz = Some(lo.max(hi));
            }
            None => {
                self.region_edit_draft.kind = "range".into();
                self.region_edit_draft.min_hz = None;
                self.region_edit_draft.max_hz = None;
            }
        }
    }

    /// Direct In-point mark at current playhead (Audition/Logic-style I/O range marking).
    fn set_selection_in_at_cursor(&mut self) {
        let dur = self.track.as_ref().map_or(0.0, Track::duration);
        if dur <= 0.0 {
            return;
        }
        let in_t = self.cursor.clamp(0.0, dur);
        let out_t = match self.selection {
            Some([_, b]) if b > in_t + 0.02 => b.min(dur),
            _ => (in_t + 1.0).min(dur),
        };
        if out_t > in_t + 0.01 {
            self.selection = Some([in_t, out_t]);
            self.sync_region_draft_from_selection();
        }
    }

    /// Direct Out-point mark at current playhead (Audition/Logic-style I/O range marking).
    fn set_selection_out_at_cursor(&mut self) {
        let dur = self.track.as_ref().map_or(0.0, Track::duration);
        if dur <= 0.0 {
            return;
        }
        let out_t = self.cursor.clamp(0.0, dur);
        let in_t = match self.selection {
            Some([a, _]) if a + 0.02 < out_t => a.max(0.0),
            _ => (out_t - 1.0).max(0.0),
        };
        if out_t > in_t + 0.01 {
            self.selection = Some([in_t, out_t]);
            self.sync_region_draft_from_selection();
        }
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.marquee_hz = None;
        self.drag_anchor = None;
        self.drag_hz_anchor = None;
        self.drag_initial_sel = None;
        self.drag_initial_hz = None;
        self.sync_region_draft_from_selection();
    }

    fn apply_timeline_interaction(&mut self, action: TimelineInteraction) {
        let dur = self.track.as_ref().map_or(0.0, Track::duration);
        let max_hz = f64::from(self.display_max_hz).max(500.0);
        match action {
            TimelineInteraction::Seek(t) => self.seek_to(t),
            TimelineInteraction::DragStart { t, hz, mode } => {
                self.drag_mode = mode;
                self.drag_anchor = Some(t);
                self.drag_hz_anchor = hz;
                self.drag_initial_sel = self.selection;
                self.drag_initial_hz = self.marquee_hz;
                if mode == SelectionDragMode::Create {
                    self.selection = Some([t, t]);
                    if self.selection_tool == SelectionTool::Marquee {
                        if let Some(h) = hz {
                            self.marquee_hz = Some([h, h]);
                        }
                    } else {
                        self.marquee_hz = None;
                    }
                    self.sync_region_draft_from_selection();
                }
            }
            TimelineInteraction::DragMove { t, hz } => {
                let t = t.clamp(0.0, dur);
                match self.drag_mode {
                    SelectionDragMode::Create => {
                        if let Some(anchor) = self.drag_anchor {
                            self.selection = Some([anchor.min(t), anchor.max(t)]);
                        }
                        if self.selection_tool == SelectionTool::Marquee {
                            if let (Some(ha), Some(hb)) = (self.drag_hz_anchor, hz) {
                                let lo = ha.min(hb).clamp(0.0, max_hz);
                                let hi = ha.max(hb).clamp(lo + 1.0, max_hz.max(lo + 1.0));
                                self.marquee_hz = Some([lo, hi]);
                            }
                        }
                    }
                    SelectionDragMode::Move => {
                        if let (Some(anchor), Some([a0, b0])) =
                            (self.drag_anchor, self.drag_initial_sel)
                        {
                            let width = (b0 - a0).abs();
                            let dt = t - anchor;
                            let new_a = (a0.min(b0) + dt).clamp(0.0, (dur - width).max(0.0));
                            self.selection = Some([new_a, (new_a + width).min(dur)]);
                        }
                        if let (Some(ha), Some(hb), Some([f0, f1])) =
                            (self.drag_hz_anchor, hz, self.drag_initial_hz)
                        {
                            let span_hz = (f1 - f0).abs();
                            let df = hb - ha;
                            let new_lo = (f0.min(f1) + df).clamp(0.0, (max_hz - span_hz).max(0.0));
                            self.marquee_hz = Some([new_lo, (new_lo + span_hz).min(max_hz)]);
                        }
                    }
                    SelectionDragMode::ResizeLeft => {
                        if let Some([_, b0]) = self.drag_initial_sel {
                            self.selection = Some([t.min(b0), t.max(b0)]);
                        }
                    }
                    SelectionDragMode::ResizeRight => {
                        if let Some([a0, _]) = self.drag_initial_sel {
                            self.selection = Some([a0.min(t), a0.max(t)]);
                        }
                    }
                    SelectionDragMode::ResizeTop => {
                        if let (Some(h), Some([f0, _])) = (hz, self.drag_initial_hz) {
                            let hc = h.clamp(0.0, max_hz);
                            self.marquee_hz = Some([f0.min(hc), f0.max(hc).max(f0.min(hc) + 1.0)]);
                        }
                    }
                    SelectionDragMode::ResizeBottom => {
                        if let (Some(h), Some([_, f1])) = (hz, self.drag_initial_hz) {
                            let hc = h.clamp(0.0, max_hz);
                            self.marquee_hz = Some([hc.min(f1), hc.max(f1).max(hc.min(f1) + 1.0)]);
                        }
                    }
                    SelectionDragMode::ResizeTopLeft => {
                        if let Some([_, b0]) = self.drag_initial_sel {
                            self.selection = Some([t.min(b0), t.max(b0)]);
                        }
                        if let (Some(h), Some([f0, _])) = (hz, self.drag_initial_hz) {
                            let hc = h.clamp(0.0, max_hz);
                            self.marquee_hz = Some([f0.min(hc), f0.max(hc).max(f0.min(hc) + 1.0)]);
                        }
                    }
                    SelectionDragMode::ResizeTopRight => {
                        if let Some([a0, _]) = self.drag_initial_sel {
                            self.selection = Some([a0.min(t), a0.max(t)]);
                        }
                        if let (Some(h), Some([f0, _])) = (hz, self.drag_initial_hz) {
                            let hc = h.clamp(0.0, max_hz);
                            self.marquee_hz = Some([f0.min(hc), f0.max(hc).max(f0.min(hc) + 1.0)]);
                        }
                    }
                    SelectionDragMode::ResizeBottomLeft => {
                        if let Some([_, b0]) = self.drag_initial_sel {
                            self.selection = Some([t.min(b0), t.max(b0)]);
                        }
                        if let (Some(h), Some([_, f1])) = (hz, self.drag_initial_hz) {
                            let hc = h.clamp(0.0, max_hz);
                            self.marquee_hz = Some([hc.min(f1), hc.max(f1).max(hc.min(f1) + 1.0)]);
                        }
                    }
                    SelectionDragMode::ResizeBottomRight => {
                        if let Some([a0, _]) = self.drag_initial_sel {
                            self.selection = Some([a0.min(t), a0.max(t)]);
                        }
                        if let (Some(h), Some([_, f1])) = (hz, self.drag_initial_hz) {
                            let hc = h.clamp(0.0, max_hz);
                            self.marquee_hz = Some([hc.min(f1), hc.max(f1).max(hc.min(f1) + 1.0)]);
                        }
                    }
                }
                self.sync_region_draft_from_selection();
            }
            TimelineInteraction::DragEnd => {
                self.drag_anchor = None;
                self.drag_hz_anchor = None;
                self.drag_initial_sel = None;
                self.drag_initial_hz = None;
                self.drag_mode = SelectionDragMode::Create;
                if let Some([a, b]) = self.selection {
                    if (b - a) < 0.02 {
                        self.selection = None;
                        self.marquee_hz = None;
                    }
                }
                self.sync_region_draft_from_selection();
            }
            TimelineInteraction::WheelZoom(factor, anchor_frac) => {
                self.zoom_timeline(factor, anchor_frac);
            }
        }
    }

    fn draw(&mut self, ctx: &egui::Context) {
        if !self.theme_applied {
            StudioTheme::apply(ctx);
            self.theme_applied = true;
        }
        self.poll(ctx);

        // File drag-and-drop support
        let dropped: Option<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
        if let Some(path) = dropped {
            if self.job.is_none() {
                self.load(path, ctx);
            }
        }

        // Automated --smoke-test runner preserved from baseline
        if self.job.is_none() && self.smoke.is_some() {
            if let Some(track) = self.track.as_ref() {
                if track.rebuilt.is_some() {
                    if let Some(path) = self.smoke.take() {
                        if !self.export(&path) {
                            eprintln!("{}", self.status);
                            std::process::exit(1);
                        }
                    }
                    eprintln!("{}", self.status);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    self.regenerate(ctx);
                }
            } else {
                self.smoke = None;
                eprintln!("{}", self.status);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // Playback engine strictly owns self.cursor; redrawing a formatted time readout never seeks!
        if let Some(p) = self.play.as_ref() {
            if p.is_finished() {
                self.stop();
                self.cursor = 0.0;
            } else {
                self.cursor = p.position_secs();
                ctx.request_repaint_after(Duration::from_millis(30));
            }
        }

        // Keyboard shortcuts when focus permits (Space = Play/Pause, Ctrl+Z = Undo, Ctrl+Shift+Z = Redo)
        if !ctx.wants_keyboard_input() {
            let (undo_k, redo_k, space_k) = ctx.input(|i| {
                let cmd = i.modifiers.command || i.modifiers.ctrl;
                let undo = cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z);
                let redo = (cmd && i.modifiers.shift && i.key_pressed(egui::Key::Z))
                    || (cmd && i.key_pressed(egui::Key::Y));
                let space = i.key_pressed(egui::Key::Space);
                (undo, redo, space)
            });
            if undo_k {
                self.undo(ctx);
            } else if redo_k {
                self.redo(ctx);
            } else if space_k && self.track.is_some() {
                if self.play.is_some() {
                    self.stop();
                } else {
                    self.start();
                }
            }
        }

        // 1. TOP MENU BAR + COMPACT TOOLBAR + TRANSPORT
        egui::TopBottomPanel::top("top_chrome").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add_enabled(self.job.is_none(), egui::Button::new("Open WAV…"))
                        .clicked()
                    {
                        ui.close_menu();
                        if let Some(path) = platform::dialogs::pick_wav_file() {
                            self.load(path, ctx);
                        }
                    }
                    if ui.button("Open by Path…").clicked() {
                        self.show_path_bar = !self.show_path_bar;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Save Session…").clicked() {
                        ui.close_menu();
                        if let Some(path) =
                            platform::dialogs::save_session_file("session.spectracraft.json")
                        {
                            if let Err(e) = self.execute(StudioCommand::SaveSession(path), ctx) {
                                self.status = e;
                            }
                        }
                    }
                    if ui
                        .add_enabled(self.job.is_none(), egui::Button::new("Open Session…"))
                        .clicked()
                    {
                        ui.close_menu();
                        if let Some(path) = platform::dialogs::pick_session_file() {
                            if let Err(e) = self.execute(StudioCommand::LoadSession(path), ctx) {
                                self.status = e;
                            }
                        }
                    }
                    ui.separator();
                    let can_export = self.track.as_ref().is_some_and(|t| t.rebuilt.is_some())
                        && self.job.is_none();
                    if ui
                        .add_enabled(can_export, egui::Button::new("Export Regenerated WAV…"))
                        .clicked()
                    {
                        ui.close_menu();
                        if let Some(path) = platform::dialogs::save_wav_file("Regenerated.wav") {
                            self.export(&path);
                        }
                    }
                    let can_variants = self.track.as_ref().is_some_and(|t| t.guitar_cues.is_some())
                        && self.dry_review == DryReviewStatus::ApprovedByHuman
                        && self.job.is_none();
                    if ui
                        .add_enabled(
                            can_variants,
                            egui::Button::new(
                                "Export Guitar Variants… (requires human dry approval)",
                            ),
                        )
                        .clicked()
                    {
                        ui.close_menu();
                        if let Some(dir) = platform::dialogs::pick_export_folder() {
                            self.export_selected_variants(&dir.join("spectral-variants"));
                        }
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(
                            self.history.can_undo(),
                            egui::Button::new(platform::undo_shortcut_label()),
                        )
                        .clicked()
                    {
                        self.undo(ctx);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.history.can_redo(),
                            egui::Button::new(platform::redo_shortcut_label()),
                        )
                        .clicked()
                    {
                        self.redo(ctx);
                        ui.close_menu();
                    }
                });
                ui.menu_button("Audio", |ui| {
                    let ready = self.track.is_some() && self.job.is_none();
                    if ui
                        .add_enabled(ready, egui::Button::new("Regenerate Excerpt"))
                        .clicked()
                    {
                        self.regenerate(ctx);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(ready, egui::Button::new("Build Editable Layers"))
                        .clicked()
                    {
                        self.build_layers(ctx);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(ready, egui::Button::new("Analyze Guitar Dry (Cue-Only)"))
                        .clicked()
                    {
                        self.analyze_guitar_dry(ctx);
                        ui.close_menu();
                    }
                    let dry_approved = self.dry_review == DryReviewStatus::ApprovedByHuman;
                    if ui
                        .add_enabled(
                            dry_approved
                                && ready
                                && self.track.as_ref().is_some_and(|t| t.dry.is_some()),
                            egui::Button::new("Rerender Post-Dry Pipeline"),
                        )
                        .clicked()
                    {
                        self.capture(ctx);
                        ui.close_menu();
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.selectable_value(
                        &mut self.canvas_mode,
                        CanvasMode::Spectrograms,
                        "Spectrograms",
                    );
                    ui.selectable_value(&mut self.canvas_mode, CanvasMode::Waveforms, "Waveforms");
                    ui.selectable_value(
                        &mut self.canvas_mode,
                        CanvasMode::Both,
                        "Spectrograms + Waveforms",
                    );
                    ui.separator();
                    if ui.button("Fit Recording").clicked() {
                        self.fit_recording();
                        ui.close_menu();
                    }
                    if ui.button("Zoom to Selection").clicked() {
                        self.zoom_to_selection();
                        ui.close_menu();
                    }
                    if ui.button("Zoom to Result").clicked() {
                        self.zoom_to_result();
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Studio Inspector").strong().size(11.0));
                    ui.selectable_value(&mut self.inspector_dock, InspectorDock::Left, "Dock Left");
                    ui.selectable_value(
                        &mut self.inspector_dock,
                        InspectorDock::Right,
                        "Dock Right",
                    );
                    ui.selectable_value(
                        &mut self.inspector_dock,
                        InspectorDock::Floating,
                        "Undock (Floating)",
                    );
                    ui.selectable_value(&mut self.inspector_dock, InspectorDock::Hidden, "Hidden");
                    ui.separator();
                    ui.label(egui::RichText::new("Lower Dock").strong().size(11.0));
                    ui.selectable_value(&mut self.lower_dock, LowerDock::Bottom, "Dock Bottom");
                    ui.selectable_value(
                        &mut self.lower_dock,
                        LowerDock::Floating,
                        "Undock (Floating)",
                    );
                    ui.selectable_value(&mut self.lower_dock, LowerDock::Hidden, "Hidden");
                    ui.separator();
                    ui.checkbox(&mut self.show_spatial_viewport, "Spatial Room Plans");
                    ui.checkbox(&mut self.rig.show_patterns, "Patterns Overlay");
                });
                ui.menu_button("Room", |ui| {
                    for preset in RoomPreset::ALL {
                        if ui
                            .selectable_label(self.rig.room_preset == preset, preset.label())
                            .clicked()
                        {
                            let _ = self.execute(StudioCommand::ApplyRoomPreset(preset), ctx);
                            ui.close_menu();
                        }
                    }
                });

                ui.separator();
                if ui
                    .add_enabled(self.job.is_none(), egui::Button::new("Open WAV…"))
                    .clicked()
                {
                    if let Some(path) = platform::dialogs::pick_wav_file() {
                        self.load(path, ctx);
                    }
                }
                if ui.selectable_label(self.show_path_bar, "Path…").clicked() {
                    self.show_path_bar = !self.show_path_bar;
                }
                if ui
                    .add_enabled(
                        self.track.is_some() && self.job.is_none(),
                        egui::Button::new("Regenerate"),
                    )
                    .clicked()
                {
                    self.regenerate(ctx);
                }
                if ui
                    .add_enabled(
                        self.track.as_ref().is_some_and(|t| t.rebuilt.is_some())
                            && self.job.is_none(),
                        egui::Button::new("Export…"),
                    )
                    .clicked()
                {
                    if let Some(path) = platform::dialogs::save_wav_file("Regenerated.wav") {
                        self.export(&path);
                    }
                }
                ui.add_enabled(
                    self.job.is_none(),
                    egui::Checkbox::new(&mut self.include_texture, "Texture"),
                );
                if self.job.is_some() {
                    ui.spinner();
                }

                // Right-aligned compact track metadata + quick panel restore buttons
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.lower_dock == LowerDock::Hidden
                        && ui.small_button("Show Dock").clicked()
                    {
                        self.lower_dock = LowerDock::Bottom;
                    }
                    if self.inspector_dock == InspectorDock::Hidden
                        && ui.small_button("Show Inspector").clicked()
                    {
                        self.inspector_dock = InspectorDock::Left;
                    }
                    if let Some(t) = self.track.as_ref() {
                        let ch = if t.recording.channels == 1 {
                            "Mono"
                        } else {
                            "Stereo→L"
                        };
                        let sel_str = match self.selection {
                            Some([a, b]) => format!(" · Sel {:.2}–{:.2}s", a, b),
                            None => String::new(),
                        };
                        ui.colored_label(
                            StudioTheme::TEXT_MUTED,
                            format!(
                                "{} · {} Hz · {:.2}s · {}{}",
                                t.name,
                                t.rate,
                                t.duration(),
                                ch,
                                sel_str
                            ),
                        );
                    } else {
                        ui.colored_label(StudioTheme::TEXT_MUTED, "No WAV loaded");
                    }
                });
            });

            if self.show_path_bar {
                ui.horizontal(|ui| {
                    ui.label("WAV path:");
                    let r = ui.text_edit_singleline(&mut self.path_input);
                    let submit = ui
                        .add_enabled(self.job.is_none(), egui::Button::new("Open"))
                        .clicked()
                        || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if submit && !self.path_input.trim().is_empty() {
                        let p = platform::normalize_user_path(&self.path_input);
                        self.load(p, ctx);
                    }
                });
            }

            ui.separator();

            // Row 2: Unified Transport + 5-Stage Pipeline Strip (Audition/Logic density so canvas is primary)
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        self.track.is_some(),
                        egui::Button::new(if self.play.is_some() {
                            "❚❚ Pause"
                        } else {
                            "▶ Play"
                        }),
                    )
                    .clicked()
                {
                    if self.play.is_some() {
                        self.stop();
                    } else {
                        self.start();
                    }
                }
                for which in [Take::Original, Take::Regenerated, Take::Difference] {
                    let enabled = self
                        .track
                        .as_ref()
                        .is_some_and(|t| which == Take::Original || t.rebuilt.is_some());
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::new(which.name()).selected(self.take == which),
                        )
                        .clicked()
                    {
                        self.switch(which);
                    }
                }
                // Playback-owned numeric position readout (read-only: redraw NEVER triggers seek!)
                egui::Frame::new()
                    .fill(StudioTheme::BG_CANVAS)
                    .stroke(Stroke::new(1.0, StudioTheme::BORDER_SUBTLE))
                    .corner_radius(3.0)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        ui.monospace(format!("{:07.3} s", self.cursor));
                    });
                let jr = ui.add(
                    egui::TextEdit::singleline(&mut self.jump_input)
                        .desired_width(50.)
                        .hint_text("sec"),
                );
                if (ui.small_button("Go").clicked()
                    || (jr.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                    && self.track.is_some()
                {
                    if let Ok(sec) = self.jump_input.trim().parse::<f64>() {
                        self.seek_to(sec);
                    }
                }
                if self.track.as_ref().is_some_and(|t| t.duration() >= 26.0)
                    && ui.small_button("25.5s").clicked()
                {
                    self.seek_to(25.5);
                }
                ui.checkbox(&mut self.audition_rms_match, "Level-match");

                ui.separator();

                // 5-Stage Pipeline Strip (authoritative status + explicit accessible bypass toggles)
                let has_dry = self.track.as_ref().is_some_and(|t| t.dry.is_some());
                let s1_txt = if has_dry { "1. Dry ●" } else { "1. Dry" };
                if ui
                    .selectable_label(self.inspector_tab == InspectorTab::Source, s1_txt)
                    .clicked()
                {
                    self.inspector_tab = InspectorTab::Source;
                    if self.inspector_dock == InspectorDock::Hidden {
                        self.inspector_dock = InspectorDock::Left;
                    }
                }
                ui.small("→");
                let s2_txt = if self.last_region_edit.is_some() {
                    "2. Edit ●"
                } else {
                    "2. Edit"
                };
                if ui
                    .selectable_label(self.inspector_tab == InspectorTab::DryEdit, s2_txt)
                    .clicked()
                {
                    self.inspector_tab = InspectorTab::DryEdit;
                    if self.inspector_dock == InspectorDock::Hidden {
                        self.inspector_dock = InspectorDock::Left;
                    }
                }
                ui.small("→");
                let mic_lbl = if self.mic_enabled {
                    egui::RichText::new("3. Mic: ON").color(StudioTheme::STAGE_ACTIVE)
                } else {
                    egui::RichText::new("3. Mic: BYP").color(StudioTheme::STAGE_BYPASS)
                };
                if ui.button(mic_lbl).clicked() {
                    let _ = self.execute(StudioCommand::SetMicEnabled(!self.mic_enabled), ctx);
                }
                ui.small("→");
                let room_lbl = if self.room_enabled {
                    egui::RichText::new("4. Room: ON").color(StudioTheme::STAGE_ACTIVE)
                } else {
                    egui::RichText::new("4. Room: BYP").color(StudioTheme::STAGE_BYPASS)
                };
                if ui.button(room_lbl).clicked() {
                    let _ = self.execute(StudioCommand::SetRoomEnabled(!self.room_enabled), ctx);
                }
                ui.small("→");
                let plugin_active =
                    self.plugin_slot.enabled && self.plugin_slot.plugin_id != PluginId::None;
                let plug_lbl = if plugin_active {
                    egui::RichText::new("5. Plug: ON").color(StudioTheme::STAGE_ACTIVE)
                } else {
                    egui::RichText::new("5. Plug: BYP").color(StudioTheme::STAGE_BYPASS)
                };
                if ui.button(plug_lbl).clicked() {
                    let mut cfg = self.plugin_slot.clone();
                    cfg.enabled = !cfg.enabled;
                    if cfg.enabled && cfg.plugin_id == PluginId::None {
                        cfg.plugin_id = PluginId::GentleEq;
                    }
                    let _ = self.execute(StudioCommand::SetPluginSlot(cfg), ctx);
                }
            });
        });

        // 2. DOCKABLE / UNDOCKABLE STUDIO INSPECTOR (Audio canvas always retains >= 56% width)
        let max_insp_w = (ctx.screen_rect().width() * 0.44).clamp(320.0, 520.0);
        let min_insp_w = 300.0_f32.min(max_insp_w);
        match self.inspector_dock {
            InspectorDock::Left => {
                egui::SidePanel::left("studio_inspector_left")
                    .resizable(true)
                    .default_width(372.0_f32.min(max_insp_w))
                    .min_width(min_insp_w)
                    .max_width(max_insp_w)
                    .show(ctx, |ui| {
                        self.draw_inspector(ui, ctx);
                    });
            }
            InspectorDock::Right => {
                egui::SidePanel::right("studio_inspector_right")
                    .resizable(true)
                    .default_width(372.0_f32.min(max_insp_w))
                    .min_width(min_insp_w)
                    .max_width(max_insp_w)
                    .show(ctx, |ui| {
                        self.draw_inspector(ui, ctx);
                    });
            }
            InspectorDock::Floating => {
                let mut open = true;
                egui::Window::new("Studio Inspector")
                    .id(egui::Id::new("studio_inspector_floating"))
                    .open(&mut open)
                    .resizable(true)
                    .default_width(390.0)
                    .default_height(560.0)
                    .min_width(310.0)
                    .min_height(320.0)
                    .show(ctx, |ui| {
                        self.draw_inspector(ui, ctx);
                    });
                if !open {
                    self.inspector_dock = InspectorDock::Hidden;
                }
            }
            InspectorDock::Hidden => {}
        }

        // 3. DOCKABLE / UNDOCKABLE LOWER CONTROLS DOCK (compact by default so canvas is primary)
        match self.lower_dock {
            LowerDock::Bottom => {
                let max_dock_h = (ctx.screen_rect().height() * 0.38).clamp(96.0, 320.0);
                egui::TopBottomPanel::bottom("lower_dock_bottom")
                    .resizable(true)
                    .default_height(if self.show_layers_panel { 164.0 } else { 52.0 })
                    .min_height(46.0)
                    .max_height(max_dock_h)
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            self.draw_lower_dock(ui, ctx);
                        });
                    });
            }
            LowerDock::Floating => {
                let mut open = true;
                egui::Window::new("Dry Review & Layers")
                    .id(egui::Id::new("lower_dock_floating"))
                    .open(&mut open)
                    .resizable(true)
                    .default_width(680.0)
                    .default_height(if self.show_layers_panel { 220.0 } else { 96.0 })
                    .min_width(360.0)
                    .min_height(72.0)
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            self.draw_lower_dock(ui, ctx);
                        });
                    });
                if !open {
                    self.lower_dock = LowerDock::Hidden;
                }
            }
            LowerDock::Hidden => {}
        }

        // 4. CENTER PRIMARY AUDIO EDITING CANVAS
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.track.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.);
                        ui.heading("SpectraCraft");
                        ui.label("Performance-guided audio reconstruction — native Rust studio workspace");
                        ui.add_space(12.);
                        ui.label("No file loaded. Open a mono or stereo WAV, drop a WAV onto this window, or use File -> Open by Path…");
                        ui.small("Original audio is never overwritten. No placeholder tracks, scores, or downloads are shown until a real WAV is loaded.");
                    });
                });
                return;
            }

            self.refresh_viewport_visuals(ctx);

            // Overview bar (full recording always visible while zoomed into an excerpt)
            let (dur, ov_peaks) = {
                let Some(t) = self.track.as_ref() else {
                    return;
                };
                (t.duration(), t.overview_peaks.clone())
            };
            if let Some(new_view) =
                draw_overview_bar(ui, &ov_peaks, dur, self.view_range, self.selection, self.cursor)
            {
                self.view_range = new_view;
                self.plot_cache_key = None;
                self.refresh_viewport_visuals(ctx);
            }

            // Canvas display & direct selection toolbar (independent of export!)
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.canvas_mode, CanvasMode::Spectrograms, "Spectrograms");
                ui.selectable_value(&mut self.canvas_mode, CanvasMode::Waveforms, "Waveforms");
                ui.selectable_value(&mut self.canvas_mode, CanvasMode::Both, "Both");
                ui.separator();
                if ui
                    .selectable_label(self.selection_tool == SelectionTool::Range, "Range")
                    .on_hover_text("Drag to select time range; drag edges to resize or interior to move")
                    .clicked()
                {
                    self.selection_tool = SelectionTool::Range;
                    self.marquee_hz = None;
                    self.sync_region_draft_from_selection();
                }
                if ui
                    .selectable_label(
                        self.selection_tool == SelectionTool::Marquee,
                        "Marquee (Hz)",
                    )
                    .on_hover_text("Drag to select time+Hz region; drag corners/edges to resize or interior to move")
                    .clicked()
                {
                    self.selection_tool = SelectionTool::Marquee;
                    self.region_edit_draft.kind = "marquee".into();
                }
                if ui
                    .small_button("[ In")
                    .on_hover_text("Set selection start to playhead")
                    .clicked()
                {
                    self.set_selection_in_at_cursor();
                }
                if ui
                    .small_button("Out ]")
                    .on_hover_text("Set selection end to playhead")
                    .clicked()
                {
                    self.set_selection_out_at_cursor();
                }
                if ui
                    .add_enabled_ui(self.selection.is_some(), |ui| ui.small_button("Clear"))
                    .inner
                    .on_hover_text("Clear selection")
                    .clicked()
                {
                    self.clear_selection();
                }
                ui.separator();
                if ui.small_button("Zoom Sel").clicked() {
                    self.zoom_to_selection();
                }
                if ui.small_button("Zoom Res").clicked() {
                    self.zoom_to_result();
                }
                if ui.small_button("Fit").clicked() {
                    self.fit_recording();
                }
                if ui.small_button("+").clicked() {
                    self.zoom_timeline(0.65, 0.5);
                }
                if ui.small_button("−").clicked() {
                    self.zoom_timeline(1.5, 0.5);
                }
                ui.separator();
                egui::ComboBox::from_id_salt("display_max_hz")
                    .selected_text(format!("Max {:.0} kHz", self.display_max_hz / 1000.0))
                    .show_ui(ui, |ui| {
                        for hz in [8_000.0_f32, 12_000.0_f32, 16_000.0_f32, 20_000.0_f32, 24_000.0_f32, 32_000.0_f32] {
                            if ui
                                .selectable_value(
                                    &mut self.display_max_hz,
                                    hz,
                                    format!("{:.0} kHz", hz / 1000.0),
                                )
                                .changed()
                            {
                                self.plot_cache_key = None;
                            }
                        }
                    });
                egui::ComboBox::from_id_salt("preview_bits")
                    .selected_text(self.preview_bit_depth.label())
                    .show_ui(ui, |ui| {
                        for b in PreviewBitDepth::ALL {
                            ui.selectable_value(&mut self.preview_bit_depth, b, b.label());
                        }
                    });
                if self.canvas_mode != CanvasMode::Waveforms {
                    spectrum_legend(ui);
                }
            });

            if let Some(seek_t) = draw_time_ruler(ui, self.view_range, self.selection, self.cursor)
            {
                self.seek_to(seek_t);
            }

            // Stacked aligned Original + Regenerated (+ Difference when Difference take active)
            let mut interaction: Option<TimelineInteraction> = None;
            let show_diff = self.take == Take::Difference
                && self.track.as_ref().is_some_and(|t| t.difference_view.is_some());
            let plot_count = match self.canvas_mode {
                CanvasMode::Both => if show_diff { 5 } else { 4 },
                _ => if show_diff { 3 } else { 2 },
            };
            // Account for per-plot title row (~24 px each) and allow plots to scale cleanly on tall monitors
            let header_overhead = plot_count as f32 * 24.0 + 14.0;
            let plot_h =
                ((ui.available_height() - header_overhead) / plot_count as f32).clamp(84.0, 340.0);

            egui::ScrollArea::vertical().id_salt("canvas_scroll").show(ui, |ui| {
                let Some(t) = self.track.as_ref() else {
                    return;
                };
                let regen_label = if let Some(v) = t.layers.as_ref().and_then(|l| l.view) {
                    format!("Regenerated · Viewing {} (plot only)", workspace::NAMES[v])
                } else {
                    "Regenerated".to_owned()
                };

                if matches!(self.canvas_mode, CanvasMode::Spectrograms | CanvasMode::Both) {
                    interaction = interaction.or(spectrum_plot_panel(
                        ui,
                        ctx,
                        "Original (spectrogram)",
                        Some(&t.original_view),
                        self.original_texture.as_ref(),
                        self.view_range,
                        self.selection,
                        self.marquee_hz,
                        self.cursor,
                        plot_h,
                    ));
                    interaction = interaction.or(spectrum_plot_panel(
                        ui,
                        ctx,
                        &format!("{regen_label} (spectrogram)"),
                        t.regenerated_view.as_ref(),
                        self.regenerated_texture.as_ref(),
                        self.view_range,
                        self.selection,
                        self.marquee_hz,
                        self.cursor,
                        plot_h,
                    ));
                    if show_diff {
                        interaction = interaction.or(spectrum_plot_panel(
                            ui,
                            ctx,
                            "Difference = Regenerated − Original (spectrogram)",
                            t.difference_view.as_ref(),
                            self.difference_texture.as_ref(),
                            self.view_range,
                            self.selection,
                            self.marquee_hz,
                            self.cursor,
                            plot_h,
                        ));
                    }
                }
                if matches!(self.canvas_mode, CanvasMode::Waveforms | CanvasMode::Both) {
                    let orig_p = &t.original_view.peaks[..];
                    let reg_p = t.regenerated_view.as_ref().map(|v| &v.peaks[..]);
                    let scale = Some(orig_p)
                        .into_iter()
                        .chain(reg_p)
                        .flatten()
                        .fold(0.1_f32, |p, (lo, hi)| p.max(lo.abs()).max(hi.abs()));
                    interaction = interaction.or(wave_plot_panel(
                        ui,
                        ctx,
                        "Original (waveform)",
                        Some(orig_p),
                        scale,
                        self.view_range,
                        self.selection,
                        self.cursor,
                        Color32::from_rgb(151, 185, 248),
                        plot_h,
                    ));
                    interaction = interaction.or(wave_plot_panel(
                        ui,
                        ctx,
                        &format!("{regen_label} (waveform)"),
                        reg_p,
                        scale,
                        self.view_range,
                        self.selection,
                        self.cursor,
                        Color32::from_rgb(106, 226, 193),
                        plot_h,
                    ));
                    if show_diff {
                        let diff_p = t.difference_view.as_ref().map(|v| &v.peaks[..]);
                        interaction = interaction.or(wave_plot_panel(
                            ui,
                            ctx,
                            "Difference = Regenerated − Original (waveform)",
                            diff_p,
                            scale,
                            self.view_range,
                            self.selection,
                            self.cursor,
                            Color32::from_rgb(245, 165, 115),
                            plot_h,
                        ));
                    }
                }
            });

            if let Some(act) = interaction {
                self.apply_timeline_interaction(act);
            }
        });
    }
}

impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.draw(ctx);
    }
}

fn main() -> eframe::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    let automated = first.as_deref() == Some(std::ffi::OsStr::new("--smoke-test"));
    let (initial, export) = if automated {
        (args.next(), args.next())
    } else {
        (first, None)
    };
    if automated && (initial.is_none() || export.is_none()) {
        eprintln!("Usage: spectracraft-desktop --smoke-test INPUT.wav OUTPUT.wav");
        std::process::exit(2);
    }
    platform::run_native_window("SpectraCraft", [1320., 820.], [860., 580.], move |cc| {
        let mut app = Studio::new();
        app.smoke = export.map(PathBuf::from);
        if let Some(path) = initial {
            app.load(path.into(), &cc.egui_ctx);
        }
        Ok(Box::new(app))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_frame(ctx: &egui::Context, app: &mut Studio, w: f32, h: f32) {
        let frame = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h))),
                ..Default::default()
            },
            |ctx| app.draw(ctx),
        );
        assert!(!frame.shapes.is_empty());
    }

    fn settle(ctx: &egui::Context, app: &mut Studio) {
        for _ in 0..400 {
            run_frame(ctx, app, 1280., 800.);
            if app.job.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        panic!("desktop job timed out: {}", app.status);
    }

    #[test]
    fn desktop_starts_in_true_empty_state() {
        let ctx = egui::Context::default();
        let mut app = Studio::new();
        assert!(app.track.is_none());
        assert_eq!(app.app_state, AppState::Empty);
        assert_eq!(app.dry_review, DryReviewStatus::NotReviewed);
        assert_eq!(app.sources.len(), 138);
        assert!(app.mics.len() >= 60);
        assert_eq!(app.surfaces.len(), 32);
        run_frame(&ctx, &mut app, 1280., 800.);
        assert!(app.track.is_none());
        assert!(!app.export(&platform::temp_dir().join("should-not-exist.wav")));
        let snap = app.inspect_state_json();
        assert_eq!(snap["app_state"], "Empty");
        assert!(snap["loaded_file"].is_null());
    }

    #[test]
    fn audition_level_matches_without_clipping_and_difference_is_real() {
        let t = Track {
            name: String::new(),
            rate: 48000,
            overview_peaks: measure_peaks(&[1., -1.]),
            original: vec![1., -1.].into(),
            rebuilt_span: Some([0, 2]),
            rebuilt: Some(vec![0.5, -0.5]),
            dry: None,
            layer_plot_only: None,
            layers: None,
            guitar_cues: None,
            recording: Recording {
                channels: 1,
                left_minus_right_db: None,
                lr_similarity: None,
            },
            original_view: Visual::measure(&[1., -1.], 48000),
            regenerated_view: Some(Visual::measure(&[0.5, -0.5], 48000)),
            difference_view: None,
        };
        assert_eq!(t.audition(Take::Original), t.audition(Take::Regenerated));
        assert!(t
            .audition(Take::Original)
            .unwrap()
            .iter()
            .all(|x| x.abs() < 0.9));
        // Unmatched difference = rebuilt - original = [-0.5, +0.5]
        let diff_raw = t.aligned_difference().unwrap();
        assert!((diff_raw[0] + 0.5).abs() < 1e-6);
        assert!((diff_raw[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn spectrogram_does_not_normalize_away_six_db_difference_and_marks_above_nyquist() {
        let x: Vec<_> = (0..9600)
            .map(|i| 0.5 * (std::f32::consts::TAU * 1500. * i as f32 / 48000.).sin())
            .collect();
        let half: Vec<_> = x.iter().map(|v| v * 0.5).collect();
        let a = Spectrum::measure(&x, 48000);
        let b = Spectrum::measure(&half, 48000);
        assert_eq!(a.width, b.width);
        assert_eq!(a.max_hz, b.max_hz);
        let (i, peak) = a
            .dbfs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .unwrap();
        assert!((peak + 6.0206).abs() < 0.01);
        assert!((peak - b.dbfs[i] - 6.0206).abs() < 0.001);
        assert_ne!(a.image().pixels[i], b.image().pixels[i]);
        let silence = Spectrum::measure(&[0.; 100], 8000);
        assert!(silence
            .image()
            .pixels
            .iter()
            .all(|&c| c == spectrum_color(SPECTRUM_BOTTOM)));
        // Display above Nyquist (24 kHz for 48 kHz audio -> ask for 32 kHz) marks top rows NaN ("No data")
        let over = Spectrum::measure_range(&x, 48000, 32_000.0);
        assert!(
            over.dbfs[0].is_nan(),
            "top row above Nyquist must be NaN / no data"
        );
    }

    #[test]
    fn complete_long_file_workflow_navigates_selects_regenerates_captures_and_exports_preserving_original(
    ) {
        let base =
            std::env::temp_dir().join(format!("spectracraft-long-workflow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir(&base).unwrap();
        let source = base.join("long_35s.wav");
        let output = base.join("excerpt_out.wav");
        // 35-second 8 kHz mono WAV (> 30 s excerpt limit) with 440 Hz tone
        let rate = 8000u32;
        let x: Vec<f32> = (0..rate as usize * 35)
            .map(|i| (0.25 * (std::f64::consts::TAU * 440. * i as f64 / rate as f64).sin()) as f32)
            .collect();
        write_wav(rate, &x, &source).unwrap();
        let original_bytes = std::fs::read(&source).unwrap();

        let ctx = egui::Context::default();
        let mut app = Studio::new();
        app.load(source.clone(), &ctx);
        settle(&ctx, &mut app);
        assert_eq!(app.app_state, AppState::Ready);
        assert!((app.track.as_ref().unwrap().duration() - 35.0).abs() < 1e-4);

        // Select excerpt 24.0–27.0 s (3.0 s) and seek directly to 25.5 s
        app.selection = Some([24.0, 27.0]);
        app.seek_to(25.5);
        assert!((app.cursor - 25.5).abs() < 1e-6);

        // Zoom to selection & Fit Recording MUST NOT move playhead or alter audio
        app.zoom_to_selection();
        assert!((app.view_range[0] - 24.0).abs() < 1e-6 && (app.view_range[1] - 27.0).abs() < 1e-6);
        assert!((app.cursor - 25.5).abs() < 1e-6);
        app.fit_recording();
        assert!((app.cursor - 25.5).abs() < 1e-6);

        // Switching all 4 Inspector tabs must NOT reset selection, playhead, source, mic pose, or room
        let rig_before = app.rig.clone();
        for tab in [
            InspectorTab::Source,
            InspectorTab::Microphone,
            InspectorTab::Materials,
            InspectorTab::Room,
        ] {
            app.inspector_tab = tab;
            run_frame(&ctx, &mut app, 1280., 800.);
            assert_eq!(app.selection, Some([24.0, 27.0]));
            assert!((app.cursor - 25.5).abs() < 1e-6);
            assert_eq!(app.rig, rig_before);
        }

        // Regenerate selected 3 s excerpt
        app.regenerate(&ctx);
        settle(&ctx, &mut app);
        let dry = app.track.as_ref().unwrap().dry.clone().unwrap();
        assert_eq!(dry.len(), rate as usize * 3);

        // Switch takes at exact same 25.5 s position (Original -> Regenerated -> Difference)
        app.switch(Take::Regenerated);
        assert!((app.cursor - 25.5).abs() < 1e-6);
        app.switch(Take::Difference);
        assert!((app.cursor - 25.5).abs() < 1e-6);

        // Core Design Contract: unreviewed dry take MUST block post-processing capture!
        let snap_foam = app.capture_snapshot();
        app.history.push(snap_foam);
        app.rig.surface_faces = [21; 6];
        let rig_expected = app.rig.clone();
        assert_eq!(app.dry_review, DryReviewStatus::NotReviewed);
        app.capture(&ctx);
        assert!(
            app.job.is_none(),
            "unreviewed dry take must block capture post-processing"
        );
        assert!(app.applied_rig.is_none());

        // Pass all 5 human listening checks (rhythm, phrasing, dynamics, sustain, artifact_free)
        app.dry_checklist = DryListeningChecklist {
            rhythm: true,
            phrasing: true,
            dynamics: true,
            sustain: true,
            artifact_free: true,
        };
        app.dry_review = app.dry_checklist.sync_status();
        assert_eq!(app.dry_review, DryReviewStatus::ApprovedByHuman);

        // Now capture rerender from SAME dry take succeeds without moving playhead!
        app.capture(&ctx);
        settle(&ctx, &mut app);
        assert!(
            (app.cursor - 25.5).abs() < 1e-6,
            "capture rerender must preserve playhead"
        );
        assert!(Arc::ptr_eq(
            &dry,
            app.track.as_ref().unwrap().dry.as_ref().unwrap()
        ));
        assert_eq!(app.applied_rig, Some(rig_expected.clone()));

        // Undo & Redo restore rig cleanly without moving playhead or touching original!
        assert!(app.history.can_undo());
        assert!(app.undo(&ctx));
        assert_eq!(app.rig.surface_faces, [1; 6]);
        assert!((app.cursor - 25.5).abs() < 1e-6);
        assert!(app.history.can_redo());
        assert!(app.redo(&ctx));
        assert_eq!(app.rig.surface_faces, [21; 6]);
        assert!((app.cursor - 25.5).abs() < 1e-6);

        // Headless workspace introspection JSON reports accurate real state
        let inspected = app.inspect_state_json();
        assert_eq!(inspected["loaded_file"]["name"], "long_35s.wav");
        assert_eq!(inspected["transport"]["take"], "Difference");
        assert_eq!(inspected["dry_review"], "Approved by human listener");
        assert_eq!(inspected["dry_listening_checklist"]["passed_count"], 5);

        // 3D camera orbit/pan must NEVER move or rotate the mic
        let mic_before_cam = app.rig.active_mic().clone();
        app.spatial_mode = SpatialViewMode::Grid3D;
        app.cam_3d.yaw_deg += 45.0;
        app.cam_3d.pitch_deg -= 15.0;
        app.cam_3d.perspective = false;
        run_frame(&ctx, &mut app, 1280., 800.);
        assert_eq!(app.rig.active_mic(), &mic_before_cam);

        // Export regenerated excerpt without modifying original file!
        assert!(app.export(&output), "{}", app.status);
        assert_eq!(std::fs::read(&source).unwrap(), original_bytes);
        let w = hound::WavReader::open(&output).unwrap();
        assert_eq!(
            (w.spec().channels, w.spec().sample_rate, w.duration()),
            (1, rate, rate * 3)
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn desktop_guitar_variant_export_blocks_unreviewed_and_supports_individual_and_all() {
        let base = std::env::temp_dir().join(format!(
            "spectracraft-desktop-variants-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir(&base).unwrap();
        let rate = 48000;
        let x: Vec<f32> = (0..rate)
            .map(|i| {
                let t = i as f64 / rate as f64;
                if !(0.15..0.75).contains(&t) {
                    return 0.0;
                }
                let p = std::f64::consts::TAU * 220.0 * (t - 0.15);
                ((0.25 * p.sin() + 0.11 * (2. * p).sin() + 0.05 * (3. * p).sin())
                    * (-2.0 * (t - 0.15)).exp()) as f32
            })
            .collect();
        let cues = reperform::analyze_guitar(&x, rate).unwrap();
        let dry = cues.synthesize_dry().unwrap();
        let mut app = Studio::new();
        app.track = Some(Track {
            name: "guitar.wav".into(),
            rate,
            overview_peaks: measure_peaks(&x),
            original_view: Visual::measure(&x, rate),
            regenerated_view: Some(Visual::measure(&dry, rate)),
            difference_view: None,
            original: x.clone().into(),
            rebuilt_span: Some([0, x.len()]),
            rebuilt: Some(dry.clone()),
            dry: Some(dry.into()),
            layer_plot_only: None,
            recording: Recording {
                channels: 1,
                left_minus_right_db: None,
                lr_similarity: None,
            },
            layers: None,
            guitar_cues: Some(cues),
        });

        // Unreviewed dry candidate MUST block variant export and write nothing
        assert_eq!(app.dry_review, DryReviewStatus::NotReviewed);
        let blocked_dir = base.join("blocked");
        assert!(!app.export_selected_variants(&blocked_dir));
        assert!(!blocked_dir.exists());

        // After explicit human review approval, individual & all selections succeed
        app.dry_review = DryReviewStatus::ApprovedByHuman;
        app.variant_selection = [false, true, false, false, false];
        let single_dir = base.join("single");
        assert!(app.export_selected_variants(&single_dir), "{}", app.status);
        assert!(single_dir.join("natural.wav").exists());
        assert!(!single_dir.join("digital.wav").exists());
        assert!(!single_dir.join("original.wav").exists());

        app.variant_selection = [true; 5];
        let all_dir = base.join("all");
        assert!(app.export_selected_variants(&all_dir), "{}", app.status);
        for v in ExportVariant::ALL {
            assert!(all_dir.join(format!("{}.wav", v.slug())).exists());
        }
        // Existing directory rejected (no overwrite)
        assert!(!app.export_selected_variants(&all_dir));
        assert_eq!(&app.track.as_ref().unwrap().original[..], &x[..]);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn dock_undock_and_resize_preserve_state_without_mutating_audio() {
        let ctx = egui::Context::default();
        let rate = 48_000_u32;
        let x: Vec<f32> = (0..rate * 2)
            .map(|i| (0.25 * (std::f64::consts::TAU * 440.0 * i as f64 / rate as f64).sin()) as f32)
            .collect();
        let mut app = Studio::new();
        app.view_range = [0.0, 2.0];
        app.selection = Some([0.4, 1.2]);
        app.cursor = 0.75;
        app.track = Some(Track {
            name: "dock_test.wav".into(),
            rate,
            overview_peaks: measure_peaks(&x),
            original_view: Visual::measure(&x, rate),
            regenerated_view: Some(Visual::measure(&x, rate)),
            difference_view: None,
            original: x.clone().into(),
            rebuilt_span: Some([0, x.len()]),
            rebuilt: Some(x.clone()),
            dry: Some(x.clone().into()),
            layer_plot_only: None,
            recording: Recording {
                channels: 1,
                left_minus_right_db: None,
                lr_similarity: None,
            },
            layers: None,
            guitar_cues: None,
        });
        let rig_before = app.rig.clone();
        let fp_before = app.post_dry_fingerprint();

        // Exercise every dock/undock combination across narrow (860x580) and wide (1440x900) viewports
        for dock in [
            InspectorDock::Left,
            InspectorDock::Right,
            InspectorDock::Floating,
            InspectorDock::Hidden,
            InspectorDock::Left,
        ] {
            app.inspector_dock = dock;
            for ldock in [LowerDock::Bottom, LowerDock::Floating, LowerDock::Hidden] {
                app.lower_dock = ldock;
                run_frame(&ctx, &mut app, 860., 580.);
                run_frame(&ctx, &mut app, 1440., 900.);
                assert_eq!(app.selection, Some([0.4, 1.2]));
                assert!((app.cursor - 0.75).abs() < 1e-6);
                assert_eq!(app.rig, rig_before);
                assert_eq!(app.post_dry_fingerprint(), fp_before);
                assert_eq!(&app.track.as_ref().unwrap().original[..], &x[..]);
            }
        }
    }

    #[test]
    fn selection_marquee_move_resize_and_in_out_controls_update_bounds_accurately() {
        let rate = 48_000_u32;
        let x = vec![0.1_f32; rate as usize * 4];
        let mut app = Studio::new();
        app.view_range = [0.0, 4.0];
        app.display_max_hz = 12_000.0;
        app.selection_tool = SelectionTool::Marquee;
        app.track = Some(Track {
            name: "sel_test.wav".into(),
            rate,
            overview_peaks: measure_peaks(&x),
            original_view: Visual::measure(&x, rate),
            regenerated_view: None,
            difference_view: None,
            original: x.clone().into(),
            rebuilt_span: None,
            rebuilt: None,
            dry: None,
            layer_plot_only: None,
            recording: Recording {
                channels: 1,
                left_minus_right_db: None,
                lr_similarity: None,
            },
            layers: None,
            guitar_cues: None,
        });

        // 1. Create marquee [0.5..1.5 s, 400..2400 Hz]
        app.apply_timeline_interaction(TimelineInteraction::DragStart {
            t: 0.5,
            hz: Some(400.0),
            mode: SelectionDragMode::Create,
        });
        app.apply_timeline_interaction(TimelineInteraction::DragMove {
            t: 1.5,
            hz: Some(2400.0),
        });
        app.apply_timeline_interaction(TimelineInteraction::DragEnd);
        assert_eq!(app.selection, Some([0.5, 1.5]));
        assert_eq!(app.marquee_hz, Some([400.0, 2400.0]));
        assert_eq!(app.region_edit_draft.kind, "marquee");

        // 2. Move marquee by +0.5 s and +600 Hz
        app.apply_timeline_interaction(TimelineInteraction::DragStart {
            t: 1.0,
            hz: Some(1400.0),
            mode: SelectionDragMode::Move,
        });
        app.apply_timeline_interaction(TimelineInteraction::DragMove {
            t: 1.5,
            hz: Some(2000.0),
        });
        app.apply_timeline_interaction(TimelineInteraction::DragEnd);
        assert_eq!(app.selection, Some([1.0, 2.0]));
        assert_eq!(app.marquee_hz, Some([1000.0, 3000.0]));

        // 3. Resize right + top corner
        app.apply_timeline_interaction(TimelineInteraction::DragStart {
            t: 2.0,
            hz: Some(3000.0),
            mode: SelectionDragMode::ResizeTopRight,
        });
        app.apply_timeline_interaction(TimelineInteraction::DragMove {
            t: 2.5,
            hz: Some(4200.0),
        });
        app.apply_timeline_interaction(TimelineInteraction::DragEnd);
        assert_eq!(app.selection, Some([1.0, 2.5]));
        assert_eq!(app.marquee_hz, Some([1000.0, 4200.0]));

        // 4. Direct In/Out playhead marks and Clear
        app.cursor = 1.25;
        app.set_selection_in_at_cursor();
        assert_eq!(app.selection, Some([1.25, 2.5]));
        app.cursor = 3.0;
        app.set_selection_out_at_cursor();
        assert_eq!(app.selection, Some([1.25, 3.0]));
        app.clear_selection();
        assert_eq!(app.selection, None);
        assert_eq!(app.marquee_hz, None);
        assert_eq!(&app.track.as_ref().unwrap().original[..], &x[..]);
    }
}

#[cfg(test)]
mod layer_tests {
    use super::*;

    #[test]
    fn native_layer_controls_preserve_reference_fixed_gain_and_view_does_not_solo_audio() {
        let ctx = egui::Context::default();
        let rate = 48000;
        let x: Vec<_> = (0..9600)
            .map(|i| (0.2 * (std::f64::consts::TAU * 440. * i as f64 / rate as f64).sin()) as f32)
            .collect();
        let packet = edge::analyze(&x, rate, edge::Options::default()).unwrap();
        let rendered = workspace::render(&packet, 734).unwrap();
        let headroom = rendered.headroom(&x);
        let mut app = Studio::new();
        app.view_range = [0.0, 0.2];
        app.track = Some(Track {
            name: "layers".into(),
            rate,
            overview_peaks: measure_peaks(&x),
            original_view: Visual::measure(&x, rate),
            regenerated_view: None,
            difference_view: None,
            original: x.clone().into(),
            rebuilt_span: Some([0, x.len()]),
            rebuilt: None,
            dry: None,
            layer_plot_only: None,
            recording: Recording {
                channels: 1,
                left_minus_right_db: None,
                lr_similarity: None,
            },
            layers: Some(LayerSession {
                packet,
                rendered,
                controls: Default::default(),
                headroom,
                view: None,
                span: [0, x.len()],
            }),
            guitar_cues: None,
        });
        app.remix_layers(&ctx);
        let full_mix = app.track.as_ref().unwrap().rebuilt.clone().unwrap();
        let reference = app
            .track
            .as_ref()
            .unwrap()
            .audition(Take::Original)
            .unwrap();

        // Setting View = Some(1) MUST change ONLY the plotted buffer, NOT solo audio!
        app.track.as_mut().unwrap().layers.as_mut().unwrap().view = Some(1);
        app.remix_layers(&ctx);
        {
            let t = app.track.as_ref().unwrap();
            assert_eq!(t.layers.as_ref().unwrap().controls.solo, None);
            assert_eq!(
                t.rebuilt.as_ref().unwrap(),
                &full_mix,
                "View must not alter audible mix"
            );
            assert_eq!(
                t.layer_plot_only.as_ref().unwrap(),
                &t.layers.as_ref().unwrap().rendered.stems[1]
            );
        }
        app.track.as_mut().unwrap().layers.as_mut().unwrap().view = None;

        for layer in 0..4 {
            app.track
                .as_mut()
                .unwrap()
                .layers
                .as_mut()
                .unwrap()
                .controls
                .solo = Some(layer);
            app.remix_layers(&ctx);
            let t = app.track.as_ref().unwrap();
            assert_eq!(
                t.rebuilt.as_ref().unwrap(),
                &t.layers.as_ref().unwrap().rendered.stems[layer]
            );
            assert_eq!(t.audition(Take::Original).unwrap(), reference);
        }

        let s = app.track.as_mut().unwrap().layers.as_mut().unwrap();
        s.controls = workspace::Controls {
            gain_db: [0., 0., 0., -6.],
            ..Default::default()
        };
        s.view = Some(1);
        app.remix_layers(&ctx);
        for width in [880., 1320.] {
            let frame = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 900.))),
                    ..Default::default()
                },
                |ctx| app.draw(ctx),
            );
            assert!(!frame.shapes.is_empty());
        }

        // Save editable analysis packet and reload cleanly without source PCM
        let path =
            std::env::temp_dir().join(format!("sc-desktop-packet-{}.json", std::process::id()));
        let bytes =
            serde_json::to_vec(&app.track.as_ref().unwrap().layers.as_ref().unwrap().packet)
                .unwrap();
        spectracraft_mono::qa::write_new(&path, &bytes).unwrap();
        app.load_layers(&path, &ctx);
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while app.job.is_some() && std::time::Instant::now() < deadline {
            app.poll(&ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.job.is_none(), "analysis load worker timed out");
        assert!(app.track.as_ref().unwrap().layers.is_some());
        assert_eq!(
            app.track
                .as_ref()
                .unwrap()
                .layers
                .as_ref()
                .unwrap()
                .controls
                .gain_db,
            [0.; 4]
        );
        std::fs::remove_file(path).unwrap();
        app.cursor = 0.12;
        app.switch(Take::Original);
        assert_eq!(app.cursor, 0.12);
        assert_eq!(&app.track.as_ref().unwrap().original[..], &x);
    }

    #[test]
    fn studio_history_clears_and_enforces_unique_dry_buffer_and_byte_caps() {
        let app = Studio::new();
        let mut hist = StudioHistory {
            max_depth: 64,
            max_audio_bytes: 16 * std::mem::size_of::<f32>(),
            max_unique_dry_buffers: 3,
            ..Default::default()
        };
        for i in 0..6_u32 {
            let mut snap = app.capture_snapshot();
            snap.dry_audio = Some(Arc::from(vec![i as f32; 4]));
            hist.push(snap);
        }
        let (unique_count, total_bytes) = hist.unique_dry_usage();
        assert!(unique_count <= 3);
        assert!(total_bytes <= 16 * std::mem::size_of::<f32>());
        hist.clear();
        assert!(!hist.can_undo());
        assert!(!hist.can_redo());
        assert_eq!(hist.unique_dry_usage(), (0, 0));
    }

    #[test]
    fn studio_seek_rejects_nan_and_begin_job_prevents_overlap_and_cancels_on_replace() {
        let mut app = Studio::new();
        app.cursor = 1.25;
        app.seek_to(f64::NAN);
        app.seek_to(f64::INFINITY);
        assert_eq!(app.cursor, 1.25);

        let (id1, cancel1, _tx1) = app.begin_job(false).expect("first job starts");
        assert!(app.begin_job(false).is_none(), "overlapping render job blocked");
        assert!(!cancel1.load(Ordering::Relaxed));

        let (id2, cancel2, _tx2) = app.begin_job(true).expect("cancelling load job starts");
        assert_ne!(id1, id2);
        assert!(cancel1.load(Ordering::Relaxed));
        assert!(!cancel2.load(Ordering::Relaxed));
    }

    #[test]
    fn track_slice_helpers_match_full_audition_without_full_track_padding() {
        let original: Vec<f32> = (0..100).map(|i| (i as f32) * 0.005).collect();
        let rebuilt: Vec<f32> = (0..20).map(|i| 0.25 + (i as f32) * 0.005).collect();
        let t = Track {
            name: String::new(),
            rate: 48_000,
            overview_peaks: measure_peaks(&original),
            original: original.into(),
            rebuilt_span: Some([30, 50]),
            rebuilt: Some(rebuilt),
            dry: None,
            layer_plot_only: None,
            layers: None,
            guitar_cues: None,
            recording: Recording {
                channels: 1,
                left_minus_right_db: None,
                lr_similarity: None,
            },
            original_view: Visual::measure(&[0.0; 100], 48_000),
            regenerated_view: None,
            difference_view: None,
        };
        let inside = t.aligned_rebuilt_slice(35, 45).unwrap();
        assert!(matches!(inside, Cow::Borrowed(_)));
        assert_eq!(inside.len(), 10);

        let full_orig = t.audition(Take::Original).unwrap();
        let slice_orig = t
            .audition_slice_with_options(Take::Original, true, PreviewBitDepth::Float32, 25, Some(55))
            .unwrap();
        assert_eq!(slice_orig, full_orig[25..55]);

        let full_reg = t.audition(Take::Regenerated).unwrap();
        let slice_reg = t
            .audition_slice_with_options(Take::Regenerated, true, PreviewBitDepth::Float32, 25, Some(55))
            .unwrap();
        assert_eq!(slice_reg, full_reg[25..55]);

        let full_diff = t.audition(Take::Difference).unwrap();
        let slice_diff = t
            .audition_slice_with_options(Take::Difference, true, PreviewBitDepth::Float32, 25, Some(55))
            .unwrap();
        assert_eq!(slice_diff, full_diff[25..55]);
    }

    #[test]
    fn desktop_session_v2_rejects_stale_approval_on_mismatched_source_and_handles_v1_presets() {
        let ctx = egui::Context::default();
        let mut app = Studio::new();
        let orig_a = vec![0.25_f32; 4800];
        let dry_a = vec![0.20_f32; 4800];
        app.track = Some(Track {
            name: "take_a.wav".into(),
            rate: 48_000,
            overview_peaks: measure_peaks(&orig_a),
            original_view: Visual::measure(&orig_a, 48_000),
            regenerated_view: None,
            difference_view: None,
            original: orig_a.into(),
            rebuilt_span: Some([0, 4800]),
            rebuilt: Some(dry_a.clone()),
            dry: Some(Arc::from(dry_a)),
            layer_plot_only: None,
            recording: Recording {
                channels: 1,
                left_minus_right_db: None,
                lr_similarity: None,
            },
            layers: None,
            guitar_cues: None,
        });
        app.dry_checklist = DryListeningChecklist {
            rhythm: true,
            phrasing: true,
            dynamics: true,
            sustain: true,
            artifact_free: true,
        };
        app.sync_dry_review_from_checklist();
        assert_eq!(app.dry_review, DryReviewStatus::ApprovedByHuman);
        assert!(app.approved_dry_sha256.is_some());

        let tmp = std::env::temp_dir().join("spectracraft_audit_h6_session.json");
        app.save_session(&tmp).expect("v2 session saves");

        // Loading on the SAME source + dry preserves approval
        app.invalidate_dry_review();
        assert_eq!(app.dry_review, DryReviewStatus::NotReviewed);
        app.load_session(&tmp, &ctx).expect("v2 session reloads");
        assert_eq!(app.dry_review, DryReviewStatus::ApprovedByHuman);

        // Switching to a different source audio clears approval on load
        let orig_b = vec![-0.4_f32; 4800];
        let dry_b = vec![-0.3_f32; 4800];
        if let Some(t) = app.track.as_mut() {
            t.original = orig_b.into();
            t.dry = Some(Arc::from(dry_b));
        }
        app.load_session(&tmp, &ctx)
            .expect("v2 session loads settings with source mismatch warning");
        assert_eq!(app.dry_review, DryReviewStatus::NotReviewed);
        assert!(app.status.contains("WARNING"));
        let _ = std::fs::remove_file(&tmp);
    }
}
