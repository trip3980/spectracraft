import React, { useEffect, useRef, useState } from 'react';
import {
  divergingDiffRgb,
  formatFreqLabel,
  magmaRgb,
  sampleSpectrogramProbe,
  type ReportFinding,
  type ReportViewerPlots,
  type SpectrogramProbe,
} from '../lib/technicalReport.ts';

const LOG_TICKS_HZ = [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000];

export function TagBadge({ tag }: { tag: 'measured' | 'inferred' }) {
  return (
    <span
      className={`inline-block px-1.5 py-0.5 text-[9px] font-mono uppercase tracking-wider border shrink-0 ${
        tag === 'measured'
          ? 'bg-[#122a38] border-[#286b8f] text-[#6cd4f8]'
          : 'bg-[#332611] border-[#825c22] text-[#f5bf5b]'
      }`}
    >
      {tag}
    </span>
  );
}

export function FindingsTable({
  title,
  subtitle,
  scopeBadge,
  findings,
  compact = false,
}: {
  title: string;
  subtitle: string;
  scopeBadge: string;
  findings: ReportFinding[];
  compact?: boolean;
}) {
  return (
    <section className="bg-[#151820] border border-[#2a3140] flex flex-col">
      <div
        className={`${
          compact ? 'px-3 py-2' : 'px-3.5 py-2.5'
        } bg-[#1b202b] border-b border-[#2a3140] flex items-center justify-between gap-2`}
      >
        <div className="min-w-0">
          <h3 className="text-[11.5px] font-semibold tracking-tight text-[#eef3fb] truncate">
            {title}
          </h3>
          <p className="text-[10px] text-[#8d99ad] truncate">{subtitle}</p>
        </div>
        <span className="px-1.5 py-0.5 bg-[#10141c] border border-[#323d52] font-mono text-[9px] text-[#9eb4d6] shrink-0">
          {scopeBadge}
        </span>
      </div>
      <div className="divide-y divide-[#222836]">
        {findings.map((f, idx) => (
          <div
            key={`${f.category}-${f.label}-${idx}`}
            className={`${compact ? 'px-3 py-1.5' : 'px-3.5 py-2'} space-y-0.5`}
          >
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-1.5 min-w-0">
                <TagBadge tag={f.tag} />
                <span className="text-[10.5px] font-medium text-[#c5d1e4] truncate">
                  {f.category}
                </span>
                <span className="text-[10px] text-[#7f8ba1] truncate">· {f.label}</span>
              </div>
            </div>
            <div
              className={`font-mono text-[11px] text-[#f1f5fc] break-words tabular-nums ${
                compact ? 'pl-1' : 'pl-[66px]'
              }`}
            >
              {f.value}
            </div>
            <div className={`text-[10px] text-[#7d899f] ${compact ? 'pl-1' : 'pl-[66px]'}`}>
              {f.detail}
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

export function WaveformComparisonCanvas({
  plots,
  compact = false,
  testId = 'report-waveform-canvas',
}: {
  plots: ReportViewerPlots;
  compact?: boolean;
  testId?: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const cssH = compact ? 148 : 190;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const width = compact ? 460 : 920;
    const height = cssH;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    ctx.fillStyle = '#0e1117';
    ctx.fillRect(0, 0, width, height);

    const padL = compact ? 44 : 56;
    const padR = 10;
    const w = width - padL - padR;
    const laneH = compact ? 54 : 74;
    const y2 = compact ? 74 : 96;
    const inv = 1 / Math.max(1e-4, plots.waveform_scale_linear);

    const drawLane = (
      bins: [number, number][],
      yTop: number,
      label: string,
      color: string,
      fillColor: string,
    ) => {
      ctx.fillStyle = '#141922';
      ctx.fillRect(padL, yTop, w, laneH);
      ctx.strokeStyle = '#252d3c';
      ctx.strokeRect(padL + 0.5, yTop + 0.5, w - 1, laneH - 1);

      const mid = yTop + laneH / 2;
      ctx.strokeStyle = '#242c3b';
      ctx.beginPath();
      ctx.moveTo(padL, mid);
      ctx.lineTo(padL + w, mid);
      ctx.stroke();

      const half = laneH * 0.44;
      if (bins.length > 0) {
        ctx.fillStyle = fillColor;
        ctx.strokeStyle = color;
        ctx.lineWidth = compact ? 1.0 : 1.2;
        for (let i = 0; i < bins.length; i++) {
          const [lo, hi] = bins[i];
          const x = padL + ((i + 0.5) / bins.length) * w;
          const yHi = mid - Math.max(-1, Math.min(1, hi * inv)) * half;
          const yLo = mid - Math.max(-1, Math.min(1, lo * inv)) * half;
          ctx.beginPath();
          ctx.moveTo(x, yHi);
          ctx.lineTo(x, Math.max(yHi + 1, yLo));
          ctx.stroke();
        }
      }

      ctx.font = compact
        ? '9px ui-monospace, SFMono-Regular, Menlo, monospace'
        : '10px ui-monospace, SFMono-Regular, Menlo, monospace';
      const labelW = ctx.measureText(label).width;
      ctx.fillStyle = 'rgba(14, 17, 23, 0.84)';
      ctx.fillRect(padL + 4, yTop + 2, labelW + 8, 13);
      ctx.fillStyle = '#cfd9ea';
      ctx.fillText(label, padL + 8, yTop + 12);
      ctx.fillStyle = '#7e8ba3';
      ctx.fillText(`+${plots.waveform_scale_linear.toFixed(2)}`, 4, yTop + 11);
      ctx.fillText(' 0.00', 4, mid + 3);
      ctx.fillText(`-${plots.waveform_scale_linear.toFixed(2)}`, 4, yTop + laneH - 3);
    };

    drawLane(
      plots.waveform_before,
      compact ? 8 : 10,
      compact ? 'ORIGINAL (RAW)' : 'ORIGINAL (RAW INPUT)',
      '#4bc3f2',
      'rgba(75,195,242,0.22)',
    );
    drawLane(
      plots.waveform_after,
      y2,
      compact ? 'FINAL EXPORT (SAME SCALE)' : 'FINAL EXPORT (SAME VERTICAL SCALE)',
      '#45d1a9',
      'rgba(69,209,169,0.22)',
    );

    ctx.fillStyle = '#8b97ad';
    ctx.font = compact
      ? '9px ui-monospace, SFMono-Regular, Menlo, monospace'
      : '10px ui-monospace, SFMono-Regular, Menlo, monospace';
    const ticks = compact ? 4 : 5;
    for (let i = 0; i <= ticks; i++) {
      const t = (plots.duration_s * i) / ticks;
      const x = padL + (w * i) / ticks;
      ctx.fillText(`${t.toFixed(2)}s`, Math.min(width - 38, x - 8), height - 4);
    }
  }, [plots, compact, cssH]);

  return (
    <canvas
      ref={canvasRef}
      data-testid={testId}
      style={{ height: `${cssH}px` }}
      className="w-full block border border-[#283142]"
    />
  );
}

export function SpectrogramCanvas({
  title,
  grid,
  cols,
  rows,
  durationS,
  maxHz,
  freqHz,
  mode,
  dbRange,
  diffMaxAbs,
  testId,
  compact = false,
}: {
  title: string;
  grid: number[];
  cols: number;
  rows: number;
  durationS: number;
  maxHz: number;
  freqHz?: number[];
  mode: 'magnitude' | 'diff';
  dbRange: [number, number];
  diffMaxAbs: number;
  testId: string;
  compact?: boolean;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [probe, setProbe] = useState<SpectrogramProbe | null>(null);
  const cssH = compact ? 124 : 164;
  const logicalW = compact ? 460 : 920;
  const padL = compact ? 40 : 56;
  const padT = compact ? 18 : 20;
  const padR = 10;
  const padB = compact ? 18 : 20;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const width = logicalW;
    const height = cssH;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    ctx.fillStyle = '#0e1117';
    ctx.fillRect(0, 0, width, height);

    const plotW = width - padL - padR;
    const plotH = height - padT - padB;

    const off = document.createElement('canvas');
    off.width = Math.max(1, cols);
    off.height = Math.max(1, rows);
    const offCtx = off.getContext('2d');
    if (offCtx && cols > 0 && rows > 0) {
      const img = offCtx.createImageData(cols, rows);
      const span = Math.max(1, dbRange[1] - dbRange[0]);
      for (let r = 0; r < rows; r++) {
        const yRow = rows - 1 - r;
        for (let c = 0; c < cols; c++) {
          const val = grid[r * cols + c] ?? dbRange[0];
          const [R, G, B] =
            mode === 'diff'
              ? divergingDiffRgb(val, diffMaxAbs)
              : magmaRgb((val - dbRange[0]) / span);
          const idx = (yRow * cols + c) * 4;
          img.data[idx] = R;
          img.data[idx + 1] = G;
          img.data[idx + 2] = B;
          img.data[idx + 3] = 255;
        }
      }
      offCtx.putImageData(img, 0, 0);
      ctx.imageSmoothingEnabled = true;
      ctx.drawImage(off, padL, padT, plotW, plotH);
    }

    ctx.strokeStyle = '#2d374a';
    ctx.strokeRect(padL + 0.5, padT + 0.5, plotW - 1, plotH - 1);

    const logLo = Math.log(20);
    const logHi = Math.log(Math.max(40, maxHz));
    ctx.font = compact
      ? '8.5px ui-monospace, SFMono-Regular, Menlo, monospace'
      : '9.5px ui-monospace, SFMono-Regular, Menlo, monospace';
    for (const hz of LOG_TICKS_HZ) {
      if (hz > maxHz * 1.02) continue;
      const u = (Math.log(hz) - logLo) / (logHi - logLo);
      const y = padT + plotH * (1 - Math.max(0, Math.min(1, u)));
      ctx.strokeStyle =
        hz === 100 || hz === 200 ? 'rgba(245, 191, 91, 0.35)' : 'rgba(255,255,255,0.08)';
      ctx.beginPath();
      ctx.moveTo(padL, y);
      ctx.lineTo(padL + plotW, y);
      ctx.stroke();
      const labelTicks = compact ? [20, 100, 1000, 10000] : [20, 100, 200, 1000, 5000, 20000];
      if (labelTicks.includes(hz)) {
        ctx.fillStyle = hz === 100 || hz === 200 ? '#f5bf5b' : '#8d99ad';
        ctx.fillText(formatFreqLabel(hz), 5, y + 3);
      }
    }

    ctx.fillStyle = '#e8eef8';
    ctx.font = compact
      ? 'bold 9.5px ui-monospace, SFMono-Regular, Menlo, monospace'
      : 'bold 10.5px ui-monospace, SFMono-Regular, Menlo, monospace';
    ctx.fillText(title, padL, 12);
    const scaleText =
      mode === 'diff'
        ? compact
          ? `±${diffMaxAbs.toFixed(0)} dB`
          : `Scale: -${diffMaxAbs.toFixed(0)} to +${diffMaxAbs.toFixed(0)} dB (Cyan=Cut, Amber=Boost)`
        : compact
          ? `${dbRange[0].toFixed(0)}..${dbRange[1].toFixed(0)} dBFS`
          : `Scale: ${dbRange[0].toFixed(0)} to ${dbRange[1].toFixed(0)} dBFS · 20 Hz–${formatFreqLabel(maxHz)} Hz`;
    ctx.fillStyle = '#8b98ae';
    ctx.font = compact
      ? '8.5px ui-monospace, SFMono-Regular, Menlo, monospace'
      : '9.5px ui-monospace, SFMono-Regular, Menlo, monospace';
    const textW = ctx.measureText(scaleText).width;
    ctx.fillText(scaleText, width - padR - textW, 12);

    ctx.fillStyle = '#8b98ae';
    const ticks = compact ? 4 : 5;
    for (let i = 0; i <= ticks; i++) {
      const t = (durationS * i) / ticks;
      const x = padL + (plotW * i) / ticks;
      ctx.fillText(`${t.toFixed(2)}s`, Math.min(width - 38, x - 8), height - 4);
    }
  }, [title, grid, cols, rows, durationS, maxHz, mode, dbRange, diffMaxAbs, compact, cssH, logicalW, padL, padT, padR, padB]);

  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return;
    const xLog = ((e.clientX - rect.left) / rect.width) * logicalW;
    const yLog = ((e.clientY - rect.top) / rect.height) * cssH;
    const plotW = logicalW - padL - padR;
    const plotH = cssH - padT - padB;
    const uX = (xLog - padL) / plotW;
    const uY = 1 - (yLog - padT) / plotH;
    setProbe(
      sampleSpectrogramProbe({
        uX,
        uY,
        grid,
        cols,
        rows,
        durationS,
        maxHz,
        freqHz,
      }),
    );
  };

  return (
    <div className="relative">
      <canvas
        ref={canvasRef}
        data-testid={testId}
        onMouseMove={handleMouseMove}
        onMouseLeave={() => setProbe(null)}
        style={{ height: `${cssH}px` }}
        className="w-full block border border-[#283142] cursor-crosshair"
      />
      {probe && (
        <div
          data-testid={`${testId}-probe`}
          className="pointer-events-none absolute bottom-5 right-2 px-1.5 py-0.5 bg-[#0b0e14]/90 border border-[#3a4a66] font-mono text-[9.5px] text-[#dce8fa] tabular-nums"
        >
          {probe.timeS.toFixed(2)}s · {formatFreqLabel(probe.freqHz)} Hz ·{' '}
          {mode === 'diff'
            ? `${probe.valueDb >= 0 ? '+' : ''}${probe.valueDb.toFixed(1)} dB`
            : `${probe.valueDb.toFixed(1)} dBFS`}
        </div>
      )}
    </div>
  );
}

export function ThirdOctaveCanvas({
  plots,
  compact = false,
  testId = 'report-third-octave-canvas',
}: {
  plots: ReportViewerPlots;
  compact?: boolean;
  testId?: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const cssH = compact ? 148 : 186;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const width = 452;
    const height = cssH;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    ctx.fillStyle = '#0e1117';
    ctx.fillRect(0, 0, width, height);

    const padL = 40;
    const padT = 22;
    const padR = 10;
    const padB = 24;
    const plotW = width - padL - padR;
    const plotH = height - padT - padB;
    const [dbMin, dbMax] = plots.third_octave.db_range;
    const span = Math.max(1, dbMax - dbMin);

    ctx.fillStyle = '#141922';
    ctx.fillRect(padL, padT, plotW, plotH);
    ctx.strokeStyle = '#252d3c';
    ctx.strokeRect(padL + 0.5, padT + 0.5, plotW - 1, plotH - 1);

    ctx.font = '9px ui-monospace, SFMono-Regular, Menlo, monospace';
    for (const db of [-90, -60, -30, 0]) {
      const u = (db - dbMin) / span;
      const y = padT + plotH * (1 - u);
      ctx.strokeStyle = 'rgba(255,255,255,0.08)';
      ctx.beginPath();
      ctx.moveTo(padL, y);
      ctx.lineTo(padL + plotW, y);
      ctx.stroke();
      ctx.fillStyle = '#7e8ba3';
      ctx.fillText(`${db}`, 8, y + 3);
    }

    const n = plots.third_octave.centers_hz.length;
    const slot = plotW / Math.max(1, n);
    for (let i = 0; i < n; i++) {
      const fc = plots.third_octave.centers_hz[i];
      const x = padL + i * slot;
      const ub = Math.max(0, Math.min(1, ((plots.third_octave.before_db[i] ?? dbMin) - dbMin) / span));
      const ua = Math.max(0, Math.min(1, ((plots.third_octave.after_db[i] ?? dbMin) - dbMin) / span));
      const bw = Math.max(2, slot * 0.4);

      if (Math.abs(fc - 100) < 2 || Math.abs(fc - 200) < 2) {
        ctx.fillStyle = 'rgba(245, 191, 91, 0.12)';
        ctx.fillRect(x, padT, slot, plotH);
      }

      ctx.fillStyle = '#4bc3f2';
      ctx.fillRect(x + 1, padT + plotH * (1 - ub), bw, plotH * ub);
      ctx.fillStyle = '#45d1a9';
      ctx.fillRect(x + 1 + bw, padT + plotH * (1 - ua), bw, plotH * ua);

      if (i % 4 === 0 || Math.abs(fc - 100) < 2 || Math.abs(fc - 200) < 2) {
        ctx.fillStyle = Math.abs(fc - 100) < 2 || Math.abs(fc - 200) < 2 ? '#f5bf5b' : '#8b98ae';
        ctx.fillText(formatFreqLabel(fc), x, height - 7);
      }
    }

    ctx.fillStyle = '#e8eef8';
    ctx.font = 'bold 10px ui-monospace, SFMono-Regular, Menlo, monospace';
    ctx.fillText('1/3-OCTAVE SPECTRUM (CYAN=BEFORE · EMERALD=AFTER)', padL, 14);
  }, [plots, cssH]);

  return (
    <canvas
      ref={canvasRef}
      data-testid={testId}
      style={{ height: `${cssH}px` }}
      className="w-full block border border-[#283142]"
    />
  );
}

export function UpperBandCanvas({
  plots,
  compact = false,
  testId = 'report-upper-band-canvas',
}: {
  plots: ReportViewerPlots;
  compact?: boolean;
  testId?: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const cssH = compact ? 148 : 186;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const width = 452;
    const height = cssH;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    ctx.fillStyle = '#0e1117';
    ctx.fillRect(0, 0, width, height);

    const padL = 36;
    const padT = 22;
    const padR = 10;
    const padB = 24;
    const plotW = width - padL - padR;
    const plotH = height - padT - padB;

    ctx.fillStyle = '#141922';
    ctx.fillRect(padL, padT, plotW, plotH);
    ctx.strokeStyle = '#252d3c';
    ctx.strokeRect(padL + 0.5, padT + 0.5, plotW - 1, plotH - 1);

    ctx.font = '9px ui-monospace, SFMono-Regular, Menlo, monospace';
    for (const v of [0, 0.5, 1.0]) {
      const y = padT + plotH * (1 - v);
      ctx.strokeStyle = 'rgba(255,255,255,0.08)';
      ctx.beginPath();
      ctx.moveTo(padL, y);
      ctx.lineTo(padL + plotW, y);
      ctx.stroke();
      ctx.fillStyle = '#7e8ba3';
      ctx.fillText(v.toFixed(1), 8, y + 3);
    }

    const drawCurve = (vals: number[], color: string, dashed = false) => {
      if (vals.length < 2) return;
      ctx.save();
      ctx.strokeStyle = color;
      ctx.lineWidth = 1.4;
      if (dashed) ctx.setLineDash([4, 3]);
      ctx.beginPath();
      for (let i = 0; i < vals.length; i++) {
        const x = padL + (i / (vals.length - 1)) * plotW;
        const y = padT + plotH * (1 - Math.max(0, Math.min(1, vals[i])));
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.stroke();
      ctx.restore();
    };

    drawCurve(plots.upper_band.before_phase_coherence, '#4bc3f2', false);
    drawCurve(plots.upper_band.after_phase_coherence, '#45d1a9', false);
    drawCurve(plots.upper_band.after_modulation_depth, '#f5bf5b', true);

    ctx.fillStyle = '#e8eef8';
    ctx.font = 'bold 10px ui-monospace, SFMono-Regular, Menlo, monospace';
    ctx.fillText('UPPER-BAND (4–16 KHZ) COHERENCE & MODULATION', padL, 14);
    ctx.fillStyle = '#8b98ae';
    ctx.font = '9px ui-monospace, SFMono-Regular, Menlo, monospace';
    ctx.fillText('Cyan: Before Coh · Emerald: After Coh · Amber (dashed): After Mod', padL, height - 7);
  }, [plots, cssH]);

  return (
    <canvas
      ref={canvasRef}
      data-testid={testId}
      style={{ height: `${cssH}px` }}
      className="w-full block border border-[#283142]"
    />
  );
}
