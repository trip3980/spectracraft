import React from 'react';

const EVIDENCE_ROWS = [
  {
    metric: 'Automated Rust test suite',
    baseline: '43 core + 4 Linux desktop tests',
    current: '55 core/bench/guitar + 5 Linux desktop tests',
  },
  {
    metric: 'Full-track envelope correlation (40.311 s)',
    baseline: '0.7132 (baseline)',
    current: '0.9750',
  },
  {
    metric: '20–27 s envelope correlation',
    baseline: '0.805 -> 0.959 (0.7959 baseline)',
    current: '0.9717',
  },
  {
    metric: 'Full-track pitch agreement (<= 100c, unfolded)',
    baseline: 'Not reported at handoff',
    current: '2,985 / 3,259 resolved (772 unresolved)',
  },
  {
    metric: 'Full-track octave errors (unfolded)',
    baseline: '260 (baseline)',
    current: '21',
  },
  {
    metric: '20–27 s pitch agreement (<= 100c, unfolded)',
    baseline: '312 / 700 frames (388 unresolved)',
    current: '480 / 589 resolved (111 unresolved)',
  },
  {
    metric: '20–27 s octave errors (unfolded)',
    baseline: '104 (baseline)',
    current: '6',
  },
  {
    metric: '25.0–26.0 s ambiguous transition (unedited)',
    baseline: 'Ambiguous / retained',
    current: '201 / 201 competing hypotheses; 7 abstained',
  },
  {
    metric: 'Cue-only rerun determinism',
    baseline: 'Sample-for-sample identical',
    current: 'Bit-identical (SHA-256 4f21c0121d1c…)',
  },
  {
    metric: 'Dry candidate peak / RMS',
    baseline: 'Headroom preserved',
    current: '-2.95 dBFS peak / -18.12 dBFS RMS',
  },
] as const;

const REFERENCES = [
  {
    id: '[1]',
    citation:
      'R. J. McAulay and T. F. Quatieri, "Speech analysis/synthesis based on a sinusoidal representation," IEEE Transactions on Acoustics, Speech, and Signal Processing, vol. 34, no. 4, pp. 744–754, Aug. 1986.',
  },
  {
    id: '[2]',
    citation:
      'X. Serra and J. O. Smith, "Spectral modeling synthesis: A sound analysis/synthesis system based on a deterministic plus stochastic decomposition," Computer Music Journal, vol. 14, no. 4, pp. 12–24, 1990.',
  },
  {
    id: '[3]',
    citation:
      'K. Karplus and A. Strong, "Digital synthesis of plucked-string and drum timbres," Computer Music Journal, vol. 7, no. 2, pp. 43–55, 1983; J. O. Smith, "Physical modeling using digital waveguides," Computer Music Journal, vol. 16, no. 4, pp. 74–91, 1992.',
  },
  {
    id: '[4]',
    citation:
      'P. McLeod and G. Wyvill, "A smarter way to find pitch," Proceedings of the International Computer Music Conference (ICMC), pp. 138–141, 2005 (Normalized Square Difference Function, NSDF).',
  },
  {
    id: '[5]',
    citation:
      'J. B. Allen and D. A. Berkley, "Image method for efficiently simulating small-room acoustics," Journal of the Acoustical Society of America, vol. 65, no. 4, pp. 943–950, Apr. 1979.',
  },
] as const;

