//! Full native Tauri 2 desktop and mobile shell over the canonical `spectracraft-mono` Rust core.
//!
//! Audit H1, H5, H13, H14, H15, M10, M15:
//! - Executes the entire SpectraCraft workstation workflow natively via Tauri IPC with ZERO Node/Express bridge.
//! - Keeps all audio DSP, Stage 1 physics, blind Before/After reporting, settings persistence,
//!   and DAW interchange strictly inside `spectracraft-mono`.
//! - Enforces a least-privilege capability boundary via `ScopedHandleRegistry`: the WebView only
//!   receives opaque `tauri-media://<id>` handles confined to the app-owned session workspace and
//!   cannot read arbitrary host filesystem paths.
//! - Rejects unknown/evicted run IDs explicitly without silent baseline fallback (H5).
//! - Validates `StudioRig` inputs before DSP execution (H13) and never fabricates default engine
//!   state in DAW bundles (M10).

use serde::{Deserialize, Serialize};
use spectracraft_mono::{
    acoustic_math,
    capture::DryReviewStatus,
    daw_bridge::{
        export_for_editor, markers_from_performance, EditorTarget, ExportBitDepth,
        InterchangeExportRequest, StemTrack,
    },
    digest, edge, gap,
    pipeline::{
        run_post_dry_pipeline, write_wav_f32_atomic_with_hash, PluginSlotConfig,
        PostDryPipelineConfig,
    },
    platform, qa, reference_profiles,
    region_edit::{apply_dry_region_edit, DryRegionEdit},
    reperform::{measure_spectral_summary, Performance, VoicingControls},
    report_export,
    settings_store::{SettingsSnapshot, SpectraCraftSettings},
    shape_profile, studio_catalog, technical_report, update_manager, workspace,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::State;

const MAX_IPC_TITLE_LEN: usize = 160;
const MAX_UPLOAD_WAV_BYTES: usize = 64 * 1024 * 1024;
const MAX_SESSIONS: usize = 8;
const MAX_RUNS_PER_SESSION: usize = 24;
const MAX_SCOPED_HANDLES: usize = 256;
const MAX_CANCELLED_JOBS: usize = 256;

fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn write_private_file(path: &Path, bytes: &[u8], read_only: bool) -> std::io::Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if read_only { 0o400 } else { 0o600 };
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = read_only;
    Ok(())
}

fn sanitize_tauri_error(raw: impl std::fmt::Display) -> String {
    let mut msg = raw.to_string();
    let temp_str = platform::temp_dir().to_string_lossy().into_owned();
    if !temp_str.is_empty() {
        msg = msg.replace(&temp_str, "<workspace>");
    }
    if let Ok(cwd) = std::env::current_dir() {
        let cwd_str = cwd.to_string_lossy().into_owned();
        if !cwd_str.is_empty() {
            msg = msg.replace(&cwd_str, "<app>");
        }
    }
    msg.chars().filter(|c| !c.is_control() || *c == '\n').take(320).collect()
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_uid(prefix: &str) -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{prefix}-{ts:x}-{n}")
}

/// Structured IPC error returned to the frontend transport (Audit H1, M15).
#[derive(Clone, Debug, Serialize)]
pub struct TauriError {
    pub code: String,
    pub message: String,
}

impl TauriError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    fn invalid(msg: impl Into<String>) -> Self {
        Self::new("INVALID_ARGUMENT", msg)
    }

    fn not_found(msg: impl Into<String>) -> Self {
        Self::new("ARTIFACT_NOT_FOUND", msg)
    }

    fn expired(msg: impl Into<String>) -> Self {
        Self::new("ARTIFACT_EXPIRED", msg)
    }

    fn internal(msg: impl Into<String>) -> Self {
        Self::new("ENGINE_ERROR", sanitize_tauri_error(msg.into()))
    }
}

impl std::fmt::Display for TauriError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

/// Confined opaque media handle registry (Audit M15).
/// Maps `tauri-media://<id>` strictly to canonicalized files under `workspace_root`.
pub struct ScopedHandleRegistry {
    workspace_root: PathBuf,
    handles: HashMap<String, PathBuf>,
    order: VecDeque<String>,
}

impl ScopedHandleRegistry {
    pub fn new(workspace_root: PathBuf) -> Self {
        let _ = ensure_private_dir(&workspace_root);
        let canonical = std::fs::canonicalize(&workspace_root).unwrap_or(workspace_root);
        Self {
            workspace_root: canonical,
            handles: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Register a file strictly inside `workspace_root` and return an opaque `tauri-media://<id>` handle.
    pub fn register(&mut self, file_path: &Path) -> Result<String, TauriError> {
        let canonical = std::fs::canonicalize(file_path)
            .map_err(|_| TauriError::not_found("Artifact file does not exist or is unreadable."))?;
        if !canonical.starts_with(&self.workspace_root) {
            return Err(TauriError::new(
                "PATH_OUT_OF_SCOPE",
                "Refused to register media handle outside the app-owned session workspace.",
            ));
        }
        if !canonical.is_file() {
            return Err(TauriError::invalid("Media handle must reference a regular file."));
        }
        let key = format!("tauri-media://{}", next_uid("m"));
        self.handles.insert(key.clone(), canonical);
        self.order.push_back(key.clone());
        while self.order.len() > MAX_SCOPED_HANDLES {
            if let Some(old) = self.order.pop_front() {
                self.handles.remove(&old);
            }
        }
        Ok(key)
    }

    /// Resolve ONLY opaque `tauri-media://<id>` handles. Any raw host filesystem path is rejected.
    pub fn resolve(&self, handle: &str) -> Result<PathBuf, TauriError> {
        let clean = handle.trim().split('?').next().unwrap_or("").trim();
        if !clean.starts_with("tauri-media://") {
            return Err(TauriError::new(
                "FORBIDDEN_PATH",
                "Direct host filesystem paths are forbidden; use an opaque tauri-media:// handle.",
            ));
        }
        let path = self
            .handles
            .get(clean)
            .ok_or_else(|| TauriError::not_found("Requested media handle is unknown or expired."))?;
        let canonical = std::fs::canonicalize(path)
            .map_err(|_| TauriError::not_found("Requested media artifact is no longer available."))?;
        if !canonical.starts_with(&self.workspace_root) {
            return Err(TauriError::new(
                "PATH_OUT_OF_SCOPE",
                "Resolved media handle escaped app-owned session workspace.",
            ));
        }
        Ok(canonical)
    }
}

struct TauriSessionRecord {
    id: String,
    label: String,
    is_reference: bool,
    source_wav: PathBuf,
    verify_dir: PathBuf,
    work_dir: PathBuf,
    dry_wav: PathBuf,
    cues_json: PathBuf,
    cues_runs: HashMap<String, (PathBuf, PathBuf)>, // run_id -> (dry_wav, cues_json)
    cues_order: VecDeque<String>,
    evicted_cues_runs: HashSet<String>,
    pipeline_runs: HashMap<String, PathBuf>,
    pipeline_order: VecDeque<String>,
    evicted_pipeline_runs: HashSet<String>,
    report_runs: HashMap<String, PathBuf>,
    report_exports: HashMap<String, String>, // "reportId:format" -> opaque handle
}

impl TauriSessionRecord {
    fn record_cues_run(&mut self, run_id: String, dry_wav: PathBuf, cues_json: PathBuf) {
        self.cues_runs.insert(run_id.clone(), (dry_wav, cues_json));
        self.cues_order.push_back(run_id);
        while self.cues_order.len() > MAX_RUNS_PER_SESSION {
            if let Some(evicted) = self.cues_order.pop_front() {
                self.cues_runs.remove(&evicted);
                self.evicted_cues_runs.insert(evicted);
            }
        }
    }

    fn record_pipeline_run(&mut self, run_id: String, wav_path: PathBuf) {
        self.pipeline_runs.insert(run_id.clone(), wav_path);
        self.pipeline_order.push_back(run_id);
        while self.pipeline_order.len() > MAX_RUNS_PER_SESSION {
            if let Some(evicted) = self.pipeline_order.pop_front() {
                self.pipeline_runs.remove(&evicted);
                self.evicted_pipeline_runs.insert(evicted);
            }
        }
    }

    fn resolve_cues_run(&self, cues_run_id: Option<&str>) -> Result<(PathBuf, PathBuf), TauriError> {
        let clean = cues_run_id.unwrap_or("").trim();
        if clean.is_empty() {
            return Ok((self.dry_wav.clone(), self.cues_json.clone()));
        }
        if let Some(pair) = self.cues_runs.get(clean) {
            return Ok(pair.clone());
        }
        if self.evicted_cues_runs.contains(clean) {
            return Err(TauriError::expired(format!(
                "Requested dry revision '{clean}' has expired and been evicted. Re-run dry regeneration."
            )));
        }
        Err(TauriError::not_found(format!(
            "Requested dry revision '{clean}' was not found in this session."
        )))
    }

    fn resolve_pipeline_run(&self, pipeline_run_id: Option<&str>) -> Result<Option<PathBuf>, TauriError> {
        let clean = pipeline_run_id.unwrap_or("").trim();
        if clean.is_empty() {
            return Ok(None);
        }
        if let Some(p) = self.pipeline_runs.get(clean) {
            return Ok(Some(p.clone()));
        }
        if self.evicted_pipeline_runs.contains(clean) {
            return Err(TauriError::expired(format!(
                "Requested pipeline revision '{clean}' has expired and been evicted. Re-render Stages 3-5."
            )));
        }
        Err(TauriError::not_found(format!(
            "Requested pipeline revision '{clean}' was not found in this session."
        )))
    }
}

pub struct TauriAppState {
    registry: ScopedHandleRegistry,
    sessions: HashMap<String, TauriSessionRecord>,
    session_order: VecDeque<String>,
    cancelled_jobs: HashSet<String>,
    cancelled_order: VecDeque<String>,
}

impl TauriAppState {
    fn new() -> Self {
        let root = platform::temp_dir().join(format!("spectracraft-tauri-{}", std::process::id()));
        Self {
            registry: ScopedHandleRegistry::new(root),
            sessions: HashMap::new(),
            session_order: VecDeque::new(),
            cancelled_jobs: HashSet::new(),
            cancelled_order: VecDeque::new(),
        }
    }

    fn record_cancelled_job(&mut self, job_id: String) {
        if self.cancelled_jobs.insert(job_id.clone()) {
            self.cancelled_order.push_back(job_id);
            while self.cancelled_order.len() > MAX_CANCELLED_JOBS {
                if let Some(old) = self.cancelled_order.pop_front() {
                    self.cancelled_jobs.remove(&old);
                }
            }
        }
    }

    fn insert_session(&mut self, rec: TauriSessionRecord) {
        let id = rec.id.clone();
        self.sessions.insert(id.clone(), rec);
        self.session_order.retain(|x| x != &id);
        self.session_order.push_back(id);
        while self.session_order.len() > MAX_SESSIONS {
            if let Some(oldest) = self.session_order.pop_front() {
                if let Some(removed) = self.sessions.remove(&oldest) {
                    if !removed.is_reference {
                        let _ = std::fs::remove_dir_all(&removed.work_dir);
                    }
                }
            }
        }
    }
}

pub struct ManagedAppState(pub Mutex<TauriAppState>);

fn sanitize_ipc_title(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_IPC_TITLE_LEN)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "SpectraCraft Session".to_string()
    } else {
        trimmed.to_string()
    }
}

