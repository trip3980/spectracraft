//! Top-level experimental native GPUI workstation view for SpectraCraft.
//!
//! Coordinates:
//! - `AppSession` (authoritative framework-neutral state, provenance, Undo/Redo, export guards)
//! - `GpuiJobBridge` (bounded background worker pool + LRU spectrogram tile cache)
//! - `TransportController` (`rodio` authoritative playback clock)
//! - `audio_canvas` (cached GPU `RenderImage` spectrogram + peaks + playhead/selection)
//! - `inspector` (five-stage pipeline controls, `SC-GOV-04` review gate, atomic export)
//! - `settings_store` (`craft-settings` workspace preference persistence)

use crate::audio_canvas::{
    AudioCanvasData, TileTextureCache, audio_canvas_element, x_to_time_secs,
};
use crate::bridge::{GpuiJobBridge, TileViewportRequest};
use crate::inspector::render_inspector;
use crate::transport::TransportController;
use gpui::{
    App, Bounds, Context, FocusHandle, Focusable, IntoElement, KeyBinding, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Render, SharedString, Window, actions,
    div, prelude::*, px, rgb,
};
use spectracraft_mono::app_jobs::JobOutcome;
use spectracraft_mono::app_session::{AppSession, AudioArtifact, AudioOrigin, DisplayTake, ExportTarget};
use spectracraft_mono::daw_bridge::{
    self, EditorTarget, ExportBitDepth, InterchangeExportRequest, StemTrack,
};
use spectracraft_mono::pipeline::PluginId;
use spectracraft_mono::platform;
use spectracraft_mono::qa;
use spectracraft_mono::region_edit::RegionFixAction;
use spectracraft_mono::reperform::CharacterTarget;
use spectracraft_mono::settings_store::{self, SpectraCraftSettings};
use spectracraft_mono::studio_catalog::RoomPreset;
use spectracraft_mono::technical_report;
use spectracraft_mono::visualization::{FrequencyScale, Peaks};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

actions!(
    spectracraft_gpui,
    [
        PlayPause,
        StopTransport,
        UndoEdit,
        RedoEdit,
        ZoomIn,
        ZoomOut,
        FitView,
        SelectOriginalTake,
        SelectDryTake,
        SelectCaptureTake,
        QuitApp,
    ]
);

pub fn register_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", PlayPause, Some("SpectraCraftWorkspace")),
        KeyBinding::new("escape", StopTransport, Some("SpectraCraftWorkspace")),
        KeyBinding::new("ctrl-z", UndoEdit, Some("SpectraCraftWorkspace")),
        KeyBinding::new("cmd-z", UndoEdit, Some("SpectraCraftWorkspace")),
        KeyBinding::new("ctrl-shift-z", RedoEdit, Some("SpectraCraftWorkspace")),
        KeyBinding::new("cmd-shift-z", RedoEdit, Some("SpectraCraftWorkspace")),
        KeyBinding::new("ctrl-q", QuitApp, Some("SpectraCraftWorkspace")),
        KeyBinding::new("cmd-q", QuitApp, Some("SpectraCraftWorkspace")),
        KeyBinding::new("=", ZoomIn, Some("SpectraCraftWorkspace")),
        KeyBinding::new("-", ZoomOut, Some("SpectraCraftWorkspace")),
        KeyBinding::new("0", FitView, Some("SpectraCraftWorkspace")),
        KeyBinding::new("1", SelectOriginalTake, Some("SpectraCraftWorkspace")),
        KeyBinding::new("2", SelectDryTake, Some("SpectraCraftWorkspace")),
        KeyBinding::new("3", SelectCaptureTake, Some("SpectraCraftWorkspace")),
    ]);
}

/// Experimental GPUI native workstation entity.
pub struct SpectraCraftGpuiApp {
    pub session: AppSession,
    pub bridge: GpuiJobBridge,
    pub transport: TransportController,
    pub texture_cache: TileTextureCache,
    pub settings: SpectraCraftSettings,
    pub status_line: String,
    pub error_banner: Option<String>,
    focus_handle: FocusHandle,
    last_canvas_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    drag_anchor_secs: Option<f64>,
    is_dragging_selection: bool,
    peaks_cache: Option<(u64, Arc<Peaks>)>,
}

