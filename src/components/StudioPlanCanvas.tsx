import React, { useEffect, useRef, useState } from 'react';
import {
  aimDirectionFromAngles,
  getMicWorldPosM,
  getSourcePosM,
  metersToDisplay,
  niceGridStepM,
  polarGain,
  unitDecimals,
  type PolarPattern,
  type RustStudioCatalogs,
  type StudioRigState,
  type Vec3,
} from '../lib/studioRig.ts';
import type { SpatialDisplayLayout } from '../lib/workspaceLayout.ts';

interface StudioPlanCanvasProps {
  rig: StudioRigState;
  catalogs: RustStudioCatalogs | null;
  onMoveMic: (micIdx: number, worldPos: Vec3, commit: boolean) => void;
  onSelectMic: (micIdx: number) => void;
  onMoveSource: (worldPos: Vec3, commit: boolean) => void;
  onTogglePatterns: (show: boolean) => void;
  onToggleGuidanceOverlay?: (show: boolean) => void;
  onToggleGridSnap: (snap: boolean) => void;
  /** Controlled spatial mode (plans | 3d | split) for workspace layout persistence. */
  spatialMode?: SpatialDisplayLayout;
  onChangeSpatialMode?: (mode: SpatialDisplayLayout) => void;
}

type Projection3D = 'perspective' | 'orthographic';

interface PlanLayout {
  padLeft: number;
  padRight: number;
  topRect: { x: number; y: number; w: number; h: number };
  frontRect: { x: number; y: number; w: number; h: number };
  pxPerM: number;
  originX: number;
  topOriginY: number;
  frontOriginY: number;
  stepM: number;
  stepUnit: number;
}

const DEFAULT_CAMERA = { yaw: -0.64, pitch: 0.44, zoom: 1.0, panX: 0, panY: 0 };

