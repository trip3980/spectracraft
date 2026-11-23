use spectracraft_mono::attack_guard::{measure_quiet_attacks, pre_attack_gain};
#[test]
fn quiet_attacks_work_at_native_rates_without_changing_the_strike() {
    for rate in [16000, 44100, 48000, 96000] {
        let n = (0.51 * rate as f64).round() as usize;
        let mut x = vec![0.; n];
        let marks = [(0.12 * rate as f64) as usize, (0.33 * rate as f64) as usize];
        for &k in &marks {
            for (i, v) in x
                .iter_mut()
                .enumerate()
                .skip(k)
                .take((0.03 * rate as f64).round() as usize)
            {
                let t = (i - k) as f64 / rate as f64;
                *v = (0.4 * (-80. * t).exp() * (std::f64::consts::TAU * 590. * t).cos()) as f32;
            }
        }
        let events = measure_quiet_attacks(&x, rate).unwrap();
        assert_eq!(events.len(), 2);
        let json = serde_json::to_value(&events).unwrap();
        let mut early = x.clone();
        for (ev, &k) in json.as_array().unwrap().iter().zip(&marks) {
            let start = ev["start_sample"].as_u64().unwrap() as usize;
            let h = ev["samples_per_block"].as_u64().unwrap() as usize;
            assert!(start.abs_diff(k) <= h);
            early[start - 10 * h..start].fill(0.07);
        }
        let gain = pre_attack_gain(&early, &events).unwrap();
        for ev in json.as_array().unwrap() {
            let start = ev["start_sample"].as_u64().unwrap() as usize;
            let h = ev["samples_per_block"].as_u64().unwrap() as usize;
            let before = early[start - 9 * h..start - 2 * h]
                .iter()
                .map(|v| v * v)
                .sum::<f32>();
            let after = early[start - 9 * h..start - 2 * h]
                .iter()
                .zip(&gain[start - 9 * h..start - 2 * h])
                .map(|(v, g)| (v * g).powi(2))
                .sum::<f32>();
            assert!(after < before * 1e-6);
            assert!(gain[start..start + 10 * h].iter().all(|g| *g == 1.));
        }
        assert!(measure_quiet_attacks(&vec![0.; n], rate)
            .unwrap()
            .is_empty());
        let held: Vec<_> = (0..n)
            .map(|i| 0.2 * (std::f64::consts::TAU * 220. * i as f64 / rate as f64).cos() as f32)
            .collect();
        assert!(measure_quiet_attacks(&held, rate).unwrap().is_empty());
    }
}

#[test]
fn attack_packet_roundtrip_renders_all_layers_without_source_pcm() {
    use spectracraft_mono::layers::{analyze, render, Controls, Packet};
    for rate in [16000, 44100, 48000, 96000] {
        let mut x = vec![0.; (0.51 * rate as f64).round() as usize];
        for onset in [0.12, 0.33] {
            let start = (onset * rate as f64) as usize;
            for (i, v) in x
                .iter_mut()
                .enumerate()
                .skip(start)
                .take((0.03 * rate as f64) as usize)
            {
                let t = (i - start) as f64 / rate as f64;
                *v = (0.4 * (-80. * t).exp() * (std::f64::consts::TAU * 590. * t).cos()) as f32;
            }
        }
        let before = x.clone();
        let json = serde_json::to_vec(&analyze(&x, rate).unwrap()).unwrap();
        assert_eq!(x, before);
        drop(x);
        let packet: Packet = serde_json::from_slice(&json).unwrap();
        for shape in [false, true] {
            let a = render(&packet, 734, shape).unwrap();
            let b = render(&packet, 734, shape).unwrap();
            assert_eq!(a.tonal, b.tonal);
            assert_eq!(a.texture, b.texture);
            let y = a.mix(Controls::default()).unwrap();
            assert_eq!(y.len(), before.len());
            assert_ne!(y, before);
            assert!(y.iter().all(|v| v.is_finite()));
            let solo = a
                .mix(Controls {
                    solo: Some(0),
                    ..Controls::default()
                })
                .unwrap();
            assert_eq!(solo, a.tonal);
            let muted = a
                .mix(Controls {
                    muted: [true, false],
                    ..Controls::default()
                })
                .unwrap();
            assert_eq!(muted, a.texture);
            assert!(a
                .mix(Controls {
                    muted: [true, true],
                    ..Controls::default()
                })
                .unwrap()
                .iter()
                .all(|v| *v == 0.));
        }
    }
}
