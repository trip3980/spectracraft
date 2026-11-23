# SpectraCraft — Technical White Paper

**Performance-guided audio reconstruction · November 2026**

> **Status:** Rust source handoff completed (`spectracraft-mono` v0.1.0, zero Python runtime or QA dependencies). The electric-guitar reconstruction path is experimental. The full performance has not passed a human listening and fidelity review; automated outputs are unverified candidates.

---

## 01 / Executive Summary

### Same performance, fresh recording

Use the recording to understand what the player did. Generate a new take from that interpretation instead of sending back a cleaned copy.

A guitar can remain recognizable through hum, clicks, or the watery artifacts of a separated stem. The timing, slides, accents, and ringing may still be audible even when the recorded waveform is a poor candidate for restoration. SpectraCraft asks a different question: **What musical action best explains the evidence, and what would that action sound like through a clean recording chain?**

### Render the stroke, not the pixels

A spectrogram is an evidence map, not the output. A faint horizontal line can be the tail of a note. A diagonal trace can be a bend or slide. A short broadband mark may be part of the attack or something unrelated. The engine looks for structure over time, records what it can support, and renders the gesture as a whole. It should not turn every bright region into a note or fill every uncertain region with noise.

### A target, not a universal stem splitter

The operator first chooses a target, such as the guitar. Energy that does not support that target can be left out of a rebuild without calling it noise. The program does not have to name every instrument in a mix before it can begin. It also cannot uniquely identify a performer, a string fingering, or a recording chain from a spectrogram alone.

### Three jobs that must stay separate

| Job | What changes | Boundary |
| --- | --- | --- |
| **Reconstruction** | Synthesizes new audio from editable performance cues. | The dry renderer (`render-dry-guitar`) reads only the saved cue packet — zero source PCM and zero source phase. |
| **Local repair** | Replaces an explicit damaged interval (`0.25–4.0 ms`) while preserving unaffected samples bit-for-bit. | A separate Rust tool (`repair-gap`), not a whole-track rebuild; refuses to write output if context or bounds are not met. |
| **Capture** | Changes the modeled path from the rendered dry source to the recorded signal (`Natural`, `Digital`, `Analog`, `Expressive`). | Capture should never be used to mask or fix a wrong musical gesture. |

---

## 02 / System Design

### From evidence to sound

```text
Original WAV  -->  Analyze  -->  Edit cues  -->  Synthesize dry  -->  Capture
  (read-only)      (multi-scale)  (re-picks,       (source-free        (post-review
                    evidence)      levels,          modal string)       finishes)
                                   uncertain)
```

1. **Read the recording at more than one scale**
   Measure energy, pitch tracks, attack timing, and decay. A short window gives better time localization at an attack; a longer window separates nearby frequency components. Operators may raise **display-only contrast** to inspect weak ridges, without applying that display gain to the audio PCM. A coherent pattern across frames and harmonics carries more weight than brightness in one bin.
2. **Keep competing explanations visible**
   Separate `supported`, `uncertain`, `out_of_target`, and `silence` evidence explicitly. The Rust layer workflow exposes `Pitch`, `Ringing`, `Texture`, and `Breath Accent` as independently auditionable stems. The pitch estimator and independent pitch verifier retain competing hypotheses and abstain when the signal does not settle the question. A strong second harmonic is not proof the played note is an octave higher; conversely, octave folding must not conceal a genuine octave error.
3. **Turn observations into an editable performance**
   For the guitar path, the saved JSON cue packet describes timing, pitch over time, levels, and string/pick controls. Explicit `re_picks` inject fresh energy into a ringing string without resetting modal phase. Uncertain regions remain inspectable so the operator can revise cues instead of accepting silent guesswork.
4. **Synthesize dry audio from cues alone**
   Dry reconstruction uses a persistent modal string bank driven strictly by the cue packet:

   $$y_{\text{dry}}[n] = \sum_{e} \sum_{k=1}^{K} a_{e,k}[n]\,\sin\!\big(\phi_{e,k}[n]\big)$$

   where $e$ indexes active string events, $k$ indexes harmonic partials with inharmonicity and frequency-dependent damping, and $a_{e,k}[n], \phi_{e,k}[n]$ evolve continuously across slides, bends, overlapping ringing, and re-pick excitations.
5. **Apply capture only after the dry take is approved**
   Capture models the recording chain downstream of the dry signal:

   $$y_{\text{capture}}[n] = \big(h_{\text{room}} * h_{\text{mic}} * g_{\text{amp}}(y_{\text{dry}})\big)[n]$$

   `Natural` approximates an SM7-class cardioid dynamic microphone at ~2 ft in a padded 10 x 10 x 9 ft room; `Digital` provides a clean direct modeled path; `Analog` adds 2x oversampled asymmetric triode nonlinearity, dynamic supply sag, passive tone stack, and close booth coloration (zero synthetic hum); `Expressive` mutates performance phrasing cues prior to synthesis and renders through the Natural capture path; `Original` is always a byte-for-byte SHA-256-verified copy of the unchanged source.

---

## 03 / Product & Workflow

