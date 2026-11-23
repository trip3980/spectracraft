use std::{fs, path::PathBuf, process::Command};

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn renders_export_without_source_at_render_time() {
    let temp = Temp(std::env::temp_dir().join(format!("sc-mono-test-{}", std::process::id())));
    fs::create_dir(&temp.0).unwrap();
    let original = temp.0.join("original.wav");
    let export = temp.0.join("result.wav");
    {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&original, spec).unwrap();
        for i in 0..12_000 {
            let s = (6000. * (std::f64::consts::TAU * 440. * i as f64 / 48000.).sin()) as i16;
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }
    let call = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_spectral-canvas-mono"))
            .current_dir(&temp.0)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(call(&["analyze", "original.wav", "features.json"])
        .status
        .success());
    assert!(call(&["analyze-texture", "original.wav", "texture.json"])
        .status
        .success());
    let features: serde_json::Value =
        serde_json::from_slice(&fs::read(temp.0.join("texture.json")).unwrap()).unwrap();
    assert!(!features["texture"].as_array().unwrap().is_empty());
    fs::remove_file(&original).unwrap(); // temporary synthetic input, not user audio
    assert!(call(&["render", "features.json", "result.wav"])
        .status
        .success());
    assert!(call(&["render", "texture.json", "texture.wav"])
        .status
        .success());
    assert_eq!(
        hound::WavReader::open(temp.0.join("texture.wav"))
            .unwrap()
            .duration(),
        12_000
    );
    let mut r = hound::WavReader::open(&export).unwrap();
    assert_eq!(
        (r.spec().channels, r.spec().sample_rate, r.duration()),
        (1, 48000, 12000)
    );
    let output: Vec<_> = r.samples::<f32>().map(|x| x.unwrap()).collect();
    assert!(output.iter().all(|x| x.is_finite() && x.abs() < 0.9));
    assert!(output.iter().any(|x| x.abs() > 0.1));
    assert!(!call(&["render", "features.json", "result.wav"])
        .status
        .success());
    let mut r = hound::WavReader::open(export).unwrap();
    assert_eq!(
        output,
        r.samples::<f32>().map(|x| x.unwrap()).collect::<Vec<_>>()
    );
}

#[test]
fn editable_layers_render_from_packet_and_reject_overwrite() {
    let temp = Temp(std::env::temp_dir().join(format!("sc-layers-cli-{}", std::process::id())));
    fs::create_dir(&temp.0).unwrap();
    let source = temp.0.join("reference.wav");
    let rate = 48000;
    let x: Vec<_> = (0..12000)
        .map(|i| (0.12 * (std::f64::consts::TAU * 440. * i as f64 / rate as f64).sin()) as f32)
        .collect();
    spectral_canvas_mono::write_wav(rate, &x, &source).unwrap();
    let before = fs::read(&source).unwrap();
    let call = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_spectral-canvas-mono"))
            .current_dir(&temp.0)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(call(&["analyze-layers", "reference.wav", "analysis.json"])
        .status
        .success());
    assert_eq!(fs::read(&source).unwrap(), before);
    fs::rename(&source, temp.0.join("preserved-reference.wav")).unwrap();
    let p: serde_json::Value =
        serde_json::from_slice(&fs::read(temp.0.join("analysis.json")).unwrap()).unwrap();
    assert!(p["base"].get("pcm").is_none());
    assert!(call(&["render-layers", "analysis.json", "export"])
        .status
        .success());
    let mut stems = Vec::new();
    for name in ["pitch", "ring", "texture", "accent", "mix"] {
        let mut r = hound::WavReader::open(temp.0.join(format!("export/{name}.wav"))).unwrap();
        assert_eq!(r.duration(), 12000);
        stems.push(r.samples::<f32>().map(Result::unwrap).collect::<Vec<_>>());
    }
    for i in 0..12000 {
        assert!((stems[..4].iter().map(|s| s[i]).sum::<f32>() - stems[4][i]).abs() < 1e-7);
    }
    let mix = fs::read(temp.0.join("export/mix.wav")).unwrap();
    assert!(!call(&["render-layers", "analysis.json", "export"])
        .status
        .success());
    assert_eq!(mix, fs::read(temp.0.join("export/mix.wav")).unwrap());
}
