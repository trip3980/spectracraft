use rustfft::{num_complex::Complex, FftPlanner};
use spectracraft_mono::{
    acoustics_physics,
    pipeline::{self, PluginId, PluginSlotConfig, PostDryPipelineConfig},
    research_diagnostics,
    string_state::{BodyResonanceConfig, BodyResonator},
    studio_catalog::{
        mic_catalog, source_catalog, surface_catalog, StudioRig, DIRECT_REF_DISTANCE_M,
        FEET_TO_METRES,
    },
    translation_layer,
};
use std::f64::consts::TAU;

const RATE: u32 = 48_000;

fn xorshift64(state: &mut u64) -> f64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    ((*state >> 11) as f64 / ((1_u64 << 53) as f64)) * 2.0 - 1.0
}

fn rms_db(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return -120.0;
    }
    let mean_sq = samples
        .iter()
        .map(|&s| f64::from(s) * f64::from(s))
        .sum::<f64>()
        / samples.len() as f64;
    if mean_sq <= 1e-24 {
        -120.0
    } else {
        10.0 * mean_sq.log10()
    }
}

fn dc_offset(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().map(|&s| f64::from(s)).sum::<f64>() / samples.len() as f64
}

fn peak_abs(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |m, &v| m.max(v.abs()))
}

fn normalize_rms_dbfs(samples: &mut [f32], target_dbfs: f64) {
    let cur = rms_db(samples);
    let gain = 10.0_f64.powf((target_dbfs - cur) / 20.0) as f32;
    for s in samples.iter_mut() {
        *s *= gain;
    }
}

/// Measure maximum narrowband spectral prominence (in dB above local neighborhood median)
/// to detect tonal whistles or narrowband ringing artifacts.
fn max_narrowband_prominence_db(
    samples: &[f32],
    rate: u32,
    min_hz: f64,
    max_hz: f64,
) -> (f64, f64) {
    let nfft = 8192_usize;
    let hop = nfft / 2;
    let n_bins = nfft / 2 + 1;
    let fs = f64::from(rate);
    let df = fs / nfft as f64;
    let win: Vec<f64> = (0..nfft)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / nfft as f64).cos()))
        .collect();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(nfft);
    let mut avg_psd = vec![0.0_f64; n_bins];
    let mut buf = vec![Complex::new(0.0_f64, 0.0_f64); nfft];
    let mut frames = 0usize;
    let mut pos = 0usize;
    while pos + nfft <= samples.len() {
        for i in 0..nfft {
            buf[i] = Complex::new(f64::from(samples[pos + i]) * win[i], 0.0);
        }
        fft.process(&mut buf);
        for k in 0..n_bins {
            avg_psd[k] += buf[k].norm_sqr();
        }
        frames += 1;
        pos += hop;
    }
    if frames == 0 {
        return (0.0, 0.0);
    }
    for v in &mut avg_psd {
        *v /= frames as f64;
    }
    let k_lo = ((min_hz / df).ceil() as usize).max(16);
    let k_hi = ((max_hz / df).floor() as usize).min(n_bins.saturating_sub(17));
    let mut worst_db = 0.0_f64;
    let mut worst_hz = 0.0_f64;
    let mut scratch = Vec::with_capacity(32);
    for k in k_lo..=k_hi {
        scratch.clear();
        for b in (k - 16)..(k - 4) {
            scratch.push(avg_psd[b]);
        }
        for b in (k + 5)..=(k + 16) {
            scratch.push(avg_psd[b]);
        }
        scratch.sort_by(f64::total_cmp);
        let med = scratch[scratch.len() / 2].max(1e-24);
        let prom_db = 10.0 * (avg_psd[k] / med).max(1e-12).log10();
        if prom_db > worst_db {
            worst_db = prom_db;
            worst_hz = k as f64 * df;
        }
    }
    (worst_db, worst_hz)
}

fn check_parseval_slices(samples: &[f32], label: &str) {
    let n = 4096_usize.min(samples.len());
    let mid = samples.len() / 2;
    let start = mid.saturating_sub(n / 2);
    let frame = &samples[start..start + n];
    let (full, one_sided) = research_diagnostics::check_rustfft_real_frame_f32(frame)
        .unwrap_or_else(|e| panic!("Parseval check errored for {label}: {e}"));
    assert!(
        full.passes && one_sided.passes,
        "Parseval failed for {label}: full={full:?}, one_sided={one_sided:?}"
    );
}

fn full_post_dry_cfg() -> PostDryPipelineConfig {
    let mut cfg = PostDryPipelineConfig::default();
    cfg.mic_enabled = true;
    cfg.room_enabled = true;
    cfg.rig = StudioRig::sm7b_foam_reference();
    cfg.plugin_slot = PluginSlotConfig {
        enabled: true,
        plugin_id: PluginId::PhaseLimiter,
        eq_low_db: 0.0,
        eq_high_db: 0.0,
        amount: 0.0, // unity drive, limiter active at -1.0 dBFS ceiling
        ceiling_dbfs: -1.0,
    };
    cfg
}

#[test]
fn test_01_white_noise_full_band() {
    let n = RATE as usize * 5;
    let mut rng = 0x1234_5678_9abc_def1_u64;
    let mut noise: Vec<f32> = (0..n).map(|_| xorshift64(&mut rng) as f32).collect();
    // Remove tiny sample-mean DC from finite PRNG realization and calibrate to -12 dBFS.
    let raw_dc = dc_offset(&noise) as f32;
    for s in &mut noise {
        *s -= raw_dc;
    }
    normalize_rms_dbfs(&mut noise, -12.0);
    let in_rms = rms_db(&noise);
    println!(
        "[Test 1] Input white noise: rms={in_rms:.3} dBFS, dc={:.6}, peak={:.4}",
        dc_offset(&noise),
        peak_abs(&noise)
    );

    // A) End-to-end Bedroom Translator -> Stage 1 cue extraction & synth
    let translated = translation_layer::analyze_from_bedroom_recording(&noise, RATE)
        .expect("bedroom translation on white noise");
    let (conditioned, profile) = translation_layer::condition_bedroom_for_analysis(&noise, RATE);
    println!(
        "[Test 1] Translator profile: hum={:?}, raw_rms={:.2} dBFS, cond_rms={:.2} dBFS, notes={}",
        profile.mains_hum_hz,
        profile.raw_rms_dbfs,
        rms_db(&conditioned),
        translated.performance.notes().len()
    );
    assert!(
        profile.mains_hum_hz.is_none(),
        "White noise should not false-trigger mains hum detection: {:?}",
        profile.mains_hum_hz
    );
    let stage1_dry = translated
        .performance
        .synthesize_dry_with_body_resonance(&BodyResonanceConfig::default())
        .expect("stage 1 dry synth");
    check_parseval_slices(&stage1_dry, "test1_stage1_dry");

    // B) DSP signal chain: Stage 1 BodyResonator -> Stage 3 Mic -> Stage 4 Room -> Stage 5 Limiter
    let mut body_buf: Vec<f64> = noise.iter().map(|&s| f64::from(s)).collect();
    let mut body = BodyResonator::new(f64::from(RATE), BodyResonanceConfig::default());
    body.process_buffer(&mut body_buf);
    let body_f32: Vec<f32> = body_buf.iter().map(|&v| v as f32).collect();

    let sources = source_catalog();
    let mics = mic_catalog();
    let surfaces = surface_catalog();
    let cfg = full_post_dry_cfg();

    // Check individual stage gains + full chain
    let mut s3_cfg = cfg.clone();
    s3_cfg.room_enabled = false;
    s3_cfg.plugin_slot.enabled = false;
    let out_s3 =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &s3_cfg, &sources, &mics, &surfaces);

    let mut s4_cfg = cfg.clone();
    s4_cfg.mic_enabled = false;
    s4_cfg.plugin_slot.enabled = false;
    let out_s4 =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &s4_cfg, &sources, &mics, &surfaces);

    let out_full =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &cfg, &sources, &mics, &surfaces);

    let body_rms = rms_db(&body_f32);
    let s3_rms = rms_db(&out_s3.samples);
    let s4_rms = rms_db(&out_s4.samples);
    let full_rms = rms_db(&out_full.samples);
    let full_dc = dc_offset(&out_full.samples);
    let full_peak = peak_abs(&out_full.samples);
    let (body_prom_db, body_prom_hz) =
        max_narrowband_prominence_db(&body_f32, RATE, 60.0, 18_000.0);
    let (full_prom_db, full_prom_hz) =
        max_narrowband_prominence_db(&out_full.samples, RATE, 60.0, 18_000.0);
    let loudness = acoustics_physics::measure_loudness_bs1770(&[&out_full.samples], RATE);

    println!(
        "[Test 1] RMS chain: in={in_rms:.2} -> body={body_rms:.2} ({:+.2} dB) -> s3_only={s3_rms:.2} -> s4_only={s4_rms:.2} -> full={full_rms:.2} ({:+.2} dB)",
        body_rms - in_rms,
        full_rms - in_rms
    );
    println!(
        "[Test 1] Full output: dc={full_dc:.6}, peak={full_peak:.4}, tp={:.2} dBTP, lufs={:.2}, body_prom={body_prom_db:.2} dB @ {body_prom_hz:.1} Hz, full_prom={full_prom_db:.2} dB @ {full_prom_hz:.1} Hz",
        loudness.true_peak_dbtp, loudness.integrated_lufs
    );

    // Checks
    check_parseval_slices(&noise, "test1_input");
    check_parseval_slices(&conditioned, "test1_conditioned");
    check_parseval_slices(&body_f32, "test1_body");
    check_parseval_slices(&out_full.samples, "test1_full");

    assert!(full_dc.abs() < 1e-3, "Excessive DC offset: {full_dc}");
    assert!(full_peak < 1.0, "Output clipped: peak={full_peak}");
    assert!(
        full_prom_db < 6.0,
        "Narrowband tonal artifact in output: {full_prom_db:.2} dB @ {full_prom_hz:.1} Hz"
    );
    // Body broadband passband matches 20*log10(dry_mix = 0.70) = -3.10 dB within 0.5 dB.
    let expected_body_db = 20.0 * BodyResonanceConfig::default().dry_mix.log10();
    assert!(
        ((body_rms - in_rms) - expected_body_db).abs() <= 0.5,
        "Body broadband gain drifted from dry_mix: got {:+.2} dB, expected {expected_body_db:+.2} dB",
        body_rms - in_rms
    );
    // Stage 4 foam reference room under canonical Timing::Physical (H15) obeys physical 1/r
    // direct-path attenuation (20*log10(DIRECT_REF_DISTANCE_M / d)) at 2.0 ft within 0.5 dB
    // across the active signal window (with sinc16 direct-path fractional delay eliminating HF droop).
    let dist_m = 2.0 * f64::from(FEET_TO_METRES);
    let expected_s4_phys_db = 20.0 * (DIRECT_REF_DISTANCE_M / dist_m).log10();
    let s4_active_rms = rms_db(&out_s4.samples[..body_f32.len().min(out_s4.samples.len())]);
    assert!(
        ((s4_active_rms - body_rms) - expected_s4_phys_db).abs() <= 0.5,
        "Room stage physical 1/r energy drifted > 0.5 dB: got {:+.2} dB, expected {expected_s4_phys_db:+.2} dB (full-buffer {:+.2} dB)",
        s4_active_rms - body_rms,
        s4_rms - body_rms
    );
    // Fixed-block audition mode (Timing::AlignDirect) preserves broadband unity energy within 0.5 dB.
    let out_s4_aligned = pipeline::run_post_dry_pipeline_fixed_block(
        &body_f32, RATE, &s4_cfg, &sources, &mics, &surfaces,
    );
    let s4_aligned_rms = rms_db(&out_s4_aligned.samples);
    assert!(
        (s4_aligned_rms - body_rms).abs() <= 0.5,
        "Aligned-direct room stage energy drift exceeded 0.5 dB: {:+.2} dB",
        s4_aligned_rms - body_rms
    );
}

