use serde_json::json;
use spectral_canvas_mono::reperform::Performance;

fn cues(shape: serde_json::Value) -> Performance {
    serde_json::from_value(json!({
        "rate": 48000, "samples": 48000, "hop_s": 0.005,
        "notes": [
            {"start": 0.1, "end": 0.4, "hz": vec![220.;60], "level_steps": vec![0.12;60], "shape":shape},
            {"start": 0.55, "end": 0.85, "hz": vec![330.;60], "level_steps": vec![0.12;60]}
        ]
    })).unwrap()
}

#[test]
fn local_harmonic_shaping_preserves_rest_and_later_note() {
    let a = cues(json!({})).synthesize_dry().unwrap();
    let b = cues(json!({"mode_weights":{"4":2.},"decay_per_second":{"4":6.}}))
        .synthesize_dry()
        .unwrap();
    assert_eq!(a.len(), 48000);
    assert!(a[..4800].iter().all(|&x| x == 0.));
    assert_eq!(a[19200..], b[19200..]);
    assert_ne!(a[4800..19200], b[4800..19200]);
    assert!(a.iter().all(|x| x.is_finite() && x.abs() < 1.));

    fn amplitude(y: &[f32], k: f64, start: usize) -> f64 {
        let u = std::f64::consts::TAU;
        let (re, im) =
            y[start..start + 2400]
                .iter()
                .enumerate()
                .fold((0., 0.), |(re, im), (i, &x)| {
                    let a = u * i as f64 * 220. * k / 48000.;
                    let w = (0.5 - 0.5 * (u * i as f64 / 2399.).cos()) * x as f64;
                    (re + w * a.cos(), im + w * a.sin())
                });
        re.hypot(im)
    }
    assert!(amplitude(&b, 4., 6000) > amplitude(&a, 4., 6000) * 1.4);
    let d = amplitude(&b, 1., 6000) / amplitude(&a, 1., 6000);
    assert!((d - 1.).abs() < 0.005);
}

#[test]
fn invalid_mode_is_rejected() {
    assert!(cues(json!({"mode_weights":{"20":2.}}))
        .synthesize_dry()
        .is_err());
}

#[test]
fn source_analysis_and_render_are_native() {
    use spectral_canvas_mono::reperform::analyze_guitar;
    let rate = 48_000;
    let wave: Vec<f32> = (0..rate)
        .map(|i| {
            let t = f64::from(i) / f64::from(rate);
            if !(0.20..0.70).contains(&t) {
                return 0.;
            }
            let p = std::f64::consts::TAU * 220. * (t - 0.2);
            ((p.sin() * 0.25 + (p * 2.).sin() * 0.10) * (-2. * (t - 0.2)).exp()) as f32
        })
        .collect();
    let p = analyze_guitar(&wave, rate).unwrap();
    let cues = serde_json::to_value(&p).unwrap();
    let n = cues["notes"].as_array().unwrap();
    assert_eq!(n.len(), 1);
    let hz = n[0]["hz"].as_array().unwrap();
    assert!((hz[hz.len() / 2].as_f64().unwrap() - 220.).abs() < 2.);
    let output = p.synthesize_dry().unwrap();
    assert_eq!(output.len(), wave.len());
    assert!(output[..8000].iter().all(|&x| x == 0.));
    assert!(output.iter().any(|x| x.abs() > 0.03));
    assert!(output.iter().all(|x| x.is_finite() && x.abs() < 1.0));
    let empty = analyze_guitar(&vec![0.; 24000], rate).unwrap();
    assert!(empty.synthesize_dry().unwrap().iter().all(|&x| x == 0.));
}

#[test]
fn editable_repick_changes_only_the_expected_part_of_the_track() {
    let make = |repicks| {
        serde_json::from_value::<Performance>(json!({
        "rate": 48000, "samples": 48000, "hop_s": 0.005,
        "notes":[{"start":0.1,"end":0.8,"hz": (0..140).map(|i|220.+f64::from(i)/5.).collect::<Vec<_>>(), "level_steps":vec![0.15;140], "re_picks":repicks}]
    })).unwrap()
    };
    let a = make(json!([])).synthesize_dry().unwrap();
    let b = make(json!([{"offset_s":0.3,"strength":0.38}]))
        .synthesize_dry()
        .unwrap();
    assert_eq!(a[..=19200], b[..=19200]);
    assert!(a[19201..19400]
        .iter()
        .zip(&b[19201..19400])
        .any(|(x, y)| (x - y).abs() > 0.01));
    assert_eq!(a[38400..], b[38400..]);
    let bad = make(json!([{"offset_s":9.0,"strength":0.5}]));
    assert!(bad.synthesize_dry().is_err());
}
