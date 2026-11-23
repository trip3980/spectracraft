# `capture-reference` — Offline Reference Catalogs for SpectraCraft

This directory holds the **offline, structured reference datasets** embedded directly into `spectracraft-mono` at compile time (`include_str!`). The workstation has **zero runtime network dependency** on `micarchive.com` or `alfaacoustics.com`.

## 1. Microphone Archive Dataset (`mic-archive-972.json`)
- **Recovered count**: **972** unique microphones across **88** brands (recovered from `https://micarchive.com/compare` and `https://micarchive.com/mics/<id>/` via `recover_micarchive.mjs`).
- **Digitized curves**: **939** entries include published manufacturer on-axis frequency-response curve samples (`<= 24` log-spaced points within the published curve's actual frequency bounds); **667** entries also include published off-axis polar attenuation summaries (90° & 180° across published frequencies).
- **Zero invented data**: Every missing manufacturer specification (sensitivity, self-noise, max SPL, impedance, range, released year, power requirement, or curve) is stored explicitly as `null` and documented in `uncertainties`.
- **Honest labeling invariant**: Published manufacturer specs and curves are **reference data only** — never measured microphone impulse responses. The capture renderer uses its explicitly labeled analytical polar pattern (`p(θ) = a + (1 − a) cos θ`) and shared parametric tonal class, always displayed separately from published values.
- **Baseline ordering**: Index `0` is preserved as `Shure SM7B` (`shure-sm7b`) for repeatable baseline tests; indices `1..971` are sorted alphabetically by `brand`, `model`.

## 2. Acoustic Materials Dataset (`acoustic-materials-32.json`)
- **Profile count**: **32** room and outdoor ground acoustic profiles (`125, 250, 500, 1000, 2000, 4000 Hz`).
- **Standards & Biot-JCA concepts**: Informed by ASTM C423 / ISO 354 (random-incidence reverberation room absorption) and ASTM E1050 / ISO 10534-2 (impedance-tube / Biot-JCA porous characterization as used in Alfa Acoustics α-Sim / α-Inverse workflows).
- **No commercial database scraping**: Alfa Acoustics' commercial Acoustic Material Database is private and is **not** scraped, copied, or simulated here.
- **Raw vs Effective separation**: Raw published Sabin absorption coefficients (including `> 1.0` reverberation-room edge-diffraction values such as `1.05` and `1.12`) are stored verbatim and displayed separately from the clamped effective absorption `alpha_eff = raw.clamp(0.0, 1.0)` used by the renderer's passive reflection multiplier `sqrt(1.0 - alpha_eff)`.
- **Unfabricated Biot parameters**: Where porous Biot parameters (porosity, airflow resistivity, tortuosity, VCL, TCL) are not published in open architectural tables, every field is explicitly `null` and labeled `Not published — not invented`.

## 3. Acoustic Source Frequency & Resonance Dataset (`source-profiles-138.json`)
- **Profile count**: **138** acoustic source entries (`Unknown`, `Instrument` families, `Human` Male/Female/Other-Ensemble), compiled offline via `build_source_profiles.mjs`.
- **Published reference charts & databases**: Compiled from published acoustic range & frequency charts (Yona Marie Music Instrument Frequency Chart, Adler/Meyer/Rossing orchestral & vocal reference tables, RWC Musical Instrument Sound Database, Single Note Database `audiolabs/SNDB`).
- **Zero invented peaks & zero range clamping**: Broad published spectral regions (body/warmth, presence/attack, high/air) and qualitative resonance notes are strictly informational. Exact narrow instrument-specific body/cavity modes are never invented, and extracted performance cues outside published fundamental ranges are **always rendered** without clamping or transposition.
- **Source-informed dry regeneration**: Each source entry maps to an explicitly labeled parametric synthesis template (`regen_model`) used by `reperform::Performance::synthesize_dry_for_source()` so changing the source alters timbre/formants/resonance character while preserving performed notes, pitch trajectories, timing, and dynamics — never blending in original audio.