fn band_rms_db(samples: &[f32], rate: u32, f_lo: f64, f_hi: f64) -> f64 {
    let nfft = 16384_usize
        .min(samples.len().next_power_of_two() / 2)
        .max(2048);
    let hop = nfft / 2;
    let fs = f64::from(rate);
    let df = fs / nfft as f64;
    let win: Vec<f64> = (0..nfft)
        .map(|i| 0.5 * (1.0 - (TAU * i as f64 / nfft as f64).cos()))
        .collect();
    let win_pow: f64 = win.iter().map(|w| w * w).sum();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(nfft);
    let mut buf = vec![Complex::new(0.0_f64, 0.0_f64); nfft];
    let k_lo = ((f_lo / df).floor() as usize).max(1);
    let k_hi = ((f_hi / df).ceil() as usize).min(nfft / 2 - 1);
    let mut acc = 0.0_f64;
    let mut frames = 0usize;
    let mut pos = 0usize;
    while pos + nfft <= samples.len() {
        for i in 0..nfft {
            buf[i] = Complex::new(f64::from(samples[pos + i]) * win[i], 0.0);
        }
        fft.process(&mut buf);
        let mut band_e = 0.0_f64;
        for k in k_lo..=k_hi {
            band_e += 2.0 * buf[k].norm_sqr() / (nfft as f64 * win_pow);
        }
        acc += band_e;
        frames += 1;
        pos += hop;
    }
    if frames == 0 || acc <= 1e-24 {
        -120.0
    } else {
        10.0 * (acc / frames as f64).log10()
    }
}

#[test]
fn test_02_brown_noise_1_over_f2() {
    let n = RATE as usize * 5;
    let mut rng = 0x9e37_79b9_7f4a_7c15_u64;
    // Leaky integrator (fc ~ 12 Hz) produces genuine 1/f^2 brown noise across 20 Hz..20 kHz
    // without unbounded random-walk DC in the test generator itself.
    let leak = (-TAU * 12.0 / f64::from(RATE)).exp();
    let mut y = 0.0_f64;
    let mut brown = Vec::with_capacity(n);
    for _ in 0..n {
        y = leak * y + xorshift64(&mut rng);
        brown.push(y as f32);
    }
    let raw_dc = dc_offset(&brown) as f32;
    for s in &mut brown {
        *s -= raw_dc;
    }
    normalize_rms_dbfs(&mut brown, -12.0);
    let in_rms = rms_db(&brown);
    let in_lf = band_rms_db(&brown, RATE, 25.0, 250.0);
    println!(
        "[Test 2] Input brown noise: rms={in_rms:.3} dBFS, lf(25-250Hz)={in_lf:.2} dBFS, dc={:.6}, peak={:.4}",
        dc_offset(&brown),
        peak_abs(&brown)
    );

    // A) Bedroom Translator -> Stage 1
    let (conditioned, profile) = translation_layer::condition_bedroom_for_analysis(&brown, RATE);
    let translated = translation_layer::analyze_from_bedroom_recording(&brown, RATE)
        .expect("bedroom translation on brown noise");
    println!(
        "[Test 2] Translator: hum={:?}, cond_rms={:.2} dBFS, notes={}",
        profile.mains_hum_hz,
        rms_db(&conditioned),
        translated.performance.notes().len()
    );
    check_parseval_slices(&conditioned, "test2_conditioned");

    // B) Stage 1 Body -> Stage 3 Mic -> Stage 4 Room -> Stage 5 Limiter
    let mut body_buf: Vec<f64> = brown.iter().map(|&s| f64::from(s)).collect();
    let mut body = BodyResonator::new(f64::from(RATE), BodyResonanceConfig::default());
    body.process_buffer(&mut body_buf);
    let body_f32: Vec<f32> = body_buf.iter().map(|&v| v as f32).collect();

    let sources = source_catalog();
    let mics = mic_catalog();
    let surfaces = surface_catalog();
    let cfg = full_post_dry_cfg();
    let out_full =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &cfg, &sources, &mics, &surfaces);

    let body_lf = band_rms_db(&body_f32, RATE, 25.0, 250.0);
    let full_lf = band_rms_db(&out_full.samples, RATE, 25.0, 250.0);
    let full_rms = rms_db(&out_full.samples);
    let full_dc = dc_offset(&out_full.samples);
    let full_peak = peak_abs(&out_full.samples);

    println!(
        "[Test 2] Chain: in_lf={in_lf:.2} -> body_lf={body_lf:.2} ({:+.2} dB) -> full_lf={full_lf:.2} ({:+.2} dB), full_rms={full_rms:.2}, dc={full_dc:.6}, peak={full_peak:.4}",
        body_lf - in_lf,
        full_lf - in_lf
    );

    check_parseval_slices(&brown, "test2_input");
    check_parseval_slices(&body_f32, "test2_body");
    check_parseval_slices(&out_full.samples, "test2_full");

    assert!(
        full_dc.abs() < 2e-3,
        "Runaway DC offset on brown noise: {full_dc}"
    );
    assert!(full_peak < 1.0, "Brown noise clipped: peak={full_peak}");
    assert!(
        (body_lf - in_lf) > -6.0 && (body_lf - in_lf) < 4.5,
        "Body filter collapsed or exaggerated LF energy: {:+.2} dB",
        body_lf - in_lf
    );
    assert!(
        (full_lf - in_lf) > -8.0 && (full_lf - in_lf) < 4.5,
        "Full pipeline collapsed or exaggerated LF energy: {:+.2} dB",
        full_lf - in_lf
    );
}

