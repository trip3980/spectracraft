use std::{fs, process::Command};
#[test]
fn cli_is_nondestructive_and_declines_intact_audio() {
    let root = std::env::temp_dir().join(format!("spectral-gap-cli-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let source = root.join("source.wav");
    let out = root.join("out.wav");
    let intact = root.join("intact.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    for (path, broken) in [(&source, true), (&intact, false)] {
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..48000 {
            for hz in [1300., 1700.] {
                let v = if broken && (24000..24096).contains(&i) {
                    0.
                } else {
                    0.2 * (std::f64::consts::TAU * hz * i as f64 / 48000.).sin() as f32
                };
                w.write_sample(v).unwrap();
            }
        }
        w.finalize().unwrap();
    }
    let before = fs::read(&source).unwrap();
    let call = |src: &std::path::Path, dest: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_spectral-canvas-mono"))
            .arg("repair-gap")
            .arg(src)
            .arg(dest)
            .args(["24000", "24096"])
            .output()
            .unwrap()
    };
    assert!(call(&source, &out).status.success());
    let first = fs::read(&out).unwrap();
    assert!(!call(&source, &out).status.success());
    assert_eq!(first, fs::read(&out).unwrap());
    let no = root.join("declined.wav");
    assert!(!call(&intact, &no).status.success());
    assert!(!no.exists());
    assert!(!call(&source, &source).status.success());
    assert_eq!(before, fs::read(&source).unwrap());
    fs::remove_dir_all(root).unwrap();
}