impl Focusable for SpectraCraftGpuiApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SpectraCraftGpuiApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let settings_snap = settings_store::load_settings(None);
        let mut session = AppSession::new(1);
        session.display.show_waveform = settings_snap.settings.workspace.show_waveform;
        session.display.show_spectrogram = settings_snap.settings.workspace.show_spectrogram;

        let bridge = GpuiJobBridge::new();
        let wakeup = bridge.wakeup();

        // Spawn an event-driven async listener using a weak view handle so closing
        // the window cleanly exits the task and cancels worker jobs without blocking any thread.
        cx.spawn(async move |weak_view, cx| {
            let mut seq = 0_u64;
            while let Some(next_seq) = wakeup.wait_next_async(seq).await {
                seq = next_seq;
                let alive = weak_view.update(cx, |this, cx| {
                    this.poll_worker_outcomes(cx);
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();

        Self {
            session,
            bridge,
            transport: TransportController::default(),
            texture_cache: TileTextureCache::new(64),
            settings: settings_snap.settings,
            status_line:
                "Experimental GPUI Shell Ready - Open a mono/stereo WAV (8-96 kHz) to begin."
                    .to_owned(),
            error_banner: None,
            focus_handle,
            last_canvas_bounds: Rc::new(RefCell::new(None)),
            drag_anchor_secs: None,
            is_dragging_selection: false,
            peaks_cache: None,
        }
    }

    /// Spawn a lightweight playback-clock refresh loop strictly while audio is playing.
    /// Does NOT advance the playhead itself; reads authoritative `TransportController::position_secs`.
    fn start_playhead_observer(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |weak_view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let keep_going = weak_view.update(cx, |this, cx| {
                    let dur = this
                        .session
                        .active_display_artifact()
                        .map(|a| a.duration_s())
                        .unwrap_or(0.0);
                    let _ = this.transport.position_secs(dur);
                    cx.notify();
                    this.transport.is_playing()
                });
                match keep_going {
                    Ok(true) => continue,
                    _ => break,
                }
            }
        })
        .detach();
    }

    /// Drain completed background jobs and publish valid ones through `AppSession`.
    pub fn poll_worker_outcomes(&mut self, cx: &mut Context<Self>) {
        let outcomes = self.bridge.drain_outcomes();
        for outcome in outcomes {
            match outcome {
                JobOutcome::WavLoaded {
                    path,
                    name,
                    artifact,
                } => {
                    self.transport.stop();
                    self.bridge.clear_visuals();
                    self.texture_cache.clear();
                    self.peaks_cache = None;
                    let max_hz = self.session.display.display_max_hz;
                    self.session
                        .load_source_artifact(path, name.clone(), artifact);
                    if let Some(src) = self.session.source.clone() {
                        let _ = self.bridge.submit_overview_visual(src, max_hz);
                    }
                    self.error_banner = None;
                    self.status_line = format!("Loaded {name} (immutable original reference).");
                }
                JobOutcome::DryReady {
                    guard,
                    performance,
                    samples,
                } => match self
                    .session
                    .publish_dry_result(&guard, performance, samples)
                {
                    Ok(dry) => {
                        let max_hz = self.session.display.display_max_hz;
                        let _ = self.bridge.submit_overview_visual(dry.clone(), max_hz);
                        self.error_banner = None;
                        self.status_line = format!(
                            "Stage 1 dry synthesized ({} frames, SHA {}). Review before post-dry.",
                            dry.frames,
                            &dry.sha256[..12.min(dry.sha256.len())]
                        );
                    }
                    Err(stale) => {
                        self.status_line = format!("Ignored stale dry completion: {stale}");
                    }
                },
                JobOutcome::RegionEditReady { guard, output } => {
                    match self.session.publish_region_edit_result(&guard, output) {
                        Ok(dry) => {
                            let max_hz = self.session.display.display_max_hz;
                            let _ = self.bridge.submit_overview_visual(dry.clone(), max_hz);
                            self.error_banner = None;
                            self.status_line = format!(
                                "Stage 2 dry edit applied (outside bit-identical, SHA {}).",
                                &dry.sha256[..12.min(dry.sha256.len())]
                            );
                        }
                        Err(stale) => {
                            self.status_line = format!("Ignored stale region edit: {stale}");
                        }
                    }
                }
                JobOutcome::PostDryReady { guard, output } => {
                    let identical = output.summary.bit_identical_to_dry;
                    match self.session.publish_post_dry_result(&guard, output) {
                        Ok(cap) => {
                            let max_hz = self.session.display.display_max_hz;
                            let _ = self.bridge.submit_overview_visual(cap.clone(), max_hz);
                            self.session.display.take = DisplayTake::Capture;
                            self.error_banner = None;
                            self.status_line = if identical {
                                "Stages 3-5 rendered (all bypassed, bit-identical to dry)."
                                    .to_owned()
                            } else {
                                format!(
                                    "Stages 3-5 capture ready ({} frames, SHA {}).",
                                    cap.frames,
                                    &cap.sha256[..12.min(cap.sha256.len())]
                                )
                            };
                        }
                        Err(stale) => {
                            self.status_line = format!("Ignored stale capture: {stale}");
                        }
                    }
                }
                JobOutcome::Failed { error, .. } => {
                    if !error.cancelled {
                        self.error_banner = Some(error.to_string());
                        self.status_line = format!("Error: {}", error.message);
                    }
                }
                JobOutcome::OverviewVisualReady { .. }
                | JobOutcome::SpectrogramTileReady { .. } => {}
            }
        }
        cx.notify();
    }

    // ---------- File & Workflow Commands ----------

    pub fn open_wav_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let id = self.session.allocate_artifact_id();
        self.error_banner = None;
        self.status_line = format!("Loading {}...", path.display());
        if let Err(err) = self.bridge.submit_load_wav(path, id) {
            self.error_banner = Some(err.to_string());
        }
        cx.notify();
    }

    pub fn trigger_open_wav_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = platform::dialogs::pick_wav_file() {
            self.open_wav_path(path, cx);
        }
    }

    /// Load the deterministic built-in six-note 48 kHz mono reference for immediate auditioning
    /// across Windows, macOS, and Linux without needing an external file first.
    pub fn load_builtin_reference(&mut self, cx: &mut Context<Self>) {
        let (samples, _) = qa::six_note();
        let id = self.session.allocate_artifact_id();
        match AudioArtifact::new_mono(
            id,
            AudioOrigin::OriginalSource,
            48_000,
            samples,
            None,
            "builtin-six-note-ref".to_owned(),
        ) {
            Ok(artifact) => {
                self.transport.stop();
                self.bridge.clear_visuals();
                self.texture_cache.clear();
                self.peaks_cache = None;
                let max_hz = self.session.display.display_max_hz;
                let ref_path = platform::temp_dir().join("spectracraft-six-note-reference.wav");
                let label = "Built-in Six-Note Reference (48 kHz)".to_owned();
                self.session
                    .load_source_artifact(ref_path, label, Arc::new(artifact));
                if let Some(src) = self.session.source.clone() {
                    let _ = self.bridge.submit_overview_visual(src, max_hz);
                }
                self.error_banner = None;
                self.status_line =
                    "Loaded built-in six-note reference (ready for Stage 1 Dry Regeneration)."
                        .to_owned();
            }
            Err(err) => {
                self.error_banner = Some(err);
            }
        }
        cx.notify();
    }

    pub fn trigger_dry_regeneration(&mut self, cx: &mut Context<Self>) {
        match self.session.begin_dry_request() {
            Ok(spec) => {
                self.error_banner = None;
                self.status_line = "Running Stage 1 dry regeneration off-thread...".to_owned();
                if let Err(err) = self.bridge.submit_dry_regen(spec) {
                    self.error_banner = Some(err.to_string());
                }
            }
            Err(err) => self.error_banner = Some(err),
        }
        cx.notify();
    }

    pub fn trigger_region_edit(&mut self, cx: &mut Context<Self>) {
        match self.session.begin_region_edit_request() {
            Ok(spec) => {
                self.error_banner = None;
                self.status_line = "Running Stage 2 surgical dry edit off-thread...".to_owned();
                if let Err(err) = self.bridge.submit_region_edit(spec) {
                    self.error_banner = Some(err.to_string());
                }
            }
            Err(err) => self.error_banner = Some(err),
        }
        cx.notify();
    }

    pub fn trigger_post_dry_render(&mut self, cx: &mut Context<Self>) {
        match self.session.begin_post_dry_request() {
            Ok(spec) => {
                self.error_banner = None;
                self.status_line = "Rendering Stages 3-5 post-dry capture off-thread...".to_owned();
                if let Err(err) = self.bridge.submit_post_dry(spec) {
                    self.error_banner = Some(err.to_string());
                }
            }
            Err(err) => self.error_banner = Some(err),
        }
        cx.notify();
    }

    pub fn export_target_to_path(
        &mut self,
        target: ExportTarget,
        out_path: &Path,
        cx: &mut Context<Self>,
    ) {
        match self.session.export_artifact_atomic(target, out_path) {
            Ok(receipt) => {
                self.error_banner = None;
                self.status_line = format!(
                    "Exported {:?} atomically to {} (WAV SHA {}).",
                    receipt.target,
                    receipt.path.display(),
                    &receipt.wav.sha256[..12.min(receipt.wav.sha256.len())]
                );
            }
            Err(err) => self.error_banner = Some(err),
        }
        cx.notify();
    }

    pub fn trigger_export_dialog(&mut self, target: ExportTarget, cx: &mut Context<Self>) {
        let default_name = match target {
            ExportTarget::Dry => "spectracraft_dry.wav",
            ExportTarget::CurrentCapture => "spectracraft_capture.wav",
        };
        if let Some(path) = platform::dialogs::save_wav_file(default_name) {
            self.export_target_to_path(target, &path, cx);
        }
    }

    pub fn trigger_save_preset_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = platform::dialogs::save_session_file("spectracraft_preset.json") {
            match self.session.save_preset_atomic(&path) {
                Ok(()) => {
                    self.error_banner = None;
                    self.status_line = format!("Saved settings preset to {}.", path.display());
                }
                Err(err) => self.error_banner = Some(err),
            }
            cx.notify();
        }
    }

    pub fn trigger_load_preset_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = platform::dialogs::pick_session_file() {
            match self.session.load_preset_file(&path) {
                Ok(kind) => {
                    self.error_banner = None;
                    self.status_line = format!(
                        "Loaded {kind:?} from {} (dry audio & unverified approvals not faked).",
                        path.display()
                    );
                }
                Err(err) => self.error_banner = Some(err),
            }
            cx.notify();
        }
    }

    /// Generate a strictly blind Before/After technical report and export it atomically to PDF or Markdown.
    pub fn export_technical_report_to_path(&mut self, out_path: &Path, cx: &mut Context<Self>) {
        let Some(source) = self.session.source.as_ref() else {
            self.error_banner = Some("Load a reference WAV before exporting a report.".to_owned());
            cx.notify();
            return;
        };
        let (after_art, stage_label) = match (&self.session.capture, self.session.capture_stale) {
            (Some(cap), false) => (cap.clone(), "Stages 1-5 Current Capture"),
            _ => match &self.session.dry {
                Some(dry) => (dry.clone(), "Stage 1/2 Dry Regeneration"),
                None => {
                    self.error_banner = Some(
                        "Run Stage 1 Dry Regeneration before exporting a blind Before/After report."
                            .to_owned(),
                    );
                    cx.notify();
                    return;
                }
            },
        };
        let input_title = self
            .session
            .source_name
            .as_deref()
            .unwrap_or("SpectraCraft Reference");
        match technical_report::build_technical_report(
            &source.samples,
            &after_art.samples,
            source.sample_rate,
            input_title,
            stage_label,
        ) {
            Ok(report) => {
                let is_pdf = out_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("pdf"))
                    .unwrap_or(false);
                let res = if is_pdf {
                    technical_report::export_report_pdf_craft(&report, out_path)
                } else {
                    technical_report::export_report(&report, out_path, "markdown")
                };
                match res {
                    Ok(receipt) => {
                        self.error_banner = None;
                        self.status_line = format!(
                            "Exported blind {} report to {} ({}).",
                            receipt.actual_format.to_uppercase(),
                            out_path.display(),
                            receipt.engine
                        );
                    }
                    Err(err) => self.error_banner = Some(err),
                }
            }
            Err(err) => self.error_banner = Some(err),
        }
        cx.notify();
    }

    pub fn trigger_export_report_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = platform::dialogs::save_report_file("spectracraft_report.pdf") {
            self.export_technical_report_to_path(&path, cx);
        }
    }

    /// Export verified generated stems (never opening original PCM) into a universal DAW interchange bundle.
    pub fn export_daw_bundle_to_dir(&mut self, out_dir: &Path, cx: &mut Context<Self>) {
        let Some(dry) = self.session.dry.as_ref() else {
            self.error_banner =
                Some("Run Stage 1 Dry Regeneration before exporting a DAW bundle.".to_owned());
            cx.notify();
            return;
        };
        let mut stems = vec![StemTrack {
            role: "dry".to_owned(),
            display_name: "Stage 1-2 Dry Regeneration".to_owned(),
            samples: dry.samples.to_vec(),
        }];
        if let (Some(cap), false) = (&self.session.capture, self.session.capture_stale) {
            stems.push(StemTrack {
                role: "capture".to_owned(),
                display_name: "Stages 3-5 Capture".to_owned(),
                samples: cap.samples.to_vec(),
            });
        }
        let markers = self
            .session
            .performance
            .as_ref()
            .map(daw_bridge::markers_from_performance)
            .unwrap_or_default();
        let title = self
            .session
            .source_name
            .clone()
            .unwrap_or_else(|| "SpectraCraft Session".to_owned());
        let req = InterchangeExportRequest {
            project_title: title,
            target: EditorTarget::UniversalDaw,
            sample_rate: dry.sample_rate,
            bit_depth: ExportBitDepth::Float32,
            tempo_bpm: 120.0,
            stems,
            markers,
            engine_state_json: None,
        };
        match daw_bridge::export_for_editor(out_dir, &req) {
            Ok(receipt) => {
                self.error_banner = None;
                self.status_line = format!(
                    "Exported {} DAW bundle ({} stems, {} markers) to {}.",
                    receipt.target_name,
                    receipt.stem_count,
                    receipt.marker_count,
                    out_dir.display()
                );
            }
            Err(err) => self.error_banner = Some(err),
        }
        cx.notify();
    }

    pub fn trigger_export_daw_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(dir) = platform::dialogs::pick_export_folder() {
            self.export_daw_bundle_to_dir(&dir, cx);
        }
    }

    // ---------- Inspector Mutators ----------

    pub fn step_source_index(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.session.sources.len().max(1);
        let next =
            ((self.session.selected_source_idx as isize + delta).rem_euclid(len as isize)) as usize;
        self.session.set_source_index(next);
        cx.notify();
    }

    pub fn set_character_target(&mut self, target: CharacterTarget, cx: &mut Context<Self>) {
        let mut voicing = self.session.voicing.clone();
        voicing.character_target = target;
        self.session.set_voicing(voicing);
        cx.notify();
    }

    pub fn toggle_checklist_item(&mut self, idx: usize, cx: &mut Context<Self>) {
        let mut c = self.session.dry_review.checklist;
        match idx {
            0 => c.rhythm = !c.rhythm,
            1 => c.phrasing = !c.phrasing,
            2 => c.dynamics = !c.dynamics,
            3 => c.sustain = !c.sustain,
            4 => c.artifact_free = !c.artifact_free,
            _ => {}
        }
        self.session.set_dry_checklist(c);
        cx.notify();
    }

    pub fn set_region_action(&mut self, action: RegionFixAction, cx: &mut Context<Self>) {
        let mut draft = self.session.region_edit_draft.clone();
        draft.action = action;
        self.session.set_region_edit_draft(draft);
        cx.notify();
    }

    pub fn select_middle_quarter(&mut self, cx: &mut Context<Self>) {
        if let Some(dry) = &self.session.dry {
            let d = dry.duration_s();
            self.session.set_selection(Some([d * 0.35, d * 0.60]));
            cx.notify();
        }
    }

    pub fn toggle_mic_stage(&mut self, cx: &mut Context<Self>) {
        self.session.set_mic_enabled(!self.session.mic_enabled);
        cx.notify();
    }

    pub fn step_mic_index(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.session.mics.len().max(1);
        let cur = self
            .session
            .rig
            .mics
            .get(self.session.rig.selected_mic)
            .map(|m| m.catalog_index)
            .unwrap_or(0);
        let next = ((cur as isize + delta).rem_euclid(len as isize)) as usize;
        self.session.set_mic_catalog_index(next);
        cx.notify();
    }

    pub fn step_preamp_drive(&mut self, delta: f64, cx: &mut Context<Self>) {
        self.session
            .set_preamp_drive(self.session.preamp_drive + delta);
        cx.notify();
    }

    pub fn toggle_room_stage(&mut self, cx: &mut Context<Self>) {
        self.session.set_room_enabled(!self.session.room_enabled);
        cx.notify();
    }

    pub fn set_room_preset(&mut self, preset: RoomPreset, cx: &mut Context<Self>) {
        self.session.set_room_preset(preset);
        cx.notify();
    }

    pub fn toggle_plugin_stage(&mut self, cx: &mut Context<Self>) {
        let mut slot = self.session.plugin_slot.clone();
        slot.enabled = !slot.enabled;
        if slot.enabled && slot.plugin_id == PluginId::None {
            slot.plugin_id = PluginId::GentleEq;
        }
        self.session.set_plugin_slot(slot);
        cx.notify();
    }

    pub fn set_plugin_id(&mut self, id: PluginId, cx: &mut Context<Self>) {
        let mut slot = self.session.plugin_slot.clone();
        slot.plugin_id = id;
        self.session.set_plugin_slot(slot);
        cx.notify();
    }

    pub fn perform_undo(&mut self, cx: &mut Context<Self>) {
        if self.session.undo() {
            if let Some(dry) = self.session.dry.clone() {
                let max_hz = self.session.display.display_max_hz;
                let _ = self.bridge.submit_overview_visual(dry, max_hz);
            }
            self.status_line = "Undo restored prior session & dry audio revision.".to_owned();
            cx.notify();
        }
    }

    pub fn perform_redo(&mut self, cx: &mut Context<Self>) {
        if self.session.redo() {
            if let Some(dry) = self.session.dry.clone() {
                let max_hz = self.session.display.display_max_hz;
                let _ = self.bridge.submit_overview_visual(dry, max_hz);
            }
            self.status_line = "Redo restored next session & dry audio revision.".to_owned();
            cx.notify();
        }
    }

    // ---------- Transport & Display-Only Controls (NEVER mutate audio) ----------

    pub fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        let art = self.session.active_display_artifact();
        match self.transport.toggle(art.as_ref(), self.session.selection) {
            Ok(true) => {
                self.error_banner = None;
                self.status_line = format!("Playing {}...", self.session.display.take.label());
                self.start_playhead_observer(cx);
            }
            Ok(false) => {
                self.status_line = "Playback stopped.".to_owned();
            }
            Err(err) => {
                self.error_banner = Some(err);
            }
        }
        cx.notify();
    }

    pub fn stop_playback(&mut self, cx: &mut Context<Self>) {
        let _ = self.transport.stop();
        self.status_line = "Playback stopped.".to_owned();
        cx.notify();
    }

    pub fn select_take(&mut self, take: DisplayTake, cx: &mut Context<Self>) {
        if self.session.artifact_for_take(take).is_none() {
            return;
        }
        let was_playing = self.transport.is_playing();
        if was_playing {
            let _ = self.transport.stop();
        }
        self.session.display.take = take;
        if was_playing
            && let Some(a) = self.session.active_display_artifact()
            && self
                .transport
                .play_artifact(&a, self.session.selection)
                .is_ok()
        {
            self.start_playhead_observer(cx);
        }
        cx.notify();
    }

    pub fn zoom_timeline(&mut self, factor: f64, cx: &mut Context<Self>) {
        let dur = self
            .session
            .active_display_artifact()
            .map(|a| a.duration_s())
            .unwrap_or(1.0)
            .max(0.05);
        let [v0, v1] = self.session.display.view_range;
        let center = 0.5 * (v0 + v1);
        let next_span = ((v1 - v0) * factor).clamp(0.02_f64.min(dur), dur);
        let lo = (center - next_span * 0.5).clamp(0.0, (dur - next_span).max(0.0));
        self.session.display.view_range = [lo, (lo + next_span).min(dur)];
        cx.notify();
    }

    pub fn fit_timeline(&mut self, cx: &mut Context<Self>) {
        let dur = self
            .session
            .active_display_artifact()
            .map(|a| a.duration_s())
            .unwrap_or(1.0)
            .max(0.05);
        self.session.display.view_range = [0.0, dur];
        cx.notify();
    }

    pub fn toggle_scale(&mut self, cx: &mut Context<Self>) {
        self.session.display.scale = match self.session.display.scale {
            FrequencyScale::Linear => FrequencyScale::Log,
            FrequencyScale::Log => FrequencyScale::Linear,
        };
        cx.notify();
    }

    pub fn toggle_waveform(&mut self, cx: &mut Context<Self>) {
        if self.session.display.show_waveform && !self.session.display.show_spectrogram {
            return;
        }
        self.session.display.show_waveform = !self.session.display.show_waveform;
        self.persist_workspace_prefs(cx);
    }

    pub fn toggle_spectrogram(&mut self, cx: &mut Context<Self>) {
        if self.session.display.show_spectrogram && !self.session.display.show_waveform {
            return;
        }
        self.session.display.show_spectrogram = !self.session.display.show_spectrogram;
        self.persist_workspace_prefs(cx);
    }

    pub fn persist_workspace_prefs(&mut self, cx: &mut Context<Self>) {
        self.settings.workspace.show_waveform = self.session.display.show_waveform;
        self.settings.workspace.show_spectrogram = self.session.display.show_spectrogram;
        let _ = settings_store::save_settings(None, self.settings.clone());
        cx.notify();
    }

    // ---------- Pointer Helpers ----------

    fn handle_canvas_mouse_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(bounds) = *self.last_canvas_bounds.borrow() else {
            return;
        };
        let dur = self
            .session
            .active_display_artifact()
            .map(|a| a.duration_s())
            .unwrap_or(0.0);
        if dur <= 0.0 {
            return;
        }
        let secs = x_to_time_secs(
            f32::from(ev.position.x),
            f32::from(bounds.origin.x) + 6.0,
            (f32::from(bounds.size.width) - 12.0).max(1.0),
            self.session.display.view_range,
        );
        self.drag_anchor_secs = Some(secs);
        self.is_dragging_selection = true;
        self.transport.seek(secs, dur);
        cx.notify();
    }

    fn handle_canvas_mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.is_dragging_selection {
            return;
        }
        let Some(anchor) = self.drag_anchor_secs else {
            return;
        };
        let Some(bounds) = *self.last_canvas_bounds.borrow() else {
            return;
        };
        let secs = x_to_time_secs(
            f32::from(ev.position.x),
            f32::from(bounds.origin.x) + 6.0,
            (f32::from(bounds.size.width) - 12.0).max(1.0),
            self.session.display.view_range,
        );
        if (secs - anchor).abs() > 0.01 {
            self.session.set_selection(Some([anchor, secs]));
            cx.notify();
        }
    }

    fn handle_canvas_mouse_up(&mut self, _ev: &MouseUpEvent, cx: &mut Context<Self>) {
        self.is_dragging_selection = false;
        self.drag_anchor_secs = None;
        cx.notify();
    }

    fn header_btn(
        id: &'static str,
        label: impl Into<SharedString>,
        active: bool,
        disabled: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        let mut el = div()
            .id(id)
            .px(px(9.0))
            .py(px(4.0))
            .rounded(px(3.0))
            .bg(if disabled {
                rgb(0x232329)
            } else if active {
                rgb(0x2a5c96)
            } else {
                rgb(0x2e2e36)
            })
            .border_1()
            .border_color(if active { rgb(0x378ef0) } else { rgb(0x3a3a44) })
            .text_xs()
            .text_color(if disabled {
                rgb(0x6c6d78)
            } else {
                rgb(0xeeeef3)
            })
            .child(label.into());
        if !disabled {
            el = el
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0x3a3a45)).border_color(rgb(0x378ef0)))
                .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)));
        }
        el
    }
}

