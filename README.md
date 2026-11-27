# SpectraCraft

A single-source audio workstation that regenerates a clean dry performance from a reference track, lets you surgically refine it, optionally places it in a chosen microphone and room environment, and exports standard audio/marker interchange bundles to external DAWs and audio editors.

Repository: <https://github.com/storytold/spectracraft>

## Authoritative Specification & Architecture Docs

- **[`SPECTRACRAFT_SPEC.md`](./SPECTRACRAFT_SPEC.md)** — Canonical Rust architecture, five-stage pipeline contract, acoustic mathematics, UI boundaries, Candidate review policy (Section 14), and verification requirements.
- **[`SPECTRACRAFT_BIBLE.md`](./SPECTRACRAFT_BIBLE.md)** — Expanded governing document (Sections 00–20, 40 stable `SC-*` enforcement requirement IDs), Rust-first engine boundary, and Section 09 Security & Robustness Checklist.
- **[`AUDIT_PROGRESS.md`](./AUDIT_PROGRESS.md)** — Implementation and verification evidence ledger tracking all 33 baseline audit findings (`H1–H15`, `M1–M18`) with independent implementation and verification dimensions.
- **[`spectracraft-mono/source-qualification/PROTOCOL.md`](./spectracraft-mono/source-qualification/PROTOCOL.md)** — Recorded-source qualification protocol, versioned fixture manifest, frozen criteria, and generated 138-source coverage matrix.
- **[`GEMINI.md`](./GEMINI.md)** & **[`.agents/skills/security-robustness-check/SKILL.md`](./.agents/skills/security-robustness-check/SKILL.md)** — Repository guide and standing pre-export security & robustness checklist.
- **[`PLATFORM.md`](./PLATFORM.md)** — Cross-platform shell architecture (native `egui`/`eframe` desktop + Tauri 2 desktop/mobile targets for Linux, macOS, Windows, iOS, and Android).
- **[`PLUGIN_ARCHITECTURE.md`](./PLUGIN_ARCHITECTURE.md)** — UI-free `AudioPipelineEngine`, `EngineStateSnapshot`, and thin DAW/editor interchange layer (`daw_bridge.rs`) for Adobe Audition, Logic Pro, standard DAWs, and ArtCraft SoundCraft.
- **[`spectracraft-mono/DESIGN-CONTRACT.md`](./spectracraft-mono/DESIGN-CONTRACT.md)** — Historical dry-listening checklist & visual/audio separation notes; reconciled under [`SPECTRACRAFT_SPEC.md`](./SPECTRACRAFT_SPEC.md) §14 and `SC-GOV-04` so valid `Candidate` audition and stage export remain available without falsely claiming human listening approval.
- **[`CRAFT_LIBS_MAINTENANCE.md`](./CRAFT_LIBS_MAINTENANCE.md)** — Pinned `storytold/craft-libs` Git dependency policy and thin adapter isolation (`report_export.rs`, `settings_store.rs`, `update_manager.rs`).
- **[`docs/GPUI_INTEGRATION.md`](./docs/GPUI_INTEGRATION.md)** — Experimental native GPUI desktop frontend architecture, MSRV/pinning isolation, performance budgets, verification ledger, and rollback instructions.
- **[`spectracraft-mono/THIRD-PARTY.md`](./spectracraft-mono/THIRD-PARTY.md)** — Third-party crate and reference attributions.

## Five-Stage Audio Pipeline

SpectraCraft processes a single-source track in this fixed, ordered pipeline (see [`SPECTRACRAFT_SPEC.md`](./SPECTRACRAFT_SPEC.md)):

1. **Stage 1 — Dry Regeneration** *(always available, cached)*: Extracts performance shape cues and resynthesizes a completely dry signal using persistent-state physics, additive re-pick, and continuous pitch bends with newly synthesized samples only.
2. **Stage 2 — Surgical Region Editing** *(dry take only)*: Applies localized time-range or rectangular time+frequency marquee fixes to the cached dry take and blends them back with short crossfades. Audio outside the selection remains bit-identical.
3. **Stage 3 — Optional Microphone** *(off / bypassed by default)*: Applies virtual microphone model, polar pattern, position, and aim to the dry take.
4. **Stage 4 — Optional Space / Room** *(off / bypassed by default)*: Applies room dimensions, 6-face surface absorption profiles, and air temperature reflections.
5. **Stage 5 — Optional Plugin Slot** *(off / bypassed by default)*: Applies optional post-capture processing (`GentleEq`, `SoftDynamics`, `NoiseControl`, `PhaseLimiter`) before final output.

