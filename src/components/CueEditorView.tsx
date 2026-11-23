import React, { useMemo, useState } from 'react';
import { Plus, RotateCcw, Trash2 } from 'lucide-react';
import {
  formatDuration,
  shortHash,
  validateRepickInput,
  type IndependentPitchReport,
  type NoteEditPatch,
  type NoteSummary,
  type SessionPayload,
  type Strike,
} from '../lib/workbench';

interface CueEditorViewProps {
  session: SessionPayload;
  onCueRenderComplete: (runId: string) => void;
}

interface RenderCueResult {
  runId: string;
  drySha256: string;
  dryWavUrl: string;
  cuesJsonUrl: string;
  pitchCheck: IndependentPitchReport;
  notes: NoteSummary[];
}

export function CueEditorView({ session, onCueRenderComplete }: CueEditorViewProps) {
  const initialOffset = Math.min(
    0.18,
    Math.max(0.02, (session.notes[0]?.duration_s ?? 0.3) * 0.4),
  ).toFixed(3);
  const [selectedIdx, setSelectedIdx] = useState<number>(0);
  const [edits, setEdits] = useState<Record<number, { levelScale: number; rePicks: Strike[] }>>({});
  const [newOffset, setNewOffset] = useState<string>(initialOffset);
  const [newStrength, setNewStrength] = useState<string>('0.45');
  const [validationError, setValidationError] = useState<string | null>(null);
  const [rendering, setRendering] = useState(false);
  const [renderError, setRenderError] = useState<string | null>(null);
  const [renderResult, setRenderResult] = useState<RenderCueResult | null>(null);
  const [noteFilter, setNoteFilter] = useState<'all' | 'uncertain'>('all');

  const notes = session.notes;
  const uncertainNotesCount = useMemo(
    () => notes.filter((n) => (n.uncertain_overlap_count ?? 0) > 0).length,
    [notes],
  );
  const visibleNotes = useMemo(
    () =>
      noteFilter === 'uncertain'
        ? notes.filter((n) => (n.uncertain_overlap_count ?? 0) > 0)
        : notes,
    [notes, noteFilter],
  );
  const activeNote = notes[selectedIdx] ?? notes[0] ?? null;

  const activeEdit = useMemo(() => {
    if (!activeNote) return { levelScale: 1.0, rePicks: [] as Strike[] };
    return (
      edits[activeNote.index] ?? {
        levelScale: 1.0,
        rePicks: activeNote.re_picks,
      }
    );
  }, [activeNote, edits]);

  const modifiedCount = Object.keys(edits).length;

  function updateActiveNote(next: { levelScale: number; rePicks: Strike[] }) {
    if (!activeNote) return;
    setEdits((prev) => ({ ...prev, [activeNote.index]: next }));
  }

  function handleAddRepick(e: React.FormEvent) {
    e.preventDefault();
    if (!activeNote) return;
    const offset = Number(newOffset);
    const strength = Number(newStrength);
    const err = validateRepickInput(activeNote.duration_s, offset, strength);
    if (err) {
      setValidationError(err);
      return;
    }
    setValidationError(null);
    const nextStrikes = [...activeEdit.rePicks, { offset_s: offset, strength }].sort(
      (a, b) => a.offset_s - b.offset_s,
    );
    updateActiveNote({ levelScale: activeEdit.levelScale, rePicks: nextStrikes });
  }

  function handleRemoveRepick(idx: number) {
    const nextStrikes = activeEdit.rePicks.filter((_, i) => i !== idx);
    updateActiveNote({ levelScale: activeEdit.levelScale, rePicks: nextStrikes });
  }

  async function handleRenderEditedCues() {
    setRendering(true);
    setRenderError(null);
    try {
      const patches: NoteEditPatch[] = Object.entries(edits).map(([key, val]) => ({
        index: Number(key),
        level_scale: val.levelScale,
        re_picks: val.rePicks,
      }));
      const resp = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/render-cues`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ patches }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        throw new Error(data.error || 'Failed to render dry audio from cues.');
      }
      setRenderResult(data as RenderCueResult);
      onCueRenderComplete(data.runId);
    } catch (err) {
      setRenderError(err instanceof Error ? err.message : String(err));
    } finally {
      setRendering(false);
    }
  }

  return (
    <div className="space-y-6">
      <section className="flex flex-wrap items-start justify-between gap-4 border-b border-slate-200 pb-5">
        <div className="max-w-3xl space-y-1">
          <h1 className="text-2xl font-bold tracking-tight text-slate-900">
            Cue &amp; Re-Pick Editor
          </h1>
          <p className="text-sm text-slate-600">
            Edit note accent scale or persistent-string <code className="font-mono text-xs">re_picks</code>{' '}
            and re-synthesize strictly from saved cue JSON via Rust{' '}
            <code className="font-mono text-xs">render-dry-guitar</code> (no source PCM access).
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <button
            type="button"
            disabled={modifiedCount === 0}
            onClick={() => {
              setEdits({});
              setRenderResult(null);
            }}
            className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-md border border-slate-300 bg-white px-3.5 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <RotateCcw className="h-4 w-4" aria-hidden="true" />
            Reset Edits ({modifiedCount})
          </button>
          <button
            type="button"
            disabled={rendering}
            onClick={handleRenderEditedCues}
            className="whitespace-nowrap rounded-md bg-blue-700 px-4 py-2 text-sm font-medium text-white hover:bg-blue-800 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {rendering ? 'Rendering in Rust…' : 'Render Dry from Cues'}
          </button>
        </div>
      </section>

      {renderError && (
        <div role="alert" className="rounded-md border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800">
          {renderError}
        </div>
      )}

      {renderResult && (
        <section
          aria-label="Source-free cue render output"
          className="rounded-lg border border-slate-200 bg-white p-5"
        >
          <div className="flex flex-wrap items-baseline justify-between gap-3">
            <div>
              <h2 className="text-base font-semibold text-slate-900">
                Source-Free Cue Render Complete (Candidate)
              </h2>
              <p className="mt-0.5 font-mono text-xs tabular-nums text-slate-500">
                New Dry SHA-256: {shortHash(renderResult.drySha256)} · Baseline Dry SHA-256:{' '}
                {shortHash(session.verification.dry_candidate_sha256)} ·{' '}
                {renderResult.drySha256 === session.verification.dry_candidate_sha256
                  ? 'Bit-identical to baseline'
                  : 'Updated synthesis from edited cues'}
              </p>
            </div>
            <div className="flex items-center gap-3 text-xs font-medium">
              <a href={`${renderResult.cuesJsonUrl}?download=1`} className="text-blue-700 hover:underline">
                Download Edited Cues
              </a>
              <a href={`${renderResult.dryWavUrl}?download=1`} className="text-blue-700 hover:underline">
                Download Dry WAV
              </a>
            </div>
          </div>

          <div className="mt-4 grid grid-cols-1 gap-4 lg:grid-cols-2">
            <div className="rounded-md border border-slate-200 bg-slate-50 p-3.5">
              <p className="mb-2 text-xs font-medium text-slate-700">Edited Dry Candidate Audition</p>
              <audio
                controls
                preload="metadata"
                src={renderResult.dryWavUrl}
                className="w-full"
                aria-label="Edited dry cue render audio player"
              />
            </div>

            <dl className="grid grid-cols-3 gap-3 rounded-md border border-slate-200 bg-slate-50 p-3.5 font-mono text-xs tabular-nums">
              <div>
                <dt className="font-sans text-slate-500">Envelope Corr</dt>
                <dd className="mt-1 text-base font-semibold text-slate-900">
                  {renderResult.pitchCheck.envelope_correlation?.toFixed(4) ?? '—'}
                </dd>
              </div>
              <div>
                <dt className="font-sans text-slate-500">Agreed &le;100c</dt>
                <dd className="mt-1 text-base font-semibold text-slate-900">
                  {renderResult.pitchCheck.agreed_within_100_cents} /{' '}
                  {renderResult.pitchCheck.both_resolved_frames}
                </dd>
              </div>
              <div>
                <dt className="font-sans text-slate-500">Octave Errors</dt>
                <dd className="mt-1 text-base font-semibold text-amber-700">
                  {renderResult.pitchCheck.octave_errors}
                </dd>
              </div>
            </dl>
          </div>
        </section>
      )}

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-12">
        {/* Note List */}
        <section className="rounded-lg border border-slate-200 bg-white lg:col-span-7">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-slate-200 px-4 py-3">
            <div>
              <h2 className="text-sm font-semibold text-slate-900">
                Analyzed Guitar Events ({visibleNotes.length} / {notes.length})
              </h2>
              <span className="font-mono text-xs tabular-nums text-slate-500">
                5 ms cue grid · filter uncertain regions to inspect &amp; revise
              </span>
            </div>
            <div className="flex items-center gap-1 rounded-md bg-slate-100 p-0.5">
              <button
                type="button"
                onClick={() => setNoteFilter('all')}
                className={`whitespace-nowrap rounded px-2.5 py-1 text-xs font-medium ${
                  noteFilter === 'all'
                    ? 'bg-white text-slate-900 shadow-xs'
                    : 'text-slate-600 hover:text-slate-900'
                }`}
              >
                All ({notes.length})
              </button>
              <button
                type="button"
                onClick={() => {
                  setNoteFilter('uncertain');
                  const firstUncertain = notes.find((n) => (n.uncertain_overlap_count ?? 0) > 0);
                  if (firstUncertain) {
                    setSelectedIdx(firstUncertain.index);
                    setNewOffset(
                      Math.min(0.18, Math.max(0.02, firstUncertain.duration_s * 0.4)).toFixed(3),
                    );
                    setValidationError(null);
                  }
                }}
                className={`whitespace-nowrap rounded px-2.5 py-1 text-xs font-medium ${
                  noteFilter === 'uncertain'
                    ? 'bg-white text-slate-900 shadow-xs'
                    : 'text-slate-600 hover:text-slate-900'
                }`}
              >
                Uncertain Overlap ({uncertainNotesCount})
              </button>
            </div>
          </div>
          <div className="max-h-[480px] overflow-y-auto">
            <table className="w-full text-left text-xs">
              <thead className="sticky top-0 bg-slate-50 text-slate-500">
                <tr className="border-b border-slate-200">
                  <th className="px-3 py-2">#</th>
                  <th className="px-3 py-2">Span</th>
                  <th className="px-3 py-2 text-right">Duration</th>
                  <th className="px-3 py-2 text-right">Median Pitch</th>
                  <th className="px-3 py-2 text-right">Uncertain</th>
                  <th className="px-3 py-2 text-right">Re-picks</th>
                  <th className="px-3 py-2 text-right">Status</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100 font-mono tabular-nums">
                {visibleNotes.length === 0 ? (
                  <tr>
                    <td colSpan={7} className="px-4 py-6 text-center font-sans text-slate-500">
                      No notes overlap uncertain frames in this recording.
                    </td>
                  </tr>
                ) : (
                  visibleNotes.map((note) => {
                    const isSel = activeNote?.index === note.index;
                    const edited = edits[note.index];
                    const strikeCount = edited ? edited.rePicks.length : note.re_picks.length;
                    const uncertainCount = note.uncertain_overlap_count ?? 0;
                    return (
                      <tr
                        key={note.index}
                        className={isSel ? 'bg-blue-50/70' : 'hover:bg-slate-50'}
                      >
                        <td className="px-3 py-2">
                          <button
                            type="button"
                            onClick={() => {
                              setSelectedIdx(note.index);
                              setNewOffset(Math.min(0.18, Math.max(0.02, note.duration_s * 0.4)).toFixed(3));
                              setValidationError(null);
                            }}
                            className="font-semibold text-blue-700 hover:underline"
                            aria-label={`Select note ${note.index + 1}`}
                          >
                            #{note.index + 1}
                          </button>
                        </td>
                        <td className="px-3 py-2 text-slate-700">
                          {note.start.toFixed(2)}–{note.end.toFixed(2)} s
                        </td>
                        <td className="px-3 py-2 text-right text-slate-700">
                          {formatDuration(note.duration_s)}
                        </td>
                        <td className="px-3 py-2 text-right text-slate-900">
                          {note.median_hz.toFixed(1)} Hz
                        </td>
                        <td
                          className={`px-3 py-2 text-right ${
                            uncertainCount > 0 ? 'font-semibold text-amber-700' : 'text-slate-400'
                          }`}
                        >
                          {uncertainCount}
                        </td>
                        <td className="px-3 py-2 text-right text-slate-700">{strikeCount}</td>
                        <td className="px-3 py-2 text-right font-sans text-slate-500">
                          {edited ? 'Modified' : 'Analyzed'}
                        </td>
                      </tr>
                    );
                  })
                )}
              </tbody>
            </table>
          </div>
        </section>

        {/* Inspector */}
        <section className="rounded-lg border border-slate-200 bg-white p-5 lg:col-span-5">
          {activeNote ? (
            <div className="space-y-5">
              <div>
                <p className="text-xs text-slate-500">Selected Cue Event</p>
                <h2 className="mt-0.5 text-lg font-semibold text-slate-900">
                  Note #{activeNote.index + 1} · {activeNote.median_hz.toFixed(1)} Hz
                </h2>
                <p className="mt-1 font-mono text-xs tabular-nums text-slate-500">
                  {activeNote.start.toFixed(3)} s to {activeNote.end.toFixed(3)} s ({formatDuration(activeNote.duration_s)}) ·
                  Range {activeNote.min_hz.toFixed(1)}–{activeNote.max_hz.toFixed(1)} Hz
                </p>
              </div>

              <div className="space-y-2 border-t border-slate-200 pt-4">
                <div className="flex items-center justify-between">
                  <label htmlFor="note-level-scale" className="text-sm font-medium text-slate-800">
                    Event Intensity Scale
                  </label>
                  <span className="font-mono text-sm tabular-nums text-slate-900">
                    {(activeEdit.levelScale * 100).toFixed(0)}%
                  </span>
                </div>
                <input
                  id="note-level-scale"
                  type="range"
                  min="0"
                  max="1.6"
                  step="0.05"
                  value={activeEdit.levelScale}
                  onChange={(e) =>
                    updateActiveNote({
                      levelScale: Number(e.target.value),
                      rePicks: activeEdit.rePicks,
                    })
                  }
                  className="w-full accent-blue-700"
                />
              </div>

              <div className="space-y-3 border-t border-slate-200 pt-4">
                <h3 className="text-sm font-semibold text-slate-900">
                  Persistent-String Re-Picks ({activeEdit.rePicks.length})
                </h3>
                {activeEdit.rePicks.length === 0 ? (
                  <p className="text-xs text-slate-500">
                    No re-picks on this event. Add a strike offset below to re-excite the ringing
                    string modes without resetting phase.
                  </p>
                ) : (
                  <ul className="divide-y divide-slate-100 rounded-md border border-slate-200">
                    {activeEdit.rePicks.map((strike, idx) => (
                      <li
                        key={`${strike.offset_s}-${idx}`}
                        className="flex items-center justify-between px-3 py-2 font-mono text-xs tabular-nums"
                      >
                        <span>
                          offset +{strike.offset_s.toFixed(3)} s · strength {strike.strength.toFixed(2)}
                        </span>
                        <button
                          type="button"
                          onClick={() => handleRemoveRepick(idx)}
                          aria-label={`Remove re-pick at ${strike.offset_s.toFixed(3)} seconds`}
                          className="rounded p-1 text-slate-500 hover:bg-slate-100 hover:text-rose-700"
                        >
                          <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                        </button>
                      </li>
                    ))}
                  </ul>
                )}

                <form onSubmit={handleAddRepick} className="grid grid-cols-2 gap-3 pt-1">
                  <div>
                    <label htmlFor="repick-offset" className="block text-xs font-medium text-slate-700">
                      Offset (seconds)
                    </label>
                    <input
                      id="repick-offset"
                      type="number"
                      step="0.01"
                      value={newOffset}
                      onChange={(e) => setNewOffset(e.target.value)}
                      className="mt-1 w-full rounded-md border border-slate-300 px-2.5 py-1.5 font-mono text-xs tabular-nums"
                    />
                  </div>
                  <div>
                    <label htmlFor="repick-strength" className="block text-xs font-medium text-slate-700">
                      Strength (0.05–1.50)
                    </label>
                    <input
                      id="repick-strength"
                      type="number"
                      step="0.05"
                      value={newStrength}
                      onChange={(e) => setNewStrength(e.target.value)}
                      className="mt-1 w-full rounded-md border border-slate-300 px-2.5 py-1.5 font-mono text-xs tabular-nums"
                    />
                  </div>
                  {validationError && (
                    <p role="alert" className="col-span-2 text-xs text-rose-700">
                      {validationError}
                    </p>
                  )}
                  <div className="col-span-2">
                    <button
                      type="button"
                      onClick={handleAddRepick}
                      className="inline-flex w-full items-center justify-center gap-1.5 rounded-md border border-slate-300 bg-slate-50 px-3 py-2 text-xs font-medium text-slate-800 hover:bg-slate-100"
                    >
                      <Plus className="h-3.5 w-3.5" aria-hidden="true" />
                      Add Re-Pick Strike
                    </button>
                  </div>
                </form>
              </div>
            </div>
          ) : (
            <p className="text-sm text-slate-500">No analyzed notes available in this session.</p>
          )}
        </section>
      </div>
    </div>
  );
}