impl Drop for SpectraCraftGpuiApp {
    fn drop(&mut self) {
        let _ = self.transport.stop();
        self.bridge.shutdown();
    }
}

impl Render for SpectraCraftGpuiApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_artifact = self.session.active_display_artifact();
        let duration_secs = active_artifact
            .as_ref()
            .map(|a| a.duration_s())
            .unwrap_or(0.0);
        let playhead_secs = self.transport.position_secs(duration_secs);
        let is_playing = self.transport.is_playing();

        // Retrieve cached peaks & spectrogram tile (never runs FFT on UI thread).
        let (peaks_arc, tile_image, peak_text, nyquist_text) = if let Some(art) = &active_artifact {
            let peaks = if let Some(vis) = self.bridge.visual_for(art.id) {
                let p = match &self.peaks_cache {
                    Some((cached_id, existing)) if *cached_id == art.id => existing.clone(),
                    _ => {
                        let arc = Arc::new(vis.peaks.clone());
                        self.peaks_cache = Some((art.id, arc.clone()));
                        arc
                    }
                };
                (Some(p), vis.peak_dbfs())
            } else {
                (None, "measuring...".to_owned())
            };

            let [v0, v1] = self.session.display.view_range;
            let f0 = ((v0 * f64::from(art.sample_rate)).floor() as usize).min(art.frames);
            let f1 = ((v1 * f64::from(art.sample_rate)).ceil() as usize).clamp(f0, art.frames);
            let tile_img = self
                .bridge
                .get_or_request_tile(
                    art,
                    TileViewportRequest {
                        frame_start: f0,
                        frame_end: f1,
                        width: 384,
                        height: 144,
                        max_hz: self.session.display.display_max_hz,
                        scale: self.session.display.scale,
                    },
                )
                .and_then(|tile| self.texture_cache.get_or_build(&tile));

            (
                peaks.0,
                tile_img,
                peaks.1,
                format!(
                    "{:.1} kHz | Nyquist {:.1} kHz | {} frames",
                    f64::from(art.sample_rate) / 1000.0,
                    f64::from(art.sample_rate) / 2000.0,
                    art.frames
                ),
            )
        } else {
            (
                None,
                None,
                "no audio".to_owned(),
                "No WAV loaded".to_owned(),
            )
        };

        let accent_hex = match self.session.display.take {
            DisplayTake::Original => 0x378ef0,
            DisplayTake::Dry => 0x34d399,
            DisplayTake::Capture => 0xf59e0b,
        };

        let canvas_data = AudioCanvasData {
            peaks: peaks_arc,
            tile_image,
            show_waveform: self.session.display.show_waveform,
            show_spectrogram: self.session.display.show_spectrogram,
            duration_secs,
            view_range: self.session.display.view_range,
            playhead_secs,
            selection: self.session.selection,
            accent_hex,
        };
        let bounds_sink = self.last_canvas_bounds.clone();

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(12.0))
            .py(px(8.0))
            .bg(rgb(0x1f1f24))
            .border_b_1()
            .border_color(rgb(0x3a3a44))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(0xeeeef3))
                            .child("SpectraCraft"),
                    )
                    .child(
                        div()
                            .px(px(6.0))
                            .py(px(1.0))
                            .rounded(px(3.0))
                            .bg(rgb(0x2a3346))
                            .text_xs()
                            .text_color(rgb(0x7cc4fa))
                            .child(format!("GPUI ({})", platform::current_platform().label())),
                    )
                    .child(Self::header_btn(
                        "open-wav",
                        "Open WAV...",
                        false,
                        false,
                        cx,
                        |this, cx| this.trigger_open_wav_dialog(cx),
                    ))
                    .child(Self::header_btn(
                        "load-demo-ref",
                        "Demo Ref",
                        false,
                        false,
                        cx,
                        |this, cx| this.load_builtin_reference(cx),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(Self::header_btn(
                        "take-orig",
                        "1 Original",
                        self.session.display.take == DisplayTake::Original,
                        self.session.source.is_none(),
                        cx,
                        |this, cx| this.select_take(DisplayTake::Original, cx),
                    ))
                    .child(Self::header_btn(
                        "take-dry",
                        "2 Dry",
                        self.session.display.take == DisplayTake::Dry,
                        self.session.dry.is_none(),
                        cx,
                        |this, cx| this.select_take(DisplayTake::Dry, cx),
                    ))
                    .child(Self::header_btn(
                        "take-cap",
                        if self.session.capture_stale {
                            "3 Capture (Stale)"
                        } else {
                            "3 Capture"
                        },
                        self.session.display.take == DisplayTake::Capture,
                        self.session.capture.is_none(),
                        cx,
                        |this, cx| this.select_take(DisplayTake::Capture, cx),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(Self::header_btn(
                        "play-toggle",
                        if is_playing { "Pause" } else { "Play" },
                        is_playing,
                        active_artifact.is_none(),
                        cx,
                        |this, cx| this.toggle_playback(cx),
                    ))
                    .child(Self::header_btn(
                        "stop-transport",
                        "Stop",
                        false,
                        active_artifact.is_none(),
                        cx,
                        |this, cx| this.stop_playback(cx),
                    ))
                    .child(Self::header_btn(
                        "zoom-in",
                        "Zoom +",
                        false,
                        active_artifact.is_none(),
                        cx,
                        |this, cx| this.zoom_timeline(0.5, cx),
                    ))
                    .child(Self::header_btn(
                        "zoom-out",
                        "Zoom -",
                        false,
                        active_artifact.is_none(),
                        cx,
                        |this, cx| this.zoom_timeline(2.0, cx),
                    ))
                    .child(Self::header_btn(
                        "zoom-fit",
                        "Fit",
                        false,
                        active_artifact.is_none(),
                        cx,
                        |this, cx| this.fit_timeline(cx),
                    ))
                    .child(Self::header_btn(
                        "toggle-scale",
                        match self.session.display.scale {
                            FrequencyScale::Linear => "Scale: Lin",
                            FrequencyScale::Log => "Scale: Log",
                        },
                        false,
                        false,
                        cx,
                        |this, cx| this.toggle_scale(cx),
                    ))
                    .child(Self::header_btn(
                        "toggle-wave",
                        "Wave",
                        self.session.display.show_waveform,
                        false,
                        cx,
                        |this, cx| this.toggle_waveform(cx),
                    ))
                    .child(Self::header_btn(
                        "toggle-spec",
                        "Spec",
                        self.session.display.show_spectrogram,
                        false,
                        cx,
                        |this, cx| this.toggle_spectrogram(cx),
                    )),
            );

        let error_strip = self.error_banner.as_ref().map(|err| {
            div()
                .px(px(12.0))
                .py(px(5.0))
                .bg(rgb(0x3a1d22))
                .border_b_1()
                .border_color(rgb(0xe65f5f))
                .text_xs()
                .text_color(rgb(0xffd2d2))
                .child(err.clone())
        });

        let workspace = div()
            .flex_1()
            .flex()
            .flex_col()
            .p(px(10.0))
            .gap(px(8.0))
            .bg(rgb(0x16161a))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(0xa1a1ac))
                    .child(format!(
                        "Take: {} | Playhead: {:.3}s / {:.3}s | View: {:.2}s - {:.2}s",
                        self.session.display.take.label(),
                        playhead_secs,
                        duration_secs,
                        self.session.display.view_range[0],
                        self.session.display.view_range[1],
                    ))
                    .child(format!("{peak_text} | {nyquist_text}")),
            )
            .child(
                div()
                    .flex_1()
                    .size_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                            this.handle_canvas_mouse_down(ev, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                        this.handle_canvas_mouse_move(ev, cx);
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                            this.handle_canvas_mouse_up(ev, cx);
                        }),
                    )
                    .child(audio_canvas_element(canvas_data, move |b| {
                        *bounds_sink.borrow_mut() = Some(b);
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(8.0))
                    .py(px(5.0))
                    .rounded(px(3.0))
                    .bg(rgb(0x1f1f24))
                    .border_1()
                    .border_color(rgb(0x3a3a44))
                    .text_xs()
                    .text_color(rgb(0xd4d4dc))
                    .child(self.status_line.clone())
                    .child(format!(
                        "Jobs: {} active / {} queued | Tiles: {} ({} KB) | FFTs: {}",
                        self.bridge.in_flight_count(),
                        self.bridge.queued_count(),
                        self.bridge.tile_cache().len(),
                        self.bridge.tile_cache().current_bytes() / 1024,
                        self.bridge.tile_cache().fft_compute_count(),
                    )),
            );

        let mut root = div()
            .id("spectracraft-gpui-root")
            .key_context("SpectraCraftWorkspace")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &PlayPause, _, cx| this.toggle_playback(cx)))
            .on_action(cx.listener(|this, _: &StopTransport, _, cx| this.stop_playback(cx)))
            .on_action(cx.listener(|this, _: &UndoEdit, _, cx| this.perform_undo(cx)))
            .on_action(cx.listener(|this, _: &RedoEdit, _, cx| this.perform_redo(cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_timeline(0.5, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_timeline(2.0, cx)))
            .on_action(cx.listener(|this, _: &FitView, _, cx| this.fit_timeline(cx)))
            .on_action(cx.listener(|this, _: &SelectOriginalTake, _, cx| {
                this.select_take(DisplayTake::Original, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectDryTake, _, cx| {
                this.select_take(DisplayTake::Dry, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectCaptureTake, _, cx| {
                this.select_take(DisplayTake::Capture, cx);
            }))
            .on_action(cx.listener(|_this, _: &QuitApp, _, cx| {
                cx.quit();
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x16161a))
            .text_color(rgb(0xeeeef3))
            .child(header);

        if let Some(err_el) = error_strip {
            root = root.child(err_el);
        }

        root.child(
            div()
                .flex_1()
                .flex()
                .flex_row()
                .size_full()
                .child(workspace)
                .child(render_inspector(self, cx)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use spectracraft_mono::app_jobs::{CancellationToken, JobRequest, execute_job};
    use spectracraft_mono::app_session::load_validated_wav_artifact;
    use std::f64::consts::TAU;

    fn temp_wav(tag: &str, freq_hz: f64) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "spectracraft-gpui-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_take.wav");
        let rate = 48_000_u32;
        let samples: Vec<f32> = (0..(rate / 2))
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let env = (t / 0.01).min(1.0) * (-2.0 * t).exp();
                (env * 0.5 * (TAU * freq_hz * t).sin()) as f32
            })
            .collect();
        spectracraft_mono::write_wav(rate, &samples, &path).unwrap();
        path
    }

    #[gpui::test]
    fn gpui_view_handles_stage_toggles_undo_redo_and_display_isolation(cx: &mut TestAppContext) {
        let wav = temp_wav("gpui_view", 220.0);
        let view = cx.new(SpectraCraftGpuiApp::new);

        view.update(cx, |app, cx| {
            let id = app.session.allocate_artifact_id();
            let (art, name) = load_validated_wav_artifact(&wav, id).unwrap();
            app.session.load_source_artifact(wav.clone(), name, art);

            // Deterministic synchronous Stage 1 dry execution via shared JobRequest
            let spec = app.session.begin_dry_request().unwrap();
            let JobOutcome::DryReady {
                guard,
                performance,
                samples,
            } = execute_job(JobRequest::DryRegen {
                spec,
                cancel: CancellationToken::new(),
            })
            .unwrap()
            else {
                panic!("expected DryReady");
            };
            let dry = app
                .session
                .publish_dry_result(&guard, performance, samples)
                .unwrap();

            // Stage toggles mark capture stale & undo restores previous state cleanly
            app.toggle_mic_stage(cx);
            assert!(app.session.mic_enabled);
            app.step_preamp_drive(0.2, cx);
            assert!((app.session.preamp_drive - 0.2).abs() < 1e-6);

            // Display zoom/scale changes never alter dry SHA or session epoch
            let epoch = app.session.epoch();
            let dry_sha = dry.sha256.clone();
            app.zoom_timeline(0.5, cx);
            app.toggle_scale(cx);
            assert_eq!(app.session.epoch(), epoch);
            assert_eq!(app.session.dry.as_ref().unwrap().sha256, dry_sha);

            // Undo & Redo work cleanly via GPUI handlers
            app.perform_undo(cx);
            assert!((app.session.preamp_drive - 0.0).abs() < 1e-6);
            app.perform_redo(cx);
            assert!((app.session.preamp_drive - 0.2).abs() < 1e-6);
        });

        let _ = std::fs::remove_dir_all(wav.parent().unwrap());
    }

    #[gpui::test]
    fn gpui_end_to_end_reference_blind_report_and_daw_export(cx: &mut TestAppContext) {
        let out_dir = std::env::temp_dir().join(format!(
            "spectracraft-gpui-e2e-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&out_dir);
        let view = cx.new(SpectraCraftGpuiApp::new);

        view.update(cx, |app, cx| {
            app.load_builtin_reference(cx);
            assert!(app.session.source.is_some());

            // Stage 1 dry regeneration
            let spec = app.session.begin_dry_request().unwrap();
            let JobOutcome::DryReady {
                guard,
                performance,
                samples,
            } = execute_job(JobRequest::DryRegen {
                spec,
                cancel: CancellationToken::new(),
            })
            .unwrap()
            else {
                panic!("expected DryReady");
            };
            app.session
                .publish_dry_result(&guard, performance, samples)
                .unwrap();

            // Export Blind Technical Report (PDF via craft-pdf) and Universal DAW bundle
            let report_pdf = out_dir.join("gpui_report.pdf");
            app.export_technical_report_to_path(&report_pdf, cx);
            assert!(app.error_banner.is_none(), "unexpected error: {:?}", app.error_banner);
            let pdf_bytes = std::fs::read(&report_pdf).expect("read exported GPUI PDF report");
            assert!(pdf_bytes.starts_with(b"%PDF-1.7\n"));

            let daw_dir = out_dir.join("daw_bundle");
            app.export_daw_bundle_to_dir(&daw_dir, cx);
            assert!(app.error_banner.is_none(), "unexpected error: {:?}", app.error_banner);
            assert!(daw_dir.exists());

            // Platform capability descriptor covers desktop GPUI + mobile Tauri matrix
            let caps = platform::current_capabilities();
            assert!(!caps.os.is_empty());
            assert_eq!(platform::supported_platform_matrix().len(), 5);
        });

        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
