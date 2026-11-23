use spectral_canvas_mono::{qa, Result};
use std::{io::Write, path::Path};

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn fixture(out: &Path) -> Result<()> {
    let (samples, mut meta) = qa::six_note();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(std::io::BufWriter::new(file), spec)?;
    for sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    meta["sha256"] = spectral_canvas_mono::digest::sha256(&std::fs::read(out)?).into();
    print_json(&meta)
}

fn gap_grid(input: &Path) -> Result<()> {
    let (spec, samples) = qa::read_audio(input)?;
    let channels = spec.channels as usize;
    let frames = samples.len() / channels;
    let mut rows = Vec::new();
    for fraction in [0.17, 0.29, 0.41, 0.53, 0.67, 0.79] {
        for ms in [0.5, 2., 4.] {
            let start = (frames as f64 * fraction) as usize;
            let end = (start + (spec.sample_rate as f64 * ms / 1000.).round() as usize).min(frames);
            let mut damaged = samples.clone();
            damaged[start * channels..end * channels].fill(0.);
            let (rendered, status) = spectral_canvas_mono::gap::fill(
                &damaged,
                spec.sample_rate,
                channels,
                start,
                end,
                true,
            );
            if rendered[..start * channels] != damaged[..start * channels]
                || rendered[end * channels..] != damaged[end * channels..]
            {
                return Err("Out-of-gap mutation".into());
            }
            if status != "filled" && rendered != damaged {
                return Err("Declined patch changed".into());
            }
            let reference = &samples[start * channels..end * channels];
            let before = reference.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
            let after = rendered[start * channels..end * channels]
                .iter()
                .zip(reference)
                .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
                .sum::<f64>();
            rows.push(serde_json::json!({
                "start_frame": start, "ms": ms, "decision": status,
                "error_reduction_db": (before > 0.).then(|| 10. * (before / after.max(1e-30)).log10())
            }));
        }
    }
    print_json(&rows)
}