export function WhitePaperSection() {
  return (
    <article className="space-y-8">
      {/* Header */}
      <section className="rounded-lg border border-slate-200 bg-white p-6">
        <p className="font-mono text-xs text-slate-500">
          Technical White Paper · November 2026 · spectracraft-mono v0.1.0
        </p>
        <h2 className="mt-2 text-2xl font-bold tracking-tight text-slate-900">
          SpectraCraft — Performance-Guided Audio Reconstruction
        </h2>
        <p className="mt-3 max-w-3xl text-sm leading-relaxed text-slate-700">
          <strong>Same performance, fresh recording.</strong> Use the recording to understand what
          the player did. Generate a new take from that interpretation instead of sending back a
          cleaned copy.
        </p>
        <p className="mt-3 max-w-3xl text-sm leading-relaxed text-slate-600">
          Status: Rust source handoff completed with zero Python runtime or QA dependencies. The
          electric-guitar reconstruction path is experimental. The full performance has not passed a
          human listening and fidelity review; automated audio outputs remain candidates.
        </p>
      </section>

      {/* 01 Executive Summary */}
      <section className="rounded-lg border border-slate-200 bg-white p-6 space-y-5">
        <div className="border-b border-slate-200 pb-3">
          <p className="font-mono text-xs text-slate-500">01 / EXECUTIVE SUMMARY</p>
          <h3 className="mt-1 text-lg font-semibold text-slate-900">
            Render the Stroke, Not the Pixels
          </h3>
        </div>

        <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
          <div className="space-y-2">
            <h4 className="text-sm font-semibold text-slate-900">
              Evidence map, not destination pixels
            </h4>
            <p className="text-xs leading-relaxed text-slate-600">
              A spectrogram is an evidence map, not the output. A faint horizontal line can be the
              tail of a note. A diagonal trace can be a bend or slide. A short broadband mark may be
              part of the attack or something unrelated. The engine looks for structure over time,
              records what it can support, and renders the gesture as a whole — without turning
              every bright region into a note or filling uncertain regions with generic noise.
            </p>
          </div>

          <div className="space-y-2">
            <h4 className="text-sm font-semibold text-slate-900">
              A target, not a universal stem splitter
            </h4>
            <p className="text-xs leading-relaxed text-slate-600">
              The operator first chooses a target, such as the guitar. Energy that does not support
              that target can be left out of a rebuild without calling it noise. The program does
              not have to name every instrument in a mix before it can begin, and it cannot uniquely
              identify a performer, string fingering, or recording chain from a spectrogram alone.
            </p>
          </div>
        </div>

        <div className="pt-2">
          <h4 className="text-sm font-semibold text-slate-900">
            Three Jobs That Must Stay Separate
          </h4>
          <div className="mt-3 grid grid-cols-1 gap-4 md:grid-cols-3">
            <div className="rounded-md border border-slate-200 bg-slate-50 p-4">
              <p className="text-sm font-semibold text-slate-900">1. Reconstruction</p>
              <p className="mt-1 text-xs leading-relaxed text-slate-600">
                Synthesizes new audio strictly from editable performance cues. The dry renderer (
                <code className="font-mono">render-dry-guitar</code>) does not read original PCM
                samples or source phase.
              </p>
            </div>
            <div className="rounded-md border border-slate-200 bg-slate-50 p-4">
              <p className="text-sm font-semibold text-slate-900">2. Local Repair</p>
              <p className="mt-1 text-xs leading-relaxed text-slate-600">
                Replaces an explicit damaged interval (0.25–4.00 ms) while preserving all unaffected
                samples unchanged. A separate Rust tool (
                <code className="font-mono">repair-gap</code>), never a whole-track rebuild.
              </p>
            </div>
            <div className="rounded-md border border-slate-200 bg-slate-50 p-4">
              <p className="text-sm font-semibold text-slate-900">3. Capture</p>
              <p className="mt-1 text-xs leading-relaxed text-slate-600">
                Changes the modeled path from the rendered dry source to the recorded signal (
                <code className="font-mono">Natural</code>, <code className="font-mono">Digital</code>,{' '}
                <code className="font-mono">Analog</code>, <code className="font-mono">Expressive</code>).
                Capture must never be used to fix a wrong musical gesture.
              </p>
            </div>
          </div>
        </div>
      </section>

      {/* 02 System Design */}
      <section className="rounded-lg border border-slate-200 bg-white p-6 space-y-5">
        <div className="border-b border-slate-200 pb-3">
          <p className="font-mono text-xs text-slate-500">02 / SYSTEM DESIGN</p>
          <h3 className="mt-1 text-lg font-semibold text-slate-900">
            From Evidence to Sound
          </h3>
        </div>

        <p className="rounded-md border border-slate-200 bg-slate-50 px-4 py-2.5 font-mono text-xs text-slate-800">
          Original WAV (read-only) &rarr; Multi-Scale Analysis &rarr; Edit Cues &rarr; Synthesize Dry (cue-only) &rarr; Post-Review Capture
        </p>

        <ol className="space-y-3 text-xs leading-relaxed text-slate-600">
          <li>
            <strong className="text-slate-900">1. Read the recording at more than one scale:</strong>{' '}
            Short windows localize attacks; longer windows separate nearby frequency partials.
            Operators can boost display-only spectrogram contrast to inspect weak ridges without
            applying that display gain to audio PCM.
          </li>
          <li>
            <strong className="text-slate-900">2. Keep competing explanations visible:</strong>{' '}
            Supported, uncertain, out-of-target, and silence evidence stay explicitly separated.
            Pitch diagnostics retain competing hypotheses and abstain when unresolved. A strong
            second harmonic is not proof of an octave jump, and octave folding must never hide a
            genuine octave error.
          </li>
          <li>
            <strong className="text-slate-900">3. Turn observations into an editable performance:</strong>{' '}
            The saved JSON cue packet holds timing, continuous pitch contours, levels, and explicit{' '}
            <code className="font-mono">re_picks</code> that inject energy into a ringing string
            without resetting modal phase. Uncertain-overlap notes are surfaced in the Cue Editor so
            they can be inspected and revised directly.
          </li>
          <li>
            <strong className="text-slate-900">4. Synthesize dry audio &amp; model capture separately:</strong>{' '}
            Dry synthesis runs a persistent modal string bank strictly from cues, followed (only
            after dry review) by optional amp, microphone, and room convolution paths.
          </li>
        </ol>

        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <div className="rounded-md border border-slate-200 bg-slate-50 p-4">
            <p className="text-xs font-medium text-slate-500">Dry Modal String Bank (Cue-Only)</p>
            <p className="mt-2 font-mono text-xs text-slate-900">
              y_dry[n] = &Sigma;_e &Sigma;_(k=1..K) a_(e,k)[n] &middot; sin(&phi;_(e,k)[n])
            </p>
            <p className="mt-2 text-xs text-slate-600">
              Continuous phase integration across bends, slides, overlapping ringing, and explicit
              re-pick strikes with frequency-dependent modal damping.
            </p>
          </div>
          <div className="rounded-md border border-slate-200 bg-slate-50 p-4">
            <p className="text-xs font-medium text-slate-500">Post-Review Capture Chain</p>
            <p className="mt-2 font-mono text-xs text-slate-900">
              y_capture[n] = (h_room * h_mic * g_amp(y_dry))[n]
            </p>
            <p className="mt-2 text-xs text-slate-600">
              Linear direct (<code className="font-mono">Digital</code>), SM7-class cardioid in a
              padded room (<code className="font-mono">Natural</code>), 2x oversampled triode +
              sag + close booth (<code className="font-mono">Analog</code>), or pre-capture gesture
              edits (<code className="font-mono">Expressive</code>).
            </p>
          </div>
        </div>
      </section>

      {/* 03 & 04 Measured Evidence */}
      <section className="rounded-lg border border-slate-200 bg-white p-6 space-y-5">
        <div className="border-b border-slate-200 pb-3">
          <p className="font-mono text-xs text-slate-500">03 &amp; 04 / MEASURED EVIDENCE</p>
          <h3 className="mt-1 text-lg font-semibold text-slate-900">
            David Monniaux — Slow Solo (40.311 s, 44.1 kHz Mono)
          </h3>
          <p className="mt-1 text-xs text-slate-500">
            Wikimedia Commons <code className="font-mono">File:Slow_solo_20110108.ogg</code> (CC
            BY-SA 3.0 / GFDL) · Source SHA-256:{' '}
            <code className="font-mono">d2ce5561cd9ff7f6…1ec5c6706</code>
          </p>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-left text-xs">
            <thead>
              <tr className="border-b border-slate-200 text-slate-500">
                <th className="py-2 pr-4 font-medium">Measurement</th>
                <th className="py-2 px-4 font-medium">Nov 2026 Handoff Checkpoint</th>
                <th className="py-2 pl-4 font-medium">Current Verified Rust Build</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100 font-mono tabular-nums">
              {EVIDENCE_ROWS.map((row) => (
                <tr key={row.metric}>
                  <td className="py-2.5 pr-4 font-sans font-medium text-slate-800">{row.metric}</td>
                  <td className="py-2.5 px-4 text-slate-600">{row.baseline}</td>
                  <td className="py-2.5 pl-4 font-semibold text-slate-900">{row.current}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="rounded-md border border-amber-200 bg-amber-50/70 p-4 text-xs text-amber-950">
          <p className="font-semibold">What these measurements do NOT establish</p>
          <p className="mt-1 leading-relaxed">
            Envelope correlation and harmonic-comb pitch agreement are engineering checks, not note
            accuracy grades or listening verdicts. Automated Linux runs cannot hear audio, have not
            tested <code className="font-mono">/dev/snd</code>, and do not verify Windows, macOS,
            iOS, or Android builds. Every rendered WAV remains a candidate pending human review.
          </p>
        </div>
      </section>

      {/* 05 Scope & Technical References */}
      <section className="rounded-lg border border-slate-200 bg-white p-6 space-y-5">
        <div className="border-b border-slate-200 pb-3">
          <p className="font-mono text-xs text-slate-500">05 / SCOPE &amp; REFERENCES</p>
          <h3 className="mt-1 text-lg font-semibold text-slate-900">
            What SpectraCraft Is Not &amp; Technical Literature
          </h3>
        </div>

        <ul className="list-disc space-y-1.5 pl-5 text-xs leading-relaxed text-slate-600">
          <li>Not a universal blind source separator or all-instrument stem splitter.</li>
          <li>Not a black-box denoiser that modifies or overwrites the original recording.</li>
          <li>Not a unique recovery of string fingering, performer identity, or physical amplifier hardware from a spectrogram alone.</li>
          <li>Not a loudness maximizer or mastering limiter; all exports stay unmastered with editing headroom.</li>
          <li>Not a Python wrapper — 100% of DSP, analysis, synthesis, repair, plotting, and QA stay in Rust.</li>
        </ul>

        <div className="border-t border-slate-200 pt-4">
          <h4 className="text-sm font-semibold text-slate-900">Technical References</h4>
          <ol className="mt-3 space-y-2 text-xs leading-relaxed text-slate-600">
            {REFERENCES.map((ref) => (
              <li key={ref.id} className="flex gap-2.5">
                <span className="shrink-0 font-mono font-semibold text-slate-800">{ref.id}</span>
                <span>{ref.citation}</span>
              </li>
            ))}
          </ol>
        </div>
      </section>
    </article>
  );
}
