//! Headless integration tests for the shared framework-neutral session (`app_session`)
//! and bounded cancellable job engine (`app_jobs`).
//!
//! Proves:
//! - Stale async result rejection across source swap (epoch bump) and superseded requests.
//! - Bounded worker pool & preview coalescing under rapid parameter updates.
//! - Clean cancellation & shutdown without leaked threads.
//! - Actual-audio Undo/Redo restoring exact dry SHA-256 without touching original audio.
//! - Stage 3-5 bypass identity (bit-identical to dry when Mic/Room/Plugin are OFF).
//! - Unchanged DSP sample parity between direct core API calls and `AppSession` + `JobRunner`.
//! - Stale-capture export blocking and original-file overwrite refusal.

use spectracraft_mono::app_jobs::{
    execute_job, CancellationToken, JobOutcome, JobRequest, JobRunner,
};
use spectracraft_mono::app_session::{
    load_validated_wav_artifact, AppSession, DisplayTake, DryReviewChecklist, ExportTarget,
    SaveKind, StaleCompletionError,
};
use spectracraft_mono::capture::DryReviewStatus;
use spectracraft_mono::pipeline::{self, PluginId, PluginSlotConfig};
use spectracraft_mono::region_edit::{DryRegionEdit, RegionFixAction};
use spectracraft_mono::reperform;
use std::f64::consts::TAU;
use std::path::PathBuf;
use std::time::Duration;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "spectracraft-app-session-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_plucked_test_wav(path: &std::path::Path, rate: u32, freq_hz: f64, secs: f64) -> Vec<f32> {
    let n = (f64::from(rate) * secs).round() as usize;
    let samples: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            let env = (t / 0.015).min(1.0) * (-2.2 * t).exp();
            let s = 0.55 * (TAU * freq_hz * t).sin()
                + 0.22 * (TAU * 2.0 * freq_hz * t).sin()
                + 0.09 * (TAU * 3.0 * freq_hz * t).sin();
            (env * s) as f32
        })
        .collect();
    spectracraft_mono::write_wav(rate, &samples, path).unwrap();
    samples
}