fn preflight(root: &Path) -> Result<()> {
    let commands = [
        vec![
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--no-deps",
        ],
        vec![
            "tree",
            "--locked",
            "--offline",
            "--no-default-features",
            "-e",
            "normal,build",
        ],
        vec![
            "check",
            "--locked",
            "--offline",
            "--lib",
            "--no-default-features",
        ],
    ];
    let mut checks = Vec::new();
    for args in commands {
        let result = std::process::Command::new("cargo")
            .args(&args)
            .current_dir(root)
            .output();
        checks.push(match result {
            Ok(process) => serde_json::json!({
                "command": args, "passed": process.status.success(),
                "stdout": String::from_utf8_lossy(&process.stdout),
                "stderr": String::from_utf8_lossy(&process.stderr)
            }),
            Err(error) => {
                serde_json::json!({"command": args, "passed": false, "error": error.to_string()})
            }
        });
    }
    let passed = checks.iter().all(|value| value["passed"] == true);
    print_json(&serde_json::json!({
        "checks": checks, "native_device_smoke": "not run", "windows": "not verified",
        "advisories": "not checked", "audio_quality": "not assessed"
    }))?;
    std::io::stdout().flush()?;
    if !passed {
        return Err("Preflight failed".into());
    }
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["guitar-boundaries",root] => {let result=qa::guitar_boundaries(Path::new(root))?;print_json(&result)?;if result["passed"]!=true{return Err("Archived boundary parity failed".into());}Ok(())}
        ["guitar-legacy",root] => print_json(&qa::guitar_legacy(Path::new(root))?),
        ["cue-passes",input,out] => print_json(&qa::cue_passes(Path::new(input),Path::new(out))?),
        ["verify-corpus",prior,independent,six] => {
            let load=|path:&str|->Result<serde_json::Value>{Ok(serde_json::from_reader(std::fs::File::open(path)?)?)};
            let result=spectral_canvas_mono::metrics::verify_corpus_reports(&load(prior)?,&load(independent)?,&load(six)?)?;
            print_json(&result)?;if result["passed"]!=true{return Err("Frozen corpus acceptance failed".into());}Ok(())
        }
        ["verify-flute",input] => {
            let report=serde_json::from_reader(std::fs::File::open(input)?)?;
            let result=spectral_canvas_mono::metrics::verify_flute_report(&report)?;
            print_json(&result)?;if result["passed"]!=true{return Err("Frozen flute acceptance failed".into());}Ok(())
        }
        ["flute-layers",input] => print_json(&qa::flute_layers(Path::new(input))?),
        ["faint-export",input,out] => print_json(&qa::faint_export(Path::new(input),Path::new(out))?),
        ["cutoff-export",input,out] => print_json(&qa::cutoff_export(Path::new(input),Path::new(out))?),
        ["flute-faint",input] => {let r=qa::flute_faint(Path::new(input))?;print_json(&r)?;if r["passed"]!=true{return Err("Frozen faint-ridge acceptance failed".into());}Ok(())},
        ["flute-cutoffs",input] => print_json(&qa::flute_cutoffs(Path::new(input))?),
        ["flute-trace",input] => print_json(&qa::flute_trace(Path::new(input))?),
        ["gesture",input,out] => print_json(&qa::gesture(Path::new(input),Path::new(out))?),
        ["accent-figure",input,out] => print_json(&spectral_canvas_mono::figures::accent(Path::new(input),Path::new(out))?),
        #[cfg(feature="bench")]
        ["reference-match",input,out] => print_json(&spectral_canvas_mono::room_bench::reference_match(Path::new(input),Path::new(out))?),
        #[cfg(feature="bench")]
        ["response-figure",out] => spectral_canvas_mono::room_bench::response_figure(Path::new(out)),
        #[cfg(feature="bench")]
        ["reference-figures",input,out] => spectral_canvas_mono::room_bench::reference_figures(Path::new(input),Path::new(out)),
        #[cfg(feature="bench")]
        ["capture-figures",root] => print_json(&spectral_canvas_mono::room_bench::capture_figures(Path::new(root))?),
        #[cfg(feature="bench")]
        ["capture-experiments",input,out] => print_json(&spectral_canvas_mono::room_bench::capture_experiments(Path::new(input),Path::new(out))?),
        #[cfg(feature="bench")]
        ["masked-holdout",root] => print_json(&spectral_canvas_mono::bench::masked_holdout(Path::new(root))?),
        #[cfg(feature="bench")]
        ["smoke-export",root,out] => print_json(&spectral_canvas_mono::bench::smoke_export(Path::new(root),Path::new(out))?),
        #[cfg(feature="bench")]
        ["smoke",root] => print_json(&spectral_canvas_mono::bench::smoke(Path::new(root))?),
        #[cfg(feature="bench")]
        ["round2",root] => print_json(&spectral_canvas_mono::bench::round2(Path::new(root))?),
        #[cfg(feature="bench")]
        ["sampler-run",root,out,mode] => print_json(&spectral_canvas_mono::bench::sampler_run(Path::new(root),Path::new(out),mode)?),
        #[cfg(feature="bench")]
        ["round3",root] => print_json(&spectral_canvas_mono::bench::round3(Path::new(root))?),
        #[cfg(feature="bench")]
        ["six-samples",root] => print_json(&spectral_canvas_mono::bench::six_samples(Path::new(root))?),
        #[cfg(feature="bench")]
        ["independent",root] => print_json(&spectral_canvas_mono::bench::independent(Path::new(root))?),
        ["gap-corpus", input] => print_json(&qa::gap_corpus(Path::new(input))?),
        ["gap-grid", input] => gap_grid(Path::new(input)),
        ["engineering", input] => print_json(&qa::engineering_checks(Path::new(input))?),
        ["preflight", root] => preflight(Path::new(root)),
        ["preflight", root, platform, desktop, advisories] => {let r=spectral_canvas_mono::preflight::check(Path::new(root),platform,*desktop=="desktop",*advisories=="advisories")?;print_json(&r)?;if r["passed"]!=true{return Err("Requested preflight gates did not pass".into());}Ok(())},
        ["six-note", out] => fixture(Path::new(out)),
        ["audit-migration", retired, active] => {
            print_json(&spectral_canvas_mono::migration::audit(Path::new(retired), Path::new(active))?)
        }
        ["compare-pcm", left, right] => {
            let (a, x) = qa::read_audio(Path::new(left))?;
            let (b, y) = qa::read_audio(Path::new(right))?;
            if a != b || x != y { return Err("WAV spec or decoded PCM differs".into()); }
            println!("Identical WAV spec and decoded PCM: {} samples", x.len());
            Ok(())
        }
        ["source-inventory",root] => print_json(&spectral_canvas_mono::migration::source_inventory(Path::new(root))?),
        ["verify-source",root,manifest] => {let expected:serde_json::Value=serde_json::from_reader(std::fs::File::open(manifest)?)?;let actual=spectral_canvas_mono::migration::source_inventory(Path::new(root))?;if actual!=expected{return Err("Source bundle fingerprints differ".into());}println!("All source fingerprints match");Ok(())},
        ["hash", input] => {
            let hash = spectral_canvas_mono::digest::sha256(&std::fs::read(input)?);
            println!("{hash}  {input}");
            Ok(())
        }
        ["stereo", input] => {
            let (spec, samples) = qa::read_audio(Path::new(input))?;
            if spec.channels != 2 { return Err("Expected stereo WAV".into()); }
            print_json(&qa::stereo(&samples)?)
        }
        ["spectrogram", input, out] => {
            let (spec, samples) = qa::read_audio(Path::new(input))?;
            if spec.channels != 1 {
                return Err("Expected mono WAV; channel folding must be explicit".into());
            }
            qa::write_new(Path::new(out), qa::spectrogram_svg(&samples, spec.sample_rate)?.as_bytes())
        }
        _ => Err(concat!(
            "Usage: spectral-qa stereo INPUT | engineering INPUT | spectrogram INPUT OUTPUT.svg | ",
            "six-note OUTPUT.wav | gap-grid INPUT | gap-corpus INPUT_DIR | compare-pcm LEFT RIGHT | ",
            "hash FILE | audit-migration RETIRED_DIR CRATE | source-inventory CRATE | verify-source CRATE MANIFEST | ",
            "preflight CRATE [PLATFORM core|desktop skip|advisories] | guitar-legacy|guitar-boundaries GUITAR_ROOT | ",
            "flute-layers|flute-faint|flute-cutoffs|flute-trace INPUT | verify-flute REPORT | ",
            "gesture|faint-export|accent-figure INPUT OUTPUT | cutoff-export|cue-passes INPUT NEW_DIR | ",
            "smoke INPUT_DIR | smoke-export INPUT_DIR NEW_DIR | six-samples|round2|round3|independent|masked-holdout PACK_ROOT | ",
            "sampler-run PACK_ROOT NEW_DIR plain|guard|shape | verify-corpus ROUND3 INDEPENDENT SIX | ",
            "capture-experiments|reference-match INPUT NEW_DIR | capture-figures DIR | response-figure OUTPUT.svg | reference-figures INPUT DIR"
        ).into()),
    }
}