## Protected Invariants (Must Stay Intact)

Any packaging, UI, or DAW-interchange work **must** preserve these core guarantees:

- **Stage 1 physics unchanged**: Persistent-state physics, additive re-pick, and continuous-bend behavior remain strictly intact.
- **Zero original PCM in Stage 1–5 outputs**: Regenerated stages never blend original PCM, source phase, or copied residuals into processed outputs.
- **Blind Before/After reporting isolation**: Before and After analyzers inspect only their own single WAV input; comparison summaries are computed strictly from numeric deltas.
- **Thin `craft-libs` adapters**: All `craft-pdf`, `craft-atomic`, and `craft-settings` calls stay isolated behind thin adapters (`report_export.rs`, `settings_store.rs`, `update_manager.rs`) pinned to explicit commits in `Cargo.lock`.
- **Independent stage bypass & display isolation**: Optional stages (3–5) default to bypassed (bit-identical to dry), and visual navigation/zooming never alters audio or render fingerprints.

## Workspace Layout

```text
.
├── Cargo.toml                         # Root Rust workspace (excludes standalone src-tauri for offline --locked core builds)
├── Cargo.lock                         # Pinned lockfile for core CLI, QA, and native egui desktop
├── package.json                       # React + Vite workstation UI + Tauri 2 build scripts
├── server.ts                          # Local Express + Vite dev/API bridge (portable os.tmpdir paths)
├── src/                               # React + TypeScript workstation frontend
├── src-tauri/                         # Tauri 2 cross-platform shell (Linux, macOS, Windows, iOS, Android; no Electron)
│   ├── Cargo.toml                     # Standalone crate linking spectracraft-mono (default-features = false)
│   ├── tauri.conf.json                # Desktop + mobile bundle config
│   ├── capabilities/default.json      # Tauri 2 ACL permissions
│   ├── icons/                         # App icons (32x32, 128x128, 512x512)
│   └── src/                           # Thin IPC bridge (lib.rs with mobile_entry_point + main.rs)
├── apps/spectracraft-gpui/            # Experimental opt-in GPUI desktop frontend (isolated workspace & Cargo.lock)
└── spectracraft-mono/                 # Native Rust DSP core, CLI, QA engine, and native egui desktop binary
    ├── src/
    │   ├── lib.rs                     # Core DSP library & module exports
    │   ├── pipeline.rs                # 5-stage pipeline, AudioPipelineEngine, EngineStateSnapshot
    │   ├── daw_bridge.rs              # Thin DAW/editor interchange layer (Audition, Logic Pro, DAW, SoundCraft)
    │   ├── technical_report.rs        # Blind Before/After analyzer & stacked comparison plots
    │   ├── report_export.rs           # Thin adapter for craft-pdf & craft-atomic
    │   ├── settings_store.rs          # Thin adapter for craft-settings (config.toml)
    │   ├── update_manager.rs          # Non-blocking launch update check & 3-choice verification flow
    │   ├── platform.rs                # OS abstraction layer
    │   ├── main.rs                    # `spectracraft-mono` CLI
    │   └── bin/
    │       ├── desktop.rs             # `spectracraft-desktop` native egui/eframe workstation
    │       └── spectracraft-qa.rs     # `spectracraft-qa` verification, reporting & DAW export CLI
    └── tests/                         # Integration, invariant, sweep & solo-instrument verification suites
```

---

## Required Toolchains

