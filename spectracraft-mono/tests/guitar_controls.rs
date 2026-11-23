use spectracraft_mono::capture::{
    render_analog_circuit, render_digital_direct, render_guitar_variant, render_natural_amp_mic,
    ExportVariant,
};
use spectracraft_mono::reperform::{
    analyze_guitar, independent_pitch_check, EvidenceStatus, Performance,
};
use std::f64::consts::TAU;

fn median_hz_in(perf: &Performance, lo_s: f64, hi_s: f64) -> Option<f64> {
    let mut vals: Vec<f64> = perf
        .notes()
        .iter()
        .flat_map(|n| {
            n.hz.iter().enumerate().filter_map(move |(k, &f)| {
                let t = n.start + k as f64 * perf.hop_s();
                (t >= lo_s && t <= hi_s).then_some(f)
            })
        })
        .collect();
    if vals.is_empty() {
        return None;
    }
    vals.sort_by(f64::total_cmp);
    Some(vals[vals.len() / 2])
}

fn cents_err(measured: f64, expected: f64) -> f64 {
    (1200.0 * (measured / expected).log2()).abs()
}

fn synth_harmonic_note(
    buf: &mut [f32],
    rate: u32,
    start_s: f64,
    dur_s: f64,
    f0: f64,
    amps: &[f64],
    decay: f64,
) {
    let start = (start_s * f64::from(rate)).round() as usize;
    let len = (dur_s * f64::from(rate)).round() as usize;
    for i in 0..len {
        let idx = start + i;
        if idx >= buf.len() {
            break;
        }
        let t = i as f64 / f64::from(rate);
        let env = (t / 0.008).min(1.0)
            * (((len - 1 - i) as f64 / f64::from(rate)) / 0.015).min(1.0)
            * (-decay * t).exp();
        let mut s = 0.0;
        for (h_idx, &a) in amps.iter().enumerate() {
            let h = (h_idx + 1) as f64;
            if h * f0 < f64::from(rate) * 0.46 {
                s += a * (TAU * h * f0 * t).sin() * (-0.35 * (h - 1.0) * t).exp();
            }
        }
        buf[idx] += (env * s) as f32;
    }
}

#[test]
fn weak_fundamental_tracks_true_f0_not_upper_harmonic() {
    for rate in [16_000, 44_100, 48_000, 96_000] {
        let mut x = vec![0.0_f32; rate as usize];
        // Very weak fundamental (h1=0.035), dominant h2/h3/h4/h5 at D3 = 146.83 Hz.
        synth_harmonic_note(
            &mut x,
            rate,
            0.15,
            0.70,
            146.83,
            &[0.035, 0.28, 0.24, 0.16, 0.10, 0.06],
            1.6,
        );
        let perf = analyze_guitar(&x, rate).unwrap();
        let f = median_hz_in(&perf, 0.25, 0.70)
            .unwrap_or_else(|| panic!("no pitch tracked at {rate} Hz"));
        assert!(
            cents_err(f, 146.83) < 40.0,
            "rate={rate}: weak fundamental tracked {f:.2} Hz instead of 146.83 Hz"
        );
        let dry = perf.synthesize_dry().unwrap();
        assert_eq!(dry.len(), x.len());
        assert_eq!(dry, perf.synthesize_dry().unwrap());
    }
}

#[test]
fn genuine_octave_jump_is_tracked_across_sample_rates() {
    for rate in [16_000, 44_100, 48_000, 96_000] {
        let mut x = vec![0.0_f32; (rate as usize * 3) / 2];
        synth_harmonic_note(
            &mut x,
            rate,
            0.12,
            0.55,
            220.0,
            &[0.24, 0.14, 0.08, 0.04],
            1.4,
        );
        synth_harmonic_note(
            &mut x,
            rate,
            0.72,
            0.55,
            440.0,
            &[0.24, 0.14, 0.08, 0.04],
            1.4,
        );
        let perf = analyze_guitar(&x, rate).unwrap();
        let lo = median_hz_in(&perf, 0.22, 0.58)
            .unwrap_or_else(|| panic!("missing low octave at {rate} Hz"));
        let hi = median_hz_in(&perf, 0.82, 1.18)
            .unwrap_or_else(|| panic!("missing high octave at {rate} Hz"));
        assert!(
            cents_err(lo, 220.0) < 35.0,
            "rate={rate}: low note tracked {lo:.2} Hz (expected 220)"
        );
        assert!(
            cents_err(hi, 440.0) < 35.0,
            "rate={rate}: high note tracked {hi:.2} Hz (expected 440)"
        );
    }
}

