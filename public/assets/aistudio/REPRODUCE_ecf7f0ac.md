# Reproduce the Rust migration

Run from the crate directory with Rust 1.89+. `cargo fetch --locked --target x86_64-unknown-linux-gnu` populates a Linux host cache. Use the corresponding host target on other systems; this is not evidence that those systems work. All experiment commands below execute Rust binaries. Shell syntax only launches commands and redirects reports.

The existing cloud workspace uses:

```sh
export PATH=/workspace/shared/sc-rust-toolchain/sysroot/bin:$PATH
export CARGO_HOME=/workspace/shared/sc-rust-toolchain/cargo-home
cargo test --locked --offline --release --features bench --lib --tests
cargo build --locked --offline --release --features bench --bins
```

For the frozen Linux desktop checks in this workspace:

```sh
CARGO_TARGET_DIR=/workspace/shared/sc-rust-toolchain/desktop-validation \
PKG_CONFIG_PATH=/workspace/shared/sc-rust-toolchain/alsa/lib/pkgconfig \
cargo test --frozen --features desktop --bin spectral-canvas-desktop
```

That ALSA metadata links the available Linux runtime library. A normal Linux development machine should install its ALSA development package. It does not provide an audio device in this workspace. For formatting here, prepend `/workspace/shared/sc-rust-toolchain/rustfmt-1.89.0-x86_64-unknown-linux-gnu/rustfmt-preview/bin` to PATH and set `LD_LIBRARY_PATH=/workspace/shared/sc-rust-toolchain/sysroot/lib`; run `cargo fmt --all --check`. A normal Rust installation needs only `cargo fmt --all --check`.

## Inputs and fresh outputs

The original workspace layout is:

- `../input/Iowa flute - original then regenerated.wav`: 44.1 kHz mono, 256896 original frames, 44100 silent frames, 256896 archived generated frames.
- `../input/vocals.wav`: 48 kHz stereo, 15947292 frames; cue-pile bench explicitly folds seconds 40–50 to mono; gap stress also uses seconds 50–60.
- `../instrument-bench/iowa-{violin,guitar,woodblock}-test.wav`.
- `../instrument-bench/decent/{bottles,hurdy,glass,clav,ds-example}.zip` and `mecate.dslibrary`, containing preset-mapped WAV/FLAC samples.
- `/workspace/shared/spectral-canvas-continuation/original.wav`, `work/new_performance.json`, and `work/dry-candidate/new_performance.json`: archived guitar source and cue references.
- The retired source files were deleted after verification. The ledger and hash receipts record the verified replacements.

Use a new output folder, for example `mkdir verification-new`, and set these shell variables for readability:

```sh
SC_QA=target/release/spectral-qa
SC_FLUTE='../input/Iowa flute - original then regenerated.wav'
SC_SAMPLES='../instrument-bench/decent'
SC_GUITAR='/workspace/shared/spectral-canvas-continuation'
SC_OUT=verification-new
```

No source fixture is bundled or overwritten. File-producing Rust commands use create-new semantics. Pick new paths when rerunning them.

## Frozen analysis and rendering gates

```sh
$SC_QA guitar-legacy "$SC_GUITAR" > "$SC_OUT/guitar-legacy.json"
$SC_QA guitar-boundaries "$SC_GUITAR" > "$SC_OUT/guitar-boundaries.json"
$SC_QA flute-layers "$SC_FLUTE" > "$SC_OUT/flute-layers.json"
$SC_QA verify-flute "$SC_OUT/flute-layers.json" > "$SC_OUT/flute-verification.json"
$SC_QA flute-faint "$SC_FLUTE" > "$SC_OUT/flute-faint.json"
$SC_QA flute-cutoffs "$SC_FLUTE" > "$SC_OUT/flute-cutoffs.json"
$SC_QA flute-trace "$SC_FLUTE" > "$SC_OUT/flute-trace.json"
$SC_QA gesture "$SC_FLUTE" "$SC_OUT/gesture-ab.wav" > "$SC_OUT/gesture.json"
$SC_QA accent-figure "$SC_FLUTE" "$SC_OUT/accent.svg" > "$SC_OUT/accent.json"
$SC_QA gap-corpus ../input > "$SC_OUT/gap-corpus.json"
$SC_QA engineering ../input/vocals.wav > "$SC_OUT/engineering.json"
$SC_QA cue-passes ../input/vocals.wav "$SC_OUT/cue-passes" > "$SC_OUT/cue-passes.json"
```

`guitar-legacy` reports archived compatibility with an explicit `passed` field; the tiny allowed 0.01 Hz pitch tolerance covers the original three-decimal cue rounding. `guitar-boundaries`, `verify-flute`, `flute-faint`, `gesture`, and `gap-corpus` also fail the process on violated acceptance gates. Cutoff/trace/masked reports preserve experimental outcomes; producing a report is not a universal-success claim. The known-decay accuracy and sustain-refusal gates run in Cargo tests.

