export type DryRegionKind = 'marquee' | 'range';

export type RegionFixAction =
  | 'straighten'
  | 'smooth'
  | 'reduce_noise'
  | 'stabilize'
  | 'note_only';

export type RegionHandle =
  | 'move'
  | 'left'
  | 'right'
  | 'top'
  | 'bottom'
  | 'top-left'
  | 'top-right'
  | 'bottom-left'
  | 'bottom-right';

export interface DryRegion {
  id: string;
  label: string;
  kind: DryRegionKind;
  startS: number;
  endS: number;
  /** Frequency bounds (Hz) for rectangular marquee; null for time-only range. */
  minHz: number | null;
  maxHz: number | null;
  note: string;
  action: RegionFixAction;
  /** 0.0..=1.0 mild user-controlled fix strength. */
  strength: number;
  /** Raised-cosine edge crossfade in ms (4..=60). */
  crossfadeMs: number;
  appliedCount: number;
  lastAppliedSha: string | null;
}

export interface DryRegionHistoryState {
  regions: DryRegion[];
  selectedId: string | null;
  past: DryRegion[][];
  future: DryRegion[][];
  /** Transient gesture-start snapshot captured when a drag/resize begins (M5). */
  gestureBaseline?: DryRegion[] | null;
}

export const REGION_FIX_ACTIONS: readonly {
  id: RegionFixAction;
  label: string;
  shortDesc: string;
}[] = [
  { id: 'straighten', label: 'Straighten', shortDesc: 'Gently steady pitch wobble while keeping bends' },
  { id: 'smooth', label: 'Smooth', shortDesc: 'Even local level bumps and soften upper harshness' },
  { id: 'reduce_noise', label: 'Reduce Noise', shortDesc: 'Tame broadband noise/hiss inside the region' },
  { id: 'stabilize', label: 'Stabilize', shortDesc: 'Mildly steady both pitch and loudness together' },
  { id: 'note_only', label: 'Note Only', shortDesc: 'Attach a short note and infer a light local fix' },
] as const;

export const MIN_REGION_DURATION_S = 0.04;
export const MIN_REGION_HZ_SPAN = 40;
export const MAX_REGION_HZ = 12000;
export const MIN_REGION_HZ = 20;

let regionIdSeq = 0;

/** Generate a globally unique, monotonic region ID that never collides after deletes (Audit M5). */
export function nextUniqueRegionId(): string {
  regionIdSeq += 1;
  const suffix = Math.random().toString(36).slice(2, 6);
  return `reg-${regionIdSeq}-${suffix}`;
}

export function createInitialDryRegionHistory(): DryRegionHistoryState {
  return {
    regions: [],
    selectedId: null,
    past: [],
    future: [],
    gestureBaseline: null,
  };
}

function cloneRegions(list: DryRegion[]): DryRegion[] {
  return list.map((r) => ({ ...r }));
}

function regionsEqual(a: DryRegion[], b: DryRegion[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const x = a[i];
    const y = b[i];
    if (
      x.id !== y.id ||
      x.startS !== y.startS ||
      x.endS !== y.endS ||
      x.minHz !== y.minHz ||
      x.maxHz !== y.maxHz ||
      x.action !== y.action ||
      x.strength !== y.strength ||
      x.crossfadeMs !== y.crossfadeMs ||
      x.note !== y.note
    ) {
      return false;
    }
  }
  return true;
}

export function clampRegionBounds(
  region: DryRegion,
  durationS: number,
  maxHzLimit = MAX_REGION_HZ,
): DryRegion {
  const safeDur = Math.max(MIN_REGION_DURATION_S * 2, Number.isFinite(durationS) ? durationS : 1);
  let startS = Math.min(region.startS, region.endS);
  let endS = Math.max(region.startS, region.endS);
  startS = Math.max(0, Math.min(safeDur - MIN_REGION_DURATION_S, startS));
  endS = Math.max(startS + MIN_REGION_DURATION_S, Math.min(safeDur, endS));

  let minHz = region.minHz;
  let maxHz = region.maxHz;
  if (region.kind === 'marquee') {
    const rawLo = minHz ?? 120;
    const rawHi = maxHz ?? 2400;
    let lo = Math.min(rawLo, rawHi);
    let hi = Math.max(rawLo, rawHi);
    lo = Math.max(MIN_REGION_HZ, Math.min(maxHzLimit - MIN_REGION_HZ_SPAN, lo));
    hi = Math.max(lo + MIN_REGION_HZ_SPAN, Math.min(maxHzLimit, hi));
    minHz = Math.round(lo);
    maxHz = Math.round(hi);
  } else {
    minHz = null;
    maxHz = null;
  }

  return {
    ...region,
    startS: Number(startS.toFixed(4)),
    endS: Number(endS.toFixed(4)),
    minHz,
    maxHz,
    strength: Math.max(0, Math.min(1, Number.isFinite(region.strength) ? region.strength : 0.55)),
    crossfadeMs: Math.max(4, Math.min(60, Number.isFinite(region.crossfadeMs) ? region.crossfadeMs : 12)),
  };
}