fn generate_log_sweep(
    rate: u32,
    duration_s: f64,
    f0: f64,
    f1: f64,
    amp: f64,
    fade_s: f64,
) -> Vec<f32> {
    let n = (f64::from(rate) * duration_s).round() as usize;
    let fs = f64::from(rate);
    let k = (f1 / f0).ln();
    let fade_n = (fade_s * fs).round() as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / fs;
        let phase = TAU * f0 * duration_s / k * ((t * k / duration_s).exp() - 1.0);
        let mut w = 1.0_f64;
        if i < fade_n {
            w = 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / fade_n as f64).cos());
        } else if i + fade_n >= n {
            let rem = n - 1 - i;
            w = 0.5 * (1.0 - (std::f64::consts::PI * rem as f64 / fade_n as f64).cos());
        }
        out.push((amp * w * phase.sin()) as f32);
    }
    out
}

#[test]
fn test_03_sine_sweep_20hz_to_20khz() {
    let dur_s = 10.0_f64;
    let f0 = 20.0_f64;
    let f1 = 20_000.0_f64;
    let sweep = generate_log_sweep(RATE, dur_s, f0, f1, 0.25, 0.015);
    let fs = f64::from(RATE);
    let ln_ratio = (f1 / f0).ln();

    // Bedroom Translator conditioning check
    let (cond, prof) = translation_layer::condition_bedroom_for_analysis(&sweep, RATE);
    println!(
        "[Test 3] Translator: hum={:?}, cond_rms={:.2} dBFS, cond_peak={:.4}",
        prof.mains_hum_hz,
        rms_db(&cond),
        peak_abs(&cond)
    );
    assert!(
        prof.mains_hum_hz.is_none(),
        "Sweep should not trigger stationary mains hum detector: {:?}",
        prof.mains_hum_hz
    );

    // Stage 1 Body -> Stage 3 Mic -> Stage 4 Room -> Stage 5 Limiter
    let mut body_buf: Vec<f64> = sweep.iter().map(|&s| f64::from(s)).collect();
    let mut body = BodyResonator::new(fs, BodyResonanceConfig::default());
    body.process_buffer(&mut body_buf);
    let body_f32: Vec<f32> = body_buf.iter().map(|&v| v as f32).collect();

    let sources = source_catalog();
    let mics = mic_catalog();
    let surfaces = surface_catalog();
    let cfg = full_post_dry_cfg();
    let mut room_only_cfg = cfg.clone();
    room_only_cfg.mic_enabled = false;
    room_only_cfg.plugin_slot.enabled = false;
    let mut mic_only_cfg = cfg.clone();
    mic_only_cfg.room_enabled = false;
    mic_only_cfg.plugin_slot.enabled = false;
    let out_mic =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &mic_only_cfg, &sources, &mics, &surfaces);
    let out_room = pipeline::run_post_dry_pipeline(
        &body_f32,
        RATE,
        &room_only_cfg,
        &sources,
        &mics,
        &surfaces,
    );
    let out_full =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &cfg, &sources, &mics, &surfaces);

    // Measure body resonance peaks at 100 Hz, 200 Hz vs 1000 Hz using instantaneous sweep time
    let time_of_freq = |f: f64| -> usize {
        let t = dur_s * (f / f0).ln() / ln_ratio;
        (t * fs).round() as usize
    };
    let local_rms_at = |sig: &[f32], f: f64| -> f64 {
        let c = time_of_freq(f);
        let half = (0.040 * fs) as usize;
        rms_db(&sig[c.saturating_sub(half)..(c + half).min(sig.len())])
    };
    let body_100 = local_rms_at(&body_f32, 100.0);
    let body_200 = local_rms_at(&body_f32, 200.0);
    let body_1000 = local_rms_at(&body_f32, 1000.0);
    let peak_100_db = body_100 - body_1000;
    let peak_200_db = body_200 - body_1000;

    // Frame-by-frame continuity check across 35 Hz .. 12 kHz (20 ms frames, 10 ms hop)
    let win = (0.020 * fs) as usize;
    let hop = (0.010 * fs) as usize;
    let start_i = time_of_freq(35.0);
    let end_i = time_of_freq(12_000.0);
    let scan_sweep = |sig: &[f32]| -> (f64, f64, f64, f64) {
        let mut prev_db: Option<f64> = None;
        let mut min_db = f64::INFINITY;
        let mut min_hz = 0.0_f64;
        let mut max_jump_db = 0.0_f64;
        let mut max_jump_hz = 0.0_f64;
        let mut pos = start_i;
        while pos + win <= end_i {
            let cur = rms_db(&sig[pos..pos + win]);
            let t = (pos as f64) / fs;
            let hz = f0 * (t * ln_ratio / dur_s).exp();
            if cur < min_db {
                min_db = cur;
                min_hz = hz;
            }
            if let Some(p) = prev_db {
                let jump = (cur - p).abs();
                if jump > max_jump_db {
                    max_jump_db = jump;
                    max_jump_hz = hz;
                }
            }
            prev_db = Some(cur);
            pos += hop;
        }
        (min_db, min_hz, max_jump_db, max_jump_hz)
    };
    let plan_sm7 = spectracraft_mono::studio_catalog::capture_paths_6band(
        &cfg.rig,
        &sources,
        &mics,
        &surfaces,
        4,
        spectracraft_mono::studio_catalog::Timing::AlignDirect,
    );
    let early_only =
        spectracraft_mono::studio_catalog::render_capture_6band(&body_f32, RATE, &plan_sm7);
    let (early_min, early_min_hz, early_jump, early_jump_hz) = scan_sweep(&early_only);
    // Compute exact physical low-band transfer function H_phys(f) = sum g_0 * exp(-j*2*pi*f*delay)
    // with ZERO crossover phase lag, to see if the 105.7 Hz dip is physical cubic-room SBIR or Lr4 phase lag.
    let mut phys_min_db = f64::INFINITY;
    let mut phys_min_hz = 0.0_f64;
    for fi in 35..=200 {
        let f = fi as f64;
        let (mut re, mut im) = (0.0_f64, 0.0_f64);
        for p in &plan_sm7.paths {
            let ph = -TAU * f * p.delay_s;
            re += p.band_gains[0] * ph.cos();
            im += p.band_gains[0] * ph.sin();
        }
        let mag_db = 20.0 * (re * re + im * im).sqrt().max(1e-9).log10();
        if mag_db < phys_min_db {
            phys_min_db = mag_db;
            phys_min_hz = f;
        }
    }
    println!("[Test 3] Pure physical plan_sm7 low-band H(f) min = {phys_min_db:+.2} dB @ {phys_min_hz:.1} Hz (paths={})", plan_sm7.paths.len());
    let mut med_cfg = cfg.clone();
    med_cfg.rig.room_preset = spectracraft_mono::studio_catalog::RoomPreset::Medium;
    med_cfg.rig.dimensions_m = spectracraft_mono::studio_catalog::RoomPreset::Medium.dimensions_m();
    med_cfg.rig.surface_faces =
        spectracraft_mono::studio_catalog::RoomPreset::Medium.default_surface_faces();
    let out_med =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &med_cfg, &sources, &mics, &surfaces);
    let (med_min, med_min_hz, med_jump, med_jump_hz) = scan_sweep(&out_med.samples);

    let mut mic_room_cfg = cfg.clone();
    mic_room_cfg.plugin_slot.enabled = false;
    let out_mic_room =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &mic_room_cfg, &sources, &mics, &surfaces);
    let (mr_min, mr_min_hz, mr_jump, mr_jump_hz) = scan_sweep(&out_mic_room.samples);
    let (body_min, body_min_hz, body_jump, body_jump_hz) = scan_sweep(&body_f32);
    let (mic_min, mic_min_hz, mic_jump, mic_jump_hz) = scan_sweep(&out_mic.samples);
    let (room_min, room_min_hz, room_jump, room_jump_hz) = scan_sweep(&out_room.samples);
    let (min_db, min_hz, max_jump_db, max_jump_hz) = scan_sweep(&out_full.samples);
    println!(
        "[Test 3] Per-stage scan (35Hz-12kHz):\n  body : min={body_min:.2} dBFS @ {body_min_hz:.1} Hz, max_step={body_jump:.2} dB @ {body_jump_hz:.1} Hz\n  mic  : min={mic_min:.2} dBFS @ {mic_min_hz:.1} Hz, max_step={mic_jump:.2} dB @ {mic_jump_hz:.1} Hz\n  room : min={room_min:.2} dBFS @ {room_min_hz:.1} Hz, max_step={room_jump:.2} dB @ {room_jump_hz:.1} Hz\n  early: min={early_min:.2} dBFS @ {early_min_hz:.1} Hz, max_step={early_jump:.2} dB @ {early_jump_hz:.1} Hz\n  m+rm : min={mr_min:.2} dBFS @ {mr_min_hz:.1} Hz, max_step={mr_jump:.2} dB @ {mr_jump_hz:.1} Hz\n  med  : min={med_min:.2} dBFS @ {med_min_hz:.1} Hz, max_step={med_jump:.2} dB @ {med_jump_hz:.1} Hz\n  full : min={min_db:.2} dBFS @ {min_hz:.1} Hz, max_step={max_jump_db:.2} dB @ {max_jump_hz:.1} Hz"
    );

    // Start & end click check: first 5 ms (during cosine fade-in) and last 2 ms (end of cosine fade-out)
    let start_click = out_full.samples[..(0.005 * fs) as usize]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0_f32, f32::max);
    let end_click = out_full.samples[out_full.samples.len() - (0.002 * fs) as usize..]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0_f32, f32::max);
    let last_sample = out_full.samples.last().copied().unwrap_or(0.0).abs();

    // Check room does not ring at a single narrowband frequency
    let (room_prom_db, room_prom_hz) =
        max_narrowband_prominence_db(&out_room.samples, RATE, 80.0, 14_000.0);

    println!(
        "[Test 3] Body peaks: 100Hz={peak_100_db:+.2} dB, 200Hz={peak_200_db:+.2} dB (ref 1kHz={body_1000:.2} dBFS)"
    );
    println!(
        "[Test 3] Sweep edges/room: start_click={start_click:.5}, end_click={end_click:.5}, last_sample={last_sample:.5}, room_prom={room_prom_db:.2} dB @ {room_prom_hz:.1} Hz"
    );

    check_parseval_slices(&out_full.samples, "test3_sweep_full");
    check_parseval_slices(&out_med.samples, "test3_sweep_medium");
    // Under canonical Timing::Physical (H15), Stage 4 room outputs at 2.0 ft carry exact
    // 1/r direct-path attenuation relative to DIRECT_REF_DISTANCE_M (0.40 m -> -3.66 dB at 2.0 ft).
    let dist_atten_db = 20.0 * (DIRECT_REF_DISTANCE_M / (2.0 * f64::from(FEET_TO_METRES))).log10();
    let out_room_aligned = pipeline::run_post_dry_pipeline_fixed_block(
        &body_f32,
        RATE,
        &room_only_cfg,
        &sources,
        &mics,
        &surfaces,
    );
    let (room_aligned_min, _, room_aligned_jump, _) = scan_sweep(&out_room_aligned.samples);

    assert!(
        body_min > -24.0 && body_jump < 2.5,
        "Body sweep discontinuity: min={body_min:.2}, step={body_jump:.2}"
    );
    assert!(
        mic_min > -24.0 && mic_jump < 2.5,
        "Mic sweep discontinuity: min={mic_min:.2}, step={mic_jump:.2}"
    );
    assert!(
        room_min > -28.0 + dist_atten_db && room_jump < 3.5,
        "Omni room sweep discontinuity (Timing::Physical): min={room_min:.2} (floor {:.2}), step={room_jump:.2}",
        -28.0 + dist_atten_db
    );
    assert!(
        room_aligned_min > -28.0 && room_aligned_jump < 3.5,
        "Omni room sweep discontinuity (Timing::AlignDirect): min={room_aligned_min:.2}, step={room_aligned_jump:.2}"
    );
    assert!(
        med_min > -31.0 + dist_atten_db && med_jump < 4.5,
        "Medium room full-chain sweep dropout/jump: min={med_min:.2} dBFS @ {med_min_hz:.1} Hz (floor {:.2}), step={med_jump:.2} dB @ {med_jump_hz:.1} Hz",
        -31.0 + dist_atten_db
    );
    // 10x10x10 cubic foam room has a physical ~-24 dB low-frequency SBIR comb null (pure physics = -23.9 dB @ 131 Hz,
    // shifted to 105.7 Hz by LR4 reflection phase); verify it stays finite with no digital dropout.
    assert!(
        min_db > -45.0,
        "Digital dropout in cubic foam room sweep: min_rms={min_db:.2} dBFS @ {min_hz:.1} Hz"
    );
    assert!(start_click < 0.02, "Start click detected: {start_click}");
    assert!(end_click < 0.02, "End click detected: {end_click}");
    assert!(
        (0.5..=4.0).contains(&peak_100_db),
        "100 Hz body peak should be gentle (0.5..4.0 dB): {peak_100_db:+.2} dB"
    );
    assert!(
        (0.3..=3.5).contains(&peak_200_db),
        "200 Hz body peak should be gentle (0.3..3.5 dB): {peak_200_db:+.2} dB"
    );
    assert!(
        room_prom_db < 10.0,
        "Room rang at a single narrowband frequency: {room_prom_db:.2} dB @ {room_prom_hz:.1} Hz"
    );
}

