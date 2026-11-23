# SpectraCraft — Rust source handoff

The existing `audit-preview5` project has been continued in place. Application code, analysis, synthesis, experiments, visual reports, and QA are implemented in Rust. `MIGRATION.tsv` accounts for all 38 retired scripts individually, with replacement entry points, evidence, and numerical/presentation differences. All 38 ledger entries have a verified Rust replacement. After verification, the retired project sources were deleted. Their baseline hashes are preserved in `RETIRED-DELETIONS.tsv`. Source audio was not overwritten.

## Verified behavior

- 55 Linux core/bench/guitar tests and 5 Linux desktop tests pass (including 12 dedicated known-pitch, multi-sample-rate, held-out, abstention, and variant-export guitar controls). The desktop tests include saved-packet import, isolated layer controls, fixed reference gain, seek, 390/1200-pixel layouts, and individual/all guitar variant export. Physical playback is unverified: this workspace has no `/dev/snd`.
- Original guitar compatibility: 48 events, identical archived event boundaries and accents; maximum pitch difference 0.001 Hz from rounding. The three restoration ranges and five boundary patches reproduce archived pitches exactly, preserving source audio and every non-pitch cue.
- Native guitar cue analysis separates `supported`, `uncertain`, `out_of_target`, and `silence` evidence, retains competing pitch hypotheses and abstains on ambiguous frames, automatically detects sustained-note `re_picks` and multi-harmonic overlapping ringing, and renders dry candidates from saved cue JSON alone. Post-dry-review variant export supports individual or `all` selection across `original`, `natural` (labeled SM7-class approx at ~2 ft in padded 10×10×9 ft room), `digital`, `analog` (2×-oversampled asymmetric triode/sag/tone-stack path with zero synthetic hum), and `expressive` (pre-capture cue gesture edits).
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
cargo test --frozen --features desktop --bin spectracraft-desktop
cargo run --locked --features desktop --bin spectracraft-desktop
```

Linux desktop development needs ALSA development metadata and the graphics/windowing dependencies of eframe. The tested workspace-specific environment is documented in `REPRODUCE.md`.

## Use

```sh
# Native monophonic guitar: edit cues between these commands.
target/release/spectracraft-mono analyze-guitar input.wav cues.json
target/release/spectracraft-mono render-dry-guitar cues.json fresh-dry.wav
# Post-dry-review variant export (selection: "all" or comma list of original,natural,digital,analog,expressive).
target/release/spectracraft-mono export-guitar-variants input.wav cues.json fresh-variants-dir all

# Full-track dry verification and variant reports (waveforms, spectrograms, level-matched A/B, independent pitch check).
target/release/spectracraft-qa guitar-verify input.wav fresh-verify-dir
target/release/spectracraft-qa guitar-variants input.wav cues.json fresh-variants-dir all
target/release/spectracraft-qa guitar-pitch-check ref.wav cand.wav 20.0 27.0

# Original 48 kHz guitar compatibility recipe.
target/release/spectracraft-mono analyze-guitar-legacy input.wav legacy-cues.json

# Editable pitch, ringing, texture, and breath-accent layers.
target/release/spectracraft-mono analyze-layers input.wav analysis.json
target/release/spectracraft-mono render-layers analysis.json fresh-output-directory

# Opt-in faint-ridge tracking, or the separate basic harmonic profile.
target/release/spectracraft-mono analyze-layers-faint input.wav faint.json
target/release/spectracraft-mono analyze-harmonic input.wav harmonic.json
target/release/spectracraft-mono render-harmonic harmonic.json fresh-harmonic.wav
```

Render commands need measured JSON only, not the source WAV. They generate fresh samples. The desktop has independent view/mute/solo/gain controls, fixed audition headroom, reset, seek and packet save/load. Loaded packets must match the current reference's rate and duration. Layer gains are session controls; saving analysis saves measurement parameters, not those mixer settings or source PCM.

Input bounds: guitar 8–96 kHz mono, at most 90 seconds; legacy guitar 48 kHz; harmonic/layer analysis 16–96 kHz mono, at most 30 seconds; flute-reference profile explicitly 44.1 kHz. `analyze-flute` selects that profile. `repair-gap` is a distinct explicit-loss repair operation: it preserves unaffected PCM and is not fresh synthesis. `capture-tail` includes the modeled tail; `capture` retains clip length. New output files/directories are required to prevent overwrites.

See `NATIVE-GUITAR.md` for explicit re-picks and `REPRODUCE.md` for every experiment and verification command. `SOURCE-SHA256.json` fingerprints source, tests, manifests and documentation; `spectracraft-qa verify-source . SOURCE-SHA256.json` verifies an extracted handoff.

## Limits

This is a completed Rust capability migration, not a certification of perceptual audio quality or exact floating-point/PCM parity with every retired implementation. Uniform phase draws match reference PCG64 vectors; synthetic Gaussian/transient streams differ. The general harmonic FFT uses a power-of-two length; resampling uses bounded native sinc; gap solving uses dense regularized equations checked independently by QR. SVG replaces Python-generated PNGs, and native controls replace the HTML workspace.

The models remain predominantly monophonic and experimental. Automated guitar analysis now detects repeated strikes (`re_picks`) and multi-harmonic overlapping ringing when spectrogram evidence supports them, while keeping ambiguous frames as `uncertain` and unrelated clicks/chirps as `out_of_target`; independent pitch checks report unresolved frames explicitly and never fold octave errors. Masked-onset detection can still miss or misidentify events; the holdout report preserves those findings. Room and capture models (including the labeled SM7-class approximation and nonlinear analog path) are idealized models, not measured identification of original equipment. Bench capture experiments intentionally process the archived generated take and must not be confused with the fresh dry-synthesis path.

Only Linux compilation and software tests are verified. Physical audio playback, Windows/macOS/iOS/Android builds and runtime behavior, human listening quality, and dependency-advisory status remain unverified. Any rendered WAV that has not been reviewed by human listening is a **candidate**, not a listening pass. The platform preflight supports those requested checks and reports failures/blocks; it does not imply they passed.

## What SpectraCraft is not

SpectraCraft is not evidence recovery for forensic use, proof of authorship, an automatic transcription guarantee, or a generic mastering tool. A plausible reconstruction is an interpretation of acoustic cues, not a claim that a missing or ambiguous event happened. This archive is Rust source code, not a signed desktop or mobile installer.

## Technical references

- [1] Serra, X., and Smith, J. O., III (1990). *Spectral Modeling Synthesis: A Sound Analysis/Synthesis System Based on a Deterministic plus Stochastic Decomposition.* Computer Music Journal, 14(4), 12–24.
- [2] Karplus, K., and Strong, A. (1983). *Digital Synthesis of Plucked-String and Drum Timbres.* Computer Music Journal, 7(2), 43–55. (Background on plucked-string synthesis; not the algorithm claimed for the current modal renderer.)
- [3] Monniaux, D. (2011). *Slow solo 20110108.* Wikimedia Commons (`CC BY-SA 3.0` / `GFDL`; not redistributed in this archive).
- [4] Lindroos, N., Penttinen, H., and Välimäki, V. *Parametric Electric Guitar Synthesis.* Aalto University.

