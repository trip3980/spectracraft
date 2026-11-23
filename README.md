# SpectraCraft

A single-source audio tool that regenerates a clean dry performance from a reference track, then lets you surgically refine it and optionally place it in a chosen microphone and room environment.

Repository: <https://github.com/storytold/spectracraft>

## Authoritative Specification

- **[`SPECTRACRAFT_SPEC.md`](./SPECTRACRAFT_SPEC.md)** — Canonical Rust architecture, five-stage pipeline contract, acoustic mathematics, UI boundaries, and verification requirements.
- **[`SPECTRACRAFT_BIBLE.md`](./SPECTRACRAFT_BIBLE.md)** — Standing engineering rules and working constraints for SpectraCraft.
- **[`PLATFORM.md`](./PLATFORM.md)** — Cross-platform desktop architecture (Linux, Windows, macOS), platform compatibility layer (`platform.rs`), CI workflow, and future iOS/Android notes.
- **[`PLUGIN_ARCHITECTURE.md`](./PLUGIN_ARCHITECTURE.md)** — UI-free buffer-in/buffer-out engine entry points (`AudioPipelineEngine`) and DAW preset state contract for future VST3/AU/AAX wrapping.
- **[`spectracraft-mono/DESIGN-CONTRACT.md`](./spectracraft-mono/DESIGN-CONTRACT.md)** — Core dry-listening gate and visual/audio separation contract.
- **[`spectracraft-mono/THIRD-PARTY.md`](./spectracraft-mono/THIRD-PARTY.md)** — Third-party crate and reference attributions.

## Five-Stage Audio Pipeline

SpectraCraft processes a single-source track in this fixed, ordered pipeline (see [`SPECTRACRAFT_SPEC.md`](./SPECTRACRAFT_SPEC.md)):

1. **Stage 1 — Dry Regeneration** *(always available, cached)*: Extracts performance shape cues and resynthesizes a completely dry signal using newly synthesized samples only. Source-character guidance and light straightening apply only in this stage.
2. **Stage 2 — Surgical Region Editing** *(dry take only)*: Applies localized time-range or rectangular time+frequency marquee fixes to the cached dry take and blends them back with short crossfades. Audio outside the selection remains unchanged.
3. **Stage 3 — Optional Microphone** *(off / bypassed by default)*: Applies virtual microphone model, polar pattern, position, and aim to the dry take.
4. **Stage 4 — Optional Space / Room** *(off / bypassed by default)*: Applies room dimensions, 6-face surface absorption profiles, and air temperature reflections.
5. **Stage 5 — Optional Plugin Slot** *(off / bypassed by default)*: Applies optional post-capture processing (`GentleEq`, `SoftDynamics`, `NoiseControl`, `PhaseLimiter`) before final output.

## Core Invariants

- **Single source**: One imported performance, voice, or stem per work unit.
- **Performance shape as reference**: Extracted performance evidence stays immutable; loose profiles guide reshaping without replacing evidence.
- **Fresh signal only**: Regenerated stages never blend original PCM, source phase, or copied residuals into processed outputs.
- **Dry-first**: All downstream stages operate strictly on the cached Stage 1/2 dry signal.
- **Independent stage bypass**: Optional stages (Stages 3–5) are off by default, independently bypassable, and preserve settings without substituting original audio.
- **Display isolation**: Zooming, scrolling, unit changes, polar/3D camera navigation, and view modes never alter audio or render fingerprints.
- **Uncertainty preserved**: Ambiguous frames remain explicitly marked rather than invented over.

## Workspace Layout

```text
.
├── Cargo.toml                         # Root Rust workspace manifest
├── Cargo.lock                         # Pinned lockfile (committed for binary reproducibility)
├── SPECTRACRAFT_SPEC.md               # Authoritative Rust specification
├── SPECTRACRAFT_BIBLE.md              # Standing project rules
├── GEMINI.md                          # Agent & verification workflow guide
├── spectracraft-mono/                 # Primary Rust crate (CLI, QA engine, native egui desktop)
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs                     # Core DSP, analysis, resynthesis, pipeline, studio catalogs
│   │   ├── main.rs                    # `spectracraft-mono` CLI
│   │   └── bin/
│   │       ├── desktop.rs             # `spectracraft-desktop` native egui workstation
│   │       └── spectracraft-qa.rs     # `spectracraft-qa` verification & reporting CLI
│   ├── tests/                         # Integration & invariant test suites
│   ├── capture-reference/             # Source, microphone, and 6-band surface reference data
│   ├── vendor/craft-atomic/           # Vendored atomic file-write helper (storytold/craft-libs)
│   ├── deny.toml                      # Dependency policy
│   └── README.md                      # Detailed Rust crate notes, CLI examples & verification details
└── .gitignore
```

## Build, Test, and Run

Requires **Rust 1.89 or later**. Core CLI and QA builds have no Python, FFmpeg, or external runtime dependencies.

### Core CLI & QA Tests

```sh
cargo test --locked
cargo build --release --locked
```

### Optional Bench Suite

```sh
cargo test --locked --release --features bench --lib --tests
cargo build --locked --release --features bench --bins
```

### Native Desktop Workstation (`spectracraft-desktop`)

On Linux, building with `--features desktop` requires standard graphics/windowing libraries and ALSA development headers (`libasound2-dev` + `pkg-config`).

```sh
cargo test --locked --features desktop --bin spectracraft-desktop
cargo run --locked --features desktop --bin spectracraft-desktop
```

### Formatting & Linting

```sh
cargo fmt --all --check
cargo clippy --locked --lib --bins --tests
```

## CLI Quick Reference

```sh
# Stage 1: Analyze cues and render a fresh dry take (no source PCM blended)
target/release/spectracraft-mono analyze-guitar input.wav cues.json
target/release/spectracraft-mono render-dry-guitar cues.json dry.wav

# Verification & reporting
target/release/spectracraft-qa guitar-verify input.wav verify-out/
```

See [`spectracraft-mono/README.md`](./spectracraft-mono/README.md), [`spectracraft-mono/NATIVE-GUITAR.md`](./spectracraft-mono/NATIVE-GUITAR.md), and [`spectracraft-mono/REPRODUCE.md`](./spectracraft-mono/REPRODUCE.md) for full CLI workflows and verification commands.

## Scope and Limits

- SpectraCraft is native Rust source code (CLI, QA, and `egui` desktop workstation), not a signed installer or forensic recovery tool.
- Virtual microphone and room models are idealized synthesis/capture choices, not recovery of historical recording gear or rooms.
- Rendered outputs are candidates until reviewed by a human listener. See [`spectracraft-mono/README.md`](./spectracraft-mono/README.md) for platform verification scope.