fn sanitize_display_filename(raw: &str) -> String {
    let base = Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("uploaded-audio.wav");
    let clean: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        .take(120)
        .collect();
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        "uploaded-audio.wav".to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_pattern(s: &str) -> Option<studio_catalog::Pattern> {
    match s.to_ascii_lowercase().as_str() {
        "omni" => Some(studio_catalog::Pattern::Omni),
        "wide_cardioid" | "wide" => Some(studio_catalog::Pattern::WideCardioid),
        "cardioid" => Some(studio_catalog::Pattern::Cardioid),
        "supercardioid" | "super" => Some(studio_catalog::Pattern::Supercardioid),
        "hypercardioid" | "hyper" => Some(studio_catalog::Pattern::Hypercardioid),
        "figure8" | "figure_8" | "figure-8" => Some(studio_catalog::Pattern::Figure8),
        _ => None,
    }
}

fn pattern_key(p: studio_catalog::Pattern) -> &'static str {
    match p {
        studio_catalog::Pattern::Omni => "omni",
        studio_catalog::Pattern::WideCardioid => "wide_cardioid",
        studio_catalog::Pattern::Cardioid => "cardioid",
        studio_catalog::Pattern::Supercardioid => "supercardioid",
        studio_catalog::Pattern::Hypercardioid => "hypercardioid",
        studio_catalog::Pattern::Figure8 => "figure8",
    }
}

fn room_preset_key(p: studio_catalog::RoomPreset) -> &'static str {
    match p {
        studio_catalog::RoomPreset::Small => "small",
        studio_catalog::RoomPreset::Medium => "medium",
        studio_catalog::RoomPreset::Large => "large",
        studio_catalog::RoomPreset::Open => "open",
        studio_catalog::RoomPreset::Direct => "direct",
    }
}

fn parse_room_preset(s: &str) -> studio_catalog::RoomPreset {
    match s.to_ascii_lowercase().as_str() {
        "medium" => studio_catalog::RoomPreset::Medium,
        "large" => studio_catalog::RoomPreset::Large,
        "open" | "open_field" => studio_catalog::RoomPreset::Open,
        "direct" => studio_catalog::RoomPreset::Direct,
        _ => studio_catalog::RoomPreset::Small,
    }
}

fn source_symbol_key(sym: studio_catalog::SourceSymbol) -> &'static str {
    match sym {
        studio_catalog::SourceSymbol::Unknown => "unknown",
        studio_catalog::SourceSymbol::Guitar => "guitar",
        studio_catalog::SourceSymbol::Amp => "amp",
        studio_catalog::SourceSymbol::Bowed => "bowed",
        studio_catalog::SourceSymbol::Wind => "wind",
        studio_catalog::SourceSymbol::Brass => "brass",
        studio_catalog::SourceSymbol::Percussion => "percussion",
        studio_catalog::SourceSymbol::Keyboard => "keyboard",
        studio_catalog::SourceSymbol::Voice => "voice",
        studio_catalog::SourceSymbol::Ensemble => "ensemble",
    }
}

