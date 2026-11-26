//! Controlled solo-instrument verification test for SpectraCraft.
//!
//! Enforces all 7 stages of the controlled solo-instrument protocol:
//! 1. Old solo-instrument archive recording selection (real 1928 Andrés Segovia solo
//!    classical guitar 78 rpm archive transfer when provided on disk, plus an offline
//!    deterministic vintage solo plucked-guitar archive fallback for air-gapped CI).
//! 2. Exact fixed Stage 3 & Stage 4 capture conditions:
//!    - Source height: 5 ft
//!    - Source position: center of room ([5, 5, 5] ft)
//!    - Microphone: Shure SM7 (`shure-sm7b`)
//!    - Microphone distance: 2 ft from source
//!    - Room: 10 × 10 × 10 ft
//!    - Room material: padded (`wedge_foam_2in` on all 6 faces)
//! 3. Strictly blind Before input analysis (instrument, spatial, harmonic, room, mic)
//!    with measured/inferred tags and zero leakage from later stages.
//! 4. Full pipeline run: cue extraction -> Stage 1 dry regeneration (zero original PCM) ->
//!    Stage 3 SM7 @ 2 ft -> Stage 4 10×10×10 ft padded room @ 5 ft center -> Stage 5
//!    PhaseLimiter -> atomic WAV export to disk.
//! 5. Strictly blind After analysis on exported WAV only + numeric-delta Comparison summary.
//! 6. Post-blind signature verification gate (fails if any foreign instrument, original
//!    archive artifact, SM7/padded-room mismatch, or harmonic inconsistency remains).
//! 7. Dark-mode craft-pdf / Markdown / DOCX-fallback export verification.

use spectracraft_mono::{
    capture, digest,
    pipeline::{self, PluginId, PluginSlotConfig, PostDryPipelineConfig},
    reperform,
    studio_catalog::{
        mic_catalog, source_catalog, surface_catalog, RoomPreset, StudioRig, FEET_TO_METRES,
    },
    technical_report::{
        analyze_after_blind, analyze_before_blind, build_technical_report, export_report,
        export_report_pdf_craft, verify_solo_instrument_signatures, CaptureConditionsSpec,
        EvidenceTag,
    },
    translation_layer,
};
use std::f64::consts::TAU;
use std::path::{Path, PathBuf};

fn read_mono_wav(path: &Path) -> (u32, Vec<f32>) {
    let mut reader = hound::WavReader::open(path).expect("open mono WAV");
    let spec = reader.spec();
    assert_eq!(spec.channels, 1, "expected mono WAV at {}", path.display());
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.expect("read f32 sample"))
            .collect(),
        hound::SampleFormat::Int => {
            let scale = 2_f32.powi(spec.bits_per_sample as i32 - 1);
            reader
                .samples::<i32>()
                .map(|s| s.expect("read i32 sample") as f32 / scale)
                .collect()
        }
    };
    (spec.sample_rate, samples)
}

