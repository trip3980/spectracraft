import React, { useEffect, useRef, useState } from 'react';
import { Download, Play, RotateCcw, Square } from 'lucide-react';
import {
  computeEffectiveLayerGains,
  type LayerChannel,
  type LayerSessionResult,
  type SessionPayload,
} from '../lib/workbench';
import { LocalRepairPanel } from './LocalRepairPanel';

interface LayersViewProps {
  session: SessionPayload;
}

const FIXED_HEADROOM = 0.707;

export function LayersView({ session }: LayersViewProps) {
  const maxDur = session.verification.duration_s;
  const defaultStart = maxDur >= 27 ? 20.0 : 0.0;
  const defaultEnd = maxDur >= 27 ? 27.0 : Math.min(maxDur, 20.0);

  const [subMode, setSubMode] = useState<'layers' | 'repair'>('layers');
  const [startS, setStartS] = useState<string>(defaultStart.toFixed(1));
  const [endS, setEndS] = useState<string>(defaultEnd.toFixed(1));
  const [faintRidges, setFaintRidges] = useState<boolean>(false);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);
  const [layerRun, setLayerRun] = useState<LayerSessionResult | null>(null);
  const [channels, setChannels] = useState<LayerChannel[]>([]);
  const [playing, setPlaying] = useState<boolean>(false);

  const audioCtxRef = useRef<AudioContext | null>(null);
  const buffersRef = useRef<Record<string, AudioBuffer>>({});
  const sourcesRef = useRef<AudioBufferSourceNode[]>([]);
  const gainNodesRef = useRef<Record<string, GainNode>>({});

  useEffect(() => {
    setStartS(defaultStart.toFixed(1));
    setEndS(defaultEnd.toFixed(1));
    setLayerRun(null);
    setChannels([]);
    stopPlayback();
  }, [session.id, defaultStart, defaultEnd]);

  useEffect(() => {
    const gains = computeEffectiveLayerGains(channels, FIXED_HEADROOM);
    for (const [id, node] of Object.entries(gainNodesRef.current)) {
      const target = gains[id] ?? 0;
      node.gain.setTargetAtTime(target, node.context.currentTime, 0.015);
    }
  }, [channels]);

  useEffect(() => {
    return () => {
      stopPlayback();
      if (audioCtxRef.current) {
        void audioCtxRef.current.close();
      }
    };
  }, []);

  function stopPlayback() {
    for (const src of sourcesRef.current) {
      try {
        src.stop();
        src.disconnect();
      } catch {
        // Already stopped
      }
    }
    sourcesRef.current = [];
    gainNodesRef.current = {};
    setPlaying(false);
  }

  async function handleAnalyzeLayers() {
    stopPlayback();
    setLoading(true);
    setError(null);
    try {
      const s = Number(startS);
      const e = Number(endS);
      if (!Number.isFinite(s) || !Number.isFinite(e) || e <= s) {
        throw new Error('End time must be greater than start time.');
      }
      if (e - s > 29.5) {
        throw new Error('Editable layers support windows up to 29.5 seconds at a time.');
      }
      const resp = await fetch(`/api/sessions/${encodeURIComponent(session.id)}/layers`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ startS: s, endS: e, faintRidges }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        throw new Error(data.error || 'Rust layer analysis failed.');
      }
      const result = data as LayerSessionResult;
      setLayerRun(result);
      setChannels(
        result.stems.map((stem) => ({
          ...stem,
          gain: 1.0,
          muted: false,
          solo: false,
        })),
      );
      buffersRef.current = {};
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  async function handlePlaySyncedStems() {
    if (!layerRun || channels.length === 0) return;
    if (playing) {
      stopPlayback();
      return;
    }
    try {
      const ctx = audioCtxRef.current ?? new AudioContext();
      audioCtxRef.current = ctx;
      if (ctx.state === 'suspended') {
        await ctx.resume();
      }
      // Decode all 4 Rust stems once if not cached.
      for (const ch of channels) {
        if (!buffersRef.current[ch.id]) {
          const resp = await fetch(ch.url);
          const ab = await resp.arrayBuffer();
          buffersRef.current[ch.id] = await ctx.decodeAudioData(ab);
        }
      }

      const effective = computeEffectiveLayerGains(channels, FIXED_HEADROOM);
      const startAt = ctx.currentTime + 0.04;
      const nodes: AudioBufferSourceNode[] = [];
      const gMap: Record<string, GainNode> = {};

      for (const ch of channels) {
        const src = ctx.createBufferSource();
        src.buffer = buffersRef.current[ch.id];
        const g = ctx.createGain();
        g.gain.value = effective[ch.id] ?? 0;
        src.connect(g);
        g.connect(ctx.destination);
        src.start(startAt);
        nodes.push(src);
        gMap[ch.id] = g;
      }

      if (nodes[0]) {
        nodes[0].onended = () => {
          setPlaying(false);
          sourcesRef.current = [];
          gainNodesRef.current = {};
        };
      }
      sourcesRef.current = nodes;
      gainNodesRef.current = gMap;
      setPlaying(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Unable to start synchronized stem playback.');
      setPlaying(false);
    }
  }

  function updateChannel(id: LayerChannel['id'], patch: Partial<LayerChannel>) {
    setChannels((prev) => prev.map((c) => (c.id === id ? { ...c, ...patch } : c)));
  }

  function resetMixer() {
    setChannels((prev) => prev.map((c) => ({ ...c, gain: 1.0, muted: false, solo: false })));
  }

  const effectiveGains = computeEffectiveLayerGains(channels, FIXED_HEADROOM);

  return (
    <div className="space-y-6">
      <section className="flex flex-wrap items-start justify-between gap-4 border-b border-slate-200 pb-5">
        <div className="max-w-3xl space-y-1">
          <p className="text-xs font-medium text-slate-500">
            Two Separate Rust Tools · Fresh Layer Synthesis vs. Bounded Local PCM Repair
          </p>
          <h1 className="text-2xl font-bold tracking-tight text-slate-900">
            {subMode === 'layers' ? 'Editable Layers Workspace' : 'Local Gap Repair'}
          </h1>
          <p className="text-sm text-slate-600">
            {subMode === 'layers' ? (
              <>
                Runs native Rust <code className="font-mono text-xs">analyze-layers</code> and{' '}
                <code className="font-mono text-xs">render-layers</code> on a chosen slice (&le; 30
                s) to synthesize four isolated stems (Pitch, Ringing, Texture, Breath Accent) with
                fixed audition headroom (-3.0 dB).
              </>
            ) : (
              <>
                Runs native Rust <code className="font-mono text-xs">repair-gap</code> to replace
                only an explicit 0.25–4.00 ms damaged interval while copying every unaffected sample
                unchanged. Separate from whole-track reconstruction.
              </>
            )}
          </p>
        </div>

        <div className="flex items-center gap-1 rounded-md bg-slate-200/70 p-1">
          <button
            type="button"
            onClick={() => setSubMode('layers')}
            className={`whitespace-nowrap rounded px-3.5 py-1.5 text-xs font-medium transition-colors ${
              subMode === 'layers'
                ? 'bg-white text-slate-900 shadow-xs'
                : 'text-slate-600 hover:text-slate-900'
            }`}
          >
            Editable Layers
          </button>
          <button
            type="button"
            onClick={() => {
              stopPlayback();
              setSubMode('repair');
            }}
            className={`whitespace-nowrap rounded px-3.5 py-1.5 text-xs font-medium transition-colors ${
              subMode === 'repair'
                ? 'bg-white text-slate-900 shadow-xs'
                : 'text-slate-600 hover:text-slate-900'
            }`}
          >
            Local Gap Repair
          </button>
        </div>
      </section>

      {subMode === 'repair' ? (
        <LocalRepairPanel session={session} />
      ) : (
        <>

      <section className="rounded-lg border border-slate-200 bg-white p-5">
        <div className="flex flex-wrap items-end gap-4">
          <div>
            <label htmlFor="layers-start" className="block text-xs font-medium text-slate-700">
              Window Start (s)
            </label>
            <input
              id="layers-start"
              type="number"
              step="0.5"
              min="0"
              max={maxDur}
              value={startS}
              onChange={(e) => setStartS(e.target.value)}
              className="mt-1 w-28 rounded-md border border-slate-300 px-2.5 py-1.5 font-mono text-sm tabular-nums"
            />
          </div>
          <div>
            <label htmlFor="layers-end" className="block text-xs font-medium text-slate-700">
              Window End (s)
            </label>
            <input
              id="layers-end"
              type="number"
              step="0.5"
              min="0.5"
              max={maxDur}
              value={endS}
              onChange={(e) => setEndS(e.target.value)}
              className="mt-1 w-28 rounded-md border border-slate-300 px-2.5 py-1.5 font-mono text-sm tabular-nums"
            />
          </div>
          <label className="flex cursor-pointer items-center gap-2 py-2 text-sm text-slate-700">
            <input
              type="checkbox"
              checked={faintRidges}
              onChange={(e) => setFaintRidges(e.target.checked)}
              className="h-4 w-4 rounded border-slate-300 accent-blue-700"
            />
            Opt-in faint-ridge tracking
          </label>
          <button
            type="button"
            disabled={loading}
            onClick={handleAnalyzeLayers}
            className="whitespace-nowrap rounded-md bg-blue-700 px-4 py-2 text-sm font-medium text-white hover:bg-blue-800 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {loading ? 'Running Rust Layer Engine…' : 'Analyze & Render Layers'}
          </button>
        </div>

        {error && (
          <p role="alert" className="mt-3 text-sm text-rose-700">
            {error}
          </p>
        )}
      </section>

      {layerRun ? (
        <section className="space-y-4 rounded-lg border border-slate-200 bg-white p-5">
          <div className="flex flex-wrap items-center justify-between gap-4 border-b border-slate-200 pb-4">
            <div>
              <h2 className="text-base font-semibold text-slate-900">
                Synchronized Four-Stem Mixer ({layerRun.startS.toFixed(2)}–{layerRun.endS.toFixed(2)} s)
              </h2>
              <p className="text-xs text-slate-500">
                Mode: {layerRun.faintRidges ? 'Faint-ridge tracking enabled' : 'Standard layer profile'} ·
                Fixed audition headroom: -3.01 dB (0.707 linear). Mixer gains do not alter saved measurement JSON.
              </p>
            </div>
            <div className="flex flex-wrap items-center gap-2.5">
              <button
                type="button"
                onClick={handlePlaySyncedStems}
                className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-md bg-slate-900 px-4 py-2 text-sm font-medium text-white hover:bg-slate-800"
              >
                {playing ? (
                  <>
                    <Square className="h-3.5 w-3.5" aria-hidden="true" />
                    Stop Mix
                  </>
                ) : (
                  <>
                    <Play className="h-3.5 w-3.5" aria-hidden="true" />
                    Play Synced Stems
                  </>
                )}
              </button>
              <button
                type="button"
                onClick={resetMixer}
                className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-md border border-slate-300 bg-white px-3 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
              >
                <RotateCcw className="h-3.5 w-3.5" aria-hidden="true" />
                Reset Gains
              </button>
              <a
                href={`${layerRun.analysisJsonUrl}?download=1`}
                className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-md border border-slate-300 bg-white px-3 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
              >
                <Download className="h-3.5 w-3.5" aria-hidden="true" />
                Packet JSON
              </a>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            {channels.map((ch) => {
              const eff = effectiveGains[ch.id] ?? 0;
              return (
                <div key={ch.id} className="rounded-md border border-slate-200 bg-slate-50 p-4">
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <h3 className="text-sm font-semibold text-slate-900">{ch.label}</h3>
                      <p className="text-xs text-slate-500">{ch.description}</p>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <button
                        type="button"
                        aria-pressed={ch.muted}
                        onClick={() => updateChannel(ch.id, { muted: !ch.muted })}
                        className={`rounded border px-2.5 py-1 text-xs font-medium ${
                          ch.muted
                            ? 'border-rose-300 bg-rose-50 text-rose-800'
                            : 'border-slate-300 bg-white text-slate-700 hover:bg-slate-100'
                        }`}
                      >
                        Mute
                      </button>
                      <button
                        type="button"
                        aria-pressed={ch.solo}
                        onClick={() => updateChannel(ch.id, { solo: !ch.solo })}
                        className={`rounded border px-2.5 py-1 text-xs font-medium ${
                          ch.solo
                            ? 'border-blue-600 bg-blue-50 text-blue-800'
                            : 'border-slate-300 bg-white text-slate-700 hover:bg-slate-100'
                        }`}
                      >
                        Solo
                      </button>
                    </div>
                  </div>

                  <div className="mt-3 flex items-center gap-3">
                    <label htmlFor={`gain-${ch.id}`} className="text-xs text-slate-600">
                      Gain
                    </label>
                    <input
                      id={`gain-${ch.id}`}
                      type="range"
                      min="0"
                      max="1.5"
                      step="0.05"
                      value={ch.gain}
                      onChange={(e) => updateChannel(ch.id, { gain: Number(e.target.value) })}
                      className="flex-1 accent-blue-700"
                    />
                    <span className="w-24 text-right font-mono text-xs tabular-nums text-slate-700">
                      {ch.gain.toFixed(2)}x ({eff.toFixed(2)})
                    </span>
                  </div>

                  <div className="mt-3 flex items-center justify-between gap-3">
                    <audio
                      controls
                      preload="metadata"
                      src={ch.url}
                      className="h-8 flex-1"
                      aria-label={`${ch.label} isolated stem player`}
                    />
                    <a
                      href={`${ch.url}?download=1`}
                      className="shrink-0 text-xs font-medium text-blue-700 hover:underline"
                    >
                      WAV
                    </a>
                  </div>
                </div>
              );
            })}
          </div>

          <div className="rounded-md border border-slate-200 bg-slate-50 p-4">
            <div className="mb-2 flex items-center justify-between">
              <span className="text-xs font-semibold text-slate-800">
                Summed Layer Mix (Rust render-layers/mix.wav)
              </span>
              <a
                href={`${layerRun.mixUrl}?download=1`}
                className="text-xs font-medium text-blue-700 hover:underline"
              >
                Download Mix WAV
              </a>
            </div>
            <audio controls preload="metadata" src={layerRun.mixUrl} className="w-full" />
          </div>
        </section>
      ) : (
        <section className="rounded-lg border border-dashed border-slate-300 bg-white p-8 text-center">
          <p className="text-sm font-medium text-slate-800">
            No layer stems generated for this session yet.
          </p>
          <p className="mt-1 text-xs text-slate-500">
            Choose a window up to 29.5 seconds above and click &ldquo;Analyze &amp; Render Layers&rdquo;
            to run the Rust layer engine.
          </p>
        </section>
      )}
        </>
      )}
    </div>
  );
}
