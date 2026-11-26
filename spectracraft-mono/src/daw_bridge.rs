//! Thin audio-editor and DAW interchange compatibility layer.
//!
//! Exports standard lossless WAV stems, sample-accurate markers, and serialized
//! [`EngineStateSnapshot`] preset chunks for external editors without coupling
//! to any proprietary closed host SDK or altering the five-stage DSP pipeline.
//!
//! Supported targets behind the [`EditorInterchangeAdapter`] interface:
//! - **Adobe Audition** (`audition`): WAV stems + Audition Markers Panel TSV + `.sesx` session XML.
//! - **Logic Pro** (`logic`): WAV stems + sample-accurate marker CSV + AU/ARA interchange JSON.
//! - **Standard DAW / Audio Editor** (`daw`): WAV stems + RFC-4180 marker CSV + interchange JSON.
//! - **ArtCraft SoundCraft** (`soundcraft`): WAV stems under `Audio Files/` + `.scraft.json`
//!   descriptor aligned with `craft-scraft-format` (`Source`, `TrackKind`, `MemoryLocation`).

use crate::{
    digest::sha256,
    pipeline::{
        write_wav_f32_atomic_with_hash, write_wav_i24_tpdf_atomic_with_hash, EngineStateSnapshot,
    },
    reperform::{EvidenceStatus, Performance},
    report_export::atomic_write_bytes,
    Result,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Supported external audio-editor / DAW interchange targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorTarget {
    AdobeAudition,
    LogicPro,
    GenericDaw,
    SoundCraft,
}

impl EditorTarget {
    pub const ALL: [EditorTarget; 4] = [
        EditorTarget::AdobeAudition,
        EditorTarget::LogicPro,
        EditorTarget::GenericDaw,
        EditorTarget::SoundCraft,
    ];

    pub fn from_id(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "audition" | "adobe_audition" | "adobe-audition" => Some(Self::AdobeAudition),
            "logic" | "logic_pro" | "logic-pro" => Some(Self::LogicPro),
            "daw" | "generic" | "generic_daw" | "standard" => Some(Self::GenericDaw),
            "soundcraft" | "artcraft" | "scraft" => Some(Self::SoundCraft),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::AdobeAudition => "audition",
            Self::LogicPro => "logic",
            Self::GenericDaw => "daw",
            Self::SoundCraft => "soundcraft",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::AdobeAudition => "Adobe Audition",
            Self::LogicPro => "Logic Pro",
            Self::GenericDaw => "Standard DAW / Audio Editor",
            Self::SoundCraft => "ArtCraft SoundCraft",
        }
    }

    fn stem_subdir(self) -> &'static str {
        match self {
            // Matches craft-scraft-format's session `Audio Files` convention.
            Self::SoundCraft => "Audio Files",
            _ => "stems",
        }
    }
}

/// Output WAV sample encoding for exported interchange stems.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportBitDepth {
    Int24Tpdf,
    Float32,
}

impl ExportBitDepth {
    pub fn from_id(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "24" | "int24" | "pcm24" | "24bit" | "int24_tpdf" => Some(Self::Int24Tpdf),
            "32" | "f32" | "float32" | "32bit" => Some(Self::Float32),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Int24Tpdf => "24-bit PCM (TPDF dither)",
            Self::Float32 => "32-bit IEEE float",
        }
    }

    pub fn bits(self) -> u16 {
        match self {
            Self::Int24Tpdf => 24,
            Self::Float32 => 32,
        }
    }
}

/// Semantic category of an exported timeline marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    Onset,
    SurgicalRegion,
    UncertainFrame,
    CuePoint,
}

impl MarkerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Onset => "Onset",
            Self::SurgicalRegion => "SurgicalRegion",
            Self::UncertainFrame => "Uncertain",
            Self::CuePoint => "Cue",
        }
    }
}

/// A portable session marker exported alongside WAV stems.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionMarker {
    pub name: String,
    pub start_s: f64,
    pub duration_s: f64,
    pub kind: MarkerKind,
    #[serde(default)]
    pub comment: String,
}

/// A single mono stem buffer to emit inside an interchange bundle.
pub struct StemTrack<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub samples: &'a [f32],
}

