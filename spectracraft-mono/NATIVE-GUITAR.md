# Native guitar rebuild

From this crate, use the Rust executables:

- `spectracraft-mono analyze-guitar INPUT.wav CUES.json`
- `spectracraft-mono render-dry-guitar CUES.json OUTPUT.wav`
- `spectracraft-mono rebuild-dry-guitar INPUT.wav OUTPUT.wav`
- `spectracraft-mono export-guitar-variants INPUT.wav CUES.json NEW_DIR SELECTION`
- `spectracraft-mono analyze-guitar-legacy INPUT.wav LEGACY_CUES.json` (48 kHz compatibility path)
- `spectracraft-qa guitar-verify INPUT.wav NEW_DIR`
- `spectracraft-qa guitar-variants INPUT.wav CUES.json NEW_DIR SELECTION`
- `spectracraft-qa guitar-pitch-check REF.wav CAND.wav [START_S END_S]`

Input: predominantly monophonic mono guitar WAV, 8–96 kHz, up to 90 seconds.
`render-dry-guitar` requires only the saved cue JSON, with no source PCM, source phase, or access to the original recording. Rerunning `render-dry-guitar` from the same cue packet is deterministic sample for sample. None of these commands requires Python or any external interpreter. Output WAVs are unmastered mono float WAVs with editing headroom, and all output paths use create-new semantics so source audio is never overwritten.

## Evidence-separated cue analysis

`analyze-guitar` separates frame evidence into `supported`, `uncertain`, `out_of_target`, and `silence` (`frame_evidence` and `summary` in `CUES.json`):

- Competing pitch hypotheses (`hypotheses`) are retained per frame using harmonic-comb scoring and time-domain NSDF agreement; frames whose ambiguity cannot be resolved abstain as `uncertain` rather than guessing or folding octave errors.
- Unrelated clicks, chirps, or narrowband non-harmonic transients are recorded as `out_of_target` rather than assumed to be pick attacks or another instrument.
- Missing harmonic structure is never filled with generic noise.
- Repeated strikes within a sustained pitch (`re_picks`) and multi-harmonic overlapping ringing (`overlapping_ring_notes`) are detected automatically when spectrogram evidence supports them, and `re_picks` remains editable in `CUES.json` (`[{"offset_s":0.3,"strength":0.38}]`, or `[]` to force the persistent-string model with a single initial strike).

## Post-dry-review variant exports

Only after the dry performance passes review, `export-guitar-variants` (and the desktop app's guitar export panel) exports one, several (comma-separated), or `all` unmastered variants into a fresh directory:

- `original`: unchanged copy of the verified source WAV (SHA-256 verified against the input; source is never overwritten).
- `natural`: clean amp-to-mic capture using a labeled approximation of an SM7-class dynamic cardioid microphone at ~2 ft in a padded 10 × 10 × 9 ft room.
- `digital`: clean direct/modeled capture path.
- `analog`: distinct 2×-oversampled asymmetric triode stage with supply sag, passive tone stack, and modeled booth capture (no synthetic hum and not a generic EQ preset).
- `expressive`: alters selected performance gestures (timing rubato, accent dynamics, vibrato/bend shaping, and sustained-note re-picks) in the cue packet *before* dry synthesis and capture, not merely via post-effects.

The guitar path remains experimental: automated measurements (envelope correlation, independent pitch agreement without octave folding, duration, headroom, and bit-identical reruns) do not replace human listening. Any unreviewed render is a **candidate**, not a certified listening pass.

