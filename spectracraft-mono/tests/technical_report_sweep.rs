//! Mandatory 10-second 20 Hz -> 20 kHz sine sweep verification test for the
//! blind Before-and-After technical report, real-buffer viewer plots, and
//! dark-mode `craft-pdf` / fallback export pipeline.

use spectracraft_mono::{
    pipeline::{self, PluginId, PluginSlotConfig, PostDryPipelineConfig},
    string_state::{BodyResonanceConfig, BodyResonator},
    studio_catalog::{mic_catalog, source_catalog, surface_catalog, StudioRig},
    technical_report::{
        analyze_after_blind, build_technical_report, export_report, export_report_pdf_craft,
        EvidenceTag,
    },
    translation_layer,
};
use std::f64::consts::{PI, TAU};

const RATE: u32 = 48_000;

fn generate_log_sine_sweep(
    rate: u32,
    duration_s: f64,
    f0: f64,
    f1: f64,
    amp: f64,
    fade_s: f64,
) -> Vec<f32> {
    let n = (f64::from(rate) * duration_s).round() as usize;
    let fs = f64::from(rate);
    let ln_ratio = (f1 / f0).ln();
    let fade_n = (fade_s * fs).round() as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / fs;
        let phase = TAU * f0 * duration_s / ln_ratio * ((t * ln_ratio / duration_s).exp() - 1.0);
        let mut w = 1.0_f64;
        if i < fade_n {
            w = 0.5 * (1.0 - (PI * i as f64 / fade_n as f64).cos());
        } else if i + fade_n >= n {
            let rem = n - 1 - i;
            w = 0.5 * (1.0 - (PI * rem as f64 / fade_n as f64).cos());
        }
        out.push((amp * w * phase.sin()) as f32);
    }
    out
}