pub fn parse_studio_rig_config(
    cfg: &serde_json::Value,
    sources: &[studio_catalog::SourceEntry],
    mics: &[studio_catalog::MicEntry],
    surfaces: &[studio_catalog::SurfaceEntry],
) -> Result<studio_catalog::StudioRig, TauriError> {
    let mut rig = studio_catalog::StudioRig::default();
    if let Some(p) = cfg.get("room_preset").and_then(|v| v.as_str()) {
        rig.apply_preset(parse_room_preset(p));
    }
    if let Some(dims) = cfg.get("dimensions_m").and_then(|v| v.as_array()) {
        if dims.len() != 3 {
            return Err(TauriError::invalid("dimensions_m must contain exactly 3 finite numbers"));
        }
        rig.dimensions_m = [
            dims[0].as_f64().ok_or_else(|| TauriError::invalid("dimensions_m[0] must be a number"))? as f32,
            dims[1].as_f64().ok_or_else(|| TauriError::invalid("dimensions_m[1] must be a number"))? as f32,
            dims[2].as_f64().ok_or_else(|| TauriError::invalid("dimensions_m[2] must be a number"))? as f32,
        ];
    }
    if let Some(idx) = cfg.get("source_index").and_then(|v| v.as_u64()) {
        rig.source_index = idx as usize;
    }
    if let Some(off) = cfg.get("source_offset_m").and_then(|v| v.as_array()) {
        if off.len() != 3 {
            return Err(TauriError::invalid("source_offset_m must contain exactly 3 finite numbers"));
        }
        rig.source_offset_m = [
            off[0].as_f64().ok_or_else(|| TauriError::invalid("source_offset_m[0] must be a number"))? as f32,
            off[1].as_f64().ok_or_else(|| TauriError::invalid("source_offset_m[1] must be a number"))? as f32,
            off[2].as_f64().ok_or_else(|| TauriError::invalid("source_offset_m[2] must be a number"))? as f32,
        ];
    }
    if let Some(v) = cfg.get("source_lock_center_xy").and_then(|v| v.as_bool()) {
        rig.source_lock_center_xy = v;
    }
    if let Some(v) = cfg.get("source_lock_height_5ft").and_then(|v| v.as_bool()) {
        rig.source_lock_height_5ft = v;
    }
    if let Some(faces) = cfg.get("surface_faces").and_then(|v| v.as_array()) {
        if faces.len() != 6 {
            return Err(TauriError::invalid("surface_faces must contain exactly 6 surface indices"));
        }
        for (i, f) in faces.iter().enumerate() {
            let sidx = f
                .as_u64()
                .ok_or_else(|| TauriError::invalid("surface_faces entries must be non-negative integers"))?;
            rig.surface_faces[i] = sidx as usize;
        }
    }
    if let Some(t) = cfg.get("temperature_c").and_then(|v| v.as_f64()) {
        rig.temperature_c = t as f32;
    }
    if let Some(mic_arr) = cfg.get("mics").and_then(|v| v.as_array()) {
        if mic_arr.is_empty() {
            return Err(TauriError::invalid("StudioRig.mics cannot be empty (at least 1 mic slot is required)"));
        }
        if mic_arr.len() > studio_catalog::StudioRig::MAX_MIC_SLOTS {
            return Err(TauriError::invalid(format!(
                "StudioRig.mics exceeds maximum {} slots",
                studio_catalog::StudioRig::MAX_MIC_SLOTS
            )));
        }
        let mut parsed = Vec::with_capacity(mic_arr.len());
        for (mi, mv) in mic_arr.iter().enumerate() {
            let mut slot = studio_catalog::MicSlot::sm7_baseline();
            slot.label = mv
                .get("label")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Mic {}", mi + 1));
            if let Some(ci) = mv.get("catalog_index").and_then(|v| v.as_u64()) {
                slot.catalog_index = ci as usize;
            }
            if let Some(ps) = mv.get("pattern_override").and_then(|v| v.as_str()) {
                slot.pattern_override = parse_pattern(ps);
            }
            if let Some(off) = mv.get("offset_m").and_then(|v| v.as_array()) {
                if off.len() != 3 {
                    return Err(TauriError::invalid("mic offset_m must contain exactly 3 finite numbers"));
                }
                slot.offset_m = [
                    off[0].as_f64().ok_or_else(|| TauriError::invalid("mic offset_m[0] must be a number"))? as f32,
                    off[1].as_f64().ok_or_else(|| TauriError::invalid("mic offset_m[1] must be a number"))? as f32,
                    off[2].as_f64().ok_or_else(|| TauriError::invalid("mic offset_m[2] must be a number"))? as f32,
                ];
            }
            if let Some(aim) = mv.get("aim") {
                slot.aim.tilt_x_deg = aim.get("tilt_x_deg").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                slot.aim.roll_y_deg = aim.get("roll_y_deg").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                slot.aim.heading_z_deg = aim.get("heading_z_deg").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                slot.aim.follow_source = aim.get("follow_source").and_then(|v| v.as_bool()).unwrap_or(false);
            }
            if let Some(en) = mv.get("enabled").and_then(|v| v.as_bool()) {
                slot.enabled = en;
            }
            parsed.push(slot);
        }
        rig.mics = parsed;
    }
    if let Some(sel) = cfg.get("selected_mic").and_then(|v| v.as_u64()) {
        rig.selected_mic = sel as usize;
    }
    rig.validate(sources.len(), mics.len(), surfaces.len())
        .map_err(|e| TauriError::invalid(e.to_string()))?;
    rig.clamp_all();
    Ok(rig)
}

fn build_session_payload(
    rec: &TauriSessionRecord,
    verification: serde_json::Value,
    cues: serde_json::Value,
    registry: &mut ScopedHandleRegistry,
) -> Result<serde_json::Value, TauriError> {
    let source_handle = registry.register(&rec.source_wav)?;
    let dry_handle = registry.register(&rec.dry_wav)?;
    let cues_handle = registry.register(&rec.cues_json)?;
    Ok(serde_json::json!({
        "id": rec.id,
        "label": rec.label,
        "isReference": rec.is_reference,
        "verification": verification,
        "cues": cues,
        "urls": {
            "sourceWav": source_handle,
            "dryWav": dry_handle,
            "cuesJson": cues_handle,
        }
    }))
}

#[derive(Serialize)]
struct PlatformInfo {
    os: &'static str,
    arch: &'static str,
    mobile: bool,
    app_data_dir: String,
    temp_dir: String,
}

#[derive(Serialize)]
struct DawTargetInfo {
    id: &'static str,
    name: &'static str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NoteEditPatch {
    index: usize,
    gain_scale: Option<f64>,
    cents_offset: Option<f64>,
    duration_scale: Option<f64>,
    muted: Option<bool>,
    repick: Option<RepickPatch>,
}

#[derive(Deserialize)]
struct RepickPatch {
    offset_s: f64,
    strength: f64,
}

#[tauri::command]
fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        mobile: cfg!(any(target_os = "ios", target_os = "android")),
        app_data_dir: platform::app_data_dir().display().to_string(),
        temp_dir: platform::temp_dir().display().to_string(),
    }
}

#[tauri::command]
fn supported_daw_targets() -> Vec<DawTargetInfo> {
    EditorTarget::ALL
        .iter()
        .map(|t| DawTargetInfo {
            id: t.id(),
            name: t.display_name(),
        })
        .collect()
}