fn fft_convolve_f64(a: &[f64], b: &[f64]) -> Vec<f64> {
    let out_len = a.len() + b.len() - 1;
    let nfft = out_len.next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(nfft);
    let inv = planner.plan_fft_inverse(nfft);
    let mut fa = vec![Complex::new(0.0_f64, 0.0_f64); nfft];
    let mut fb = vec![Complex::new(0.0_f64, 0.0_f64); nfft];
    for (i, &v) in a.iter().enumerate() {
        fa[i].re = v;
    }
    for (i, &v) in b.iter().enumerate() {
        fb[i].re = v;
    }
    fwd.process(&mut fa);
    fwd.process(&mut fb);
    for i in 0..nfft {
        fa[i] *= fb[i];
    }
    inv.process(&mut fa);
    let scale = 1.0 / nfft as f64;
    fa[..out_len].iter().map(|c| c.re * scale).collect()
}

#[test]
fn test_04_inverse_sweep_impulse_response() {
    let dur_s = 6.0_f64;
    let pad_s = 2.0_f64;
    let f0 = 20.0_f64;
    let f1 = 20_000.0_f64;
    let fs = f64::from(RATE);
    let ln_ratio = (f1 / f0).ln();

    let sweep = generate_log_sweep(RATE, dur_s, f0, f1, 0.20, 0.015);
    let n_sweep = sweep.len();
    let n_pad = (pad_s * fs).round() as usize;
    let mut padded = Vec::with_capacity(n_sweep + n_pad);
    padded.extend_from_slice(&sweep);
    padded.resize(n_sweep + n_pad, 0.0_f32);

    // Matched Farina inverse filter: time-reversed sweep weighted by exp(-i * ln(f1/f0) / (N-1))
    let inv_filter: Vec<f64> = (0..n_sweep)
        .map(|i| {
            let weight = (-(i as f64 / (n_sweep - 1) as f64) * ln_ratio).exp();
            f64::from(sweep[n_sweep - 1 - i]) * weight
        })
        .collect();

    // 1) Self-check deconvolution identity on raw sweep
    let raw_f64: Vec<f64> = sweep.iter().map(|&s| f64::from(s)).collect();
    let ir_id = fft_convolve_f64(&raw_f64, &inv_filter);
    let (id_peak_idx, id_peak_val) =
        ir_id
            .iter()
            .enumerate()
            .fold((0usize, 0.0_f64), |(bi, bv), (i, &v)| {
                if v.abs() > bv {
                    (i, v.abs())
                } else {
                    (bi, bv)
                }
            });
    assert!(id_peak_val > 1e-6, "Identity deconvolution peak vanished");
    assert!(
        id_peak_idx.abs_diff(n_sweep - 1) <= 2,
        "Identity IR peak misaligned: idx={id_peak_idx} vs expected {}",
        n_sweep - 1
    );
    let id_pre_max = ir_id
        [id_peak_idx - (0.500 * fs) as usize..id_peak_idx - (0.010 * fs) as usize]
        .iter()
        .fold(0.0_f64, |m, &v| m.max(v.abs()));
    let id_pre_db = 20.0 * (id_pre_max / id_peak_val).max(1e-12).log10();

    // 2) Full linear pipeline: Stage 1 Body -> Stage 3 SM7B -> Stage 4 Foam Room -> Stage 5 Limiter (below ceiling)
    let mut body_buf: Vec<f64> = padded.iter().map(|&s| f64::from(s)).collect();
    let mut body = BodyResonator::new(fs, BodyResonanceConfig::default());
    body.process_buffer(&mut body_buf);
    let body_f32: Vec<f32> = body_buf.iter().map(|&v| v as f32).collect();

    let sources = source_catalog();
    let mics = mic_catalog();
    let surfaces = surface_catalog();
    let cfg = full_post_dry_cfg();
    let out_full =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &cfg, &sources, &mics, &surfaces);

    let body_ir = fft_convolve_f64(&body_buf, &inv_filter);
    let full_f64: Vec<f64> = out_full.samples.iter().map(|&s| f64::from(s)).collect();
    let full_ir = fft_convolve_f64(&full_f64, &inv_filter);

    let analyze_ir = |ir: &[f64], label: &str| {
        let search_lo = (n_sweep - 1).saturating_sub((0.020 * fs) as usize);
        let search_hi = (n_sweep - 1 + (0.020 * fs) as usize).min(ir.len());
        let (rel_i, peak) = ir[search_lo..search_hi].iter().enumerate().fold(
            (0usize, 0.0_f64),
            |(bi, bv), (i, &v)| {
                if v.abs() > bv {
                    (i, v.abs())
                } else {
                    (bi, bv)
                }
            },
        );
        let p_idx = search_lo + rel_i;
        let offset_ms = (p_idx as isize - (n_sweep - 1) as isize) as f64 * 1000.0 / fs;

        let win_rms_rel_db = |start_ms: f64, end_ms: f64| -> f64 {
            let a = (p_idx as isize + (start_ms * 0.001 * fs).round() as isize).max(0) as usize;
            let b = ((p_idx as isize + (end_ms * 0.001 * fs).round() as isize).max(0) as usize)
                .min(ir.len());
            if b <= a {
                return -120.0;
            }
            let ms = ir[a..b].iter().map(|&v| v * v).sum::<f64>() / (b - a) as f64;
            10.0 * (ms / (peak * peak).max(1e-24)).max(1e-18).log10()
        };
        let pre_peak_db = 20.0
            * (ir[p_idx - (0.500 * fs) as usize..p_idx - (0.015 * fs) as usize]
                .iter()
                .fold(0.0_f64, |m, &v| m.max(v.abs()))
                / peak.max(1e-12))
            .max(1e-12)
            .log10();
        let tail_50_150 = win_rms_rel_db(50.0, 150.0);
        let tail_250_500 = win_rms_rel_db(250.0, 500.0);
        let tail_1000_1500 = win_rms_rel_db(1000.0, 1500.0);
        let norm_peak_db = 20.0 * (peak / id_peak_val).max(1e-12).log10();
        let tail_f32: Vec<f32> = ir
            [p_idx + (0.030 * fs) as usize..(p_idx + (1.030 * fs) as usize).min(ir.len())]
            .iter()
            .map(|&v| (v / peak.max(1e-12)) as f32)
            .collect();
        let (tail_prom_db, tail_prom_hz) =
            max_narrowband_prominence_db(&tail_f32, RATE, 60.0, 12_000.0);
        println!(
            "[Test 4] IR({label}): gain={norm_peak_db:+.2} dB, offset={offset_ms:+.2} ms, pre(-500..-15ms)={pre_peak_db:.1} dB, tail[50..150ms]={tail_50_150:.1} dB, tail[250..500ms]={tail_250_500:.1} dB, tail[1000..1500ms]={tail_1000_1500:.1} dB, tail_prom={tail_prom_db:.2} dB @ {tail_prom_hz:.1} Hz"
        );
        (
            offset_ms,
            pre_peak_db,
            tail_50_150,
            tail_250_500,
            tail_1000_1500,
            tail_prom_db,
        )
    };

    println!("[Test 4] Identity sweep deconvolution: peak_idx={id_peak_idx}, pre(-500..-10ms)={id_pre_db:.1} dB");
    let (body_off, body_pre, _b50, b250, b1000, _bprom) = analyze_ir(&body_ir, "body");
    let (full_off, full_pre, f50, f250, f1000, fprom) = analyze_ir(&full_ir, "full_pipeline");

    assert!(
        id_pre_db < -40.0,
        "Identity deconvolution sidelobes too high: {id_pre_db:.1} dB"
    );
    assert!(
        body_off.abs() < 3.0,
        "Body IR peak shifted excessively: {body_off:+.2} ms"
    );
    assert!(
        full_off.abs() < 5.0,
        "Full IR peak shifted excessively: {full_off:+.2} ms"
    );
    assert!(
        body_pre < -35.0,
        "Body IR pre-ringing too high: {body_pre:.1} dB"
    );
    assert!(
        full_pre < -35.0,
        "Full IR pre-ringing too high: {full_pre:.1} dB"
    );
    assert!(
        b250 < -50.0 && b1000 < -65.0,
        "Body IR failed to decay cleanly: 250ms={b250:.1}, 1000ms={b1000:.1}"
    );
    assert!(f50 < -25.0, "Full IR early tail too hot: {f50:.1} dB");
    assert!(
        f250 < f50,
        "Full IR tail not decaying monotonically: 50ms={f50:.1}, 250ms={f250:.1}"
    );
    assert!(
        f1000 < -55.0,
        "Full IR tail ringing too long at 1.0-1.5s: {f1000:.1} dB"
    );
    assert!(
        fprom < 14.0,
        "Full IR tail has excessive single-frequency whistle: {fprom:.2} dB"
    );
}

