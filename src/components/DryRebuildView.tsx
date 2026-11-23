import React, { useMemo, useState } from 'react';
import { Download, Volume2 } from 'lucide-react';
import {
  formatDbfs,
  formatDuration,
  formatHz,
  shortHash,
  type SessionPayload,
} from '../lib/workbench';

interface DryRebuildViewProps {
  session: SessionPayload;
  onResetToReference: () => void;
  onGoToVariants: () => void;
}

type PlotKind = 'waveforms' | 'spectrograms';
type WindowKind = 'full' | 'detail' | 'transition';

export function DryRebuildView({
  session,
  onResetToReference,
  onGoToVariants,
}: DryRebuildViewProps) {
  const [plotKind, setPlotKind] = useState<PlotKind>('waveforms');
  const [windowKind, setWindowKind] = useState<WindowKind>('full');
  const [evidenceFilter, setEvidenceFilter] = useState<'all' | 'uncertain' | 'out_of_target'>('all');
  const [displayContrast, setDisplayContrast] = useState<number>(100);

  const v = session.verification;
  const fullPitch = v.independent_pitch_full;
  const detailPitch = v.independent_pitch_20_27s;
  const summary = v.cue_summary;

  const activeSvgUrl = useMemo(() => {
    if (plotKind === 'waveforms') {
      if (windowKind === 'detail' && session.files.waveformsDetail) {
        return session.files.waveformsDetail;
      }
      return session.files.waveformsFull;
    }
    if (windowKind === 'transition' && session.files.spectrogramsTransition) {
      return session.files.spectrogramsTransition;
    }
    if (windowKind === 'detail' && session.files.spectrogramsDetail) {
      return session.files.spectrogramsDetail;
    }
    return session.files.spectrogramsFull;
  }, [plotKind, windowKind, session.files]);

  const filteredTransition = useMemo(() => {
    const frames = session.transitionEvidence;
    if (evidenceFilter === 'all') return frames.slice(0, 60);
    return frames.filter((f) => f.status === evidenceFilter).slice(0, 60);
  }, [session.transitionEvidence, evidenceFilter]);

  return (
    <div className="space-y-8">
      <section className="border-b border-slate-200 pb-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="max-w-3xl space-y-2">
            <p className="text-xs font-medium text-slate-500">
              Native Rust Engine · Source-Free Cue Synthesis · Candidate Audio Pending Human Review
            </p>
            <h1 className="text-2xl font-bold tracking-tight text-slate-900 text-balance">
              {session.label}
            </h1>
            <p className="text-sm text-slate-600">
              Dry performance rebuilt strictly from saved cue JSON (zero source PCM or source phase).
              Original WAV is read-only and verified unchanged via SHA-256.
            </p>
            {session.attribution && (
              <p className="text-xs text-slate-500">{session.attribution}</p>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-3">
            {!session.isReference && (
              <button
                type="button"
                onClick={onResetToReference}
                className="whitespace-nowrap rounded-md border border-slate-300 bg-white px-3.5 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
              >
                Load Slow Solo Reference
              </button>
            )}
            <a
              href={`${session.files.cuesJson}?download=1`}
              className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-md border border-slate-300 bg-white px-3.5 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
            >
              <Download className="h-4 w-4" aria-hidden="true" />
              Download Cues JSON
            </a>
            <button
              type="button"
              onClick={onGoToVariants}
              className="whitespace-nowrap rounded-md bg-blue-700 px-4 py-2 text-sm font-medium text-white hover:bg-blue-800"
            >
              Proceed to Variant Exports
            </button>
          </div>
        </div>
      </section>

      {/* Three Jobs Separate Strip */}
      <section
        aria-label="Architectural job boundaries"
        className="grid grid-cols-1 gap-3 md:grid-cols-3"
      >
        <div className="rounded-md border border-slate-200 bg-white px-4 py-3">
          <p className="text-xs font-semibold text-slate-900">1. Reconstruction (Active View)</p>
          <p className="mt-1 text-xs text-slate-600">
            Synthesizes fresh dry audio from editable cues alone — zero source PCM or source phase.
          </p>
        </div>
        <div className="rounded-md border border-slate-200 bg-white px-4 py-3">
          <p className="text-xs font-semibold text-slate-900">2. Local Repair (Separate Tool)</p>
          <p className="mt-1 text-xs text-slate-600">
            Replaces explicit 0.25–4.00 ms loss intervals in <code className="font-mono">Layers &amp; Repair</code> while preserving unaffected PCM.
          </p>
        </div>
        <div className="rounded-md border border-slate-200 bg-white px-4 py-3">
          <p className="text-xs font-semibold text-slate-900">3. Capture (Post-Review Only)</p>
          <p className="mt-1 text-xs text-slate-600">
            Applies modeled amp, mic, and room finishes in <code className="font-mono">Variants</code> only after dry cues are approved.
          </p>
        </div>
      </section>

      {/* Primary KPI Strip */}
      <section aria-label="Dry verification summary" className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <div className="rounded-lg border border-slate-200 bg-white p-4">
          <p className="text-xs font-medium text-slate-500">Full Envelope Correlation</p>
          <p className="mt-1 font-mono text-2xl font-semibold tabular-nums text-slate-900">
            {fullPitch.envelope_correlation !== null
              ? fullPitch.envelope_correlation.toFixed(4)
              : '—'}
          </p>
          <p className="mt-1 font-mono text-xs tabular-nums text-slate-500">
            {detailPitch?.envelope_correlation != null
              ? `20–27 s: ${detailPitch.envelope_correlation.toFixed(4)}`
              : `Duration: ${formatDuration(v.duration_s)}`}
          </p>
        </div>

        <div className="rounded-lg border border-slate-200 bg-white p-4">
          <p className="text-xs font-medium text-slate-500">Independent Pitch Agreement</p>
          <p className="mt-1 font-mono text-2xl font-semibold tabular-nums text-slate-900">
            {fullPitch.agreed_within_100_cents}
            <span className="ml-1.5 text-sm font-normal text-slate-500">
              / {fullPitch.both_resolved_frames} resolved
            </span>
          </p>
          <p className="mt-1 font-mono text-xs tabular-nums text-slate-500">
            Octave errors: {fullPitch.octave_errors} · Unresolved: {fullPitch.unresolved_frames}
          </p>
        </div>

        <div className="rounded-lg border border-slate-200 bg-white p-4">
          <p className="text-xs font-medium text-slate-500">Dry Candidate Headroom</p>
          <p className="mt-1 font-mono text-2xl font-semibold tabular-nums text-slate-900">
            {formatDbfs(v.dry_peak_dbfs)}
          </p>
          <p className="mt-1 font-mono text-xs tabular-nums text-slate-500">
            RMS {formatDbfs(v.dry_rms_dbfs)} · A/B match {v.ab_dry_match_gain_db >= 0 ? '+' : ''}
            {v.ab_dry_match_gain_db.toFixed(2)} dB
          </p>
        </div>

        <div className="rounded-lg border border-slate-200 bg-white p-4">
          <p className="text-xs font-medium text-slate-500">Determinism &amp; Integrity</p>
          <p className="mt-1 text-lg font-semibold text-slate-900">
            {v.cue_only_rerun_identical && v.source_unchanged
              ? 'Bit-Identical Cue Rerun'
              : 'Verification Attention Required'}
          </p>
          <p className="mt-1 font-mono text-xs tabular-nums text-slate-500">
            Dry SHA-256: {shortHash(v.dry_candidate_sha256)}
          </p>
        </div>
      </section>

      {/* Audition Deck */}
      <section aria-label="Audio audition deck" className="rounded-lg border border-slate-200 bg-white p-5">
        <div className="mb-4 flex flex-wrap items-baseline justify-between gap-2">
          <div>
            <h2 className="text-base font-semibold text-slate-900">
              Candidate Audio Audition
            </h2>
            <p className="text-xs text-slate-500">
              All WAV files are streamed directly from the Rust verification outputs. Automated runs
              cannot listen; audition these tracks directly before approving the dry performance.
            </p>
          </div>
          <span className="font-mono text-xs tabular-nums text-slate-500">
            {v.sample_rate} Hz · {v.samples.toLocaleString()} samples · {formatDuration(v.duration_s)}
          </span>
        </div>

        <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
          <div className="rounded-md border border-slate-200 bg-slate-50 p-4">
            <div className="mb-2 flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-sm font-semibold text-slate-900">
                <Volume2 className="h-4 w-4 text-slate-600" aria-hidden="true" />
                Original Reference (Unchanged)
              </div>
              <a
                href={`${session.files.sourceWav}?download=1`}
                className="text-xs font-medium text-blue-700 hover:underline"
              >
                Download
              </a>
            </div>
            <p className="mb-3 font-mono text-xs tabular-nums text-slate-500">
              Peak {formatDbfs(v.source_peak_dbfs)} · RMS {formatDbfs(v.source_rms_dbfs)}
            </p>
            <audio
              controls
              preload="metadata"
              src={session.files.sourceWav}
              className="w-full"
              aria-label="Original Reference audio player"
            />
          </div>

          <div className="rounded-md border border-slate-200 bg-slate-50 p-4">
            <div className="mb-2 flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-sm font-semibold text-slate-900">
                <Volume2 className="h-4 w-4 text-blue-700" aria-hidden="true" />
                Dry Rebuild Candidate (Cue-Only)
              </div>
              <a
                href={`${session.files.dryWav}?download=1`}
                className="text-xs font-medium text-blue-700 hover:underline"
              >
                Download
              </a>
            </div>
            <p className="mb-3 font-mono text-xs tabular-nums text-slate-500">
              Peak {formatDbfs(v.dry_peak_dbfs)} · RMS {formatDbfs(v.dry_rms_dbfs)}
            </p>
            <audio
              controls
              preload="metadata"
              src={session.files.dryWav}
              className="w-full"
              aria-label="Dry Rebuild Candidate audio player"
            />
          </div>

          <div className="rounded-md border border-slate-200 bg-slate-50 p-4">
            <div className="mb-2 flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 text-sm font-semibold text-slate-900">
                <Volume2 className="h-4 w-4 text-slate-600" aria-hidden="true" />
                Level-Matched A/B Sequence
              </div>
              <a
                href={`${session.files.abWav}?download=1`}
                className="text-xs font-medium text-blue-700 hover:underline"
              >
                Download
              </a>
            </div>
            <p className="mb-3 font-mono text-xs tabular-nums text-slate-500">
              Original then 0.8 s silence then matched Dry ({v.ab_dry_match_gain_db >= 0 ? '+' : ''}
              {v.ab_dry_match_gain_db.toFixed(2)} dB)
            </p>
            <audio
              controls
              preload="metadata"
              src={session.files.abWav}
              className="w-full"
              aria-label="Level-Matched A/B Sequence audio player"
            />
          </div>
        </div>
      </section>

      {/* Same-Scale Rust SVG Plots */}
      <section aria-label="Same-scale waveforms and spectrograms" className="rounded-lg border border-slate-200 bg-white p-5">
        <div className="mb-4 flex flex-wrap items-center justify-between gap-4">
          <div>
            <h2 className="text-base font-semibold text-slate-900">
              Same-Scale Rust Waveforms &amp; Spectrograms
            </h2>
            <p className="text-xs text-slate-500">
              Rendered natively in Rust with locked amplitude and dBFS color scales across panels.
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-3">
            <div className="flex items-center gap-1 rounded-md bg-slate-100 p-1">
              <button
                type="button"
                onClick={() => {
                  setPlotKind('waveforms');
                  if (windowKind === 'transition') setWindowKind('full');
                }}
                className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium transition-colors ${
                  plotKind === 'waveforms'
                    ? 'bg-white text-slate-900 shadow-xs'
                    : 'text-slate-600 hover:text-slate-900'
                }`}
              >
                Waveforms
              </button>
              <button
                type="button"
                onClick={() => setPlotKind('spectrograms')}
                className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium transition-colors ${
                  plotKind === 'spectrograms'
                    ? 'bg-white text-slate-900 shadow-xs'
                    : 'text-slate-600 hover:text-slate-900'
                }`}
              >
                Spectrograms
              </button>
            </div>

            <div className="flex items-center gap-1 rounded-md bg-slate-100 p-1">
              <button
                type="button"
                onClick={() => setWindowKind('full')}
                className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium transition-colors ${
                  windowKind === 'full'
                    ? 'bg-white text-slate-900 shadow-xs'
                    : 'text-slate-600 hover:text-slate-900'
                }`}
              >
                Full Track
              </button>
              {session.files.waveformsDetail && (
                <button
                  type="button"
                  onClick={() => setWindowKind('detail')}
                  className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium transition-colors ${
                    windowKind === 'detail'
                      ? 'bg-white text-slate-900 shadow-xs'
                      : 'text-slate-600 hover:text-slate-900'
                  }`}
                >
                  20–27 s Detail
                </button>
              )}
              {plotKind === 'spectrograms' && session.files.spectrogramsTransition && (
                <button
                  type="button"
                  onClick={() => setWindowKind('transition')}
                  className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium transition-colors ${
                    windowKind === 'transition'
                      ? 'bg-white text-slate-900 shadow-xs'
                      : 'text-slate-600 hover:text-slate-900'
                  }`}
                >
                  25–26 s Transition
                </button>
              )}
            </div>
          </div>
        </div>

        {plotKind === 'spectrograms' && (
          <div className="mb-3 flex flex-wrap items-center justify-between gap-3 rounded-md border border-slate-200 bg-slate-50 px-3.5 py-2">
            <div className="flex flex-wrap items-center gap-3">
              <label htmlFor="display-contrast-slider" className="text-xs font-medium text-slate-800">
                Display-Only Contrast
              </label>
              <input
                id="display-contrast-slider"
                type="range"
                min="100"
                max="240"
                step="10"
                value={displayContrast}
                onChange={(e) => setDisplayContrast(Number(e.target.value))}
                className="w-36 accent-blue-700"
              />
              <span className="font-mono text-xs tabular-nums text-slate-700">
                {displayContrast}%
              </span>
              {displayContrast !== 100 && (
                <button
                  type="button"
                  onClick={() => setDisplayContrast(100)}
                  className="text-xs font-medium text-blue-700 hover:underline"
                >
                  Reset
                </button>
              )}
            </div>
            <p className="text-xs text-slate-500">
              Visual inspection aid for weak ridges — never applies gain to audio PCM.
            </p>
          </div>
        )}

        <div className="svg-viewport overflow-x-auto rounded-md border border-slate-200 bg-white p-2">
          <img
            src={activeSvgUrl}
            alt={`Same-scale ${plotKind} comparison (${windowKind} window)`}
            className="min-w-[680px] rounded"
            style={
              plotKind === 'spectrograms' && displayContrast !== 100
                ? { filter: `contrast(${displayContrast}%)` }
                : undefined
            }
          />
        </div>
      </section>

      {/* Independent Pitch & Evidence Separation Tables */}
      <section className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <div className="rounded-lg border border-slate-200 bg-white p-5">
          <h2 className="text-base font-semibold text-slate-900">
            Independent Pitch Check (No Octave Folding)
          </h2>
          <p className="mt-1 text-xs text-slate-500">
            Separate 10 ms harmonic-comb verifier. Unresolved frames are counted explicitly as
            unresolved — never as passes — and octave errors are never folded.
          </p>

          <div className="mt-4 overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead>
                <tr className="border-b border-slate-200 text-xs font-medium text-slate-500">
                  <th className="py-2 pr-3">Metric</th>
                  <th className="py-2 px-3 text-right">Full Track</th>
                  {detailPitch && <th className="py-2 pl-3 text-right">20–27 s Window</th>}
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100 font-mono text-xs tabular-nums">
                <tr>
                  <td className="py-2 pr-3 font-sans text-slate-700">Total 10 ms frames</td>
                  <td className="py-2 px-3 text-right text-slate-900">{fullPitch.total_frames}</td>
                  {detailPitch && (
                    <td className="py-2 pl-3 text-right text-slate-900">{detailPitch.total_frames}</td>
                  )}
                </tr>
                <tr>
                  <td className="py-2 pr-3 font-sans text-slate-700">Both resolved</td>
                  <td className="py-2 px-3 text-right text-slate-900">{fullPitch.both_resolved_frames}</td>
                  {detailPitch && (
                    <td className="py-2 pl-3 text-right text-slate-900">{detailPitch.both_resolved_frames}</td>
                  )}
                </tr>
                <tr>
                  <td className="py-2 pr-3 font-sans text-slate-700">Agreed (&le; 100 cents)</td>
                  <td className="py-2 px-3 text-right font-semibold text-slate-900">
                    {fullPitch.agreed_within_100_cents}
                  </td>
                  {detailPitch && (
                    <td className="py-2 pl-3 text-right font-semibold text-slate-900">
                      {detailPitch.agreed_within_100_cents}
                    </td>
                  )}
                </tr>
                <tr>
                  <td className="py-2 pr-3 font-sans text-slate-700">Octave errors (unfolded)</td>
                  <td className="py-2 px-3 text-right text-amber-700">{fullPitch.octave_errors}</td>
                  {detailPitch && (
                    <td className="py-2 pl-3 text-right text-amber-700">{detailPitch.octave_errors}</td>
                  )}
                </tr>
                <tr>
                  <td className="py-2 pr-3 font-sans text-slate-700">Disagreed (non-octave)</td>
                  <td className="py-2 px-3 text-right text-slate-700">{fullPitch.disagreed_non_octave}</td>
                  {detailPitch && (
                    <td className="py-2 pl-3 text-right text-slate-700">{detailPitch.disagreed_non_octave}</td>
                  )}
                </tr>
                <tr>
                  <td className="py-2 pr-3 font-sans text-slate-700">Unresolved (total)</td>
                  <td className="py-2 px-3 text-right text-slate-700">{fullPitch.unresolved_frames}</td>
                  {detailPitch && (
                    <td className="py-2 pl-3 text-right text-slate-700">{detailPitch.unresolved_frames}</td>
                  )}
                </tr>
                <tr>
                  <td className="py-2 pr-3 font-sans text-slate-500">Unresolved breakdown (ref / dry / both)</td>
                  <td className="py-2 px-3 text-right text-slate-500">
                    {fullPitch.unresolved_reference} / {fullPitch.unresolved_dry} / {fullPitch.unresolved_both}
                  </td>
                  {detailPitch && (
                    <td className="py-2 pl-3 text-right text-slate-500">
                      {detailPitch.unresolved_reference} / {detailPitch.unresolved_dry} / {detailPitch.unresolved_both}
                    </td>
                  )}
                </tr>
                <tr>
                  <td className="py-2 pr-3 font-sans text-slate-700">Envelope correlation</td>
                  <td className="py-2 px-3 text-right font-semibold text-slate-900">
                    {fullPitch.envelope_correlation?.toFixed(4) ?? '—'}
                  </td>
                  {detailPitch && (
                    <td className="py-2 pl-3 text-right font-semibold text-slate-900">
                      {detailPitch.envelope_correlation?.toFixed(4) ?? '—'}
                    </td>
                  )}
                </tr>
              </tbody>
            </table>
          </div>
        </div>

        <div className="rounded-lg border border-slate-200 bg-white p-5">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <div>
              <h2 className="text-base font-semibold text-slate-900">
                Evidence Separation &amp; Competing Hypotheses
              </h2>
              <p className="mt-1 text-xs text-slate-500">
                5 ms frame accounting across {summary.note_count} notes ({summary.total_repicks} re-picks,{' '}
                {summary.overlapping_ring_notes} overlapping-ringing notes).
              </p>
            </div>
          </div>

          <dl className="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
            <div className="rounded-md border border-slate-200 bg-slate-50 p-3">
              <dt className="text-xs text-slate-500">Supported</dt>
              <dd className="mt-0.5 font-mono text-lg font-semibold tabular-nums text-slate-900">
                {summary.supported_frames}
              </dd>
            </div>
            <div className="rounded-md border border-slate-200 bg-slate-50 p-3">
              <dt className="text-xs text-slate-500">Uncertain</dt>
              <dd className="mt-0.5 font-mono text-lg font-semibold tabular-nums text-amber-700">
                {summary.uncertain_frames}
              </dd>
            </div>
            <div className="rounded-md border border-slate-200 bg-slate-50 p-3">
              <dt className="text-xs text-slate-500">Out of Target</dt>
              <dd className="mt-0.5 font-mono text-lg font-semibold tabular-nums text-slate-700">
                {summary.out_of_target_frames}
              </dd>
            </div>
            <div className="rounded-md border border-slate-200 bg-slate-50 p-3">
              <dt className="text-xs text-slate-500">Silence</dt>
              <dd className="mt-0.5 font-mono text-lg font-semibold tabular-nums text-slate-500">
                {summary.silence_frames}
              </dd>
            </div>
          </dl>

          <div className="mt-5 flex flex-wrap items-center justify-between gap-2">
            <p className="text-xs font-medium text-slate-700">
              {v.transition_25_26s
                ? `25.0–26.0 s Ambiguous Transition (${v.transition_25_26s.frames_with_competing_hypotheses}/${v.transition_25_26s.frames} frames retain competing hypotheses; ${v.transition_25_26s.uncertain} abstained)`
                : 'Sampled Frame Evidence & Competing Hypotheses'}
            </p>
            <div className="flex items-center gap-1 rounded bg-slate-100 p-0.5">
              {(['all', 'uncertain', 'out_of_target'] as const).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  onClick={() => setEvidenceFilter(mode)}
                  className={`whitespace-nowrap rounded px-2 py-1 text-xs font-medium ${
                    evidenceFilter === mode
                      ? 'bg-white text-slate-900 shadow-xs'
                      : 'text-slate-600 hover:text-slate-900'
                  }`}
                >
                  {mode === 'all' ? 'All' : mode === 'uncertain' ? 'Uncertain' : 'Out of Target'}
                </button>
              ))}
            </div>
          </div>

          <div className="mt-2 max-h-56 overflow-y-auto rounded-md border border-slate-200">
            <table className="w-full text-left text-xs">
              <thead className="sticky top-0 bg-slate-50 text-slate-500">
                <tr className="border-b border-slate-200">
                  <th className="px-2.5 py-1.5">Time</th>
                  <th className="px-2.5 py-1.5">Status</th>
                  <th className="px-2.5 py-1.5">Selected</th>
                  <th className="px-2.5 py-1.5">Top Hypotheses</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100 font-mono tabular-nums">
                {filteredTransition.length === 0 ? (
                  <tr>
                    <td colSpan={4} className="px-3 py-4 text-center font-sans text-slate-500">
                      No frames match this filter in the inspected window.
                    </td>
                  </tr>
                ) : (
                  filteredTransition.map((f) => (
                    <tr key={f.time_s}>
                      <td className="px-2.5 py-1.5 text-slate-600">{f.time_s.toFixed(3)} s</td>
                      <td className="px-2.5 py-1.5 font-sans text-slate-800">{f.status}</td>
                      <td className="px-2.5 py-1.5 text-slate-900">{formatHz(f.selected_hz)}</td>
                      <td className="px-2.5 py-1.5 text-slate-600">
                        {f.hypotheses.length > 0
                          ? f.hypotheses
                              .slice(0, 2)
                              .map((h) => `${h.hz.toFixed(1)} Hz (${h.score.toFixed(2)})`)
                              .join(' · ')
                          : f.reason}
                      </td>
                    </tr>
                  ))
                )}
              </tbody>
            </table>
          </div>
        </div>
      </section>
    </div>
  );
}
