use serde_json::json;
use spectracraft_mono::reperform::{BoundaryPatch, Performance};
#[test]
fn supported_pitch_patch_preserves_every_other_cue_and_is_atomic() {
    let rate = 48000;
    let source: Vec<_> = (0..rate)
        .map(|i| {
            let phase = std::f64::consts::TAU * 440. * i as f64 / rate as f64;
            (0.25 * phase.sin() + 0.12 * (2. * phase).sin() + 0.05 * (3. * phase).sin()) as f32
        })
        .collect();
    let mut hz = vec![440.; 100];
    hz[..3].fill(220.);
    let original = json!({"rate":rate,"samples":rate,"hop_s":0.005,"notes":[{"start":0.1,"end":0.6,"hz":hz,"level_steps":vec![0.1;100],"re_picks":[{"offset_s":0.3,"strength":0.3}],"shape":{"mode_weights":{"4":2.}}},{"start":0.7,"end":0.9,"hz":vec![330.;40],"level_steps":vec![0.1;40]}]});
    let p: Performance = serde_json::from_value(original).unwrap();
    let before = serde_json::to_value(&p).unwrap();
    let source_before = source.clone();
    let good = BoundaryPatch {
        start_s: 0.1,
        end_s: 0.115,
        approximate_hz: 440.,
    };
    let fixed = p.patch_boundaries(&source, rate, &[good]).unwrap();
    let mut after = serde_json::to_value(&fixed).unwrap();
    assert!((after["notes"][0]["hz"][0].as_f64().unwrap() - 440.).abs() < 3.);
    assert_eq!(
        after["notes"][0]["hz"].as_array().unwrap()[3..],
        before["notes"][0]["hz"].as_array().unwrap()[3..]
    );
    after["notes"][0]["hz"] = before["notes"][0]["hz"].clone();
    assert_eq!(after, before);
    assert_eq!(source, source_before);
    let bad = BoundaryPatch {
        approximate_hz: 700.,
        ..good
    };
    assert!(p.patch_boundaries(&source, rate, &[good, bad]).is_err());
    assert_eq!(serde_json::to_value(&p).unwrap(), before);
    assert_eq!(p.boundary_evidence(&source, rate).unwrap().len(), 1);
    assert!(fixed.boundary_evidence(&source, rate).unwrap().is_empty());
    let a = p.synthesize_dry().unwrap();
    let b = fixed.synthesize_dry().unwrap();
    assert_eq!(a[..4800], b[..4800]);
    assert_eq!(a[28800..], b[28800..]);
}
#[test]
fn unsupported_or_cross_event_patch_is_rejected() {
    let p:Performance=serde_json::from_value(json!({"rate":48000,"samples":48000,"hop_s":0.005,"notes":[{"start":0.1,"end":0.2,"hz":vec![440.;20],"level_steps":vec![0.1;20]}]})).unwrap();
    for (start, end) in [(0.1, 0.2), (0.09, 0.11), (f64::NAN, 0.11)] {
        assert!(p
            .patch_boundaries(
                &vec![0.; 48000],
                48000,
                &[BoundaryPatch {
                    start_s: start,
                    end_s: end,
                    approximate_hz: 440.
                }]
            )
            .is_err());
    }
}
