# SpectraCraft — Standing Program Bible & Operational Execution Contract

> **Authority & Canonical Reference**:
> This Project Bible is the permanent operational specification for **SpectraCraft** and works directly
> alongside the complete 17-section canonical contract retained at [`SPECTRACRAFT_SPEC.md`](./SPECTRACRAFT_SPEC.md).
> Whenever any ambiguity arises, `SPECTRACRAFT_SPEC.md` is authoritative and binding for every change to
> the Rust engine, CLI/QA tools, catalogs, native desktop shell, server bridge, and workstation UI.

**Engineering-contract expansion: 2026-11-26.** The implementation baseline is commit `9fbd775800b039f77cb6c9a901a458a0dadb715a`. Sections 00–09 retain the established product rules; Sections 10–20 define enforcement, acceptance evidence, and implementation order. Requirements describe required behavior, not proof that current code already complies. [`AUDIT_PROGRESS.md`](./AUDIT_PROGRESS.md) records unresolved work and verified implementation evidence.

**Reading order:** Read the canonical specification, the relevant pipeline sections below, [Section 10](#10-authority-evidence-and-completion), the applicable enforcement requirements, and the implementation ledger before changing behavior. Existing code or passing tests cannot override a canonical requirement. Documentation changes alone do not resolve an implementation finding.

---

## 00. Ponytail Engineering Discipline (Standing Change Rules)

Before writing any code, stop at the first rung that satisfies the request:

1. **Does the requested behavior belong in scope?** Omit unnecessary additions (YAGNI).
2. **Does the codebase already provide the behavior?** Reuse the existing implementation.
3. **Does Rust stdlib or an existing crate provide the behavior?** Use the supported implementation.
4. **Does a native platform feature satisfy the requirement?** Use that capability through the existing boundary.
5. **Can a small clear change satisfy the complete requirement?** Prefer the smallest correct change.
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

- **Product Name**: **SpectraCraft** (`spectracraft-mono` crate and main CLI binary, `spectracraft-desktop` native desktop binary, `spectracraft-qa` CLI/QA binary). All product-owned crates, binaries, modules, docs, titles, commands, and UI labels MUST consistently identify SpectraCraft.
- **Native Rust Core**: All SpectraCraft-owned audio processing, analysis, synthesis, acoustic math, plotting, CLI tools, fixtures, and verification tools MUST remain Rust.
- **No Forbidden Dependencies**: Python, Python wrappers, browser substitutes, and promotional dashboards MUST NOT become application, engine, or verification dependencies. (The React/Express preview layer in this workspace is strictly a thin transport/UI shell over the real compiled `spectracraft-qa` / `spectracraft-mono` Rust binaries.)
- **Scope**: Reconstruct a single-source performance for subsequent audio work. Full-mix separation, universal transcription, historical gear identification, forensic recovery, and full DAW replacement are explicitly out of scope.
- **Build Order**: Maintain a rock-solid runnable core workflow first (import → shape extraction → dry regeneration → original/dry comparison → basic region tools), with optional capture (Mic, Room) and plugin stages cleanly layered on top.

### Rust-First Engine Rule (Standing Language & Ownership Contract)

SpectraCraft has two parts: the React + Vite UI (`src/`) is TypeScript and is what the browser preview runs, while the audio engine, Stage 1 physics, persistent string state, rendering, blind reporting, and all DSP live in Rust (`spectracraft-mono/` and related Rust crates). Follow these rules on **every** change:

- **Prefer Rust**: Edit Rust code in the `spectracraft-mono` crates (and any other Rust engine directories) unless the task is explicitly a UI-only change.
- **Thin TypeScript surface only**: Treat the TypeScript layer as a thin viewer, control surface, and adapter only. Do not move engine logic, physics, or DSP into TypeScript.
- **No preview-convenience drift**: Do not default to TypeScript solutions just because the browser preview can hot-reload them.
- **Tie-breaker goes to Rust**: If a change could be implemented in either language, implement the shared behavior in Rust and keep the TypeScript side as a thin caller.
- **Preserve engine ownership**: Keep shared audio, physics, validation, and revision semantics in Rust. Language-percentage statistics do not prove compliance with that boundary.
- **Name implementation ownership explicitly**: Every change summary must identify affected Rust modules and adapters; a documentation-only or UI-only change must identify that scope honestly.
- **Engine-owned invariants**: Keep Stage 1 persistent-state physics, additive re-pick, continuous bend, and blind Before/After reporting strictly in Rust. Leave the TypeScript UI as the viewer and control surface only.

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
- **Real-Time & Resource Safety**: Zero blocking I/O, contended locks, or avoidable allocations on audio thread; explicit `Result` propagation; minimal documented `unsafe`; atomic file writes (`craft-atomic` pinned via `storytold/craft-libs@05b99ee5dcd64f2c954a693ef361301214ddc3c7`, MIT OR Apache-2.0, behind thin adapter `report_export.rs`) protecting originals and outputs from partial writes/overwrite.

---

## 08. Codebase Architecture Map & Verification Gates

### Implementation Ownership Map

Paths below are repository-relative. A listed module establishes an implementation location, not verified compliance. Surface-specific gaps and acceptance evidence belong in [`AUDIT_PROGRESS.md`](./AUDIT_PROGRESS.md).

| Spec Section | Primary implementation location | Responsibility / verification boundary |
|---|---|---|
| 02, 03 Evidence & Timeline | `spectracraft-mono/src/{reperform,shape_profile,digest,pipeline}.rs` | Evidence identity, source-free packets, immutable buffers, and revision ownership |
| 04 Stage 1 Dry | `spectracraft-mono/src/{reperform,string_state,attack_guard,reference_profiles}.rs` | Persistent source physics, deterministic synthesis, and evidence precedence |
| 05 Stage 2 Surgical Edits | `spectracraft-mono/src/region_edit.rs`; `src/lib/dryRegions.ts`; `src/App.tsx` | Rust edit behavior plus adapter wiring; selection history does not prove audio undo |
| 06–08 Capture & Plugins | `spectracraft-mono/src/{pipeline,capture,studio_catalog,acoustic_math,acoustics_physics}.rs` | Stage ownership, physical paths, routing, bypass, and complete tails |
| 10 Catalogs | `spectracraft-mono/src/studio_catalog.rs`; `spectracraft-mono/capture-reference/` | Manifest inventory, provenance, and effective renderer mappings |
| 12–14 Native & CLI | `spectracraft-mono/src/bin/desktop.rs`; `spectracraft-mono/src/main.rs`; `spectracraft-mono/src/bin/spectracraft-qa.rs` | Native application, main CLI, and QA/export command boundaries |
| 12–14 Existing adapters | `src/`; `server.ts`; `src-tauri/` | Thin control/transport surfaces; packaged operation requires independent verification |
| 15 Persistence & Export | `spectracraft-mono/src/{report_export,settings_store,update_manager,daw_bridge}.rs` | Atomic publication, truthful provenance, recovery, and pinned infrastructure adapters |
| 16–17 Qualification | `spectracraft-mono/src/source_qualification.rs`; `spectracraft-mono/source-qualification/` | Manifest-driven recorded-source qualification, 138-source coverage matrix, and frozen scoring criteria |

### Required Quality & Verification Discipline (Section 16 & 17)
Every substantial engine or pipeline change MUST report **PASS / FAIL / NOT RUN** with exact commands and artifacts against:
1. **Rust Quality Suite**: `cargo fmt --all --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, `cargo test --locked --all-targets --all-features`, `cargo build --locked --release --all-targets --all-features`, plus `cargo audit` / `cargo outdated` / `cargo tree -d` / `cargo deny check` (marking unavailable tools `NOT RUN`).
2. **Contract Proofs**: Source-free deterministic render & original SHA-256 invariance; known synthetic controls (weak fundamentals, octave jumps, slides, repeats, overlap, quiet attacks, silence, clicks/chirps); stage bypass & no-double-application checks; surgical bit-identical outside & frequency-scope checks; catalog manifest & late-index checks; display-isolation invariance checks.
3. **Honest Handoff**: Never claim unverified platform runtime support or treat numerical scores/screenshots as proof of completed human listening review.

---

## 09. Standing Security & Robustness Checklist (Pre-Export & File-Handling Gate)

Run this checklist whenever the user requests a security, robustness, or pre-export audit, or whenever new file-handling, IPC, server bridge, or export code is added. Also mirrored as a project-local agent skill at `.agents/skills/security-robustness-check/SKILL.md`.

### Critical
- No secrets or API keys in frontend TypeScript, CSS, or committed files.
- No database or sensitive queries running in the browser. All real work stays in the Rust engine or `server.ts`.
- File uploads (WAV) are validated for type and size before they reach the Rust binary.
- No original PCM or copied recording noise can enter Stage 1 or generated DAW stems; explicitly requested unchanged original reference copies remain separately identified.
- Temporary files and session folders are cleaned up and not left world-readable.

### High Priority
- Server-side validation of all inputs that reach the Rust CLI or Express endpoints.
- Path traversal protection on any file paths accepted from the UI or CLI.
- Error messages from `server.ts` and the Rust binaries do not leak internal paths or stack details to the client.
- CORS and any API endpoints are not left wide open.
- Dependencies (`Cargo.toml` and `package.json`) do not contain known vulnerable or unpinned packages.
- The DAW interchange layer keeps generated stems source-free, labels any explicitly requested unchanged original reference copy separately, and sanitizes user-controlled session metadata.

### Medium Priority
- Environment variables and local config are not committed.
- Logging does not record full audio paths or user data unnecessarily.
- The Express session map and any long-lived caches have a reasonable bound or cleanup path.
- Tauri and desktop IPC commands do not accept arbitrary file paths without checking.

### Standing Execution Rules
1. **Prefer Rust** for any engine, IPC, or interchange fixes (per Section 01 Rust-First Engine Rule).
2. **Protect core invariants**: never alter Stage 1 persistent-state physics, additive re-pick, continuous bend, or blind Before/After reporting during security or hardening passes.
3. **Trigger discipline**: run this checklist on security/robustness/pre-export audits and whenever new file-handling, upload, IPC, or export paths are added or modified.
4. **Honest environment boundaries**: inspect actual toolchain and platform-library availability. Run applicable checks when prerequisites exist. Record unavailable checks as `NOT RUN` with the concrete missing prerequisite; the word sandbox alone does not establish that Rust checks are unavailable.

---

## 10. Authority, Evidence, and Completion

**SC-GOV-01 — Preserve the canonical contract.** `SPECTRACRAFT_SPEC.md` remains authoritative. This expansion operationalizes existing rules without changing the nine invariants, five-stage order, native Rust engine ownership, or single-source scope. The existing React/Express and Tauri surfaces remain thin adapters; adapter work does not authorize replacing the canonical native workstation or moving shared DSP into TypeScript.

**SC-GOV-02 — Separate requirements, implementation, and evidence.** A requirement states the desired behavior; source code establishes implementation; a reproducible observation establishes verification for a declared scope. A menu, configuration field, module, screenshot, successful compilation, or passing helper test alone MUST NOT establish complete workflow support. Each completion claim MUST cite the governing requirement, tested revision, affected surface, acceptance case, and actual outcome.

**SC-GOV-03 — Use independent status dimensions.** Implementation status uses `Pending`, `In Progress`, `Implemented`, `Verified`, or `Blocked`. Verification outcomes use `PASS`, `FAIL`, or `NOT RUN`. Artifact validity uses `Current`, `Stale`, `Unavailable`, or `Failed`. Human review remains `Candidate` until an explicit review of the matching dry revision is recorded. These dimensions MUST NOT substitute for one another. `Implemented` without passing relevant evidence remains unverified.

**SC-GOV-04 — Reconcile older instructions explicitly.** Canonical Section 14 permits Candidate audition and valid stage export without claiming human approval. Earlier mandatory-listening-gate language in `spectracraft-mono/DESIGN-CONTRACT.md`, helper tests, or UI states MUST NOT override that rule. Dry-first means a valid generated dry dependency exists before capture; dry-first does not manufacture listening approval. Human approval of dry audio also does not imply listening approval of every capture/plugin variant. A conflict in existing tests requires a documented contract correction with replacement coverage, never silent assertion deletion.

Every change MUST state the requirement addressed, expected observable result, affected dependencies, relevant failure paths, and verification performed. A documentation-only change requires document consistency and reference checks, not an unrelated full DSP rerun. A source change requires the targeted behavioral checks and broader gates warranted by the affected release surface. A release requires the complete applicable gate set in Section 18.

---

## 11. Authoritative State and Asynchronous Operations

**SC-STATE-01 — One authoritative project state.** Shared Rust application logic MUST own reference identity, Stage 1 base, accepted Stage 2 revision, effective stage settings, downstream dependency identities, and review bindings. Existing Rust types and modules SHOULD be extended before introducing parallel state frameworks. Native UI, HTTP records, Tauri IPC, and React state MUST reference the same validated semantics. Frontend controls or cached filenames MUST NOT independently certify an artifact as current or reviewed.

**SC-STATE-02 — Validate every publication boundary.** Each operation MUST capture project/session identity, session epoch, request sequence, expected parent revision, effective configuration fingerprint, renderer/catalog versions, format, and deterministic state. Opening, replacing, closing, or restoring a session MUST invalidate prior pending work. Every completion MUST recheck dependencies after all asynchronous boundaries, including file reads, subprocess completion, and audio decoding, before publishing buffers, status, labels, URLs, or history. Accepted mutations MUST compare the expected parent at the Rust boundary. Cancellation releases resources; freshness validation remains mandatory when cancellation arrives too late.

**SC-STATE-03 — Resolve exact artifacts.** Every generated buffer, candidate, report, variant, and export MUST retain an immutable record containing stage, parents, content hash, effective settings, renderer/catalog identities, sample rate, channel layout, frame count, timeline origin, physical delay, and tail status. Unknown, expired, missing, or cross-project explicit selectors MUST fail explicitly. Current-result export and dependent processing MUST also reject stale dependencies. Initial dry, last output, original PCM, and default engine state MUST NOT silently replace requested artifacts. A documented omitted-selector compatibility path may select baseline only when no explicit identifier was supplied. Retained validated historical artifacts may be exported only through an explicitly historical selection preserving the historical revision, producing settings, and labels; historical export cannot certify an artifact as current or authorize a stale-parent edit.

**SC-STATE-04 — Bind review and invalidation to actual audio.** Human review MUST identify project, source/evidence identity, exact effective dry revision/hash, explicit review action, and review scope. Accepted dry changes invalidate inherited approval. Undo or restore may recover an earlier review only when the exact reviewed artifact and identities are restored. Numerical scores and copied checkboxes cannot create review. Valid Candidate outputs remain available for audition and stage export with truthful labeling. Pending controls on bypassed stages preserve active audio; enabled effective changes follow Section 05. Accepted dry edits mark downstream output stale immediately while retaining the canonical deferred rerender policy during surgical editing.

**Required scenarios:** Delay session A's render and decode, open B or close A, then finish A; no A state may appear in B. Submit two edits against one parent; only a valid current-parent transaction may commit. Render capture, change an enabled control with Live Preview off, then request current export; stale output must be rejected. Change a bypassed stage-specific control; active output identity must remain unchanged. Approve dry A, produce B, and undo; approval must follow validated artifact identity rather than checkbox state.

---

## 12. Recoverable Projects and Edit Transactions

**SC-PROJECT-01 — A saved project must recover the work.** A versioned project manifest MUST preserve original asset identity/checksum and recoverable location, reference/cue identities, renderer/catalog identities, deterministic synthesis settings, effective and pending stage controls, Stage 1 base, ordered accepted edits, current generated revisions, review bindings, relevant analysis-layer state, and workspace state. Recovery MUST retain validated generated assets or sufficient compatible reconstruction data to reproduce the accepted output. Ephemeral server run IDs alone are insufficient. Success requires durable referenced data and a consistent manifest, not merely serialized controls. Settings-only saves MUST be labeled presets.

**SC-PROJECT-02 — Restore transactionally.** Loading MUST validate schema, supported version, sizes, checksums, lineage, required catalog/model identities, and assets in temporary state before replacing the active project. Missing or changed assets require an explicit unresolved/relink state; another loaded track cannot satisfy the dependency. Legacy control-only files may be imported as presets without restoring unbound approval. Failed load/save preserves the last valid project and original files. Browser recovery or project export/import MUST preserve the same semantics; refresh/navigation requires an explicit unsaved-work policy. Renderer upgrades MUST NOT silently regenerate different audio and retain the prior hash or approval.

**SC-EDIT-01 — Commit and undo audio revisions.** Accepted edits MUST retain exact parent/output identities, committed time/frequency bounds, request, effective parameters, and acceptance order. Apply, Undo, and Redo MUST restore actual dry buffers or validated reproducible revisions and update dependencies atomically. Stage 2 bypass returns the unmodified Stage 1 base. Moving or deleting a selection cannot relocate or undo accepted audio. Upstream regeneration preserves region definitions and requests but invalidates incompatible patch audio. History retention MUST participate in artifact ownership and eviction.

**SC-EDIT-02 — Separate gestures, selections, and editable surfaces.** Geometry history MUST capture the pre-gesture state, keep intermediate pointer movement transient, and commit one transaction on release. Cancel restores the pre-gesture state. Region identifiers MUST remain unique through deletion, recreation, undo, and restore. Surgical callbacks MUST validate that the displayed buffer is the matching current generated dry revision; original, captured, difference, and isolated-layer views cannot silently supply dry-edit coordinates.

**SC-EDIT-03 — Honor zero effect and exact bounds.** Strength zero MUST return bit-identical input. Nonzero strength MUST approach zero continuously without hidden minimum processing. Temporal samples outside accepted bounds remain bit-identical. Directly modified coefficients and taper support stay within the authorized frequency mask. An operation unable to honor scope MUST reject unchanged; a wider operation requires an explicitly changed selection and new acceptance. Finite temporal windows do not justify claims of mathematically perfect frequency isolation.

**Required scenarios:** Save after accepted edits, reopen from empty state and over another project, and compare source/evidence/dry/history identities. Remove a required asset and verify transactional failure. Apply/Undo/Redo and export each state; hashes must match recorded revisions. Recreate regions at identical coordinates without collisions. Drag through many pointer moves and undo once. Run zero-strength tests for every fix kind plus out-of-region exact comparisons.

---

## 13. Audio Coordinates, Playback, and Responsiveness

**SC-MEDIA-01 — Use the actual buffer format.** Original asset, engine-rendered audio, and decoded monitoring PCM MUST each carry their own sample rate, channel layout, frame count, and timeline origin. Plot indexing and frequency calibration MUST use the displayed buffer's actual rate. Browser decoding may resample to the playback context; that monitoring derivative must not redefine original evidence. Above-source-Nyquist content remains unavailable even when the monitoring rate is higher. Selection conversion MUST use one documented seconds-to-engine-frame rule with half-open bounds and bounds checks. Excerpts retain source timeline offsets; a passage beginning at 20 seconds cannot masquerade as source time zero.

**SC-MEDIA-02 — Publish one capability contract.** Rust validation MUST define supported encodings, rates, channels, duration, geometry, and resource limits. Import, slicing/layers, synthesis, preview, and export MUST share that contract or expose explicit operation-specific limits before work begins. Unicode display names require valid transport encoding and remain separate from generated storage paths. Channel selection, downmixing, resampling, and excerpts require explicit derivative provenance. Full-track requests MUST NOT silently become 30-second or 90-second outputs. Until full-timeline processing satisfies the canonical requirement, excerpt support remains a disclosed limitation, not full-track compliance.

**SC-MEDIA-03 — Let the audio clock own transport.** Playback position MUST derive from the audio engine clock and explicit anchors. Animation frames and rounded displays cannot drive seek or crossfade offsets. Replacement buffers MUST update identity, gain, rate, channels, duration, and timeline together. Output tails remain audible and seekable beyond reference duration. Switching to a shorter buffer follows a documented end-of-take policy. Device removal, output-rate changes, and underruns produce controlled state and recovery behavior. Monitoring alignment/gain matching remains separate from canonical render/export data.

**SC-PERF-01 — Keep expensive work off interactive paths.** Analysis, synthesis, capture, and expensive report generation MUST run through bounded workers outside GUI event handlers and audio callbacks. Coalescing/debouncing MUST retain immutable inputs and stale-completion rejection. Audio callbacks perform no blocking I/O, contended locks, or avoidable allocation. Playhead-only redraws MUST perform zero new FFT or waveform-analysis work. Cache audio-derived graphics by buffer identity, rate, viewport, dimensions, and scale; lightweight overlays update separately. Shared spectral analysis remains Rust-owned under the existing engine boundary. Performance evidence MUST state hardware, fixture duration/rate, display size, latency distribution, memory/disk budgets, and cancellation behavior; a fast helper test does not establish application responsiveness.

**Required scenarios:** Plot known impulses/tones at 8, 44.1, 48, and 96 kHz with mismatched monitoring rates. Test the actual declared WAV capability matrix through downstream operations. Seek into a capture tail, swap longer/shorter buffers, and preserve logical time. Instrument analysis invocation counts during playhead animation. Exercise cancellation and rapid control changes while playback continues. Retain the last valid audio when device or render transitions fail.

---

## 14. Physical Audio and Evidence Verification

**SC-DSP-01 — Prove source isolation structurally.** Fresh-signal verification MUST render a saved source-free cue packet without original-file access, repeat the same declared execution configuration, compare deterministic samples, and verify unchanged original checksums. Cue packets must lack source PCM, phase, residuals, and source-file capabilities. Acoustic scores cannot prove absence or presence of copied PCM. Original monitoring and explicitly requested unchanged reference copies remain separately labeled and outside the generated processing graph.

**SC-DSP-02 — Measure physical behavior in normal output.** Canonical capture MUST apply actual path delay, calibrated inverse-distance pressure attenuation, effective directional response, and declared routing exactly once. Predicted metadata alone does not satisfy the requirement. The canonical speed of sound remains `343.0 m/s`; temperature-dependent propagation requires a separate canonical change. Distance singularities require a documented positive-distance policy. Aligned/level-matched monitoring uses separately identified transforms and cannot remove propagation from ordinary capture exports. Room-only processing uses neutral pickup; combined mic/room processing must avoid double application.

The neutral direct-path acceptance fixture MUST enable microphone processing, bypass room/coloration, and compare positive distances outside singularity guards. Doubling distance must produce the modeled one-half pressure ratio (approximately `-6.0206 dB`) and additional delay `delta_distance / 343.0`. Initial acceptance budgets are at most `0.1 dB` gain error and one sample of impulse-arrival error at each tested rate. Measure gain using a `200 Hz` sine and a `200 ms` RMS window beginning at least `50 ms` after the predicted direct arrival. Measure direct impulse arrival at the absolute response peak, choosing the earliest frame for exact ties and accounting for the declared timeline origin and separately measured algorithmic latency. The test MUST NOT subtract physical propagation as latency. A fractional-delay implementation must separately document and test the intended interpolation response. These budgets apply to the neutral fixture, not arbitrary music or colored microphones. The `0.6096 m` reference remains approximately `1.777 ms`. Any budget change requires independent numerical justification and recorded review, not matching the current faulty output.

**SC-DSP-03 — Preserve complete declared tails.** Stateful stages MUST record output frame count, physical delay, tail policy, termination threshold/window, maximum resource limit, and completion status. Offline rendering continues with zero excitation until the documented residual/state criterion is met. Termination MUST account for scheduled later reflections and remaining filter/feedback state; a short quiet interval alone is insufficient. Reaching a safety cap before completion produces an explicit incomplete-tail result or failure, never a complete ordinary export. Bounded previews disclose omitted intervals/tails and cannot define export length. Tests compare short excitation near the input endpoint with the same excitation followed by sufficient explicit silence, covering direct delay, late reflections, decay, and plugin state.

**SC-DSP-04 — Verify generated excitation and numerical domains.** Noise intended as centered bipolar excitation MUST exhibit both polarities and a statistically justified mean bound for a declared seed, distribution, and sample count. Renderer tests preserve deterministic replay and finite output. Geometry, coefficients, gains, and filter state MUST remain within declared valid domains. Passive per-path reflection factors follow Section 06; constructive interference does not justify a false universal rule that summed waveform peaks must always decrease.

**SC-DSP-05 — Separate measurement from inference.** Reports MUST distinguish structural provenance, direct sample measurements, physical model predictions, acoustic estimates, and human listening observations. Every metric MUST identify formula, units, interval, preprocessing, valid domain, calibration fixture, and uncertainty/unavailable state. Blind acoustic estimators MUST NOT consume target labels or render settings as evidence of inferred recording conditions. Room/material or distance classifiers cannot become proof of physical geometry or source leakage. Diagnose failing renderer tests separately from estimator calibration; retain independent held-out controls and explicit denominators for uncertainty/disagreement.

**SC-DSP-06 — Match comparison rigor to the claim.** Bypass identity, unchanged originals, out-of-region samples, zero-strength identity, and deterministic rerenders within the same declared execution configuration require exact comparisons. Physical formulas use independently justified absolute/relative, sample-time, or dB tolerances. Cross-platform comparison MUST declare compiler, target, renderer/backend, format, seeds, and tolerances before claiming equivalence; a universal bit-identical floating-point guarantee cannot be assumed. A release goal of zero known acceptance failures does not mean zero uncertainty or proof that no undiscovered defect exists.

---

## 15. Validation, API Errors, and Trust Boundaries

**SC-API-01 — Validate semantic state in Rust.** Native UI, CLI, HTTP, IPC, and project loading MUST invoke shared Rust validation before rendering or committing state. Validation covers finite numbers, format/rate, bounded collections, catalog identifiers, geometry, routing, and resource requirements. An empty microphone list, invalid selected index, or excessive microphone count must produce a defined empty state or actionable error before indexing; the current four-slot limit remains explicit. An all-disabled rig cannot silently reactivate a slot. Invalid imported state leaves the last valid project unchanged. Clamping cannot substitute for semantic validation when clamping changes the meaning of imported work.

**SC-API-02 — Return stable, bounded errors.** Parser, validation, queue, process, filesystem, and stream failures MUST have an appropriate error boundary. HTTP responses use stable error codes, bounded messages, and request identity; IPC maps the same semantics. Unknown artifacts normally return 404, known expired artifacts 410, revision conflicts 409, malformed input 400, oversized bodies 413, and unexpected internal failures 500. Ownership checks must avoid disclosing another user's resource existence. Internal stack traces and private paths remain in appropriately limited internal logs. Express 4 asynchronous failures require explicit forwarding. Help/document lookup uses own-key validation or a Map. No native work starts after failed request resolution.

**SC-SEC-01 — Make deployment trust explicit.** The local bridge MUST bind loopback by default. Remote binding requires explicit configuration, authentication, per-project authorization, and an appropriate protected transport. Administrative source maintenance requires separate authorization and remains disabled in ordinary packaged sessions. Origin checks compare complete configured origins and supplement authentication; caller-controlled Host or forwarded headers cannot create trust. Proxy headers are honored only through configured trusted proxies. Security evidence must distinguish HTTP-client probes, browser behavior, native IPC, and external deployment protections.

**SC-SEC-02 — Scope native capabilities.** Tauri commands MUST expose only required operations through explicit custom-command permissions and an appropriate content security policy. File operations require project-scoped or user-selected capabilities enforced in Rust; a valid extension or absolute path alone does not establish authorization. Path handling must account for traversal, symlinks, and replacement races for the supported platform. Native file handles must not become source-access capabilities in synthesis-facing packets. Resource handles and public error payloads must avoid unnecessary private-path disclosure.

**Required scenarios:** Import malformed rig JSON and excessive collections; preserve active state without panic. Request unknown, expired, foreign-project, and stale artifacts; verify no baseline substitution or unauthorized work. Exercise malformed JSON and oversized requests through the real HTTP boundary. Test default loopback behavior, unauthorized remote mutations, forged forwarded-host headers, and IPC paths outside selected scope. Validate packaged content policy and command permissions through the actual application boundary.

---

## 16. Storage Ownership, Recovery, Export, and Maintenance

**SC-STORE-01 — Own storage before cleanup.** A storage root MUST have exclusive process ownership or an explicitly supported shared-storage protocol before destructive recovery or pruning. An empty in-memory map does not establish abandonment. A second process failing ownership or listener startup MUST leave active files unchanged. Recovery distinguishes live instances, recoverable projects, retained history, committed artifacts, and abandoned temporary jobs. Platform-appropriate private permissions must be verified; failed permission changes cannot be reported as successful isolation.

**SC-STORE-02 — Lease required artifacts and handle stream failures.** Playback, jobs, exports, project records, and retained undo history MUST keep references/leases to required artifacts. Eviction excludes referenced data. Media serving opens a file once, derives metadata from the descriptor, and uses an error-handled stream with disconnect cleanup. Missing-file, read, and response errors release resources without terminating the process. Cancellation cannot delete another operation's inputs or the only recoverable committed output.

**SC-STORE-03 — Bound actual resources.** Jobs write to private owned temporary directories and publish only validated successful results. Failure/cancellation removes unpublished outputs. Budgets cover active and queued work, upload bytes, processing memory, retained files/bytes, previews, reports, and interchange bundles. The existing four-active/twelve-waiting Rust job policy may remain until measurement justifies a change. Map length alone is not a disk quota. Equivalent reports should reuse validated cached artifacts. Exhausted storage must reject new work safely when no unreferenced data can be evicted.

**SC-EXPORT-01 — Export producing state, not current controls.** Reports and DAW bundles MUST reference the exact chosen stage artifact, actual effective settings, source/evidence lineage, renderer/catalog identities, and stem hashes. A newly created default engine snapshot cannot stand in for missing provenance. Missing provenance requires an explicit unavailable state and no false reconstruction claim. Current controls edited after rendering cannot relabel prior audio. Original reference copies, when explicitly requested, remain unchanged and clearly separate from generated stems.

**SC-EXPORT-02 — Publish complete, faithful files.** Ordinary float32 dry/capture export MUST preserve the chosen buffer's samples and signal level. Integer conversion, clipping policy, quantization, and any dither must be explicit and tested. Normalization or mastering belongs only to an explicitly enabled finishing plugin; ordinary stage export introduces no separate normalization or mastering operation. Atomic no-overwrite publication protects originals and unrelated destinations. Multi-file bundles and projects require a recoverable transaction, such as a staged generation with a final manifest/publish marker; a sequence of individually atomic writes does not establish atomic publication of the set. Failed writes leave no apparently complete output. Durability guarantees and any platform limitations must be documented.

**SC-UPDATE-01 — Stage maintenance before publishing.** Source dependency maintenance is distinct from application updates and normal audio use. An exclusive workspace transaction MUST resolve/build/test isolated staged state, verify the active workspace has not changed, and publish all relevant manifests, lockfiles, and pin metadata through a recoverable transaction. Spawn failure, failed checks, timeout, cancellation, publication failure, and rollback failure each require an explicit outcome. Rollback success requires restored-byte verification. Cancellation must terminate and await descendant processes before releasing ownership. The updater must never report a partially published workspace as successfully updated or cleanly rolled back.

**SC-UPDATE-02 — Stage the selected target.** Update state MUST distinguish package identity, target kind, resolved version/full revision, and display text. Strings such as `craft-libs@<pin>` are labels, not raw-pin parser contracts. Download-later must fetch the requested target with the staged manifest/lockfile and record exact resolved dependencies. Fetching current dependencies does not establish that another target is available. Offline activation must validate the exact staged identities. Outdated-package detection must not advertise an unsupported activation operation.

**Required scenarios:** Start a second server against a live root and occupied port; retained files remain unchanged. Inject read errors and disconnect clients during streaming. Cancel/fail jobs after partial output, exceed quotas, and repeat report exports while observing retained bytes. Export nondefault render settings and round-trip the actual snapshot. Inject disk-full/interrupted writes. Run updater staging/activation with unavailable Cargo, failed verification, cancellation, concurrent maintenance, failed publication, and failed restoration against isolated test workspaces.

---

## 17. Transport Parity and Packaged Applications

**SC-PLATFORM-01 — Share commands and semantics across surfaces.** Native Rust, CLI, browser bridge, and Tauri MUST reuse authoritative Rust operations and validation. Existing adapters must expose consistent command/result schemas for supported import, analysis, synthesis, edit, capture, project, report, and export workflows. Transport selection must be explicit. An installed Tauri application cannot depend on an unstarted Express development server or an assumed `/api` origin. Implement native IPC and scoped media access with bounded jobs; adding an unauthenticated network sidecar does not satisfy the packaging requirement. Unsupported capabilities must be reported explicitly before work, rather than represented by nonfunctional controls.

**SC-PLATFORM-02 — Verify each shipped application.** A platform claim requires evidence for the exact package, application surface, OS/architecture, toolchain, dependencies, and device/runtime scope. Core compilation does not establish egui, Tauri, installer, or mobile support. The acceptance workflow covers clean installation/launch, real file import, regeneration, playback, accepted edit, save/reopen, capture, and verified export without development services. Each claimed Linux, Windows, macOS, iOS, or Android target needs separate evidence. Simulator/emulator results remain distinct from physical-device playback results; a universal build does not establish runtime coverage for every included architecture.

Parity compares canonical output, validation, revisions, provenance, and effective settings for the same fixtures. Platform dialogs and monitoring-device resampling may differ without changing canonical exported audio. Existing native functionality remains protected while optional adapters mature. Unsupported mobile or desktop targets remain `NOT RUN` or `Blocked`; configuration entries and icons do not establish support.

---

## 18. Verification Matrix and Release Gates

**SC-VERIFY-01 — Report independent build surfaces.** CI MUST report frontend, core Rust, native desktop, and excluded Tauri workspace results separately. A failed DSP test cannot turn later skipped compilation into a pass. Release artifacts MUST be built from the exact tested source/lockfiles. Commands below are required target gates, not claims that the current baseline passes; platform prerequisites and missing lockfiles must be resolved and recorded.

| Gate | Commands or observable check | Evidence scope |
|---|---|---|
| Core ownership/build | `cargo check --locked --lib --no-default-features`; `cargo test --locked` | Core and default CLI behavior only |
| Rust quality/release | Full Section 08 / canonical Section 16 quality suite | Formatting, warnings, all applicable features, tests, release binaries |
| Native desktop | `cargo check --locked --features desktop --bin spectracraft-desktop`; `cargo test --locked --features desktop --bin spectracraft-desktop`; release desktop build | Native compilation/tests; GUI runtime still requires a smoke test |
| Existing web adapter | `npm ci`; `npm run lint`; `npm run test:ts`; `npm run build`; actual HTTP/UI workflow tests | TypeScript and adapter behavior; helper tests alone do not cover UI wiring |
| Tauri workspace | `cargo check --locked --manifest-path src-tauri/Cargo.toml`; `npm run tauri:build`; installed workflow smoke test | Separate application lockfile, platform libraries, package and transport |
| Distribution/runtime | Install and run the actual target package through Section 17's workflow | Exact OS, architecture, simulator/device, audio device, and package identity |

**SC-VERIFY-02 — Attach behavior evidence.** Regression coverage MUST target observable user/data outcomes, including the scenarios in Sections 11–17. Rust-generated fixtures verify DSP; adapter/unit mocks may isolate scheduling or failures but cannot prove actual DSP quality or packaged operation. Reproduce a defect with a targeted failing case before changing behavior when practical, then retain the regression. Property-based checks are appropriate for bounded regions, geometry, serialized state, and finite values; integration/fault-injection checks are required for lifecycle races, persistence, and error recovery. Human listening remains separately recorded. A failed classifier must not be converted into a deterministic provenance claim.

**SC-VERIFY-03 — Make dependency and integrity checks reproducible.** Commit the root Cargo lockfile, application lockfiles for excluded Rust applications, and npm lockfile. Resolve exact dependency sets through supported tools; declare compatible Rust/Node baselines and test the stated minimums. Advisory review must identify enabled features, reachable APIs, reviewed versions, and any unresolved advisory; dependency presence alone does not prove exploitability, and apparent unreachability does not justify abandoning maintenance. Check optional features explicitly. Preserve licenses and attribution. Source-integrity manifests must use a documented root, include the actual relevant manifests/lockfiles and required source coverage, and be regenerated only after final changes. A stale checksum manifest cannot certify a handoff.

**SC-VERIFY-04 — Define completion without overclaiming.** A finding becomes `Verified` only when the implementation satisfies the governing requirement and relevant acceptance cases pass with recorded evidence. A release requires no known unresolved Critical/High defect in the claimed surface, passing applicable contract/build/runtime gates, and truthful capability limits. Medium findings need an explicit disposition, owner, and rationale; omitting a required capability from marketing does not turn an unmet canonical milestone into compliance. `NOT RUN`, `Blocked`, and missing human listening remain visible. Screenshots, test counts, or UI polish cannot replace end-to-end evidence.

Every verification record MUST include commit and working-tree state, lockfile identities, commands and exit outcomes, platform/toolchain, fixture identity/provenance, relevant artifact hashes, requirement IDs, and remaining uncertainty. Exact contract checks require exact comparisons; measured or inferred behavior uses Section 14's declared domains and tolerances. New changes invalidate evidence for affected behavior; historical logs remain historical. Unchanged unrelated checks need not be repeated merely because an agent session resumed.

---

## 19. Implementation Order and Resumable Handoff

The following sequence addresses root causes before expanding dependent application surfaces. Independent work may proceed in parallel when shared interfaces are agreed and file ownership is clear. Every batch ends with targeted verification and an updated [`AUDIT_PROGRESS.md`](./AUDIT_PROGRESS.md); no batch receives a completion label from documentation changes alone.

| Batch | Concrete work and main locations | Exit evidence |
|---|---|---|
| 1. Establish safe baseline | Fix desktop configuration initializer; make target checks independent; protect loopback/authorization, startup ownership, media errors, and unsafe source maintenance in `desktop.rs`, workflow, `server.ts`, `update_manager.rs` | Native compile result plus lifecycle/security/failure regressions; unrelated red gates remain visible |
| 2. Establish authoritative state | Reuse/extend Rust `pipeline.rs` state and shared validators; wire session epochs, expected-parent commits, immutable artifacts, and review identities through adapters | Session race, concurrent Apply, unknown-selector, and review-binding cases |
| 3. Preserve work | Implement project manifests/recovery, accepted-audio history, selection gestures/IDs, exact export provenance, and atomic outputs | Save/reopen, missing-asset, Undo/Redo, nondefault DAW snapshot, and failed-write cases |
| 4. Correct signal and coordinates | Fix physical propagation, tails, generated noise, bounded edits, decoded rates, format/channel limits, and honest report classification | Known-input DSP and rate/bounds fixtures through ordinary render/export paths |
| 5. Complete transport and responsiveness | Connect Tauri to shared Rust operations; remove GUI-thread renders and repeated spectral work; preserve playback clock/tails | Packaged import-to-export workflow plus measured interaction/cancellation behavior |
| 6. Close release evidence | Finish staging, quotas, dependency review/locks, source hashes, and each claimed platform's package/runtime gates | Exact tested release artifacts, recorded remaining limits, and no unresolved required gate |

### Resume protocol for human and AI implementers

1. Read the current specification, relevant Bible requirements, ledger, current branch, and working-tree changes. Inspect already modified files before applying another patch; an interrupted tool history does not establish a completed fix.
2. Select the smallest unresolved dependency or failed gate and state the required observable result. Reuse existing types and applicable repository skills; load only workflows relevant to the selected work.
3. Record baseline evidence, implement the focused correction, and run the regression plus necessary dependent checks. Keep code changes separate from unjustified oracle or threshold changes.
4. Update status, exact files, tested revision, commands/results, artifacts, remaining failures, and the next concrete action in the ledger. `Implemented` remains distinct from `Verified`.
5. Before interruption or handoff, leave enough durable context to resume without repeating the whole audit. Preserve uncommitted user work and identify any necessary migration or rollback path.

The implementation ledger retains the original H1–H15 and M1–M18 audit identifiers. New findings receive new identifiers without silently renumbering prior evidence. Existing unit tests are starting points, not automatic proof of the acceptance cases. No new agent framework, dashboard, or duplicate specification is required to follow this process.

---

## 20. Reference Basis and Change Control

The canonical [specification](./SPECTRACRAFT_SPEC.md) defines product behavior. The reviewed [baseline commit](https://github.com/trip3980/spectracraft/tree/9fbd775800b039f77cb6c9a901a458a0dadb715a) grounds the implementation map and initial ledger. External references below explain platform mechanisms; external references do not establish SpectraCraft compliance or introduce runtime network dependencies.

| Primary reference | Relevant enforcement |
|---|---|
| [W3C Web Audio, decodeAudioData](https://www.w3.org/TR/webaudio/#dom-baseaudiocontext-decodeaudiodata) | Decoded PCM may use the playback context's sample rate; actual buffer metadata controls indexing. |
| [Node.js stream documentation](https://nodejs.org/api/stream.html) | Stream lifecycle and error propagation require explicit handling at the media boundary. |
| [Tauri capabilities](https://v2.tauri.app/security/capabilities/) and [content security policy](https://v2.tauri.app/security/csp/) | Native command authorization and webview content policy are distinct boundaries. |
| [Cargo manifests and lockfiles](https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html) | Application dependency resolution requires committed exact dependency state. |
| [OWASP REST Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/REST_Security_Cheat_Sheet.html) | Authentication, authorization, origin handling, input validation, and error exposure require separate controls. |

Future changes to these requirements MUST identify affected canonical sections, compatibility/migration consequences, and acceptance updates. A deliberate product-contract change requires an explicit canonical specification revision; an implementation failure cannot quietly redefine the product. The standing handoff states completed work, actual evidence, unresolved behavior, and the next action without converting intentions into claims of correctness.
