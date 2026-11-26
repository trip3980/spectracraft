//! Thin compatibility isolation layer for all `storytold/craft-libs` usage in SpectraCraft.
//!
//! # Architectural Rule
//! Every call into `craft-libs` crates MUST live in this module. The rest of SpectraCraft
//! (`technical_report`, `pipeline`, CLI binaries, tests) talks exclusively to the functions
//! exposed here. If a future `craft-libs` bump changes an upstream API, only this file needs
//! to be updated.
//!
//! # Exact `craft-libs` API Surface Currently Consumed
//! - Crate `craft-pdf` (pinned Git rev `05b99ee5dcd64f2c954a693ef361301214ddc3c7`, `default-features = false`):
//!   - `craft_pdf::classic::Writer`
//!     - `Writer::with_capacity(capacity: usize) -> Result<Writer, craft_pdf::classic::Error>`
//!     - `Writer::object(&mut self, id: u32, body: &str) -> Result<(), craft_pdf::classic::Error>`
//!     - `Writer::stream(&mut self, id: u32, entries: &str, data: &[u8]) -> Result<(), craft_pdf::classic::Error>`
//!     - `Writer::finish(self, root: u32, info: Option<u32>) -> Result<Vec<u8>, craft_pdf::classic::Error>`
//!   - `craft_pdf::classic::Error` (boxed via `Into<Box<dyn std::error::Error>>`)
//! - Crate `craft-atomic` (pinned Git rev `05b99ee5dcd64f2c954a693ef361301214ddc3c7`):
//!   - `craft_atomic::atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()>`
//!   - `craft_atomic::write_new(path: &Path, contents: &[u8]) -> std::io::Result<()>`
//!
//! # Maintenance Policy
//! Pins are explicit Git commits (or release tags once published on `storytold/craft-libs`),
//! never floating branches. See `CRAFT_LIBS_MAINTENANCE.md` for the deliberate update check,
//! bump procedure, and mandatory 20 Hz–20 kHz sweep verification gate.

use crate::technical_report::{CraftLibsIntegration, EvidenceTag, ExportReceipt, TechnicalReport};
use crate::{digest, Result};
use serde::Serialize;
use std::fmt::Write as _;
use std::path::Path;

pub const CRAFT_LIBS_REPO: &str = "https://github.com/storytold/craft-libs";
pub const CRAFT_LIBS_PIN_KIND: &str = "rev";
pub const CRAFT_LIBS_PIN: &str = "05b99ee5dcd64f2c954a693ef361301214ddc3c7";
pub const CRAFT_LIBS_PIN_SHORT: &str = "05b99ee5";

#[derive(Clone, Debug, Serialize)]
pub struct CraftLibsPinInfo {
    pub crate_name: &'static str,
    pub git: &'static str,
    pub pin_kind: &'static str,
    pub pin_value: &'static str,
    pub features: &'static str,
    pub symbols_used: &'static [&'static str],
}

#[derive(Clone, Debug, Serialize)]
pub struct ArtCraftSharedCratePoint {
    pub crate_name: &'static str,
    pub target_domain: &'static str,
    pub status_in_spectracraft: &'static str,
    pub guidance: &'static str,
}

pub const CRAFT_LIBS_PINS: &[CraftLibsPinInfo] = &[
    CraftLibsPinInfo {
        crate_name: "craft-pdf",
        git: CRAFT_LIBS_REPO,
        pin_kind: CRAFT_LIBS_PIN_KIND,
        pin_value: CRAFT_LIBS_PIN,
        features: "default-features = false",
        symbols_used: &[
            "craft_pdf::classic::Writer::with_capacity",
            "craft_pdf::classic::Writer::object",
            "craft_pdf::classic::Writer::stream",
            "craft_pdf::classic::Writer::finish",
            "craft_pdf::classic::Error",
        ],
    },
    CraftLibsPinInfo {
        crate_name: "craft-atomic",
        git: CRAFT_LIBS_REPO,
        pin_kind: CRAFT_LIBS_PIN_KIND,
        pin_value: CRAFT_LIBS_PIN,
        features: "default",
        symbols_used: &["craft_atomic::atomic_write", "craft_atomic::write_new"],
    },
    CraftLibsPinInfo {
        crate_name: "craft-settings",
        git: CRAFT_LIBS_REPO,
        pin_kind: CRAFT_LIBS_PIN_KIND,
        pin_value: CRAFT_LIBS_PIN,
        features: "default",
        symbols_used: &[
            "craft_settings::Settings",
            "craft_settings::Location",
            "craft_settings::SettingsStore::{open, get, set, flush, path, is_read_only}",
            "craft_settings::DocumentMut",
            "craft_settings::MigrateError",
        ],
    },
];

