# Native guitar rebuild
From this crate, use the Rust executable:
- spectral-canvas-mono analyze-guitar INPUT.wav CUES.json
- spectral-canvas-mono render-dry-guitar CUES.json OUTPUT.wav
- spectral-canvas-mono rebuild-dry-guitar INPUT.wav OUTPUT.wav

Input: predominantly monophonic mono guitar WAV, 8–96 kHz, up to 90 seconds.
The second command requires only cue JSON; the third runs both stages in one
process. None requires an interpreter. Output is unmastered mono float WAV.
The guitar path is experimental: successful rendering does not certify phrasing,
articulations, timbre, or full feature parity with retired experiments.

For an editable, opt-in persistent-string event, set `re_picks` to
`[{"offset_s":0.3,"strength":0.38}]` on a note, or to `[]` to enable the
model with one initial strike and no re-picks. Offset is relative to the
note's start; the existing pitch curve continues to drive the ringing
modes. Automatic source analysis does not assign those re-pick cues yet.
