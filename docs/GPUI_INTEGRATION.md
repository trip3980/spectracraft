# SpectraCraft Experimental Native GPUI Integration

> **Status**: Experimental opt-in native frontend (`apps/spectracraft-gpui/`).
> **Default desktop status**: Preserved — `spectracraft-desktop` (`egui`), CLI (`spectracraft-mono`, `spectracraft-qa`), React/Express bridge, and Tauri 2 shell (`src-tauri/`) remain untouched as the primary surfaces.
> **Promotion decision**: **Keep experimental — do not replace default desktop yet.** Live GPU/display-server, real OS audio-device, and cross-platform macOS/Windows smoke checks are not yet proven in this headless Linux container.

---

## 1. Phase 0 — Baseline Inspection & MSRV Reconciliation

### Working Tree State vs. Inspection Reference
- **Inspection reference SHA**: `9fbd775800b039f77cb6c9a901a458a0dadb715a` (published `main` baseline).
- **Local workspace differences preserved**:
  - Workspace has no `.git` metadata directory in this container; all unpushed audit corrections (`H1–H15`, `M1–M18`), `SPECTRACRAFT_BIBLE.md` Sections 00–20 (`SC-*`), `AUDIT_PROGRESS.md`, `GEMINI.md`, `source_qualification.rs`, and `desktop.rs` (`preamp_drive` initializer) were preserved intact.
  - Pre-existing baseline Rust test failures observed prior to GPUI work (recorded transparently and NOT suppressed):
    1. `audio_engineering_suite::test_01_white_noise_full_band` (`-4.74 dB` room stage energy drift under H15 calibrated `1/r` distance attenuation vs legacy `<= 0.5 dB` unity check).
    2. `audio_engineering_suite::test_03_sine_sweep_20hz_to_20khz` (`min=-28.35` vs `-28.0` threshold under same H15 attenuation shift).
    3. `solo_instrument_verification::known_geometry_ir_and_independent_snr_estimator_fixtures` (`23.5 dB < 28 dB` synthetic melody SNR fixture).
    4. `technical_report_sweep::mandatory_10s_sine_sweep_before_after_report_and_export` (`485571 != 480000` frames after H14 room tail preservation).
  - Pre-existing `spectracraft-desktop` compile mismatches discovered when Linux X11/GTK/ALSA `-dev` libraries were enabled (`rig.validate(...)` 3-arg count, `LooseProfileLibrary.instrument_families`, 10-tuple `post_dry_fingerprint` with `preamp_drive`, and test `DryListeningChecklist` field names) were minimally repaired so the existing `egui` desktop compiles and tests cleanly.

### Toolchain & MSRV Reconciliation
| Component | Declared / Pinned Rust Version | Edition | Notes |
| --- | --- | --- | --- |
| Root workspace (`Cargo.toml`) | `rust-version = "1.89"` | 2021 | Core `spectracraft-mono` does NOT assume Zed's toolchain as its MSRV. |
| Pinned `craft-libs` (`05b99ee5dcd64f2c954a693ef361301214ddc3c7`) | `craft-atomic` inherits `1.95` | 2021 | Preserved unchanged behind `report_export.rs` and `settings_store.rs`. |
| Experimental GPUI (`apps/spectracraft-gpui/rust-toolchain.toml`) | `channel = "1.100.0"` | 2024 | Matches pinned Zed snapshot `ed8dc1b6ca115f3ce70117ebb448dfb61ea030bf` (`gpui 0.2.2`, `gpui_platform 0.1.0`, Apache-2.0). |

### Dependency Isolation
- Root `Cargo.toml` excludes `"apps/spectracraft-gpui"`.
- Root `Cargo.lock` contains **zero** `gpui` / `gpui_platform` dependencies.
- `apps/spectracraft-gpui/` is a standalone workspace with its own committed `apps/spectracraft-gpui/Cargo.lock` and `apps/spectracraft-gpui/rust-toolchain.toml`.
- `spectracraft-mono/Cargo.toml` splits `playback = ["dep:rodio"]` and `dialogs = ["dep:rfd"]` out of `desktop = ["dep:eframe", "playback", "dialogs"]`, keeping `desktop` 100% backward-compatible while allowing `apps/spectracraft-gpui` to reuse `AudioPlayback` and native file dialogs without pulling `eframe`/`egui`.

---

## 2. Architecture & Non-Negotiable Boundaries

