use spectral_canvas_mono::{dsp, edge};
fn contrast(x: &[f32], a: f64, b: f64, f0: f64) -> f64 {
    let lo = (a * 44100.).round() as usize;
    let hi = (b * 44100.).round() as usize;
    let v: Vec<_> = x[lo..hi].iter().map(|v| *v as f64).collect();
    let s = dsp::spectrum(&v, &dsp::hann(v.len()), 65536);
    let mut signal = 0_f64;
    let mut background = Vec::new();
    for (k, v) in s.iter().enumerate() {
        let d = (k as f64 * 44100. / 65536. - f0).abs();
        if d < 18. {
            signal = signal.max(v * v);
        }
        if d > 28. && d < 120. {
            background.push(v * v);
        }
    }
    10. * (signal / dsp::median(&background).max(1e-20)).log10()
}
#[test]
fn crossing_ridges_are_evidenced_without_bridging_or_false_lines() {
    let rate = 44100;
    for (f0, speed, depth, seed) in [
        (1548., 420., -36., 7),
        (792., -540., -39., 11),
        (2196., 295., -32., 6),
    ] {
        let mut rng = dsp::Random::new(seed);
        let mut dry = Vec::new();
        let mut full = Vec::new();
        for i in 0..(1.85 * rate as f64).round() as usize {
            let t = i as f64 / rate as f64;
            let gate = ((t - 0.12) / 0.09).clamp(0., 1.) * ((1.73 - t) / 0.07).clamp(0., 1.);
            let phase = std::f64::consts::TAU * ((f0 - speed * 0.92) * t + 0.5 * speed * t * t);
            let loud = 0.36 * gate * phase.cos();
            let weak = 0.36
                * 10_f64.powf(depth / 20.)
                * gate
                * (std::f64::consts::TAU * f0 * t + 0.73).cos();
            let noise = rng.normal() * 0.00065;
            dry.push((loud + noise) as f32);
            full.push((loud + noise + weak) as f32);
        }
        let start = (0.47 * rate as f64) as usize;
        let w = dsp::hann(120);
        for i in 0..120 {
            let v = (rng.normal() * 0.11 * w[i]) as f32;
            dry[start + i] += v;
            full[start + i] += v;
        }
        let before = edge::analyze(&full, rate, edge::Options::default()).unwrap();
        let extra = edge::analyze(
            &full,
            rate,
            edge::Options {
                faint: true,
                ..Default::default()
            },
        )
        .unwrap();
        let target: Vec<_> = extra
            .tracks
            .iter()
            .skip(before.tracks.len())
            .filter(|t| dsp::median(&t.hz.iter().map(|h| (h - f0).abs()).collect::<Vec<_>>()) < 3.)
            .collect();
        assert!(target.len() >= 2, "{f0}: {} target tracks", target.len());
        assert!(target
            .iter()
            .all(|t| !((t.frames[0] * 128) as f64 / 44100. < 0.92
                && (t.frames.last().unwrap() * 128) as f64 / 44100. > 0.92)));
        let frame = (0.35 * rate as f64 / 128.) as usize;
        assert!((extra.base.pitch[frame] - (f0 + speed * (0.35 - 0.92))).abs() < 20.);
        let old = edge::render(&before, 734, false, false).unwrap().mix();
        let new = edge::render(&extra, 734, false, false).unwrap().mix();
        for (a, b) in [(0.26, 0.50), (1.32, 1.58)] {
            let gain = contrast(&new, a, b, f0) - contrast(&old, a, b, f0);
            assert!(gain > 8., "{f0} [{a},{b}] contrast improvement {gain}");
        }
        let no_target = edge::analyze(
            &dry,
            rate,
            edge::Options {
                faint: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!no_target
            .tracks
            .iter()
            .any(|t| dsp::median(&t.hz.iter().map(|h| (h - f0).abs()).collect::<Vec<_>>()) < 3.));
    }
}