/// Deterministic offline vintage 78 rpm-style solo plucked guitar recording used when the
/// external Internet Archive WAV is not staged on disk (e.g., offline `cargo test --offline`).
fn synthesize_offline_vintage_solo_guitar_archive(rate: u32) -> Vec<f32> {
    let fs = f64::from(rate);
    let dur_s = 6.0_f64;
    let n = (fs * dur_s).round() as usize;
    let mut clean = vec![0.0_f64; n];
    // 10 plucked classical guitar notes across standard register (110 Hz .. 523 Hz)
    let notes: [(f64, f64, f64); 10] = [
        (0.20, 0.55, 220.0),
        (0.72, 0.52, 261.6),
        (1.22, 0.54, 329.6),
        (1.75, 0.50, 293.7),
        (2.26, 0.56, 196.0),
        (2.82, 0.52, 246.9),
        (3.35, 0.58, 392.0),
        (3.92, 0.52, 329.6),
        (4.45, 0.58, 164.8),
        (5.04, 0.72, 220.0),
    ];
    for &(t0, len_s, f0) in &notes {
        let i0 = (t0 * fs).round() as usize;
        let ilen = (len_s * fs).round() as usize;
        for j in 0..ilen {
            if i0 + j >= n {
                break;
            }
            let t = j as f64 / fs;
            let attack = 1.0 - (-t / 0.0035).exp();
            let mut v = 0.0_f64;
            for h in 1..=7 {
                let fh = f0 * f64::from(h);
                if fh >= fs * 0.42 {
                    break;
                }
                let amp = 0.24 / (f64::from(h)).powf(1.18);
                let decay = (-t / (0.28 / (1.0 + 0.35 * f64::from(h - 1)))).exp();
                v += amp * decay * (TAU * fh * t + 0.2 * f64::from(h)).sin();
            }
            clean[i0 + j] += attack * v;
        }
    }

    // Add realistic vintage archive artifacts: broadband shellac surface hiss, 60 Hz hum,
    // reverberant room echo, and discrete shellac crackle clicks.
    let mut state: u64 = 0x5E60_1928_CAFE_BABEu64;
    let mut next_u = || -> f64 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let bits = (state >> 11) as f64 / ((1u64 << 53) as f64);
        bits * 2.0 - 1.0
    };
    let echo_delay = (0.042 * fs).round() as usize;
    let mut out = vec![0.0_f32; n];
    for i in 0..n {
        let t = i as f64 / fs;
        let rev = if i >= echo_delay {
            0.22 * clean[i - echo_delay]
        } else {
            0.0
        };
        let hiss = 0.012 * next_u();
        let hum = 0.008 * (TAU * 60.0 * t).sin();
        let mut s = clean[i] + rev + hiss + hum;
        // 10 sharp shellac crackle pops
        if i > 10_000 && i % 27_000 == 0 {
            s += 0.32;
        }
        out[i] = s.clamp(-0.98, 0.98) as f32;
    }
    out
}

fn verify_fixed_capture_conditions(rig: &StudioRig) {
    let mics = mic_catalog();
    let surfaces = surface_catalog();

    // Source position: center of 10x10x10 ft room, height 5 ft
    let src = rig.source_pos_m();
    let five_ft_m = 5.0 * FEET_TO_METRES;
    assert!(
        (src[0] - five_ft_m).abs() < 1e-4
            && (src[1] - five_ft_m).abs() < 1e-4
            && (src[2] - five_ft_m).abs() < 1e-4,
        "Source must be centered at 5 ft height ([5,5,5] ft), got {:?} m",
        src
    );

    // Room: 10 x 10 x 10 ft
    let ten_ft_m = 10.0 * FEET_TO_METRES;
    assert_eq!(rig.room_preset, RoomPreset::Small);
    for (axis, &dim) in rig.dimensions_m.iter().enumerate() {
        assert!(
            (dim - ten_ft_m).abs() < 1e-4,
            "Room axis {axis} must be 10 ft ({ten_ft_m} m), got {dim} m"
        );
    }

    // Room material: padded (2-inch acoustic wedge foam across all 6 faces)
    assert_eq!(rig.surface_faces, [1, 1, 1, 1, 1, 1]);
    for &idx in &rig.surface_faces {
        assert_eq!(surfaces[idx].id, "wedge_foam_2in");
    }

    // Microphone: Shure SM7 at 2 ft distance aimed directly at source
    let slot = rig.active_mic();
    assert_eq!(mics[slot.catalog_index].id, "shure-sm7b");
    let dist_m =
        (slot.offset_m[0].powi(2) + slot.offset_m[1].powi(2) + slot.offset_m[2].powi(2)).sqrt();
    let two_ft_m = 2.0 * FEET_TO_METRES;
    assert!(
        (dist_m - two_ft_m).abs() < 1e-4,
        "Mic distance must be exactly 2 ft ({two_ft_m} m), got {dist_m} m"
    );
}