#[test]
fn continuous_slide_tracks_glide_without_splitting_into_stepped_artifacts() {
    for rate in [16_000, 44_100, 48_000, 96_000] {
        let n = (rate as f64 * 1.2) as usize;
        let mut x = vec![0.0_f32; n];
        let start = (0.15 * f64::from(rate)) as usize;
        let len = (0.80 * f64::from(rate)) as usize;
        let mut phase = 0.0;
        for i in 0..len {
            let u = i as f64 / len as f64;
            let f0 = 196.0 * (293.66 / 196.0_f64).powf(u);
            phase += TAU * f0 / f64::from(rate);
            let env = (i as f64 / (0.01 * f64::from(rate))).min(1.0)
                * ((len - 1 - i) as f64 / (0.02 * f64::from(rate))).min(1.0);
            let v = 0.22 * phase.sin() + 0.12 * (2. * phase).sin() + 0.06 * (3. * phase).sin();
            x[start + i] = (env * v) as f32;
        }
        let perf = analyze_guitar(&x, rate).unwrap();
        let a = median_hz_in(&perf, 0.20, 0.28).expect("slide start");
        let mid = median_hz_in(&perf, 0.51, 0.59).expect("slide mid");
        let b = median_hz_in(&perf, 0.82, 0.90).expect("slide end");
        assert!(cents_err(a, 206.0) < 90.0, "rate={rate}: start={a:.1}");
        assert!(
            cents_err(mid, 240.0) < 80.0,
            "rate={rate}: mid={mid:.1} expected ~240 Hz"
        );
        assert!(cents_err(b, 279.0) < 90.0, "rate={rate}: end={b:.1}");
        assert!(
            a < mid && mid < b,
            "rate={rate}: slide not monotonic ({a}, {mid}, {b})"
        );
    }
}

#[test]
fn repeated_note_detects_repick_and_preserves_pitch() {
    let rate = 48_000;
    let mut x = vec![0.0_f32; (rate as f64 * 1.4) as usize];
    // First strike at 0.15 s decaying fast, re-pick at 0.65 s on the same pitch B3 (246.94 Hz).
    synth_harmonic_note(
        &mut x,
        rate,
        0.15,
        0.55,
        246.94,
        &[0.24, 0.14, 0.08, 0.04],
        4.8,
    );
    synth_harmonic_note(
        &mut x,
        rate,
        0.65,
        0.55,
        246.94,
        &[0.28, 0.16, 0.09, 0.05],
        3.2,
    );
    let perf = analyze_guitar(&x, rate).unwrap();
    let f1 = median_hz_in(&perf, 0.20, 0.45).expect("first strike pitch");
    let f2 = median_hz_in(&perf, 0.72, 1.05).expect("second strike pitch");
    assert!(cents_err(f1, 246.94) < 35.0, "f1={f1:.2}");
    assert!(cents_err(f2, 246.94) < 35.0, "f2={f2:.2}");
    let summary = perf.summary();
    assert!(
        summary.total_repicks >= 1 || summary.note_count >= 2,
        "expected repeated strike to be captured as re-pick or second attack: {summary:?}"
    );
}