export function StudioPlanCanvas({
  rig,
  catalogs,
  onMoveMic,
  onSelectMic,
  onMoveSource,
  onTogglePatterns,
  onToggleGuidanceOverlay,
  onToggleGridSnap,
  spatialMode,
  onChangeSpatialMode,
}: StudioPlanCanvasProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const canvas3dRef = useRef<HTMLCanvasElement | null>(null);
  const [resizeTick, setResizeTick] = useState(0);
  const [internalMode, setInternalMode] = useState<SpatialDisplayLayout>('plans');
  const mode = spatialMode ?? internalMode;
  const setMode = (next: SpatialDisplayLayout) => {
    setInternalMode(next);
    onChangeSpatialMode?.(next);
  };
  const [projection, setProjection] = useState<Projection3D>('perspective');
  // Camera state is 100% visual-only — orbiting/panning/zooming NEVER touches rig or audio
  const [camera, setCamera] = useState(DEFAULT_CAMERA);
  const [planZoom, setPlanZoom] = useState(1.0);
  const [planPanX, setPlanPanX] = useState(0); // shared horizontal X pan in pixels
  const layoutRef = useRef<PlanLayout | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => setResizeTick((t) => t + 1));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const dragRef = useRef<
    | { kind: 'source'; view: 'top' | 'front' }
    | { kind: 'mic'; micIdx: number; view: 'top' | 'front' }
    | { kind: 'plan_pan'; startX: number; origPanX: number }
    | { kind: 'orbit'; startX: number; startY: number; origYaw: number; origPitch: number }
    | { kind: 'pan3d'; startX: number; startY: number; origPanX: number; origPanY: number }
    | null
  >(null);

  const activeSource = catalogs?.sources[rig.sourceIndex] ?? null;
  const sourceSymbol = activeSource?.symbol ?? 'guitar';

  const getEffectivePattern = (micIdx: number): PolarPattern => {
    const slot = rig.mics[micIdx];
    if (!slot) return 'cardioid';
    if (slot.patternOverride) return slot.patternOverride;
    return catalogs?.mics[slot.catalogIndex]?.default_pattern ?? 'cardioid';
  };

  useEffect(() => {
    void resizeTick;
    const dpr = Math.min(2, window.devicePixelRatio || 1);

    const drawInto = (
      canvas: HTMLCanvasElement | null,
      kind: 'plans' | '3d',
    ) => {
      if (!canvas) return;
      const rect = canvas.getBoundingClientRect();
      const w = Math.max(140, Math.floor(rect.width));
      const h = Math.max(120, Math.floor(rect.height));
      if (canvas.width !== Math.floor(w * dpr) || canvas.height !== Math.floor(h * dpr)) {
        canvas.width = Math.floor(w * dpr);
        canvas.height = Math.floor(h * dpr);
      }
      const ctx = canvas.getContext('2d');
      if (!ctx) return;
      ctx.save();
      ctx.scale(dpr, dpr);
      ctx.fillStyle = '#1b1b1b';
      ctx.fillRect(0, 0, w, h);
      if (kind === 'plans') {
        const layout = computePlanLayout(w, h, rig, planZoom, planPanX);
        layoutRef.current = layout;
        drawPlansView(ctx, w, h, layout, rig, sourceSymbol, getEffectivePattern);
      } else {
        draw3dGridView(ctx, w, h, rig, sourceSymbol, getEffectivePattern, camera, projection);
      }
      ctx.restore();
    };

    if (mode === 'plans') {
      drawInto(canvasRef.current, 'plans');
    } else if (mode === '3d') {
      layoutRef.current = null;
      drawInto(canvas3dRef.current, '3d');
    } else {
      // Split mode: Plans & 3D simultaneously synced to the exact same StudioRigState
      drawInto(canvasRef.current, 'plans');
      drawInto(canvas3dRef.current, '3d');
    }
  }, [rig, sourceSymbol, mode, camera, projection, planZoom, planPanX, resizeTick, catalogs]);

  const snapIfNeeded = (valM: number, shiftKey: boolean): number => {
    if (!rig.gridSnap || shiftKey) return valM;
    const stepM = layoutRef.current?.stepM ?? 0.3048;
    return Math.round(valM / stepM) * stepM;
  };

  const pointerToRoomPos = (
    e: React.PointerEvent<HTMLCanvasElement>,
    view: 'top' | 'front',
    current: Vec3,
  ): Vec3 | null => {
    const layout = layoutRef.current;
    const canvas = canvasRef.current;
    if (!layout || !canvas) return null;
    const rect = canvas.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    const rawX = (px - layout.originX) / layout.pxPerM;
    const nextX = snapIfNeeded(rawX, e.shiftKey);
    if (view === 'top') {
      const rawY = (layout.topOriginY - py) / layout.pxPerM;
      return { x: nextX, y: snapIfNeeded(rawY, e.shiftKey), z: current.z };
    }
    const rawZ = (layout.frontOriginY - py) / layout.pxPerM;
    return { x: nextX, y: current.y, z: snapIfNeeded(rawZ, e.shiftKey) };
  };

  const handlePointerDown3d = (e: React.PointerEvent<HTMLCanvasElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    if (e.shiftKey) {
      dragRef.current = {
        kind: 'pan3d',
        startX: e.clientX,
        startY: e.clientY,
        origPanX: camera.panX,
        origPanY: camera.panY,
      };
    } else {
      dragRef.current = {
        kind: 'orbit',
        startX: e.clientX,
        startY: e.clientY,
        origYaw: camera.yaw,
        origPitch: camera.pitch,
      };
    }
  };

  const handlePointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    const rect = e.currentTarget.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;

    if (mode === '3d') {
      handlePointerDown3d(e);
      return;
    }

    const layout = layoutRef.current;
    if (!layout) return;
    const inTop =
      py >= layout.topRect.y && py <= layout.topRect.y + layout.topRect.h;
    const inFront =
      py >= layout.frontRect.y && py <= layout.frontRect.y + layout.frontRect.h;
    const view: 'top' | 'front' | null = inTop ? 'top' : inFront ? 'front' : null;
    if (!view) return;

    const toScreen = (p: Vec3) => ({
      x: layout.originX + p.x * layout.pxPerM,
      y:
        view === 'top'
          ? layout.topOriginY - p.y * layout.pxPerM
          : layout.frontOriginY - p.z * layout.pxPerM,
    });

    // Hit-test mics first (radius 14 px) — dragging empty space NEVER edits source or mic!
    for (let i = 0; i < rig.mics.length; i++) {
      const sp = toScreen(getMicWorldPosM(rig, i));
      if (Math.hypot(px - sp.x, py - sp.y) <= 14) {
        onSelectMic(i);
        dragRef.current = { kind: 'mic', micIdx: i, view };
        return;
      }
    }

    // Hit-test source (radius 14 px)
    const srcPt = toScreen(getSourcePosM(rig));
    if (Math.hypot(px - srcPt.x, py - srcPt.y) <= 14) {
      dragRef.current = { kind: 'source', view };
      return;
    }

    // Bare drag on empty plan space pans the shared X viewport only (never moves source/mic!)
    dragRef.current = { kind: 'plan_pan', startX: e.clientX, origPanX: planPanX };
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const d = dragRef.current;
    if (!d) return;
    if (d.kind === 'orbit') {
      setCamera((c) => ({
        ...c,
        yaw: d.origYaw + (e.clientX - d.startX) * 0.011,
        pitch: Math.max(0.08, Math.min(1.42, d.origPitch + (e.clientY - d.startY) * 0.011)),
      }));
      return;
    }
    if (d.kind === 'pan3d') {
      setCamera((c) => ({
        ...c,
        panX: d.origPanX + (e.clientX - d.startX),
        panY: d.origPanY + (e.clientY - d.startY),
      }));
      return;
    }
    if (d.kind === 'plan_pan') {
      setPlanPanX(d.origPanX + (e.clientX - d.startX));
      return;
    }
    if (d.kind === 'source') {
      const next = pointerToRoomPos(e, d.view, getSourcePosM(rig));
      if (next) onMoveSource(next, false);
      return;
    }
    if (d.kind === 'mic') {
      const next = pointerToRoomPos(e, d.view, getMicWorldPosM(rig, d.micIdx));
      if (next) onMoveMic(d.micIdx, next, false);
    }
  };

  const handlePointerUp = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const d = dragRef.current;
    if (!d) return;
    dragRef.current = null;
    if (d.kind === 'source') {
      const next = pointerToRoomPos(e, d.view, getSourcePosM(rig));
      if (next) onMoveSource(next, true);
    } else if (d.kind === 'mic') {
      const next = pointerToRoomPos(e, d.view, getMicWorldPosM(rig, d.micIdx));
      if (next) onMoveMic(d.micIdx, next, true);
    }
  };

  const handleWheel3d = (e: React.WheelEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.12 : 0.89;
    setCamera((c) => ({ ...c, zoom: Math.max(0.45, Math.min(3.5, c.zoom * factor)) }));
  };

  const handleWheel = (e: React.WheelEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.12 : 0.89;
    if (mode === '3d') {
      setCamera((c) => ({ ...c, zoom: Math.max(0.45, Math.min(3.5, c.zoom * factor)) }));
    } else {
      setPlanZoom((z) => Math.max(0.6, Math.min(4.0, z * factor)));
    }
  };

  const stepInfo = niceGridStepM(rig.unit, Math.max(rig.roomMeters.x, rig.roomMeters.y) / planZoom);
  const sqX = Math.max(1, Math.round(rig.roomMeters.x / stepInfo.stepM));
  const sqY = Math.max(1, Math.round(rig.roomMeters.y / stepInfo.stepM));
  const dec = unitDecimals(rig.unit);
  const stepText = Number(stepInfo.stepUnit.toFixed(dec));
  const srcPos = getSourcePosM(rig);
  const micPos = getMicWorldPosM(rig, rig.selectedMic);
  const distM = Math.hypot(srcPos.x - micPos.x, srcPos.y - micPos.y, srcPos.z - micPos.z);

  return (
    <div ref={containerRef} className="flex flex-col w-full h-full min-h-0 bg-[#1e1e1e]">
      {/* Header toolbar */}
      <div className="h-7 px-1.5 bg-[#2a2a2a] border-b border-[#383838] flex items-center justify-between gap-1 text-[11px] shrink-0 overflow-x-auto">
        <div className="flex items-center gap-0.5 shrink-0">
          <button
            type="button"
            onClick={() => setMode('plans')}
            className={`px-1.5 py-0.5 border text-[10px] ${
              mode === 'plans'
                ? 'bg-[#378ef0] text-white border-[#378ef0]'
                : 'bg-[#323232] text-[#bdbdbd] border-[#424242] hover:bg-[#3a3a3a]'
            }`}
          >
            Plans
          </button>
          <button
            type="button"
            onClick={() => setMode('3d')}
            className={`px-1.5 py-0.5 border text-[10px] ${
              mode === '3d'
                ? 'bg-[#378ef0] text-white border-[#378ef0]'
                : 'bg-[#323232] text-[#bdbdbd] border-[#424242] hover:bg-[#3a3a3a]'
            }`}
          >
            3D Grid
          </button>
          <button
            type="button"
            onClick={() => setMode('split')}
            title="Show Top/Front Plans and 3D Grid simultaneously synchronized"
            className={`px-1.5 py-0.5 border text-[10px] ${
              mode === 'split'
                ? 'bg-[#378ef0] text-white border-[#378ef0]'
                : 'bg-[#323232] text-[#bdbdbd] border-[#424242] hover:bg-[#3a3a3a]'
            }`}
          >
            Split
          </button>

          {(mode === '3d' || mode === 'split') && (
            <>
              <button
                type="button"
                onClick={() => setProjection((p) => (p === 'perspective' ? 'orthographic' : 'perspective'))}
                className="px-1.5 py-0.5 border border-[#424242] bg-[#323232] text-[#c8c8c8] hover:bg-[#3a3a3a] text-[10px]"
                title="Switch 3D projection (camera only — never changes audio)"
              >
                {projection === 'perspective' ? 'Persp' : 'Ortho'}
              </button>
              <button
                type="button"
                onClick={() => {
                  setCamera(DEFAULT_CAMERA);
                  if (mode === 'split') {
                    setPlanZoom(1.0);
                    setPlanPanX(0);
                  }
                }}
                className="px-1.5 py-0.5 border border-[#424242] bg-[#323232] text-[#c8c8c8] hover:bg-[#3a3a3a] text-[10px]"
                title="Fit View resets viewing camera only"
              >
                Fit View
              </button>
            </>
          )}
          {mode === 'plans' && (
            <button
              type="button"
              onClick={() => {
                setPlanZoom(1.0);
                setPlanPanX(0);
              }}
              className="px-1.5 py-0.5 border border-[#424242] bg-[#323232] text-[#c8c8c8] hover:bg-[#3a3a3a] text-[10px]"
              title="Reset plan pan & zoom"
            >
              Fit
            </button>
          )}
        </div>

        <div className="flex items-center gap-1.5 text-[10px] shrink-0">
          <label
            className="flex items-center gap-1 text-[#bcbcbc] cursor-pointer"
            title="Visual polar pattern overlay (never changes audio)"
          >
            <input
              type="checkbox"
              checked={rig.showPatterns}
              onChange={(e) => onTogglePatterns(e.target.checked)}
              className="accent-[#378ef0] w-3 h-3"
            />
            <span>Patterns</span>
          </label>
          {onToggleGuidanceOverlay && (
            <label
              className="flex items-center gap-1 text-[#bcbcbc] cursor-pointer"
              title="Display-only 3:1 distance guidance circle in Top & 3D views (never changes audio)"
            >
              <input
                type="checkbox"
                checked={rig.showGuidanceOverlay}
                onChange={(e) => onToggleGuidanceOverlay(e.target.checked)}
                className="accent-[#378ef0] w-3 h-3"
              />
              <span>3:1</span>
            </label>
          )}
          <label
            className="flex items-center gap-1 text-[#bcbcbc] cursor-pointer"
            title="Snap source & mic drag to grid (hold Shift for free drag)"
          >
            <input
              type="checkbox"
              checked={rig.gridSnap}
              onChange={(e) => onToggleGridSnap(e.target.checked)}
              className="accent-[#378ef0] w-3 h-3"
            />
            <span>Snap</span>
          </label>
        </div>
      </div>

      {/* Main Spatial Canvas(es) — Plans, 3D Grid, or Synchronized Split */}
      <div className="flex-1 min-h-0 flex flex-col divide-y divide-[#383838] relative">
        {(mode === 'plans' || mode === 'split') && (
          <div className="flex-1 min-h-0 relative">
            <canvas
              ref={canvasRef}
              onPointerDown={handlePointerDown}
              onPointerMove={handlePointerMove}
              onPointerUp={handlePointerUp}
              onWheel={handleWheel}
              className="w-full h-full block cursor-default"
            />
          </div>
        )}
        {(mode === '3d' || mode === 'split') && (
          <div className="flex-1 min-h-0 relative">
            <canvas
              ref={canvas3dRef}
              onPointerDown={handlePointerDown3d}
              onPointerMove={handlePointerMove}
              onPointerUp={handlePointerUp}
              onWheel={handleWheel3d}
              className="w-full h-full block cursor-grab active:cursor-grabbing"
            />
          </div>
        )}
      </div>

      {/* Truthful Grid Scale & Live Geometry Footer */}
      <div className="h-6 px-2 bg-[#242424] border-t border-[#383838] flex items-center justify-between text-[10px] font-mono tabular-nums text-[#9e9e9e] shrink-0">
        <span className="truncate">
          {sqX} × {sqY} squares · {stepText} {rig.unit} per square
        </span>
        <span className="shrink-0">
          d={metersToDisplay(distM, rig.unit).toFixed(dec)} {rig.unit} · src Z=
          {metersToDisplay(srcPos.z, rig.unit).toFixed(dec)} {rig.unit}
        </span>
      </div>
    </div>
  );
}