/// Measure fundamental power vs out-of-band / harmonic distortion (THD+N ratio in dB)
/// on a steady-state slice of a pure sine tone at `f_hz`.
fn tone_thd_and_spurious_db(samples: &[f32], rate: u32, f_hz: f64) -> (f64, f64) {
    let nfft = 16_384_usize;
    let mid = samples.len() / 2;
    let start = mid.saturating_sub(nfft / 2);
    let slice = &samples[start..start + nfft];
    let fs = f64::from(rate);
    let df = fs / nfft as f64;
    let win: Vec<f64> = (0..nfft)
        .map(|i| {
            // Blackman-Harris 4-term window for >90 dB sidelobe suppression
            let x = TAU * i as f64 / nfft as f64;
            0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos()
        })
        .collect();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(nfft);
    let mut buf: Vec<Complex<f64>> = slice
        .iter()
        .enumerate()
        .map(|(i, &s)| Complex::new(f64::from(s) * win[i], 0.0))
        .collect();
    fft.process(&mut buf);

    let k0 = (f_hz / df).round() as usize;
    let guard = 6usize;
    let mut fund_e = 0.0_f64;
    let mut non_fund_e = 0.0_f64;
    let mut worst_spur = 0.0_f64;
    for k in 1..=(nfft / 2) {
        let p = buf[k].norm_sqr();
        if k.abs_diff(k0) <= guard {
            fund_e += p;
        } else {
            non_fund_e += p;
            if p > worst_spur {
                worst_spur = p;
            }
        }
    }
    let thd_n_db = 10.0 * (non_fund_e / fund_e.max(1e-24)).max(1e-18).log10();
    let worst_spur_db = 10.0 * (worst_spur / fund_e.max(1e-24)).max(1e-18).log10();
    (thd_n_db, worst_spur_db)
}

