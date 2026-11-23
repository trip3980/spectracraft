# SpectraCraft — Standing Program Bible & Operational Execution Contract

> **Authority & Canonical Reference**:
> This Project Bible is the permanent operational specification for **SpectraCraft** and works directly
> alongside the complete 17-section canonical contract retained at [`SPECTRACRAFT_SPEC.md`](./SPECTRACRAFT_SPEC.md)
> (mirrored in [`spectracraft-mono/SPECTRACRAFT_SPEC.md`](./spectracraft-mono/SPECTRACRAFT_SPEC.md)).
> Whenever any ambiguity arises, `SPECTRACRAFT_SPEC.md` is authoritative and binding for every change to
> the Rust engine, CLI/QA tools, catalogs, native desktop shell, server bridge, and workstation UI.

---

## 00. Ponytail Engineering Discipline (Standing Change Rules)

Before writing any code, stop at the first rung that satisfies the request:

1. **Does this need to exist?** If no, skip it (YAGNI).
2. **Already in this codebase?** Reuse it.
3. **Rust stdlib or existing crate does it?** Use it.
4. **Native platform feature does it?** Use it.
5. **Can it be one clear line or a tiny change?** Write that.
6. **Only then** write the minimum new code that works.

### Never Cut
- Validation and error handling
- The SpectraCraft invariants (dry-first, shape as reference, no original samples blended, stage bypass, display isolation, uncertainty preserved)
- Tests for risky audio or state logic

### Standing Change Hygiene
- Prefer editing existing files over creating new ones.
- Mark any deliberate shortcut with a short comment explaining the upgrade path.
- Keep the 5-stage pipeline and stage boundaries intact.
- End each change with one line stating what was skipped or left unchecked.

---

## 01. Project Identity, Scope & Engine Boundary

- **Product Name**: **SpectraCraft** (`spectracraft-mono` crate, `spectracraft-mono` desktop binary, `spectracraft-qa` CLI/QA binary). All product-owned crates, binaries, modules, docs, titles, commands, and UI labels MUST consistently identify SpectraCraft.
- **Native Rust Core**: All SpectraCraft-owned audio processing, analysis, synthesis, acoustic math, plotting, CLI tools, fixtures, and verification tools MUST remain Rust.
- **No Forbidden Dependencies**: Python, Python wrappers, browser substitutes, and promotional dashboards MUST NOT become application, engine, or verification dependencies. (The React/Express preview layer in this workspace is strictly a thin transport/UI shell over the real compiled `spectracraft-qa` / `spectracraft-mono` Rust binaries.)
- **Scope**: Reconstruct a single-source performance for subsequent audio work. Full-mix separation, universal transcription, historical gear identification, forensic recovery, and full DAW replacement are explicitly out of scope.
- **Build Order**: Maintain a rock-solid runnable core workflow first (import → shape extraction → dry regeneration → original/dry comparison → basic region tools), with optional capture (Mic, Room) and plugin stages cleanly layered on top.

---

## 02. The 9 Non-Negotiable Invariants

1. **Single-source invariant**: One work unit = one imported instrument performance, voice, or stem (chords, repeated attacks, simultaneous partials, and overlapping ringing within that one source remain valid).
2. **Performance-reference invariant**: Extracted `ReferenceShape` is the immutable evidence reference. Explicit correction requests operate only on derived rendering instructions. Source models, catalog ranges, and loose profiles NEVER replace evidence.
3. **Fresh-signal invariant**: Regenerated audio consists exclusively of newly synthesized samples. Original PCM, source phase, sampled attacks, copied residuals, and original waveform fragments MUST NEVER enter any processing-stage output. Original playback and comparison remain isolated monitoring operations.
4. **Dry-first invariant**: Every processed output depends on an available regenerated dry take. Microphone, room, material, and plugin settings MUST NOT participate in foundational dry synthesis.
5. **Surgical-edit invariant**: Every localized correction targets the regenerated dry take BEFORE capture. Original audio and downstream captured audio are NEVER surgical-edit targets.
6. **Independent-bypass invariant**: Every stage has an explicit bypass contract. Bypass preserves stored settings and NEVER substitutes original audio.
7. **Display-isolation invariant**: Display commands (tabs, layer View, contrast, frequency display limits, display precision, units, overlays, projection, camera orbit/pan/zoom/Fit View, layout/docking/reordering, snap toggles) MUST NOT alter audio, performance evidence, effective DSP settings, playback position, or audio-render fingerprints.
8. **Uncertainty invariant**: Supported, Uncertain, OutOfTarget, and Silence remain distinct states. Numerical scores, source expectations, and plausible synthesis MUST NOT convert uncertainty into claims about musical events. Published instrument ranges NEVER invalidate supported out-of-range cues.
9. **State invariant**: Every render uses explicit inputs, immutable parameter snapshots, and dependency revisions. No stage may pull hidden audio inputs or inherit undocumented DSP state.