/// Input parameters for [`export_for_editor`].
pub struct InterchangeExportRequest<'a> {
    pub session_title: &'a str,
    pub sample_rate: u32,
    pub bit_depth: ExportBitDepth,
    pub target: EditorTarget,
    pub stems: &'a [StemTrack<'a>],
    pub markers: &'a [SessionMarker],
    pub engine_state: Option<&'a EngineStateSnapshot>,
    pub source_signature_note: Option<&'a str>,
}

/// SHA-256 verified record of a single file written into an interchange folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrittenAsset {
    pub role: String,
    pub relative_path: String,
    pub sha256: String,
    pub bytes: u64,
}

/// Receipt returned after a successful interchange bundle export.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterchangeReceipt {
    pub schema_version: u32,
    pub target_id: String,
    pub target_name: String,
    pub session_title: String,
    pub sample_rate: u32,
    pub bit_depth: String,
    pub stem_count: usize,
    pub marker_count: usize,
    pub original_audio_opened: bool,
    pub assets: Vec<WrittenAsset>,
}

/// Metadata passed to each thin [`EditorInterchangeAdapter`] implementation after
/// common lossless WAV stems and optional [`EngineStateSnapshot`] are written.
pub struct WrittenStemInfo {
    pub name: String,
    pub slug: String,
    pub relative_path: String,
    pub sample_count: usize,
    pub sha256: String,
}

pub struct InterchangeWriteContext<'a> {
    pub session_title: &'a str,
    pub session_slug: &'a str,
    pub sample_rate: u32,
    pub bit_depth: ExportBitDepth,
    pub stems: &'a [WrittenStemInfo],
    pub markers: &'a [SessionMarker],
    pub engine_state: Option<&'a EngineStateSnapshot>,
    pub source_signature_note: Option<&'a str>,
}

/// Thin, extensible interface for target-specific editor/DAW session files.
///
/// New DAWs or plugin bridges can implement this trait without touching
/// Stage 1–5 DSP, WAV emission, or atomic persistence.
pub trait EditorInterchangeAdapter {
    fn target(&self) -> EditorTarget;
    fn emit_editor_files(
        &self,
        out_dir: &Path,
        ctx: &InterchangeWriteContext<'_>,
    ) -> Result<Vec<WrittenAsset>>;
}

/// Derive standard timeline markers (note onsets + contiguous uncertain-evidence spans)
/// directly from extracted Stage 1 performance cues.
pub fn markers_from_performance(perf: &Performance) -> Vec<SessionMarker> {
    let mut markers = Vec::new();
    for (idx, note) in perf.notes.iter().enumerate() {
        let mut hz_vals: Vec<f64> = note
            .hz
            .iter()
            .copied()
            .filter(|v| v.is_finite() && *v > 0.0)
            .collect();
        hz_vals.sort_by(f64::total_cmp);
        let median_hz = if hz_vals.is_empty() {
            0.0
        } else {
            hz_vals[hz_vals.len() / 2]
        };
        let duration_s = (note.end - note.start).max(0.0);
        let comment = if median_hz > 0.0 {
            format!("Stage 1 note cue ({median_hz:.1} Hz)")
        } else {
            "Stage 1 note cue".to_string()
        };
        markers.push(SessionMarker {
            name: format!("Note {:02}", idx + 1),
            start_s: note.start.max(0.0),
            duration_s,
            kind: MarkerKind::Onset,
            comment,
        });
    }

    // Group contiguous uncertain frames into range markers so engineers can inspect them.
    let hop_s = perf.hop_s.max(0.005);
    let mut uncertain_start: Option<f64> = None;
    let mut uncertain_end: f64 = 0.0;
    let mut uncertain_idx = 0usize;
    for frame in &perf.frame_evidence {
        if frame.status == EvidenceStatus::Uncertain {
            if uncertain_start.is_none() {
                uncertain_start = Some(frame.time_s.max(0.0));
            }
            uncertain_end = frame.time_s.max(0.0) + hop_s;
        } else if let Some(start) = uncertain_start.take() {
            uncertain_idx += 1;
            markers.push(SessionMarker {
                name: format!("Uncertain {:02}", uncertain_idx),
                start_s: start,
                duration_s: (uncertain_end - start).max(0.0),
                kind: MarkerKind::UncertainFrame,
                comment: "Uncertain cue evidence preserved from Stage 1 analysis".to_string(),
            });
        }
    }
    if let Some(start) = uncertain_start {
        uncertain_idx += 1;
        markers.push(SessionMarker {
            name: format!("Uncertain {:02}", uncertain_idx),
            start_s: start,
            duration_s: (uncertain_end - start).max(0.0),
            kind: MarkerKind::UncertainFrame,
            comment: "Uncertain cue evidence preserved from Stage 1 analysis".to_string(),
        });
    }

    markers.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    markers
}

