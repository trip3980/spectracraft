import React, { useEffect, useRef, useState } from 'react';
import {
  computeWaveformColumns,
  getEffectiveBufferSampleRate,
  renderSpectrogramToCanvas,
} from '../lib/audioCanvas.ts';
import { classifyNoteEvidence } from '../lib/contract.ts';
import {
  transformRegionByHandle,
  type DryRegion,
  type DryRegionKind,
  type RegionHandle,
} from '../lib/dryRegions.ts';
import { computeNyquistDisplay } from '../lib/studioRig.ts';
import type { FrameEvidence, NoteSummary } from '../lib/workbench.ts';

export type DisplayMode = 'spectrogram' | 'waveform' | 'both';
export type CanvasTool = 'scrub' | 'marquee' | 'range' | 'zoom';

export interface CursorReadout {
  timeS: number;
  freqHz: number | null;
  track: 'original' | 'result';
}

interface AudioViewportProps {
  originalBuffer: AudioBuffer | null;
  resultBuffer: AudioBuffer | null;
  resultLabel: string;
  viewedLayerLabel: string | null;
  sampleRate: number;
  durationS: number;
  viewStartS: number;
  viewEndS: number;
  playheadS: number;
  selection: [number, number] | null;
  displayMode: DisplayMode;
  displayMaxHz: number;
  spectrogramDbFloor: number;
  showEvidenceOverlay: boolean;
  notes: NoteSummary[];
  frameEvidence: FrameEvidence[];
  activeTool: CanvasTool;
  onSeek: (timeS: number) => void;
  onSelectRange: (range: [number, number] | null) => void;
  onZoomViewport: (nextStartS: number, nextEndS: number) => void;
  onCursorMove: (cursor: CursorReadout | null) => void;
  // Stage 2 Surgical Region Editing (Dry Take only — Audit H7 & M5)
  isDrySurfaceEditable?: boolean;
  onSwitchToDrySurface?: () => void;
  dryRegions?: DryRegion[];
  selectedRegionId?: string | null;
  onSelectDryRegion?: (id: string | null) => void;
  onCreateDryRegion?: (input: {
    kind: DryRegionKind;
    startS: number;
    endS: number;
    minHz?: number | null;
    maxHz?: number | null;
  }) => void;
  onBeginDryRegionGesture?: () => void;
  onCancelDryRegionGesture?: () => void;
  onTransformDryRegion?: (id: string, next: DryRegion, commit: boolean) => void;
}

const AXIS_W = 50;

