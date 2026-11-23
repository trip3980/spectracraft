import React, { useMemo, useState } from 'react';
import { Download, RotateCcw } from 'lucide-react';
import {
  shortHash,
  validateGapRepairRange,
  type GapRepairResult,
  type SessionPayload,
} from '../lib/workbench';

interface LocalRepairPanelProps {
  session: SessionPayload;
}

export function LocalRepairPanel({ session }: LocalRepairPanelProps) {
  const v = session.verification;
  const defaultStart = v.duration_s >= 22 ? '21.500' : Math.min(0.5, Math.max(0.1, v.duration_s * 0.25)).toFixed(3);

  const [startS, setStartS] = useState<string>(defaultStart);
  const [gapMs, setGapMs] = useState<string>('2.00');
  const [running, setRunning] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<GapRepairResult | null>(null);

  const preview = useMemo(
    () => validateGapRepairRange(v.sample_rate, v.samples, Number(startS), Number(gapMs)),
    [v.sample_rate, v.samples, startS, gapMs],
  );

  async function handleRunRepair() {
    if (!preview.ok) {
      setError(preview.error);
      return;
    }
    setRunning(true);
    setError(null);
    try {
      const resp = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/repair-gap`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ startS: Number(startS), gapMs: Number(gapMs) }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        throw new Error(data.error || 'Rust local gap repair failed.');
      }
      setResult(data as GapRepairResult);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="space-y-6">
      <section className="rounded-lg border border-slate-200 bg-white p-5">
        <div className="mb-4 max-w-3xl space-y-1">
          <h2 className="text-base font-semibold text-slate-900">
            Bounded Local Gap Repair (Rust <code className="font-mono text-xs">repair-gap</code>)
          </h2>
          <p className="text-xs leading-relaxed text-slate-600">
            Replaces only an explicitly identified damaged interval (0.25 to 4.00 ms) using
            surrounding ~48 ms context, while copying every unaffected sample bit-for-bit from the
            read-only source WAV. This is a bounded local repair tool — not whole-track
            reconstruction and not capture processing. If the interval cannot be safely bridged,
            Rust declines and writes no file.
          </p>
        </div>

        <div className="flex flex-wrap items-end gap-4">
          <div>
            <label htmlFor="gap-start-s" className="block text-xs font-medium text-slate-700">
              Interval Start (seconds)
            </label>
            <input
              id="gap-start-s"
              type="number"
              step="0.05"
              min="0.05"
              max={Math.max(0.1, v.duration_s - 0.06).toFixed(3)}
              value={startS}
              onChange={(e) => setStartS(e.target.value)}
              className="mt-1 w-32 rounded-md border border-slate-300 px-2.5 py-1.5 font-mono text-sm tabular-nums"
            />
          </div>

          <div>
            <label htmlFor="gap-width-ms" className="block text-xs font-medium text-slate-700">
              Explicit Loss Width (0.25–4.00 ms)
            </label>
            <input
              id="gap-width-ms"
              type="number"
              step="0.25"
              min="0.25"
              max="4.00"
              value={gapMs}
              onChange={(e) => setGapMs(e.target.value)}
              className="mt-1 w-32 rounded-md border border-slate-300 px-2.5 py-1.5 font-mono text-sm tabular-nums"
            />
          </div>

          <button
            type="button"
            disabled={running || !preview.ok}
            onClick={() => void handleRunRepair()}
            className="whitespace-nowrap rounded-md bg-blue-700 px-4 py-2 text-sm font-medium text-white hover:bg-blue-800 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {running ? 'Running Rust repair-gap…' : 'Run Local Gap Repair'}
          </button>

          {result && (
            <button
              type="button"
              onClick={() => {
                setResult(null);
                setError(null);
              }}
              className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-md border border-slate-300 bg-white px-3 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
            >
              <RotateCcw className="h-3.5 w-3.5" aria-hidden="true" />
              Clear Result
            </button>
          )}
        </div>

        <div className="mt-3 font-mono text-xs tabular-nums text-slate-600">
          {preview.ok ? (
            <span>
              Exact target span: frames [{preview.startFrame.toLocaleString()},{' '}
              {preview.endFrame.toLocaleString()}) · {preview.repairedFrames} frames at{' '}
              {v.sample_rate} Hz · Context guard: &ge; {preview.contextFrames} frames (48 ms) on
              both sides
            </span>
          ) : (
            <span className="font-sans text-amber-800">{preview.error}</span>
          )}
        </div>

        {error && (
          <p role="alert" className="mt-3 text-sm text-rose-700">
            {error}
          </p>
        )}
      </section>

      {result ? (
        <section
          aria-label="Local gap repair output"
          className="rounded-lg border border-slate-200 bg-white p-5"
        >
          <div className="flex flex-wrap items-baseline justify-between gap-3 border-b border-slate-200 pb-4">
            <div>
              <h3 className="text-base font-semibold text-slate-900">
                {result.status === 'filled'
                  ? 'Local Repair Completed (Bounded Candidate WAV)'
                  : 'Local Repair Declined by Rust Safety Gate'}
              </h3>
              <p className="mt-1 font-mono text-xs tabular-nums text-slate-600">
                Span: {result.startS.toFixed(4)} s (+{result.gapMs.toFixed(2)} ms · frames [
                {result.startFrame.toLocaleString()}, {result.endFrame.toLocaleString()})) · Source
                unchanged: {result.unaffectedPreserved ? 'Verified via SHA-256' : 'Check failed'}
              </p>
            </div>
            {result.repairedWavUrl && (
              <a
                href={`${result.repairedWavUrl}?download=1`}
                className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-50"
              >
                <Download className="h-3.5 w-3.5" aria-hidden="true" />
                Download Repaired WAV
              </a>
            )}
          </div>

          <dl className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-3 font-mono text-xs tabular-nums">
            <div className="rounded-md border border-slate-200 bg-slate-50 p-3">
              <dt className="font-sans text-slate-500">Rust Decision</dt>
              <dd
                className={`mt-1 text-sm font-semibold ${
                  result.status === 'filled' ? 'text-emerald-700' : 'text-amber-700'
                }`}
              >
                {result.status === 'filled' ? 'Filled (new WAV written)' : 'Declined (no output)'}
              </dd>
            </div>
            <div className="rounded-md border border-slate-200 bg-slate-50 p-3">
              <dt className="font-sans text-slate-500">Original Source SHA-256</dt>
              <dd className="mt-1 text-slate-900">{shortHash(result.sourceSha256)}</dd>
            </div>
            <div className="rounded-md border border-slate-200 bg-slate-50 p-3">
              <dt className="font-sans text-slate-500">Repaired Output SHA-256</dt>
              <dd className="mt-1 text-slate-900">
                {result.repairedSha256 ? shortHash(result.repairedSha256) : 'Not written'}
              </dd>
            </div>
          </dl>

          <p className="mt-4 rounded-md border border-slate-200 bg-slate-50 px-3.5 py-2.5 font-mono text-xs text-slate-700">
            {result.message}
          </p>

          {result.repairedWavUrl && (
            <div className="mt-4 rounded-md border border-slate-200 bg-slate-50 p-4">
              <p className="mb-2 text-xs font-medium text-slate-700">
                Repaired Track Candidate Audition (All samples outside [{result.startFrame},{' '}
                {result.endFrame}) copied unchanged)
              </p>
              <audio
                controls
                preload="metadata"
                src={result.repairedWavUrl}
                className="w-full"
                aria-label="Local gap repaired audio player"
              />
            </div>
          )}
        </section>
      ) : (
        <section className="rounded-lg border border-dashed border-slate-300 bg-white p-8 text-center">
          <p className="text-sm font-medium text-slate-800">
            No local gap repair run in this session yet.
          </p>
          <p className="mt-1 text-xs text-slate-500">
            Specify an explicit 0.25–4.00 ms damaged interval above and run{' '}
            <code className="font-mono">spectracraft-mono repair-gap</code> to generate a
            bounded repair copy without touching the original file.
          </p>
        </section>
      )}
    </div>
  );
}
