# SpectraCraft — Cross-Platform Architecture & Build Guide

SpectraCraft is structured so the **same native Rust workspace** builds and runs on **Linux**, **Windows**, and **macOS**, while keeping the five-stage audio engine completely free of OS-specific assumptions so mobile targets (**iOS** and **Android**) can reuse it later without rewriting the DSP core.

See [`SPECTRACRAFT_SPEC.md`](./SPECTRACRAFT_SPEC.md) for the authoritative five-stage pipeline and invariants.

---

## 1. Engine vs. Platform Boundary

### OS-Independent Core Engine (`--no-default-features`)
All five audio stages and their supporting math/catalog modules use only Rust `std` plus pure-Rust dependencies (`hound`, `rustfft`, `serde`, `serde_json`, `craft-atomic`):

| Stage / Subsystem | Modules (`spectracraft-mono/src/`) | OS-Specific Assumptions |
| --- | --- | --- |
| Stage 1 — Dry Regeneration | `reperform.rs`, `lib.rs`, `harmonic.rs`, `edge.rs`, `cue_passes.rs`, `reference_profiles.rs`, `shape_profile.rs`, `string_state.rs`, `attack_guard.rs`, `continuation.rs`, `masked.rs`, `dsp.rs` | None (pure Rust DSP) |
| Stage 2 — Surgical Region Editing (dry only) | `region_edit.rs` | None (pure Rust slice math & crossfades) |
| Stage 3 — Optional Microphone | `capture.rs`, `studio_catalog.rs`, `acoustic_math.rs` | None (pure math; optional `SPECTRAL_SOURCE_CATALOG_PATH` env var via `std::path::Path`) |
| Stage 4 — Optional Space / Room | `capture.rs`, `studio_catalog.rs`, `acoustic_math.rs` | None (pure image-source & 6-band FIR math) |
| Stage 5 — Optional Plugin Slot & Atomic Saves | `pipeline.rs`, `vendor/craft-atomic/` | DSP is pure Rust; `craft-atomic` encapsulates Linux/Unix (`fsync`/`rename`), macOS (`F_FULLFSYNC` via `rustix`), and Windows (retry backoff for sharing/lock violations) |
| Portable Paths & Host Metadata | `platform.rs` (non-feature section) | `std::env::temp_dir()`, OS-appropriate home/app-data paths, quote/tilde path normalization, macOS vs Ctrl shortcut labels |

### Desktop-Only Surface (`--features desktop`)
All desktop OS integrations are isolated in [`spectracraft-mono/src/platform.rs`](./spectracraft-mono/src/platform.rs) behind `#[cfg(feature = "desktop")]`, and consumed by [`spectracraft-mono/src/bin/desktop.rs`](./spectracraft-mono/src/bin/desktop.rs):

| Capability | Adapter in `platform.rs` | Linux Backend | Windows Backend | macOS Backend |
| --- | --- | --- | --- | --- |
| Native File Dialogs | `platform::dialogs::*` (`rfd = 0.15.4`) | XDG Desktop Portal (`ashpd`, pure Rust D-Bus — no GTK3 required) | Win32 COM `IFileDialog` (`windows-sys`) | AppKit `NSOpenPanel` / `NSSavePanel` (`objc2-app-kit`) |
| Audio Output Device | `platform::AudioPlayback` (`rodio = 0.20.1` / `cpal`) | ALSA (`libasound2`) | WASAPI (`windows`) | CoreAudio (`coreaudio-rs`) |
| Windowing & GPU | `platform::run_native_window` (`eframe = 0.31.1` / `glow`) | X11 + OpenGL (`x11` / `glutin` — runs on X11 & XWayland) | Win32 + WGL (`windows-sys` / `glutin`) | AppKit + NSOpenGL (`objc2-app-kit` / `cgl`) |
| Accessibility | `eframe` `"accesskit"` feature | AT-SPI (`accesskit_unix`) | UI Automation (`accesskit_windows`) | NSAccessibility (`accesskit_macos`) |
| Paths & Shortcuts | `platform::{temp_dir, app_data_dir, normalize_user_path, *_shortcut_label}` | `$XDG_DATA_HOME` / `~/.local/share/spectracraft`, `Ctrl+Z` | `%APPDATA%\SpectraCraft`, quote stripping, `Ctrl+Z` | `~/Library/Application Support/SpectraCraft`, `Cmd+Z` |

---

## 2. Building and Testing on Linux, Windows, and macOS

Requires **Rust 1.89 or later** (`rustup`).

