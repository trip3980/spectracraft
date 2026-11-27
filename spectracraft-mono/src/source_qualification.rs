//! Manifest-driven recorded-source qualification runner and independent Rust scorer.
//!
//! Enforces strict separation between:
//! - Catalog presence & renderer class
//! - Synthetic engineering controls vs real recorded performances
//! - Technical contract gates vs musical targets vs human listening review (`NOT RUN`)
//! - Missing local recording assets (`BLOCKED`, never silently substituted)

use crate::{
    digest,
    pipeline::{self, PostDryPipelineConfig},
    qa,
    reperform::{self, EvidenceStatus, Performance, VoicingControls},
    studio_catalog::{self, mic_catalog, surface_catalog, SourceEntry},
    write_wav, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    f64::consts::TAU,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QualificationManifest {
    pub schema_version: String,
    pub manifest_version: String,
    pub baseline_commit: String,
    pub cases: Vec<FixtureCase>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FixtureCase {
    pub case_id: String,
    pub source_catalog_id: String,
    pub display_family: String,
    pub provenance_class: String,
    pub partition: String,
    pub performance_group: String,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub dataset: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub asset_identifier: Option<String>,
    #[serde(default)]
    pub attribution: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub performer_identity: Option<String>,
    #[serde(default)]
    pub upstream_original_sha256: Option<String>,
    #[serde(default)]
    pub local_wav_env_var: Option<String>,
    #[serde(default)]
    pub local_wav_path: Option<String>,
    #[serde(default)]
    pub expected_wav_sha256: Option<String>,
    #[serde(default)]
    pub expected_sample_rate: Option<u32>,
    #[serde(default)]
    pub expected_channels: Option<u16>,
    #[serde(default)]
    pub expected_frame_count: Option<usize>,
    #[serde(default)]
    pub required_application_surfaces: Vec<String>,
    #[serde(default)]
    pub expected_events: Vec<ExpectedEvent>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExpectedEvent {
    pub kind: String,
    pub onset_s: f64,
    pub offset_s: f64,
    #[serde(default)]
    pub hz: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QualificationCriteria {
    pub schema_version: String,
    pub criteria_version: String,
    pub technical_gates: TechnicalGateCriteria,
    pub musical_targets: MusicalTargetCriteria,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TechnicalGateCriteria {
    pub require_original_sha256_unchanged: bool,
    pub require_source_free_cue_roundtrip: bool,
    pub require_deterministic_repeat_render: bool,
    pub require_all_samples_finite: bool,
    pub max_abs_sample_headroom: f64,
    pub require_exact_frame_count_preservation: bool,
    pub min_non_silent_rms: f64,
    pub require_stage2_to_5_identity_bypass: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MusicalTargetCriteria {
    pub pitched_notes: PitchedNoteCriteria,
    pub unpitched_hits: UnpitchedHitCriteria,
    pub false_voicing: FalseVoicingCriteria,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PitchedNoteCriteria {
    pub min_precision: f64,
    pub min_recall: f64,
    pub onset_tolerance_ms: f64,
    pub pitch_tolerance_cents: f64,
    pub offset_tolerance_min_ms: f64,
    pub offset_tolerance_duration_fraction: f64,
    pub allow_octave_folding: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UnpitchedHitCriteria {
    pub min_precision: f64,
    pub min_recall: f64,
    pub onset_tolerance_ms: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FalseVoicingCriteria {
    pub max_false_voicing_unvoiced_rate: f64,
    pub max_false_voicing_silent_rate: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CaseQualificationResult {
    pub case_id: String,
    pub source_catalog_id: String,
    pub display_family: String,
    pub provenance_class: String,
    pub partition: String,
    pub analyzer_used: String,
    pub renderer_model_id: String,
    pub technical_status: String,
    pub musical_status: String,
    pub human_review_status: String,
    pub overall_automated_status: String,
    pub detail: String,
    pub sample_rate: Option<u32>,
    pub frame_count: Option<usize>,
    pub original_sha256: Option<String>,
    pub cue_packet_sha256: Option<String>,
    pub dry_output_sha256: Option<String>,
    pub pitched_precision: Option<f64>,
    pub pitched_recall: Option<f64>,
    pub pitched_offset_precision: Option<f64>,
    pub pitched_offset_recall: Option<f64>,
    pub unpitched_precision: Option<f64>,
    pub unpitched_recall: Option<f64>,
    pub false_voicing_silent_rate: Option<f64>,
    pub artifacts_dir: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct QualificationRunReport {
    pub schema_version: String,
    pub manifest_version: String,
    pub criteria_version: String,
    pub total_cases: usize,
    pub pass_count: usize,
    pub fail_count: usize,
    pub blocked_count: usize,
    pub not_run_count: usize,
    pub all_required_automated_passed: bool,
    pub human_review_status: String,
    pub cases: Vec<CaseQualificationResult>,
}

fn find_catalog_source<'a>(sources: &'a [SourceEntry], id: &str) -> Option<(usize, &'a SourceEntry)> {
    sources.iter().enumerate().find(|(_, s)| s.id == id)
}

/// Deterministic built-in synthetic engineering controls (never substitute for real recordings).
fn synthesize_builtin_control(asset_id: &str, sample_rate: u32, frame_count: usize) -> Option<Vec<f32>> {
    let fs = f64::from(sample_rate);
    match asset_id {
        "builtin:ctrl_guitar_plucked_arpeggio" => {
            let notes: [(f64, f64, f64); 4] = [
                (0.15, 0.65, 196.0),
                (0.70, 1.20, 246.94),
                (1.25, 1.75, 293.66),
                (1.80, 2.40, 392.0),
            ];
            let mut out = vec![0.0_f32; frame_count];
            for (onset, offset, hz) in notes {
                let start = (onset * fs).round() as usize;
                let end = ((offset * fs).round() as usize).min(frame_count);
                for i in start..end {
                    let lt = (i - start) as f64 / fs;
                    let attack = (lt / 0.008).min(1.0);
                    let decay = (-lt / 0.35).exp();
                    let rel = if i + 480 > end {
                        (end - i) as f64 / 480.0
                    } else {
                        1.0
                    };
                    let val = 0.28
                        * attack
                        * decay
                        * rel
                        * ((TAU * hz * lt).sin()
                            + 0.48 * (TAU * 2.0 * hz * lt).sin()
                            + 0.22 * (TAU * 3.0 * hz * lt).sin());
                    out[i] += val as f32;
                }
            }
            Some(out)
        }
        "builtin:ctrl_bass_low_register" => {
            let notes: [(f64, f64, f64); 2] = [(0.15, 0.90, 41.20), (1.00, 1.75, 55.0)];
            let mut out = vec![0.0_f32; frame_count];
            for (onset, offset, hz) in notes {
                let start = (onset * fs).round() as usize;
                let end = ((offset * fs).round() as usize).min(frame_count);
                for i in start..end {
                    let lt = (i - start) as f64 / fs;
                    let env = (lt / 0.012).min(1.0) * (-lt / 0.55).exp();
                    let val = 0.35
                        * env
                        * ((TAU * hz * lt).sin() + 0.35 * (TAU * 2.0 * hz * lt).sin());
                    out[i] += val as f32;
                }
            }
            Some(out)
        }
        "builtin:ctrl_vocal_sustained_vowel" => {
            let notes: [(f64, f64, f64); 2] = [(0.15, 0.95, 440.0), (1.05, 1.85, 523.25)];
            let mut out = vec![0.0_f32; frame_count];
            for (onset, offset, hz) in notes {
                let start = (onset * fs).round() as usize;
                let end = ((offset * fs).round() as usize).min(frame_count);
                for i in start..end {
                    let lt = (i - start) as f64 / fs;
                    let attack = (lt / 0.03).min(1.0);
                    let rel = if i + 960 > end {
                        (end - i) as f64 / 960.0
                    } else {
                        1.0
                    };
                    let val = 0.28
                        * attack
                        * rel
                        * ((TAU * hz * lt).sin()
                            + 0.42 * (TAU * 2.0 * hz * lt).sin()
                            + 0.18 * (TAU * 3.0 * hz * lt).sin());
                    out[i] += val as f32;
                }
            }
            Some(out)
        }
        "builtin:ctrl_unpitched_percussion_clicks" => {
            let onsets = [0.20_f64, 0.70, 1.20];
            let mut out = vec![0.0_f32; frame_count];
            let mut rng = 0x517c_c1b7_2722_0a95_u64;
            for onset in onsets {
                let start = (onset * fs).round() as usize;
                let len = (0.08 * fs).round() as usize;
                let end = (start + len).min(frame_count);
                for i in start..end {
                    let lt = (i - start) as f64 / fs;
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    let u = ((rng >> 32) as f64 / (u32::MAX as f64)) * 2.0 - 1.0;
                    out[i] += (0.45 * (-lt / 0.015).exp() * u) as f32;
                }
            }
            Some(out)
        }
        "builtin:ctrl_true_silence" => Some(vec![0.0_f32; frame_count]),
        _ => None,
    }
}

fn cents_diff(a_hz: f64, b_hz: f64) -> f64 {
    if a_hz <= 0.0 || b_hz <= 0.0 {
        return f64::INFINITY;
    }
    1200.0 * (a_hz / b_hz).log2().abs()
}

/// Independent one-to-one pitched event matcher (no octave folding, no DTW).
fn score_pitched_events(
    expected: &[&ExpectedEvent],
    cues: &Performance,
    criteria: &PitchedNoteCriteria,
) -> (f64, f64, f64, f64) {
    if expected.is_empty() && cues.notes.is_empty() {
        return (1.0, 1.0, 1.0, 1.0);
    }
    if expected.is_empty() {
        return (0.0, 1.0, 0.0, 1.0);
    }
    if cues.notes.is_empty() {
        return (1.0, 0.0, 1.0, 0.0);
    }

    let onset_tol_s = criteria.onset_tolerance_ms / 1000.0;
    let mut used_cand = vec![false; cues.notes.len()];
    let mut matched_onset = 0usize;
    let mut matched_offset = 0usize;

    for exp in expected {
        let Some(exp_hz) = exp.hz else {
            continue;
        };
        let mut best: Option<(usize, f64)> = None;
        for (idx, note) in cues.notes.iter().enumerate() {
            if used_cand[idx] || note.hz.is_empty() {
                continue;
            }
            let onset_err = (note.start - exp.onset_s).abs();
            if onset_err > onset_tol_s {
                continue;
            }
            let mut sorted_hz: Vec<f64> = note.hz.iter().copied().filter(|v| *v > 0.0).collect();
            if sorted_hz.is_empty() {
                continue;
            }
            sorted_hz.sort_by(|a, b| a.total_cmp(b));
            let med_hz = sorted_hz[sorted_hz.len() / 2];
            let cents = cents_diff(med_hz, exp_hz);
            if cents <= criteria.pitch_tolerance_cents {
                let cost = onset_err * 1000.0 + cents;
                if best.map_or(true, |(_, c)| cost < c) {
                    best = Some((idx, cost));
                }
            }
        }
        if let Some((idx, _)) = best {
            used_cand[idx] = true;
            matched_onset += 1;
            let dur = (exp.offset_s - exp.onset_s).max(0.0);
            let off_tol = (criteria.offset_tolerance_min_ms / 1000.0)
                .max(dur * criteria.offset_tolerance_duration_fraction);
            if (cues.notes[idx].end - exp.offset_s).abs() <= off_tol {
                matched_offset += 1;
            }
        }
    }

    let prec = matched_onset as f64 / cues.notes.len() as f64;
    let rec = matched_onset as f64 / expected.len() as f64;
    let off_prec = matched_offset as f64 / cues.notes.len() as f64;
    let off_rec = matched_offset as f64 / expected.len() as f64;
    (prec, rec, off_prec, off_rec)
}

/// Independent RMS-envelope transient detector for unpitched percussion scoring.
fn detect_energy_onsets_s(samples: &[f32], rate: u32) -> Vec<f64> {
    let hop = ((rate as usize) / 200).max(1); // 5 ms hop
    let win = hop * 2;
    let mut energies = Vec::new();
    let mut pos = 0usize;
    while pos + win <= samples.len() {
        let rms = (samples[pos..pos + win]
            .iter()
            .map(|&s| f64::from(s) * f64::from(s))
            .sum::<f64>()
            / win as f64)
            .sqrt();
        energies.push(rms);
        pos += hop;
    }
    let mut onsets = Vec::new();
    let min_gap_frames = 8; // 40 ms refractory
    let mut last_idx: Option<usize> = None;
    for i in 1..energies.len() {
        let prev = energies[i - 1];
        let curr = energies[i];
        if curr > 0.025 && curr > prev * 2.5 {
            if last_idx.map_or(true, |l| i >= l + min_gap_frames) {
                onsets.push((i * hop) as f64 / f64::from(rate));
                last_idx = Some(i);
            }
        }
    }
    onsets
}

fn score_unpitched_events(
    expected: &[&ExpectedEvent],
    dry: &[f32],
    rate: u32,
    criteria: &UnpitchedHitCriteria,
) -> (f64, f64) {
    let detected = detect_energy_onsets_s(dry, rate);
    if expected.is_empty() && detected.is_empty() {
        return (1.0, 1.0);
    }
    if expected.is_empty() {
        return (0.0, 1.0);
    }
    if detected.is_empty() {
        return (1.0, 0.0);
    }
    let tol_s = criteria.onset_tolerance_ms / 1000.0;
    let mut used = vec![false; detected.len()];
    let mut matched = 0usize;
    for exp in expected {
        let mut best: Option<(usize, f64)> = None;
        for (idx, &t) in detected.iter().enumerate() {
            if used[idx] {
                continue;
            }
            let err = (t - exp.onset_s).abs();
            if err <= tol_s && best.map_or(true, |(_, e)| err < e) {
                best = Some((idx, err));
            }
        }
        if let Some((idx, _)) = best {
            used[idx] = true;
            matched += 1;
        }
    }
    (
        matched as f64 / detected.len() as f64,
        matched as f64 / expected.len() as f64,
    )
}

fn resolve_local_wav_path(case: &FixtureCase) -> Option<PathBuf> {
    if let Some(env_var) = &case.local_wav_env_var {
        if let Some(val) = std::env::var_os(env_var) {
            let p = PathBuf::from(val);
            if !p.as_os_str().is_empty() {
                return Some(p);
            }
        }
    }
    case.local_wav_path
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn f32_slice_sha256(samples: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for &s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    digest::sha256(&bytes)
}

pub fn run_qualification_campaign(
    manifest_path: &Path,
    criteria_path: &Path,
    out_dir: &Path,
    case_filter: Option<&str>,
) -> Result<QualificationRunReport> {
    let manifest: QualificationManifest =
        serde_json::from_str(&std::fs::read_to_string(manifest_path)?)?;
    let criteria: QualificationCriteria =
        serde_json::from_str(&std::fs::read_to_string(criteria_path)?)?;
    let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
    let mics = mic_catalog();
    let surfaces = surface_catalog();

    std::fs::create_dir_all(out_dir)?;

    let mut results = Vec::new();
    for case in &manifest.cases {
        if let Some(filter) = case_filter {
            if case.case_id != filter {
                continue;
            }
        }
        let res = evaluate_case(case, &criteria, &sources, &mics, &surfaces, out_dir)?;
        results.push(res);
    }

    if let Some(filter) = case_filter {
        if results.is_empty() {
            return Err(format!("Qualification case '{filter}' not found in manifest").into());
        }
    }

    let pass_count = results
        .iter()
        .filter(|r| r.overall_automated_status == "PASS")
        .count();
    let fail_count = results
        .iter()
        .filter(|r| r.overall_automated_status == "FAIL")
        .count();
    let blocked_count = results
        .iter()
        .filter(|r| r.overall_automated_status == "BLOCKED")
        .count();
    let not_run_count = results
        .iter()
        .filter(|r| r.overall_automated_status == "NOT RUN")
        .count();

    let report = QualificationRunReport {
        schema_version: "1.0.0".to_string(),
        manifest_version: manifest.manifest_version,
        criteria_version: criteria.criteria_version,
        total_cases: results.len(),
        pass_count,
        fail_count,
        blocked_count,
        not_run_count,
        all_required_automated_passed: fail_count == 0 && blocked_count == 0 && not_run_count == 0 && !results.is_empty(),
        human_review_status: "NOT RUN".to_string(),
        cases: results,
    };

    let report_path = out_dir.join("qualification-report.json");
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
    Ok(report)
}

fn evaluate_case(
    case: &FixtureCase,
    criteria: &QualificationCriteria,
    sources: &[SourceEntry],
    mics: &[studio_catalog::MicEntry],
    surfaces: &[studio_catalog::SurfaceEntry],
    out_dir: &Path,
) -> Result<CaseQualificationResult> {
    let Some((source_idx, source_entry)) = find_catalog_source(sources, &case.source_catalog_id)
    else {
        return Ok(CaseQualificationResult {
            case_id: case.case_id.clone(),
            source_catalog_id: case.source_catalog_id.clone(),
            display_family: case.display_family.clone(),
            provenance_class: case.provenance_class.clone(),
            partition: case.partition.clone(),
            analyzer_used: "none".to_string(),
            renderer_model_id: "unknown".to_string(),
            technical_status: "FAIL".to_string(),
            musical_status: "NOT RUN".to_string(),
            human_review_status: "NOT RUN".to_string(),
            overall_automated_status: "FAIL".to_string(),
            detail: format!("Catalog ID '{}' not found in source catalog", case.source_catalog_id),
            sample_rate: None,
            frame_count: None,
            original_sha256: None,
            cue_packet_sha256: None,
            dry_output_sha256: None,
            pitched_precision: None,
            pitched_recall: None,
            pitched_offset_precision: None,
            pitched_offset_recall: None,
            unpitched_precision: None,
            unpitched_recall: None,
            false_voicing_silent_rate: None,
            artifacts_dir: None,
        });
    };

    let (samples, rate, orig_sha_before, orig_file_path): (Vec<f32>, u32, String, Option<PathBuf>) =
        if case.provenance_class == "synthetic_control" {
            let asset_id = case.asset_identifier.as_deref().unwrap_or("");
            let rate = case.expected_sample_rate.unwrap_or(48_000);
            let frames = case.expected_frame_count.unwrap_or(rate as usize * 2);
            let Some(buf) = synthesize_builtin_control(asset_id, rate, frames) else {
                return Ok(blocked_case_result(
                    case,
                    source_entry,
                    format!("Unknown built-in synthetic control '{asset_id}'"),
                ));
            };
            let sha = f32_slice_sha256(&buf);
            (buf, rate, sha, None)
        } else {
            // Real recording: NEVER silently substitute another file or synthetic control!
            let Some(wav_path) = resolve_local_wav_path(case) else {
                return Ok(blocked_case_result(
                    case,
                    source_entry,
                    format!(
                        "Real recording asset not supplied locally (set {} or local_wav_path in manifest); synthetic fallback forbidden.",
                        case.local_wav_env_var.as_deref().unwrap_or("local_wav_path")
                    ),
                ));
            };
            if !wav_path.exists() {
                return Ok(blocked_case_result(
                    case,
                    source_entry,
                    format!(
                        "Real recording asset missing on disk at '{}'; synthetic fallback forbidden.",
                        wav_path.display()
                    ),
                ));
            }
            let raw_bytes = std::fs::read(&wav_path)?;
            let actual_sha = digest::sha256(&raw_bytes);
            if let Some(expected_sha) = &case.expected_wav_sha256 {
                if !expected_sha.eq_ignore_ascii_case(&actual_sha) {
                    return Ok(blocked_case_result(
                        case,
                        source_entry,
                        format!(
                            "Original WAV SHA-256 mismatch for '{}': expected {}, got {}",
                            wav_path.display(),
                            expected_sha,
                            actual_sha
                        ),
                    ));
                }
            }
            let (spec, buf) = qa::read_audio(&wav_path)?;
            if spec.channels != 1 {
                return Ok(blocked_case_result(
                    case,
                    source_entry,
                    format!(
                        "Expected explicit mono WAV derivative, got {} channels; silent downmix forbidden.",
                        spec.channels
                    ),
                ));
            }
            (buf, spec.sample_rate, actual_sha, Some(wav_path))
        };

    // Execute shared Rust analysis + dry synthesis
    let cues = reperform::analyze_guitar(&samples, rate)?;
    let cues_json = serde_json::to_string_pretty(&cues)?;
    let cue_sha = digest::sha256(cues_json.as_bytes());
    let voicing = VoicingControls::default();

    // Source-free round-trip render from serialized cues only
    let cues_reloaded: Performance = serde_json::from_str(&cues_json)?;
    let dry_a = cues_reloaded.synthesize_dry_for_source_with_voicing(source_entry, &voicing)?;
    let dry_b = cues_reloaded.synthesize_dry_for_source_with_voicing(source_entry, &voicing)?;

    // Technical gates
    let mut tech_failures = Vec::new();
    if criteria.technical_gates.require_original_sha256_unchanged {
        if let Some(ref p) = orig_file_path {
            let after_sha = digest::sha256(&std::fs::read(p)?);
            if after_sha != orig_sha_before {
                tech_failures.push("Original file SHA-256 changed during analysis/render".to_string());
            }
        }
    }
    if criteria.technical_gates.require_deterministic_repeat_render && dry_a != dry_b {
        tech_failures.push("Repeated source-free dry renders were not sample-identical".to_string());
    }
    if criteria.technical_gates.require_all_samples_finite
        && dry_a.iter().any(|s| !s.is_finite())
    {
        tech_failures.push("Dry output contains NaN or Inf sample".to_string());
    }
    let peak = dry_a
        .iter()
        .fold(0.0_f64, |acc, &s| acc.max(f64::from(s).abs()));
    if peak > criteria.technical_gates.max_abs_sample_headroom + 1e-6 {
        tech_failures.push(format!(
            "Dry peak {:.4} exceeds declared headroom {:.4}",
            peak, criteria.technical_gates.max_abs_sample_headroom
        ));
    }
    if criteria.technical_gates.require_exact_frame_count_preservation
        && dry_a.len() != samples.len()
    {
        tech_failures.push(format!(
            "Dry frame count {} != input frame count {}",
            dry_a.len(),
            samples.len()
        ));
    }
    let is_silence_case = case.case_id.contains("silence");
    let dry_rms = if dry_a.is_empty() {
        0.0
    } else {
        (dry_a.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / dry_a.len() as f64)
            .sqrt()
    };
    if !is_silence_case && dry_rms < criteria.technical_gates.min_non_silent_rms {
        tech_failures.push(format!(
            "Dry RMS {:.6} below non-silent minimum {:.6}",
            dry_rms, criteria.technical_gates.min_non_silent_rms
        ));
    }

    // Stage 2–5 bypass identity check
    if criteria.technical_gates.require_stage2_to_5_identity_bypass {
        let mut cfg = PostDryPipelineConfig::default();
        cfg.rig.source_index = source_idx;
        cfg.mic_enabled = false;
        cfg.room_enabled = false;
        cfg.plugin_slot.enabled = false;
        let post = pipeline::run_post_dry_pipeline(&dry_a, rate, &cfg, sources, mics, surfaces);
        if post.samples != dry_a {
            tech_failures.push("Bypassed Stages 3-5 did not return bit-identical dry output".to_string());
        }
    }

    // Save retained case artifacts (cue packet + dry WAV)
    let case_dir = out_dir.join(&case.case_id);
    std::fs::create_dir_all(&case_dir)?;
    std::fs::write(case_dir.join("cues.json"), &cues_json)?;
    let dry_wav_path = case_dir.join("dry_candidate.wav");
    if dry_wav_path.exists() {
        let _ = std::fs::remove_file(&dry_wav_path);
    }
    write_wav(rate, &dry_a, &dry_wav_path)?;
    let dry_sha = f32_slice_sha256(&dry_a);

    // Musical scoring
    let pitched_expected: Vec<&ExpectedEvent> = case
        .expected_events
        .iter()
        .filter(|e| e.kind == "pitched")
        .collect();
    let unpitched_expected: Vec<&ExpectedEvent> = case
        .expected_events
        .iter()
        .filter(|e| e.kind == "unpitched")
        .collect();

    let mut pitched_precision = None;
    let mut pitched_recall = None;
    let mut pitched_offset_precision = None;
    let mut pitched_offset_recall = None;
    let mut unpitched_precision = None;
    let mut unpitched_recall = None;
    let mut false_voicing_silent_rate = None;

    let mut musical_failures = Vec::new();
    let musical_status = if is_silence_case {
        let total_frames = cues.frame_evidence.len().max(1);
        let voiced_frames = cues
            .frame_evidence
            .iter()
            .filter(|f| f.status == EvidenceStatus::Supported && f.selected_hz.is_some())
            .count();
        let fv_rate = voiced_frames as f64 / total_frames as f64;
        false_voicing_silent_rate = Some(fv_rate);
        if fv_rate > criteria.musical_targets.false_voicing.max_false_voicing_silent_rate
            || !cues.notes.is_empty()
            || dry_rms > 1e-6
        {
            musical_failures.push(format!(
                "True-silence control produced false voicing rate {:.4}, notes={}, dry_rms={:.6}",
                fv_rate,
                cues.notes.len(),
                dry_rms
            ));
            "FAIL"
        } else {
            "PASS"
        }
    } else if !pitched_expected.is_empty() {
        let (p, r, op, or) =
            score_pitched_events(&pitched_expected, &cues, &criteria.musical_targets.pitched_notes);
        pitched_precision = Some(p);
        pitched_recall = Some(r);
        pitched_offset_precision = Some(op);
        pitched_offset_recall = Some(or);
        if p < criteria.musical_targets.pitched_notes.min_precision
            || r < criteria.musical_targets.pitched_notes.min_recall
        {
            musical_failures.push(format!(
                "Pitched note score below target (precision={:.3}, recall={:.3}; required >= {:.2})",
                p, r, criteria.musical_targets.pitched_notes.min_precision
            ));
            "FAIL"
        } else {
            "PASS"
        }
    } else if !unpitched_expected.is_empty() {
        let (p, r) = score_unpitched_events(
            &unpitched_expected,
            &dry_a,
            rate,
            &criteria.musical_targets.unpitched_hits,
        );
        unpitched_precision = Some(p);
        unpitched_recall = Some(r);
        if p < criteria.musical_targets.unpitched_hits.min_precision
            || r < criteria.musical_targets.unpitched_hits.min_recall
        {
            musical_failures.push(format!(
                "Unpitched hit score below target (precision={:.3}, recall={:.3}; required >= {:.2})",
                p, r, criteria.musical_targets.unpitched_hits.min_precision
            ));
            "FAIL"
        } else {
            "PASS"
        }
    } else {
        "NOT RUN"
    };

    let technical_status = if tech_failures.is_empty() {
        "PASS"
    } else {
        "FAIL"
    };
    let overall = if technical_status == "FAIL" || musical_status == "FAIL" {
        "FAIL"
    } else if musical_status == "NOT RUN" {
        "NOT RUN"
    } else {
        "PASS"
    };

    let mut detail_parts = Vec::new();
    detail_parts.extend(tech_failures);
    detail_parts.extend(musical_failures);
    let detail = if detail_parts.is_empty() {
        format!(
            "Technical gates passed; musical status {}; human listening remains NOT RUN.",
            musical_status
        )
    } else {
        detail_parts.join("; ")
    };

    Ok(CaseQualificationResult {
        case_id: case.case_id.clone(),
        source_catalog_id: case.source_catalog_id.clone(),
        display_family: case.display_family.clone(),
        provenance_class: case.provenance_class.clone(),
        partition: case.partition.clone(),
        analyzer_used: "reperform::analyze_guitar (75-1400 Hz, 65 Hz HPF)".to_string(),
        renderer_model_id: source_entry.regen_model.model_id.to_string(),
        technical_status: technical_status.to_string(),
        musical_status: musical_status.to_string(),
        human_review_status: "NOT RUN".to_string(),
        overall_automated_status: overall.to_string(),
        detail,
        sample_rate: Some(rate),
        frame_count: Some(samples.len()),
        original_sha256: Some(orig_sha_before),
        cue_packet_sha256: Some(cue_sha),
        dry_output_sha256: Some(dry_sha),
        pitched_precision,
        pitched_recall,
        pitched_offset_precision,
        pitched_offset_recall,
        unpitched_precision,
        unpitched_recall,
        false_voicing_silent_rate,
        artifacts_dir: Some(case_dir.display().to_string()),
    })
}

fn blocked_case_result(
    case: &FixtureCase,
    source_entry: &SourceEntry,
    reason: String,
) -> CaseQualificationResult {
    CaseQualificationResult {
        case_id: case.case_id.clone(),
        source_catalog_id: case.source_catalog_id.clone(),
        display_family: case.display_family.clone(),
        provenance_class: case.provenance_class.clone(),
        partition: case.partition.clone(),
        analyzer_used: "not_executed".to_string(),
        renderer_model_id: source_entry.regen_model.model_id.to_string(),
        technical_status: "BLOCKED".to_string(),
        musical_status: "BLOCKED".to_string(),
        human_review_status: "NOT RUN".to_string(),
        overall_automated_status: "BLOCKED".to_string(),
        detail: reason,
        sample_rate: None,
        frame_count: None,
        original_sha256: None,
        cue_packet_sha256: None,
        dry_output_sha256: None,
        pitched_precision: None,
        pitched_recall: None,
        pitched_offset_precision: None,
        pitched_offset_recall: None,
        unpitched_precision: None,
        unpitched_recall: None,
        false_voicing_silent_rate: None,
        artifacts_dir: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_real_recording_blocks_without_synthetic_fallback() {
        let dir = std::env::temp_dir().join(format!(
            "spectracraft-qual-missing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("source-qualification")
            .join("manifest.json");
        let criteria_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("source-qualification")
            .join("criteria.json");
        std::env::remove_var("SPECTRACRAFT_MONNIAUX_WAV");
        let report = run_qualification_campaign(
            &manifest_path,
            &criteria_path,
            &dir,
            Some("real_monniaux_slow_solo_20110108"),
        )
        .expect("campaign run");
        assert_eq!(report.cases.len(), 1);
        assert_eq!(report.cases[0].overall_automated_status, "BLOCKED");
        assert_eq!(report.cases[0].human_review_status, "NOT RUN");
        assert!(!report.all_required_automated_passed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn synthetic_guitar_and_silence_controls_pass_technical_gates() {
        let dir = std::env::temp_dir().join(format!(
            "spectracraft-qual-ctrl-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("source-qualification")
            .join("manifest.json");
        let criteria_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("source-qualification")
            .join("criteria.json");
        let guitar = run_qualification_campaign(
            &manifest_path,
            &criteria_path,
            &dir,
            Some("ctrl_guitar_plucked_arpeggio"),
        )
        .expect("guitar control");
        assert_eq!(guitar.cases[0].technical_status, "PASS");
        assert_eq!(guitar.cases[0].human_review_status, "NOT RUN");

        let silence = run_qualification_campaign(
            &manifest_path,
            &criteria_path,
            &dir,
            Some("ctrl_true_silence"),
        )
        .expect("silence control");
        assert_eq!(silence.cases[0].overall_automated_status, "PASS");
        assert_eq!(silence.cases[0].false_voicing_silent_rate, Some(0.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coverage_matrix_matches_138_source_catalog_ids() {
        let matrix_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("source-qualification")
            .join("coverage-matrix.json");
        let val: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(matrix_path).expect("read matrix"))
                .expect("parse matrix");
        let rows = val["rows"].as_array().expect("rows array");
        let sources = studio_catalog::load_source_catalog_from_disk_or_embedded();
        assert_eq!(rows.len(), sources.len());
        for (row, src) in rows.iter().zip(sources.iter()) {
            assert_eq!(row["catalog_id"].as_str(), Some(src.id.as_str()));
            assert_eq!(row["human_review_status"].as_str(), Some("NOT RUN"));
        }
    }
}
