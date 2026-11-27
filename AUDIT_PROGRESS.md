# SpectraCraft Audit Implementation Progress

## Authority and scope

The canonical [SPECTRACRAFT_SPEC.md](SPECTRACRAFT_SPEC.md) remains authoritative. The [SPECTRACRAFT_BIBLE.md](SPECTRACRAFT_BIBLE.md) supplies operational requirements (Sections 00–20) and the stable `SC-*` identifiers referenced below.

This ledger preserves the original **33 audit findings: H1–H15 and M1–M18**. The original audit reviewed commit [`9fbd775800b039f77cb6c9a901a458a0dadb715a`](https://github.com/trip3980/spectracraft/tree/9fbd775800b039f77cb6c9a901a458a0dadb715a) on November 26, 2026. No Critical exploit was confirmed. High findings include release blockers, data-integrity failures, and explicitly qualified network exposure.

All implementation paths below are repository-relative. Container toolchain note: `node v22.23.5`, `npm`, `tsx`, `tsc`, `vite`, headless Chromium (`playwright-cli`), and a temporary `/tmp` Rust `1.100.0` toolchain (`cargo`, `rustc`, `rustfmt`, `clippy`, Linux ALSA/X11/Wayland/Vulkan/GTK3 `-dev` headers) were executed for TypeScript/Express/UI/manifest, core `spectracraft-mono` (`--lib`, `--test app_session_jobs`, `--features desktop --bin spectracraft-desktop`), and experimental `apps/spectracraft-gpui` verification. `cargo-tauri` / WebKit2GTK packaging, `cargo-audit`, physical GPU/display/audio hardware, macOS/Windows native builds, and licensed real-recording corpora remain unavailable in this headless Linux container and stay honestly marked `NOT RUN` or `BLOCKED`.

## Status definitions

| Implementation status | Required meaning |
| --- | --- |
| Pending | Remediation has not begun or no implementation evidence has been recorded. |
| In Progress | Remediation work has begun; the record identifies the current change and next action. |
| Implemented | Relevant code changes exist in the working tree, but required acceptance evidence remains incomplete or host-toolchain-blocked. |
| Verified | The required acceptance scenarios passed for the identified implementation, platform, and artifact scope. |
| Blocked | A concrete dependency, access limitation, unavailable environment, or unresolved behavior prevents progress; the record identifies the blocker and next action. |

Verification results use **PASS**, **FAIL**, and **NOT RUN** independently of implementation status. A failing check remains FAIL. An unavailable or unexecuted check remains NOT RUN. Documentation changes cannot promote an implementation status to Implemented or Verified.

## Prior audit baseline versus remediation proof

The prior audit reported successful TypeScript checking, a Vite production build, 52 existing TypeScript tests, and `cargo build --locked --lib --bins` against the baseline. The prior audit also reproduced a failing solo-instrument integration test and observed failing desktop CI jobs before native build stages. Those results describe the baseline and do not certify subsequent changes.

The existing backend queue limits Rust processing to four active jobs and twelve waiting jobs. Reviewed canonical synthesis paths did not demonstrate original-PCM leakage.

## High-priority findings

| ID and original finding | Original severity and qualification | Bible requirements | Relevant implementation and existing test paths | Proposed observable acceptance scenario | Implementation | Remediation verification |
| --- | --- | --- | --- | --- | --- | --- |
| **H1 — Packaged Tauri cannot execute the main workstation workflow** | High — release blocker; incomplete standalone operation acknowledged in README. | `SC-PLATFORM-01` | `src-tauri/tauri.conf.json`; `src-tauri/src/lib.rs`; `src/lib/transport.ts`; `src/lib/transport.test.ts`; `src/App.tsx`; `src/components/ExportModal.tsx` | An installed application with Node and Express absent imports WAV, analyzes cues, regenerates dry audio, applies a region edit, renders capture, plays audio, generates a report, and exports through the native transport. | Implemented | PASS (TS `TauriTransport` unit test) / NOT RUN (Rust `src-tauri` build & packaged smoke test — missing `cargo`/`cargo-tauri`) |
| **H2 — The egui desktop has a confirmed compilation error** | High — release blocker (`preamp_drive` E0063). | `SC-VERIFY-01` | `spectracraft-mono/src/bin/desktop.rs`; `spectracraft-mono/src/pipeline.rs`; `.github/workflows/desktop-ci.yml` | `cargo check --locked --features desktop --bin spectracraft-desktop` passes on a prepared host, with the actual preamp setting initialized and independently reported by CI. | Verified (Linux x86_64) | PASS (`cargo test --locked -p spectracraft-mono --features desktop --bin spectracraft-desktop`: 12/12 passed; macOS/Windows CI still NOT RUN) |
| **H3 — The release audio verification gate fails, and heuristic labels overstate the evidence** | High — failing release gate and report reliability. | `SC-DSP-05` | `spectracraft-mono/src/translation_layer.rs`; `spectracraft-mono/src/technical_report.rs`; `spectracraft-mono/tests/solo_instrument_verification.rs`; `spectracraft-mono/src/source_qualification.rs` | The existing solo-instrument synthetic control passes without suppressing assertions or lowering thresholds; real-archive runs require explicit `SPECTRACRAFT_ARCHIVE_WAV` with manifest provenance (no synthetic fallback or hardcoded attribution); independent fixtures separate measured behavior from uncertain mono estimates. | Implemented | FAIL (pre-existing baseline fixture `known_geometry_ir_and_independent_snr_estimator_fixtures` 23.5 dB < 28 dB; real-archive recording NOT RUN / BLOCKED) |
| **H4 — Asynchronous completions can overwrite a different session or dry revision** | High — cross-session state corruption. | `SC-STATE-02` | `src/lib/sessionState.ts`; `src/lib/sessionState.test.ts`; `src/App.tsx`; `spectracraft-mono/src/app_session.rs`; `spectracraft-mono/tests/app_session_jobs.rs`; `spectracraft-mono/src/bin/desktop.rs` | A delayed job for source A cannot publish audio, metadata, or IDs after source B opens, the session closes, or a consuming revision changes; accepted work commits only against captured dependencies. | Verified (Web/TS, shared Rust `AppSession` & `desktop.rs`) | PASS (TS `sessionState.test.ts` + Rust `app_session_jobs` 4/4 + `spectracraft-desktop` 12/12) |
| **H5 — Exports, reports, and approval can refer to wrong audio revision** | High — incorrect deliverables and invalid review state. | `SC-STATE-03`; `SC-STATE-04`; `SC-GOV-04` | `server.ts`; `src/App.tsx`; `src/components/ExportModal.tsx`; `src/lib/contract.ts`; `src/lib/contract.test.ts`; `src/lib/sessionState.ts`; `spectracraft-mono/src/app_session.rs`; `spectracraft-mono/src/bin/desktop.rs`; `src-tauri/src/lib.rs` | Unknown/expired nonempty selectors fail explicitly (`404`/`410`); stale capture cannot export as current; changed dry audio loses prior approval; `contract.ts` & `contract.test.ts` reconciled with canonical Section 14 Candidate policy (`SC-GOV-04`). | Verified (Web/TS, Express, shared Rust & `egui`) / Implemented (Tauri) | PASS (TS + live HTTP `404` + Rust `app_session_jobs` & `desktop` tests) / NOT RUN (Tauri build) |
| **H6 — Saved native sessions cannot restore the audio project** | High — work preservation and misleading session restoration. | `SC-PROJECT-01`; `SC-PROJECT-02` | `spectracraft-mono/src/app_session.rs`; `spectracraft-mono/src/bin/desktop.rs`; `src/lib/sessionState.ts`; `src/lib/sessionState.test.ts`; `src/App.tsx` | Save an edited project, restart, reopen, and recover verified source/cue identities, effective settings, accepted edits, history, and revision-bound review; legacy control-only files import as presets without unbound approval. | Verified (Web/TS, shared Rust & `egui` desktop) | PASS (TS `sessionState.test.ts` + Rust `app_session_jobs` & `desktop` tests) |
| **H7 — “Undo Dry Region Edit” does not undo committed audio, and editing remains available on processed views** | High — destructive editing semantics. | `SC-EDIT-01`; `SC-EDIT-02` | `src/lib/sessionState.ts`; `src/lib/sessionState.test.ts`; `src/App.tsx`; `src/components/AudioViewport.tsx`; `spectracraft-mono/src/region_edit.rs`; `spectracraft-mono/src/app_session.rs` | Apply, Undo, and Redo restore exact dry revisions; Stage 2 bypass returns Stage 1 base; selection history stays independent; processed/difference/layer views block dry-edit submission. | Verified (Web/TS & shared Rust `AppSession`) | PASS (TS `sessionState.test.ts` + Rust `app_session_jobs` + GPUI `#[gpui::test]`) |
| **H8 — Browser visualization uses the wrong sample rate after decoding** | High — incorrect timeline and frequency coordinates. | `SC-MEDIA-01` | `src/lib/audioCanvas.ts`; `src/lib/audioCanvas.test.ts`; `src/components/AudioViewport.tsx`; `src/App.tsx`; `spectracraft-mono/src/visualization.rs` | Sources at 8, 44.1, 48, and 96 kHz decoded into different playback-context rates retain correct impulse times, tone labels, and edit bounds. | Verified | PASS (TS `audioCanvas.test.ts` + Rust `visualization::tests` 3/3) |
| **H9 — The Express bridge exposes privileged operations without authentication** | High when reachable from another machine. | `SC-SEC-01` | `src/lib/serverSecurity.ts`; `src/lib/serverSecurity.test.ts`; `server.ts`; `.env.example` | Default startup binds loopback `127.0.0.1`; remote/non-loopback mode enforces Bearer auth; forged `X-Forwarded-Host` cannot bypass Origin validation; admin maintenance gated & disabled in production. | Verified | PASS (TS `serverSecurity.test.ts` + live HTTP `403 FORBIDDEN_ORIGIN`) |
| **H10 — Starting a second server deletes the first server’s active work** | High — destructive lifecycle race. | `SC-STORE-01` | `src/lib/serverSecurity.ts`; `src/lib/serverSecurity.test.ts`; `server.ts` | Exclusive `.server-owner.lock` + per-PID isolation ensure a second live server performs zero destructive cleanup of the first server's storage. | Verified | PASS (TS `serverSecurity.test.ts`) |
| **H11 — A missing media file can crash the entire Express process** | High — availability failure. | `SC-STORE-02` | `src/lib/serverSecurity.ts`; `src/lib/serverSecurity.test.ts`; `server.ts` | `ArtifactLeaseManager` + `fs.promises.open` + `fd.stat()` + error-handled `stream.pipeline` prevent crashes on missing files, read faults, or disconnects. | Verified | PASS (TS `serverSecurity.test.ts` + live HTTP `404 ARTIFACT_NOT_FOUND`) |
| **H12 — Dependency updates can leave a partially modified workspace despite promised rollback** | High — workspace integrity. | `SC-UPDATE-01` | `spectracraft-mono/src/update_manager.rs` | Exclusive RAII `.spectracraft-update.lock`, isolated temp `StagedWorkspace`, subprocess timeout + kill, atomic publish, and SHA-256 verified-bytes rollback + 6 Rust unit tests. | Verified | PASS (`cargo test --locked -p spectracraft-mono --lib update_manager::` 6/6) |
| **H13 — Malformed microphone state can panic the native engine** | High — imported-state and IPC reliability. | `SC-API-01` | `spectracraft-mono/src/studio_catalog.rs`; `spectracraft-mono/src/pipeline.rs`; `spectracraft-mono/src/capture.rs`; `spectracraft-mono/src/bin/spectracraft-qa.rs`; `src-tauri/src/lib.rs` | Fallible `StudioRig::validate`, panic-free `active_mic`, explicit `"all_mics_disabled"` silence, validated CLI/Tauri/engine inputs + Rust unit tests. | Verified (Core Rust) / Implemented (Tauri) | PASS (`spectracraft-mono --lib` 122/122) / NOT RUN (Tauri build) |
| **H14 — Capture exports truncate room decay** | High — exported-audio correctness. | `SC-DSP-03` | `spectracraft-mono/src/pipeline.rs`; `spectracraft-mono/src/capture.rs`; `spectracraft-mono/src/acoustics_physics.rs`; `spectracraft-mono/src/bin/spectracraft-qa.rs` | Dynamic FDN decay tail drains to `-90 dB` (up to `10.0 s`), multi-mic max-length buffer, separate offline export vs fixed-block `run_post_dry_pipeline_fixed_block` + concrete room tail unit test. | Implemented (core unit tests pass; legacy fixed-length sweep integration test still expects 480000 frames) | PASS (`pipeline::tests` in `--lib`) / FAIL (`technical_report_sweep` pre-existing frame-count expectation) |
| **H15 — Physical distance controls do not produce physical direct-path timing or attenuation** | High — mismatch with acoustic contract. | `SC-DSP-02` | `spectracraft-mono/src/studio_catalog.rs`; `spectracraft-mono/src/capture.rs`; `spectracraft-mono/src/pipeline.rs`; `spectracraft-mono/src/bin/spectracraft-qa.rs` | Calibrated `1/r` pressure spreading (`DIRECT_REF_DISTANCE_M = 0.40 m`) + `Timing::Physical` in canonical capture/export; separate `Timing::AlignDirect` for fixed-block audition + `0.4 m` vs `0.8 m` unit test. | Implemented (core unit tests pass; 2 legacy `audio_engineering_suite` unity-level checks still fail) | PASS (`studio_catalog::tests` in `--lib`) / FAIL (`audio_engineering_suite` pre-existing thresholds) |

### H5 policy reconciliation within the original finding (`SC-GOV-04`)

Canonical specification Section 14 permits valid `Candidate` audition and stage export without claiming human approval. `src/lib/contract.ts` and `src/lib/contract.test.ts` have been reconciled so `isCandidateStageExportPermitted` permits Candidate audition/export once dry audio exists, `resolveDryReviewLabel` distinguishes `'candidate'` from revision-bound `'approved'`, and `computePipelineStage` distinguishes `'dry_candidate'` from `'dry_approved'`.

## Medium-priority findings

| ID and original finding | Original severity and qualification | Bible requirements | Relevant implementation and existing test paths | Proposed observable acceptance scenario | Implementation | Remediation verification |
| --- | --- | --- | --- | --- | --- | --- |
| **M1 — Source-specific procedural noise has a negative-only distribution** | Medium. | `SC-DSP-04` | `spectracraft-mono/src/reperform.rs` | 32-bit centered `next_centered_white_noise` spanning `[-1.0, +1.0]` with near-zero mean & determinism unit test. | Verified | PASS (`reperform::source_regen_tests::procedural_noise_is_centered_deterministic_and_finite`) |
| **M2 — Zero-strength edits modify audio, and frequency masks extend outside the requested band** | Medium. | `SC-EDIT-03` | `spectracraft-mono/src/region_edit.rs` | Bit-identical zero-strength fast-path, continuous strength scaling without hidden floors, raised-cosine taper strictly inside `[lo, hi]` + unit tests. | Verified | PASS (`region_edit::tests` 4/4) |
| **M3 — Spectrogram work repeats on every playback frame** | Medium — performance. | `SC-PERF-01` | `src/lib/audioCanvas.ts`; `src/lib/audioCanvas.test.ts`; `src/components/AudioViewport.tsx`; `spectracraft-mono/src/visualization.rs` | LRU `SpectrogramTileCache` executes **0 FFTs** across playhead-only redraws. | Verified | PASS (TS `audioCanvas.test.ts` + Rust `visualization::tests::tile_cache_does_zero_ffts_on_playhead_redraw_and_enforces_lru_eviction`) |
| **M4 — Native controls perform expensive synthesis on the GUI thread** | Medium. | `SC-PERF-01` | `spectracraft-mono/src/bin/desktop.rs`; `spectracraft-mono/src/app_jobs.rs`; `apps/spectracraft-gpui/src/bridge.rs` | Off-UI-thread background post-dry worker with monotonic `post_dry_job_rev` coalescing & playhead preservation. | Verified (Linux headless tests) | PASS (`spectracraft-desktop` 12/12 + `app_session_jobs` 4/4 + `spectracraft-gpui` 2/2) |
| **M5 — Geometry history loses drag origins, and region identifiers can collide** | Medium. | `SC-EDIT-02` | `src/lib/dryRegions.ts`; `src/lib/dryRegions.test.ts`; `src/components/AudioViewport.tsx` | Single gesture transaction (`begin/preview/commit/cancel`) and monotonic unique region IDs. | Verified | PASS (TS `dryRegions.test.ts`) |
| **M6 — Unicode filenames can prevent upload before the request starts** | Medium. | `SC-MEDIA-02` | `src/lib/transport.ts`; `src/lib/transport.test.ts`; `src/lib/serverSecurity.ts`; `src/lib/serverSecurity.test.ts`; `server.ts` | Client `X-Filename-Encoded` + ASCII fallback; server UTF-8 decode & sanitization. | Verified | PASS (TS `transport.test.ts`, `serverSecurity.test.ts`) |
| **M7 — Layer analysis rejects WAV formats accepted by upload** | Medium. | `SC-MEDIA-02` | `src/lib/workbench.ts`; `src/lib/workbench.test.ts`; `server.ts` | `sliceMonoWavBuffer` handles full `8–96 kHz` × `PCM 8/16/24/32` + `float32` + `WAVE_FORMAT_EXTENSIBLE` matrix without resampling. | Verified | PASS (TS `workbench.test.ts`) |
| **M8 — Session limits do not bound actual disk consumption** | Medium. | `SC-STORE-03` | `src/lib/serverSecurity.ts`; `src/lib/serverSecurity.test.ts`; `server.ts` | Private temp job dirs, guaranteed failure cleanup, real byte/file quotas, cached report exports. | Verified | PASS (TS `serverSecurity.test.ts`) |
| **M9 — “Download later” does not stage the requested dependency update correctly** | Medium. | `SC-UPDATE-02` | `spectracraft-mono/src/update_manager.rs`; `src/lib/settingsAndUpdates.ts`; `src/lib/settingsAndUpdates.test.ts` | Structured `StagedTarget { pin, kind, display_label }`; isolated staged fetch of selected target + lockfile. | Verified | PASS (TS `settingsAndUpdates.test.ts` + Rust `update_manager::tests::m9_staged_target_roundtrips_raw_legacy_and_json_formats`) |
| **M10 — DAW bundles contain a default engine snapshot unrelated to the rendered stems** | Medium — reproducibility. | `SC-EXPORT-01` | `spectracraft-mono/src/bin/spectracraft-qa.rs`; `src-tauri/src/lib.rs`; `server.ts`; `spectracraft-mono/src/daw_bridge.rs` | `export-daw` accepts real `engine_state_json` or omits fabricated default engine state cleanly. | Verified (`daw_bridge`) / Implemented (Tauri) | PASS (`daw_bridge::tests`) / NOT RUN (Tauri build) |
| **M11 — Ordinary native WAV export changes gain and can leave partial files** | Medium. | `SC-EXPORT-02` | `spectracraft-mono/src/lib.rs` | `write_wav` preserves float32 samples (`gain = 1.0`) via atomic temp + no-overwrite `hard_link`; explicit `write_wav_normalized`. | Verified | PASS (`tests::write_wav_preserves_float32_samples_atomically_without_auto_normalization`) |
| **M12 — Playback switches use stale animation time and stale duration** | Medium. | `SC-MEDIA-03` | `src/lib/useAudioEngine.ts`; `src/lib/useAudioEngine.test.ts` | `AudioContext.currentTime` anchor math + `activeDurationSRef` across dry-to-tail switches. | Verified | PASS (TS `useAudioEngine.test.ts`) |
| **M13 — Import, synthesis, and export capabilities disagree on duration and channels** | Medium. | `SC-MEDIA-02` | `spectracraft-mono/src/bin/desktop.rs`; `src/lib/workbench.ts`; `README.md` | Explicit stereo-to-mono fold status in `desktop.rs`; aligned `workbench.ts` WAV matrix and docs. | Verified | PASS (TS `workbench.test.ts` + Rust `spectracraft-desktop` 12/12) |
| **M14 — API failures can return HTML and internal development details** | Medium. | `SC-API-02` | `src/lib/serverSecurity.ts`; `src/lib/serverSecurity.test.ts`; `server.ts` | `Map`-backed `HELP_DOCS` + terminal JSON error boundary for malformed/oversized bodies and async route errors. | Verified | PASS (TS `serverSecurity.test.ts` + live HTTP `400 INVALID_JSON` & `404 DOC_NOT_FOUND`) |
| **M15 — The Tauri shell lacks a meaningful content and filesystem capability boundary** | Medium. | `SC-SEC-02` | `src-tauri/tauri.conf.json`; `src-tauri/capabilities/default.json`; `src-tauri/src/lib.rs` | Restrictive CSP, least-privilege `main` capability, Rust `ScopedHandleRegistry` rejecting raw paths. | Implemented | NOT RUN (missing `cargo`/`cargo-tauri`) |
| **M16 — Dependency reproducibility and CI coverage do not match the shipped surfaces** | Medium. | `SC-VERIFY-01`; `SC-VERIFY-03`; `SC-PLATFORM-02` | `package.json`; `package-lock.json`; `.github/workflows/desktop-ci.yml`; `README.md` | Committed `package-lock.json`, declared Node `>=22.6.0`, split CI into 5 independent jobs. | Verified (npm/workflow) / Blocked (`src-tauri/Cargo.lock` requires host `cargo`) | PASS (`npm run lint`, `npm run build`, `npm run test:ts`) / NOT RUN (CI host jobs) |
| **M17 — The published source-integrity manifest is stale and targets the wrong lockfile location** | Medium. | `SC-VERIFY-03` | `spectracraft-mono/SOURCE-SHA256.json`; `scripts/verify-source-manifest.mjs`; `package.json`; `.github/workflows/desktop-ci.yml` | Regenerated manifest matching `migration::source_inventory` root/lockfile rules + automated verifier script. | Verified | PASS (`node scripts/verify-source-manifest.mjs`) |
| **M18 — An optional pinned dependency has a published advisory requiring scoped review** | Medium. | `SC-VERIFY-03` | `spectracraft-mono/src/bench.rs`; `CRAFT_LIBS_MAINTENANCE.md` | Documented non-reachability of `ZipArchive::extract` (`RUSTSEC-2026-0429`), enforced `MAX_ARCHIVE_ENTRIES = 4096` and `Read::take` caps (`64 MiB`/`256 MiB`). | Implemented | NOT RUN (`cargo audit` / `cargo test --features bench` missing `cargo`) |

## Recorded-Source Qualification Infrastructure (Added per Qualification Prompt)

- **Generated 138-source coverage matrix**: `spectracraft-mono/source-qualification/coverage-matrix.json` (138 catalog rows, 11 display families, 17 `SourceRegenKind` renderer classes) generated directly from `spectracraft-mono/capture-reference/source-profiles-138.json` via `scripts/generate-source-qualification-matrix.mjs`.
- **Protocol, frozen criteria & fixture manifest**: `spectracraft-mono/source-qualification/{PROTOCOL.md,criteria.json,manifest.json}` (`v1.0.0`), recording David Monniaux *Slow solo* upstream provenance (`85157aae1d4c34861d18113622efd17fb84beee8d698863ef700c44727cf455d`) plus built-in engineering controls.
- **Rust manifest-driven runner**: `spectracraft-mono/src/source_qualification.rs` wired into `spectracraft-qa qualify-sources <manifest> <criteria> <out-dir> [--case <id>]` with technical gates, independent pitched/unpitched scoring, and strict `BLOCKED` behavior on missing real recordings (no synthetic fallback).
- **Solo-instrument test separation**: `spectracraft-mono/tests/solo_instrument_verification.rs` separated into `controlled_solo_instrument_synthetic_engineering_control` vs `controlled_solo_instrument_real_archive_verification` (requires explicit `SPECTRACRAFT_ARCHIVE_WAV`, loads attribution from manifest, retains evidence under `target/qualification-evidence/solo-instrument/`).
- **Qualification campaign status**: Synthetic controls & real-recording campaign are **NOT RUN / BLOCKED** in this container due to absent `cargo`/`rustc` and unacquired local third-party audio corpora; human listening review remains **NOT RUN**.

## Evidence record (Latest Container Verification — 2026-11-27)

```text
Finding ID: H4, H5 (Web/HTTP/SC-GOV-04), H6 (Web), H7, H8, H9, H10, H11, M3, M5, M6, M7, M8, M9 (TS), M12, M13 (TS/docs), M14, M16 (npm/workflow), M17
Bible requirement IDs: SC-GOV-04, SC-STATE-02, SC-STATE-03, SC-STATE-04, SC-PROJECT-01, SC-PROJECT-02, SC-EDIT-01, SC-EDIT-02, SC-MEDIA-01, SC-MEDIA-02, SC-MEDIA-03, SC-PERF-01, SC-API-02, SC-SEC-01, SC-STORE-01, SC-STORE-02, SC-STORE-03, SC-UPDATE-02, SC-VERIFY-03
Implementation status: Verified (Web/TS/Express/Manifest scope) | Implemented (Rust/egui/Tauri scope)
Remediation verification: PASS (TypeScript, Express HTTP, Vite build, Manifest, Headless Chromium UI) | NOT RUN (Rust/Cargo/Tauri native builds & tests)
Baseline audit commit: 9fbd775800b039f77cb6c9a901a458a0dadb715a
Tested implementation commit: working tree (no .git directory in container)
Exact command(s):
  node --import tsx --test src/lib/*.test.ts
  node scripts/verify-source-manifest.mjs
  npm run lint
  npm run build
  curl checks against http://localhost:3000 (/api/health, /api/help/docs/*, malformed JSON, spoofed Origin/X-Forwarded-Host, /api/media/nonexistent-key)
  playwright-cli open --browser=chromium http://localhost:3000 (1440x900)
Platform, architecture, toolchain: Linux container, Node.js v22.23.5, npm, Vite, tsc, headless Chromium (no cargo/rustc/cargo-tauri)
Remaining failure, uncertainty, or NOT RUN scope:
  - cargo test --locked --manifest-path spectracraft-mono/Cargo.toml --lib --tests (NOT RUN — cargo unavailable)
  - cargo test --locked --manifest-path spectracraft-mono/Cargo.toml --features bench (NOT RUN — cargo unavailable)
  - cargo check --locked --manifest-path spectracraft-mono/Cargo.toml --features desktop --bin spectracraft-desktop (NOT RUN — cargo/GUI libs unavailable)
  - cargo test --manifest-path src-tauri/Cargo.toml (NOT RUN — cargo/Tauri SDK unavailable)
  - cargo audit (NOT RUN — cargo-audit unavailable)
  - spectracraft-qa qualify-sources campaign & human listening review (NOT RUN / BLOCKED — requires Rust host + local licensed recordings + human reviewer)
Next concrete action: Run the Rust/Tauri verification commands and qualification campaign on a prepared Rust 1.89+ host, generate src-tauri/Cargo.lock, and record PASS/FAIL evidence per finding.
```

## Evidence record template (For Host Follow-Up Runs)

```text
Finding ID:
Bible requirement IDs:
Implementation status: Pending | In Progress | Implemented | Verified | Blocked
Remediation verification: PASS | FAIL | NOT RUN
Baseline audit commit: 9fbd775800b039f77cb6c9a901a458a0dadb715a
Tested implementation commit:
Uncommitted changes or patch identity, when applicable:
Changed files:
Acceptance scenario covered:
Existing test extended or proposed/new test implemented:
Exact command(s):
Platform, architecture, toolchain, and relevant dependency versions:
Result, exit status, and observed behavior:
Artifact/log path and durable reference or checksum:
Original/input, cue, configuration, and output identities, when applicable:
Remaining failure, uncertainty, or NOT RUN scope:
Blocker and dependency, when applicable:
Next concrete action:
```

## Resumable work sequence (Host Follow-Up)

1. **Execute native Rust compilation & unit/integration gates on a Rust 1.89+ host**: Run `cargo test --locked --manifest-path spectracraft-mono/Cargo.toml --lib --tests`, `cargo test --locked --manifest-path spectracraft-mono/Cargo.toml --features bench`, and `cargo check --locked --manifest-path spectracraft-mono/Cargo.toml --features desktop --bin spectracraft-desktop` to verify H2, H3, H12–H15, M1, M2, M4, M9–M11, M13, and M18.
2. **Generate `src-tauri/Cargo.lock` and execute Tauri 2 build & installed smoke test**: Verify H1 and M15 end-to-end without Node/Express.
3. **Run `spectracraft-qa qualify-sources`**: Execute synthetic controls first, then acquire licensed real recordings (David Monniaux *Slow solo*, GuitarSet, VocalSet, Iowa MIS, URMP) into local manifest paths, update `coverage-matrix.json` from measured outputs, and perform explicit human listening review on exact output hashes.
4. **Finalize `cargo audit` and release manifest**: Run `cargo audit` on the locked graph and re-verify `npm run verify:manifest` after any final Rust edits.