fn run_controlled_solo_verification(
    archive_label: &str,
    raw_samples: &[f32],
    rate: u32,
    out_dir: &Path,
) {
    let _ = std::fs::remove_dir_all(out_dir);
    std::fs::create_dir_all(out_dir).expect("create verification output dir");

    // 2. Fixed capture conditions (standard initial procedure)
    let rig = StudioRig::sm7b_foam_reference();
    verify_fixed_capture_conditions(&rig);
    let conditions = CaptureConditionsSpec::sm7_padded_10ft_reference();

    // 3. Strictly blind Before input analysis (raw archive file ONLY, prior to any rendering)
    let standalone_before = analyze_before_blind(raw_samples, rate).expect("blind before report");
    assert_eq!(standalone_before.blind_scope, "raw_input_only");
    assert!(standalone_before.parseval_verified);
    assert!(standalone_before
        .findings
        .iter()
        .any(|f| f.tag == EvidenceTag::Measured));
    assert!(standalone_before
        .findings
        .iter()
        .any(|f| f.tag == EvidenceTag::Inferred));
    for required_cat in [
        "Instrument Type",
        "Harmonic Structure",
        "Spatial Relationships",
        "Room Type & Materials",
        "Microphone Signature",
    ] {
        assert!(
            standalone_before
                .findings
                .iter()
                .any(|f| f.category == required_cat),
            "Before report missing required signature category: {required_cat}"
        );
    }

    // 4. Pipeline run:
    //    4a. Extract performance cues from archive recording (zero original PCM stored in cues)
    let (conditioned_private, _bedroom_prof) =
        translation_layer::condition_bedroom_for_analysis(raw_samples, rate);
    let cues = reperform::analyze_guitar(&conditioned_private, rate)
        .or_else(|_| reperform::analyze_guitar(raw_samples, rate))
        .expect("extract solo guitar performance cues");
    assert!(
        !cues.notes().is_empty(),
        "Cue extraction must detect solo instrument notes"
    );

    //    4b. Stage 1 dry regeneration (strictly from cues — zero original PCM!)
    let dry_take = cues.synthesize_dry().expect("Stage 1 dry regeneration");
    assert_eq!(dry_take.len(), raw_samples.len());
    assert_ne!(
        digest::sha256(
            &dry_take
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect::<Vec<_>>()
        ),
        digest::sha256(
            &raw_samples
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect::<Vec<_>>()
        ),
        "Stage 1 dry output must be freshly synthesized, never copied from original PCM"
    );

    //    4c. Stage 2 skipped (dry only; not needed for clean Stage 1 cue take)
    //    4d. Stage 3 (Shure SM7 @ 2 ft) + Stage 4 (10x10x10 ft padded room @ 5 ft height center)
    //        + Stage 5 (PhaseLimiter true-peak safety ceiling)
    let sources = source_catalog();
    let mics = mic_catalog();
    let surfaces = surface_catalog();
    let cfg = PostDryPipelineConfig {
        mic_enabled: true,
        preamp_drive: 0.0,
        room_enabled: true,
        rig: rig.clone(),
        plugin_slot: PluginSlotConfig {
            enabled: true,
            plugin_id: PluginId::PhaseLimiter,
            eq_low_db: 0.0,
            eq_high_db: 0.0,
            amount: 0.0,
            ceiling_dbfs: -1.0,
        },
    };
    let rendered =
        pipeline::run_post_dry_pipeline(&dry_take, rate, &cfg, &sources, &mics, &surfaces);
    assert!(rendered.summary.mic_stage_active);
    assert!(rendered.summary.room_stage_active);
    assert!(rendered.summary.plugin_slot_active);
    assert!(
        !rendered.summary.original_audio_opened,
        "Original PCM must NEVER be opened or blended in the pipeline"
    );
    assert!(!rendered.summary.bit_identical_to_dry);

    //    4e. Export final track crash-safely to disk and reload for truly blind After analysis
    let final_wav_path = out_dir.join("final-export-sm7-padded.wav");
    pipeline::write_wav_f32_atomic(&final_wav_path, rate, &rendered.samples)
        .expect("atomic export final WAV");
    let (exported_rate, exported_samples) = read_mono_wav(&final_wav_path);
    assert_eq!(exported_rate, rate);
    assert_eq!(exported_samples.len(), rendered.samples.len());

    // 5. Blind output analysis (final export ONLY) + numeric-delta Comparison summary
    let standalone_after =
        analyze_after_blind(&exported_samples, exported_rate).expect("blind after report");
    let report = build_technical_report(
        raw_samples,
        &exported_samples,
        rate,
        archive_label,
        "Stage 1 Dry Regeneration (0% original PCM) -> Stage 3 Shure SM7 @ 2 ft -> Stage 4 10x10x10 ft Padded Room (5 ft center) -> Stage 5 PhaseLimiter",
    )
    .expect("build technical report");

    // Confirm strict blindness: After report is bit-identical to standalone After analysis
    assert_eq!(report.before, standalone_before);
    assert_eq!(
        report.after, standalone_after,
        "After report must have zero access to Before report"
    );
    assert_eq!(report.after.blind_scope, "final_export_only");
    assert!(report.after.parseval_verified);
    assert!(report.comparison.generated_after_both_blind_reports);
    assert!(!report.comparison.deltas.is_empty());

    // 6. Signature verification gate
    let sig_verify = verify_solo_instrument_signatures(&report.before, &report.after, &conditions);

    // Print comprehensive verification readout for human audit (--nocapture)
    println!("\n================ CONTROLLED SOLO-INSTRUMENT VERIFICATION ================");
    println!("Archive Source : {archive_label}");
    println!(
        "Extracted Cues : {} notes ({:.2} s @ {} Hz)",
        cues.notes().len(),
        report.before.duration_s,
        rate
    );
    println!("Capture Rig    : Shure SM7B @ 2.0 ft | 10x10x10 ft padded (wedge_foam_2in) | Source center @ 5.0 ft");
    println!(
        "Before Inst.   : {} ({:.0}%)",
        report.before.signatures.instrument.display_name,
        report.before.signatures.instrument.confidence * 100.0
    );
    println!(
        "After Inst.    : {} ({:.0}%)",
        report.after.signatures.instrument.display_name,
        report.after.signatures.instrument.confidence * 100.0
    );
    println!("\n--- BEFORE FINDINGS (BLIND: RAW ARCHIVE ONLY) ---");
    for f in &report.before.findings {
        println!(
            "  [{}] {} | {}: {} ({})",
            f.tag.label(),
            f.category,
            f.label,
            f.value,
            f.detail
        );
    }
    println!("\n--- AFTER FINDINGS (BLIND: FINAL EXPORT ONLY) ---");
    for f in &report.after.findings {
        println!(
            "  [{}] {} | {}: {} ({})",
            f.tag.label(),
            f.category,
            f.label,
            f.value,
            f.detail
        );
    }
    println!("\n--- COMPARISON SUMMARY (NUMERIC DELTAS) ---");
    for d in &report.comparison.deltas {
        println!(
            "  {} : {} -> {} | {}",
            d.metric, d.before_value, d.after_value, d.summary
        );
    }
    println!("\n--- SIGNATURE VERIFICATION CHECKS ---");
    for c in &sig_verify.checks {
        println!(
            "  [{}] {} — {}",
            if c.passed { "GREEN" } else { "RED" },
            c.label,
            c.detail
        );
    }
    println!("=========================================================================\n");

    assert!(
        sig_verify.no_foreign_instrument_signatures,
        "Foreign instrument signature check failed: {:?}",
        sig_verify.checks
    );
    assert!(
        sig_verify.original_archive_signatures_absent,
        "Original archive signatures still present: {:?}",
        sig_verify.checks
    );
    assert!(
        sig_verify.applied_sm7_and_padded_room_match,
        "Applied SM7 / 10x10x10 padded room signature mismatch: {:?}",
        sig_verify.checks
    );
    assert!(
        sig_verify.harmonic_content_consistent,
        "Harmonic content consistency failed: {:?}",
        sig_verify.checks
    );
    assert!(
        sig_verify.all_passed,
        "Solo-instrument signature verification gate failed: {:?}",
        sig_verify.checks
    );
    assert!(
        report.comparison.no_unexpected_dropouts_or_clicks,
        "Final output must have 0 dropouts and 0 clicks (got dropouts={}, clicks={})",
        report.after.dropout_count, report.after.click_count
    );

    // 7. Completion gate: confirm craft-pdf dark-mode PDF + Markdown + DOCX fallback exports work
    let pdf_path = out_dir.join("solo-verification-report.pdf");
    let pdf_receipt = export_report_pdf_craft(&report, &pdf_path).expect("craft-pdf export");
    assert_eq!(pdf_receipt.actual_format, "pdf");
    assert!(!pdf_receipt.fallback_used);
    let pdf_bytes = std::fs::read(&pdf_path).expect("read exported PDF");
    assert!(pdf_bytes.starts_with(b"%PDF-1.7\n"));
    assert!(pdf_bytes.len() > 60_000);

    let md_path = out_dir.join("solo-verification-report.md");
    let md_receipt = export_report(&report, &md_path, "markdown").expect("markdown export");
    assert_eq!(md_receipt.actual_format, "markdown");
    let md_text = std::fs::read_to_string(&md_path).expect("read exported MD");
    assert!(md_text.contains("Instrument Type"));
    assert!(md_text.contains("Microphone Signature"));

    let docx_path = out_dir.join("solo-verification-report-docx-fallback.md");
    let docx_receipt = export_report(&report, &docx_path, "docx").expect("docx fallback export");
    assert!(docx_receipt.fallback_used);
}

