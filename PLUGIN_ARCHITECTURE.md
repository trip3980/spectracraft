# SpectraCraft Plugin Architecture & Host Entry Points

This document defines the UI-free Rust audio processing boundary in `spectracraft-mono` so the exact five-stage pipeline (`SPECTRACRAFT_SPEC.md`) can later be wrapped as a **VST3**, **AU (Audio Unit)**, or **AAX** plugin without duplicating DSP code or coupling to `egui` / `eframe` / desktop OS adapters.

> **Status**: Engine entry points (`AudioPipelineEngine`, `EngineStateSnapshot`) are implemented in `spectracraft-mono/src/pipeline.rs` with zero UI dependencies. Actual VST3, AU, and AAX wrapper crates are intentionally **not** implemented yet.

---

## 1. Crate & Dependency Isolation

The library crate `spectracraft-mono` compiles **without** `egui`, `eframe`, `rfd`, or `rodio` whenever the `desktop` Cargo feature is disabled (the default). Future plugin wrapper crates (e.g. `spectracraft-vst3`, `spectracraft-au`, `spectracraft-aax`) depend strictly on `spectracraft-mono` with default features:

```toml
[dependencies]
spectracraft-mono = { path = "../spectracraft-mono", default-features = false }
```

This guarantees:
- Zero GUI, windowing, file-dialog, or OS audio-device dependencies inside the DSP engine.
- Identical bit-for-bit DSP math across the CLI (`spectracraft-mono`), desktop workstation (`spectracraft-desktop`), QA suite (`spectracraft-qa`), and future DAW plugin formats.

---

## 2. Authoritative Five-Stage Pipeline Boundary

All wrappers MUST preserve the exact `SPECTRACRAFT_SPEC.md` processing order and invariants:

```text
Reference Audio (read-only shape cue source; NEVER blended or stored)
   │
   ▼
[Stage 1] Dry Regeneration
   AudioPipelineEngine::regenerate_dry_from_reference(&mut self, reference_mono: &[f32]) -> Result<&[f32]>
   AudioPipelineEngine::resynthesize_dry_from_cues(&mut self) -> Result<&[f32]>
   │  (produces cached dry buffer; zero original PCM blended)
   ▼
[Stage 2] Surgical Region Editing (dry take only)
   AudioPipelineEngine::apply_dry_region_edit(&mut self, edit: &DryRegionEdit) -> Result<RegionEditOutput>
   │  (samples outside [start_sample, end_sample] remain bit-identical)
   ▼
[Stage 3] Optional Microphone  (post_dry.mic_enabled, OFF by default)
[Stage 4] Optional Space/Room  (post_dry.room_enabled, OFF by default)
[Stage 5] Optional Plugin Slot (post_dry.plugin_slot, OFF by default)
   AudioPipelineEngine::process_post_dry_buffer(&self, dry_in: &[f32], out_buf: &mut [f32]) -> Result<PipelineStageSummary>
   AudioPipelineEngine::process_cached_dry_into(&self, out_buf: &mut [f32]) -> Result<PipelineStageSummary>
   │
   ▼
Final Output Buffer (out_buf: &mut [f32])
```

For offline / ARA / render-in-place hosts that supply a full clip and edit list in one call:

```rust
AudioPipelineEngine::process_full_pipeline_buffer(
    &mut self,
    reference_mono: &[f32],
    region_edits: &[DryRegionEdit],
    out_buf: &mut [f32],
) -> Result<PipelineStageSummary>
```

---

## 3. Clean Entry Points (`spectracraft_mono::pipeline::AudioPipelineEngine`)

