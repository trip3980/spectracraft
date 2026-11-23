use spectral_canvas_mono::{
    dsp::Random,
    harmonic::{analyze, render, Profile, HOP},
};
#[test]
fn original_sample_rate_pitch_noise_and_silence_gates() {
    for rate in [16000, 22050, 44100, 48000, 96000] {
        let n = (0.65 * rate as f64).round() as usize;
        let mut rng = Random::new(7);
        let cases = [
            ("silent", vec![0.; n]),
            (
                "noise",
                (0..n).map(|_| (rng.normal() * 0.035) as f32).collect(),
            ),
            (
                "low-note",
                (0..n)
                    .map(|i| {
                        (0.25 * (std::f64::consts::TAU * 131. * i as f64 / rate as f64).sin())
                            as f32
                    })
                    .collect(),
            ),
            (
                "high-note",
                (0..n)
                    .map(|i| {
                        (0.25 * (std::f64::consts::TAU * 2250. * i as f64 / rate as f64).sin())
                            as f32
                    })
                    .collect(),
            ),
        ];
        for (label, x) in cases {
            let packet = analyze(&x, rate, Profile::General).unwrap();
            let y = render(&packet, 734, true, false).unwrap().mix();
            assert_eq!(y.len(), x.len());
            assert!(y.iter().all(|v| v.is_finite()));
            let central: Vec<_> = (0..packet.pitch.len())
                .filter(|&i| {
                    let t = (i * HOP) as f64 / rate as f64;
                    t > 0.1 && t < 0.5
                })
                .collect();
            let voiced =
                central.iter().filter(|&&i| packet.voiced[i]).count() as f64 / central.len() as f64;
            if label == "silent" {
                assert!(!packet.voiced.iter().any(|v| *v));
                assert!(y.iter().all(|v| *v == 0.));
            }
            if label == "noise" {
                assert!(voiced < 0.05, "{rate} noise voiced {voiced}");
            }
            if label.ends_with("note") {
                let pitch: Vec<_> = central.iter().map(|&i| packet.pitch[i]).collect();
                let target = if label == "low-note" { 131. } else { 2250. };
                let median = spectral_canvas_mono::dsp::median(&pitch);
                assert!(voiced > 0.95, "{rate} {label} voiced {voiced}");
                assert!(
                    (median - target).abs() < 3.,
                    "{rate} {label} pitch {median}"
                );
            }
        }
    }
}
#[test]
fn invalid_analysis_inputs_fail_and_silent_packet_is_strict_json() {
    for (x, rate) in [(vec![], 44100), (vec![f32::NAN], 44100), (vec![0.; 100], 0)] {
        assert!(analyze(&x, rate, Profile::General).is_err());
    }
    assert!(analyze(&[0.; 100], 48000, Profile::FluteReference).is_err());
    let p = analyze(&vec![0.; 28665], 44100, Profile::General).unwrap();
    let bytes = serde_json::to_vec(&p).unwrap();
    let restored = serde_json::from_slice(&bytes).unwrap();
    assert!(render(&restored, 734, true, true)
        .unwrap()
        .mix()
        .iter()
        .all(|v| *v == 0.));
}