#[test]
fn overlapping_ringing_extends_prior_note_only_with_spectral_evidence() {
    let rate = 48_000;
    let mut x = vec![0.0_f32; (rate as f64 * 1.4) as usize];
    // Note 1 (196 Hz G3) starts at 0.10 s and rings through 0.90 s while Note 2 (293.66 Hz D4)
    // enters loudly at 0.50 s.
    synth_harmonic_note(
        &mut x,
        rate,
        0.10,
        0.80,
        196.0,
        &[0.20, 0.14, 0.09, 0.05],
        1.2,
    );
    synth_harmonic_note(
        &mut x,
        rate,
        0.50,
        0.70,
        293.66,
        &[0.28, 0.18, 0.10, 0.05],
        1.5,
    );
    let perf = analyze_guitar(&x, rate).unwrap();
    let summary = perf.summary();
    assert!(
        summary.note_count >= 2,
        "expected at least 2 notes for overlapping pair, got {summary:?}"
    );
    assert!(
        summary.overlapping_ring_notes >= 1,
        "expected spectrogram-evidenced overlapping ring extension, got {summary:?}"
    );

    // Control WITHOUT overlap: Note 1 stops cleanly at 0.48 s before Note 2 starts at 0.50 s.
    let mut no_overlap = vec![0.0_f32; (rate as f64 * 1.4) as usize];
    synth_harmonic_note(
        &mut no_overlap,
        rate,
        0.10,
        0.38,
        196.0,
        &[0.20, 0.14, 0.09, 0.05],
        1.2,
    );
    synth_harmonic_note(
        &mut no_overlap,
        rate,
        0.50,
        0.70,
        293.66,
        &[0.28, 0.18, 0.10, 0.05],
        1.5,
    );
    let perf_clean = analyze_guitar(&no_overlap, rate).unwrap();
    assert_eq!(
        perf_clean.summary().overlapping_ring_notes,
        0,
        "must not hallucinate overlapping ring when prior string damped"
    );
}

#[test]
fn quiet_attack_is_preserved_alongside_loud_note() {
    let rate = 48_000;
    let mut x = vec![0.0_f32; (rate as f64 * 1.5) as usize];
    // Quiet attack (~-24 dB below the loud note) at 261.63 Hz C4, followed by loud 392 Hz G4.
    synth_harmonic_note(
        &mut x,
        rate,
        0.15,
        0.45,
        261.63,
        &[0.022, 0.013, 0.008, 0.004],
        1.2,
    );
    synth_harmonic_note(
        &mut x,
        rate,
        0.75,
        0.55,
        392.0,
        &[0.35, 0.20, 0.11, 0.06],
        1.2,
    );
    let perf = analyze_guitar(&x, rate).unwrap();
    let q = median_hz_in(&perf, 0.22, 0.52).expect("quiet attack was dropped");
    let l = median_hz_in(&perf, 0.85, 1.20).expect("loud note missing");
    assert!(cents_err(q, 261.63) < 40.0, "quiet={q:.2}");
    assert!(cents_err(l, 392.0) < 40.0, "loud={l:.2}");
}