1. **Non-destructive provenance:** Source WAVs are read-only and verified unchanged by SHA-256. Every output uses `create_new` semantics into fresh files/directories.
2. **Dry-first verification:** Run `spectracraft-qa guitar-verify` to produce the cue packet, source-free dry candidate, level-matched A/B sequence, bit-identical cue-only rerun verification, independent harmonic-comb pitch check (unfolded), and locked-scale waveforms/spectrograms.
3. **Editable layers & local repair:** Use `analyze-layers` / `render-layers` for isolated 4-stem synthesis (`pitch.wav`, `ring.wav`, `texture.wav`, `accent.wav`, `mix.wav`) with fixed `-3.01 dB` audition headroom, or `repair-gap` for bounded `0.25–4.0 ms` explicit interval repair when unaffected PCM must remain untouched.
4. **Approval before finish:** Review and edit the dry performance first. Only after dry approval select individual variants or `all` (`Original`, `Natural`, `Digital`, `Analog`, `Expressive`) via `spectracraft-qa guitar-variants` or the native Rust desktop app (`spectracraft-desktop`). All exports remain unmastered with editing headroom.

---

## 04 / Measured Evidence

### Reference Track

- **Recording:** David Monniaux — *Slow Solo* (40.311 s, 44.1 kHz mono electric guitar), Wikimedia Commons `File:Slow_solo_20110108.ogg` (CC BY-SA 3.0 / GFDL), decoded to float32 WAV (`SHA-256: d2ce5561cd9ff7f6c6dae9c569888c228ac3f7e419475c0fc2b01531ec5c6706`).
- **Verification discipline:** All pitch checks are run on an independent 10 ms harmonic-comb verifier with **no octave folding** and **explicit unresolved accounting**. The ambiguous transition near 25.0–26.0 s is left unedited.

| Metric / Check | Nov 2026 Handoff Checkpoint | Current Verified Rust Build |
| --- | --- | --- |
| Automated Rust test suite | 43 core + 4 Linux desktop tests | **55** core/bench/guitar + **5** Linux desktop tests (`cargo test --locked`) |
| Full-track envelope correlation (40.311 s) | 0.7132 (baseline) | **0.9750** |
| 20–27 s envelope correlation | 0.805 -> 0.959 (0.7959 baseline) | **0.9717** |
| Full-track pitch agreement (resolved, <= 100c, unfolded) | — | **2,985 / 3,259** (772 unresolved) |
| Full-track octave errors (unfolded) | 260 (baseline) | **21** |
| 20–27 s pitch agreement (resolved, <= 100c, unfolded) | 312 / 700 frames (388 unresolved) | **480 / 589** resolved (111 unresolved) |
| 20–27 s octave errors (unfolded) | 104 (baseline) | **6** |
| 25.0–26.0 s ambiguous transition (unedited) | Ambiguous / retained | **201 / 201** frames retain competing hypotheses; **7** abstained |
| Source-free cue-only rerun determinism | Bit-identical | **Bit-identical** (`SHA-256: 4f21c0121d1c663f...`) |
| Dry peak / RMS headroom | Safe headroom | **-2.95 dBFS peak** / **-18.12 dBFS RMS** (source -18.04 dBFS) |
| Zero-Python migration & source inventory | Verified | **38 / 38** retired entries replaced; **34** `.rs` files only |

### What these measurements do NOT prove

- They are **not** note-accuracy grades or a human listening verdict.
- Automated builds in a headless Linux container **cannot listen** to audio or test `/dev/snd` hardware playback.
- A Linux x86_64 test pass does **not** verify Windows, macOS, iOS, or Android builds or audio drivers.
- `cargo audit` was skipped because `cargo-audit` is not installed in this environment.
- Every generated WAV remains an **experimental candidate** until a human listener reviews it.

---

## 05 / Scope & Technical References

### What SpectraCraft is not

- Not a universal blind source separator or multi-instrument stem splitter.
- Not a black-box neural denoiser that overwrites or smears the original waveform.
- Not a guarantee of physical string fingering, performer identity, or exact historical gear recovery from a spectrogram alone.
- Not a mastering limiter or loudness maximizer; candidate exports intentionally preserve editing headroom.
- Not a Python wrapper — 100% of DSP, analysis, synthesis, repair, plotting, and QA execute in native Rust.

### Technical References

1. **R. J. McAulay and T. F. Quatieri**, "Speech analysis/synthesis based on a sinusoidal representation," *IEEE Transactions on Acoustics, Speech, and Signal Processing*, vol. 34, no. 4, pp. 744–754, Aug. 1986.
2. **X. Serra and J. O. Smith**, "Spectral modeling synthesis: A sound analysis/synthesis system based on a deterministic plus stochastic decomposition," *Computer Music Journal*, vol. 14, no. 4, pp. 12–24, 1990.
3. **K. Karplus and A. Strong**, "Digital synthesis of plucked-string and drum timbres," *Computer Music Journal*, vol. 7, no. 2, pp. 43–55, 1983; **J. O. Smith**, "Physical modeling using digital waveguides," *Computer Music Journal*, vol. 16, no. 4, pp. 74–91, 1992.
4. **P. McLeod and G. Wyvill**, "A smarter way to find pitch," *Proceedings of the International Computer Music Conference (ICMC)*, pp. 138–141, 2005 (Normalized Square Difference Function, NSDF).
5. **J. B. Allen and D. A. Berkley**, "Image method for efficiently simulating small-room acoustics," *Journal of the Acoustical Society of America*, vol. 65, no. 4, pp. 943–950, Apr. 1979.