/// Export lossless WAV stems, serialized engine state, and target-specific editor files.
pub fn export_for_editor(
    out_dir: &Path,
    req: &InterchangeExportRequest<'_>,
) -> Result<InterchangeReceipt> {
    let adapter: Box<dyn EditorInterchangeAdapter> = match req.target {
        EditorTarget::AdobeAudition => Box::new(AdobeAuditionAdapter),
        EditorTarget::LogicPro => Box::new(LogicProAdapter),
        EditorTarget::GenericDaw => Box::new(GenericDawAdapter),
        EditorTarget::SoundCraft => Box::new(SoundCraftAdapter),
    };
    export_with_adapter(out_dir, req, adapter.as_ref())
}

/// Export using any caller-supplied [`EditorInterchangeAdapter`] implementation.
pub fn export_with_adapter(
    out_dir: &Path,
    req: &InterchangeExportRequest<'_>,
    adapter: &dyn EditorInterchangeAdapter,
) -> Result<InterchangeReceipt> {
    if !crate::valid_rate(req.sample_rate) {
        return Err(format!("Unsupported sample rate {} Hz", req.sample_rate).into());
    }
    if req.stems.is_empty() {
        return Err("At least one audio stem is required for DAW interchange export".into());
    }
    for stem in req.stems {
        if stem.samples.is_empty() {
            return Err(format!("Stem '{}' has zero samples", stem.name).into());
        }
    }

    let target = adapter.target();
    let stem_dir_rel = target.stem_subdir();
    let stem_dir_abs = out_dir.join(stem_dir_rel);
    std::fs::create_dir_all(&stem_dir_abs)?;

    let mut assets = Vec::new();
    let mut written_stems = Vec::new();
    for (idx, stem) in req.stems.iter().enumerate() {
        let slug = sanitize_slug(stem.slug, &format!("stem-{}", idx + 1));
        let rel_path = format!("{stem_dir_rel}/{slug}.wav");
        let abs_path = out_dir.join(&rel_path);
        let written = match req.bit_depth {
            ExportBitDepth::Int24Tpdf => {
                write_wav_i24_tpdf_atomic_with_hash(&abs_path, req.sample_rate, stem.samples)?
            }
            ExportBitDepth::Float32 => {
                write_wav_f32_atomic_with_hash(&abs_path, req.sample_rate, stem.samples)?
            }
        };
        assets.push(WrittenAsset {
            role: format!("stem:{slug}"),
            relative_path: rel_path.clone(),
            sha256: written.sha256.clone(),
            bytes: written.bytes,
        });
        written_stems.push(WrittenStemInfo {
            name: stem.name.to_string(),
            slug,
            relative_path: rel_path,
            sample_count: stem.samples.len(),
            sha256: written.sha256,
        });
    }

    if let Some(state) = req.engine_state {
        let rel = "spectracraft-state.json";
        let bytes = serde_json::to_vec_pretty(state)?;
        assets.push(write_asset(out_dir, rel, "engine_state", &bytes)?);
    }

    let clean_session_title =
        sanitize_display_text(req.session_title, "SpectraCraft Session", 160);
    let session_slug = sanitize_slug(&clean_session_title, "spectracraft-session");
    let ctx = InterchangeWriteContext {
        session_title: &clean_session_title,
        session_slug: &session_slug,
        sample_rate: req.sample_rate,
        bit_depth: req.bit_depth,
        stems: &written_stems,
        markers: req.markers,
        engine_state: req.engine_state,
        source_signature_note: req.source_signature_note,
    };

    let editor_assets = adapter.emit_editor_files(out_dir, &ctx)?;
    assets.extend(editor_assets);

    let receipt = InterchangeReceipt {
        schema_version: 1,
        target_id: target.id().to_string(),
        target_name: target.display_name().to_string(),
        session_title: clean_session_title,
        sample_rate: req.sample_rate,
        bit_depth: req.bit_depth.label().to_string(),
        stem_count: written_stems.len(),
        marker_count: req.markers.len(),
        original_audio_opened: false,
        assets,
    };

    let receipt_bytes = serde_json::to_vec_pretty(&receipt)?;
    atomic_write_bytes(&out_dir.join("interchange-receipt.json"), &receipt_bytes)?;
    Ok(receipt)
}