pub const ARTCRAFT_SHARED_INTEGRATION_POINTS: &[ArtCraftSharedCratePoint] = &[
    ArtCraftSharedCratePoint {
        crate_name: "craft-pdf",
        target_domain: "PDF export & inspection",
        status_in_spectracraft: "Active (pinned Git dependency via report_export.rs)",
        guidance: "Use for dark-mode technical white-paper PDF output; keep all calls isolated in report_export.rs.",
    },
    ArtCraftSharedCratePoint {
        crate_name: "craft-atomic",
        target_domain: "Crash-safe atomic writes",
        status_in_spectracraft: "Active (pinned Git dependency via report_export.rs)",
        guidance: "Use for all WAV, JSON, PDF, and Markdown artifact writes through report_export::atomic_write_bytes.",
    },
    ArtCraftSharedCratePoint {
        crate_name: "craft-wordmodel",
        target_domain: "Shared structured document model (ArtCraft / WordCraft / DeckCraft)",
        status_in_spectracraft: "Future integration point (not yet linked)",
        guidance: "Adopt before adding native DOCX export via craft-docx instead of inventing a custom document AST.",
    },
    ArtCraftSharedCratePoint {
        crate_name: "craft-docx",
        target_domain: "Word (.docx) report export",
        status_in_spectracraft: "Deferred (Markdown fallback documented in report_export.rs)",
        guidance: "Enable behind report_export.rs when craft-wordmodel/craft-opc dependency budget is approved.",
    },
    ArtCraftSharedCratePoint {
        crate_name: "craft-timeline",
        target_domain: "Shared timeline tracks & clips (FilmCraft / SpectraCraft)",
        status_in_spectracraft: "Future integration point (not yet linked)",
        guidance: "Prefer craft-timeline data models if SpectraCraft expands beyond single-source editing into multi-clip timelines.",
    },
    ArtCraftSharedCratePoint {
        crate_name: "craft-ui",
        target_domain: "Shared ArtCraft desktop widgets & theming",
        status_in_spectracraft: "Future integration point (currently targets newer egui than desktop feature)",
        guidance: "Adopt when desktop eframe/egui version aligns with craft-libs rather than duplicating desktop chrome.",
    },
    ArtCraftSharedCratePoint {
        crate_name: "craft-settings",
        target_domain: "Shared user preferences & schema persistence",
        status_in_spectracraft: "Active (pinned Git dependency via settings_store.rs)",
        guidance: "Stores local uncommitted config.toml with atomic saves, backups, defaults, reset, and unknown ArtCraft key preservation.",
    },
    ArtCraftSharedCratePoint {
        crate_name: "craft-audio / craft-audio-dsp",
        target_domain: "Shared audio containers & utility DSP",
        status_in_spectracraft: "Evaluate carefully (never replace SpectraCraft Stage 1 physics)",
        guidance: "May share I/O or utility wrappers in the future, while preserving SpectraCraft Stage 1 dry physics invariants.",
    },
];

/// Crash-safe file write wrapper around `craft_atomic::atomic_write`.
/// Automatically ensures parent directories exist before writing.
pub fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    craft_atomic::atomic_write(path, bytes)?;
    Ok(())
}

/// Crash-safe exclusive-create file write wrapper around `craft_atomic::write_new`
/// (fails with `AlreadyExists` if the target file already exists).
pub fn atomic_write_new_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    craft_atomic::write_new(path, bytes)?;
    Ok(())
}

/// Parsed refs returned by `git ls-remote --tags --heads` against `CRAFT_LIBS_REPO`.
pub struct CraftLibsRemoteRefs {
    pub tags: Vec<(String, String)>,
    pub main_rev: Option<String>,
}