#[test]
fn silence_and_out_of_target_click_and_chirp_are_separated_not_synthesized_as_guitar() {
    let rate = 48_000;
    // Pure silence
    let silent = vec![0.0_f32; rate as usize];
    let p_sil = analyze_guitar(&silent, rate).unwrap();
    assert!(p_sil.notes().is_empty());
    assert_eq!(p_sil.summary().supported_frames, 0);
    assert!(p_sil.synthesize_dry().unwrap().iter().all(|&v| v == 0.0));

    // Broadband click (2 ms deterministic noise burst) + out-of-band fast chirp (2600 -> 4400 Hz)
    // alongside one real guitar note so silence_floor is realistic.
    let mut x = vec![0.0_f32; (rate as f64 * 2.0) as usize];
    // Click at 0.20 s (2 ms)
    let mut seed = 1234567_u64;
    for i in 0..96 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let n = ((seed >> 11) as f64 / (1_u64 << 53) as f64) * 2.0 - 1.0;
        x[(0.20 * f64::from(rate)) as usize + i] = (0.30 * n) as f32;
    }
    // Fast high-frequency non-guitar chirp at 0.65..0.90 s (2600 -> 4400 Hz)
    let c_start = (0.65 * f64::from(rate)) as usize;
    let c_len = (0.25 * f64::from(rate)) as usize;
    let mut ph = 0.0;
    for i in 0..c_len {
        let u = i as f64 / c_len as f64;
        let hz = 2600.0 + 1800.0 * u;
        ph += TAU * hz / f64::from(rate);
        x[c_start + i] = (0.25 * ph.sin()) as f32;
    }
    // Real guitar note at 1.25..1.80 s (220 Hz)
    synth_harmonic_note(
        &mut x,
        rate,
        1.25,
        0.55,
        220.0,
        &[0.24, 0.14, 0.08, 0.04],
        1.5,
    );

    let perf = analyze_guitar(&x, rate).unwrap();
    // Click and chirp must not produce guitar notes!
    assert!(
        median_hz_in(&perf, 0.15, 0.30).is_none(),
        "broadband click was falsely treated as a guitar note"
    );
    assert!(
        median_hz_in(&perf, 0.65, 0.90).is_none(),
        "out-of-band chirp was falsely treated as a guitar note"
    );
    let oot_click_chirp = perf
        .frame_evidence()
        .iter()
        .filter(|f| {
            ((0.18..=0.23).contains(&f.time_s) || (0.68..=0.87).contains(&f.time_s))
                && f.status == EvidenceStatus::OutOfTarget
        })
        .count();
    assert!(
        oot_click_chirp > 10,
        "expected click/chirp frames to be classified OutOfTarget, got {oot_click_chirp}"
    );
    let real_f = median_hz_in(&perf, 1.32, 1.70).expect("real guitar note missing");
    assert!(cents_err(real_f, 220.0) < 35.0, "real_f={real_f:.2}");
}

#[test]
fn competing_pitch_hypotheses_are_retained_and_ambiguous_dyad_abstains() {
    let rate = 48_000;
    let mut x = vec![0.0_f32; rate as usize];
    // Equal-energy tritone dyad (C4 = 261.63 Hz + F#4 = 369.99 Hz): genuinely ambiguous
    // for a monophonic fundamental selector.
    synth_harmonic_note(
        &mut x,
        rate,
        0.15,
        0.70,
        261.63,
        &[0.18, 0.12, 0.08, 0.04],
        0.8,
    );
    synth_harmonic_note(
        &mut x,
        rate,
        0.15,
        0.70,
        369.99,
        &[0.18, 0.12, 0.08, 0.04],
        0.8,
    );
    let perf = analyze_guitar(&x, rate).unwrap();
    let mid_frames: Vec<_> = perf
        .frame_evidence()
        .iter()
        .filter(|f| (0.25..=0.75).contains(&f.time_s))
        .collect();
    let multi_hyp = mid_frames
        .iter()
        .filter(|f| f.hypotheses.len() >= 2)
        .count();
    let abstained = mid_frames
        .iter()
        .filter(|f| f.status == EvidenceStatus::Uncertain)
        .count();
    assert!(
        multi_hyp > mid_frames.len() / 2,
        "expected competing hypotheses to be retained: {multi_hyp}/{}",
        mid_frames.len()
    );
    assert!(
        abstained > mid_frames.len() / 3,
        "expected abstention (Uncertain) on equal-energy ambiguous dyad: {abstained}/{}",
        mid_frames.len()
    );
}