| Host Lifecycle Hook | VST3 / AU / AAX Equivalent | `AudioPipelineEngine` Method |
|---|---|---|
| Construct / instantiate | `createInstance` / `AudioComponentInstanceNew` / `CreateEffect` | `AudioPipelineEngine::new(sample_rate)` or `with_catalogs(...)` |
| Prepare sample rate | `setupProcessing` / `Initialize` / `ResetFieldData` | `engine.prepare(sample_rate)` |
| Stage 1 (capture / ARA analyze) | ARA `notifyAudioSourceContentChanged` / offline capture | `engine.regenerate_dry_from_reference(ref_mono)` |
| Stage 1 (re-voice source) | Parameter change on Source / Voicing | `engine.resynthesize_dry_from_cues()` |
| Stage 2 (surgical dry edit) | Region edit commit from plugin editor | `engine.apply_dry_region_edit(&edit)` |
| Stages 3–5 (buffer in → out) | `process` / `Render` / `ProcessAudio` | `engine.process_post_dry_buffer(dry_in, out_buf)` |
| Stages 3–5 (from cached dry) | ARA / cached-take playback render | `engine.process_cached_dry_into(out_buf)` |
| Save preset / chunk | `getState` / `kAudioUnitProperty_ClassInfo` / `GetChunk` | `engine.export_state_json()` / `engine.snapshot_state()` |
| Restore preset / chunk | `setState` / `kAudioUnitProperty_ClassInfo` / `SetChunk` | `engine.import_state_json(bytes)` / `engine.apply_state(state)` |

### Input/Output Validation Contracts
- `prepare` / `new` validate `sample_rate` in `8_000..=96_000` Hz.
- `process_post_dry_buffer` and `process_full_pipeline_buffer` refuse mismatched `in.len() != out.len()` and reject any non-finite (`NaN` / `±Inf`) input samples with explicit `Err(...)`.
- Bypass guarantee: when `mic_enabled == false`, `room_enabled == false`, and `plugin_slot.enabled == false` (the default), `process_post_dry_buffer` writes a **bit-identical** copy of `dry_in` into `out_buf` (`summary.bit_identical_to_dry == true`, `summary.original_audio_opened == false`).

---

## 4. State Serialization (`EngineStateSnapshot`)

`EngineStateSnapshot` (serialized via `serde_json` with `#[serde(deny_unknown_fields)]`) captures all audio-affecting parameters and **zero UI state**:

```rust
pub struct EngineStateSnapshot {
    pub version: u32,                    // currently 1
    pub voicing: VoicingControls,        // Stage 1 voicing & straightening
    pub post_dry: PostDryPipelineConfig, // Stages 3, 4, 5 (mic, room, materials, plugin slot)
}
```

Display-only workstation settings (dock positions, window sizes, zoom, 3D camera angles, waveform/spectrogram view mode) are deliberately excluded so DAW presets remain portable across headless renders, desktop sessions, and future plugin editors.

---

## 5. Offline vs. Real-Time Considerations for Future Wrappers

1. **Stages 1 & 2 (Dry Regeneration & Surgical Region Editing) are clip-level / ARA-style operations**:
   - Cue extraction (`reperform::analyze_guitar`) and surgical region splicing (`region_edit::apply_dry_region_edit`) operate over an analyzed excerpt buffer rather than sample-by-sample streaming.
   - Future VST3/AU/AAX wrappers should integrate Stages 1–2 via **ARA2** (Audio Random Access) or an explicit host-side capture/commit buffer, running analysis/resynthesis on a background worker thread and publishing the resulting `Arc<[f32]>` dry buffer atomically to the audio thread.
2. **Stages 3–5 (Mic, Room, Plugin Slot) are buffer-in / buffer-out**:
   - `process_post_dry_buffer` already takes `dry_in: &[f32]` and writes `out_buf: &mut [f32]` with deterministic output.
   - Before running Stages 3–5 directly on a real-time DAW audio callback thread (`process()`), a future wrapper should add a stateful streaming wrapper (pre-allocated FIR/IIR filter state, overlap-add room convolution ring buffer, and persistent dynamics envelope state) so zero heap allocations occur inside the audio thread callback. The current `AudioPipelineEngine` defines the exact parameter, bypass, and buffer contract that streaming wrapper must match bit-for-bit on whole-clip renders.

---

## 6. Invariants Any Future Plugin Wrapper Must Preserve

1. **No original sample blending**: Never add a dry/wet mix knob that blends the host's raw input audio into the Stage 1–5 regenerated signal.
2. **No stage coupling**: Toggling Stage 3 (Mic) must never implicitly alter Stage 4 (Room) or Stage 5 (Plugin), and vice versa.
3. **Dry-first**: Stages 3–5 always consume the Stage 1/2 dry signal; bypassing Stages 3–5 returns the Stage 1/2 dry signal unchanged.
4. **Display isolation**: Editor UI actions (resizing plugin window, zooming, orbiting 3D room view, toggling polar overlay) must never dirty DAW automation parameters or alter rendered audio.