function computePlanLayout(
  w: number,
  h: number,
  rig: StudioRigState,
  planZoom: number,
  planPanX: number,
): PlanLayout {
  const padLeft = 36;
  const padRight = 12;
  const padTop = 18;
  const gap = 22;
  const padBot = 16;

  const usableW = Math.max(40, w - padLeft - padRight);
  const usableH = Math.max(60, h - padTop - gap - padBot);

  const rx = Math.max(1.5, rig.roomMeters.x);
  const ry = Math.max(1.5, rig.roomMeters.y);
  const rz = Math.max(1.5, rig.roomMeters.z);

  // Split vertical space proportionally to truthful depth (Y) vs height (Z), with sane minimums
  const fracTop = Math.max(0.36, Math.min(0.68, ry / (ry + rz)));
  const topH = Math.floor(usableH * fracTop);
  const frontH = usableH - topH;

  // Shared physical pixels-per-metre across BOTH Top and Front plans (same square size!)
  const fitPxW = usableW / rx;
  const fitPxTop = (topH - 12) / ry;
  const fitPxFront = (frontH - 12) / rz;
  const basePxPerM = Math.max(1.5, Math.min(fitPxW, fitPxTop, fitPxFront));
  const pxPerM = basePxPerM * planZoom;

  const roomPxW = rx * pxPerM;
  const originX = padLeft + (usableW - roomPxW) * 0.5 + planPanX;
  const topRect = { x: padLeft, y: padTop, w: usableW, h: topH };
  const frontRect = { x: padLeft, y: padTop + topH + gap, w: usableW, h: frontH };

  const roomPxY = ry * pxPerM;
  const roomPxZ = rz * pxPerM;
  const topOriginY = topRect.y + (topRect.h + roomPxY) * 0.5;
  const frontOriginY = frontRect.y + (frontRect.h + roomPxZ) * 0.5;

  const { stepM, stepUnit } = niceGridStepM(rig.unit, Math.max(rx, ry) / planZoom);

  return {
    padLeft,
    padRight,
    topRect,
    frontRect,
    pxPerM,
    originX,
    topOriginY,
    frontOriginY,
    stepM,
    stepUnit,
  };
}