#[test]
fn controlled_solo_instrument_archive_verification() {
    let default_real_archive =
        PathBuf::from("/tmp/spectracraft-solo-verify/segovia_1928_30s_48k.wav");
    let env_archive = std::env::var_os("SPECTRACRAFT_ARCHIVE_WAV").map(PathBuf::from);
    let chosen_path = env_archive.filter(|p| p.exists()).or_else(|| {
        default_real_archive
            .exists()
            .then_some(default_real_archive)
    });

    let out_dir = std::env::temp_dir().join("spectracraft-solo-instrument-verify");
    if let Some(wav_path) = chosen_path {
        let (rate, samples) = read_mono_wav(&wav_path);
        let label = format!(
            "Archive Solo Guitar — Andrés Segovia, Bach Courante (1928, 78 rpm Victrola 1298-B / IA 78_courante_andrs-segovia-bach_gbia3032379b) [{}]",
            wav_path.display()
        );
        run_controlled_solo_verification(&label, &samples, rate, &out_dir);
    } else {
        let rate = 48_000_u32;
        let samples = synthesize_offline_vintage_solo_guitar_archive(rate);
        run_controlled_solo_verification(
            "Offline Vintage 78rpm Solo Plucked Guitar Archive Harness",
            &samples,
            rate,
            &out_dir,
        );
    }
    let _ = std::fs::remove_dir_all(&out_dir);
}

