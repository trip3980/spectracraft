# SpectraCraft — Core Design Contract

## Purpose
Reconstruct one musical performance source at a time from a damaged or imperfect reference recording.

Output is a **new dry take**, not a cleaned version of the original waveform. The original audio is never blended into the result.

## Unit of Work
One source = one isolated instrument take, voice, or stem.
- May contain bleed, separation artifacts, crackle, wow, shimmer, etc.
- Still treated as a single musical subject (chords, ringing sustain, overlapping gestures are allowed and expected).
- Does **not** demix, classify every sound, or decide what is "noise".
- Other sources are imported and processed separately.
- The DAW is where tracks are reassembled.

## Core Invariant: Same Performance, Fresh Recording
Preserve (as evidence allows):
- Note onsets & offsets
- Pitch trajectories / bends / slides
- Timing, phrasing, dynamics
- Sustain & ringing behavior

Discard:
- Recording artifacts (crackle, wow/flutter, digital shimmer, separation smear, room tone not belonging to the source, etc.)

## Pipeline Stages (Strict Order)
1. **Analysis**
   - Spectrogram and other visual representations are **evidence only**.
   - Views may raise contrast, isolate layers, or expose faint details for inspection; these views never automatically appear in the output.
   - Track through time: pitch contours, harmonic trails, amplitude envelopes, note state (carry ringing notes into the next gesture).
   - Classify cues into: strong & coherent, weaker but consistent, unresolved / uncertain.
   - Rules of evidence:
     - A click is not automatically a pick attack.
     - Pitch fluctuation is attributed to the player only when evidence supports it; otherwise preserve uncertainty (do not flatten or invent).
2. **Performance Cue Extraction**
   - Produce editable, serializable performance cues (`cues.json`).
   - Cues are the sole source of truth for synthesis.
   - Original waveform is never used as a sample source.
3. **Synthesis**
   - Generate a new dry WAV from the cues alone.
   - No original samples blended in.
   - Result is a reconstruction, not recovered archival audio.
   - Goal is not to reverse-engineer the original guitar/amp/mic/room.
4. **Post-Processing (Gated)**
   - Further processing (new mic/room, analog/digital chain, expressive interpretation) is offered **only after** the dry reconstruction passes human listening checks for:
     - Rhythm
     - Phrasing
     - Dynamics
     - Sustain
     - Absence of artifacts

## Delivery Requirements
- Original remains untouched and available for A/B comparison.
- Original and reconstruction are time-aligned and level-matched.
- Export: clean, unmastered WAV with headroom.
- Editable performance cues are saved so the dry take can be regenerated or adjusted without re-running analysis.

## Implementation Constraints & Non-Goals
- App and engine: native Rust only.
- Visualizations exist to explain and audit the result; they are not proof the new take is correct.
- Human listening check is the final gate before any post-processing.
- Non-goals: do not demix a full mix; do not treat everything outside the target as noise; do not replace the DAW; do not invent confident answers when evidence is ambiguous; do not optimize for a pretty spectrogram.