export function AudioViewport({
  originalBuffer,
  resultBuffer,
  resultLabel,
  viewedLayerLabel,
  sampleRate,
  durationS,
  viewStartS,
  viewEndS,
  playheadS,
  selection,
  displayMode,
  displayMaxHz,
  spectrogramDbFloor,
  showEvidenceOverlay,
  notes,
  frameEvidence,
  activeTool,
  onSeek,
  onSelectRange,
  onZoomViewport,
  onCursorMove,
  isDrySurfaceEditable = true,
  onSwitchToDrySurface,
  dryRegions = [],
  selectedRegionId = null,
  onSelectDryRegion,
  onCreateDryRegion,
  onBeginDryRegionGesture,
  onCancelDryRegionGesture,
  onTransformDryRegion,
}: AudioViewportProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [resizeTick, setResizeTick] = useState(0);
  const overviewRef = useRef<HTMLCanvasElement | null>(null);
  const rulerRef = useRef<HTMLCanvasElement | null>(null);
  const origCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const resCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const resTrackWrapRef = useRef<HTMLDivElement | null>(null);
  const [resSize, setResSize] = useState<{ w: number; h: number }>({ w: 600, h: 180 });
  const [draftRegion, setDraftRegion] = useState<{
    kind: DryRegionKind;
    startS: number;
    endS: number;
    minHz: number | null;
    maxHz: number | null;
  } | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => {
      setResizeTick((t) => t + 1);
      if (resTrackWrapRef.current) {
        const r = resTrackWrapRef.current.getBoundingClientRect();
        setResSize({ w: Math.max(120, r.width), h: Math.max(60, r.height) });
      }
    });
    ro.observe(el);
    if (resTrackWrapRef.current) {
      const r = resTrackWrapRef.current.getBoundingClientRect();
      setResSize({ w: Math.max(120, r.width), h: Math.max(60, r.height) });
    }
    return () => ro.disconnect();
  }, []);

  const dragMode = useRef<{
    kind: 'overview' | 'range' | 'scrub' | 'dry-create';
    createKind?: DryRegionKind;
    anchorS: number;
    anchorHz: number | null;
    origViewStart: number;
    origViewEnd: number;
  } | null>(null);

  const regionDragRef = useRef<{
    id: string;
    handle: RegionHandle;
    origin: DryRegion;
    startClientX: number;
    startClientY: number;
    lastNext: DryRegion;
  } | null>(null);

  // Overview waveform strip
  useEffect(() => {
    const canvas = overviewRef.current;
    if (!canvas || !originalBuffer) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const rect = canvas.getBoundingClientRect();
    const w = Math.max(100, Math.floor(rect.width));
    const h = Math.max(20, Math.floor(rect.height));
    canvas.width = Math.floor(w * dpr);
    canvas.height = Math.floor(h * dpr);
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.save();
    ctx.scale(dpr, dpr);

    ctx.fillStyle = '#1b1b1b';
    ctx.fillRect(0, 0, w, h);

    const pcm = originalBuffer.getChannelData(0);
    const decodedRate = getEffectiveBufferSampleRate(originalBuffer, sampleRate);
    const cols = computeWaveformColumns(pcm, decodedRate, 0, durationS, w);
    const midY = h / 2;
    ctx.fillStyle = '#5a708a';
    for (let x = 0; x < cols.length; x++) {
      const yTop = midY - cols[x].max * (midY - 2);
      const yBot = midY - cols[x].min * (midY - 2);
      ctx.fillRect(x, yTop, 1, Math.max(1, yBot - yTop));
    }

    if (selection && durationS > 0) {
      const sx0 = (selection[0] / durationS) * w;
      const sx1 = (selection[1] / durationS) * w;
      ctx.fillStyle = 'rgba(76, 198, 172, 0.28)';
      ctx.fillRect(sx0, 0, Math.max(2, sx1 - sx0), h);
    }

    if (durationS > 0) {
      const vx0 = (viewStartS / durationS) * w;
      const vx1 = (viewEndS / durationS) * w;
      ctx.fillStyle = 'rgba(55, 142, 240, 0.14)';
      ctx.fillRect(vx0, 0, Math.max(4, vx1 - vx0), h);
      ctx.strokeStyle = '#378ef0';
      ctx.lineWidth = 1;
      ctx.strokeRect(vx0 + 0.5, 0.5, Math.max(4, vx1 - vx0) - 1, h - 1);

      const px = (playheadS / durationS) * w;
      ctx.strokeStyle = '#ffffff';
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(px + 0.5, 0);
      ctx.lineTo(px + 0.5, h);
      ctx.stroke();
    }
    ctx.restore();
  }, [originalBuffer, sampleRate, durationS, viewStartS, viewEndS, playheadS, selection, resizeTick]);

  // Shared Time Ruler
  useEffect(() => {
    const canvas = rulerRef.current;
    if (!canvas) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const rect = canvas.getBoundingClientRect();
    const w = Math.max(100, Math.floor(rect.width));
    const h = Math.max(18, Math.floor(rect.height));
    canvas.width = Math.floor(w * dpr);
    canvas.height = Math.floor(h * dpr);
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.save();
    ctx.scale(dpr, dpr);

    ctx.fillStyle = '#242424';
    ctx.fillRect(0, 0, w, h);

    ctx.fillStyle = '#202020';
    ctx.fillRect(0, 0, AXIS_W, h);
    ctx.strokeStyle = '#383838';
    ctx.beginPath();
    ctx.moveTo(AXIS_W + 0.5, 0);
    ctx.lineTo(AXIS_W + 0.5, h);
    ctx.stroke();

    const plotW = Math.max(10, w - AXIS_W);
    const span = Math.max(0.05, viewEndS - viewStartS);
    const candidates = [0.05, 0.1, 0.25, 0.5, 1, 2, 5, 10];
    let step = candidates[candidates.length - 1];
    for (const c of candidates) {
      if (span / c <= Math.max(4, plotW / 80)) {
        step = c;
        break;
      }
    }

    const firstTick = Math.ceil(viewStartS / step) * step;
    ctx.fillStyle = '#9a9a9a';
    ctx.strokeStyle = '#4a4a4a';
    ctx.font = '10px "IBM Plex Mono", monospace';
    for (let t = firstTick; t <= viewEndS + 1e-6; t += step) {
      const x = AXIS_W + ((t - viewStartS) / span) * plotW;
      ctx.beginPath();
      ctx.moveTo(Math.round(x) + 0.5, h - 6);
      ctx.lineTo(Math.round(x) + 0.5, h);
      ctx.stroke();
      const label = step < 1 ? `${t.toFixed(2)}s` : `${t.toFixed(1)}s`;
      ctx.fillText(label, Math.min(w - 36, x + 3), h - 7);
    }

    if (playheadS >= viewStartS && playheadS <= viewEndS) {
      const px = AXIS_W + ((playheadS - viewStartS) / span) * plotW;
      ctx.fillStyle = '#378ef0';
      ctx.beginPath();
      ctx.moveTo(px - 4, 0);
      ctx.lineTo(px + 4, 0);
      ctx.lineTo(px, 6);
      ctx.closePath();
      ctx.fill();
    }

    ctx.restore();
  }, [viewStartS, viewEndS, playheadS, resizeTick]);

  // Render Original & Result canvases
  useEffect(() => {
    drawTrackCanvas({
      canvas: origCanvasRef.current,
      buffer: originalBuffer,
      sampleRate,
      viewStartS,
      viewEndS,
      playheadS,
      selection,
      displayMode,
      displayMaxHz,
      dbFloor: spectrogramDbFloor,
      waveColor: '#7ab8f5',
      showEvidenceOverlay,
      notes,
      frameEvidence,
    });
  }, [
    originalBuffer,
    sampleRate,
    viewStartS,
    viewEndS,
    playheadS,
    selection,
    displayMode,
    displayMaxHz,
    spectrogramDbFloor,
    showEvidenceOverlay,
    notes,
    frameEvidence,
    resizeTick,
  ]);

  useEffect(() => {
    drawTrackCanvas({
      canvas: resCanvasRef.current,
      buffer: resultBuffer,
      sampleRate,
      viewStartS,
      viewEndS,
      playheadS,
      selection,
      displayMode,
      displayMaxHz,
      dbFloor: spectrogramDbFloor,
      waveColor: '#4cc6ac',
      showEvidenceOverlay: false,
      notes: [],
      frameEvidence: [],
    });
  }, [
    resultBuffer,
    sampleRate,
    viewStartS,
    viewEndS,
    playheadS,
    selection,
    displayMode,
    displayMaxHz,
    spectrogramDbFloor,
    resizeTick,
  ]);

  const timeFromPlotPointer = (e: React.PointerEvent<HTMLCanvasElement>): number => {
    const rect = e.currentTarget.getBoundingClientRect();
    const plotW = Math.max(1, rect.width - AXIS_W);
    const rel = Math.max(0, Math.min(1, (e.clientX - rect.left - AXIS_W) / plotW));
    return viewStartS + rel * (viewEndS - viewStartS);
  };

  const freqFromPlotPointer = (e: React.PointerEvent<HTMLCanvasElement>): number | null => {
    if (displayMode === 'waveform') return null;
    const rect = e.currentTarget.getBoundingClientRect();
    const specH = displayMode === 'both' ? Math.max(20, Math.floor(rect.height * 0.68)) : Math.max(20, rect.height);
    const relY = Math.max(0, Math.min(1, (e.clientY - rect.top) / specH));
    return Math.max(20, Math.min(displayMaxHz, displayMaxHz * (1 - relY)));
  };

  const handlePlotPointerDown = (
    e: React.PointerEvent<HTMLCanvasElement>,
    track: 'original' | 'result',
  ) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    const t = timeFromPlotPointer(e);
    const hz = freqFromPlotPointer(e);
    if (activeTool === 'zoom') {
      const span = viewEndS - viewStartS;
      const factor = e.shiftKey ? 2.0 : 0.5;
      const nextSpan = Math.max(0.15, Math.min(durationS, span * factor));
      const rel = (t - viewStartS) / Math.max(1e-6, span);
      let ns = t - rel * nextSpan;
      let ne = ns + nextSpan;
      if (ns < 0) {
        ns = 0;
        ne = Math.min(durationS, nextSpan);
      }
      if (ne > durationS) {
        ne = durationS;
        ns = Math.max(0, durationS - nextSpan);
      }
      onZoomViewport(ns, ne);
      return;
    }

    // Stage 2 Surgical Region Creation strictly on the Dry Take pane (Audit H7)
    if (
      track === 'result' &&
      isDrySurfaceEditable &&
      !viewedLayerLabel &&
      (activeTool === 'marquee' || activeTool === 'range') &&
      onCreateDryRegion
    ) {
      const kind: DryRegionKind =
        activeTool === 'marquee' && displayMode !== 'waveform' ? 'marquee' : 'range';
      dragMode.current = {
        kind: 'dry-create',
        createKind: kind,
        anchorS: t,
        anchorHz: hz,
        origViewStart: viewStartS,
        origViewEnd: viewEndS,
      };
      setDraftRegion({
        kind,
        startS: t,
        endS: t,
        minHz: kind === 'marquee' ? (hz ?? 200) : null,
        maxHz: kind === 'marquee' ? (hz ?? 1800) : null,
      });
      return;
    }

    if (activeTool === 'range' || activeTool === 'marquee' || e.shiftKey) {
      dragMode.current = {
        kind: 'range',
        anchorS: t,
        anchorHz: hz,
        origViewStart: viewStartS,
        origViewEnd: viewEndS,
      };
      onSelectRange([t, t]);
      return;
    }

    dragMode.current = {
      kind: 'scrub',
      anchorS: t,
      anchorHz: hz,
      origViewStart: viewStartS,
      origViewEnd: viewEndS,
    };
    onSeek(t);
  };

  const handlePlotPointerMove = (
    e: React.PointerEvent<HTMLCanvasElement>,
    track: 'original' | 'result',
  ) => {
    const t = timeFromPlotPointer(e);
    const freqHz = freqFromPlotPointer(e);
    onCursorMove({ timeS: t, freqHz, track });

    const drag = dragMode.current;
    if (!drag) return;
    if (drag.kind === 'dry-create') {
      const a = Math.min(drag.anchorS, t);
      const b = Math.max(drag.anchorS, t);
      const isMarquee = drag.createKind === 'marquee';
      const h0 = drag.anchorHz ?? 400;
      const h1 = freqHz ?? h0;
      setDraftRegion({
        kind: drag.createKind ?? 'range',
        startS: a,
        endS: b,
        minHz: isMarquee ? Math.min(h0, h1) : null,
        maxHz: isMarquee ? Math.max(h0, h1) : null,
      });
    } else if (drag.kind === 'range') {
      const a = Math.min(drag.anchorS, t);
      const b = Math.max(drag.anchorS, t);
      onSelectRange(b - a > 0.01 ? [a, b] : null);
    } else if (drag.kind === 'scrub') {
      onSeek(t);
    }
  };

  const handlePlotPointerUp = () => {
    const drag = dragMode.current;
    dragMode.current = null;
    if (drag?.kind === 'dry-create' && draftRegion && onCreateDryRegion) {
      if (draftRegion.endS - draftRegion.startS >= 0.03) {
        onCreateDryRegion({
          kind: draftRegion.kind,
          startS: draftRegion.startS,
          endS: draftRegion.endS,
          minHz: draftRegion.minHz,
          maxHz: draftRegion.maxHz,
        });
      }
      setDraftRegion(null);
    }
  };

  const startRegionHandleDrag = (
    e: React.PointerEvent<HTMLDivElement>,
    region: DryRegion,
    handle: RegionHandle,
  ) => {
    if (!isDrySurfaceEditable || viewedLayerLabel) return;
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    onSelectDryRegion?.(region.id);
    onBeginDryRegionGesture?.();
    regionDragRef.current = {
      id: region.id,
      handle,
      origin: region,
      startClientX: e.clientX,
      startClientY: e.clientY,
      lastNext: region,
    };
  };

  const moveRegionHandleDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    const drag = regionDragRef.current;
    if (!drag || !onTransformDryRegion) return;
    e.stopPropagation();
    const plotW = Math.max(10, resSize.w - AXIS_W);
    const specH = displayMode === 'both' ? Math.max(20, Math.floor(resSize.h * 0.68)) : Math.max(20, resSize.h);
    const span = Math.max(0.05, viewEndS - viewStartS);
    const dx = e.clientX - drag.startClientX;
    const dy = e.clientY - drag.startClientY;
    const deltaS = (dx / plotW) * span;
    const deltaHz = -(dy / specH) * displayMaxHz;
    const next = transformRegionByHandle(
      drag.origin,
      drag.handle,
      deltaS,
      deltaHz,
      durationS,
      displayMaxHz,
    );
    drag.lastNext = next;
    onTransformDryRegion(drag.id, next, false);
  };

  const endRegionHandleDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    const drag = regionDragRef.current;
    if (!drag) return;
    e.stopPropagation();
    regionDragRef.current = null;
    onTransformDryRegion?.(drag.id, drag.lastNext, true);
  };

  const cancelRegionHandleDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    const drag = regionDragRef.current;
    if (!drag) return;
    e.stopPropagation();
    regionDragRef.current = null;
    onCancelDryRegionGesture?.();
  };

  const handlePlotWheel = (e: React.WheelEvent<HTMLCanvasElement>) => {
    if (durationS <= 0) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const plotW = Math.max(1, rect.width - AXIS_W);
    const rel = Math.max(0, Math.min(1, (e.clientX - rect.left - AXIS_W) / plotW));
    const span = viewEndS - viewStartS;
    const anchorT = viewStartS + rel * span;

    if (Math.abs(e.deltaX) > Math.abs(e.deltaY) || e.shiftKey) {
      const shiftAmt = ((e.deltaX || e.deltaY) / 400) * span;
      let ns = viewStartS + shiftAmt;
      let ne = viewEndS + shiftAmt;
      if (ns < 0) {
        ns = 0;
        ne = Math.min(durationS, span);
      }
      if (ne > durationS) {
        ne = durationS;
        ns = Math.max(0, durationS - span);
      }
      onZoomViewport(ns, ne);
      return;
    }

    const factor = e.deltaY > 0 ? 1.25 : 0.8;
    const nextSpan = Math.max(0.15, Math.min(durationS, span * factor));
    let ns = anchorT - rel * nextSpan;
    let ne = ns + nextSpan;
    if (ns < 0) {
      ns = 0;
      ne = Math.min(durationS, nextSpan);
    }
    if (ne > durationS) {
      ne = durationS;
      ns = Math.max(0, durationS - nextSpan);
    }
    onZoomViewport(ns, ne);
  };

  const handleOverviewPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (durationS <= 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    const rect = e.currentTarget.getBoundingClientRect();
    const t = Math.max(0, Math.min(1, (e.clientX - rect.left) / Math.max(1, rect.width))) * durationS;
    const span = viewEndS - viewStartS;
    let ns = t - span / 2;
    let ne = t + span / 2;
    if (ns < 0) {
      ns = 0;
      ne = Math.min(durationS, span);
    }
    if (ne > durationS) {
      ne = durationS;
      ns = Math.max(0, durationS - span);
    }
    onZoomViewport(ns, ne);
    dragMode.current = {
      kind: 'overview',
      anchorS: t,
      anchorHz: null,
      origViewStart: ns,
      origViewEnd: ne,
    };
  };

  const handleOverviewPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const drag = dragMode.current;
    if (!drag || drag.kind !== 'overview' || durationS <= 0) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const t = Math.max(0, Math.min(1, (e.clientX - rect.left) / Math.max(1, rect.width))) * durationS;
    const span = viewEndS - viewStartS;
    let ns = t - span / 2;
    let ne = t + span / 2;
    if (ns < 0) {
      ns = 0;
      ne = Math.min(durationS, span);
    }
    if (ne > durationS) {
      ne = durationS;
      ns = Math.max(0, durationS - span);
    }
    onZoomViewport(ns, ne);
  };

  const cursorClass =
    activeTool === 'range' || activeTool === 'marquee'
      ? 'cursor-crosshair'
      : activeTool === 'zoom'
        ? 'cursor-zoom-in'
        : 'cursor-default';

  const plotW = Math.max(10, resSize.w - AXIS_W);
  const specH = displayMode === 'both' ? Math.max(20, Math.floor(resSize.h * 0.68)) : resSize.h;
  const viewSpan = Math.max(0.05, viewEndS - viewStartS);
  const timeToX = (t: number) => AXIS_W + ((t - viewStartS) / viewSpan) * plotW;
  const freqToY = (hz: number) => Math.max(0, Math.min(specH, (1 - hz / displayMaxHz) * specH));

  return (
    <div ref={containerRef} className="flex-1 min-h-0 flex flex-col bg-[#1a1a1a]">
      {/* Overview strip */}
      <div className="h-7 bg-[#1d1d1d] border-b border-[#343434] flex items-center shrink-0">
        <div
          style={{ width: AXIS_W }}
          className="h-full border-r border-[#343434] flex items-center justify-center text-[10px] font-mono text-[#7a7a7a] shrink-0"
        >
          FULL
        </div>
        <canvas
          ref={overviewRef}
          onPointerDown={handleOverviewPointerDown}
          onPointerMove={handleOverviewPointerMove}
          onPointerUp={handlePlotPointerUp}
          className="flex-1 h-full block cursor-ew-resize"
        />
      </div>

      {/* Shared Time Ruler */}
      <div className="h-5 border-b border-[#343434] shrink-0">
        <canvas ref={rulerRef} className="w-full h-full block" />
      </div>

      {/* Stacked Original & Regenerated/Layer Plots */}
      <div className="flex-1 min-h-0 flex flex-col divide-y divide-[#343434]">
        {/* Top Track: Original (Evidence Only) */}
        <div className="flex-1 min-h-0 relative flex flex-col">
          <div className="absolute top-1.5 left-[58px] z-10 pointer-events-none flex items-center gap-2 bg-[#1a1a1a]/85 border border-[#363636] px-1.5 py-0.5 text-[11px]">
            <span className="w-2 h-2 bg-[#7ab8f5] inline-block" />
            <span className="font-medium text-[#e0e0e0]">Original</span>
            <span className="font-mono text-[10px] text-[#8f8f8f]">
              {showEvidenceOverlay
                ? 'evidence overlay: strong / weak / uncertain (visual only)'
                : 'read-only reference · never blended'}
            </span>
          </div>
          <canvas
            ref={origCanvasRef}
            onPointerDown={(e) => handlePlotPointerDown(e, 'original')}
            onPointerMove={(e) => handlePlotPointerMove(e, 'original')}
            onPointerUp={handlePlotPointerUp}
            onPointerLeave={() => onCursorMove(null)}
            onWheel={handlePlotWheel}
            className={`w-full h-full block ${cursorClass}`}
          />
        </div>

        {/* Bottom Track: Regenerated / Dry Take (with Stage 2 Surgical Region Overlays) */}
        <div ref={resTrackWrapRef} className="flex-1 min-h-0 relative flex flex-col overflow-hidden">
          <div className="absolute top-1.5 left-[58px] z-10 flex items-center gap-2 bg-[#1a1a1a]/90 border border-[#363636] px-1.5 py-0.5 text-[11px]">
            <span className="w-2 h-2 bg-[#4cc6ac] inline-block pointer-events-none" />
            <span className="font-medium text-[#e0e0e0] pointer-events-none">
              {viewedLayerLabel ? `Layer View: ${viewedLayerLabel}` : resultLabel}
            </span>
            {viewedLayerLabel ? (
              <span className="font-mono text-[10px] text-[#e5a93c] pointer-events-none">
                plot only (dry editing disabled)
              </span>
            ) : !isDrySurfaceEditable ? (
              <span className="flex items-center gap-1.5 font-mono text-[10px] text-[#e5a93c]">
                <span>Dry editing disabled on this view</span>
                {onSwitchToDrySurface && (
                  <button
                    type="button"
                    data-testid="switch-to-dry-to-edit-btn"
                    onClick={onSwitchToDrySurface}
                    className="px-1.5 py-0.2 bg-[#2a3f59] hover:bg-[#355070] border border-[#4b7bb5] text-[#e5f0ff] font-sans text-[10px]"
                  >
                    Switch to Dry to Edit
                  </button>
                )}
              </span>
            ) : (activeTool === 'marquee' || activeTool === 'range') ? (
              <span className="font-mono text-[10px] text-[#8bc2ff] pointer-events-none">
                Stage 2 Dry Edit: drag to mark {activeTool === 'marquee' ? 'time × freq marquee' : 'in/out range'}
              </span>
            ) : null}
          </div>
          <canvas
            ref={resCanvasRef}
            onPointerDown={(e) => handlePlotPointerDown(e, 'result')}
            onPointerMove={(e) => handlePlotPointerMove(e, 'result')}
            onPointerUp={handlePlotPointerUp}
            onPointerLeave={() => onCursorMove(null)}
            onWheel={handlePlotWheel}
            className={`w-full h-full block ${cursorClass}`}
          />

          {/* Interactive Stage 2 Surgical Dry Regions Overlay (Dry Take ONLY — Audit H7) */}
          {!viewedLayerLabel && isDrySurfaceEditable && (
            <div className="absolute inset-0 pointer-events-none z-20">
              {dryRegions.map((reg) => {
                if (reg.endS < viewStartS || reg.startS > viewEndS) return null;
                const x0 = Math.max(AXIS_W, timeToX(reg.startS));
                const x1 = Math.min(resSize.w, timeToX(reg.endS));
                const width = Math.max(8, x1 - x0);
                const isMarquee = reg.kind === 'marquee' && displayMode !== 'waveform' && reg.minHz !== null && reg.maxHz !== null;
                const top = isMarquee ? freqToY(reg.maxHz!) : 0;
                const bottom = isMarquee ? freqToY(reg.minHz!) : resSize.h;
                const height = Math.max(18, bottom - top);
                const isSelected = reg.id === selectedRegionId;

                return (
                  <div
                    key={reg.id}
                    data-dry-region-id={reg.id}
                    style={{ left: `${x0}px`, top: `${top}px`, width: `${width}px`, height: `${height}px` }}
                    onPointerDown={(e) => startRegionHandleDrag(e, reg, 'move')}
                    onPointerMove={moveRegionHandleDrag}
                    onPointerUp={endRegionHandleDrag}
                    onPointerCancel={cancelRegionHandleDrag}
                    className={`absolute pointer-events-auto cursor-move border transition-colors ${
                      isSelected
                        ? 'border-[#f0b44c] bg-[#f0b44c]/18 shadow-[0_0_0_1px_rgba(240,180,76,0.35)]'
                        : 'border-[#4cc6ac]/80 bg-[#4cc6ac]/12 hover:bg-[#4cc6ac]/20'
                    }`}
                  >
                    {/* Region Tag */}
                    <div className="absolute top-0.5 left-1 right-1 flex items-center justify-between gap-1 pointer-events-none select-none">
                      <span
                        className={`px-1 py-0.2 font-mono text-[9px] truncate ${
                          isSelected ? 'bg-[#2a2214]/90 text-[#f5ca7a]' : 'bg-[#162220]/90 text-[#7fe0cb]'
                        }`}
                      >
                        {reg.label} · {reg.action.replace('_', ' ')}
                        {reg.note ? ` · "${reg.note.slice(0, 22)}"` : ''}
                      </span>
                    </div>

                    {/* Left & Right edge resize handles */}
                    <div
                      onPointerDown={(e) => startRegionHandleDrag(e, reg, 'left')}
                      onPointerMove={moveRegionHandleDrag}
                      onPointerUp={endRegionHandleDrag}
                      className="absolute inset-y-0 -left-1 w-2 cursor-ew-resize"
                      title="Drag left edge to resize region start"
                    />
                    <div
                      onPointerDown={(e) => startRegionHandleDrag(e, reg, 'right')}
                      onPointerMove={moveRegionHandleDrag}
                      onPointerUp={endRegionHandleDrag}
                      className="absolute inset-y-0 -right-1 w-2 cursor-ew-resize"
                      title="Drag right edge to resize region end"
                    />

                    {/* Top, bottom & corner resize handles for rectangular marquee */}
                    {isMarquee && (
                      <>
                        <div
                          onPointerDown={(e) => startRegionHandleDrag(e, reg, 'top')}
                          onPointerMove={moveRegionHandleDrag}
                          onPointerUp={endRegionHandleDrag}
                          className="absolute inset-x-1 -top-1 h-2 cursor-ns-resize"
                          title="Drag top edge to adjust max Hz"
                        />
                        <div
                          onPointerDown={(e) => startRegionHandleDrag(e, reg, 'bottom')}
                          onPointerMove={moveRegionHandleDrag}
                          onPointerUp={endRegionHandleDrag}
                          className="absolute inset-x-1 -bottom-1 h-2 cursor-ns-resize"
                          title="Drag bottom edge to adjust min Hz"
                        />
                        <div
                          onPointerDown={(e) => startRegionHandleDrag(e, reg, 'top-left')}
                          onPointerMove={moveRegionHandleDrag}
                          onPointerUp={endRegionHandleDrag}
                          className="absolute -top-1.5 -left-1.5 w-2.5 h-2.5 bg-[#f0b44c] border border-[#1a1a1a] cursor-nwse-resize"
                        />
                        <div
                          onPointerDown={(e) => startRegionHandleDrag(e, reg, 'top-right')}
                          onPointerMove={moveRegionHandleDrag}
                          onPointerUp={endRegionHandleDrag}
                          className="absolute -top-1.5 -right-1.5 w-2.5 h-2.5 bg-[#f0b44c] border border-[#1a1a1a] cursor-nesw-resize"
                        />
                        <div
                          onPointerDown={(e) => startRegionHandleDrag(e, reg, 'bottom-left')}
                          onPointerMove={moveRegionHandleDrag}
                          onPointerUp={endRegionHandleDrag}
                          className="absolute -bottom-1.5 -left-1.5 w-2.5 h-2.5 bg-[#f0b44c] border border-[#1a1a1a] cursor-nesw-resize"
                        />
                        <div
                          onPointerDown={(e) => startRegionHandleDrag(e, reg, 'bottom-right')}
                          onPointerMove={moveRegionHandleDrag}
                          onPointerUp={endRegionHandleDrag}
                          className="absolute -bottom-1.5 -right-1.5 w-2.5 h-2.5 bg-[#f0b44c] border border-[#1a1a1a] cursor-nwse-resize"
                        />
                      </>
                    )}
                  </div>
                );
              })}

              {/* Live drag preview box while drawing a new region */}
              {draftRegion && draftRegion.endS > draftRegion.startS && (() => {
                const x0 = Math.max(AXIS_W, timeToX(draftRegion.startS));
                const x1 = Math.min(resSize.w, timeToX(draftRegion.endS));
                const isMarquee = draftRegion.kind === 'marquee' && displayMode !== 'waveform' && draftRegion.minHz !== null && draftRegion.maxHz !== null;
                const top = isMarquee ? freqToY(draftRegion.maxHz!) : 0;
                const bottom = isMarquee ? freqToY(draftRegion.minHz!) : resSize.h;
                return (
                  <div
                    style={{
                      left: `${x0}px`,
                      top: `${top}px`,
                      width: `${Math.max(4, x1 - x0)}px`,
                      height: `${Math.max(12, bottom - top)}px`,
                    }}
                    className="absolute border border-dashed border-[#8bc2ff] bg-[#378ef0]/20"
                  />
                );
              })()}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function drawTrackCanvas(opts: {
  canvas: HTMLCanvasElement | null;
  buffer: AudioBuffer | null;
  sampleRate: number;
  viewStartS: number;
  viewEndS: number;
  playheadS: number;
  selection: [number, number] | null;
  displayMode: DisplayMode;
  displayMaxHz: number;
  dbFloor: number;
  waveColor: string;
  showEvidenceOverlay: boolean;
  notes: NoteSummary[];
  frameEvidence: FrameEvidence[];
}) {
  const {
    canvas,
    buffer,
    sampleRate,
    viewStartS,
    viewEndS,
    playheadS,
    selection,
    displayMode,
    displayMaxHz,
    dbFloor,
    waveColor,
    showEvidenceOverlay,
    notes,
    frameEvidence,
  } = opts;
  if (!canvas) return;
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const rect = canvas.getBoundingClientRect();
  const w = Math.max(120, Math.floor(rect.width));
  const h = Math.max(60, Math.floor(rect.height));
  if (canvas.width !== Math.floor(w * dpr) || canvas.height !== Math.floor(h * dpr)) {
    canvas.width = Math.floor(w * dpr);
    canvas.height = Math.floor(h * dpr);
  }
  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  ctx.save();
  ctx.scale(dpr, dpr);

  ctx.fillStyle = '#202020';
  ctx.fillRect(0, 0, AXIS_W, h);
  ctx.fillStyle = '#15171c';
  ctx.fillRect(AXIS_W, 0, w - AXIS_W, h);

  const plotW = Math.max(10, w - AXIS_W);
  const span = Math.max(0.05, viewEndS - viewStartS);

  if (!buffer) {
    ctx.fillStyle = '#6f6f6f';
    ctx.font = '11px "IBM Plex Mono", monospace';
    ctx.fillText('Decoding PCM audio…', AXIS_W + 14, h / 2);
    ctx.restore();
    return;
  }

  const pcm = buffer.getChannelData(0);
  // Always use decoded AudioBuffer.sampleRate for PCM indexing and FFT frequencies (Audit H8).
  // Keep original file sampleRate strictly for Nyquist provenance ceiling.
  const decodedSampleRate = getEffectiveBufferSampleRate(buffer, sampleRate);
  const nyquistSampleRate = Math.min(decodedSampleRate, sampleRate > 0 ? sampleRate : decodedSampleRate);

  if (displayMode === 'spectrogram' || displayMode === 'both') {
    const specH = displayMode === 'both' ? Math.floor(h * 0.68) : h;
    const off = document.createElement('canvas');
    off.width = Math.min(plotW, 640);
    off.height = Math.min(specH, 240);
    const octx = off.getContext('2d');
    if (octx) {
      renderSpectrogramToCanvas(
        octx,
        pcm,
        decodedSampleRate,
        viewStartS,
        viewEndS,
        displayMaxHz,
        off.width,
        off.height,
        1024,
        dbFloor,
        -12,
        nyquistSampleRate,
      );
      ctx.imageSmoothingEnabled = true;
      ctx.drawImage(off, AXIS_W, 0, plotW, specH);
    }

    // Visual-only Cue Evidence Overlay (strong / weak / uncertain / out-of-target)
    if (showEvidenceOverlay && notes.length > 0) {
      for (const n of notes) {
        if (n.end < viewStartS || n.start > viewEndS) continue;
        const x0 = AXIS_W + Math.max(0, (n.start - viewStartS) / span) * plotW;
        const x1 = AXIS_W + Math.min(1, (n.end - viewStartS) / span) * plotW;
        const yHz = Math.max(4, Math.min(specH - 4, (1 - n.median_hz / displayMaxHz) * specH));
        const cls = classifyNoteEvidence(n);
        const col =
          cls === 'strong_coherent'
            ? '#4cc6ac'
            : cls === 'weak_consistent'
              ? '#e5a93c'
              : '#e07070';
        ctx.strokeStyle = col;
        ctx.lineWidth = cls === 'strong_coherent' ? 1.8 : 1.3;
        ctx.setLineDash(cls === 'uncertain' ? [3, 2] : []);
        ctx.beginPath();
        ctx.moveTo(x0, yHz);
        ctx.lineTo(Math.max(x0 + 2, x1), yHz);
        ctx.stroke();
      }
      ctx.setLineDash([]);

      // Mark out-of-target & uncertain frame evidence along top edge of spectrogram
      for (const f of frameEvidence) {
        if (f.time_s < viewStartS || f.time_s > viewEndS) continue;
        if (f.status !== 'uncertain' && f.status !== 'out_of_target') continue;
        const fx = AXIS_W + ((f.time_s - viewStartS) / span) * plotW;
        ctx.fillStyle = f.status === 'uncertain' ? 'rgba(229, 169, 60, 0.7)' : 'rgba(176, 132, 232, 0.75)';
        ctx.fillRect(fx, specH - 5, 2, 5);
      }
    }

    const nyq = computeNyquistDisplay(nyquistSampleRate, displayMaxHz);
    if (nyq.hasAboveNyquist) {
      const nyqY = Math.round(nyq.nyquistFractionFromTop * specH);
      ctx.strokeStyle = '#e5a93c';
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 3]);
      ctx.beginPath();
      ctx.moveTo(AXIS_W, nyqY + 0.5);
      ctx.lineTo(w, nyqY + 0.5);
      ctx.stroke();
      ctx.setLineDash([]);

      if (nyqY >= 16) {
        const nyqLabel = `No data (above Nyquist · ${(nyq.nyquistHz / 1000).toFixed(1)} kHz)`;
        ctx.font = '10px "IBM Plex Mono", monospace';
        const textW = ctx.measureText(nyqLabel).width;
        const boxW = textW + 12;
        const boxX = w - boxW - 8;
        const boxY = Math.max(4, Math.min(nyqY - 17, 6));
        ctx.fillStyle = 'rgba(22, 22, 24, 0.88)';
        ctx.fillRect(boxX, boxY, boxW, 15);
        ctx.strokeStyle = 'rgba(229, 169, 60, 0.55)';
        ctx.lineWidth = 1;
        ctx.strokeRect(boxX + 0.5, boxY + 0.5, boxW - 1, 14);
        ctx.fillStyle = '#e5a93c';
        ctx.textAlign = 'right';
        ctx.fillText(nyqLabel, w - 14, boxY + 11);
        ctx.textAlign = 'left';
      }
    }

    ctx.fillStyle = '#8d8d8d';
    ctx.font = '10px "IBM Plex Mono", monospace';
    const steps = 4;
    for (let i = 0; i <= steps; i++) {
      const frac = i / steps;
      const y = frac * (specH - 12) + 9;
      const hz = Math.round(displayMaxHz * (1 - frac));
      const label = hz >= 1000 ? `${(hz / 1000).toFixed(hz % 1000 === 0 ? 0 : 1)}k` : `${hz}`;
      ctx.fillText(label, 6, y);
      if (i > 0 && i < steps) {
        ctx.strokeStyle = 'rgba(255,255,255,0.06)';
        ctx.beginPath();
        ctx.moveTo(AXIS_W, y - 3);
        ctx.lineTo(w, y - 3);
        ctx.stroke();
      }
    }
  }

  if (displayMode === 'waveform' || displayMode === 'both') {
    const waveTop = displayMode === 'both' ? Math.floor(h * 0.68) : 0;
    const waveH = h - waveTop;
    if (displayMode === 'both') {
      ctx.fillStyle = '#181a1f';
      ctx.fillRect(AXIS_W, waveTop, plotW, waveH);
      ctx.strokeStyle = '#343434';
      ctx.beginPath();
      ctx.moveTo(0, waveTop + 0.5);
      ctx.lineTo(w, waveTop + 0.5);
      ctx.stroke();
    }

    const midY = waveTop + waveH / 2;
    ctx.strokeStyle = '#2d3138';
    ctx.beginPath();
    ctx.moveTo(AXIS_W, midY + 0.5);
    ctx.lineTo(w, midY + 0.5);
    ctx.stroke();

    const cols = computeWaveformColumns(pcm, decodedSampleRate, viewStartS, viewEndS, plotW);
    const amp = waveH * 0.44;
    ctx.fillStyle = waveColor;
    for (let i = 0; i < cols.length; i++) {
      const y0 = midY - cols[i].max * amp;
      const y1 = midY - cols[i].min * amp;
      ctx.fillRect(AXIS_W + i, y0, 1, Math.max(1, y1 - y0));
    }

    ctx.fillStyle = '#8d8d8d';
    ctx.font = '10px "IBM Plex Mono", monospace';
    ctx.fillText('+1.0', 6, waveTop + 11);
    ctx.fillText(' 0.0', 6, midY + 3);
    if (waveH > 44) {
      ctx.fillText('-1.0', 6, waveTop + waveH - 4);
    }
  }

  ctx.strokeStyle = '#383838';
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(AXIS_W + 0.5, 0);
  ctx.lineTo(AXIS_W + 0.5, h);
  ctx.stroke();

  if (selection) {
    const [s0, s1] = selection;
    const x0 = AXIS_W + ((s0 - viewStartS) / span) * plotW;
    const x1 = AXIS_W + ((s1 - viewStartS) / span) * plotW;
    if (x1 > AXIS_W && x0 < w) {
      const cx0 = Math.max(AXIS_W, x0);
      const cx1 = Math.min(w, x1);
      ctx.fillStyle = 'rgba(76, 198, 172, 0.16)';
      ctx.fillRect(cx0, 0, Math.max(1, cx1 - cx0), h);
      ctx.strokeStyle = 'rgba(76, 198, 172, 0.78)';
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(cx0 + 0.5, 0);
      ctx.lineTo(cx0 + 0.5, h);
      ctx.moveTo(cx1 + 0.5, 0);
      ctx.lineTo(cx1 + 0.5, h);
      ctx.stroke();
    }
  }

  if (playheadS >= viewStartS && playheadS <= viewEndS) {
    const px = AXIS_W + ((playheadS - viewStartS) / span) * plotW;
    ctx.strokeStyle = '#ffffff';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(Math.round(px) + 0.5, 0);
    ctx.lineTo(Math.round(px) + 0.5, h);
    ctx.stroke();
  }

  ctx.restore();
}