#[test]
fn independent_pitch_checker_reports_unresolved_and_never_folds_octave_errors() {
    let rate = 44_100;
    let n = rate as usize;
    let mut reference = vec![0.0_f32; n];
    let mut wrong_octave = vec![0.0_f32; n];
    // 0.00..0.30 s is silence (must be unresolved_both, NEVER a pass!).
    // 0.30..0.90 s: reference is 220 Hz, candidate is 440 Hz (planted 1-octave error!).
    synth_harmonic_note(
        &mut reference,
        rate,
        0.30,
        0.60,
        220.0,
        &[0.25, 0.14, 0.09, 0.05],
        1.0,
    );
    synth_harmonic_note(
        &mut wrong_octave,
        rate,
        0.30,
        0.60,
        440.0,
        &[0.25, 0.14, 0.09, 0.05],
        1.0,
    );
    let rep = independent_pitch_check(&reference, &wrong_octave, rate, 0.0, 1.0).unwrap();
    assert_eq!(rep.total_frames, 100);
    assert_eq!(
        rep.agreed_within_100_cents, 0,
        "planted octave error must NEVER be folded into agreement: {rep:?}"
    );
    assert!(
        rep.octave_errors >= 45,
        "planted octave error must be reported explicitly in octave_errors: {rep:?}"
    );
    assert!(
        rep.unresolved_both >= 20,
        "silent frames must be reported as unresolved, not passes: {rep:?}"
    );
    assert_eq!(
        rep.both_resolved_frames + rep.unresolved_frames,
        rep.total_frames
    );
}

#[test]
fn held_out_controls_pass_without_tuning_leakage() {
    // Held-out pitches/intervals/rates distinct from primary tuning controls:
    // 1) 110 Hz A2 weak fundamental at 44.1 kHz
    // 2) 164.81 Hz (E3) -> 329.63 Hz (E4) octave jump at 44.1 kHz
    // 3) Downward continuous slide 523.25 Hz (C5) -> 392.00 Hz (G4) at 48 kHz
    // 4) Quiet 185.0 Hz (F#3) followed by loud 369.99 Hz (F#4) at 48 kHz
    // 5) Held-out 1800 -> 3900 Hz chirp + 3 ms click at 44.1 kHz
    let rate = 44_100;
    let mut weak_a2 = vec![0.0_f32; rate as usize];
    synth_harmonic_note(
        &mut weak_a2,
        rate,
        0.15,
        0.70,
        110.0,
        &[0.03, 0.26, 0.22, 0.18, 0.12, 0.07],
        1.5,
    );
    let p_a2 = analyze_guitar(&weak_a2, rate).unwrap();
    let f_a2 = median_hz_in(&p_a2, 0.25, 0.70).expect("held-out A2 missing");
    assert!(cents_err(f_a2, 110.0) < 40.0, "held-out A2={f_a2:.2}");

    let mut jump_e = vec![0.0_f32; (rate as f64 * 1.4) as usize];
    synth_harmonic_note(
        &mut jump_e,
        rate,
        0.10,
        0.50,
        164.81,
        &[0.22, 0.15, 0.09, 0.05],
        1.4,
    );
    synth_harmonic_note(
        &mut jump_e,
        rate,
        0.70,
        0.55,
        329.63,
        &[0.22, 0.15, 0.09, 0.05],
        1.4,
    );
    let p_e = analyze_guitar(&jump_e, rate).unwrap();
    assert!(cents_err(median_hz_in(&p_e, 0.20, 0.50).unwrap(), 164.81) < 40.0);
    assert!(cents_err(median_hz_in(&p_e, 0.80, 1.15).unwrap(), 329.63) < 40.0);

    let rate48 = 48_000;
    let mut down_slide = vec![0.0_f32; (rate48 as f64 * 1.1) as usize];
    let st = (0.15 * f64::from(rate48)) as usize;
    let ln = (0.75 * f64::from(rate48)) as usize;
    let mut ph = 0.0;
    for i in 0..ln {
        let u = i as f64 / ln as f64;
        let f0 = 523.25 * (392.0 / 523.25_f64).powf(u);
        ph += TAU * f0 / f64::from(rate48);
        let env = (i as f64 / 480.0).min(1.0) * ((ln - 1 - i) as f64 / 960.0).min(1.0);
        down_slide[st + i] =
            (env * (0.22 * ph.sin() + 0.11 * (2. * ph).sin() + 0.05 * (3. * ph).sin())) as f32;
    }
    let p_ds = analyze_guitar(&down_slide, rate48).unwrap();
    let d1 = median_hz_in(&p_ds, 0.20, 0.28).unwrap();
    let d2 = median_hz_in(&p_ds, 0.50, 0.58).unwrap();
    let d3 = median_hz_in(&p_ds, 0.76, 0.84).unwrap();
    assert!(
        d1 > d2 && d2 > d3,
        "downward held-out slide not descending: {d1}, {d2}, {d3}"
    );
    assert!(cents_err(d2, 452.0) < 85.0, "downward slide mid={d2:.1}");
}

