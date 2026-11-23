import React, { useState } from 'react';
import { Download } from 'lucide-react';
import {
  ALL_VARIANTS,
  VARIANT_DESCRIPTORS,
  buildVariantSelection,
  formatDbfs,
  shortHash,
  toggleVariantSelection,
  type SessionPayload,
  type VariantKey,
  type VariantsManifest,
} from '../lib/workbench';

interface VariantsViewProps {
  session: SessionPayload;
  editedCueRunId: string | null;
  onVariantsUpdated: (manifest: VariantsManifest) => void;
}

export function VariantsView({
  session,
  editedCueRunId,
  onVariantsUpdated,
}: VariantsViewProps) {
  const [selected, setSelected] = useState<VariantKey[]>([...ALL_VARIANTS]);
  const [dryReviewed, setDryReviewed] = useState<boolean>(false);
  const [useEditedCues, setUseEditedCues] = useState<boolean>(Boolean(editedCueRunId));
  const [exporting, setExporting] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [figureKind, setFigureKind] = useState<'waveforms' | 'spectrograms'>('waveforms');
  const [figureSpan, setFigureSpan] = useState<'full' | 'detail'>('full');

  const manifest = session.variants;
  const selectionString = buildVariantSelection(selected);

  async function handleExport() {
    if (!selectionString) {
      setError('Select at least one export version before running.');
      return;
    }
    setExporting(true);
    setError(null);
    try {
      const resp = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/variants`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          selection: selectionString,
          cuesRunId: useEditedCues && editedCueRunId ? editedCueRunId : undefined,
        }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        throw new Error(data.error || 'Variant export failed.');
      }
      onVariantsUpdated(data as VariantsManifest);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setExporting(false);
    }
  }

  const activeFigureUrl =
    manifest?.figures &&
    (figureKind === 'waveforms'
      ? figureSpan === 'full'
        ? manifest.figures.waveformsFull
        : manifest.figures.waveformsDetail
      : figureSpan === 'full'
        ? manifest.figures.spectrogramsFull
        : manifest.figures.spectrogramsDetail);

  return (
    <div className="space-y-6">
      <section className="border-b border-slate-200 pb-5">
        <div className="max-w-3xl space-y-1">
          <p className="text-xs font-medium text-slate-500">
            Post-Dry-Review Capture &amp; Performance Variants · Unmastered with Editing Headroom
          </p>
          <h1 className="text-2xl font-bold tracking-tight text-slate-900">
            Individual &amp; All Variant Exports
          </h1>
          <p className="text-sm text-slate-600">
            Export any single version, a custom subset, or all five versions into a fresh output
            directory via Rust <code className="font-mono text-xs">spectracraft-qa guitar-variants</code>.
            Original audio is copied byte-for-byte unchanged and verified by SHA-256.
          </p>
        </div>
      </section>

      {/* Selection & Gate Panel */}
      <section className="rounded-lg border border-slate-200 bg-white p-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-base font-semibold text-slate-900">Select Versions to Export</h2>
            <p className="text-xs text-slate-500">
              Active selection string:{' '}
              <code className="font-mono text-slate-800">{selectionString || '(none)'}</code>
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => setSelected([...ALL_VARIANTS])}
              className="whitespace-nowrap rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-50"
            >
              Select All (5)
            </button>
            <button
              type="button"
              onClick={() => setSelected(['natural'])}
              className="whitespace-nowrap rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-50"
            >
              Only Natural
            </button>
            <button
              type="button"
              onClick={() => setSelected([])}
              className="whitespace-nowrap rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-50"
            >
              Clear
            </button>
          </div>
        </div>

        <div className="mt-4 grid grid-cols-1 gap-3 md:grid-cols-2 lg:grid-cols-5">
          {VARIANT_DESCRIPTORS.map((desc) => {
            const checked = selected.includes(desc.key);
            return (
              <label
                key={desc.key}
                className={`flex cursor-pointer flex-col justify-between rounded-md border p-3.5 transition-colors ${
                  checked
                    ? 'border-blue-600 bg-blue-50/40'
                    : 'border-slate-200 bg-slate-50/60 hover:bg-slate-50'
                }`}
              >
                <div>
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-sm font-semibold text-slate-900">{desc.title}</span>
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={() => setSelected((prev) => toggleVariantSelection(prev, desc.key))}
                      className="h-4 w-4 rounded border-slate-300 accent-blue-700"
                    />
                  </div>
                  <p className="mt-1 text-xs font-medium text-slate-700">{desc.subtitle}</p>
                  <p className="mt-1.5 text-xs leading-relaxed text-slate-500">
                    {desc.pathSummary}
                  </p>
                </div>
              </label>
            );
          })}
        </div>

        <div className="mt-5 flex flex-wrap items-center justify-between gap-4 border-t border-slate-200 pt-4">
          <div className="flex flex-wrap items-center gap-5">
            <label className="flex cursor-pointer items-center gap-2 text-xs text-slate-800">
              <input
                type="checkbox"
                checked={dryReviewed}
                onChange={(e) => setDryReviewed(e.target.checked)}
                className="h-4 w-4 rounded border-slate-300 accent-blue-700"
              />
              <span>
                <strong>Dry approval required:</strong> I have reviewed the dry performance first
                (capture models the recording chain and should not fix a wrong musical gesture)
              </span>
            </label>

            {editedCueRunId && (
              <label className="flex cursor-pointer items-center gap-2 text-xs text-slate-700">
                <input
                  type="checkbox"
                  checked={useEditedCues}
                  onChange={(e) => setUseEditedCues(e.target.checked)}
                  className="h-4 w-4 rounded border-slate-300 accent-blue-700"
                />
                Use edited cues from Cue Editor ({editedCueRunId})
              </label>
            )}
          </div>

          <button
            type="button"
            disabled={exporting || !dryReviewed || !selectionString}
            onClick={handleExport}
            className="whitespace-nowrap rounded-md bg-blue-700 px-4 py-2 text-sm font-medium text-white hover:bg-blue-800 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {exporting ? 'Exporting in Rust…' : `Export Selected (${selected.length})`}
          </button>
        </div>

        {error && (
          <p role="alert" className="mt-3 text-sm text-rose-700">
            {error}
          </p>
        )}
      </section>

      {/* Exported Outputs */}
      {manifest && (
        <>
          <section className="rounded-lg border border-slate-200 bg-white p-5">
            <div className="mb-4 flex flex-wrap items-baseline justify-between gap-2">
              <div>
                <h2 className="text-base font-semibold text-slate-900">
                  Exported Unmastered Candidates (Selection: {manifest.selection})
                </h2>
                <p className="text-xs text-slate-500">
                  Source SHA-256: <span className="font-mono">{shortHash(manifest.source_sha256)}</span> ·
                  Source unchanged: {manifest.source_unchanged ? 'Verified' : 'Check failed'} ·
                  Shared waveform scale: &plusmn;{manifest.shared_waveform_scale_linear.toFixed(3)} ·
                  Shared spectrogram scale: {manifest.shared_spectrogram_scale_dbfs[0]} to{' '}
                  {manifest.shared_spectrogram_scale_dbfs[1]} dBFS
                </p>
              </div>
            </div>

            <div className="space-y-3">
              {manifest.exports.map((exp) => (
                <div
                  key={exp.variant}
                  className="grid grid-cols-1 items-center gap-4 rounded-md border border-slate-200 bg-slate-50 p-4 lg:grid-cols-12"
                >
                  <div className="lg:col-span-5">
                    <div className="flex items-baseline gap-2">
                      <h3 className="text-sm font-semibold text-slate-900">{exp.label}</h3>
                    </div>
                    <p className="mt-1 font-mono text-xs tabular-nums text-slate-500">
                      Peak {formatDbfs(exp.peak_dbfs)} · RMS {formatDbfs(exp.rms_dbfs)} · Headroom{' '}
                      {exp.headroom_below_0dbfs_db.toFixed(2)} dB · SHA-256 {shortHash(exp.sha256)}
                      {exp.unchanged_copy_of_original ? ' · Matches Original' : ''}
                    </p>
                  </div>

                  <div className="lg:col-span-5">
                    {exp.url ? (
                      <audio
                        controls
                        preload="metadata"
                        src={exp.url}
                        className="w-full"
                        aria-label={`${exp.variant} audio player`}
                      />
                    ) : (
                      <span className="text-xs text-slate-500">Audio stream unavailable</span>
                    )}
                  </div>

                  <div className="flex justify-end lg:col-span-2">
                    {exp.url && (
                      <a
                        href={`${exp.url}?download=1`}
                        className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-100"
                      >
                        <Download className="h-3.5 w-3.5" aria-hidden="true" />
                        {exp.file}
                      </a>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </section>

          {activeFigureUrl && (
            <section className="rounded-lg border border-slate-200 bg-white p-5">
              <div className="mb-4 flex flex-wrap items-center justify-between gap-4">
                <div>
                  <h2 className="text-base font-semibold text-slate-900">
                    Matching Variant Waveforms &amp; Spectrograms
                  </h2>
                  <p className="text-xs text-slate-500">
                    All exported variants share identical linear amplitude and dBFS color scales.
                  </p>
                </div>
                <div className="flex flex-wrap items-center gap-3">
                  <div className="flex items-center gap-1 rounded-md bg-slate-100 p-1">
                    <button
                      type="button"
                      onClick={() => setFigureKind('waveforms')}
                      className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium ${
                        figureKind === 'waveforms'
                          ? 'bg-white text-slate-900 shadow-xs'
                          : 'text-slate-600 hover:text-slate-900'
                      }`}
                    >
                      Waveforms
                    </button>
                    <button
                      type="button"
                      onClick={() => setFigureKind('spectrograms')}
                      className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium ${
                        figureKind === 'spectrograms'
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
                      onClick={() => setFigureSpan('full')}
                      className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium ${
                        figureSpan === 'full'
                          ? 'bg-white text-slate-900 shadow-xs'
                          : 'text-slate-600 hover:text-slate-900'
                      }`}
                    >
                      Full Track
                    </button>
                    <button
                      type="button"
                      onClick={() => setFigureSpan('detail')}
                      className={`whitespace-nowrap rounded px-3 py-1.5 text-xs font-medium ${
                        figureSpan === 'detail'
                          ? 'bg-white text-slate-900 shadow-xs'
                          : 'text-slate-600 hover:text-slate-900'
                      }`}
                    >
                      Detail ({manifest.detail_window_s[0]}–{manifest.detail_window_s[1]} s)
                    </button>
                  </div>
                </div>
              </div>

              <div className="svg-viewport overflow-x-auto rounded-md border border-slate-200 bg-white p-2">
                <img
                  src={activeFigureUrl}
                  alt={`Variant comparison ${figureKind} (${figureSpan})`}
                  className="min-w-[680px] rounded"
                />
              </div>
            </section>
          )}

          {manifest.technicalReport && (
            <section className="rounded-lg border border-slate-200 bg-white p-5">
              <h2 className="text-base font-semibold text-slate-900">
                Technical Report — What Comparisons Show &amp; Cannot Establish
              </h2>
              <pre className="mt-3 overflow-x-auto whitespace-pre-wrap rounded-md border border-slate-200 bg-slate-50 p-4 font-mono text-xs leading-relaxed text-slate-700">
                {manifest.technicalReport}
              </pre>
            </section>
          )}
        </>
      )}
    </div>
  );
}