#[tauri::command(rename_all = "camelCase")]
fn get_studio_catalogs(_reload: Option<bool>) -> Result<serde_json::Value, TauriError> {
    let source_list = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let source_count = source_list.len();
    let sources: Vec<serde_json::Value> = source_list
        .into_iter()
        .enumerate()
        .map(|(index, s)| {
            serde_json::json!({
                "index": index,
                "id": s.id,
                "name": s.name,
                "category": s.category.label(),
                "family": s.family,
                "symbol": source_symbol_key(s.symbol),
                "published_range_hz": s.published_range_hz,
                "published_fundamental_notes": s.published_fundamental_notes,
                "published_practical_range_hz": s.published_practical_range_hz,
                "spectral_regions": {
                    "body_warmth_hz": s.spectral_regions.body_warmth_hz,
                    "presence_attack_hz": s.spectral_regions.presence_attack_hz,
                    "air_high_hz": s.spectral_regions.air_high_hz,
                    "formants_and_resonances_note": s.spectral_regions.formants_and_resonances_note,
                },
                "published_notes": s.published_notes,
                "source_references": s.source_references,
                "uncertainties": s.uncertainties,
                "radiation_label": s.radiation.label(),
                "radiation_alpha": s.radiation.directivity_alpha(),
                "synthesis_model_hint": s.synthesis_model_hint,
                "regen_model": {
                    "model_id": s.regen_model.model_id,
                    "model_label": s.regen_model.model_label,
                    "description": s.regen_model.description,
                    "physics_constraint": {
                        "attack_ms": s.regen_model.family_kind.physics_constraint().attack_ms,
                        "release_ms": s.regen_model.family_kind.physics_constraint().release_ms,
                        "breath_texture_amount": s.regen_model.family_kind.physics_constraint().breath_texture_amount,
                        "dynamic_brightness_amount": s.regen_model.family_kind.physics_constraint().dynamic_brightness_amount,
                        "harmonic_ceiling_hz": s.regen_model.family_kind.physics_constraint().harmonic_ceiling_hz,
                        "resonance_contour_summary": s.regen_model.family_kind.physics_constraint().resonance_contour_summary,
                        "texture_mechanism": s.regen_model.family_kind.physics_constraint().texture_mechanism,
                        "plausibility_note": s.regen_model.family_kind.physics_constraint().plausibility_note,
                    },
                },
            })
        })
        .collect();

    let mics: Vec<serde_json::Value> = studio_catalog::mic_catalog()
        .into_iter()
        .enumerate()
        .map(|(index, m)| {
            let shared_explanation = m.shared_model_explanation();
            let offaxis_json = m.published_offaxis.as_ref().map(|o| {
                serde_json::json!({
                    "freqs_hz": o.freqs_hz,
                    "atten_90deg_db": o.atten_90deg_db,
                    "atten_180deg_db": o.atten_180deg_db,
                })
            });
            serde_json::json!({
                "index": index,
                "id": m.id,
                "brand": m.brand,
                "model": m.model,
                "source_url": m.source_url,
                "transducer": m.transducer.label(),
                "default_pattern": pattern_key(m.pattern),
                "default_pattern_label": m.pattern.label(),
                "published_type": m.published_type,
                "published_patterns": m.published_patterns,
                "published_curves": m.published_curves,
                "published_primary_curve": m.published_primary_curve,
                "published_fr_points": m.published_fr_points,
                "published_offaxis": offaxis_json,
                "published_range_text": m.published_range_text,
                "published_range_hz": m.published_range_hz,
                "published_sensitivity_dbv": m.published_sensitivity_dbv,
                "published_self_noise_dba": m.published_self_noise_dba,
                "published_max_spl_db": m.published_max_spl_db,
                "published_impedance_ohm": m.published_impedance_ohm,
                "published_released_year": m.published_released_year,
                "published_measurement_dist_cm": m.published_measurement_dist_cm,
                "published_power": m.published_power,
                "source_document": m.source_document,
                "uncertainties": m.uncertainties,
                "renderer_profile_id": m.renderer_profile.id(),
                "renderer_profile_description": m.renderer_profile.description(),
                "curve_provenance": m.curve_provenance,
                "published_notes": m.published_notes,
                "shared_model_explanation": shared_explanation,
            })
        })
        .collect();

    let surfaces: Vec<serde_json::Value> = studio_catalog::surface_catalog()
        .into_iter()
        .enumerate()
        .map(|(index, s)| {
            serde_json::json!({
                "index": index,
                "id": s.id,
                "name": s.name,
                "category": s.category,
                "material_type": s.material_type,
                "thickness_mm": s.thickness_mm,
                "assembly": s.assembly,
                "incidence": s.incidence,
                "standard": s.standard,
                "response_kind": s.response_kind,
                "raw_absorption_6band": s.absorption_6band,
                "effective_absorption_6band": s.effective_absorption(),
                "pressure_reflectance_6band": s.pressure_reflectance(),
                "mean_reflectance": s.mean_reflectance(),
                "biot": {
                    "porosity": s.biot.porosity,
                    "airflow_resistivity_pa_s_m2": s.biot.airflow_resistivity_pa_s_m2,
                    "tortuosity": s.biot.tortuosity,
                    "vcl_um": s.biot.vcl_um,
                    "tcl_um": s.biot.tcl_um,
                    "standard_ref": s.biot.standard_ref,
                    "status": s.biot.status,
                },
                "provenance": s.provenance,
                "notes": s.notes,
                "uncertainties": s.uncertainties,
            })
        })
        .collect();

    let presets: Vec<serde_json::Value> = studio_catalog::RoomPreset::ALL
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "key": room_preset_key(p),
                "label": p.label(),
                "dimensions_m": p.dimensions_m(),
                "default_surface_faces": p.default_surface_faces(),
            })
        })
        .collect();

    let patterns: Vec<serde_json::Value> = studio_catalog::Pattern::ALL
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "key": pattern_key(p),
                "label": p.label(),
                "alpha": p.alpha(),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "bands_hz": studio_catalog::BANDS_HZ,
        "face_labels": studio_catalog::FACE_LABELS,
        "sources": sources,
        "mics": mics,
        "surfaces": surfaces,
        "room_presets": presets,
        "patterns": patterns,
        "loose_profiles": reference_profiles::loose_profile_library(),
        "catalog_provenance": {
            "engine": "spectracraft-mono::studio_catalog (Native Tauri 2 Rust IPC)",
            "source_count": source_count,
            "mic_count": studio_catalog::mic_catalog().len(),
            "surface_count": studio_catalog::surface_catalog().len(),
        }
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn analyze_studio_rig(rig_config: serde_json::Value) -> Result<serde_json::Value, TauriError> {
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let mics = studio_catalog::mic_catalog();
    let surfaces = studio_catalog::surface_catalog();
    let rig = parse_studio_rig_config(&rig_config, &sources, &mics, &surfaces)?;
    let analysis = acoustic_math::analyze_studio_rig(&rig, &sources, &mics, &surfaces);
    serde_json::to_value(analysis).map_err(|e| TauriError::internal(e.to_string()))
}