## Mapped instrument corpus

```sh
$SC_QA smoke ../instrument-bench > "$SC_OUT/smoke.json"
$SC_QA six-samples "$SC_SAMPLES" > "$SC_OUT/six-samples.json"
$SC_QA round2 "$SC_SAMPLES" > "$SC_OUT/round2.json"
$SC_QA round3 "$SC_SAMPLES" > "$SC_OUT/round3.json"
$SC_QA independent "$SC_SAMPLES" > "$SC_OUT/independent.json"
$SC_QA verify-corpus "$SC_OUT/round3.json" "$SC_OUT/independent.json" "$SC_OUT/six-samples.json" > "$SC_OUT/corpus-verification.json"
$SC_QA masked-holdout "$SC_SAMPLES" > "$SC_OUT/masked-holdout.json"
$SC_QA sampler-run "$SC_SAMPLES" "$SC_OUT/sampler-shaped" shape > "$SC_OUT/sampler-shaped.json"
```

`sampler-run` also supports `plain` and `guard`; each takes a fresh output directory and exports twelve native/44.1 kHz WAVs with separate layer and onset measurements. `verify-corpus` enforces the original 5 ms attack, 3 dB tail/peak-share, and six-case 2 dB/5 ms regression limits. Baseline guard-off failures are diagnostic controls, not failures of the accepted shaped renders.

## Capture experiments and plots

```sh
$SC_QA capture-experiments "$SC_FLUTE" "$SC_OUT/capture" > "$SC_OUT/capture.json"
$SC_QA capture-figures "$SC_OUT/capture" > "$SC_OUT/capture-figures.json"
$SC_QA response-figure "$SC_OUT/capture/panels/response.svg"
$SC_QA reference-match "$SC_FLUTE" "$SC_OUT/reference-match" > "$SC_OUT/reference-match.json"
$SC_QA reference-figures "$SC_FLUTE" "$SC_OUT/reference-match"
```

Capture experiments operate on the archived generated half of the fixture, never the original half as synthesis material. Loudness matching is constant gain with a 2 or 3 dB true-peak margin. The room-fit winner uses the first-note training score only; the second-note scoring region is held out of selection. SVGs replace the retired plotting dependency and retain explicit common scales; layout/color palettes are not pixel replicas.

## Inventory, dependencies, platform status

```sh
$SC_QA audit-migration PATH_TO_EMPTY_RETIRED_DIRECTORY . > "$SC_OUT/migration.json"
$SC_QA verify-source . SOURCE-SHA256.json
cargo tree --locked --offline --no-default-features -e normal,build
cargo tree --locked --offline --features bench -e normal,build
$SC_QA preflight . linux core skip > "$SC_OUT/linux-preflight.json"
```

Extended preflight syntax is `preflight CRATE PLATFORM core|desktop skip|advisories`. Platforms: linux, windows, macos, ios, android. It runs target checks; desktop checks require the host OS to match. Advisory checks invoke installed `cargo audit --no-fetch` and fail if unavailable. No advisory scan, non-Linux build or physical-device test is included in the verified claim.

The migration audit never deletes anything. The report pairs every retired file with its hash and exact replacement row. Intentional absences are accepted only through the hash receipts and a `replaced` ledger row. Unexpected missing or extra retired scripts fail. Source/test files must all be `.rs`.

## Fixture and generic utilities

```sh
$SC_QA six-note "$SC_OUT/six-note.wav" > "$SC_OUT/six-note.json"
$SC_QA compare-pcm "$SC_OUT/six-note.wav" PATH_TO_ORIGINAL_SIX_NOTE.wav
$SC_QA spectrogram INPUT_MONO.wav "$SC_OUT/spectrum.svg"
$SC_QA hash INPUT_FILE
```

The original fixture was independently compared at all 192000 decoded samples; see `evidence/fixture-parity.txt`. `spectral-canvas-mono repair-gap INPUT OUTPUT START_FRAME END_FRAME` handles explicit bounded loss, preserves all outside samples and rejects existing outputs. `capture-tail INPUT OUTPUT ROOM MIC X Y Z` uses the native application's room model with a full tail; the separate bench lattice reproduces the retired longer experiment.

Optional audition-export variants retain the same analysis/verification reports:

```sh
$SC_QA smoke-export ../instrument-bench "$SC_OUT/smoke-audio" > "$SC_OUT/smoke-audio.json"
$SC_QA cutoff-export "$SC_FLUTE" "$SC_OUT/cutoff-audio" > "$SC_OUT/cutoff-audio.json"
$SC_QA faint-export "$SC_FLUTE" "$SC_OUT/faint-original-before-after.wav" > "$SC_OUT/faint-audio.json"
```

These create fresh generated/listening outputs. The faint comparison explicitly contains the reference as its first comparison segment; that reference segment is not passed to synthesis. Its remaining segments are generated from measured packets.