export function addDryRegion(
  state: DryRegionHistoryState,
  input: {
    kind: DryRegionKind;
    startS: number;
    endS: number;
    minHz?: number | null;
    maxHz?: number | null;
    note?: string;
    action?: RegionFixAction;
    strength?: number;
  },
  durationS: number,
): DryRegionHistoryState {
  const existingNums = state.regions.map((r) => {
    const m = /\b(\d+)$/.exec(r.label);
    return m ? Number(m[1]) : 0;
  });
  const nextNum = Math.max(state.regions.length, ...existingNums) + 1;
  const id = nextUniqueRegionId();
  const region = clampRegionBounds(
    {
      id,
      label: input.kind === 'marquee' ? `Marquee ${nextNum}` : `Range ${nextNum}`,
      kind: input.kind,
      startS: input.startS,
      endS: input.endS,
      minHz: input.kind === 'marquee' ? (input.minHz ?? 160) : null,
      maxHz: input.kind === 'marquee' ? (input.maxHz ?? 2600) : null,
      note: input.note ?? '',
      action: input.action ?? 'stabilize',
      strength: input.strength ?? 0.55,
      crossfadeMs: 12,
      appliedCount: 0,
      lastAppliedSha: null,
    },
    durationS,
  );
  return {
    regions: [...state.regions, region],
    selectedId: region.id,
    past: [...state.past.slice(-39), cloneRegions(state.regions)],
    future: [],
    gestureBaseline: null,
  };
}

/** Capture gesture-start snapshot before interactive drag/resize preview updates (Audit M5). */
export function beginRegionGesture(state: DryRegionHistoryState): DryRegionHistoryState {
  return {
    ...state,
    gestureBaseline: cloneRegions(state.regions),
  };
}

/** Update region geometry transiently during pointer drag without polluting undo history (Audit M5). */
export function previewRegionGesture(
  state: DryRegionHistoryState,
  id: string,
  patch: Partial<DryRegion>,
  durationS: number,
): DryRegionHistoryState {
  const idx = state.regions.findIndex((r) => r.id === id);
  if (idx < 0) return state;
  const baseline = state.gestureBaseline ?? cloneRegions(state.regions);
  const updated = state.regions.map((r) =>
    r.id === id ? clampRegionBounds({ ...r, ...patch }, durationS) : r,
  );
  return {
    ...state,
    regions: updated,
    gestureBaseline: baseline,
  };
}

/**
 * Commit an interactive drag/resize gesture using the captured gesture-start baseline,
 * producing exactly ONE undo entry pointing back to the starting geometry (Audit M5).
 */
export function commitRegionGesture(
  state: DryRegionHistoryState,
  id: string,
  patch: Partial<DryRegion>,
  durationS: number,
): DryRegionHistoryState {
  const idx = state.regions.findIndex((r) => r.id === id);
  if (idx < 0) return { ...state, gestureBaseline: null };
  const baseline = state.gestureBaseline ?? cloneRegions(state.regions);
  const updated = state.regions.map((r) =>
    r.id === id ? clampRegionBounds({ ...r, ...patch }, durationS) : r,
  );
  if (regionsEqual(baseline, updated)) {
    return {
      ...state,
      regions: updated,
      gestureBaseline: null,
    };
  }
  return {
    regions: updated,
    selectedId: state.selectedId,
    past: [...state.past.slice(-39), cloneRegions(baseline)],
    future: [],
    gestureBaseline: null,
  };
}

/** Cancel an in-progress drag gesture, restoring the exact gesture-start regions (Audit M5). */
export function cancelRegionGesture(state: DryRegionHistoryState): DryRegionHistoryState {
  if (!state.gestureBaseline) return state;
  return {
    ...state,
    regions: cloneRegions(state.gestureBaseline),
    gestureBaseline: null,
  };
}

export function updateDryRegion(
  state: DryRegionHistoryState,
  id: string,
  patch: Partial<DryRegion>,
  durationS: number,
  recordHistory = true,
): DryRegionHistoryState {
  if (!recordHistory) {
    return previewRegionGesture(state, id, patch, durationS);
  }
  if (state.gestureBaseline) {
    return commitRegionGesture(state, id, patch, durationS);
  }
  const idx = state.regions.findIndex((r) => r.id === id);
  if (idx < 0) return state;
  const updated = state.regions.map((r) =>
    r.id === id ? clampRegionBounds({ ...r, ...patch }, durationS) : r,
  );
  return {
    regions: updated,
    selectedId: state.selectedId,
    past: [...state.past.slice(-39), cloneRegions(state.regions)],
    future: [],
    gestureBaseline: null,
  };
}