```
spectracraft-mono (framework-neutral Rust engine)
├── src/app_session.rs      # Authoritative AppSession, immutable Arc<AudioArtifact>,
│                           # SessionEpoch, CompletionGuard, DryReviewRecord (SC-GOV-04),
│                           # actual-audio Undo/Redo, atomic export & preset save/load
├── src/app_jobs.rs         # Bounded worker pool (1..=4 threads, bounded queue),
│                           # CancellationToken, preview coalescing, explicit shutdown/join
├── src/visualization.rs    # Canonical Peaks, to_dbfs, Spectrum, Visual, and bounded LRU
│                           # SpectrogramTileCache (zero UI framework types)
├── src/platform.rs         # Fine-grained features: playback (rodio), dialogs (rfd), desktop (eframe)
├── src/report_export.rs    # Pinned craft-pdf / craft-atomic isolation layer (unchanged)
└── src/settings_store.rs   # Pinned craft-settings isolation layer (unchanged)

apps/spectracraft-gpui (isolated experimental GPUI workspace)
├── src/main.rs             # gpui_platform::application() bootstrap + window options
├── src/app.rs              # SpectraCraftGpuiApp entity, keyboard actions, WeakEntity worker wakeups
├── src/bridge.rs           # GpuiJobBridge wrapping JobRunner + 64-tile / 32 MiB SpectrogramTileCache
├── src/transport.rs        # TransportController wrapping rodio AudioPlayback (clock owned by audio)
├── src/audio_canvas.rs     # Custom GPUI canvas: cached RenderImage tiles + peaks + selection + playhead
└── src/inspector.rs        # Five-stage inspector, SC-GOV-04 review checklist, atomic export & presets
```

### Enforced Invariants
1. **Single-source five-stage order**: `Stage 1 Dry -> Stage 2 Surgical (dry only) -> Stage 3 Mic -> Stage 4 Room -> Stage 5 Plugin`.
2. **Source-free synthesis**: Stages 1–5 never open or blend original PCM; bypassing Mic/Room/Plugin never substitutes original PCM (verified bit-identical to dry when all bypassed).
3. **Immutable `Arc<AudioArtifact>` provenance**: Every buffer carries `id`, `origin`, `sample_rate`, `channels`, `frames`, `sha256`, `parent_id`, and `config_digest`.
4. **Validated publication boundary**: Every async completion carries a `CompletionGuard` (`session_id`, `epoch`, `request_seq`, `expected_parent_id`, `config_digest`) validated before mutating `AppSession`.
5. **Thread discipline**: All DSP, FFT, WAV decoding, hashing, and disk writes run in `JobRunner` off UI callbacks; GPUI and blocking I/O never enter audio render callbacks.
6. **Revision-bound listening review (`SC-GOV-04`)**: Approval binds to exact `dry.sha256` and clears on source swap, dry regeneration, Stage 2 edit, or settings-only preset load.
7. **Display isolation**: `DisplayState` (`take`, `view_range`, `display_max_hz`, `scale`) is isolated from audio buffers, `SessionEpoch`, and transport clock.
8. **Honest persistence & export**: Settings-only JSON saves are labeled `SaveKind::SettingsPreset`; stale post-dry captures are blocked from `ExportTarget::CurrentCapture`; overwriting the original source WAV is refused.

---

## 3. Explicit Performance Budgets (Defined Before Measurement)

| Metric | Target Budget | Measurement Status |
| --- | --- | --- |
| Playhead-only redraw FFT executions | **0 FFTs** (100% tile cache hit) | **VERIFIED** in headless unit tests (`tile_cache_does_zero_ffts_on_playhead_redraw_and_enforces_lru_eviction`) |
| Bounded spectrogram tile cache memory | **<= 64 tiles / <= 32 MiB** | **VERIFIED** structurally & in LRU eviction unit test |
| Background worker threads | **2 workers (clamped 1..=4)**, joined on Drop/shutdown | **VERIFIED** in `runner_bounds_concurrency_coalesces_previews_and_shuts_down_cleanly` |
| Bounded pending job queue | **<= 16 pending**, preview coalescing on `PostDry` & `OverviewVisual` | **VERIFIED** in `app_session_jobs` integration test |
| Live GPU frame time / 60 fps scrolling & scrubbing | **<= 16.6 ms p95** | **NOT RUN** (headless container lacks GPU/display server) |
| Live OS audio output latency / underruns | **0 audible glitches** | **NOT RUN** (headless container lacks audio hardware) |

---

## 4. Verification Ledger (Executed Results — Linux x86_64 Headless Container)