### Canonical Rust Pipeline & Evidence Types
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    DryRegeneration,
    SurgicalRegionEditing,
    Microphone,
    SpaceRoom,
    PluginSlot,
}

pub const PIPELINE_ORDER: [Stage; 5] = [
    Stage::DryRegeneration,
    Stage::SurgicalRegionEditing,
    Stage::Microphone,
    Stage::SpaceRoom,
    Stage::PluginSlot,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceStatus {
    Supported,
    Uncertain,
    OutOfTarget,
    Silence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRange {
    pub start_frame: u64,
    pub end_frame_exclusive: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalFixKind {
    Straighten,
    Smooth,
    ReduceNoise,
    Stabilize,
}
```

---

## 03. Data Ownership, Timeline & Buffer Contracts

| Artifact | Ownership & Rules |
|---|---|
| `OriginalAsset` | Immutable file identity, full SHA-256 checksum, actual sample rate, channel layout, sample count, duration. Original bytes never mutated; decoding/resampling/channel conversion recorded as separate derivatives with distinct checksums. Never silently downmix, truncate, pad, or relabel a derivative as original. |
| `ReferenceShape` | Serializable source-free evidence (`onsets/offsets`, `pitch/partial trajectories` with bends/slides/vibrato, `amplitude envelopes/accents/repicks`, `spectral envelopes/formants`, `noise-texture params`, `attack/decay/sustain/release`, `overlapping ringing`, `EvidenceStatus` + strength + competing hypotheses). Hash MUST remain invariant across voicing/capture/plugin changes; only explicit reanalysis creates a new revision. |
| `CuePacket` | Editable derived rendering instructions, reference-shape identity, uncertainty, source-model config, deterministic seeds, synthesis params. Strictly excludes original PCM, original phase, copied residuals, and file-access handles. |
| `GeneratedAudio` | Immutable buffers carrying sample rate, channel layout, integer frame count, logical timeline origin, SHA-256 content hash, render revision, and provenance. Publish new buffers on change; never mutate active playback buffers in place. |
| **Timeline** | Committed boundaries use integer sample frames (`SampleRange`). Preserve imported timeline throughout; record physical propagation delay separately from algorithmic latency; record output tails explicitly; at unchanged sample rate, foundational dry output MUST cover the complete reference frame count (no implicit excerpt cap). |

---

## 04. Clean 5-Stage Pipeline Contract

### Stage 1 — Dry Regeneration (Always Available · Cached Foundation)
- **Inputs**: Analysis takes `OriginalAsset` + analysis settings; Synthesis takes `ReferenceShape`, derived cue instructions, source-character settings, implemented source-model parameters, render format, and deterministic seed.
- **Dry Definition**: Intrinsic source behavior only (string ringing, body resonance, supported breath/bow/pick texture, attack character). Zero mic coloration, zero propagation delay, zero room reflections, zero material absorption, zero spatial processing, zero plugins.
- **DDSP-Style Core**: Procedural harmonic/modal oscillator bank (internal continuous phase through slides/bends; repeated excitation without resetting ringing state; preserves supported overlap) + reproducible filtered stochastic noise residual (never copies source residual; never fills unresolved gaps with generic noise).
- **Voicing & Character Controls**:
  - Soft source-physics voicing constraints (cue ALWAYS wins on atypical bends, swells, or out-of-range notes).
  - Character target (`Keep` / `Natural` / `Analog` / `Digital`), source variants (`Standard` / `Rich` / `Basic` / `Warm` / `Bright`), signal path (`Default` / `Mic'd` / `Pickup/Direct` / `Analog Preamp` / `Clean Digital`), bounded visible guidance strength.
  - Character selection MUST NOT silently enable mic, room, or plugins; Analog MUST NOT auto-add hum.
  - Rough character estimate (`Natural` / `Analog` / `Digital` / `Unclear` + loose family/mic/room suggestions with uncertainty) requires explicit user acceptance before altering DSP and never claims exact historical gear identification.
  - Global light straightening (`Off` by default) operates only on derived dry instructions while preserving intentional bends, slides, vibrato, and timing — no MIDI/quantization required.
- **Bypass Contract**: Returns existing valid cached dry buffer for active reference + effective config; if none exists, reports `DryUnavailable` and blocks dependent rendering/export. While bypassed, edited dry controls remain visibly pending; cached metadata continues identifying the settings that actually produced the audible buffer.

### Stage 2 — Surgical Region Editing on the Dry Take (Dry Stage Only)
- **Tools & Ownership**: Rectangular marquee (time × frequency) on dry spectral views + draggable in/out time ranges on dry waveform & spectral views (side-by-side with read-only Original). Move + 8-handle edge/corner resize; multiple independent regions; selection edits mutate metadata ONLY and NEVER commit audio.
- **Fix Requests**: Short user note + bounded `LocalFixKind` (`Straighten`, `Smooth`, `ReduceNoise`, `Stabilize`). Free notes resolve ONLY to supported bounded operations; unsupported descriptions stay visibly unresolved — never run undocumented processing.
- **Scope & Blending**:
  - **Temporal scope**: Commit strictly inside `[start_frame, end_frame_exclusive)`. Short raised-cosine crossfades sit ENTIRELY inside the selected interval (bounded by region length), blending new regional synthesis with generated dry audio only. Samples outside `[start_frame, end_frame_exclusive)` MUST remain **bit-for-bit identical**. Never normalize or reprocess the whole dry take for a patch.
  - **Frequency scope**: Directly modified components/coefficients stay limited to the selected frequency mask; never silently widen a frequency-limited edit into an undisclosed full-band edit (reject unchanged if scope cannot be honored; never claim perfect spectral isolation from a finite temporal window).
- **Acceptance, Overlap, Undo & Upstream Invalidation**:
  - Explicit `Apply` required before replacing committed dry audio; reject stale candidates; accepted bounds are stored independently of mutable selection geometry (moving/deleting selection never relocates or undoes accepted audio).
  - Overlapping edits record acceptance order + parent/output revisions; newer accepted edits cannot be overwritten by older-parent candidates.
  - Independent Undo/Redo histories for selection geometry vs accepted resynthesis.
  - Stage 1 changes invalidate candidate/accepted patch buffers while preserving region definitions & fix requests (never splice old-revision patches into a new dry take).
- **Preview & Downstream Scheduling**:
  - Looped solo preview + same-bounds Before/Candidate A/B + debounced live candidate regeneration while moving/resizing. Dry preview path by default (mic/room only if explicitly enabled for audition); preserves full-take transport position.
  - Selection/candidate edits leave downstream stages valid; accepted dry edits mark downstream stages stale and defer rerender until leaving dry editing or explicit refresh (block exporting stale downstream audio as current).
- **Bypass Contract**: Returns Stage 1 dry audio unchanged while preserving regions, requests, and edit history; empty edit set = identity.

### Stage 3 — Optional Microphone (OFF / Bypassed by Default)
- **Inputs**: Current Stage 2 dry output + explicit source/receiver geometry + selected mic profile, polar pattern, aim, implemented response, and optional implemented source-radiation model.
- **Output**: Direct mic capture buffer + explicit immutable `CaptureContext` handed directly to Stage 4 (carrying the exact generated dry excitation, direct buffer, poses, mic transfer snapshot, direct delay/gain/filter/timeline/routing, and flags of already-applied direct operations; zero original PCM or global-state shortcuts).
- **Enabled Behavior**: Implemented mic response, direct distance, propagation delay, directional pickup, independent multi-mic slots with explicit channel/slot routing (no undocumented mic sum).
- **Bypass Contract**: Returns Stage 2 audible buffer unchanged; exposes a neutral pickup in `CaptureContext` while retaining shared receiver positions for an independently enabled Stage 4; stored mic model/pattern changes CANNOT affect room-only audio.

### Stage 4 — Optional Space / Room (OFF / Bypassed by Default)
- **Default Configuration**: Bypassed, initialized to a `10 × 10 × 10 ft` padded room (2-inch acoustic wedge foam baseline).
- **Inputs**: Complete Stage 3 output + `CaptureContext` + room topology (`Closed` 6 faces / `Open` ground-only — open placement guides NEVER create invisible walls/ceiling), physical dimensions (m), independent 6-face materials, shared source/receiver geometry, acoustic model params.
- **No Double Application Rule**:
  - Direct path: If Stage 3 enabled, retain Stage 3 direct result ONCE; if Stage 3 bypassed, compute room direct propagation from `CaptureContext` dry excitation + neutral receiver.
  - Reflected paths: Build each reflected bounce from `CaptureContext` dry excitation, applying full path delay, inverse-distance pressure attenuation, per-band surface reflection factor, source radiation (if implemented), and effective mic pickup ONCE per path. NEVER divide through microphone nulls to recover excitation.
- **Bypass Contract**: Returns Stage 3 audible buffer unchanged (including timeline & length), preserves room config, ignores room tails; operates independently of Stage 3.

### Stage 5 — Optional Plugin Slot (OFF / Empty by Default)
- **Inputs**: Current Stage 4 output + explicitly selected compatible processor + saved settings (`Gentle EQ`, `Soft Dynamics`, `Noise Control`, `Phase Limiter`, or compatible Rust-hosted processor).
- **Strict Isolation**: Receives predecessor audio + declared context ONLY. MUST NOT access original audio, re-enter extraction, rewrite shape evidence, receive surgical edits, or modify upstream dry buffers.
- **State & Latency**: Explicit offline state initialization; reset history on bypass transitions or input-revision changes; account for declared algorithmic latency without erasing physical acoustic propagation delay.
- **Bypass Contract**: Returns Stage 4 audio unchanged; empty slot = exact identity.

---

## 05. Render Dependency & Cache Invalidation Matrix

`Reference Analysis → Stage 1 (Dry) → Stage 2 (Surgical) → Stage 3 (Mic) → Stage 4 (Room) → Stage 5 (Plugin)`

| Trigger | Preserved | Invalidated |
|---|---|---|
| Dry setting / voicing / source change | ReferenceShape | Stage 1, 2, 3, 4, 5 |
| Accepted surgical edit / Stage 2 bypass toggle | ReferenceShape, Stage 1 | Stage 2, 3, 4, 5 |
| Mic setting change (when Stage 3 enabled) | ReferenceShape, Stage 1, Stage 2 | Stage 3, 4, 5 |
| Room / surface material change (when Stage 4 enabled) | ReferenceShape, Stage 1, Stage 2, Stage 3 (if direct unchanged) | Stage 4, 5 |
| Shared 3D source/receiver placement change | ReferenceShape, Stage 1, Stage 2 | Every enabled capture stage consuming geometry (Stage 3/4/5) |
| Plugin setting change (when Stage 5 enabled) | ReferenceShape, Stages 1–4 | Stage 5 only |
| Bypassed stage-specific control edit | All active audio | None (saved as pending until re-enabled) |
| Display / camera / unit / selection-only edit | All committed audio & caches | None |

- Render jobs MUST use immutable snapshots + monotonic request IDs, discard obsolete completions, validate finite samples/format/revision before publishing, and report stage failures explicitly (NEVER silently bypass or substitute original/stale audio).

---

## 06. Catalogs, Provenance & Exact Acoustic Mathematics

- **Offline `capture-reference` Package** (zero runtime network calls):
  - Documented baseline inventory: `138` sources, `972` microphones (Mic Archive), `32` architectural surface materials — treated as manifest inventory checks, NEVER hard-coded capacity limits. Compatible additions/corrections load without engine changes.
  - Strictly separate published manufacturer/standard facts from effective renderer behavior; never invent missing measurements or Biot-JCA parameters; disclose shared renderer profiles explicitly.
  - Preserve provenance including Leslie Gaston-Bird's *Math Fundamentals for Audio*, Mic Archive, ASTM/ISO absorption standards, instrument-range references, RWC/SNDB, and DDSP/SMS literature.
- **Exact Acoustic Formulas** (internal units: metres, seconds + integer sample frames, Hz, linear amplitude):
  - `speed_of_sound_m_per_s = 343.0`
  - `delay_seconds = path_length_m / 343.0` → `delay_samples = delay_seconds * sample_rate_hz` (anchor check: `0.6096 m` / 2 ft ≈ `1.777 ms`)
  - Pressure attenuation is inverse-distance (`level_change_db = -20.0 * log10(d2 / d1)`), NEVER inverse-square on waveform amplitude.
  - Passive per-band reflection: `reflection_pressure = sqrt(1.0 - effective_absorption)` (`effective_absorption` clamped to `0.0..=1.0`, raw published values kept separately); multi-bounce = product across bounces.
  - Informational estimates (explicitly labeled as estimates; unavailable in Open mode; zero-absorption guarded against NaN/Inf; never replace geometric reflections):
    - `total_absorption_sabins = sum(surface_area_m2 * effective_absorption)`
    - `estimated_rt60_seconds = 0.161 * room_volume_m3 / total_absorption_sabins`
    - `axial_mode_hz = 343.0 * mode_index / (2.0 * dimension_m)`

---

## 07. Workspace, 3D Scene, Playback, Export & Safety Rules

- **Dark Audio-First Workstation**: Empty state shows zero fake tracks/measurements/audits; demos open only by explicit labeled action backed by real files. Aligned Original + Dry waveform/spectrogram + full-recording overview + shared time ruler & playhead + Nyquist unavailable-band marker.
- **Left Inspector Order**: `Source → Microphone → Materials → Room Size`. Shared authoritative 3D scene (`X` width, `Y` depth, `Z` height in metres). Display units (`ft/in/yd/mm/cm/m`) are presentation-only.
- **Source Defaults**: Footprint center, `1.524 m` (5 ft) height; center/5 ft locks OFF by default; `Reset Source` never resets mics or materials.
- **Mic & Aim**: Independent multi-mic slots; model swap preserves pose; `Top X/Y` drag keeps `Z`, `Front X/Z` drag keeps `Y`; aim commands never move mic; rotation release within 8° snaps to cardinal axis; SM7-style 2 ft reference setup stays inactive while Stage 3 is bypassed.
- **Room Presets**: Small `10×10 ft`, Medium `20×20 ft`, Large `50×50 ft` (default height `10 ft`); Open = ground only (`50×50×50 ft` visual guide only). W/L linked by default, H independent.
- **Playback, Export & Review**: Playback engine owns playhead (never seeks on rounded display redraw); logical time preserved across Original/Regenerated switches; level-matched monitoring copies never alter exported audio; ordinary dry/capture exports remain unmastered with headroom and preserved acoustic tails (no hidden normalization); generated audio remains `Candidate` until explicit human review bound to exact dry revision.
- **Real-Time & Resource Safety**: Zero blocking I/O, contended locks, or avoidable allocations on audio thread; explicit `Result` propagation; minimal documented `unsafe`; atomic file writes (`vendor/craft-atomic` pinned to `storytold/craft-libs@b24eba64758f1b9135e7a3afab1af88260f9323b`, MIT OR Apache-2.0) protecting originals and outputs from partial writes/overwrite.

---

## 08. Codebase Architecture Map & Verification Gates

### Current Module Map (`spectracraft-mono/src/`)
| Spec Section | Primary Rust Module(s) | Status / Notes |
|---|---|---|
| 02, 03 Evidence & Timeline | `reperform.rs`, `shape_profile.rs`, `digest.rs` | Active; `EvidenceStatus`, immutable SHA-256 hashes, source-free cues |
| 04 Stage 1 Dry & DDSP | `reperform.rs`, `string_state.rs`, `attack_guard.rs`, `reference_profiles.rs` | Active; harmonic+noise DDSP, cue-precedence physics, rough guess, bend-preserving straightening |
| 05 Stage 2 Surgical Edits | `region_edit.rs`, `src/lib/dryRegions.ts` | Active; raised-cosine inside-bounds crossfade, bit-identical outside check, Undo/Redo, debounced solo preview |
| 06, 07, 08 Stages 3–5 | `pipeline.rs`, `capture.rs`, `studio_catalog.rs`, `acoustic_math.rs` | Active; all post-dry stages OFF by default, independently bypassable, atomic WAV writes via `craft-atomic` |
| 10 Offline Catalogs | `studio_catalog.rs`, `capture-reference/` | Active; 138 sources, 972 mics, 32 surfaces, manifest-driven reload without engine changes |
| 12, 13, 14 Native + Shell UI | `src/bin/spectracraft-mono.rs`, `src/bin/spectracraft-qa.rs`, `src/` | Active; shared 3D scene, Plans/3D, Inspector, Dock, Transport, atomic export |
| 15 Atomic I/O | `vendor/craft-atomic/` | Active; pinned `storytold/craft-libs@b24eba64758f1b9135e7a3afab1af88260f9323b` (MIT OR Apache-2.0) |

### Required Quality & Verification Discipline (Section 16 & 17)
Every substantial engine or pipeline change MUST report **PASS / FAIL / NOT RUN** with exact commands and artifacts against:
1. **Rust Quality Suite**: `cargo fmt --all --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, `cargo test --locked --all-targets --all-features`, `cargo build --locked --release --all-targets --all-features`, plus `cargo audit` / `cargo outdated` / `cargo tree -d` / `cargo deny check` (marking unavailable tools `NOT RUN`).
2. **Contract Proofs**: Source-free deterministic render & original SHA-256 invariance; known synthetic controls (weak fundamentals, octave jumps, slides, repeats, overlap, quiet attacks, silence, clicks/chirps); stage bypass & no-double-application checks; surgical bit-identical outside & frequency-scope checks; catalog manifest & late-index checks; display-isolation invariance checks.
3. **Honest Handoff**: Never claim unverified platform runtime support or treat numerical scores/screenshots as proof of completed human listening review.