#[test]
fn mandatory_10s_sine_sweep_before_after_report_and_export() {
    // 1. Generate a 10-second constant-amplitude log sine sweep (20 Hz -> 20 kHz)
    let sweep = generate_log_sine_sweep(RATE, 10.0, 20.0, 20_000.0, 0.25, 0.015);
    assert_eq!(sweep.len(), RATE as usize * 10);

    // 2. Run through Bedroom Translator private check -> Stage 1 BodyResonator -> Stage 3 Mic -> Stage 4 Room -> Stage 5 Limiter
    let (_conditioned, bedroom_prof) =
        translation_layer::condition_bedroom_for_analysis(&sweep, RATE);
    assert!(
        bedroom_prof.mains_hum_hz.is_none(),
        "Pure sweep should not falsely trigger mains hum: {:?}",
        bedroom_prof.mains_hum_hz
    );

    let mut body_buf: Vec<f64> = sweep.iter().map(|&s| f64::from(s)).collect();
    let mut body = BodyResonator::new(f64::from(RATE), BodyResonanceConfig::default());
    body.process_buffer(&mut body_buf);
    let body_f32: Vec<f32> = body_buf.iter().map(|&v| v as f32).collect();

    let sources = source_catalog();
    let mics = mic_catalog();
    let surfaces = surface_catalog();
    let mut cfg = PostDryPipelineConfig::default();
    cfg.mic_enabled = true;
    cfg.room_enabled = true;
    cfg.rig = StudioRig::sm7b_foam_reference();
    cfg.plugin_slot = PluginSlotConfig {
        enabled: true,
        plugin_id: PluginId::PhaseLimiter,
        eq_low_db: 0.0,
        eq_high_db: 0.0,
        amount: 0.0,
        ceiling_dbfs: -1.0,
    };
    let rendered =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &cfg, &sources, &mics, &surfaces);
    let final_export = &rendered.samples;
    // Canonical H14 post-dry pipeline preserves natural Stage 4 room decay tail beyond sweep.len().
    assert!(
        final_export.len() > sweep.len(),
        "Canonical post-dry pipeline must preserve Stage 4 room decay tail: final={}, sweep={}",
        final_export.len(),
        sweep.len()
    );
    assert!(
        final_export.len() <= sweep.len() + RATE as usize,
        "Padded 10x10x10 foam room tail should stay bounded within 1 s: final={}, sweep={}",
        final_export.len(),
        sweep.len()
    );
    let fixed_block = pipeline::run_post_dry_pipeline_fixed_block(
        &body_f32, RATE, &cfg, &sources, &mics, &surfaces,
    );
    assert_eq!(
        fixed_block.samples.len(),
        sweep.len(),
        "Fixed-block audition pipeline must preserve exact input frame count"
    );

    // 3. Generate Before & After reports plus real-buffer viewer data
    let report = build_technical_report(
        &sweep,
        final_export,
        RATE,
        "10s Log Sine Sweep (20 Hz - 20 kHz)",
        "Stage 1 Body (100/200 Hz) -> Stage 3 SM7B Mic -> Stage 4 Foam Room -> Stage 5 PhaseLimiter",
    )
    .expect("build technical report");

    // 4a. Assert both reports are produced and strictly independent
    assert_eq!(report.before.blind_scope, "raw_input_only");
    assert_eq!(report.after.blind_scope, "final_export_only");
    assert!(!report.before.findings.is_empty());
    assert!(!report.after.findings.is_empty());
    assert!(
        report
            .before
            .findings
            .iter()
            .any(|f| f.tag == EvidenceTag::Measured)
            && report
                .before
                .findings
                .iter()
                .any(|f| f.tag == EvidenceTag::Inferred),
        "Before report must tag measured and inferred items"
    );
    assert!(
        report
            .after
            .findings
            .iter()
            .any(|f| f.tag == EvidenceTag::Measured)
            && report
                .after
                .findings
                .iter()
                .any(|f| f.tag == EvidenceTag::Inferred),
        "After report must tag measured and inferred items"
    );
    // Strict blindness check: analyzing `final_export` alone gives bit-identical AfterReport
    let standalone_after = analyze_after_blind(final_export, RATE).expect("standalone after");
    assert_eq!(
        report.after, standalone_after,
        "After report must not be influenced by Before report"
    );
    assert!(report.before.parseval_verified);
    assert!(report.after.parseval_verified);

    // 4b. Assert waveform and spectrogram views are non-empty, finite, and identically time-aligned
    let p = &report.plots;
    assert_eq!(p.sample_rate, RATE);
    assert_eq!(report.before.samples, sweep.len());
    assert_eq!(report.after.samples, final_export.len());
    let expected_plot_dur_s = final_export.len() as f64 / f64::from(RATE);
    assert!(
        (p.duration_s - expected_plot_dur_s).abs() < 1e-4,
        "Aligned viewer plot duration ({:.5} s) must match tail-preserving export ({:.5} s)",
        p.duration_s,
        expected_plot_dur_s
    );
    assert!(p.waveform_scale_linear > 0.1 && p.waveform_scale_linear.is_finite());
    assert_eq!(p.waveform_before.len(), p.waveform_bins);
    assert_eq!(p.waveform_after.len(), p.waveform_bins);
    assert_eq!(p.residual_waveform.len(), p.waveform_bins);
    assert!(
        p.waveform_before.iter().any(|&[lo, hi]| hi - lo > 0.1),
        "Original waveform must be non-empty"
    );
    assert!(
        p.waveform_after.iter().any(|&[lo, hi]| hi - lo > 0.1),
        "Final waveform must be non-empty"
    );

    let expected_cells = p.spectrogram_cols * p.spectrogram_rows;
    assert!(expected_cells > 0);
    assert_eq!(p.spectrogram_before_db.len(), expected_cells);
    assert_eq!(p.spectrogram_after_db.len(), expected_cells);
    assert_eq!(p.spectrogram_diff_db.len(), expected_cells);
    assert_eq!(p.spectrogram_freq_hz.len(), p.spectrogram_rows);
    assert!(p.spectrogram_before_db.iter().all(|v| v.is_finite()));
    assert!(p.spectrogram_after_db.iter().all(|v| v.is_finite()));
    assert!(p.spectrogram_diff_db.iter().all(|v| v.is_finite()));
    let max_before_db = p
        .spectrogram_before_db
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let max_after_db = p
        .spectrogram_after_db
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max_before_db > -30.0 && max_after_db > -30.0,
        "Spectrograms must contain sweep energy: before_max={max_before_db}, after_max={max_after_db}"
    );
    assert_eq!(p.third_octave.centers_hz.len(), 28);
    assert_eq!(p.upper_band.times_s.len(), p.spectrogram_cols);

    // 4c. Comparison summary notes body-resonance peaks AND absence of dropouts/clicks
    assert!(report.comparison.generated_after_both_blind_reports);
    assert!(
        report.comparison.body_resonance_peaks_noted,
        "Comparison summary must note 100/200 Hz body resonance peaks when enabled. before=({:+.2}, {:+.2}), after=({:+.2}, {:+.2}), deltas={:?}",
        report.before.body_100hz_prominence_db,
        report.before.body_200hz_prominence_db,
        report.after.body_100hz_prominence_db,
        report.after.body_200hz_prominence_db,
        report.comparison.deltas
    );
    assert!(
        report
            .comparison
            .deltas
            .iter()
            .any(|d| d.metric.contains("Body resonance") && d.summary.contains("100/200 Hz")),
        "Expected body resonance delta entry in comparison summary: {:?}",
        report.comparison.deltas
    );
    assert!(
        report.comparison.no_unexpected_dropouts_or_clicks,
        "Expected zero dropouts and clicks in final sweep: dropouts={}, clicks={}",
        report.after.dropout_count, report.after.click_count
    );
    assert!(
        report
            .comparison
            .deltas
            .iter()
            .any(|d| d.summary.contains("No unexpected dropouts or clicks")),
        "Comparison summary must explicitly state absence of unexpected dropouts or clicks"
    );

    // 4d. Export via craft-pdf (dark-mode PDF with embedded plots) & documented DOCX/Markdown fallback
    let tmp_dir = std::env::temp_dir().join("spectracraft-sweep-report-test");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).expect("create temp test dir");

    let pdf_path = tmp_dir.join("sweep-technical-report.pdf");
    let pdf_receipt =
        export_report_pdf_craft(&report, &pdf_path).expect("craft-pdf dark-mode PDF export");
    assert_eq!(pdf_receipt.actual_format, "pdf");
    assert!(!pdf_receipt.fallback_used);
    assert!(pdf_receipt.engine.contains("craft-pdf"));
    let pdf_bytes = std::fs::read(&pdf_path).expect("read exported PDF");
    assert!(pdf_bytes.starts_with(b"%PDF-1.7\n"));
    assert!(pdf_bytes.ends_with(b"%%EOF\n"));
    assert!(
        pdf_bytes.len() > 80_000,
        "PDF should contain full text and 3 embedded spectrogram images (got {} bytes)",
        pdf_bytes.len()
    );
    let pdf_lossy = String::from_utf8_lossy(&pdf_bytes);
    assert!(pdf_lossy.contains("SPECTRACRAFT TECHNICAL REPORT"));
    assert!(pdf_lossy.contains("/Subtype /Image"));
    assert!(pdf_lossy.contains("/Im1 Do"));
    assert!(pdf_lossy.contains("/Im2 Do"));
    assert!(pdf_lossy.contains("/Im3 Do"));

    // Markdown export + documented DOCX fallback
    let md_path = tmp_dir.join("sweep-technical-report.md");
    let md_receipt = export_report(&report, &md_path, "markdown").expect("markdown export");
    assert_eq!(md_receipt.actual_format, "markdown");
    assert!(!md_receipt.fallback_used);
    let md_text = std::fs::read_to_string(&md_path).expect("read markdown");
    assert!(md_text.contains("## 1. Comparison Summary"));
    assert!(md_text.contains("Body peaks at 100/200 Hz"));

    let docx_fallback_path = tmp_dir.join("sweep-technical-report-docx-fallback.md");
    let docx_receipt =
        export_report(&report, &docx_fallback_path, "docx").expect("docx fallback export");
    assert!(docx_receipt.fallback_used);
    assert!(docx_receipt
        .fallback_note
        .as_deref()
        .unwrap_or("")
        .contains("craft-docx"));
    let fb_text = std::fs::read_to_string(&docx_fallback_path).expect("read fallback doc");
    assert!(fb_text.contains("Fallback Notice"));

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
