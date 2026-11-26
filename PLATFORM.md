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
| Stage 5 — Optional Plugin Slot & Atomic Saves | `pipeline.rs`, `report_export.rs` (`craft-atomic`) | DSP is pure Rust; `craft-atomic` (pinned `storytold/craft-libs`) encapsulates Linux/Unix (`fsync`/`rename`), macOS (`F_FULLFSYNC` via `rustix`), and Windows (retry backoff for sharing/lock violations) |
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

## 3. Tauri 2 Cross-Platform Shell (Linux, macOS, Windows, iOS & Android)

Alongside the native Rust `egui`/`eframe` desktop binary (`spectracraft-desktop`), SpectraCraft now includes a thin **Tauri 2** shell under `src-tauri/` that packages the React + Vite workstation UI directly on top of the unchanged `spectracraft-mono` core crate (`default-features = false`, zero Electron):

- **`src-tauri/Cargo.toml`**: Standalone workspace (`exclude = ["src-tauri"]` in root `Cargo.toml` so offline `--locked` core builds remain untouched) declaring `crate-type = ["staticlib", "cdylib", "rlib"]` for desktop + iOS + Android linking.
- **`src-tauri/src/lib.rs`**: Uses `#[cfg_attr(mobile, tauri::mobile_entry_point)]` and exposes thin IPC commands (`platform_info`, `supported_daw_targets`, `get_settings`, `update_settings`, `reset_settings_defaults`, `export_daw_interchange`) directly backed by `spectracraft-mono`.
- **`src-tauri/tauri.conf.json`**: Configures desktop bundles (`deb`, `appimage`, `dmg`/`app`, `msi`/`nsis`) plus mobile minimums (`iOS.minimumSystemVersion = "15.0"`, `android.minSdkVersion = 26`).

### Desktop Tauri 2 Targets

```bash
# Linux x64
cargo tauri build --target x86_64-unknown-linux-gnu

# Windows x64
cargo tauri build --target x86_64-pc-windows-msvc

# macOS arm64 / x64 / Universal
cargo tauri build --target aarch64-apple-darwin
cargo tauri build --target x86_64-apple-darwin
cargo tauri build --target universal-apple-darwin
```

### Mobile Tauri 2 Targets (iOS & Android)

Mobile scaffolds (`src-tauri/gen/apple` and `src-tauri/gen/android`) are generated on demand and intentionally git-ignored so signing certificates, Apple Team IDs, and Android keystores never enter version control:

```bash
# iOS (macOS + Xcode 15+ required)
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo tauri ios init
cargo tauri ios build

# Android (Android SDK API 26+ + NDK + JDK 17 required)
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo tauri android init
cargo tauri android build
```

Remaining mobile production items before App Store / Play Store distribution:
1. **Signing & Provisioning**: Configure Apple Developer Team ID / provisioning profiles for iOS and a release keystore for Android.
2. **Native Mobile File Pickers & Audio Session**: Wire `UIDocumentPickerViewController` (iOS) / Storage Access Framework (Android) and `AVAudioSession` / Oboe focus handling when shipping on-device audio capture.