fn write_asset(out_dir: &Path, rel_path: &str, role: &str, bytes: &[u8]) -> Result<WrittenAsset> {
    let abs = out_dir.join(rel_path);
    atomic_write_bytes(&abs, bytes)?;
    Ok(WrittenAsset {
        role: role.to_string(),
        relative_path: rel_path.to_string(),
        sha256: sha256(bytes),
        bytes: bytes.len() as u64,
    })
}

fn sanitize_slug(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        let low = ch.to_ascii_lowercase();
        if low.is_ascii_alphanumeric() {
            out.push(low);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn sanitize_display_text(raw: &str, fallback: &str, max_chars: usize) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max_chars)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn neutralize_spreadsheet_formula(s: String) -> String {
    if s.starts_with(['=', '+', '-', '@']) {
        format!("'{s}")
    } else {
        s
    }
}

fn csv_escape(s: &str) -> String {
    let clean: String = s
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
        .collect();
    let safe = neutralize_spreadsheet_formula(clean);
    if safe.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

fn tsv_sanitize(s: &str) -> String {
    let clean: String = s
        .chars()
        .filter_map(|c| match c {
            '\t' | '\r' | '\n' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect();
    neutralize_spreadsheet_formula(clean)
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_control() && !matches!(ch, '\t' | '\n' | '\r') {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn format_hms_ms(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let total_m = total_s / 60;
    let m = total_m % 60;
    let h = total_m / 60;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

fn sec_to_samples(seconds: f64, sample_rate: u32) -> u64 {
    (seconds.max(0.0) * f64::from(sample_rate)).round() as u64
}

// ---------------------------------------------------------------------------
// 1. Adobe Audition adapter (Markers Panel TSV + Multitrack .sesx XML)
// ---------------------------------------------------------------------------

pub struct AdobeAuditionAdapter;

impl EditorInterchangeAdapter for AdobeAuditionAdapter {
    fn target(&self) -> EditorTarget {
        EditorTarget::AdobeAudition
    }

    fn emit_editor_files(
        &self,
        out_dir: &Path,
        ctx: &InterchangeWriteContext<'_>,
    ) -> Result<Vec<WrittenAsset>> {
        let mut written = Vec::new();

        // Standard Adobe Audition Markers Panel TSV format.
        let mut tsv = String::from("Name\tStart\tDuration\tTime Format\tType\tDescription\n");
        for m in ctx.markers {
            let marker_type = if m.duration_s > 0.001 {
                "Subclip"
            } else {
                "Cue"
            };
            tsv.push_str(&format!(
                "{}\t{}\t{}\tdecimal\t{}\t{}\n",
                tsv_sanitize(&m.name),
                format_hms_ms(m.start_s),
                format_hms_ms(m.duration_s),
                marker_type,
                tsv_sanitize(&m.comment),
            ));
        }
        written.push(write_asset(
            out_dir,
            "audition-markers.csv",
            "audition_markers_tsv",
            tsv.as_bytes(),
        )?);

        // Minimal standards-compliant Adobe Audition multitrack .sesx XML referencing relative stems.
        let max_samples = ctx.stems.iter().map(|s| s.sample_count).max().unwrap_or(0);
        let duration_s = max_samples as f64 / f64::from(ctx.sample_rate);
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n");
        xml.push_str("<!DOCTYPE sesx>\n");
        xml.push_str("<sesx version=\"1.9\">\n");
        xml.push_str(&format!(
            "  <session appVersion=\"14.0\" audioChannelType=\"mono\" bitDepth=\"{}\" duration=\"{:.6}\" sampleRate=\"{}\">\n",
            ctx.bit_depth.bits(),
            duration_s,
            ctx.sample_rate
        ));
        xml.push_str(&format!(
            "    <name>{}</name>\n",
            xml_escape(ctx.session_title)
        ));
        xml.push_str("    <tracks>\n");
        for (idx, stem) in ctx.stems.iter().enumerate() {
            let track_id = idx + 1;
            xml.push_str(&format!(
                "      <audioTrack automationLaneOpenState=\"false\" id=\"{track_id}\" index=\"{track_id}\" select=\"false\" visible=\"true\">\n"
            ));
            xml.push_str(&format!(
                "        <trackParameters><name>{}</name></trackParameters>\n",
                xml_escape(&stem.name)
            ));
            xml.push_str(&format!(
                "        <audioClip clipAutoCrossfade=\"true\" crossFadeHeadClipID=\"-1\" crossFadeTailClipID=\"-1\" endPoint=\"{}\" fileID=\"{idx}\" hue=\"180\" id=\"{track_id}\" name=\"{}\" offline=\"false\" select=\"false\" sourceInPoint=\"0\" sourceOutPoint=\"{}\" startPoint=\"0\" zOrder=\"0\"/>\n",
                stem.sample_count,
                xml_escape(&stem.name),
                stem.sample_count,
            ));
            xml.push_str("      </audioTrack>\n");
        }
        xml.push_str("    </tracks>\n");
        xml.push_str("    <files>\n");
        for (idx, stem) in ctx.stems.iter().enumerate() {
            xml.push_str(&format!(
                "      <file id=\"{idx}\" importerPrivateSettings=\"\" mediaHandler=\"AmioWav\" relativePath=\"{}\"/>\n",
                xml_escape(&stem.relative_path)
            ));
        }
        xml.push_str("    </files>\n");
        xml.push_str("  </session>\n");
        xml.push_str("</sesx>\n");

        let sesx_name = format!("{}.sesx", ctx.session_slug);
        written.push(write_asset(
            out_dir,
            &sesx_name,
            "audition_sesx",
            xml.as_bytes(),
        )?);
        Ok(written)
    }
}

// ---------------------------------------------------------------------------
// 2. Logic Pro adapter (Sample-accurate markers CSV + AU/ARA interchange JSON)
// ---------------------------------------------------------------------------

pub struct LogicProAdapter;

impl EditorInterchangeAdapter for LogicProAdapter {
    fn target(&self) -> EditorTarget {
        EditorTarget::LogicPro
    }

    fn emit_editor_files(
        &self,
        out_dir: &Path,
        ctx: &InterchangeWriteContext<'_>,
    ) -> Result<Vec<WrittenAsset>> {
        let mut written = Vec::new();

        let mut csv =
            String::from("Number,Name,StartTime,Duration,StartSample,LengthSamples,Kind,Comment\n");
        for (idx, m) in ctx.markers.iter().enumerate() {
            let start_sample = sec_to_samples(m.start_s, ctx.sample_rate);
            let len_samples = sec_to_samples(m.duration_s, ctx.sample_rate);
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{}\n",
                idx + 1,
                csv_escape(&m.name),
                format_hms_ms(m.start_s),
                format_hms_ms(m.duration_s),
                start_sample,
                len_samples,
                m.kind.label(),
                csv_escape(&m.comment),
            ));
        }
        written.push(write_asset(
            out_dir,
            "logic-markers.csv",
            "logic_markers_csv",
            csv.as_bytes(),
        )?);

        let manifest = serde_json::json!({
            "schema_version": 1,
            "format": "logic-pro-interchange",
            "host_target": "Logic Pro (AU / ARA2 / WAV Stem Import)",
            "session_title": ctx.session_title,
            "sample_rate": ctx.sample_rate,
            "bit_depth": ctx.bit_depth.bits(),
            "source_signature_note": ctx.source_signature_note,
            "original_audio_opened": false,
            "tracks": ctx.stems.iter().enumerate().map(|(i, s)| serde_json::json!({
                "track_index": i + 1,
                "name": s.name,
                "relative_wav_path": s.relative_path,
                "start_sample": 0,
                "length_samples": s.sample_count,
                "channels": 1,
                "sha256": s.sha256,
            })).collect::<Vec<_>>(),
            "markers": ctx.markers.iter().enumerate().map(|(i, m)| serde_json::json!({
                "number": i + 1,
                "name": m.name,
                "start_s": m.start_s,
                "duration_s": m.duration_s,
                "start_sample": sec_to_samples(m.start_s, ctx.sample_rate),
                "length_samples": sec_to_samples(m.duration_s, ctx.sample_rate),
                "kind": m.kind,
                "comment": m.comment,
            })).collect::<Vec<_>>(),
            "au_preset_state": ctx.engine_state,
        });
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
        written.push(write_asset(
            out_dir,
            "logic-interchange.json",
            "logic_interchange_json",
            &manifest_bytes,
        )?);
        Ok(written)
    }
}

// ---------------------------------------------------------------------------
// 3. Generic DAW / Audio Editor adapter (Standard CSV + interchange JSON)
// ---------------------------------------------------------------------------

pub struct GenericDawAdapter;

impl EditorInterchangeAdapter for GenericDawAdapter {
    fn target(&self) -> EditorTarget {
        EditorTarget::GenericDaw
    }

    fn emit_editor_files(
        &self,
        out_dir: &Path,
        ctx: &InterchangeWriteContext<'_>,
    ) -> Result<Vec<WrittenAsset>> {
        let mut written = Vec::new();

        let mut csv = String::from("Name,StartSec,DurationSec,StartSample,Kind,Comment\n");
        for m in ctx.markers {
            csv.push_str(&format!(
                "{},{:.6},{:.6},{},{},{}\n",
                csv_escape(&m.name),
                m.start_s.max(0.0),
                m.duration_s.max(0.0),
                sec_to_samples(m.start_s, ctx.sample_rate),
                m.kind.label(),
                csv_escape(&m.comment),
            ));
        }
        written.push(write_asset(
            out_dir,
            "markers.csv",
            "markers_csv",
            csv.as_bytes(),
        )?);

        let manifest = serde_json::json!({
            "schema_version": 1,
            "format": "spectracraft-daw-interchange",
            "session_title": ctx.session_title,
            "sample_rate": ctx.sample_rate,
            "bit_depth": ctx.bit_depth.bits(),
            "source_signature_note": ctx.source_signature_note,
            "original_audio_opened": false,
            "stems": ctx.stems.iter().map(|s| serde_json::json!({
                "name": s.name,
                "slug": s.slug,
                "relative_path": s.relative_path,
                "channels": 1,
                "sample_count": s.sample_count,
                "sha256": s.sha256,
            })).collect::<Vec<_>>(),
            "markers": ctx.markers,
            "engine_state": ctx.engine_state,
        });
        let bytes = serde_json::to_vec_pretty(&manifest)?;
        written.push(write_asset(
            out_dir,
            "daw-interchange.json",
            "daw_interchange_json",
            &bytes,
        )?);
        Ok(written)
    }
}

// ---------------------------------------------------------------------------
// 4. ArtCraft SoundCraft adapter (aligned with craft-scraft-format schema)
// ---------------------------------------------------------------------------

pub struct SoundCraftAdapter;

impl EditorInterchangeAdapter for SoundCraftAdapter {
    fn target(&self) -> EditorTarget {
        EditorTarget::SoundCraft
    }

    fn emit_editor_files(
        &self,
        out_dir: &Path,
        ctx: &InterchangeWriteContext<'_>,
    ) -> Result<Vec<WrittenAsset>> {
        // Aligned with `craft-scraft-format` (`Source`, `FileFormat::Wav`, `TrackKind::Audio`,
        // `MemoryLocation`, `MarkerKind::Marker` / `MarkerKind::Selection`, `Audio Files/` layout).
        let sources: Vec<serde_json::Value> = ctx
            .stems
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                serde_json::json!({
                    "id": idx + 1,
                    "name": s.name,
                    "path": s.relative_path,
                    "channels": 1,
                    "frames": s.sample_count,
                    "sample_rate": ctx.sample_rate,
                    "format": "Wav",
                    "sha256": s.sha256,
                })
            })
            .collect();

        let tracks: Vec<serde_json::Value> = ctx
            .stems
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                serde_json::json!({
                    "id": idx + 1,
                    "name": s.name,
                    "kind": "Audio",
                    "channel_format": "Mono",
                    "clips": [{
                        "id": idx + 1,
                        "name": s.name,
                        "source_id": idx + 1,
                        "start": 0,
                        "end": s.sample_count,
                        "source_offset": 0,
                    }],
                })
            })
            .collect();

        let memory_locations: Vec<serde_json::Value> = ctx
            .markers
            .iter()
            .enumerate()
            .map(|(idx, m)| {
                let start = sec_to_samples(m.start_s, ctx.sample_rate);
                let len = sec_to_samples(m.duration_s, ctx.sample_rate);
                let kind = if len > 0 { "Selection" } else { "Marker" };
                serde_json::json!({
                    "id": idx + 1,
                    "number": (idx + 1) as u32,
                    "name": m.name,
                    "kind": kind,
                    "start": start,
                    "end": start + len,
                    "comments": m.comment,
                    "ruler": 1,
                })
            })
            .collect();

        let doc = serde_json::json!({
            "schema_version": 1,
            "format": "soundcraft-interchange",
            "compatible_crate": "craft-scraft-format",
            "session_extension": "scraft",
            "name": ctx.session_title,
            "sample_rate": ctx.sample_rate,
            "bit_depth": ctx.bit_depth.bits(),
            "source_signature_note": ctx.source_signature_note,
            "original_audio_opened": false,
            "sources": sources,
            "tracks": tracks,
            "memory_locations": memory_locations,
            "spectracraft_engine_state": ctx.engine_state,
        });
        let bytes = serde_json::to_vec_pretty(&doc)?;
        let filename = format!("{}.scraft.json", ctx.session_slug);
        Ok(vec![write_asset(
            out_dir,
            &filename,
            "soundcraft_scraft_json",
            &bytes,
        )?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::AudioPipelineEngine;
    use std::f64::consts::TAU;

    fn test_tone(rate: u32, hz: f64, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| (0.3 * (TAU * hz * i as f64 / f64::from(rate)).sin()) as f32)
            .collect()
    }

    #[test]
    fn all_editor_targets_export_valid_wavs_markers_and_engine_state() {
        let rate = 48_000_u32;
        let dry = test_tone(rate, 220.0, 2400);
        let final_take = test_tone(rate, 220.0, 2400);
        let mut engine = AudioPipelineEngine::new(rate).expect("engine init");
        engine.post_dry_config_mut().mic_enabled = true;
        let snapshot = engine.snapshot_state();

        let markers = vec![
            SessionMarker {
                name: "Note 01".into(),
                start_s: 0.01,
                duration_s: 0.02,
                kind: MarkerKind::Onset,
                comment: "A3 dry onset".into(),
            },
            SessionMarker {
                name: "Uncertain 01".into(),
                start_s: 0.035,
                duration_s: 0.0,
                kind: MarkerKind::UncertainFrame,
                comment: "Low confidence".into(),
            },
        ];

        let stems = vec![
            StemTrack {
                name: "Dry Take (Stage 1)",
                slug: "dry-take",
                samples: &dry,
            },
            StemTrack {
                name: "Final Take (Stages 3-5)",
                slug: "final-take",
                samples: &final_take,
            },
        ];

        let base =
            std::env::temp_dir().join(format!("spectracraft-daw-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);

        for target in EditorTarget::ALL {
            let out_dir = base.join(target.id());
            let req = InterchangeExportRequest {
                session_title: "Solo Guitar Test",
                sample_rate: rate,
                bit_depth: ExportBitDepth::Int24Tpdf,
                target,
                stems: &stems,
                markers: &markers,
                engine_state: Some(&snapshot),
                source_signature_note: Some("Solo guitar · Shure SM7 · 10x10x10 padded"),
            };
            let receipt = export_for_editor(&out_dir, &req).expect("interchange export");
            assert_eq!(receipt.target_id, target.id());
            assert_eq!(receipt.stem_count, 2);
            assert_eq!(receipt.marker_count, 2);
            assert!(!receipt.original_audio_opened);
            assert!(out_dir.join("interchange-receipt.json").is_file());

            // Verify EngineStateSnapshot round-trip from exported preset JSON.
            let state_raw = std::fs::read(out_dir.join("spectracraft-state.json")).unwrap();
            let mut restored = AudioPipelineEngine::new(rate).unwrap();
            restored.import_state_json(&state_raw).unwrap();
            assert!(restored.post_dry_config().mic_enabled);

            // Verify exported WAV files are readable lossless mono WAVs of exact length.
            let wav_rel = if target == EditorTarget::SoundCraft {
                "Audio Files/dry-take.wav"
            } else {
                "stems/dry-take.wav"
            };
            let reader = hound::WavReader::open(out_dir.join(wav_rel)).unwrap();
            assert_eq!(reader.spec().channels, 1);
            assert_eq!(reader.spec().sample_rate, rate);
            assert_eq!(reader.spec().bits_per_sample, 24);
            assert_eq!(reader.len() as usize, dry.len());

            // Verify target-specific artifacts exist and contain expected structure.
            match target {
                EditorTarget::AdobeAudition => {
                    let tsv =
                        std::fs::read_to_string(out_dir.join("audition-markers.csv")).unwrap();
                    assert!(
                        tsv.starts_with("Name\tStart\tDuration\tTime Format\tType\tDescription")
                    );
                    assert!(tsv.contains("Note 01"));
                    let sesx =
                        std::fs::read_to_string(out_dir.join("solo-guitar-test.sesx")).unwrap();
                    assert!(sesx.contains("<sesx version=\"1.9\">"));
                    assert!(sesx.contains("stems/dry-take.wav"));
                }
                EditorTarget::LogicPro => {
                    let csv = std::fs::read_to_string(out_dir.join("logic-markers.csv")).unwrap();
                    assert!(csv.contains("StartSample"));
                    assert!(csv.contains("Note 01"));
                    let manifest: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(out_dir.join("logic-interchange.json")).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(manifest["format"], "logic-pro-interchange");
                    assert_eq!(manifest["tracks"].as_array().unwrap().len(), 2);
                }
                EditorTarget::GenericDaw => {
                    let csv = std::fs::read_to_string(out_dir.join("markers.csv")).unwrap();
                    assert!(csv.contains("StartSec"));
                    let manifest: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(out_dir.join("daw-interchange.json")).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(manifest["stems"].as_array().unwrap().len(), 2);
                }
                EditorTarget::SoundCraft => {
                    let doc: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(out_dir.join("solo-guitar-test.scraft.json")).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(doc["compatible_crate"], "craft-scraft-format");
                    assert_eq!(doc["sources"][0]["format"], "Wav");
                    assert_eq!(doc["sources"][0]["path"], "Audio Files/dry-take.wav");
                    assert_eq!(doc["memory_locations"].as_array().unwrap().len(), 2);
                    assert_eq!(doc["memory_locations"][0]["kind"], "Selection");
                    assert_eq!(doc["memory_locations"][1]["kind"], "Marker");
                }
            }
        }

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn sanitizes_control_chars_xml_and_csv_formulas() {
        assert_eq!(xml_escape("<a&b>\x00\x07"), "&lt;a&amp;b&gt;");
        assert_eq!(csv_escape("=CMD()"), "'=CMD()");
        assert_eq!(csv_escape("+SUM(A1,A2)"), "\"'+SUM(A1,A2)\"");
        assert_eq!(tsv_sanitize("=1+1\tbad\x00"), "'=1+1 bad");
        assert_eq!(
            sanitize_display_text("My\r\nTake\x00", "Fallback", 64),
            "My  Take"
        );
    }
}