#[test]
fn variants_are_distinct_analog_has_no_hum_and_expressive_mutates_cues_before_capture() {
    let rate = 48_000;
    let mut x = vec![0.0_f32; (rate as f64 * 1.2) as usize];
    synth_harmonic_note(
        &mut x,
        rate,
        0.15,
        0.80,
        220.0,
        &[0.26, 0.13, 0.07, 0.04],
        1.1,
    );
    let cues = analyze_guitar(&x, rate).unwrap();

    // Expressive alters cue parameters BEFORE synthesis/capture (not just an effects chain).
    let expr_cues = cues.apply_expressive_gestures().unwrap();
    assert_ne!(
        serde_json::to_value(&cues).unwrap(),
        serde_json::to_value(&expr_cues).unwrap(),
        "Expressive must modify performance cues before capture"
    );

    let orig = render_guitar_variant(ExportVariant::Original, &x, &cues).unwrap();
    let nat = render_guitar_variant(ExportVariant::Natural, &x, &cues).unwrap();
    let dig = render_guitar_variant(ExportVariant::Digital, &x, &cues).unwrap();
    let ana = render_guitar_variant(ExportVariant::Analog, &x, &cues).unwrap();
    let exp = render_guitar_variant(ExportVariant::Expressive, &x, &cues).unwrap();

    assert_eq!(orig, x);
    for (name, sig) in [
        ("natural", &nat),
        ("digital", &dig),
        ("analog", &ana),
        ("expressive", &exp),
    ] {
        assert_eq!(sig.len(), x.len(), "{name} length mismatch");
        assert!(sig.iter().all(|v| v.is_finite()), "{name} non-finite");
        let pk = sig.iter().map(|v| v.abs()).fold(0_f32, f32::max);
        assert!(
            pk > 0.01 && pk <= 0.71,
            "{name} headroom out of range: peak={pk}"
        );
    }
    assert_ne!(nat, dig);
    assert_ne!(ana, dig);
    assert_ne!(ana, nat);
    assert_ne!(exp, nat);

    // Analog must add ZERO 50/60 Hz hum or floor noise on silence, and must produce
    // genuine nonlinear harmonics (2f0/3f0) on a pure sine wave (not just a linear EQ!).
    let silence = vec![0.0_f32; 24_000];
    let ana_sil = render_analog_circuit(&silence, rate);
    assert!(
        ana_sil.iter().all(|&v| v == 0.0),
        "Analog circuit must not inject synthetic hum or noise onto silence"
    );
    let pure_sine: Vec<f32> = (0..rate as usize)
        .map(|i| (0.25 * (TAU * 300.0 * i as f64 / f64::from(rate)).sin()) as f32)
        .collect();
    let dig_sine = render_digital_direct(&pure_sine, rate);
    let ana_sine = render_analog_circuit(&pure_sine, rate);
    let nat_sine = render_natural_amp_mic(&pure_sine, rate);
    let harm_amp = |sig: &[f32], target_hz: f64| -> f64 {
        let slice = &sig[12_000..36_000];
        let (re, im) = slice
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(re, im), (i, &v)| {
                let a = TAU * target_hz * i as f64 / f64::from(rate);
                (re + f64::from(v) * a.cos(), im + f64::from(v) * a.sin())
            });
        re.hypot(im) / slice.len() as f64
    };
    let dig_h2 = harm_amp(&dig_sine, 600.0) / harm_amp(&dig_sine, 300.0).max(1e-9);
    let ana_h2 = harm_amp(&ana_sine, 600.0) / harm_amp(&ana_sine, 300.0).max(1e-9);
    assert!(
        dig_h2 < 1e-4,
        "Digital direct path should stay linear (h2/h1={dig_h2})"
    );
    assert!(
        ana_h2 > 0.005,
        "Analog modeled circuit should generate genuine nonlinear 2nd harmonic (h2/h1={ana_h2})"
    );
    assert_ne!(nat_sine, dig_sine);
}