#[test]
fn test_05_pure_tones_critical_frequencies() {
    let freqs = [
        50.0_f64, 60.0, 100.0, 200.0, 440.0, 1000.0, 5000.0, 10_000.0, 15_000.0,
    ];
    let dur_s = 2.0_f64;
    let fs = f64::from(RATE);
    let n = (dur_s * fs).round() as usize;
    let fade_n = (0.015 * fs).round() as usize;
    let amp = 10.0_f64.powf(-12.0 / 20.0) * std::f64::consts::SQRT_2; // -12 dBFS RMS sine

    let sources = source_catalog();
    let mics = mic_catalog();
    let surfaces = surface_catalog();
    let cfg = full_post_dry_cfg();

    for &f_hz in &freqs {
        let tone: Vec<f32> = (0..n)
            .map(|i| {
                let mut w = 1.0_f64;
                if i < fade_n {
                    w = 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / fade_n as f64).cos());
                } else if i + fade_n >= n {
                    let rem = n - 1 - i;
                    w = 0.5 * (1.0 - (std::f64::consts::PI * rem as f64 / fade_n as f64).cos());
                }
                (amp * w * (TAU * f_hz * i as f64 / fs).sin()) as f32
            })
            .collect();

        // 1) Bedroom Translator hum-removal & conditioning check
        let (cond, prof) = translation_layer::condition_bedroom_for_analysis(&tone, RATE);
        let in_mid_rms = rms_db(&tone[n / 4..3 * n / 4]);
        let cond_mid_rms = rms_db(&cond[n / 4..3 * n / 4]);
        let cond_loss_db = cond_mid_rms - in_mid_rms;

        // 2) Stage 1 BodyResonator linearity check
        let mut body_buf: Vec<f64> = tone.iter().map(|&s| f64::from(s)).collect();
        let mut body = BodyResonator::new(fs, BodyResonanceConfig::default());
        body.process_buffer(&mut body_buf);
        let body_f32: Vec<f32> = body_buf.iter().map(|&v| v as f32).collect();
        let (body_thdn, body_spur) = tone_thd_and_spurious_db(&body_f32, RATE, f_hz);

        // 3) Full Stages 3-5 check (THD/aliasing)
        let out_full =
            pipeline::run_post_dry_pipeline(&body_f32, RATE, &cfg, &sources, &mics, &surfaces);
        let (full_thdn, full_spur) = tone_thd_and_spurious_db(&out_full.samples, RATE, f_hz);

        println!(
            "[Test 5] {f_hz:7.1} Hz | Translator: hum={:?}, cond_delta={cond_loss_db:+.2} dB | Body: thd+n={body_thdn:.1} dB, spur={body_spur:.1} dB | Full: thd+n={full_thdn:.1} dB, spur={full_spur:.1} dB",
            prof.mains_hum_hz
        );

        assert!(
            cond_loss_db > -6.0,
            "Translator notched musical tone at {f_hz} Hz: delta={cond_loss_db:+.2} dB (hum={:?})",
            prof.mains_hum_hz
        );
        assert!(
            body_thdn < -75.0 && body_spur < -80.0,
            "Body filter distorted {f_hz} Hz tone: thd+n={body_thdn:.1} dB, spur={body_spur:.1} dB"
        );
        assert!(
            full_thdn < -55.0 && full_spur < -60.0,
            "Full pipeline introduced aliasing/distortion on {f_hz} Hz tone: thd+n={full_thdn:.1} dB, spur={full_spur:.1} dB"
        );
        check_parseval_slices(&out_full.samples, &format!("test5_{f_hz:.0}hz"));
    }

    // Also test Stage 1 StringState bandlimiting at high pitch (no aliasing foldback above Nyquist)
    let mut s1 = spectracraft_mono::string_state::StringState::new();
    let weights: [f64; spectracraft_mono::string_state::MODE_COUNT] =
        std::array::from_fn(|i| 1.0 / ((i + 1) as f64).powf(0.91));
    let decays: [f64; spectracraft_mono::string_state::MODE_COUNT] =
        std::array::from_fn(|i| 1.2 + 0.25 * (i + 1) as f64);
    let losses = spectracraft_mono::string_state::per_sample_losses(&decays, fs);
    let pickup = [1.0_f64; spectracraft_mono::string_state::MODE_COUNT];
    s1.repick(0.5, &weights);
    let mut high_tone = Vec::with_capacity(RATE as usize);
    let f_high = 4_000.0_f64; // k >= 6 would exceed 24 kHz Nyquist if not bandlimited!
    for _ in 0..RATE as usize {
        high_tone.push(s1.tick_cached(f_high, fs, &losses, &pickup) as f32);
    }
    // Check spectrum of high_tone below f_high (should contain ZERO aliased partials below 3800 Hz!)
    let nfft = 16_384_usize;
    let slice = &high_tone[4096..4096 + nfft];
    let fft = FftPlanner::<f64>::new().plan_fft_forward(nfft);
    let mut buf: Vec<Complex<f64>> = slice
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w = 0.5 * (1.0 - (TAU * i as f64 / nfft as f64).cos());
            Complex::new(f64::from(s) * w, 0.0)
        })
        .collect();
    fft.process(&mut buf);
    let df = fs / nfft as f64;
    let k_fund = (f_high / df).round() as usize;
    let fund_p = buf[k_fund.saturating_sub(4)..=k_fund + 4]
        .iter()
        .map(|c| c.norm_sqr())
        .fold(0.0_f64, f64::max);
    let k_sub_max = (3500.0 / df).floor() as usize;
    let mut max_alias_p = 0.0_f64;
    let mut max_alias_hz = 0.0_f64;
    for k in 4..=k_sub_max {
        let p = buf[k].norm_sqr();
        if p > max_alias_p {
            max_alias_p = p;
            max_alias_hz = k as f64 * df;
        }
    }
    let alias_db = 10.0 * (max_alias_p / fund_p.max(1e-24)).max(1e-18).log10();
    println!("[Test 5] Stage 1 StringState @ 4000 Hz sub-fundamental alias peak = {alias_db:.1} dB @ {max_alias_hz:.1} Hz");
    assert!(
        alias_db < -60.0,
        "Stage 1 StringState aliased above-Nyquist modes down to {max_alias_hz:.1} Hz ({alias_db:.1} dB)"
    );
}

#[test]
fn test_06_click_impulse_train() {
    let fs = f64::from(RATE);
    let n = RATE as usize * 3; // 3.0 s
    let impulse_indices: Vec<usize> = (1..=5).map(|k| (k * RATE as usize) / 2).collect(); // 0.5, 1.0, 1.5, 2.0, 2.5 s
    let mut train = vec![0.0_f32; n];
    for &idx in &impulse_indices {
        train[idx] = 1.0_f32; // single-sample 0 dBFS impulse
    }

    // 1) Bedroom Translator check
    let (cond, prof) = translation_layer::condition_bedroom_for_analysis(&train, RATE);
    assert!(
        prof.mains_hum_hz.is_none(),
        "Impulse train should not falsely trigger mains hum: {:?}",
        prof.mains_hum_hz
    );
    assert!(
        cond.iter().all(|v| v.is_finite()),
        "Translator produced non-finite samples"
    );

    // 2) Stage 1 BodyResonator
    let mut body_buf: Vec<f64> = train.iter().map(|&s| f64::from(s)).collect();
    let mut body = BodyResonator::new(fs, BodyResonanceConfig::default());
    body.process_buffer(&mut body_buf);
    let body_f32: Vec<f32> = body_buf.iter().map(|&v| v as f32).collect();

    let sources = source_catalog();
    let mics = mic_catalog();
    let surfaces = surface_catalog();
    let cfg = full_post_dry_cfg();
    let mut linear_cfg = cfg.clone();
    linear_cfg.plugin_slot.enabled = false;

    let out_lin =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &linear_cfg, &sources, &mics, &surfaces);
    let out_full =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &cfg, &sources, &mics, &surfaces);

    let first_imp = impulse_indices[0];
    let body_pre = peak_abs(&body_f32[..first_imp]);
    let lin_pre = peak_abs(&out_lin.samples[..first_imp]);
    let full_pre = peak_abs(&out_full.samples[..first_imp]);

    // Check decay envelope after the 5th impulse (t = 2.5 s .. 3.0 s) and between impulses 2 and 3
    let imp_idx = impulse_indices[4];
    let pk_lin = peak_abs(&out_lin.samples[imp_idx..imp_idx + 128]);
    let pk_full = peak_abs(&out_full.samples[imp_idx..imp_idx + 128]);
    let win_rel = |sig: &[f32], pk: f32, ms_a: f64, ms_b: f64| -> f64 {
        let a = imp_idx + (ms_a * 0.001 * fs).round() as usize;
        let b = (imp_idx + (ms_b * 0.001 * fs).round() as usize).min(sig.len());
        rms_db(&sig[a..b]) - 20.0 * f64::from(pk.max(1e-9)).log10()
    };
    let lin_10_50 = win_rel(&out_lin.samples, pk_lin, 10.0, 50.0);
    let lin_100_200 = win_rel(&out_lin.samples, pk_lin, 100.0, 200.0);
    let lin_300_480 = win_rel(&out_lin.samples, pk_lin, 300.0, 480.0);
    let full_10_50 = win_rel(&out_full.samples, pk_full, 10.0, 50.0);
    let full_100_200 = win_rel(&out_full.samples, pk_full, 100.0, 200.0);
    let full_300_480 = win_rel(&out_full.samples, pk_full, 300.0, 480.0);

    // Also check pre-arrival energy right before the 2nd impulse (450..495 ms after 1st impulse)
    let before_2nd =
        peak_abs(&out_lin.samples[impulse_indices[1] - (0.010 * fs) as usize..impulse_indices[1]]);
    let before_2nd_db = 20.0 * f64::from((before_2nd / pk_lin.max(1e-9)).max(1e-12)).log10();

    println!(
        "[Test 6] Pre-echo before 1st impulse: body={body_pre:.3e}, linear={lin_pre:.3e}, full={full_pre:.3e}"
    );
    println!(
        "[Test 6] Linear decay (pk={pk_lin:.4}): [10..50ms]={lin_10_50:.1} dB, [100..200ms]={lin_100_200:.1} dB, [300..480ms]={lin_300_480:.1} dB, pre-2nd(-10..0ms)={before_2nd_db:.1} dB"
    );
    let loud_full = acoustics_physics::measure_loudness_bs1770(&[&out_full.samples], RATE);
    println!(
        "[Test 6] Full+Lim decay (pk={pk_full:.4}, tp={:.2} dBTP): [10..50ms]={full_10_50:.1} dB, [100..200ms]={full_100_200:.1} dB, [300..480ms]={full_300_480:.1} dB",
        loud_full.true_peak_dbtp
    );

    check_parseval_slices(&out_full.samples, "test6_impulse_full");
    assert_eq!(
        body_pre, 0.0,
        "Body filter leaked pre-echo before first impulse: {body_pre}"
    );
    assert_eq!(
        lin_pre, 0.0,
        "Stage 3/4 leaked pre-echo before first impulse: {lin_pre}"
    );
    assert_eq!(
        full_pre, 0.0,
        "Full pipeline leaked pre-echo before first impulse: {full_pre}"
    );
    assert!(
        pk_full <= 0.892,
        "Stage 5 limiter failed to catch 0 dBFS impulse: pk={pk_full}"
    );
    assert!(
        lin_100_200 < lin_10_50 && lin_300_480 < lin_100_200,
        "Linear impulse tail did not decay monotonically: {lin_10_50:.1} -> {lin_100_200:.1} -> {lin_300_480:.1}"
    );
    assert!(
        lin_300_480 < -65.0 && full_300_480 < -65.0,
        "Extended ringing past 300-480 ms: lin={lin_300_480:.1} dB, full={full_300_480:.1} dB"
    );
    assert!(
        before_2nd_db < -60.0,
        "Residual ringing did not settle before next impulse: {before_2nd_db:.1} dB"
    );
}

