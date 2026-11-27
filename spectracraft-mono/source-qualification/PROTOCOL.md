# SpectraCraft Recorded-Source Qualification Protocol

Version: `1.0.0`
Baseline commit: `9fbd775800b039f77cb6c9a901a458a0dadb715a`

## 1. Purpose and Honesty Boundary

This directory defines the reproducible qualification protocol, frozen acceptance criteria (`criteria.json`), versioned fixture manifest (`manifest.json`), and programmatically generated 138-source coverage matrix (`coverage-matrix.json`) for SpectraCraft.

- **Catalog inventory**: Generated exclusively from `spectracraft-mono/capture-reference/source-profiles-138.json` via `node scripts/generate-source-qualification-matrix.mjs` (138 entries, 11 display families, 17 `SourceRegenKind` renderer classes). Never maintain a second handwritten catalog list.
- **No synthetic-for-real substitution**: Synthetic controls verify DSP/pipeline engineering mechanics; they never establish real-recording qualification. Missing local real recordings produce `BLOCKED` or `NOT RUN` and never silently select another file or synthetic signal.
- **Separate status dimensions**: Catalog presence, renderer availability, synthetic-control status, real-recording technical status, real-recording musical status, application-surface status (`cli`, `desktop_egui`, `web_bridge`, `tauri_packaged`), and human listening review (`NOT RUN` until an actual human reviewer accepts the exact output hash) remain strictly independent.
- **No Python dependency**: All analysis, rendering, and scoring execute in Rust (`spectracraft-mono/src/source_qualification.rs` via `spectracraft-qa qualify-sources`).

## 2. Primary Real-Recording Sources & Partitioning

Acquire locally owned or appropriately licensed recordings (do not commit third-party audio to Git unless redistribution is explicitly permitted):

1. **Baseline regression recording** (documented in `spectracraft-mono/THIRD-PARTY.md`):
   - David Monniaux, *Slow solo* (`File:Slow_solo_20110108.ogg`), CC BY-SA 3.0 / GFDL, upstream SHA-256 `85157aae1d4c34861d18113622efd17fb84beee8d698863ef700c44727cf455d`. Treated as development/regression material; full timeline preserved alongside recording-specific 20–27 s and 25–26 s diagnostics.
2. **External primary corpora** (verify access/reuse terms before acquisition):
   - GuitarSet (`https://zenodo.org/records/3371780`) — guitar performances & aligned annotations (inspect publisher-listed annotation errors).
   - VocalSet (`https://zenodo.org/records/1442513`) — singing vowels/techniques across singers (supplement with separately licensed lyric, speech, rap, and whisper phrases).
   - University of Iowa MIS (`https://theremin.music.uiowa.edu/MIS.html`) — isolated instrument notes/articulations as range/attack controls (not a substitute for continuous phrases).
   - URMP (`https://labsites.rochester.edu/air/projects/URMP.html`) — isolated individual instrumental tracks & annotations.

### Partitioning Rules
- Acquisition smoke target: 2 genuine phrase recordings per display family; full target: >= 6 independent phrase recordings across >= 3 performers/origins per family partitioned into `development`, `validation`, and untouched `holdout`.
- Every recording-qualified named source requires >= 3 real phrase cases across >= 2 independent origins, including an untouched `holdout` origin.
- All crops, channels, resamples, codecs, or derivatives of one performance stay in one `performance_group` and one partition.

## 3. Execution Commands (Rust Host Required)

```sh
# Regenerate 138-source coverage matrix from catalog
node scripts/generate-source-qualification-matrix.mjs

# Run qualification manifest (controls + any locally supplied real recordings)
cargo run --release --locked --manifest-path spectracraft-mono/Cargo.toml --bin spectracraft-qa -- \
  qualify-sources \
  spectracraft-mono/source-qualification/manifest.json \
  spectracraft-mono/source-qualification/criteria.json \
  target/qualification-evidence/latest

# Run a single case ID
cargo run --release --locked --manifest-path spectracraft-mono/Cargo.toml --bin spectracraft-qa -- \
  qualify-sources \
  spectracraft-mono/source-qualification/manifest.json \
  spectracraft-mono/source-qualification/criteria.json \
  target/qualification-evidence/single-case \
  --case ctrl_guitar_plucked_arpeggio

# Run separated solo-instrument tests (synthetic control vs explicit real archive)
cargo test --locked --manifest-path spectracraft-mono/Cargo.toml --test solo_instrument_verification
SPECTRACRAFT_ARCHIVE_WAV=/path/to/verified.wav \
  SPECTRACRAFT_ARCHIVE_MANIFEST=/path/to/fixture-meta.json \
  cargo test --locked --manifest-path spectracraft-mono/Cargo.toml --test solo_instrument_verification -- --ignored
```

## 4. Frozen Acceptance Criteria Summary

See `criteria.json` (`v1.0.0`) for exact machine-readable limits:
- **Technical gates (every applicable case)**: Original SHA-256 unchanged; source-free `CuePacket` renders deterministically without source file access; repeated renders sample-identical; finite samples within declared headroom; complete declared duration preserved; non-silent performance verified; Stage 2–5 bypass bit-identical; Stage 1 bypass uses cached dry or returns `DryUnavailable`.
- **Musical targets (proposed project targets for clearly annotated applicable cases)**:
  - Pitched note events: precision >= 95%, recall >= 95% (50 ms onset, 50 cents pitch); offset-aware reported separately (`max(50 ms, 20% duration)`).
  - Continuous voiced pitch: >= 95% reliable voiced frames within 50 cents; raw octave errors < 1% (no octave folding).
  - False voicing: < 1% in reliable unvoiced/true-silent frames (counted separately).
  - Unpitched hits: precision >= 95%, recall >= 95% (30 ms onset; ghost notes included).
  - Genuine metric inapplicability (e.g., F0 on unpitched shaker) is marked `N/A` with a predeclared reason; unsupported claimed behavior remains `FAIL` or `BLOCKED`; missing recordings remain `BLOCKED` or `NOT RUN`.
