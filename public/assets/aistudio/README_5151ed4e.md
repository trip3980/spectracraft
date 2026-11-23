# Spectral Canvas — Rust source handoff

The existing `audit-preview5` project has been continued in place. Application code, analysis, synthesis, experiments, visual reports, and QA are implemented in Rust. `MIGRATION.tsv` accounts for all 38 retired scripts individually, with replacement entry points, evidence, and numerical/presentation differences. All 38 ledger entries have a verified Rust replacement. After verification, the retired project sources were deleted. Their baseline hashes are preserved in `RETIRED-DELETIONS.tsv`. Source audio was not overwritten.

## Verified behavior

- 43 Linux core/bench tests and four Linux desktop tests pass. The desktop tests include saved-packet import, isolated layer controls, fixed reference gain, seek, and 390/1200-pixel layouts. Physical playback is unverified: this workspace has no `/dev/snd`.
- Original guitar compatibility: 48 events, identical archived event boundaries and accents; maximum pitch difference 0.001 Hz from rounding. The three restoration ranges and five boundary patches reproduce archived pitches exactly, preserving source audio and every non-pitch cue.
- Five-rate harmonic pitch/noise/silence gates and four-rate quiet-attack gates pass. Twenty mapped instrument clips pass all 60 frozen render-seed checks; six native/44.1 kHz regressions pass.
- Flute resonance passes the unchanged three-seed tail, held-note and timing thresholds. Faint-ridge controls cover three synthetic crossings, no-target cases and the real flute: the required ridge improves 3.69 dB, and the final tail is bit-identical across seeds.
- Breath accents pass local carrier, timing, full-excerpt RMS, and quantized outside-window equality checks. Masked attacks conserve the local texture energy to the original 1e-10 tolerance; their five-bed holdout reports detection and off-target behavior separately.
- Cue piles have exclusive bins and a shared fresh phase field; isolated stems sum to the full render. The known harmonic-stack control and real ten-second vocal excerpt pass.
- Gap repair has an independent Householder-QR oracle, stereo CLI tests and strict real-source/control/stress verification. Accepted repairs improve sample error; this is not a listening verdict.
- Native capture benches implement loudness/true-peak matching, a 1.5-second image-source lattice, frequency-dependent wall FIRs, and the 145-candidate training-only fit with held-out scoring. SVG reports use explicit common numeric scales.

Current results are the `evidence/rust-*` files. Earlier `tests.txt`, `desktop-check.txt`, `migration-audit.json`, and similarly unprefixed files in the working directory are historical checkpoint evidence, not current results. The source bundle selects current reports plus original fixture-parity evidence.

## Build

Use Rust 1.89 or later. Dependencies are pinned in `Cargo.toml` and `Cargo.lock`; they are not vendored. First fetch needs registry access. No Python, NumPy, SciPy, Matplotlib, Playwright, FFmpeg, or external audio decoder is required.

```sh
cargo fetch --locked --target x86_64-unknown-linux-gnu
cargo test --locked --release --features bench --lib --tests
cargo build --locked --release --features bench --bins
cargo fmt --all --check
```

Add `--offline` after fetching to reproduce the tested offline builds. The core has four direct dependencies: hound, rustfft, serde, and serde_json. Optional `bench` dependencies provide native ZIP/XML, FLAC and EBU R128 processing. `desktop` adds eframe, rfd and rodio; it needs normal platform graphics/windowing and audio libraries. This is Rust application/DSP code, not a freestanding or entirely static executable. Recorded Linux CLI runtime dependencies are libc, libm, libgcc_s and the loader.

```sh
cargo test --frozen --features desktop --bin spectral-canvas-desktop
cargo run --locked --features desktop --bin spectral-canvas-desktop
```

Linux desktop development needs ALSA development metadata and the graphics/windowing dependencies of eframe. The tested workspace-specific environment is documented in `REPRODUCE.md`.

## Use

```sh
# Native monophonic guitar: edit cues between these commands.
target/release/spectral-canvas-mono analyze-guitar input.wav cues.json
target/release/spectral-canvas-mono render-dry-guitar cues.json fresh-dry.wav

# Original 48 kHz guitar compatibility recipe.
target/release/spectral-canvas-mono analyze-guitar-legacy input.wav legacy-cues.json

# Editable pitch, ringing, texture, and breath-accent layers.
target/release/spectral-canvas-mono analyze-layers input.wav analysis.json
target/release/spectral-canvas-mono render-layers analysis.json fresh-output-directory

# Opt-in faint-ridge tracking, or the separate basic harmonic profile.
target/release/spectral-canvas-mono analyze-layers-faint input.wav faint.json
target/release/spectral-canvas-mono analyze-harmonic input.wav harmonic.json
target/release/spectral-canvas-mono render-harmonic harmonic.json fresh-harmonic.wav
```

Render commands need measured JSON only, not the source WAV. They generate fresh samples. The desktop has independent view/mute/solo/gain controls, fixed audition headroom, reset, seek and packet save/load. Loaded packets must match the current reference's rate and duration. Layer gains are session controls; saving analysis saves measurement parameters, not those mixer settings or source PCM.

Input bounds: guitar 8–96 kHz mono, at most 90 seconds; legacy guitar 48 kHz; harmonic/layer analysis 16–96 kHz mono, at most 30 seconds; flute-reference profile explicitly 44.1 kHz. `analyze-flute` selects that profile. `repair-gap` is a distinct explicit-loss repair operation: it preserves unaffected PCM and is not fresh synthesis. `capture-tail` includes the modeled tail; `capture` retains clip length. New output files/directories are required to prevent overwrites.

See `NATIVE-GUITAR.md` for explicit re-picks and `REPRODUCE.md` for every experiment and verification command. `SOURCE-SHA256.json` fingerprints source, tests, manifests and documentation; `spectral-qa verify-source . SOURCE-SHA256.json` verifies an extracted handoff.

## Limits

This is a completed Rust capability migration, not a certification of perceptual audio quality or exact floating-point/PCM parity with every retired implementation. Uniform phase draws match reference PCG64 vectors; synthetic Gaussian/transient streams differ. The general harmonic FFT uses a power-of-two length; resampling uses bounded native sinc; gap solving uses dense regularized equations checked independently by QR. SVG replaces Python-generated PNGs, and native controls replace the HTML workspace.

The models remain predominantly monophonic and experimental. Source analysis does not automatically assign re-picks. Masked-onset detection can miss or misidentify events; the holdout report preserves those findings. Room models are idealized assumptions, not measured identification of original equipment. Bench capture experiments intentionally process the archived generated take and must not be confused with the fresh dry-synthesis path.

Only Linux compilation and software tests are verified. Physical audio playback, Windows/macOS/mobile builds and runtime behavior, listening quality, and dependency-advisory status remain unverified. The platform preflight supports those requested checks and reports failures/blocks; it does not imply they passed.