#[tauri::command]
fn load_reference_session(state: State<'_, ManagedAppState>) -> Result<serde_json::Value, TauriError> {
    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let session_id = "slow-solo-ref".to_string();
    let work_dir = guard.registry.workspace_root().join(&session_id);
    let verify_dir = work_dir.join("verify");
    let source_wav = work_dir.join("reference-six-note.wav");
    let dry_wav = verify_dir.join("dry.wav");
    let cues_json = verify_dir.join("cues.json");

    if !source_wav.exists() {
        std::fs::create_dir_all(&work_dir).map_err(|e| TauriError::internal(e.to_string()))?;
        let (samples, _) = qa::six_note();
        qa::write_wav_new_i16(48_000, &samples, &source_wav)
            .map_err(|e| TauriError::internal(e.to_string()))?;
    }
    if !dry_wav.exists() || !cues_json.exists() {
        let _ = std::fs::remove_dir_all(&verify_dir);
        qa::guitar_verify(&source_wav, &verify_dir)
            .map_err(|e| TauriError::internal(e.to_string()))?;
    }

    let verification: serde_json::Value = serde_json::from_reader(
        std::fs::File::open(verify_dir.join("verification.json"))
            .map_err(|e| TauriError::internal(e.to_string()))?,
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;
    let cues: serde_json::Value = serde_json::from_reader(
        std::fs::File::open(&cues_json).map_err(|e| TauriError::internal(e.to_string()))?,
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;

    let rec = TauriSessionRecord {
        id: session_id,
        label: "Built-in Six-Note Reference (48 kHz mono)".to_string(),
        is_reference: true,
        source_wav,
        verify_dir,
        work_dir,
        dry_wav,
        cues_json,
        cues_runs: HashMap::new(),
        cues_order: VecDeque::new(),
        evicted_cues_runs: HashSet::new(),
        pipeline_runs: HashMap::new(),
        pipeline_order: VecDeque::new(),
        evicted_pipeline_runs: HashSet::new(),
        report_runs: HashMap::new(),
        report_exports: HashMap::new(),
    };
    let payload = build_session_payload(&rec, verification, cues, &mut guard.registry)?;
    guard.insert_session(rec);
    Ok(payload)
}

#[tauri::command(rename_all = "camelCase")]
fn import_wav(
    file_name: String,
    wav_bytes: Vec<u8>,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    if wav_bytes.len() < 44 || wav_bytes.len() > MAX_UPLOAD_WAV_BYTES {
        return Err(TauriError::invalid(
            "WAV payload must be between 44 bytes and 64 MiB.",
        ));
    }
    if &wav_bytes[0..4] != b"RIFF" || &wav_bytes[8..12] != b"WAVE" {
        return Err(TauriError::invalid(
            "Uploaded file is not a valid RIFF/WAVE audio file.",
        ));
    }
    let display_name = sanitize_display_filename(&file_name);
    let session_id = next_uid("upload");

    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let work_dir = guard.registry.workspace_root().join(&session_id);
    let verify_dir = work_dir.join("verify");
    let source_wav = work_dir.join("source.wav");
    ensure_private_dir(&work_dir).map_err(|e| TauriError::internal(e.to_string()))?;
    write_private_file(&source_wav, &wav_bytes, true)
        .map_err(|e| TauriError::internal(e.to_string()))?;

    if let Err(err) = qa::guitar_verify(&source_wav, &verify_dir) {
        let _ = std::fs::remove_dir_all(&work_dir);
        return Err(TauriError::invalid(format!(
            "Rust analysis rejected this WAV: {}",
            sanitize_tauri_error(err)
        )));
    }

    let dry_wav = verify_dir.join("dry.wav");
    let cues_json = verify_dir.join("cues.json");
    let verification: serde_json::Value = serde_json::from_reader(
        std::fs::File::open(verify_dir.join("verification.json"))
            .map_err(|e| TauriError::internal(e.to_string()))?,
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;
    let cues: serde_json::Value = serde_json::from_reader(
        std::fs::File::open(&cues_json).map_err(|e| TauriError::internal(e.to_string()))?,
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;

    let rec = TauriSessionRecord {
        id: session_id,
        label: display_name,
        is_reference: false,
        source_wav,
        verify_dir,
        work_dir,
        dry_wav,
        cues_json,
        cues_runs: HashMap::new(),
        cues_order: VecDeque::new(),
        evicted_cues_runs: HashSet::new(),
        pipeline_runs: HashMap::new(),
        pipeline_order: VecDeque::new(),
        evicted_pipeline_runs: HashSet::new(),
        report_runs: HashMap::new(),
        report_exports: HashMap::new(),
    };
    let payload = build_session_payload(&rec, verification, cues, &mut guard.registry)?;
    guard.insert_session(rec);
    Ok(payload)
}

#[tauri::command(rename_all = "camelCase")]
fn get_reference_shape(
    session_id: String,
    source_index: usize,
    cues_run_id: Option<String>,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    let guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let rec = guard
        .sessions
        .get(&session_id)
        .ok_or_else(|| TauriError::not_found("Session not found"))?;
    let (_dry, cues_path) = rec.resolve_cues_run(cues_run_id.as_deref())?;
    let (spec, samples) =
        qa::read_audio(&rec.source_wav).map_err(|e| TauriError::internal(e.to_string()))?;
    let perf: Performance = serde_json::from_reader(
        std::fs::File::open(&cues_path).map_err(|e| TauriError::internal(e.to_string()))?,
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let selected = sources.get(source_index);
    let shape = shape_profile::extract_reference_shape(&samples, spec.sample_rate, &perf, selected)
        .map_err(|e| TauriError::internal(e.to_string()))?;
    serde_json::to_value(shape).map_err(|e| TauriError::internal(e.to_string()))
}

#[tauri::command(rename_all = "camelCase")]
fn analyze_layers(
    session_id: String,
    start_s: f64,
    end_s: f64,
    faint_ridges: bool,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let rec = guard
        .sessions
        .get(&session_id)
        .ok_or_else(|| TauriError::not_found("Session not found"))?;
    let source_wav = rec.source_wav.clone();
    let layer_dir = rec.work_dir.join("layers").join(next_uid("lay"));
    ensure_private_dir(&layer_dir).map_err(|e| TauriError::internal(e.to_string()))?;

    let (spec, full) = qa::read_audio(&source_wav).map_err(|e| TauriError::internal(e.to_string()))?;
    if spec.channels != 1 {
        return Err(TauriError::invalid("Expected mono WAV for layer analysis"));
    }
    let rate = spec.sample_rate;
    let total_dur = full.len() as f64 / rate as f64;
    let clamped_start = start_s.clamp(0.0, total_dur);
    let clamped_end = end_s.clamp(clamped_start, (clamped_start + 29.5).min(total_dur));
    let s0 = (clamped_start * rate as f64).floor() as usize;
    let s1 = ((clamped_end * rate as f64).ceil() as usize).min(full.len());
    if s1.saturating_sub(s0) < (rate as f64 * 0.2) as usize {
        return Err(TauriError::invalid("Selected layer window must be at least 0.2 seconds."));
    }
    let slice = &full[s0..s1];
    let packet = edge::analyze(
        slice,
        rate,
        edge::Options {
            faint: faint_ridges,
            ..Default::default()
        },
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;

    let rendered = workspace::render(&packet, 734).map_err(|e| TauriError::internal(e.to_string()))?;
    let gain = rendered.headroom(&[]);

    let mut handles = serde_json::Map::new();
    for (name, stem) in ["pitch", "ring", "texture", "accent"]
        .iter()
        .zip(&rendered.stems)
    {
        let out_wav = layer_dir.join(format!("{name}.wav"));
        let scaled: Vec<f32> = stem.iter().map(|v| v * gain).collect();
        write_wav_f32_atomic_with_hash(&out_wav, rate, &scaled)
            .map_err(|e| TauriError::internal(e.to_string()))?;
        let h = guard.registry.register(&out_wav)?;
        handles.insert((*name).to_string(), serde_json::Value::String(h));
    }
    let mix: Vec<f32> = rendered
        .mix(&Default::default())
        .map_err(|e| TauriError::internal(e.to_string()))?
        .into_iter()
        .map(|v| v * gain)
        .collect();
    let mix_wav = layer_dir.join("mix.wav");
    write_wav_f32_atomic_with_hash(&mix_wav, rate, &mix)
        .map_err(|e| TauriError::internal(e.to_string()))?;
    let mix_handle = guard.registry.register(&mix_wav)?;
    handles.insert("mix".to_string(), serde_json::Value::String(mix_handle));

    Ok(serde_json::json!({
        "startS": s0 as f64 / rate as f64,
        "endS": s1 as f64 / rate as f64,
        "sampleRate": rate,
        "commonGain": gain,
        "faintRidges": faint_ridges,
        "urls": handles,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn render_cues(
    session_id: String,
    patches: Vec<NoteEditPatch>,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let (base_cues_path, work_dir) = {
        let rec = guard
            .sessions
            .get(&session_id)
            .ok_or_else(|| TauriError::not_found("Session not found"))?;
        (rec.cues_json.clone(), rec.work_dir.clone())
    };

    let mut cues_val: serde_json::Value = serde_json::from_reader(
        std::fs::File::open(&base_cues_path).map_err(|e| TauriError::internal(e.to_string()))?,
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;

    if let Some(notes) = cues_val.get_mut("notes").and_then(|v| v.as_array_mut()) {
        for patch in &patches {
            if let Some(note) = notes.get_mut(patch.index) {
                if let Some(obj) = note.as_object_mut() {
                    if let Some(gs) = patch.gain_scale {
                        obj.insert("gain_scale".into(), serde_json::json!(gs));
                    }
                    if let Some(co) = patch.cents_offset {
                        obj.insert("cents_offset".into(), serde_json::json!(co));
                    }
                    if let Some(ds) = patch.duration_scale {
                        obj.insert("duration_scale".into(), serde_json::json!(ds));
                    }
                    if let Some(m) = patch.muted {
                        obj.insert("muted".into(), serde_json::json!(m));
                    }
                    if let Some(ref rp) = patch.repick {
                        obj.insert(
                            "repick".into(),
                            serde_json::json!({ "offset_s": rp.offset_s, "strength": rp.strength }),
                        );
                    }
                }
            }
        }
    }

    let run_id = next_uid("cues");
    let run_dir = work_dir.join("cues-runs").join(&run_id);
    std::fs::create_dir_all(&run_dir).map_err(|e| TauriError::internal(e.to_string()))?;
    let edited_cues_path = run_dir.join("cues.json");
    let dry_out_path = run_dir.join("dry.wav");
    let cues_bytes = serde_json::to_vec_pretty(&cues_val).map_err(|e| TauriError::internal(e.to_string()))?;
    report_export::atomic_write_bytes(&edited_cues_path, &cues_bytes)
        .map_err(|e| TauriError::internal(e.to_string()))?;

    let perf: Performance =
        serde_json::from_slice(&cues_bytes).map_err(|e| TauriError::invalid(e.to_string()))?;
    perf.write_dry_wav(&dry_out_path)
        .map_err(|e| TauriError::internal(e.to_string()))?;
    let dry_sha = digest::sha256(
        &std::fs::read(&dry_out_path).map_err(|e| TauriError::internal(e.to_string()))?,
    );

    let dry_handle = guard.registry.register(&dry_out_path)?;
    let cues_handle = guard.registry.register(&edited_cues_path)?;
    if let Some(rec) = guard.sessions.get_mut(&session_id) {
        rec.record_cues_run(run_id.clone(), dry_out_path, edited_cues_path);
    }

    Ok(serde_json::json!({
        "runId": run_id,
        "dryWavUrl": dry_handle,
        "cuesJsonUrl": cues_handle,
        "drySha256": dry_sha,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn regenerate_source(
    session_id: String,
    source_index: usize,
    cues_run_id: Option<String>,
    voicing: Option<serde_json::Value>,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let (cues_path, work_dir) = {
        let rec = guard
            .sessions
            .get(&session_id)
            .ok_or_else(|| TauriError::not_found("Session not found"))?;
        let (_dry, cues) = rec.resolve_cues_run(cues_run_id.as_deref())?;
        (cues, rec.work_dir.clone())
    };

    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let idx = source_index.min(sources.len().saturating_sub(1));
    let source = &sources[idx];
    let perf: Performance = serde_json::from_reader(
        std::fs::File::open(&cues_path).map_err(|e| TauriError::internal(e.to_string()))?,
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;
    let voicing_ctrl: VoicingControls = voicing
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let samples = perf
        .synthesize_dry_for_source_with_voicing(source, &voicing_ctrl)
        .map_err(|e| TauriError::internal(e.to_string()))?;
    let spectral = measure_spectral_summary(&samples, perf.rate());

    let run_id = next_uid("src");
    let run_dir = work_dir.join("source-runs").join(&run_id);
    std::fs::create_dir_all(&run_dir).map_err(|e| TauriError::internal(e.to_string()))?;
    let out_wav = run_dir.join("dry-source.wav");
    let sha = write_wav_f32_atomic_with_hash(&out_wav, perf.rate(), &samples)
        .map_err(|e| TauriError::internal(e.to_string()))?
        .sha256;

    let wav_handle = guard.registry.register(&out_wav)?;
    if let Some(rec) = guard.sessions.get_mut(&session_id) {
        rec.record_cues_run(run_id.clone(), out_wav, cues_path);
    }

    Ok(serde_json::json!({
        "runId": run_id,
        "wavUrl": wav_handle,
        "sha256": sha,
        "source_index": idx,
        "source_id": source.id,
        "source_name": source.name,
        "synthesis_model_hint": source.synthesis_model_hint,
        "regen_model_id": source.regen_model.model_id,
        "regen_model_label": source.regen_model.model_label,
        "voicing": voicing_ctrl,
        "original_audio_opened": false,
        "samples": samples.len(),
        "sample_rate": perf.rate(),
        "note_count": perf.notes().len(),
        "spectral": spectral,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn dry_region_edit(
    session_id: String,
    edit: serde_json::Value,
    source_index: usize,
    voicing: Option<serde_json::Value>,
    cues_run_id: Option<String>,
    commit: bool,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let (dry_path, cues_path, work_dir) = {
        let rec = guard
            .sessions
            .get(&session_id)
            .ok_or_else(|| TauriError::not_found("Session not found"))?;
        let (dry, cues) = rec.resolve_cues_run(cues_run_id.as_deref())?;
        (dry, cues, rec.work_dir.clone())
    };

    let (spec, cached_dry) =
        qa::read_audio(&dry_path).map_err(|e| TauriError::internal(e.to_string()))?;
    let perf: Performance = serde_json::from_reader(
        std::fs::File::open(&cues_path).map_err(|e| TauriError::internal(e.to_string()))?,
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let source = &sources[source_index.min(sources.len().saturating_sub(1))];
    let voicing_ctrl: VoicingControls = voicing
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let parsed_edit: DryRegionEdit =
        serde_json::from_value(edit).map_err(|e| TauriError::invalid(e.to_string()))?;

    let res = apply_dry_region_edit(&cached_dry, &perf, source, &voicing_ctrl, &parsed_edit)
        .map_err(|e| TauriError::invalid(e.to_string()))?;

    let run_id = next_uid("reg");
    let run_dir = work_dir.join("region-runs").join(&run_id);
    std::fs::create_dir_all(&run_dir).map_err(|e| TauriError::internal(e.to_string()))?;
    let out_edited = run_dir.join("edited-dry.wav");
    let out_before = run_dir.join("before-slice.wav");
    let out_after = run_dir.join("after-slice.wav");

    let edited_sha = write_wav_f32_atomic_with_hash(&out_edited, spec.sample_rate, &res.edited_dry)
        .map_err(|e| TauriError::internal(e.to_string()))?
        .sha256;
    let before_sha = write_wav_f32_atomic_with_hash(&out_before, spec.sample_rate, &res.before_slice)
        .map_err(|e| TauriError::internal(e.to_string()))?
        .sha256;
    let after_sha = write_wav_f32_atomic_with_hash(&out_after, spec.sample_rate, &res.after_slice)
        .map_err(|e| TauriError::internal(e.to_string()))?
        .sha256;
    let spectral = measure_spectral_summary(&res.edited_dry, spec.sample_rate);

    let edited_handle = guard.registry.register(&out_edited)?;
    let before_handle = guard.registry.register(&out_before)?;
    let after_handle = guard.registry.register(&out_after)?;
    if commit {
        if let Some(rec) = guard.sessions.get_mut(&session_id) {
            rec.record_cues_run(run_id.clone(), out_edited, cues_path);
        }
    }

    Ok(serde_json::json!({
        "runId": run_id,
        "committed": commit,
        "editedWavUrl": edited_handle,
        "beforeSliceUrl": before_handle,
        "afterSliceUrl": after_handle,
        "edited_sha256": edited_sha,
        "before_slice_sha256": before_sha,
        "after_slice_sha256": after_sha,
        "start_sample": res.start_sample,
        "end_sample": res.end_sample,
        "crossfade_samples": res.crossfade_samples,
        "affected_notes": res.affected_notes,
        "effective_action": res.effective_action,
        "unresolved_note": res.unresolved_note,
        "outside_bit_identical": res.outside_bit_identical,
        "original_audio_opened": false,
        "sample_rate": spec.sample_rate,
        "samples": res.edited_dry.len(),
        "spectral": spectral,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn pipeline_render(
    session_id: String,
    mic_enabled: bool,
    room_enabled: bool,
    rig_config: serde_json::Value,
    plugin_slot: Option<serde_json::Value>,
    cues_run_id: Option<String>,
    preamp_drive: Option<f64>,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let (dry_path, work_dir) = {
        let rec = guard
            .sessions
            .get(&session_id)
            .ok_or_else(|| TauriError::not_found("Session not found"))?;
        let (dry, _cues) = rec.resolve_cues_run(cues_run_id.as_deref())?;
        (dry, rec.work_dir.clone())
    };

    let (spec, cached_dry) =
        qa::read_audio(&dry_path).map_err(|e| TauriError::internal(e.to_string()))?;
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let mics = studio_catalog::mic_catalog();
    let surfaces = studio_catalog::surface_catalog();
    let rig = parse_studio_rig_config(&rig_config, &sources, &mics, &surfaces)?;
    let parsed_slot: PluginSlotConfig = plugin_slot
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let pipe_cfg = PostDryPipelineConfig {
        mic_enabled,
        room_enabled,
        rig: rig.clone(),
        plugin_slot: parsed_slot,
        preamp_drive: preamp_drive.unwrap_or(0.0),
    };
    let out_res = run_post_dry_pipeline(
        &cached_dry,
        spec.sample_rate,
        &pipe_cfg,
        &sources,
        &mics,
        &surfaces,
    );

    let run_id = next_uid("pipe");
    let run_dir = work_dir.join("pipeline-runs").join(&run_id);
    std::fs::create_dir_all(&run_dir).map_err(|e| TauriError::internal(e.to_string()))?;
    let out_wav = run_dir.join("pipeline.wav");
    let wav_sha = write_wav_f32_atomic_with_hash(&out_wav, spec.sample_rate, &out_res.samples)
        .map_err(|e| TauriError::internal(e.to_string()))?
        .sha256;
    let spectral = measure_spectral_summary(&out_res.samples, spec.sample_rate);
    let acoustic_analysis = acoustic_math::analyze_studio_rig(&rig, &sources, &mics, &surfaces);

    let wav_handle = guard.registry.register(&out_wav)?;
    if let Some(rec) = guard.sessions.get_mut(&session_id) {
        rec.record_pipeline_run(run_id.clone(), out_wav);
    }

    Ok(serde_json::json!({
        "runId": run_id,
        "wavUrl": wav_handle,
        "wav_sha256": wav_sha,
        "samples": out_res.samples.len(),
        "sample_rate": spec.sample_rate,
        "summary": out_res.summary,
        "spectral": spectral,
        "acoustic_analysis": acoustic_analysis,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn repair_gap(
    session_id: String,
    start_s: f64,
    gap_ms: f64,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let (source_wav, work_dir) = {
        let rec = guard
            .sessions
            .get(&session_id)
            .ok_or_else(|| TauriError::not_found("Session not found"))?;
        (rec.source_wav.clone(), rec.work_dir.clone())
    };
    let (spec, samples) =
        qa::read_audio(&source_wav).map_err(|e| TauriError::internal(e.to_string()))?;
    let rate = spec.sample_rate;
    let start_frame = (start_s.max(0.0) * rate as f64).round() as usize;
    let gap_frames = (gap_ms.clamp(0.25, 4.0) * 0.001 * rate as f64).round() as usize;
    let end_frame = (start_frame + gap_frames).min(samples.len());

    let run_id = next_uid("gap");
    let run_dir = work_dir.join("gap-runs").join(&run_id);
    std::fs::create_dir_all(&run_dir).map_err(|e| TauriError::internal(e.to_string()))?;
    let out_wav = run_dir.join("repaired.wav");
    gap::repair_wav(&source_wav, &out_wav, start_frame, end_frame)
        .map_err(|e| TauriError::invalid(e.to_string()))?;

    let wav_handle = guard.registry.register(&out_wav)?;
    Ok(serde_json::json!({
        "runId": run_id,
        "repairedUrl": wav_handle,
        "startFrame": start_frame,
        "endFrame": end_frame,
        "repairedFrames": end_frame.saturating_sub(start_frame),
        "sampleRate": rate,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn render_variants(
    session_id: String,
    selection: String,
    cues_run_id: Option<String>,
    dry_approved: bool,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    if !dry_approved {
        return Err(TauriError::invalid(
            "Stage 4 variants require passing the 5/5 human dry listening check first.",
        ));
    }
    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let (source_wav, cues_path, work_dir) = {
        let rec = guard
            .sessions
            .get(&session_id)
            .ok_or_else(|| TauriError::not_found("Session not found"))?;
        let (_dry, cues) = rec.resolve_cues_run(cues_run_id.as_deref())?;
        (rec.source_wav.clone(), cues, rec.work_dir.clone())
    };
    let out_dir = work_dir.join("variants").join(next_uid("var"));
    let mut manifest = qa::export_guitar_variants(
        &source_wav,
        &cues_path,
        &out_dir,
        &selection,
        DryReviewStatus::ApprovedByHuman,
    )
    .map_err(|e| TauriError::invalid(e.to_string()))?;

    if let Some(exports) = manifest.get_mut("exports").and_then(|v| v.as_array_mut()) {
        for item in exports {
            if let Some(file_rel) = item.get("file").and_then(|v| v.as_str()).map(str::to_owned) {
                let candidate = out_dir.join(&file_rel);
                if let Ok(handle) = guard.registry.register(&candidate) {
                    if let Some(obj) = item.as_object_mut() {
                        obj.insert("url".into(), serde_json::Value::String(handle));
                    }
                }
            }
        }
    }
    Ok(manifest)
}

#[tauri::command(rename_all = "camelCase")]
fn generate_technical_report(
    session_id: String,
    cues_run_id: String,
    pipeline_run_id: String,
    title: String,
    pipeline_label: String,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let (source_wav, after_wav, work_dir) = {
        let rec = guard
            .sessions
            .get(&session_id)
            .ok_or_else(|| TauriError::not_found("Session not found"))?;
        let (dry, _cues) = rec.resolve_cues_run(Some(&cues_run_id))?;
        let pipe_opt = rec.resolve_pipeline_run(Some(&pipeline_run_id))?;
        (rec.source_wav.clone(), pipe_opt.unwrap_or(dry), rec.work_dir.clone())
    };

    let (sb, before) = qa::read_audio(&source_wav).map_err(|e| TauriError::internal(e.to_string()))?;
    let (sa, after) = qa::read_audio(&after_wav).map_err(|e| TauriError::internal(e.to_string()))?;
    if sb.sample_rate != sa.sample_rate {
        return Err(TauriError::invalid("Sample rate mismatch between before and after WAVs."));
    }
    let report = technical_report::build_technical_report(
        &before,
        &after,
        sb.sample_rate,
        &sanitize_ipc_title(&title),
        &sanitize_ipc_title(&pipeline_label),
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;

    let report_id = next_uid("rep");
    let report_dir = work_dir.join("reports").join(&report_id);
    std::fs::create_dir_all(&report_dir).map_err(|e| TauriError::internal(e.to_string()))?;
    let report_json = report_dir.join("report.json");
    let bytes = serde_json::to_vec_pretty(&report).map_err(|e| TauriError::internal(e.to_string()))?;
    report_export::atomic_write_bytes(&report_json, &bytes)
        .map_err(|e| TauriError::internal(e.to_string()))?;
    if let Some(rec) = guard.sessions.get_mut(&session_id) {
        rec.report_runs.insert(report_id.clone(), report_json);
    }

    Ok(serde_json::json!({
        "reportId": report_id,
        "report": report,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn export_technical_report(
    session_id: String,
    report_id: String,
    format: String,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    let fmt = format.trim().to_ascii_lowercase();
    let ext = match fmt.as_str() {
        "pdf" => "pdf",
        "markdown" | "md" => "md",
        "docx" => "docx",
        _ => return Err(TauriError::invalid("Unsupported report format (expected pdf, markdown, or docx).")),
    };

    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let cache_key = format!("{report_id}:{ext}");
    let report_json = {
        let rec = guard
            .sessions
            .get(&session_id)
            .ok_or_else(|| TauriError::not_found("Session not found"))?;
        if let Some(existing_handle) = rec.report_exports.get(&cache_key) {
            if let Ok(path) = guard.registry.resolve(existing_handle) {
                let meta = std::fs::metadata(&path).map_err(|e| TauriError::internal(e.to_string()))?;
                return Ok(serde_json::json!({
                    "format": ext,
                    "downloadUrl": existing_handle,
                    "bytes": meta.len(),
                    "cached": true,
                }));
            }
        }
        rec.report_runs
            .get(&report_id)
            .cloned()
            .ok_or_else(|| TauriError::not_found("Requested technical report was not found."))?
    };

    let report: technical_report::TechnicalReport = serde_json::from_reader(
        std::fs::File::open(&report_json).map_err(|e| TauriError::internal(e.to_string()))?,
    )
    .map_err(|e| TauriError::internal(e.to_string()))?;
    let out_path = report_json
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("technical-report.{ext}"));
    let receipt = report_export::export_report(&report, &out_path, &fmt)
        .map_err(|e| TauriError::internal(e.to_string()))?;
    let handle = guard.registry.register(&out_path)?;
    if let Some(rec) = guard.sessions.get_mut(&session_id) {
        rec.report_exports.insert(cache_key, handle.clone());
    }

    Ok(serde_json::json!({
        "format": ext,
        "downloadUrl": handle,
        "receipt": receipt,
        "cached": false,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn export_daw_interchange(
    session_id: String,
    target: String,
    bit_depth: String,
    cues_run_id: String,
    pipeline_run_id: String,
    state: State<'_, ManagedAppState>,
) -> Result<serde_json::Value, TauriError> {
    let editor_target = EditorTarget::from_id(&target)
        .ok_or_else(|| TauriError::invalid("Unsupported DAW target (expected audition, logic, daw, or soundcraft)"))?;
    let depth = ExportBitDepth::from_id(&bit_depth)
        .ok_or_else(|| TauriError::invalid("Unsupported bit depth (expected 24 or 32)"))?;

    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let (dry_wav, final_wav, cues_path, title, out_dir) = {
        let rec = guard
            .sessions
            .get(&session_id)
            .ok_or_else(|| TauriError::not_found("Session not found"))?;
        let (dry, cues) = rec.resolve_cues_run(Some(&cues_run_id))?;
        let pipe_opt = rec.resolve_pipeline_run(Some(&pipeline_run_id))?;
        let out_dir = rec
            .work_dir
            .join("daw-interchange")
            .join(format!("{}-{}", editor_target.id(), next_uid("daw")));
        (dry.clone(), pipe_opt.unwrap_or(dry), cues, rec.label.clone(), out_dir)
    };

    let (dry_spec, dry_samples) =
        qa::read_audio(&dry_wav).map_err(|e| TauriError::internal(e.to_string()))?;
    let (final_spec, final_samples) =
        qa::read_audio(&final_wav).map_err(|e| TauriError::internal(e.to_string()))?;
    if dry_spec.channels != 1 || final_spec.channels != 1 {
        return Err(TauriError::invalid("Expected mono WAV stems for DAW interchange export"));
    }
    if dry_spec.sample_rate != final_spec.sample_rate {
        return Err(TauriError::invalid("Sample rate mismatch between dry and final stems"));
    }

    let markers = std::fs::File::open(&cues_path)
        .ok()
        .and_then(|f| serde_json::from_reader::<_, Performance>(f).ok())
        .map(|perf| markers_from_performance(&perf))
        .unwrap_or_default();

    let stems = [
        StemTrack {
            name: "Dry Take (Stage 1)",
            slug: "dry-take",
            samples: &dry_samples,
        },
        StemTrack {
            name: "Final Take (Stages 3-5)",
            slug: "final-take",
            samples: &final_samples,
        },
    ];

    // Audit M10: Never fabricate a default AudioPipelineEngine snapshot when no live snapshot exists.
    let req = InterchangeExportRequest {
        session_title: &sanitize_ipc_title(&title),
        sample_rate: dry_spec.sample_rate,
        bit_depth: depth,
        target: editor_target,
        stems: &stems,
        markers: &markers,
        engine_state: None,
        source_signature_note: Some(
            "Engine state snapshot was not supplied for this export; no default engine state was fabricated.",
        ),
    };
    let receipt =
        export_for_editor(&out_dir, &req).map_err(|e| TauriError::internal(e.to_string()))?;

    let mut files = Vec::with_capacity(receipt.assets.len());
    for asset in &receipt.assets {
        let abs = out_dir.join(&asset.relative_path);
        let handle = guard.registry.register(&abs).ok();
        files.push(serde_json::json!({
            "role": asset.role,
            "relative_path": asset.relative_path,
            "sha256": asset.sha256,
            "bytes": asset.bytes,
            "downloadUrl": handle,
        }));
    }

    Ok(serde_json::json!({
        "target_id": receipt.target_id,
        "target_name": receipt.target_name,
        "sample_rate": receipt.sample_rate,
        "bit_depth": receipt.bit_depth,
        "stem_count": receipt.stem_count,
        "marker_count": receipt.marker_count,
        "original_audio_opened": receipt.original_audio_opened,
        "files": files,
    }))
}

#[tauri::command]
fn get_settings() -> SettingsSnapshot {
    spectracraft_mono::settings_store::load_settings(None)
}

#[tauri::command(rename_all = "camelCase")]
fn save_settings(settings: SpectraCraftSettings) -> Result<SettingsSnapshot, TauriError> {
    spectracraft_mono::settings_store::save_settings(None, settings)
        .map_err(|e| TauriError::internal(e.to_string()))
}

#[tauri::command]
fn reset_settings() -> Result<SettingsSnapshot, TauriError> {
    spectracraft_mono::settings_store::reset_settings(None)
        .map_err(|e| TauriError::internal(e.to_string()))
}

#[tauri::command(rename_all = "camelCase")]
fn check_updates(online: bool) -> Result<serde_json::Value, TauriError> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let report = update_manager::run_update_check(&root, None, online);
    serde_json::to_value(report).map_err(|e| TauriError::internal(e.to_string()))
}

#[tauri::command(rename_all = "camelCase")]
fn run_update_action(
    action: String,
    notice_id: String,
    target: String,
) -> Result<serde_json::Value, TauriError> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let tgt = target.trim();
    let tgt_opt = if tgt.is_empty() { None } else { Some(tgt) };
    let res = match action.trim() {
        "not_now" => update_manager::apply_not_now(None, &notice_id),
        "download_later" => update_manager::apply_download_later(&root, None, tgt_opt),
        "update_now" => update_manager::apply_update_now(&root, None, tgt_opt),
        other => {
            return Err(TauriError::invalid(format!(
                "Unsupported update action '{other}'"
            )));
        }
    }
    .map_err(|e| TauriError::internal(e.to_string()))?;
    serde_json::to_value(res).map_err(|e| TauriError::internal(e.to_string()))
}

#[tauri::command(rename_all = "camelCase")]
fn read_scoped_media(
    handle: String,
    state: State<'_, ManagedAppState>,
) -> Result<Vec<u8>, TauriError> {
    let path = {
        let guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
        guard.registry.resolve(&handle)?
    };
    std::fs::read(&path).map_err(|_| TauriError::not_found("Unable to read scoped media artifact."))
}

#[tauri::command(rename_all = "camelCase")]
fn get_help_doc(doc_id: String) -> Result<serde_json::Value, TauriError> {
    let (title, rel_path) = match doc_id.trim() {
        "spec" => ("SpectraCraft Architecture Spec", "SPECTRACRAFT_SPEC.md"),
        "bible" => ("SpectraCraft Engineering Bible", "SPECTRACRAFT_BIBLE.md"),
        "readme" => ("SpectraCraft README", "README.md"),
        "craft-libs" => ("craft-libs Maintenance", "CRAFT_LIBS_MAINTENANCE.md"),
        _ => return Err(TauriError::not_found("Unknown help document ID.")),
    };
    let candidate = Path::new(rel_path);
    if candidate.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(TauriError::invalid("Invalid document path."));
    }
    let content = std::env::current_dir()
        .ok()
        .and_then(|cwd| std::fs::canonicalize(&cwd).ok())
        .and_then(|root| {
            let full = std::fs::canonicalize(root.join(candidate)).ok()?;
            if full.starts_with(&root) && full.is_file() {
                std::fs::read_to_string(full).ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            format!("# {title}\n\nDocument `{rel_path}` is bundled with the SpectraCraft repository.")
        });
    Ok(serde_json::json!({
        "id": doc_id.trim(),
        "title": title,
        "path": rel_path,
        "content": content,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn cancel_job(job_id: String, state: State<'_, ManagedAppState>) -> Result<bool, TauriError> {
    let mut guard = state.0.lock().map_err(|_| TauriError::internal("State lock poisoned"))?;
    let clean = job_id.trim();
    if clean.is_empty() {
        return Ok(false);
    }
    guard.record_cancelled_job(clean.to_string());
    Ok(true)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ManagedAppState(Mutex::new(TauriAppState::new())))
        .invoke_handler(tauri::generate_handler![
            platform_info,
            supported_daw_targets,
            get_studio_catalogs,
            analyze_studio_rig,
            load_reference_session,
            import_wav,
            get_reference_shape,
            analyze_layers,
            render_cues,
            regenerate_source,
            dry_region_edit,
            pipeline_render,
            repair_gap,
            render_variants,
            generate_technical_report,
            export_technical_report,
            export_daw_interchange,
            get_settings,
            save_settings,
            reset_settings,
            check_updates,
            run_update_action,
            read_scoped_media,
            get_help_doc,
            cancel_job,
        ])
        .run(tauri::generate_context!())
        .expect("error while running SpectraCraft Tauri shell");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m15_scoped_handle_registry_rejects_raw_paths_and_out_of_scope_files() {
        let root = std::env::temp_dir().join(format!("sc_tauri_scope_{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("sc_tauri_outside_{}.wav", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let inside = root.join("take.wav");
        std::fs::write(&inside, b"RIFF").unwrap();
        std::fs::write(&outside, b"SECRET").unwrap();

        let mut reg = ScopedHandleRegistry::new(root.clone());
        let handle = reg.register(&inside).unwrap();
        assert!(handle.starts_with("tauri-media://"));
        assert!(reg.resolve(&handle).is_ok());

        // Direct host paths must be rejected
        assert!(reg.resolve(&inside.display().to_string()).is_err());
        assert!(reg.resolve("/etc/passwd").is_err());
        // Registering file outside workspace root must be rejected
        assert!(reg.register(&outside).is_err());

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn h13_parse_studio_rig_config_rejects_empty_mics_and_oob_selected_mic() {
        let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
        let mics = studio_catalog::mic_catalog();
        let surfaces = studio_catalog::surface_catalog();

        let empty_mics = serde_json::json!({ "mics": [] });
        assert!(parse_studio_rig_config(&empty_mics, &sources, &mics, &surfaces).is_err());

        let oob_sel = serde_json::json!({
            "mics": [{ "label": "Mic 1", "catalog_index": 0, "enabled": true }],
            "selected_mic": 99
        });
        assert!(parse_studio_rig_config(&oob_sel, &sources, &mics, &surfaces).is_err());
    }

    #[test]
    fn h5_strict_run_resolution_rejects_unknown_and_evicted_run_ids() {
        let rec = TauriSessionRecord {
            id: "s1".into(),
            label: "Test".into(),
            is_reference: false,
            source_wav: PathBuf::from("/tmp/src.wav"),
            verify_dir: PathBuf::from("/tmp/v"),
            work_dir: PathBuf::from("/tmp/w"),
            dry_wav: PathBuf::from("/tmp/dry.wav"),
            cues_json: PathBuf::from("/tmp/cues.json"),
            cues_runs: HashMap::new(),
            cues_order: VecDeque::new(),
            evicted_cues_runs: HashSet::from(["cues-old".to_string()]),
            pipeline_runs: HashMap::new(),
            pipeline_order: VecDeque::new(),
            evicted_pipeline_runs: HashSet::from(["pipe-old".to_string()]),
            report_runs: HashMap::new(),
            report_exports: HashMap::new(),
        };

        assert_eq!(rec.resolve_cues_run(Some("unknown")).unwrap_err().code, "ARTIFACT_NOT_FOUND");
        assert_eq!(rec.resolve_cues_run(Some("cues-old")).unwrap_err().code, "ARTIFACT_EXPIRED");
        assert_eq!(rec.resolve_pipeline_run(Some("pipe-old")).unwrap_err().code, "ARTIFACT_EXPIRED");
    }
}