#[test]
fn signature_verification_fails_on_foreign_or_mismatched_signatures() {
    let rate = 48_000_u32;
    let fs = f64::from(rate);
    let archive = synthesize_offline_vintage_solo_guitar_archive(rate);
    let before = analyze_before_blind(&archive, rate).expect("before");
    let conditions = CaptureConditionsSpec::sm7_padded_10ft_reference();

    // Case A: Passing the raw noisy archive through unchanged MUST FAIL verification!
    let after_raw = analyze_after_blind(&archive, rate).expect("after raw");
    let check_raw = verify_solo_instrument_signatures(&before, &after_raw, &conditions);
    assert!(
        !check_raw.all_passed,
        "Raw archive passthrough must fail signature verification"
    );
    assert!(
        !check_raw.original_archive_signatures_absent,
        "Raw archive passthrough must fail original_archive_signatures_absent"
    );
    assert!(
        !check_raw.applied_sm7_and_padded_room_match,
        "Raw archive passthrough must fail SM7/padded room match"
    );

    // Case B: Substituting a foreign instrument (continuous pure sustained woodwind/sine at 880 Hz)
    // MUST FAIL foreign-instrument & harmonic checks!
    let foreign: Vec<f32> = (0..archive.len())
        .map(|i| (0.25 * (TAU * 880.0 * i as f64 / fs).sin()) as f32)
        .collect();
    let after_foreign = analyze_after_blind(&foreign, rate).expect("after foreign");
    let check_foreign = verify_solo_instrument_signatures(&before, &after_foreign, &conditions);
    assert!(
        !check_foreign.all_passed,
        "Foreign continuous 880 Hz tone must fail solo guitar verification"
    );
    assert!(
        !check_foreign.no_foreign_instrument_signatures,
        "Foreign tone must fail instrument identity check"
    );

    // Case C: Rendering dry guitar through a huge live reflective hall with an omni mic
    // MUST FAIL the SM7 + 10x10x10 padded room condition check!
    let (cond, _) = translation_layer::condition_bedroom_for_analysis(&archive, rate);
    let cues = reperform::analyze_guitar(&cond, rate).expect("cues");
    let dry = cues.synthesize_dry().expect("dry");
    let hall = capture::render(
        &dry,
        rate,
        capture::Setup {
            room: capture::Room::Hall,
            mic: capture::Mic::Omni,
            custom_dimensions: Some([22.0, 18.0, 12.0]),
            custom_materials: Some([capture::Material::ConcreteLike; 3]),
            offset: [0.0, 6.0, 0.0],
        },
    );
    let after_hall = analyze_after_blind(&hall, rate).expect("after hall");
    let check_hall = verify_solo_instrument_signatures(&before, &after_hall, &conditions);
    assert!(
        !check_hall.all_passed || !check_hall.applied_sm7_and_padded_room_match,
        "Distant omni reflective hall must fail SM7 @ 2 ft + 10x10x10 padded room verification: {:?}",
        check_hall.checks
    );
}