### Linux (`x86_64-unknown-linux-gnu`)
Install system build headers required by `cpal` (ALSA) and `eframe`/`winit`/`glutin` (X11 + OpenGL):

```bash
sudo apt-get update
sudo apt-get install -y \
  pkg-config \
  libasound2-dev \
  libx11-dev \
  libxcb-render0-dev \
  libxcb-shape0-dev \
  libxcb-xfixes0-dev \
  libxkbcommon-dev \
  libgl1-mesa-dev

cargo test --locked
cargo test --locked --features desktop --bin spectracraft-desktop
cargo build --locked --release --features desktop --bin spectracraft-desktop
```

> **Wayland note:** `spectracraft-mono/Cargo.toml` currently pins `eframe` with `features = ["accesskit", "default_fonts", "glow", "x11"]`, which runs natively on X11 and via XWayland on Wayland desktops while staying strictly compatible with the pinned `Cargo.lock`. On pure Wayland sessions without XWayland, adding `"wayland"` to `eframe` features and refreshing `Cargo.lock` (`cargo update --workspace`) enables native Wayland support.

### Windows (`x86_64-pc-windows-msvc`)
Install the **Visual Studio Build Tools** ("Desktop development with C++") and the `x86_64-pc-windows-msvc` Rust toolchain. No external audio or graphics SDK is required (WASAPI, Win32, COM `IFileDialog`, and WGL are built into Windows):

```powershell
cargo test --locked
cargo test --locked --features desktop --bin spectracraft-desktop
cargo build --locked --release --features desktop --bin spectracraft-desktop
```

Output executable: `target\release\spectracraft-desktop.exe` (and CLI binaries `spectracraft-mono.exe`, `spectracraft-qa.exe`).

### macOS (`aarch64-apple-darwin` / `x86_64-apple-darwin`)
Install **Xcode Command Line Tools** (`xcode-select --install`). CoreAudio, AppKit, NSOpenGL, and `NSOpenPanel`/`NSSavePanel` come from the macOS SDK:

```zsh
cargo test --locked
cargo test --locked --features desktop --bin spectracraft-desktop
cargo build --locked --release --features desktop --bin spectracraft-desktop
```

Output executable: `target/release/spectracraft-desktop` (plus `spectracraft-mono` and `spectracraft-qa`).

### Automated CI Matrix
See [`.github/workflows/desktop-ci.yml`](./.github/workflows/desktop-ci.yml) for the GitHub Actions workflow covering `ubuntu-latest`, `windows-latest`, and `macos-latest`.

---

## 3. Future Mobile Targets (iOS & Android — Deferred)

Mobile support is intentionally deferred. The audio engine does **not** need to be rewritten when mobile work starts because `spectracraft-mono` already builds as a standalone library with `--no-default-features` (`eframe`, `rfd`, and `rodio` are optional under `--features desktop`, enforced by `src/preflight.rs`).

What will be required later for mobile:

1. **Core Library Packaging**:
   - Compile `spectracraft-mono` with `--no-default-features` as a `staticlib` (iOS: `aarch64-apple-ios`, `aarch64-apple-ios-sim`) and `cdylib` (Android: `aarch64-linux-android`, `x86_64-linux-android`) behind a thin C/UniFFI or Swift/Kotlin bridge exposing the 5-stage pipeline commands and audio buffers.
2. **Mobile Audio Device Layer** (replaces desktop `platform::AudioPlayback`):
   - **iOS**: `AVAudioSession` category/route/interruption management + CoreAudio / `AVAudioEngine` playback callback.
   - **Android**: `Oboe` / `AAudio` (supported by `cpal` when built with the Android NDK or via a thin native audio callback) + audio focus handling.
3. **Mobile File Access** (replaces desktop `platform::dialogs`):
   - **iOS**: `UIDocumentPickerViewController` with security-scoped URLs and bookmark/copy-into-sandbox handling.
   - **Android**: Storage Access Framework (`ACTION_OPEN_DOCUMENT` / `ACTION_CREATE_DOCUMENT`) streaming through content `Uri` file descriptors rather than raw filesystem paths.
4. **Mobile UI & Lifecycle** (replaces desktop `eframe` windowing):
   - Touch-first UI shell (SwiftUI/UIKit on iOS, Jetpack Compose on Android, or mobile `winit`/`egui` with software keyboard & safe-area insets), plus OS background/suspension state persistence and tighter memory budgets for long recordings.
5. **Signing & Distribution**:
   - Apple App Store / TestFlight provisioning + hardened runtime, and Android APK/AAB signing.