function drawPlansView(
  ctx: CanvasRenderingContext2D,
  _w: number,
  _h: number,
  layout: PlanLayout,
  rig: StudioRigState,
  sourceSymbol: string,
  getPattern: (idx: number) => PolarPattern,
) {
  drawSinglePlan(ctx, layout, 'top', rig, sourceSymbol, getPattern);
  drawSinglePlan(ctx, layout, 'front', rig, sourceSymbol, getPattern);
}

function drawSinglePlan(
  ctx: CanvasRenderingContext2D,
  layout: PlanLayout,
  view: 'top' | 'front',
  rig: StudioRigState,
  sourceSymbol: string,
  getPattern: (idx: number) => PolarPattern,
) {
  const box = view === 'top' ? layout.topRect : layout.frontRect;
  const isOpen = rig.preset === 'open';
  const dec = unitDecimals(rig.unit);

  // Panel background
  ctx.fillStyle = '#202020';
  ctx.fillRect(box.x, box.y, box.w, box.h);
  ctx.strokeStyle = '#363636';
  ctx.lineWidth = 1;
  ctx.strokeRect(box.x + 0.5, box.y + 0.5, box.w - 1, box.h - 1);

  // Title above panel
  ctx.fillStyle = '#a8a8a8';
  ctx.font = '10px "IBM Plex Mono", monospace';
  const dimLabel =
    view === 'top'
      ? `TOP · X×Y (${metersToDisplay(rig.roomMeters.x, rig.unit).toFixed(dec)} × ${metersToDisplay(rig.roomMeters.y, rig.unit).toFixed(dec)} ${rig.unit})`
      : `FRONT · X×Z (${metersToDisplay(rig.roomMeters.x, rig.unit).toFixed(dec)} × ${metersToDisplay(rig.roomMeters.z, rig.unit).toFixed(dec)} ${rig.unit})`;
  ctx.fillText(dimLabel, box.x + 2, box.y - 4);

  const rx = rig.roomMeters.x;
  const rVert = view === 'top' ? rig.roomMeters.y : rig.roomMeters.z;
  const roomX0 = layout.originX;
  const roomW = rx * layout.pxPerM;
  const roomH = rVert * layout.pxPerM;
  const roomYBottom = view === 'top' ? layout.topOriginY : layout.frontOriginY;
  const roomYTop = roomYBottom - roomH;

  // Clip interior grid & objects to panel
  ctx.save();
  ctx.beginPath();
  ctx.rect(box.x + 1, box.y + 1, box.w - 2, box.h - 2);
  ctx.clip();

  // Room interior fill
  ctx.fillStyle = isOpen ? '#1c2128' : '#242424';
  ctx.fillRect(roomX0, roomYTop, roomW, roomH);

  // True physical square grid (exact same physical stepM on X and Y/Z!)
  ctx.strokeStyle = '#2e2e2e';
  ctx.lineWidth = 1;
  for (let xm = 0; xm <= rx + 1e-4; xm += layout.stepM) {
    const gx = roomX0 + xm * layout.pxPerM;
    ctx.beginPath();
    ctx.moveTo(gx, roomYTop);
    ctx.lineTo(gx, roomYBottom);
    ctx.stroke();
  }
  for (let ym = 0; ym <= rVert + 1e-4; ym += layout.stepM) {
    const gy = roomYBottom - ym * layout.pxPerM;
    ctx.beginPath();
    ctx.moveTo(roomX0, gy);
    ctx.lineTo(roomX0 + roomW, gy);
    ctx.stroke();
  }

  // Room boundaries (Open preset: dashed guide + solid ground line on Front!)
  ctx.strokeStyle = isOpen ? '#4e627a' : '#727272';
  ctx.lineWidth = 1.25;
  ctx.setLineDash(isOpen ? [4, 3] : []);
  ctx.strokeRect(roomX0 + 0.5, roomYTop + 0.5, roomW - 1, roomH - 1);
  ctx.setLineDash([]);
  if (isOpen && view === 'front') {
    // Ground plane is solid in Open mode
    ctx.strokeStyle = '#6ba368';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(roomX0, roomYBottom);
    ctx.lineTo(roomX0 + roomW, roomYBottom);
    ctx.stroke();
  }

  const toScreen = (p: Vec3) => ({
    x: roomX0 + p.x * layout.pxPerM,
    y: view === 'top' ? roomYBottom - p.y * layout.pxPerM : roomYBottom - p.z * layout.pxPerM,
  });

  // Source icon (always visible across all tabs!)
  const srcPos = getSourcePosM(rig);
  const sp = toScreen(srcPos);
  if (view === 'front') {
    // 5 ft reference line
    const y5ft = roomYBottom - 1.524 * layout.pxPerM;
    if (y5ft >= roomYTop && y5ft <= roomYBottom) {
      ctx.strokeStyle = 'rgba(232, 168, 56, 0.25)';
      ctx.setLineDash([2, 3]);
      ctx.beginPath();
      ctx.moveTo(roomX0, y5ft);
      ctx.lineTo(roomX0 + roomW, y5ft);
      ctx.stroke();
      ctx.setLineDash([]);
    }
  }
  drawSourceIcon(ctx, sp.x, sp.y, sourceSymbol, rig.sourceFacingDeg, view);

  // Optional display-only 3:1 guidance radius circle in Top view when 2+ active mics exist
  if (view === 'top' && rig.showGuidanceOverlay) {
    const activeDists = rig.mics
      .filter((m) => m.enabled)
      .map((m) => Math.max(0.2, Math.hypot(m.offsetM.x, m.offsetM.y, m.offsetM.z)));
    if (activeDists.length >= 2) {
      const r3to1 = Math.min(...activeDists) * 3.0;
      const rPx = r3to1 * layout.pxPerM;
      ctx.save();
      ctx.strokeStyle = 'rgba(240, 180, 84, 0.55)';
      ctx.lineWidth = 1;
      ctx.setLineDash([5, 4]);
      ctx.beginPath();
      ctx.arc(sp.x, sp.y, rPx, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.fillStyle = 'rgba(240, 180, 84, 0.85)';
      ctx.font = '8.5px "IBM Plex Mono", monospace';
      ctx.fillText(
        `3:1 (${metersToDisplay(r3to1, rig.unit).toFixed(dec)} ${rig.unit})`,
        sp.x + 4,
        sp.y - rPx + 10,
      );
      ctx.restore();
    }
  }

  // Microphones
  rig.mics.forEach((slot, idx) => {
    const mp = toScreen(getMicWorldPosM(rig, idx));
    const pat = getPattern(idx);
    const aim = aimDirectionFromAngles(slot.aim);
    const aimAngle = Math.atan2(view === 'top' ? -aim.y : -aim.z, aim.x);
    const isSel = idx === rig.selectedMic;

    // Optional large visual-only polar pattern overlay (NEVER hides small symbol or changes audio)
    if (rig.showPatterns) {
      ctx.save();
      ctx.translate(mp.x, mp.y);
      ctx.rotate(aimAngle);
      ctx.beginPath();
      const rad = 26;
      for (let i = 0; i <= 48; i++) {
        const th = (i / 48) * Math.PI * 2;
        const g = Math.abs(polarGain(pat, Math.cos(th))) * rad;
        const px = Math.cos(th) * g;
        const py = Math.sin(th) * g;
        if (i === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      }
      ctx.closePath();
      ctx.fillStyle = isSel ? 'rgba(55, 142, 240, 0.16)' : 'rgba(130, 175, 220, 0.08)';
      ctx.fill();
      ctx.strokeStyle = isSel ? 'rgba(55, 142, 240, 0.72)' : 'rgba(140, 170, 200, 0.45)';
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.restore();
    }

    // Aim line
    ctx.strokeStyle = isSel ? '#378ef0' : '#7ba6d6';
    ctx.lineWidth = isSel ? 1.5 : 1;
    ctx.beginPath();
    ctx.moveTo(mp.x, mp.y);
    ctx.lineTo(mp.x + Math.cos(aimAngle) * 18, mp.y + Math.sin(aimAngle) * 18);
    ctx.stroke();

    // Small pickup-pattern symbol directly on the mic marker (always visible!)
    drawMicPatternSymbol(ctx, mp.x, mp.y, aimAngle, pat, isSel, slot.enabled);

    ctx.fillStyle = isSel ? '#d8ebff' : '#9eb8d6';
    ctx.font = '9px "IBM Plex Mono", monospace';
    ctx.fillText(`M${idx + 1}`, mp.x + 9, mp.y - 7);
  });

  ctx.restore();

  // Left vertical ruler labels
  ctx.fillStyle = '#7e7e7e';
  ctx.font = '9px "IBM Plex Mono", monospace';
  ctx.textAlign = 'right';
  const topLabel = `${metersToDisplay(rVert, rig.unit).toFixed(dec === 0 ? 0 : 1)}`;
  ctx.fillText(topLabel, box.x - 4, Math.max(box.y + 9, roomYTop + 4));
  ctx.fillText('0', box.x - 4, Math.min(box.y + box.h - 2, roomYBottom));
  ctx.textAlign = 'left';
}

function drawSourceIcon(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  symbol: string,
  facingDeg: number,
  view: 'top' | 'front',
) {
  // Visual facing arrow (separate from acoustic radiation!)
  if (view === 'top') {
    const rad = ((facingDeg - 90) * Math.PI) / 180;
    ctx.strokeStyle = 'rgba(232, 168, 56, 0.65)';
    ctx.lineWidth = 1.25;
    ctx.beginPath();
    ctx.moveTo(x, y);
    ctx.lineTo(x + Math.cos(rad) * 14, y + Math.sin(rad) * 14);
    ctx.stroke();
  }

  ctx.save();
  ctx.translate(x, y);
  ctx.fillStyle = '#e8a838';
  ctx.strokeStyle = '#1b1b1b';
  ctx.lineWidth = 1;

  if (symbol === 'voice' || symbol === 'ensemble') {
    // Recognizable person head + shoulders glyph
    ctx.beginPath();
    ctx.arc(0, -3.5, 3, 0, Math.PI * 2);
    ctx.fill();
    ctx.beginPath();
    ctx.arc(0, 4.5, 5, Math.PI, 0);
    ctx.fill();
  } else if (symbol === 'guitar' || symbol === 'bowed') {
    // Recognizable instrument body + neck
    ctx.beginPath();
    ctx.arc(0, 2, 4.5, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
    ctx.strokeStyle = '#e8a838';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(0, -2);
    ctx.lineTo(0, -8);
    ctx.stroke();
  } else if (symbol === 'amp' || symbol === 'keyboard') {
    ctx.fillRect(-5.5, -4.5, 11, 9);
    ctx.strokeRect(-5.5, -4.5, 11, 9);
    ctx.strokeStyle = '#1b1b1b';
    ctx.beginPath();
    ctx.arc(0, 0, 2.5, 0, Math.PI * 2);
    ctx.stroke();
  } else {
    ctx.beginPath();
    ctx.arc(0, 0, 5.5, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  }
  ctx.restore();
}

function drawMicPatternSymbol(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  angle: number,
  pattern: PolarPattern,
  selected: boolean,
  enabled: boolean,
) {
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(angle);
  const stroke = !enabled ? '#6a6a6a' : selected ? '#378ef0' : '#7db4ee';
  const fill = !enabled ? '#262626' : selected ? '#1d3654' : '#1e2a38';
  ctx.strokeStyle = stroke;
  ctx.fillStyle = fill;
  ctx.lineWidth = selected ? 1.6 : 1.2;

  if (pattern === 'omni') {
    // Circle = omni
    ctx.beginPath();
    ctx.arc(0, 0, 6, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  } else if (pattern === 'figure8') {
    // Two lobes = figure-8
    ctx.beginPath();
    ctx.arc(3.5, 0, 3.6, 0, Math.PI * 2);
    ctx.arc(-3.5, 0, 3.6, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  } else {
    // Heart / directional cardioid glyph
    ctx.beginPath();
    for (let i = 0; i <= 24; i++) {
      const th = (i / 24) * Math.PI * 2;
      const r = Math.max(1.2, Math.abs(polarGain(pattern, Math.cos(th))) * 7.5);
      const px = Math.cos(th) * r - 1.5;
      const py = Math.sin(th) * r;
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    ctx.closePath();
    ctx.fill();
    ctx.stroke();
  }
  ctx.restore();
}

function draw3dGridView(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  rig: StudioRigState,
  sourceSymbol: string,
  getPattern: (idx: number) => PolarPattern,
  camera: { yaw: number; pitch: number; zoom: number; panX: number; panY: number },
  projection: Projection3D,
) {
  const rx = rig.roomMeters.x;
  const ry = rig.roomMeters.y;
  const rz = rig.roomMeters.z;
  const maxDim = Math.max(rx, ry, rz, 1);
  const scale = (Math.min(w, h) * 0.52 * camera.zoom) / maxDim;
  const cx = w * 0.5 + camera.panX;
  const cy = h * 0.56 + camera.panY;
  const cyaw = Math.cos(camera.yaw);
  const syaw = Math.sin(camera.yaw);
  const cpitch = Math.cos(camera.pitch);
  const spitch = Math.sin(camera.pitch);
  const isOpen = rig.preset === 'open';

  const proj = (x: number, y: number, z: number) => {
    const lx = x - rx * 0.5;
    const ly = y - ry * 0.5;
    const lz = z - rz * 0.35;
    const x1 = lx * cyaw - ly * syaw;
    const y1 = lx * syaw + ly * cyaw;
    const y2 = y1 * cpitch - lz * spitch;
    const z2 = y1 * spitch + lz * cpitch;
    const persp =
      projection === 'perspective' ? 1 / (1 + y2 / (maxDim * 2.8)) : 1.0;
    return {
      x: cx + x1 * scale * persp,
      y: cy - z2 * scale * persp,
    };
  };

  // Floor grid
  const { stepM } = niceGridStepM(rig.unit, Math.max(rx, ry));
  ctx.strokeStyle = isOpen ? '#2b3932' : '#2c2c2c';
  ctx.lineWidth = 1;
  for (let xm = 0; xm <= rx + 1e-4; xm += stepM) {
    const a = proj(xm, 0, 0);
    const b = proj(xm, ry, 0);
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
  }
  for (let ym = 0; ym <= ry + 1e-4; ym += stepM) {
    const a = proj(0, ym, 0);
    const b = proj(rx, ym, 0);
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
  }

  // Room wireframe (ground solid; walls/ceiling dashed in Open mode)
  const pts = [
    proj(0, 0, 0),
    proj(rx, 0, 0),
    proj(rx, ry, 0),
    proj(0, ry, 0),
    proj(0, 0, rz),
    proj(rx, 0, rz),
    proj(rx, ry, rz),
    proj(0, ry, rz),
  ];
  const floorEdges: [number, number][] = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 0],
  ];
  const upperEdges: [number, number][] = [
    [4, 5],
    [5, 6],
    [6, 7],
    [7, 4],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
  ];

  ctx.strokeStyle = isOpen ? '#5f9a61' : '#666666';
  ctx.lineWidth = 1.3;
  floorEdges.forEach(([a, b]) => {
    ctx.beginPath();
    ctx.moveTo(pts[a].x, pts[a].y);
    ctx.lineTo(pts[b].x, pts[b].y);
    ctx.stroke();
  });

  ctx.strokeStyle = isOpen ? 'rgba(92, 114, 138, 0.45)' : '#525252';
  ctx.lineWidth = 1;
  ctx.setLineDash(isOpen ? [4, 4] : []);
  upperEdges.forEach(([a, b]) => {
    ctx.beginPath();
    ctx.moveTo(pts[a].x, pts[a].y);
    ctx.lineTo(pts[b].x, pts[b].y);
    ctx.stroke();
  });
  ctx.setLineDash([]);

  // Source + floor drop-line
  const srcPos = getSourcePosM(rig);
  const srcFloor = proj(srcPos.x, srcPos.y, 0);
  const srcPt = proj(srcPos.x, srcPos.y, srcPos.z);
  ctx.strokeStyle = 'rgba(232, 168, 56, 0.45)';
  ctx.setLineDash([3, 3]);
  ctx.beginPath();
  ctx.moveTo(srcFloor.x, srcFloor.y);
  ctx.lineTo(srcPt.x, srcPt.y);
  ctx.stroke();
  ctx.setLineDash([]);
  drawSourceIcon(ctx, srcPt.x, srcPt.y, sourceSymbol, rig.sourceFacingDeg, 'front');

  // Optional display-only 3:1 guidance radius ring projected on 3D floor when 2+ active mics exist
  if (rig.showGuidanceOverlay) {
    const activeDists = rig.mics
      .filter((m) => m.enabled)
      .map((m) => Math.max(0.2, Math.hypot(m.offsetM.x, m.offsetM.y, m.offsetM.z)));
    if (activeDists.length >= 2) {
      const r3to1 = Math.min(...activeDists) * 3.0;
      ctx.save();
      ctx.strokeStyle = 'rgba(240, 180, 84, 0.52)';
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      ctx.beginPath();
      const segs = 48;
      for (let i = 0; i <= segs; i++) {
        const th = (i / segs) * Math.PI * 2;
        const pt = proj(srcPos.x + Math.cos(th) * r3to1, srcPos.y + Math.sin(th) * r3to1, 0);
        if (i === 0) ctx.moveTo(pt.x, pt.y);
        else ctx.lineTo(pt.x, pt.y);
      }
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.restore();
    }
  }

  // Microphones + aim rays + floor drop-lines
  rig.mics.forEach((slot, idx) => {
    const mp = getMicWorldPosM(rig, idx);
    const mFloor = proj(mp.x, mp.y, 0);
    const mPt = proj(mp.x, mp.y, mp.z);
    const aim = aimDirectionFromAngles(slot.aim);
    const rayLen = Math.min(maxDim * 0.25, 1.0);
    const aPt = proj(mp.x + aim.x * rayLen, mp.y + aim.y * rayLen, mp.z + aim.z * rayLen);
    const isSel = idx === rig.selectedMic;

    ctx.strokeStyle = 'rgba(55, 142, 240, 0.35)';
    ctx.setLineDash([3, 3]);
    ctx.beginPath();
    ctx.moveTo(mFloor.x, mFloor.y);
    ctx.lineTo(mPt.x, mPt.y);
    ctx.stroke();
    ctx.setLineDash([]);

    ctx.strokeStyle = isSel ? '#378ef0' : '#7ba6d6';
    ctx.lineWidth = isSel ? 1.6 : 1;
    ctx.beginPath();
    ctx.moveTo(mPt.x, mPt.y);
    ctx.lineTo(aPt.x, aPt.y);
    ctx.stroke();

    const angle2d = Math.atan2(aPt.y - mPt.y, aPt.x - mPt.x);
    drawMicPatternSymbol(ctx, mPt.x, mPt.y, angle2d, getPattern(idx), isSel, slot.enabled);
  });

  // Orientation Axis Triad (bottom-left corner: shows +X, +Y, +Z Up)
  const ax = 36;
  const ay = h - 32;
  const triadLen = 20;
  const drawAxis = (dx: number, dy: number, dz: number, color: string, label: string) => {
    const x1 = dx * cyaw - dy * syaw;
    const y1 = dx * syaw + dy * cyaw;
    const z2 = y1 * spitch + dz * cpitch;
    const ex = ax + x1 * triadLen;
    const ey = ay - z2 * triadLen;
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(ax, ay);
    ctx.lineTo(ex, ey);
    ctx.stroke();
    ctx.fillStyle = color;
    ctx.font = '9px "IBM Plex Mono", monospace';
    ctx.fillText(label, ex + 2, ey + 3);
  };
  drawAxis(1, 0, 0, '#e06c75', '+X');
  drawAxis(0, 1, 0, '#98c379', '+Y');
  drawAxis(0, 0, 1, '#61afef', '+Z Up');

  ctx.fillStyle = '#8b8b8b';
  ctx.font = '10px "IBM Plex Mono", monospace';
  ctx.fillText(
    `3D (${projection === 'perspective' ? 'Persp' : 'Ortho'}) · Drag orbit · Shift+Drag pan · Wheel zoom`,
    8,
    14,
  );
}