**Toolchain**: `rustc 1.100.0 (0ef1bc386 2026-11-09)`, `cargo 1.100.0`, `rustfmt`, `clippy`, `node v22.23.5`, `tsc`, `vite`.
**Lockfile Isolation Audit**:
- Root `Cargo.lock`: **425 packages**, contains `gpui` / `gpui_platform`: **False**.
- `apps/spectracraft-gpui/Cargo.lock`: **612 packages**, `gpui = 0.2.2`, `gpui_platform = 0.1.0` (pinned Zed rev `ed8dc1b6ca115f3ce70117ebb448dfb61ea030bf`).
- Shared crate parity (root vs GPUI lockfile): `spectracraft-mono 0.1.0`, `serde 1.0.228`, `serde_json 1.0.150`, `rustfft 6.4.1`, `hound 3.5.1`, `rodio 0.20.1`, `rfd 0.15.4`, `image 0.25.10`, `smallvec 1.16.6`, `craft-atomic 0.1.0`, `craft-settings 0.1.0` — **100% version parity**.

| Verification Target | Exact Command | Result |
| --- | --- | --- |
| Core lib (no desktop/GPUI features) | `cargo check --locked -p spectracraft-mono --lib --no-default-features` | **PASS** |
| Core lib (`playback` without `eframe`/`egui`) | `cargo check --locked -p spectracraft-mono --lib --features playback` | **PASS** |
| Core unit tests (incl. `visualization::`, `daw_bridge::`, `settings_store::`, `update_manager::`, `pipeline::`) | `cargo test --locked -p spectracraft-mono --lib` | **PASS (122/122)** |
| Shared session & job boundary integration tests | `cargo test --locked -p spectracraft-mono --test app_session_jobs` | **PASS (4/4)** |
| Existing `egui` desktop preservation (`spectracraft-desktop`) | `cargo test --locked -p spectracraft-mono --features desktop --bin spectracraft-desktop` | **PASS (12/12)** |
| Experimental GPUI locked check | `cargo check --locked --manifest-path apps/spectracraft-gpui/Cargo.toml` | **PASS** |
| Experimental GPUI headless tests (`#[gpui::test]` + canvas) | `cargo test --locked --manifest-path apps/spectracraft-gpui/Cargo.toml` | **PASS (2/2)** |
| Experimental GPUI strict Clippy | `cargo clippy --locked --manifest-path apps/spectracraft-gpui/Cargo.toml --all-targets -- -D warnings` | **PASS (0 warnings)** |
| Experimental GPUI formatting | `cargo fmt --manifest-path apps/spectracraft-gpui/Cargo.toml -- --check` | **PASS** |
| TypeScript strict unused check & unit tests | `npx tsc --noEmit --noUnusedLocals --noUnusedParameters && node --import tsx --test src/lib/*.test.ts` | **PASS (78/78)** |
| Source integrity manifest | `npm run verify:manifest` | **PASS (65/65)** |
| Live GPU / Wayland / X11 interactive GPUI window | *(requires physical display server + GPU driver)* | **NOT RUN** |
| Live OS audio output hardware audition | *(requires physical ALSA/CoreAudio/WASAPI audio device)* | **NOT RUN** |
| macOS & Windows native GPUI / `egui` builds | *(covered by `.github/workflows/gpui-spike.yml` on GitHub Actions)* | **NOT RUN** |
| Pre-existing baseline integration failures (4 tests in `audio_engineering_suite`, `solo_instrument_verification`, `technical_report_sweep`) | Recorded transparently in Section 1; NOT introduced by GPUI work and NOT suppressed | **FAIL (pre-existing baseline)** |

---

## 5. Reversible Rollback Instructions

Each phase is cleanly reversible without affecting core DSP, `spectracraft-desktop`, CLI, Express bridge, or Tauri:

- **Remove experimental GPUI frontend completely (Phases 1, 3, 4, 5)**:
  1. Delete `apps/spectracraft-gpui/`, `.github/workflows/gpui-spike.yml`, and `docs/GPUI_INTEGRATION.md`.
  2. Remove `"apps/spectracraft-gpui"` from `exclude` in root `Cargo.toml`.
  3. Root `Cargo.lock` requires zero changes because GPUI was never added to it.
- **Keep or revert shared Rust boundary (Phase 2 & visualization extraction)**:
  - `spectracraft-mono/src/{app_session,app_jobs,visualization}.rs` and `spectracraft-mono/tests/app_session_jobs.rs` are pure framework-neutral Rust with zero GPUI dependencies and improve `spectracraft-desktop` directly; revert only if removing shared session/visualization abstractions is explicitly desired.
  - Reverting `playback`/`dialogs` feature split in `spectracraft-mono/Cargo.toml` and `platform.rs` restores `desktop = ["dep:eframe", "dep:rfd", "dep:rodio"]` with zero lockfile impact.