#[test]
fn test_07_digital_silence() {
    let fs = f64::from(RATE);
    let n = RATE as usize * 3; // 3.0 s exact digital silence
    let silence = vec![0.0_f32; n];

    // 1) Bedroom Translator on exact silence
    let (cond, prof) = translation_layer::condition_bedroom_for_analysis(&silence, RATE);
    let translated = translation_layer::analyze_from_bedroom_recording(&silence, RATE)
        .expect("Translator should handle exact silence gracefully");
    assert_eq!(
        peak_abs(&cond),
        0.0,
        "Translator injected noise into silence"
    );
    assert_eq!(dc_offset(&cond), 0.0, "Translator injected DC into silence");
    assert!(
        prof.mains_hum_hz.is_none(),
        "False hum on silence: {:?}",
        prof.mains_hum_hz
    );
    assert_eq!(
        translated.performance.notes().len(),
        0,
        "False notes detected in silence"
    );

    // 2) Stage 1 un-picked StringState + BodyResonator
    let mut s1 = spectracraft_mono::string_state::StringState::new();
    let decays = [1.5_f64; spectracraft_mono::string_state::MODE_COUNT];
    let losses = spectracraft_mono::string_state::per_sample_losses(&decays, fs);
    let pickup = [1.0_f64; spectracraft_mono::string_state::MODE_COUNT];
    let mut s1_peak = 0.0_f64;
    for _ in 0..4096 {
        s1_peak = s1_peak.max(s1.tick_cached(220.0, fs, &losses, &pickup).abs());
    }
    assert_eq!(
        s1_peak, 0.0,
        "Un-picked StringState emitted non-zero sample"
    );

    let mut body_buf = vec![0.0_f64; n];
    let mut body = BodyResonator::new(fs, BodyResonanceConfig::default());
    body.process_buffer(&mut body_buf);
    let body_f32: Vec<f32> = body_buf.iter().map(|&v| v as f32).collect();
    assert_eq!(
        peak_abs(&body_f32),
        0.0,
        "BodyResonator injected noise into silence"
    );

    // 3) Full Stages 3-5 on exact silence (both preamp=0 and preamp=0.5)
    let sources = source_catalog();
    let mics = mic_catalog();
    let surfaces = surface_catalog();
    let cfg = full_post_dry_cfg();
    let t0 = std::time::Instant::now();
    let out_sil =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &cfg, &sources, &mics, &surfaces);
    let dt_sil = t0.elapsed();

    let mut preamp_cfg = cfg.clone();
    preamp_cfg.preamp_drive = 0.5;
    let out_preamp =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &preamp_cfg, &sources, &mics, &surfaces);

    assert_eq!(
        peak_abs(&out_sil.samples),
        0.0,
        "Full pipeline injected noise into silence"
    );
    assert_eq!(
        dc_offset(&out_sil.samples),
        0.0,
        "Full pipeline injected DC into silence"
    );
    assert_eq!(
        peak_abs(&out_preamp.samples),
        0.0,
        "Preamp injected noise/DC into silence"
    );

    // 4) Denormal / subnormal slowdown check: compare active noise vs impulse-into-3s-silence tail
    let mut rng = 0xfeed_face_cafe_beef_u64;
    let active: Vec<f32> = (0..n)
        .map(|_| (xorshift64(&mut rng) * 0.25) as f32)
        .collect();
    let t_act0 = std::time::Instant::now();
    let _out_act = pipeline::run_post_dry_pipeline(&active, RATE, &cfg, &sources, &mics, &surfaces);
    let dt_act = t_act0.elapsed();

    // Seed a tiny impulse followed by 3 s of zeros to drive IIR/FDN states toward IEEE-754 subnormals
    let mut sub_in = vec![0.0_f32; n];
    sub_in[0] = 1e-25_f32;
    let t_sub0 = std::time::Instant::now();
    let mut sub_body = BodyResonator::new(fs, BodyResonanceConfig::default());
    let mut sub_b: Vec<f64> = sub_in.iter().map(|&s| f64::from(s)).collect();
    sub_body.process_buffer(&mut sub_b);
    let sub_f32: Vec<f32> = sub_b.iter().map(|&v| v as f32).collect();
    let out_sub = pipeline::run_post_dry_pipeline(&sub_f32, RATE, &cfg, &sources, &mics, &surfaces);
    let dt_sub = t_sub0.elapsed();

    let ratio_sil = dt_sil.as_secs_f64() / dt_act.as_secs_f64().max(1e-6);
    let ratio_sub = dt_sub.as_secs_f64() / dt_act.as_secs_f64().max(1e-6);
    println!(
        "[Test 7] Silence: peak={:.3e}, dc={:.3e}, preamp_peak={:.3e} | Timing (3s): active={:.1} ms, silence={:.1} ms ({ratio_sil:.2}x), subnormal_tail={:.1} ms ({ratio_sub:.2}x)",
        peak_abs(&out_sil.samples),
        dc_offset(&out_sil.samples),
        peak_abs(&out_preamp.samples),
        dt_act.as_secs_f64() * 1000.0,
        dt_sil.as_secs_f64() * 1000.0,
        dt_sub.as_secs_f64() * 1000.0
    );

    assert!(
        out_sub.samples.iter().all(|v| v.is_finite()),
        "Subnormal tail produced NaN/Inf"
    );
    assert!(
        ratio_sil < 3.0,
        "Silence ran unexpectedly slowly vs active audio ({ratio_sil:.2}x)"
    );
    assert!(
        ratio_sub < 3.5,
        "Denormal/subnormal slowdown detected on decay tail ({ratio_sub:.2}x vs active)"
    );
}

fn generate_pink_noise_with_isp_peaks(rate: u32, duration_s: f64, peak_dbfs: f64) -> Vec<f32> {
    let n = (f64::from(rate) * duration_s).round() as usize;
    let mut rng = 0x55aa_1357_9bdf_2468_u64;
    // Paul Kellet's classic 1/f pinking filter
    let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) =
        (0.0_f64, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let w = xorshift64(&mut rng);
        b0 = 0.99886 * b0 + w * 0.0555179;
        b1 = 0.99332 * b1 + w * 0.0750759;
        b2 = 0.96900 * b2 + w * 0.1538520;
        b3 = 0.86650 * b3 + w * 0.3104856;
        b4 = 0.55000 * b4 + w * 0.5329522;
        b5 = -0.7616 * b5 - w * 0.0168980;
        let pink = b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362;
        b6 = w * 0.115926;
        out.push(pink);
    }
    // Remove mean DC and scale pink bed so RMS is ~-18 dBFS before adding peaks
    let mean = out.iter().sum::<f64>() / out.len() as f64;
    for v in &mut out {
        *v -= mean;
    }
    let rms = (out.iter().map(|v| v * v).sum::<f64>() / out.len() as f64)
        .sqrt()
        .max(1e-12);
    let bed_gain = 10.0_f64.powf(-16.0 / 20.0) / rms;
    for v in &mut out {
        *v *= bed_gain;
    }
    // Inject periodic short high-frequency transient bursts (fs/4 = 12 kHz, 45° phase -> +3 dB ISP)
    let burst_step = (f64::from(rate) * 0.45) as usize;
    let mut pos = (f64::from(rate) * 0.25) as usize;
    while pos + 64 < n {
        for k in 0..48 {
            let env = (std::f64::consts::PI * k as f64 / 48.0).sin().powi(2);
            // 12 kHz sine at 45° offset: sample values [+, +, -, -] create +3 dB inter-sample peaks
            let carrier = (TAU * 0.25 * k as f64 + std::f64::consts::FRAC_PI_4).sin();
            out[pos + k] += 0.95 * env * carrier;
        }
        pos += burst_step;
    }
    let pk = out.iter().fold(0.0_f64, |m, &v| m.max(v.abs())).max(1e-12);
    let target_pk = 10.0_f64.powf(peak_dbfs / 20.0);
    let s = target_pk / pk;
    out.into_iter().map(|v| (v * s) as f32).collect()
}