#[test]
fn stale_result_rejected_on_source_swap_and_config_change() {
    let dir = temp_dir("stale");
    let wav_a = dir.join("take_a.wav");
    let wav_b = dir.join("take_b.wav");
    make_plucked_test_wav(&wav_a, 48_000, 220.0, 0.6);
    make_plucked_test_wav(&wav_b, 48_000, 330.0, 0.6);

    let mut session = AppSession::new(1);
    let id_a = session.allocate_artifact_id();
    let (art_a, name_a) = load_validated_wav_artifact(&wav_a, id_a).unwrap();
    session.load_source_artifact(wav_a.clone(), name_a, art_a);

    // Start dry job for source A
    let dry_spec_a = session.begin_dry_request().unwrap();
    let outcome_a = execute_job(JobRequest::DryRegen {
        spec: dry_spec_a,
        cancel: CancellationToken::new(),
    })
    .unwrap();

    // Swap source to B BEFORE publishing A's completion!
    let id_b = session.allocate_artifact_id();
    let (art_b, name_b) = load_validated_wav_artifact(&wav_b, id_b).unwrap();
    session.load_source_artifact(wav_b.clone(), name_b, art_b);

    match outcome_a {
        JobOutcome::DryReady {
            guard,
            performance,
            samples,
        } => {
            let err = session
                .publish_dry_result(&guard, performance, samples)
                .unwrap_err();
            assert!(
                matches!(err, StaleCompletionError::EpochMismatch { .. }),
                "expected EpochMismatch on source swap, got {err:?}"
            );
            assert!(
                session.dry.is_none(),
                "stale dry completion must never publish into new source epoch"
            );
        }
        other => panic!("unexpected outcome: {:?}", std::mem::discriminant(&other)),
    }

    // Now run valid dry job for B, then mutate Mic toggle before publishing a stale PostDry job.
    let dry_spec_b = session.begin_dry_request().unwrap();
    let JobOutcome::DryReady {
        guard,
        performance,
        samples,
    } = execute_job(JobRequest::DryRegen {
        spec: dry_spec_b,
        cancel: CancellationToken::new(),
    })
    .unwrap()
    else {
        panic!("expected DryReady");
    };
    session
        .publish_dry_result(&guard, performance, samples)
        .unwrap();

    let post_spec = session.begin_post_dry_request().unwrap();
    let JobOutcome::PostDryReady {
        guard: post_guard,
        output,
    } = execute_job(JobRequest::PostDry {
        spec: post_spec,
        cancel: CancellationToken::new(),
    })
    .unwrap()
    else {
        panic!("expected PostDryReady");
    };

    // Change Mic stage BEFORE publishing -> must reject as ConfigChanged.
    session.set_mic_enabled(true);
    let err = session
        .publish_post_dry_result(&post_guard, output)
        .unwrap_err();
    assert!(
        matches!(err, StaleCompletionError::ConfigChanged { .. }),
        "expected ConfigChanged, got {err:?}"
    );
    assert!(session.capture.is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn end_to_end_parity_bypass_identity_undo_redo_and_export_guards() {
    let dir = temp_dir("e2e");
    let wav_path = dir.join("guitar_phrase.wav");
    let orig_samples = make_plucked_test_wav(&wav_path, 48_000, 196.0, 0.8);
    let orig_disk_bytes = std::fs::read(&wav_path).unwrap();

    let mut session = AppSession::new(42);
    let id = session.allocate_artifact_id();
    let (art, name) = load_validated_wav_artifact(&wav_path, id).unwrap();
    let orig_sha = art.sha256.clone();
    session.load_source_artifact(wav_path.clone(), name, art);

    // Run Stage 1 via shared session + runner and compare directly against core `reperform`.
    let dry_spec = session.begin_dry_request().unwrap();
    let direct_perf = reperform::analyze_guitar(&orig_samples, 48_000).unwrap();
    let direct_dry = direct_perf
        .synthesize_dry_for_source_with_voicing(session.current_source_entry(), &session.voicing)
        .unwrap();

    let mut runner = JobRunner::new(2, 16, None);
    runner
        .submit(JobRequest::DryRegen {
            spec: dry_spec,
            cancel: CancellationToken::new(),
        })
        .unwrap();
    let outcome = runner.recv_timeout(Duration::from_secs(30)).unwrap();
    let JobOutcome::DryReady {
        guard,
        performance,
        samples,
    } = outcome
    else {
        panic!("expected DryReady");
    };
    let dry_v1 = session
        .publish_dry_result(&guard, performance, samples)
        .unwrap();

    assert_eq!(
        dry_v1.samples.as_ref(),
        direct_dry.as_slice(),
        "AppSession + JobRunner Stage 1 dry output must be bit-identical to core DSP"
    );
    assert_eq!(session.dry_review_status(), DryReviewStatus::NotReviewed);

    // Stage 3-5 all bypassed -> capture MUST be bit-identical to dry!
    let post_bypass = session.begin_post_dry_request().unwrap();
    runner
        .submit(JobRequest::PostDry {
            spec: post_bypass,
            cancel: CancellationToken::new(),
        })
        .unwrap();
    let JobOutcome::PostDryReady {
        guard: g_cap,
        output: out_cap,
    } = runner.recv_timeout(Duration::from_secs(15)).unwrap()
    else {
        panic!("expected PostDryReady");
    };
    assert!(out_cap.summary.bit_identical_to_dry);
    assert!(!out_cap.summary.original_audio_opened);
    let cap_bypass = session.publish_post_dry_result(&g_cap, out_cap).unwrap();
    assert_eq!(cap_bypass.samples.as_ref(), dry_v1.samples.as_ref());

    // Approve dry revision v1 explicitly
    session.set_dry_checklist(DryReviewChecklist {
        rhythm: true,
        phrasing: true,
        dynamics: true,
        sustain: true,
        artifact_free: true,
    });
    assert_eq!(
        session.dry_review_status(),
        DryReviewStatus::ApprovedByHuman
    );

    // Stage 2 bounded surgical edit on dry -> outside region bit-identical, approval invalidated, capture stale!
    session.set_selection(Some([0.15, 0.45]));
    session.set_region_edit_draft(DryRegionEdit {
        id: "r-test".into(),
        kind: "range".into(),
        start_s: 0.15,
        end_s: 0.45,
        min_hz: None,
        max_hz: None,
        note: String::new(),
        action: RegionFixAction::Smooth,
        strength: 0.65,
        crossfade_ms: 12.0,
    });
    let edit_spec = session.begin_region_edit_request().unwrap();
    runner
        .submit(JobRequest::RegionEdit {
            spec: edit_spec,
            cancel: CancellationToken::new(),
        })
        .unwrap();
    let JobOutcome::RegionEditReady {
        guard: g_edit,
        output: out_edit,
    } = runner.recv_timeout(Duration::from_secs(15)).unwrap()
    else {
        panic!("expected RegionEditReady");
    };
    assert!(out_edit.outside_bit_identical);
    let dry_v2 = session
        .publish_region_edit_result(&g_edit, out_edit)
        .unwrap();

    // New dry revision must clear revision-bound approval and mark capture stale
    assert_eq!(session.dry_review_status(), DryReviewStatus::NotReviewed);
    assert!(session.capture_stale);
    // Exporting stale capture MUST be refused!
    let stale_export = dir.join("stale_capture.wav");
    assert!(
        session
            .export_artifact_atomic(ExportTarget::CurrentCapture, &stale_export)
            .is_err(),
        "stale capture export must be blocked"
    );

    // Undo restores exact dry v1 SHA-256; Redo restores exact dry v2 SHA-256!
    assert!(session.undo());
    assert_eq!(session.dry.as_ref().unwrap().sha256, dry_v1.sha256);
    assert!(session.redo());
    assert_eq!(session.dry.as_ref().unwrap().sha256, dry_v2.sha256);

    // Enable Mic (with preamp_drive) + Room + Plugin and re-render Stages 3-5
    session.set_mic_enabled(true);
    session.set_preamp_drive(0.35);
    session.set_room_enabled(true);
    session.set_plugin_slot(PluginSlotConfig {
        enabled: true,
        plugin_id: PluginId::GentleEq,
        eq_low_db: 1.0,
        eq_high_db: -0.5,
        amount: 0.45,
        ceiling_dbfs: -1.0,
    });
    let cfg = session.post_dry_config();
    let direct_cap = pipeline::run_post_dry_pipeline(
        &dry_v2.samples,
        dry_v2.sample_rate,
        &cfg,
        &session.sources,
        &session.mics,
        &session.surfaces,
    );

    let post_spec = session.begin_post_dry_request().unwrap();
    runner
        .submit(JobRequest::PostDry {
            spec: post_spec,
            cancel: CancellationToken::new(),
        })
        .unwrap();
    let JobOutcome::PostDryReady {
        guard: g_final,
        output: out_final,
    } = runner.recv_timeout(Duration::from_secs(20)).unwrap()
    else {
        panic!("expected PostDryReady");
    };
    let cap_final = session
        .publish_post_dry_result(&g_final, out_final)
        .unwrap();
    assert!(!session.capture_stale);
    assert_eq!(
        cap_final.samples.as_ref(),
        direct_cap.samples.as_slice(),
        "GPUI/shared post-dry capture must be bit-identical to core pipeline"
    );

    // Overwriting original WAV must be refused!
    assert!(
        session
            .export_artifact_atomic(ExportTarget::CurrentCapture, &wav_path)
            .is_err(),
        "must refuse overwriting original WAV"
    );

    // Atomic export of fresh capture succeeds and matches samples
    let out_wav = dir.join("final_capture.wav");
    let receipt = session
        .export_artifact_atomic(ExportTarget::CurrentCapture, &out_wav)
        .unwrap();
    assert_eq!(receipt.artifact_id, cap_final.id);
    assert!(out_wav.exists());

    // Settings-only preset save is explicitly labeled SettingsPreset and never carries over approval to a different session
    let preset_path = dir.join("rig_preset.spectracraft.json");
    session.save_preset_atomic(&preset_path).unwrap();
    let mut fresh_session = AppSession::new(99);
    let kind = fresh_session.load_preset_file(&preset_path).unwrap();
    assert_eq!(kind, SaveKind::SettingsPreset);
    assert!(fresh_session.mic_enabled);
    assert!((fresh_session.preamp_drive - 0.35).abs() < 1e-6);
    assert_eq!(
        fresh_session.dry_review_status(),
        DryReviewStatus::NotReviewed
    );

    // Display-only state isolation: switching take or zoom never alters source/dry hashes or epoch.
    let epoch_before = session.epoch();
    session.display.take = DisplayTake::Original;
    session.display.view_range = [0.1, 0.5];
    assert_eq!(session.epoch(), epoch_before);
    assert_eq!(session.source.as_ref().unwrap().sha256, orig_sha);

    // Original source file on disk remained completely untouched.
    assert_eq!(std::fs::read(&wav_path).unwrap(), orig_disk_bytes);

    runner.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runner_bounds_concurrency_coalesces_previews_and_shuts_down_cleanly() {
    let dir = temp_dir("coalesce");
    let wav_path = dir.join("tone.wav");
    make_plucked_test_wav(&wav_path, 48_000, 261.63, 0.5);

    let mut session = AppSession::new(7);
    let id = session.allocate_artifact_id();
    let (art, name) = load_validated_wav_artifact(&wav_path, id).unwrap();
    session.load_source_artifact(wav_path, name, art);

    let dry_spec = session.begin_dry_request().unwrap();
    let JobOutcome::DryReady {
        guard,
        performance,
        samples,
    } = execute_job(JobRequest::DryRegen {
        spec: dry_spec,
        cancel: CancellationToken::new(),
    })
    .unwrap()
    else {
        panic!("expected DryReady");
    };
    session
        .publish_dry_result(&guard, performance, samples)
        .unwrap();

    // 1 worker, max_pending = 4 -> submit 25 rapid PostDry slider updates; coalescing keeps queue bounded!
    let mut runner = JobRunner::new(1, 4, None);
    session.set_mic_enabled(true);
    for step in 0..25 {
        session.set_preamp_drive(step as f64 / 24.0);
        let spec = session.begin_post_dry_request().unwrap();
        runner
            .submit(JobRequest::PostDry {
                spec,
                cancel: CancellationToken::new(),
            })
            .expect("coalesced preview submission must never overflow bounded queue");
        assert!(runner.queued_count() <= runner.max_pending());
    }
    assert!(
        runner.total_coalesced() > 0,
        "rapid PostDry updates should coalesce queued previews"
    );

    // Drain until idle, then verify clean shutdown
    runner.cancel_all_pending();
    runner.shutdown();
    assert_eq!(runner.in_flight_count(), 0);
    assert_eq!(runner.queued_count(), 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wav_loader_rejects_directories_control_chars_and_undersized_files() {
    let dir = temp_dir("wav_guards");
    // 1. Directory path must be rejected before opening
    assert!(load_validated_wav_artifact(&dir, 1).is_err());

    // 2. Control characters in path must be rejected
    let bad_path = dir.join("evil\nname.wav");
    assert!(load_validated_wav_artifact(&bad_path, 2).is_err());

    // 3. Undersized file (< 44-byte RIFF header) must be rejected before decoding
    let tiny = dir.join("tiny.wav");
    std::fs::write(&tiny, b"RIFF1234").unwrap();
    assert!(load_validated_wav_artifact(&tiny, 3).is_err());

    let _ = std::fs::remove_dir_all(&dir);
}