| Target | Required Toolchains & System SDKs |
| --- | --- |
| **Core Rust CLI / QA** | **Rust 1.89+** (`rustup`, `cargo`, `rustfmt`, `clippy`). Zero Python or FFmpeg runtime dependencies. |
| **Web / React UI & Express Bridge** | **Node.js 22.6+** (native TypeScript execution for `npm start`) and `npm` with `package-lock.json` (`npm ci`). |
| **Native `egui` Desktop** (`spectracraft-desktop`) | Rust 1.89+; on Linux: `pkg-config`, `libasound2-dev`, `libx11-dev`, `libxcb-*`, `libxkbcommon-dev`, `libgl1-mesa-dev`; on Windows: Visual Studio C++ Build Tools; on macOS: Xcode Command Line Tools. |
| **Tauri 2 Desktop (Linux x64)** | Rust 1.89+, Node.js 22.6+, Tauri 2 CLI (`cargo install tauri-cli --version "^2"`), plus WebKitGTK & GTK3 dev libs (`libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `patchelf`). |
| **Tauri 2 Desktop (macOS arm64 / x64)** | Rust 1.89+ (`aarch64-apple-darwin` & `x86_64-apple-darwin`), Node.js 22.6+, Tauri 2 CLI, Xcode Command Line Tools. |
| **Tauri 2 Desktop (Windows x64)** | Rust 1.89+ (`x86_64-pc-windows-msvc`), Node.js 22.6+, Tauri 2 CLI, Visual Studio C++ Build Tools, Microsoft Edge WebView2 runtime. |
| **Tauri 2 Mobile (iOS)** | macOS host, **Xcode 15+**, CocoaPods (`brew install cocoapods`), Rust targets `aarch64-apple-ios` & `aarch64-apple-ios-sim`, Apple Developer Team ID for device signing. |
| **Tauri 2 Mobile (Android)** | **Android Studio**, Android SDK (API 26+), Android NDK (`NDK_HOME`), Java 17 (`JAVA_HOME`), Rust targets `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`, keystore for release signing. |

---

## Build Commands by Platform Target

### 1. Core Rust Binaries (CLI & QA)

```sh
cargo build --release --locked
```

This produces `target/release/spectracraft-mono` and `target/release/spectracraft-qa`. To launch the optional local React + Express workstation bridge against the compiled Rust binaries from a fresh clone:

```sh
npm install
npm run dev
```

### 2. Native Desktop Shell (`spectracraft-desktop` via `egui`/`eframe`)

SpectraCraft preserves its native Rust `egui`/`eframe` desktop application alongside Tauri 2:

```sh
# Linux x64 / macOS arm64 & x64 / Windows x64
cargo build --locked --release --features desktop --bin spectracraft-desktop
cargo run --locked --features desktop --bin spectracraft-desktop
```

> **Packaging note:** `cargo build --locked --release --features desktop --bin spectracraft-desktop` outputs a **single standalone executable binary** (`target/release/spectracraft-desktop` or `spectracraft-desktop.exe`), not a packaged OS installer (`.deb`, `.dmg`, `.msi`, or `.exe` setup). Creating a distributable installer for `spectracraft-desktop` requires a separate packaging step (for example `cargo-deb`, `cargo-packager`, `cargo-bundle`, or Inno Setup / WiX).

### 2b. Experimental Native GPUI Shell (`apps/spectracraft-gpui`)

An isolated, reversible experimental GPUI frontend lives in `apps/spectracraft-gpui/` (excluded from the root workspace so root `Cargo.lock` stays GPUI-free). It reuses shared `spectracraft-mono` session, worker, visualization, and playback modules without replacing `spectracraft-desktop`:

```sh
# Check, test, or run the experimental GPUI frontend (uses apps/spectracraft-gpui/rust-toolchain.toml)
cargo check --locked --manifest-path apps/spectracraft-gpui/Cargo.toml
cargo test --locked --manifest-path apps/spectracraft-gpui/Cargo.toml
cargo run --locked --manifest-path apps/spectracraft-gpui/Cargo.toml -- [optional-input.wav]
```

See [`docs/GPUI_INTEGRATION.md`](./docs/GPUI_INTEGRATION.md) for MSRV reconciliation, pinned Zed revision details, performance budgets, verification status, and rollback instructions.

### 3. Tauri 2 Cross-Platform Installers (Linux, Windows, macOS)

> **Native Tauri 2 IPC & Capability Boundary:** `src-tauri/src/lib.rs` wires the full Stage 1–5 workstation workflow directly to `spectracraft-mono` via `TauriTransport` (`src/lib/transport.ts`) with **zero Node/Express bridge required** in packaged desktop builds. A strict CSP (`src-tauri/tauri.conf.json`), least-privilege main-window capability (`src-tauri/capabilities/default.json`), and Rust-owned `ScopedHandleRegistry` (`tauri-media://<id>`) ensure the WebView can never read arbitrary host filesystem paths.

Install frontend dependencies and the Tauri 2 CLI once on a host with Rust 1.89+ and platform SDKs:

```sh
npm install
cargo install tauri-cli --version "^2" --locked
```

Run in development mode:

```sh
npm run tauri:dev
```

Build release installer packages per host platform (outputs land under `src-tauri/target/<triple>/release/bundle/` and can be copied into `dist-installers/`):

```sh
# Generic host installer build via npm script (invokes `cargo tauri build`)
npm run tauri:build

# Linux (x64 — .deb, .rpm, .AppImage; requires WebKitGTK 4.1 + GTK3 dev libs)
cargo tauri build --target x86_64-unknown-linux-gnu

# Windows (x64 — .msi, NSIS .exe; requires MSVC Build Tools + WebView2)
cargo tauri build --target x86_64-pc-windows-msvc

# macOS (Apple Silicon arm64 — .app, .dmg; requires macOS + Xcode CLT)
rustup target add aarch64-apple-darwin
cargo tauri build --target aarch64-apple-darwin

# macOS (Intel x64 — .app, .dmg; requires macOS + Xcode CLT)
rustup target add x86_64-apple-darwin
cargo tauri build --target x86_64-apple-darwin

# macOS (Universal arm64 + x64 — .app, .dmg)
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run tauri:build:macos-universal
```

### 4. Tauri 2 Mobile Builds (iOS & Android)

Tauri 2 mobile configuration is included in `src-tauri/tauri.conf.json` (`iOS` & `android` bundle settings) and `src-tauri/src/lib.rs` (`#[cfg_attr(mobile, tauri::mobile_entry_point)]`). Signing identities / keystores are intentionally not committed.

```sh
# iOS (macOS + Xcode 15+ + CocoaPods required)
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
npm run tauri:ios:init
cargo tauri ios dev          # Simulator / connected device
npm run tauri:ios:build      # IPA / Xcode archive (configure Apple Team ID in Xcode or tauri.conf.json)

# Android (Android SDK API 26+ + Android NDK + Java 17 required)
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
npm run tauri:android:init
cargo tauri android dev      # Emulator / connected device
npm run tauri:android:build  # APK / AAB (configure release keystore before Play Store distribution)
```

### 5. Installer Build Status (Cloud Container vs. Local Host)

| Package / Target | Built in Cloud Container? | Output / Requirement |
| --- | --- | --- |
| **Linux x64 CLI/QA `.deb`** (`spectracraft-cli`) | **Built** (`dpkg-deb` present) | `dist-installers/spectracraft-cli_0.1.0_amd64.deb` (installs `/usr/bin/spectracraft-mono` & `/usr/bin/spectracraft-qa` from prebuilt Linux release binaries; CLI/QA only, not a GUI installer) |
| **Tauri 2 Linux GUI** (`.deb`, `.AppImage`) | **Pending local host** | Missing `rustc`, `cargo`, `cargo-tauri`, `pkg-config`, `libwebkit2gtk-4.1-dev`, `gtk+-3.0`, `linuxdeploy`/`appimagetool` |
| **Tauri 2 Windows GUI** (`.msi`, NSIS `.exe`) | **Pending local host** | Requires Windows host + Rust MSVC + Visual Studio C++ Build Tools + Tauri 2 CLI |
| **Tauri 2 macOS GUI** (`.app`, `.dmg`) | **Pending local host** | Requires macOS host + Xcode Command Line Tools + Rust + Tauri 2 CLI |
| **Tauri 2 Mobile** (`.ipa`, `.apk`/`.aab`) | **Pending local host** | Requires macOS + Xcode (iOS) or Android SDK/NDK + Java 17 (Android) + Rust targets + Tauri 2 CLI |
| **Native `egui` Desktop** (`spectracraft-desktop`) | **Pending local host** | Requires Rust 1.89+ and OS graphics/audio dev libs; outputs a single executable binary |

---

## Audio-Editor & DAW Interchange Layer

SpectraCraft includes a thin, extensible standard-interchange bridge (`spectracraft-mono/src/daw_bridge.rs`) behind `EditorInterchangeAdapter` so external editors receive lossless 24-bit TPDF or 32-bit float WAV stems, sample-accurate markers, and `EngineStateSnapshot` preset JSON (`spectracraft-state.json`):

| Target ID | External Editor | Exported Standard Artifacts |
| --- | --- | --- |
| `audition` | **Adobe Audition** | `stems/*.wav` + `audition-markers.csv` (Audition Markers Panel TSV) + `<session>.sesx` (Audition XML multitrack session) + `spectracraft-state.json` |
| `logic` | **Logic Pro** | `stems/*.wav` + `logic-markers.csv` (sample-accurate marker CSV) + `logic-interchange.json` (AU/ARA stem + marker + preset descriptor) + `spectracraft-state.json` |
| `daw` | **Standard DAW / Audio Editor** | `stems/*.wav` + `markers.csv` (RFC-4180 CSV) + `daw-interchange.json` + `spectracraft-state.json` |
| `soundcraft` | **ArtCraft SoundCraft** | `Audio Files/*.wav` + `<session>.scraft.json` (aligned with `craft-scraft-format` `Source`, `TrackKind::Audio`, and `MemoryLocation`) + `spectracraft-state.json` |

CLI usage:

```sh
target/release/spectracraft-qa export-daw audition   dry.wav final.wav out-audition/   cues.json "Session Title" 24
target/release/spectracraft-qa export-daw logic      dry.wav final.wav out-logic/      cues.json "Session Title" 24
target/release/spectracraft-qa export-daw daw        dry.wav final.wav out-daw/        cues.json "Session Title" 32
target/release/spectracraft-qa export-daw soundcraft dry.wav final.wav out-soundcraft/ cues.json "Session Title" 24
```

You can also trigger any of the four targets directly from the workstation **Export** dialog (`POST /api/sessions/:id/daw-export`) or the Tauri IPC command `export_daw_interchange`. See [`PLUGIN_ARCHITECTURE.md`](./PLUGIN_ARCHITECTURE.md) for the full adapter interface and future VST3/AU/AAX host boundary.

---

## Running Tests & Verification

```sh
# Rust core, CLI, DAW interchange, blind report 20 Hz–20 kHz sweep, and solo-instrument tests
cargo test --locked
cargo fmt --all --check
cargo clippy --locked --lib --bins --tests
cargo audit

# Native egui desktop tests
cargo test --locked --features desktop --bin spectracraft-desktop

# Source integrity fingerprint verification (regenerate after Rust source edits)
npm run verify:manifest
target/release/spectracraft-qa verify-source spectracraft-mono spectracraft-mono/SOURCE-SHA256.json

# Frontend TypeScript check, unit tests, and production bundle
npm ci
npm run lint
npm run test:ts
npm run build
```

## Express Bridge Security & Capability Alignment

When running the optional browser bridge (`server.ts`):
- **Loopback by default**: binds to `127.0.0.1` unless `--host <addr>` or `SPECTRACRAFT_HOST` is explicitly supplied.
- **Remote & maintenance protection**: non-loopback binds or `SPECTRACRAFT_REMOTE_MODE=1` require `SPECTRACRAFT_AUTH_TOKEN` (`Authorization: Bearer ...`); privileged maintenance endpoints support `SPECTRACRAFT_ADMIN_TOKEN`; `/api/updates/action` is disabled in `NODE_ENV=production` unless `SPECTRACRAFT_ENABLE_DEV_UPDATES=1`.
- **Origin & proxy hardening**: mutating requests validate full `scheme://host:port` origins and ignore `X-Forwarded-Host` unless `SPECTRACRAFT_TRUST_PROXY=1`; configure explicit origins with `SPECTRACRAFT_ALLOWED_ORIGINS`.
- **Concurrent server safety**: session storage acquires an exclusive `.server-owner.lock`; a second live instance isolates itself in a per-PID directory without deleting the first instance's artifacts.

| Workflow | Channels | Sample Rates | Max Duration | Formats / Notes |
| --- | --- | --- | --- | --- |
| **Web Upload & Stage 1–5** | Mono only | 8–96 kHz | 90 s | PCM 8/16/24/32-bit, IEEE Float 32-bit, `WAVE_FORMAT_EXTENSIBLE` (zero implicit resampling) |
| **Editable Layers Slicer** | Mono only | 8–96 kHz | 29.5 s window | Matches full upload WAV matrix; emits conversion provenance (`sourceEncoding`, `convertedToFloat32`) |
| **Native `egui` Desktop** | Mono or stereo (explicitly folded to mono with status notice) | 8–96 kHz | 90 s | Background worker renders Stages 3–5 off the GUI thread; v2 session files bind dry approval to SHA-256 |

## CLI Quick Reference

```sh
# Stage 1: Analyze cues and render a fresh dry take (no source PCM blended)
target/release/spectracraft-mono analyze-guitar input.wav cues.json
target/release/spectracraft-mono render-dry-guitar cues.json dry.wav

# Verification, blind technical reporting, DAW interchange & settings
target/release/spectracraft-qa guitar-verify input.wav verify-out/
target/release/spectracraft-qa technical-report raw.wav final.wav report.json
target/release/spectracraft-qa export-report report.json report.pdf pdf
target/release/spectracraft-qa export-daw soundcraft dry.wav final.wav out-soundcraft/
target/release/spectracraft-qa check-craft-libs --online
target/release/spectracraft-qa update-check . --online
target/release/spectracraft-qa settings-get
```

## Scope and Limits

- Virtual microphone and room models are idealized synthesis/capture choices, not forensic recovery of historical recording gear or rooms.
- Rendered outputs are candidates until reviewed by a human listener.
- Mobile targets (iOS and Android) are configured via Tauri 2 (`src-tauri/`) with signing credentials intentionally excluded from version control.