/// Query `storytold/craft-libs` remote tags and `main` HEAD once for both
/// `check_craft_libs_status` and `update_manager::run_update_check`.
pub fn query_craft_libs_remote_refs() -> std::result::Result<CraftLibsRemoteRefs, String> {
    let out = std::process::Command::new("git")
        .args([
            "ls-remote",
            "--tags",
            "--heads",
            &format!("{CRAFT_LIBS_REPO}.git"),
        ])
        .output()
        .map_err(|err| format!("Unable to invoke git ls-remote: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "git ls-remote returned non-zero status ({}); skipping online comparison.",
            out.status
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut tags = Vec::new();
    let mut main_rev = None;
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let (Some(sha), Some(refname)) = (parts.next(), parts.next()) else {
            continue;
        };
        if refname == "refs/heads/main" {
            main_rev = Some(sha.to_string());
        } else if let Some(tag) = refname.strip_prefix("refs/tags/") {
            if !tag.ends_with("^{}") {
                tags.push((tag.to_string(), sha.to_string()));
            }
        }
    }
    Ok(CraftLibsRemoteRefs { tags, main_rev })
}

/// Summary embedded in every `TechnicalReport` describing active `craft-libs` crates and fallbacks.
pub fn craft_libs_integration_summary() -> CraftLibsIntegration {
    CraftLibsIntegration {
        crates_used: vec![
            format!(
                "craft-pdf (pinned Git {CRAFT_LIBS_PIN_KIND} {CRAFT_LIBS_PIN_SHORT} from storytold/craft-libs, isolated in report_export.rs)"
            ),
            format!(
                "craft-atomic (pinned Git {CRAFT_LIBS_PIN_KIND} {CRAFT_LIBS_PIN_SHORT} from storytold/craft-libs, isolated in report_export.rs)"
            ),
            format!(
                "craft-settings (pinned Git {CRAFT_LIBS_PIN_KIND} {CRAFT_LIBS_PIN_SHORT} from storytold/craft-libs, isolated in settings_store.rs)"
            ),
        ],
        unavailable_fallbacks: vec![
            "craft-docx not linked (requires craft-wordmodel/craft-opc/craft-drawingml stack); DOCX export falls back to readable Markdown white paper (dark-mode PDF with embedded plots is available via craft-pdf).".into(),
            "craft-ui not linked (targets newer egui than workspace desktop feature); in-program viewer renders directly in SpectraCraft dark-mode UI.".into(),
        ],
    }
}

/// Build an informational JSON report for `spectracraft-qa check-craft-libs [--online]`.
/// Never modifies files and never returns an error merely because a newer upstream tag exists.
pub fn check_craft_libs_status(online: bool) -> serde_json::Value {
    let mut upstream_tags: Vec<String> = Vec::new();
    let mut upstream_main_rev: Option<String> = None;
    let mut online_note: String =
        "Offline mode (pass --online to query storytold/craft-libs via git ls-remote).".into();
    let mut update_available: Option<bool> = None;

    if online {
        match query_craft_libs_remote_refs() {
            Ok(refs) => {
                upstream_main_rev = refs.main_rev;
                upstream_tags = refs
                    .tags
                    .into_iter()
                    .map(|(tag, sha)| format!("{tag} ({})", &sha[..sha.len().min(8)]))
                    .collect();
                let newer_main = upstream_main_rev
                    .as_deref()
                    .map(|sha| sha != CRAFT_LIBS_PIN)
                    .unwrap_or(false);
                update_available = Some(newer_main || !upstream_tags.is_empty());
                online_note = if upstream_tags.is_empty() {
                    if newer_main {
                        "No release tags published yet on storytold/craft-libs; main has newer commits than pinned rev (informational only).".into()
                    } else {
                        "No release tags published yet on storytold/craft-libs; pinned rev matches current main.".into()
                    }
                } else {
                    "Release tags detected on storytold/craft-libs; review changelog before manually bumping Cargo.toml pins.".into()
                };
            }
            Err(note) => {
                online_note = note;
            }
        }
    }

    serde_json::json!({
        "repository": CRAFT_LIBS_REPO,
        "isolation_module": "spectracraft-mono/src/report_export.rs",
        "auto_update_enabled": false,
        "fail_build_on_newer_tag": false,
        "current_pins": CRAFT_LIBS_PINS,
        "online_checked": online,
        "upstream_tags": upstream_tags,
        "upstream_main_rev": upstream_main_rev,
        "update_available": update_available,
        "note": online_note,
        "artcraft_shared_integration_points": ARTCRAFT_SHARED_INTEGRATION_POINTS,
        "manual_bump_steps": [
            "1. Inspect tags/commits: git ls-remote --tags --heads https://github.com/storytold/craft-libs.git",
            "2. Diff consumed symbols in spectracraft-mono/src/report_export.rs against upstream changelog.",
            "3. Update explicit rev/tag pins for craft-pdf, craft-atomic, and craft-settings in spectracraft-mono/Cargo.toml and CRAFT_LIBS_PIN in report_export.rs.",
            "4. Run: cargo update -p craft-pdf -p craft-atomic -p craft-settings",
            "5. Rebuild release binaries and regenerate SOURCE-SHA256.json: cargo build --release --locked && ./target/release/spectracraft-qa source-inventory spectracraft-mono > spectracraft-mono/SOURCE-SHA256.json",
            "6. Run mandatory verification gate: cargo test --locked --release -p spectracraft-mono --test technical_report_sweep && cargo test --locked -p spectracraft-mono --lib && npx --yes tsx --test src/lib/*.test.ts"
        ]
    })
}

// ======================= DARK-MODE PDF EXPORT VIA CRAFT-PDF =======================

fn pdf_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let ascii = match ch {
            '—' | '–' => '-',
            '−' => '-',
            '·' => '|',
            '×' => 'x',
            '±' => '+',
            '→' => '>',
            c if c.is_ascii() && (c as u32) >= 32 && (c as u32) <= 126 => c,
            _ => ' ',
        };
        match ascii {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            c => out.push(c),
        }
    }
    out
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(3)).collect();
        t.push_str("...");
        t
    }
}

fn magma_rgb(u: f32) -> [u8; 3] {
    let u = u.clamp(0.0, 1.0);
    let (r, g, b) = if u < 0.25 {
        let t = u / 0.25;
        (14.0 + 38.0 * t, 17.0 + 20.0 * t, 28.0 + 96.0 * t)
    } else if u < 0.55 {
        let t = (u - 0.25) / 0.30;
        (52.0 + 138.0 * t, 37.0 + 32.0 * t, 124.0 + 44.0 * t)
    } else if u < 0.82 {
        let t = (u - 0.55) / 0.27;
        (190.0 + 60.0 * t, 69.0 + 125.0 * t, 168.0 - 110.0 * t)
    } else {
        let t = (u - 0.82) / 0.18;
        (250.0 + 5.0 * t, 194.0 + 58.0 * t, 58.0 + 180.0 * t)
    };
    [r as u8, g as u8, b as u8]
}