export function deleteDryRegion(
  state: DryRegionHistoryState,
  id: string,
): DryRegionHistoryState {
  const remaining = state.regions.filter((r) => r.id !== id);
  if (remaining.length === state.regions.length) return state;
  const nextSelected =
    state.selectedId === id ? (remaining[remaining.length - 1]?.id ?? null) : state.selectedId;
  return {
    regions: remaining,
    selectedId: nextSelected,
    past: [...state.past.slice(-39), cloneRegions(state.regions)],
    future: [],
    gestureBaseline: null,
  };
}

export function selectDryRegion(
  state: DryRegionHistoryState,
  id: string | null,
): DryRegionHistoryState {
  if (state.selectedId === id) return state;
  return { ...state, selectedId: id };
}

export function undoDryRegionHistory(state: DryRegionHistoryState): DryRegionHistoryState {
  if (state.past.length === 0) return state;
  const previous = state.past[state.past.length - 1];
  const selectedStillExists = previous.some((r) => r.id === state.selectedId);
  return {
    regions: cloneRegions(previous),
    selectedId: selectedStillExists ? state.selectedId : (previous[0]?.id ?? null),
    past: state.past.slice(0, -1),
    future: [cloneRegions(state.regions), ...state.future.slice(0, 39)],
  };
}

export function redoDryRegionHistory(state: DryRegionHistoryState): DryRegionHistoryState {
  if (state.future.length === 0) return state;
  const next = state.future[0];
  const selectedStillExists = next.some((r) => r.id === state.selectedId);
  return {
    regions: cloneRegions(next),
    selectedId: selectedStillExists ? state.selectedId : (next[0]?.id ?? null),
    past: [...state.past.slice(-39), cloneRegions(state.regions)],
    future: state.future.slice(1),
  };
}

/**
 * Pure move/resize transform for dragging a region interior or edge/corner handle.
 */
export function transformRegionByHandle(
  origin: DryRegion,
  handle: RegionHandle,
  deltaS: number,
  deltaHz: number,
  durationS: number,
  maxHzLimit = MAX_REGION_HZ,
): DryRegion {
  const safeDur = Math.max(MIN_REGION_DURATION_S * 2, durationS);
  const spanS = origin.endS - origin.startS;
  let startS = origin.startS;
  let endS = origin.endS;
  let minHz = origin.minHz;
  let maxHz = origin.maxHz;

  if (handle === 'move') {
    const clampedStart = Math.max(0, Math.min(safeDur - spanS, origin.startS + deltaS));
    startS = clampedStart;
    endS = clampedStart + spanS;
    if (origin.kind === 'marquee' && minHz !== null && maxHz !== null) {
      const spanHz = maxHz - minHz;
      const clampedMin = Math.max(MIN_REGION_HZ, Math.min(maxHzLimit - spanHz, minHz + deltaHz));
      minHz = clampedMin;
      maxHz = clampedMin + spanHz;
    }
  } else {
    if (handle === 'left' || handle === 'top-left' || handle === 'bottom-left') {
      startS = Math.min(origin.endS - MIN_REGION_DURATION_S, origin.startS + deltaS);
    }
    if (handle === 'right' || handle === 'top-right' || handle === 'bottom-right') {
      endS = Math.max(origin.startS + MIN_REGION_DURATION_S, origin.endS + deltaS);
    }
    if (origin.kind === 'marquee' && minHz !== null && maxHz !== null) {
      if (handle === 'bottom' || handle === 'bottom-left' || handle === 'bottom-right') {
        minHz = Math.min(maxHz - MIN_REGION_HZ_SPAN, minHz + deltaHz);
      }
      if (handle === 'top' || handle === 'top-left' || handle === 'top-right') {
        maxHz = Math.max(minHz + MIN_REGION_HZ_SPAN, maxHz + deltaHz);
      }
    }
  }

  return clampRegionBounds({ ...origin, startS, endS, minHz, maxHz }, safeDur, maxHzLimit);
}

/** Serialize a DryRegion into the Rust `spectracraft-qa dry-region-edit` JSON payload. */
export function serializeDryRegionForRust(region: DryRegion) {
  return {
    id: region.id,
    kind: region.kind,
    start_s: region.startS,
    end_s: region.endS,
    min_hz: region.kind === 'marquee' ? region.minHz : null,
    max_hz: region.kind === 'marquee' ? region.maxHz : null,
    note: region.note,
    action: region.action,
    strength: region.strength,
    crossfade_ms: region.crossfadeMs,
  };
}