fn max_consecutive_at_ceiling(samples: &[f32], ceiling: f32) -> usize {
    let thresh = ceiling - 1e-4;
    let mut best = 0usize;
    let mut cur = 0usize;
    let mut prev_sign = 0_i8;
    for &s in samples {
        let sign = if s >= thresh {
            1
        } else if s <= -thresh {
            -1
        } else {
            0
        };
        if sign != 0 && sign == prev_sign {
            cur += 1;
            best = best.max(cur);
        } else if sign != 0 {
            cur = 1;
            best = best.max(1);
        } else {
            cur = 0;
        }
        prev_sign = sign;
    }
    best
}

#[test]
fn test_08_near_full_scale_pink_noise_limiter_lufs() {
    let fs = f64::from(RATE);
    let pink = generate_pink_noise_with_isp_peaks(RATE, 5.0, -1.0);
    let in_loud = acoustics_physics::measure_loudness_bs1770(&[&pink], RATE);
    let in_pk = peak_abs(&pink);

    // 1) Bedroom Translator check
    let (cond, prof) = translation_layer::condition_bedroom_for_analysis(&pink, RATE);
    assert!(
        prof.mains_hum_hz.is_none(),
        "Pink noise falsely triggered hum: {:?}",
        prof.mains_hum_hz
    );
    assert!(
        cond.iter().all(|v| v.is_finite()),
        "Translator emitted non-finite on pink noise"
    );

    // 2) Full pipeline with Stage 5 PhaseLimiter (unity drive + hot drive to force true-peak limiting)
    let mut body_buf: Vec<f64> = pink.iter().map(|&s| f64::from(s)).collect();
    let mut body = BodyResonator::new(fs, BodyResonanceConfig::default());
    body.process_buffer(&mut body_buf);
    let body_f32: Vec<f32> = body_buf.iter().map(|&v| v as f32).collect();

    let sources = source_catalog();
    let mics = mic_catalog();
    let surfaces = surface_catalog();

    // (a) Standard full pipeline (unity drive, -1.0 dBFS ceiling)
    let cfg_unity = full_post_dry_cfg();
    let out_unity =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &cfg_unity, &sources, &mics, &surfaces);
    let loud_unity = acoustics_physics::measure_loudness_bs1770(&[&out_unity.samples], RATE);

    // (b) Stage 5 limiter stress: direct hot pink+ISP signal into Stage 5 PhaseLimiter at -1.0 dBFS ceiling (amount=0.85 -> +3 dB drive)
    let mut cfg_lim_stress = full_post_dry_cfg();
    cfg_lim_stress.mic_enabled = false;
    cfg_lim_stress.room_enabled = false;
    cfg_lim_stress.plugin_slot.amount = 0.85;
    cfg_lim_stress.plugin_slot.ceiling_dbfs = -1.0;
    let out_stress =
        pipeline::run_post_dry_pipeline(&pink, RATE, &cfg_lim_stress, &sources, &mics, &surfaces);
    let loud_stress = acoustics_physics::measure_loudness_bs1770(&[&out_stress.samples], RATE);

    // (c) Full pipeline with hot drive (+3 dB) through Body -> Mic -> Room -> PhaseLimiter
    let mut cfg_full_hot = full_post_dry_cfg();
    cfg_full_hot.plugin_slot.amount = 0.85;
    cfg_full_hot.plugin_slot.ceiling_dbfs = -1.0;
    let out_full_hot =
        pipeline::run_post_dry_pipeline(&body_f32, RATE, &cfg_full_hot, &sources, &mics, &surfaces);
    let loud_full_hot = acoustics_physics::measure_loudness_bs1770(&[&out_full_hot.samples], RATE);

    let ceiling_lin = 10.0_f64.powf(-1.0 / 20.0) as f32;
    let flat_stress = max_consecutive_at_ceiling(&out_stress.samples, ceiling_lin);
    let flat_full_hot = max_consecutive_at_ceiling(&out_full_hot.samples, ceiling_lin);

    // LUFS stability across first half (0..2.5s) vs second half (2.5..5.0s)
    let half = out_unity.samples.len() / 2;
    let lufs_h1 = acoustics_physics::measure_loudness_bs1770(&[&out_unity.samples[..half]], RATE)
        .integrated_lufs;
    let lufs_h2 = acoustics_physics::measure_loudness_bs1770(&[&out_unity.samples[half..]], RATE)
        .integrated_lufs;
    let lufs_drift = (lufs_h1 - lufs_h2).abs();

    let s_h1 = acoustics_physics::measure_loudness_bs1770(&[&out_stress.samples[..half]], RATE)
        .integrated_lufs;
    let s_h2 = acoustics_physics::measure_loudness_bs1770(&[&out_stress.samples[half..]], RATE)
        .integrated_lufs;
    let s_drift = (s_h1 - s_h2).abs();

    println!(
        "[Test 8] Input pink+ISP: pk={in_pk:.4} ({:.2} dBFS), tp={:.2} dBTP, lufs={:.2}",
        20.0 * f64::from(in_pk).log10(),
        in_loud.true_peak_dbtp,
        in_loud.integrated_lufs
    );
    println!(
        "[Test 8] Full (unity): pk={:.4}, tp={:.2} dBTP, lufs={:.2} (h1={lufs_h1:.2}, h2={lufs_h2:.2}, drift={lufs_drift:.2} LU)",
        peak_abs(&out_unity.samples),
        loud_unity.true_peak_dbtp,
        loud_unity.integrated_lufs
    );
    println!(
        "[Test 8] Stage 5 ISP stress (+3dB): pk={:.4}, tp={:.2} dBTP, lufs={:.2} (drift={s_drift:.2} LU), max_flat_run={flat_stress}",
        peak_abs(&out_stress.samples),
        loud_stress.true_peak_dbtp,
        loud_stress.integrated_lufs
    );
    println!(
        "[Test 8] Full+Hot (+3dB): pk={:.4}, tp={:.2} dBTP, lufs={:.2}, max_flat_run={flat_full_hot}",
        peak_abs(&out_full_hot.samples),
        loud_full_hot.true_peak_dbtp,
        loud_full_hot.integrated_lufs
    );

    check_parseval_slices(&out_unity.samples, "test8_pink_full");
    check_parseval_slices(&out_stress.samples, "test8_pink_limiter_stress");
    assert!(
        in_loud.true_peak_dbtp > -0.5,
        "Test signal must contain > -0.5 dBTP inter-sample peaks: {:.2}",
        in_loud.true_peak_dbtp
    );
    assert!(
        loud_unity.true_peak_dbtp <= -0.95,
        "Full pipeline exceeded -1 dBTP ceiling: {:.2} dBTP",
        loud_unity.true_peak_dbtp
    );
    assert!(
        loud_stress.true_peak_dbtp <= -0.95,
        "Stage 5 true-peak limiter failed to catch inter-sample peaks: {:.2} dBTP (ceiling -1.0 dBTP)",
        loud_stress.true_peak_dbtp
    );
    assert!(
        loud_full_hot.true_peak_dbtp <= -0.95,
        "Full+Hot pipeline exceeded -1 dBTP ceiling: {:.2} dBTP",
        loud_full_hot.true_peak_dbtp
    );
    assert!(
        flat_stress <= 2 && flat_full_hot <= 2,
        "Hard clipping flat-top detected: stress_run={flat_stress}, full_hot_run={flat_full_hot}"
    );
    assert!(
        loud_unity.integrated_lufs.is_finite() && lufs_drift < 1.5 && s_drift < 1.5,
        "LUFS measurement unstable: unity_drift={lufs_drift:.2} LU, stress_drift={s_drift:.2} LU"
    );
}