fn diverging_diff_rgb(diff_db: f32, max_abs: f32) -> [u8; 3] {
    let u = (diff_db / max_abs.max(1.0)).clamp(-1.0, 1.0);
    if u >= 0.0 {
        let r = 24.0 + 230.0 * u;
        let g = 28.0 + 152.0 * u;
        let b = 36.0 + 14.0 * u;
        [r as u8, g as u8, b as u8]
    } else {
        let t = -u;
        let r = 24.0 + 18.0 * t;
        let g = 28.0 + 178.0 * t;
        let b = 36.0 + 214.0 * t;
        [r as u8, g as u8, b as u8]
    }
}

fn spectrogram_to_rgb_bytes(grid: &[f32], cols: usize, rows: usize, db_range: [f32; 2]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(cols * rows * 3);
    let span = (db_range[1] - db_range[0]).max(1.0);
    for row in (0..rows).rev() {
        for col in 0..cols {
            let db = grid.get(row * cols + col).copied().unwrap_or(db_range[0]);
            let u = (db - db_range[0]) / span;
            rgb.extend_from_slice(&magma_rgb(u));
        }
    }
    rgb
}

fn diff_spectrogram_to_rgb_bytes(grid: &[f32], cols: usize, rows: usize, max_abs: f32) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(cols * rows * 3);
    for row in (0..rows).rev() {
        for col in 0..cols {
            let d = grid.get(row * cols + col).copied().unwrap_or(0.0);
            rgb.extend_from_slice(&diverging_diff_rgb(d, max_abs));
        }
    }
    rgb
}

fn pdf_text(
    stream: &mut String,
    font: &str,
    size: f64,
    rgb: (f64, f64, f64),
    x: f64,
    y: f64,
    text: &str,
) {
    let _ = writeln!(
        stream,
        "BT /{font} {size:.1} Tf {:.3} {:.3} {:.3} rg 1 0 0 1 {x:.2} {y:.2} Tm ({}) Tj ET",
        rgb.0,
        rgb.1,
        rgb.2,
        pdf_escape(text)
    );
}

fn pdf_rect_fill(stream: &mut String, rgb: (f64, f64, f64), x: f64, y: f64, w: f64, h: f64) {
    let _ = writeln!(
        stream,
        "{:.3} {:.3} {:.3} rg {x:.2} {y:.2} {w:.2} {h:.2} re f",
        rgb.0, rgb.1, rgb.2
    );
}

fn pdf_rect_stroke(stream: &mut String, rgb: (f64, f64, f64), x: f64, y: f64, w: f64, h: f64) {
    let _ = writeln!(
        stream,
        "0.6 w {:.3} {:.3} {:.3} RG {x:.2} {y:.2} {w:.2} {h:.2} re S",
        rgb.0, rgb.1, rgb.2
    );
}