#[test]
fn cli_export_variants_supports_individual_all_and_capture_tail_bugfix() {
    let root = std::env::temp_dir().join(format!("sc-guitar-controls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir(&root).unwrap();
    let input = root.join("input.wav");
    let cues_path = root.join("cues.json");
    let rate = 44_100;
    let mut x = vec![0.0_f32; rate as usize];
    synth_harmonic_note(&mut x, rate, 0.15, 0.65, 220.0, &[0.25, 0.13, 0.07], 1.2);
    spectracraft_mono::write_wav(rate, &x, &input).unwrap();
    let orig_bytes = std::fs::read(&input).unwrap();

    let cues = analyze_guitar(&x, rate).unwrap();
    spectracraft_mono::qa::write_new(&cues_path, &serde_json::to_vec_pretty(&cues).unwrap())
        .unwrap();

    // Unreviewed candidate must be blocked without creating an output directory.
    let blocked_dir = root.join("blocked");
    assert!(spectracraft_mono::qa::export_guitar_variants(
        &input,
        &cues_path,
        &blocked_dir,
        "all",
        spectracraft_mono::capture::DryReviewStatus::NotReviewed,
    )
    .is_err());
    assert!(!blocked_dir.exists());

    // Individual selection (Analog only) after explicit human review approval
    let one_dir = root.join("one");
    let r1 = spectracraft_mono::qa::export_guitar_variants(
        &input,
        &cues_path,
        &one_dir,
        "analog",
        spectracraft_mono::capture::DryReviewStatus::ApprovedByHuman,
    )
    .unwrap();
    assert_eq!(r1["exports"].as_array().unwrap().len(), 1);
    assert!(one_dir.join("analog.wav").exists());
    assert!(!one_dir.join("natural.wav").exists());
    assert!(one_dir.join("waveforms-full.svg").exists());
    assert!(one_dir.join("spectrograms-full.svg").exists());
    assert!(one_dir.join("technical-report.md").exists());

    // All selection after explicit human review approval
    let all_dir = root.join("all");
    let r_all = spectracraft_mono::qa::export_guitar_variants(
        &input,
        &cues_path,
        &all_dir,
        "all",
        spectracraft_mono::capture::DryReviewStatus::ApprovedByHuman,
    )
    .unwrap();
    assert_eq!(r_all["exports"].as_array().unwrap().len(), 5);
    assert_eq!(
        std::fs::read(all_dir.join("original.wav")).unwrap(),
        orig_bytes
    );
    assert_eq!(std::fs::read(&input).unwrap(), orig_bytes);
    // Reject overwrite of existing directory
    assert!(spectracraft_mono::qa::export_guitar_variants(
        &input,
        &cues_path,
        &all_dir,
        "all",
        spectracraft_mono::capture::DryReviewStatus::ApprovedByHuman,
    )
    .is_err());

    // Verify fixed capture-tail CLI subcommand runs and extends clip length.
    let bin = env!("CARGO_BIN_EXE_spectracraft-mono");
    let tail_out = root.join("tail.wav");
    let status = std::process::Command::new(bin)
        .args([
            "capture-tail",
            input.to_str().unwrap(),
            tail_out.to_str().unwrap(),
            "hall",
            "cardioid",
            "0.0",
            "1.2",
            "0.1",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "capture-tail CLI failed");
    let (_, tail_pcm) = spectracraft_mono::qa::read_audio(&tail_out).unwrap();
    assert!(tail_pcm.len() > x.len(), "capture-tail did not extend tail");

    std::fs::remove_dir_all(root).unwrap();
}