/// Export a dark-mode-styled PDF 1.7 white paper with embedded plots using pinned `craft-pdf`
/// (`CraftPdfDocument` wrapper) and `atomic_write_bytes`.
pub fn export_report_pdf_craft(report: &TechnicalReport, out_path: &Path) -> Result<ExportReceipt> {
    let page_w = 612.0_f64;
    let page_h = 792.0_f64;
    let bg = (0.071, 0.078, 0.098);
    let card = (0.106, 0.118, 0.145);
    let border = (0.215, 0.243, 0.298);
    let fg = (0.910, 0.929, 0.961);
    let muted = (0.610, 0.655, 0.725);
    let cyan = (0.310, 0.780, 0.940);
    let amber = (0.950, 0.700, 0.280);
    let emerald = (0.298, 0.776, 0.675);

    // ===== PAGE 1: Dark-Mode White-Paper Text, Blind Before/After Tables & Comparison =====
    let mut p1 = String::with_capacity(16_384);
    pdf_rect_fill(&mut p1, bg, 0.0, 0.0, page_w, page_h);
    pdf_rect_fill(
        &mut p1,
        (0.095, 0.115, 0.155),
        0.0,
        page_h - 68.0,
        page_w,
        68.0,
    );
    pdf_rect_fill(&mut p1, cyan, 0.0, page_h - 70.0, page_w, 2.0);

    pdf_text(
        &mut p1,
        "F2",
        16.0,
        fg,
        36.0,
        page_h - 32.0,
        "SPECTRACRAFT TECHNICAL REPORT",
    );
    pdf_text(
        &mut p1,
        "F1",
        9.5,
        muted,
        36.0,
        page_h - 49.0,
        &truncate_chars(
            &format!(
                "{}  |  Pipeline: {}  |  {:.1} kHz mono  |  {:.2} s",
                report.title,
                report.pipeline_label,
                f64::from(report.before.sample_rate) / 1000.0,
                report.before.duration_s
            ),
            98,
        ),
    );
    pdf_text(
        &mut p1,
        "F3",
        7.5,
        emerald,
        36.0,
        page_h - 62.0,
        "BLIND PROTOCOL: Before analyzes raw input only | After analyzes final export only | Comparison lists significant numeric deltas only",
    );

    // Section 1: Comparison Summary
    let mut y = page_h - 90.0;
    pdf_text(
        &mut p1,
        "F2",
        11.0,
        cyan,
        36.0,
        y,
        "1. COMPARISON SUMMARY (SIGNIFICANT NUMERIC DELTAS)",
    );
    y -= 10.0;
    let delta_rows = report.comparison.deltas.len().min(9);
    let comp_h = 18.0 + delta_rows as f64 * 14.5;
    pdf_rect_fill(&mut p1, card, 36.0, y - comp_h, page_w - 72.0, comp_h);
    pdf_rect_stroke(&mut p1, border, 36.0, y - comp_h, page_w - 72.0, comp_h);
    let mut cy = y - 14.0;
    for d in report.comparison.deltas.iter().take(delta_rows) {
        pdf_text(
            &mut p1,
            "F2",
            8.0,
            fg,
            44.0,
            cy,
            &truncate_chars(&d.metric, 26),
        );
        pdf_text(
            &mut p1,
            "F3",
            7.8,
            emerald,
            185.0,
            cy,
            &truncate_chars(&d.summary, 76),
        );
        cy -= 14.5;
    }

    // Sections 2 & 3: Before (Blind) & After (Blind) Findings Cards
    let mut draw_findings_card = |heading: &str,
                                  findings: &[crate::technical_report::ReportFinding],
                                  max_rows: usize,
                                  top_y: f64|
     -> f64 {
        pdf_text(&mut p1, "F2", 11.0, cyan, 36.0, top_y, heading);
        let box_top = top_y - 10.0;
        let rows = findings.len().min(max_rows);
        let h = 16.0 + rows as f64 * 16.5;
        pdf_rect_fill(&mut p1, card, 36.0, box_top - h, page_w - 72.0, h);
        pdf_rect_stroke(&mut p1, border, 36.0, box_top - h, page_w - 72.0, h);
        let mut row_y = box_top - 14.0;
        for f in findings.iter().take(rows) {
            let tag_rgb = if f.tag == EvidenceTag::Measured {
                cyan
            } else {
                amber
            };
            pdf_text(
                &mut p1,
                "F3",
                7.2,
                tag_rgb,
                44.0,
                row_y,
                &format!("[{}]", f.tag.label().to_uppercase()),
            );
            pdf_text(
                &mut p1,
                "F2",
                8.0,
                fg,
                108.0,
                row_y,
                &truncate_chars(&f.category, 22),
            );
            pdf_text(
                &mut p1,
                "F1",
                7.8,
                fg,
                224.0,
                row_y,
                &truncate_chars(&f.value, 66),
            );
            row_y -= 16.5;
        }
        box_top - h
    };
    let after_b = draw_findings_card(
        "2. BEFORE ANALYSIS (BLIND - RAW INPUT ONLY)",
        &report.before.findings,
        10,
        y - comp_h - 18.0,
    );
    let _ = draw_findings_card(
        "3. AFTER ANALYSIS (BLIND - FINAL EXPORT ONLY)",
        &report.after.findings,
        9,
        after_b - 18.0,
    );

    pdf_text(
        &mut p1,
        "F3",
        7.2,
        muted,
        36.0,
        24.0,
        &format!(
            "Generated by SpectraCraft via craft-pdf ({CRAFT_LIBS_PIN_KIND} {CRAFT_LIBS_PIN_SHORT}) | Page 1 of 2 - Measured & Inferred Technical White Paper"
        ),
    );

    // ===== PAGE 2: Embedded High-Contrast Audio Plots =====
    let mut p2 = String::with_capacity(32_768);
    pdf_rect_fill(&mut p2, bg, 0.0, 0.0, page_w, page_h);
    pdf_rect_fill(
        &mut p2,
        (0.095, 0.115, 0.155),
        0.0,
        page_h - 48.0,
        page_w,
        48.0,
    );
    pdf_rect_fill(&mut p2, cyan, 0.0, page_h - 50.0, page_w, 2.0);
    pdf_text(
        &mut p2,
        "F2",
        13.0,
        fg,
        36.0,
        page_h - 30.0,
        "AUDIO BUFFER DIAGNOSTIC PLOTS (SHARED SCALES)",
    );
    pdf_text(
        &mut p2,
        "F3",
        7.8,
        muted,
        36.0,
        page_h - 42.0,
        &format!(
            "Waveform scale: +/-{:.3} linear | Spectrogram: 20 Hz - {:.0} Hz ({:.0} to {:.0} dBFS) | Diff: +/-{:.0} dB",
            report.plots.waveform_scale_linear,
            report.plots.max_hz,
            report.plots.spectrogram_db_range[0],
            report.plots.spectrogram_db_range[1],
            report.plots.diff_db_range[1]
        ),
    );

    let plot_x = 40.0;
    let plot_w = page_w - 80.0;
    let wf_y = page_h - 150.0;
    let wf_h = 76.0;
    pdf_text(
        &mut p2,
        "F2",
        9.0,
        cyan,
        plot_x,
        wf_y + wf_h + 6.0,
        "A. WAVEFORM OVERVIEW (ORIGINAL [CYAN] VS FINAL [EMERALD], SAME LINEAR SCALE)",
    );
    pdf_rect_fill(&mut p2, card, plot_x, wf_y, plot_w, wf_h);
    pdf_rect_stroke(&mut p2, border, plot_x, wf_y, plot_w, wf_h);
    let draw_wf = |stream: &mut String,
                   bins: &[[f32; 2]],
                   scale: f32,
                   rgb: (f64, f64, f64),
                   y0: f64,
                   h: f64| {
        if bins.is_empty() {
            return;
        }
        let mid = y0 + h * 0.5;
        let half = h * 0.44;
        let inv = 1.0 / scale.max(1e-4) as f64;
        let _ = writeln!(stream, "0.75 w {:.3} {:.3} {:.3} RG", rgb.0, rgb.1, rgb.2);
        for (i, &[lo, hi]) in bins.iter().enumerate() {
            let x = plot_x + (i as f64 + 0.5) * plot_w / bins.len() as f64;
            let y_lo = mid + (lo as f64 * inv).clamp(-1.0, 1.0) * half;
            let y_hi = mid + (hi as f64 * inv).clamp(-1.0, 1.0) * half;
            let _ = writeln!(stream, "{x:.2} {y_lo:.2} m {x:.2} {y_hi:.2} l S");
        }
    };
    draw_wf(
        &mut p2,
        &report.plots.waveform_before,
        report.plots.waveform_scale_linear,
        cyan,
        wf_y + wf_h * 0.5,
        wf_h * 0.5,
    );
    draw_wf(
        &mut p2,
        &report.plots.waveform_after,
        report.plots.waveform_scale_linear,
        emerald,
        wf_y,
        wf_h * 0.5,
    );

    let sp_h = 84.0;
    let mut sp_y = wf_y;
    for (title, xobj) in [
        (
            "B. SPECTROGRAM - ORIGINAL INPUT (LOG 20 HZ - 20 KHZ, -96 TO 0 DBFS)",
            "Im1",
        ),
        (
            "C. SPECTROGRAM - FINAL EXPORT (IDENTICAL LOG SCALE & DBFS FLOOR)",
            "Im2",
        ),
        (
            "D. DIFFERENCE SPECTROGRAM (FINAL - ORIGINAL: AMBER = BOOST, CYAN = CUT)",
            "Im3",
        ),
    ] {
        sp_y -= sp_h + 24.0;
        pdf_text(&mut p2, "F2", 9.0, cyan, plot_x, sp_y + sp_h + 6.0, title);
        pdf_rect_stroke(&mut p2, border, plot_x, sp_y, plot_w, sp_h);
        let _ = writeln!(
            p2,
            "q {plot_w:.2} 0 0 {sp_h:.2} {plot_x:.2} {sp_y:.2} cm /{xobj} Do Q"
        );
    }
    let sp3_y = sp_y;

    let bot_h = 112.0;
    let bot_y = sp3_y - bot_h - 26.0;
    let half_w = (plot_w - 16.0) * 0.5;
    pdf_text(
        &mut p2,
        "F2",
        8.5,
        cyan,
        plot_x,
        bot_y + bot_h + 6.0,
        "E. 1/3-OCTAVE SPECTRUM (25 HZ - 12.5 KHZ)",
    );
    pdf_rect_fill(&mut p2, card, plot_x, bot_y, half_w, bot_h);
    pdf_rect_stroke(&mut p2, border, plot_x, bot_y, half_w, bot_h);
    let nb = report.plots.third_octave.centers_hz.len().max(1);
    let bar_slot = (half_w - 12.0) / nb as f64;
    for i in 0..nb {
        let bx = plot_x + 6.0 + i as f64 * bar_slot;
        let b_db = report
            .plots
            .third_octave
            .before_db
            .get(i)
            .copied()
            .unwrap_or(-96.0);
        let a_db = report
            .plots
            .third_octave
            .after_db
            .get(i)
            .copied()
            .unwrap_or(-96.0);
        let ub = ((b_db + 96.0) / 96.0).clamp(0.0, 1.0) as f64;
        let ua = ((a_db + 96.0) / 96.0).clamp(0.0, 1.0) as f64;
        pdf_rect_fill(
            &mut p2,
            (0.28, 0.52, 0.78),
            bx,
            bot_y + 12.0,
            bar_slot * 0.42,
            ub * (bot_h - 20.0),
        );
        pdf_rect_fill(
            &mut p2,
            emerald,
            bx + bar_slot * 0.44,
            bot_y + 12.0,
            bar_slot * 0.42,
            ua * (bot_h - 20.0),
        );
    }
    pdf_text(
        &mut p2,
        "F3",
        6.5,
        muted,
        plot_x + 6.0,
        bot_y + 3.0,
        "25Hz    100Hz   200Hz    1kHz     4kHz    12.5kHz",
    );

    let rx = plot_x + half_w + 16.0;
    pdf_text(
        &mut p2,
        "F2",
        8.5,
        cyan,
        rx,
        bot_y + bot_h + 6.0,
        "F. UPPER-BAND (4-16 KHZ) COHERENCE & MODULATION",
    );
    pdf_rect_fill(&mut p2, card, rx, bot_y, half_w, bot_h);
    pdf_rect_stroke(&mut p2, border, rx, bot_y, half_w, bot_h);
    let draw_series = |stream: &mut String, ys: &[f32], rgb: (f64, f64, f64)| {
        if ys.len() < 2 {
            return;
        }
        let _ = writeln!(stream, "1.0 w {:.3} {:.3} {:.3} RG", rgb.0, rgb.1, rgb.2);
        for (i, &v) in ys.iter().enumerate() {
            let x = rx + 6.0 + (i as f64 / (ys.len() - 1) as f64) * (half_w - 12.0);
            let y = bot_y + 14.0 + (v.clamp(0.0, 1.0) as f64) * (bot_h - 22.0);
            let _ = writeln!(stream, "{x:.2} {y:.2} {}", if i == 0 { "m" } else { "l" });
        }
        let _ = writeln!(stream, "S");
    };
    draw_series(
        &mut p2,
        &report.plots.upper_band.before_phase_coherence,
        cyan,
    );
    draw_series(
        &mut p2,
        &report.plots.upper_band.after_phase_coherence,
        emerald,
    );
    draw_series(
        &mut p2,
        &report.plots.upper_band.after_modulation_depth,
        amber,
    );
    pdf_text(
        &mut p2,
        "F3",
        6.5,
        muted,
        rx + 6.0,
        bot_y + 3.0,
        "Cyan: Before Coh | Emerald: After Coh | Amber: After Mod",
    );

    pdf_text(
        &mut p2,
        "F3",
        7.2,
        muted,
        36.0,
        24.0,
        &format!(
            "Generated by SpectraCraft via craft-pdf ({CRAFT_LIBS_PIN_KIND} {CRAFT_LIBS_PIN_SHORT}) | Page 2 of 2 - Real Audio Buffer Plots"
        ),
    );

    let cols = report.plots.spectrogram_cols;
    let rows = report.plots.spectrogram_rows;
    let im1_rgb = spectrogram_to_rgb_bytes(
        &report.plots.spectrogram_before_db,
        cols,
        rows,
        report.plots.spectrogram_db_range,
    );
    let im2_rgb = spectrogram_to_rgb_bytes(
        &report.plots.spectrogram_after_db,
        cols,
        rows,
        report.plots.spectrogram_db_range,
    );
    let im3_rgb = diff_spectrogram_to_rgb_bytes(
        &report.plots.spectrogram_diff_db,
        cols,
        rows,
        report.plots.diff_db_range[1],
    );

    let mut doc = craft_pdf::classic::Writer::with_capacity(im1_rgb.len() * 3 + 64_000)?;
    doc.object(1, "<< /Type /Catalog /Pages 2 0 R >>")?;
    doc.object(2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>")?;
    doc.object(
        3,
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R >> >> /Contents 8 0 R >>",
    )?;
    doc.object(
        4,
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R >> /XObject << /Im1 10 0 R /Im2 11 0 R /Im3 12 0 R >> >> /Contents 9 0 R >>",
    )?;
    doc.object(5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")?;
    doc.object(
        6,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>",
    )?;
    doc.object(7, "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>")?;
    doc.stream(8, "", p1.as_bytes())?;
    doc.stream(9, "", p2.as_bytes())?;
    let im_dict = format!(
        "/Type /XObject /Subtype /Image /Width {cols} /Height {rows} /ColorSpace /DeviceRGB /BitsPerComponent 8"
    );
    doc.stream(10, &im_dict, &im1_rgb)?;
    doc.stream(11, &im_dict, &im2_rgb)?;
    doc.stream(12, &im_dict, &im3_rgb)?;
    doc.object(
        13,
        &format!(
            "<< /Title ({}) /Creator (SpectraCraft + craft-pdf) >>",
            pdf_escape(&format!("SpectraCraft Technical Report - {}", report.title))
        ),
    )?;

    let pdf_bytes = doc.finish(1, Some(13))?;
    atomic_write_bytes(out_path, &pdf_bytes)?;

    Ok(ExportReceipt {
        requested_format: "pdf".into(),
        actual_format: "pdf".into(),
        extension: "pdf".into(),
        content_type: "application/pdf".into(),
        engine: format!(
            "craft-pdf (pinned Git {CRAFT_LIBS_PIN_KIND} {CRAFT_LIBS_PIN_SHORT} via report_export.rs)"
        ),
        fallback_used: false,
        fallback_note: None,
        bytes: pdf_bytes.len(),
        sha256: digest::sha256(&pdf_bytes),
    })
}

/// Export readable Markdown white paper (including embedded numeric plot summaries),
/// also serving as the documented fallback when `docx` (`craft-docx`) is requested.
pub fn export_report_markdown(
    report: &TechnicalReport,
    out_path: &Path,
    requested_format: &str,
) -> Result<ExportReceipt> {
    let is_docx_fallback = requested_format.eq_ignore_ascii_case("docx");
    let mut doc = String::with_capacity(report.whitepaper_markdown.len() + 2048);
    if is_docx_fallback {
        doc.push_str(
            "> **Fallback Notice**: `craft-docx` from `storytold/craft-libs` is not linked in this build (deferred pending `craft-wordmodel`/`craft-opc` adoption; see `CRAFT_LIBS_MAINTENANCE.md`). Exported readable Markdown technical report instead; dark-mode PDF with embedded plots is available via pinned `craft-pdf`.\n\n",
        );
    }
    doc.push_str(&report.whitepaper_markdown);
    doc.push_str("\n## 5. Embedded Plot Data Summary (Derived From Audio Buffers)\n\n");
    let _ = writeln!(
        doc,
        "- **Waveform Overview**: {} bins per channel, shared linear scale `±{:.4}`.",
        report.plots.waveform_bins, report.plots.waveform_scale_linear
    );
    let _ = writeln!(
        doc,
        "- **Spectrograms (Original, Final, Difference)**: `{} × {}` log-frequency grid (`20 Hz` – `{:.0} Hz`, shared scale `{:.0}` to `{:.0} dBFS`, diff `±{:.0} dB`).",
        report.plots.spectrogram_cols,
        report.plots.spectrogram_rows,
        report.plots.max_hz,
        report.plots.spectrogram_db_range[0],
        report.plots.spectrogram_db_range[1],
        report.plots.diff_db_range[1]
    );
    doc.push_str("\n### 1/3-Octave Spectrum Table (dBFS)\n\n| Band (Hz) | Before (dBFS) | After (dBFS) | Delta (dB) |\n| --- | --- | --- | --- |\n");
    for i in 0..report.plots.third_octave.centers_hz.len() {
        let _ = writeln!(
            doc,
            "| {:.1} | {:.2} | {:.2} | {:+.2} |",
            report.plots.third_octave.centers_hz[i],
            report
                .plots
                .third_octave
                .before_db
                .get(i)
                .copied()
                .unwrap_or(-96.0),
            report
                .plots
                .third_octave
                .after_db
                .get(i)
                .copied()
                .unwrap_or(-96.0),
            report
                .plots
                .third_octave
                .delta_db
                .get(i)
                .copied()
                .unwrap_or(0.0)
        );
    }

    let bytes = doc.as_bytes();
    atomic_write_bytes(out_path, bytes)?;

    Ok(ExportReceipt {
        requested_format: requested_format.to_ascii_lowercase(),
        actual_format: "markdown".into(),
        extension: "md".into(),
        content_type: "text/markdown; charset=utf-8".into(),
        engine: if is_docx_fallback {
            "markdown fallback (craft-docx deferred; isolated in report_export.rs)".into()
        } else {
            format!(
                "SpectraCraft Markdown white-paper exporter + craft-atomic ({CRAFT_LIBS_PIN_SHORT})"
            )
        },
        fallback_used: is_docx_fallback,
        fallback_note: if is_docx_fallback {
            Some(
                "craft-docx is not linked in this build; exported readable Markdown report instead (dark-mode PDF with embedded plots is available via pinned craft-pdf)."
                    .into(),
            )
        } else {
            None
        },
        bytes: bytes.len(),
        sha256: digest::sha256(bytes),
    })
}

pub fn export_report(
    report: &TechnicalReport,
    out_path: &Path,
    format: &str,
) -> Result<ExportReceipt> {
    match format.to_ascii_lowercase().as_str() {
        "pdf" => export_report_pdf_craft(report, out_path),
        "md" | "markdown" => export_report_markdown(report, out_path, "markdown"),
        "docx" => export_report_markdown(report, out_path, "docx"),
        other => Err(format!(
            "Unsupported report export format '{other}'; use pdf, markdown, or docx"
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn craft_libs_pins_are_explicit_and_non_floating() {
        assert!(!CRAFT_LIBS_PINS.is_empty());
        for pin in CRAFT_LIBS_PINS {
            assert_eq!(pin.git, CRAFT_LIBS_REPO);
            assert!(
                pin.pin_kind == "rev" || pin.pin_kind == "tag",
                "craft-libs must never track a floating branch"
            );
            assert!(!pin.pin_value.is_empty());
            assert!(!pin.symbols_used.is_empty());
        }
        let status = check_craft_libs_status(false);
        assert_eq!(status["auto_update_enabled"], false);
        assert_eq!(status["fail_build_on_newer_tag"], false);
        assert_eq!(
            status["isolation_module"],
            "spectracraft-mono/src/report_export.rs"
        );
    }
}
