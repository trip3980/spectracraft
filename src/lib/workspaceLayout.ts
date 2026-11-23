/**
 * Pure ArtCraft-style rearrangeable workspace layout state.
 *
 * Strict architectural boundary (Refactoring Process §3 & §4):
 * - This module owns ONLY UI chrome geometry (panel widths, dock height,
 *   horizontal pane order, floating/undocked windows, and spatial view split).
 * - It has ZERO references to StudioRigState, audio buffers, playback, or
 *   catalogs — rearranging or resizing panels cannot alter acoustic state.
 */

export type WorkspacePaneId = 'inspector' | 'timeline' | 'spatial';
export type SpatialDisplayLayout = 'plans' | '3d' | 'split';

export interface FloatingBounds {
  floating: boolean;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface WorkspaceLayoutState {
  /** Horizontal order of docked panes left-to-right (default: Preview 19 inspector -> timeline -> spatial). */
  paneOrder: [WorkspacePaneId, WorkspacePaneId, WorkspacePaneId];
  inspectorOpen: boolean;
  inspectorWidth: number;
  inspectorFloat: FloatingBounds;
  spatialOpen: boolean;
  spatialWidth: number;
  spatialFloat: FloatingBounds;
  spatialMode: SpatialDisplayLayout;
  /** Fraction [0.25..0.75] of spatial pane given to Plans when spatialMode === 'split'. */
  spatialSplitRatio: number;
  bottomDockCollapsed: boolean;
  bottomDockHeight: number;
}

export const LAYOUT_STORAGE_KEY = 'spectracraft.workspace-layout.v1';

export const LAYOUT_LIMITS = {
  inspectorMinW: 250,
  inspectorMaxW: 520,
  spatialMinW: 270,
  spatialMaxW: 680,
  dockMinH: 112,
  dockMaxH: 400,
  floatMinW: 280,
  floatMinH: 260,
  floatMaxW: 900,
  floatMaxH: 780,
} as const;

export function createDefaultWorkspaceLayout(): WorkspaceLayoutState {
  return {
    paneOrder: ['inspector', 'timeline', 'spatial'],
    inspectorOpen: true,
    inspectorWidth: 330,
    inspectorFloat: { floating: false, x: 56, y: 108, w: 344, h: 660 },
    spatialOpen: true,
    spatialWidth: 372,
    spatialFloat: { floating: false, x: 980, y: 108, w: 420, h: 640 },
    spatialMode: 'plans',
    spatialSplitRatio: 0.52,
    bottomDockCollapsed: false,
    bottomDockHeight: 204,
  };
}

function clamp(v: number, lo: number, hi: number, fallback: number): number {
  if (!Number.isFinite(v)) return fallback;
  return Math.max(lo, Math.min(hi, Math.round(v)));
}

function clampFloatBounds(b: Partial<FloatingBounds> | undefined, fb: FloatingBounds): FloatingBounds {
  return {
    floating: typeof b?.floating === 'boolean' ? b.floating : fb.floating,
    x: clamp(b?.x ?? fb.x, 8, 1280, fb.x),
    y: clamp(b?.y ?? fb.y, 40, 720, fb.y),
    w: clamp(b?.w ?? fb.w, LAYOUT_LIMITS.floatMinW, LAYOUT_LIMITS.floatMaxW, fb.w),
    h: clamp(b?.h ?? fb.h, LAYOUT_LIMITS.floatMinH, LAYOUT_LIMITS.floatMaxH, fb.h),
  };
}

export function sanitizeWorkspaceLayout(
  raw: Partial<WorkspaceLayoutState> | null | undefined,
): WorkspaceLayoutState {
  const def = createDefaultWorkspaceLayout();
  if (!raw || typeof raw !== 'object') return def;

  let paneOrder: [WorkspacePaneId, WorkspacePaneId, WorkspacePaneId] = def.paneOrder;
  if (
    Array.isArray(raw.paneOrder) &&
    raw.paneOrder.length === 3 &&
    raw.paneOrder.includes('inspector') &&
    raw.paneOrder.includes('timeline') &&
    raw.paneOrder.includes('spatial')
  ) {
    paneOrder = [raw.paneOrder[0], raw.paneOrder[1], raw.paneOrder[2]];
  }

  const spatialMode: SpatialDisplayLayout =
    raw.spatialMode === 'plans' || raw.spatialMode === '3d' || raw.spatialMode === 'split'
      ? raw.spatialMode
      : def.spatialMode;

  const rawRatio = Number(raw.spatialSplitRatio);
  const spatialSplitRatio = Number.isFinite(rawRatio)
    ? Math.max(0.25, Math.min(0.75, rawRatio))
    : def.spatialSplitRatio;

  return {
    paneOrder,
    inspectorOpen: typeof raw.inspectorOpen === 'boolean' ? raw.inspectorOpen : def.inspectorOpen,
    inspectorWidth: clamp(
      raw.inspectorWidth ?? def.inspectorWidth,
      LAYOUT_LIMITS.inspectorMinW,
      LAYOUT_LIMITS.inspectorMaxW,
      def.inspectorWidth,
    ),
    inspectorFloat: clampFloatBounds(raw.inspectorFloat, def.inspectorFloat),
    spatialOpen: typeof raw.spatialOpen === 'boolean' ? raw.spatialOpen : def.spatialOpen,
    spatialWidth: clamp(
      raw.spatialWidth ?? def.spatialWidth,
      LAYOUT_LIMITS.spatialMinW,
      LAYOUT_LIMITS.spatialMaxW,
      def.spatialWidth,
    ),
    spatialFloat: clampFloatBounds(raw.spatialFloat, def.spatialFloat),
    spatialMode,
    spatialSplitRatio,
    bottomDockCollapsed:
      typeof raw.bottomDockCollapsed === 'boolean'
        ? raw.bottomDockCollapsed
        : def.bottomDockCollapsed,
    bottomDockHeight: clamp(
      raw.bottomDockHeight ?? def.bottomDockHeight,
      LAYOUT_LIMITS.dockMinH,
      LAYOUT_LIMITS.dockMaxH,
      def.bottomDockHeight,
    ),
  };
}

export function resizeInspectorWidth(
  layout: WorkspaceLayoutState,
  nextWidth: number,
): WorkspaceLayoutState {
  return {
    ...layout,
    inspectorWidth: clamp(
      nextWidth,
      LAYOUT_LIMITS.inspectorMinW,
      LAYOUT_LIMITS.inspectorMaxW,
      layout.inspectorWidth,
    ),
  };
}

export function resizeSpatialWidth(
  layout: WorkspaceLayoutState,
  nextWidth: number,
): WorkspaceLayoutState {
  return {
    ...layout,
    spatialWidth: clamp(
      nextWidth,
      LAYOUT_LIMITS.spatialMinW,
      LAYOUT_LIMITS.spatialMaxW,
      layout.spatialWidth,
    ),
  };
}

export function resizeBottomDockHeight(
  layout: WorkspaceLayoutState,
  nextHeight: number,
): WorkspaceLayoutState {
  return {
    ...layout,
    bottomDockCollapsed: false,
    bottomDockHeight: clamp(
      nextHeight,
      LAYOUT_LIMITS.dockMinH,
      LAYOUT_LIMITS.dockMaxH,
      layout.bottomDockHeight,
    ),
  };
}

/** Reorder a pane one slot left (-1) or right (+1) without losing any pane. */
export function movePaneInOrder(
  layout: WorkspaceLayoutState,
  pane: WorkspacePaneId,
  direction: -1 | 1,
): WorkspaceLayoutState {
  const idx = layout.paneOrder.indexOf(pane);
  const target = idx + direction;
  if (idx < 0 || target < 0 || target >= layout.paneOrder.length) return layout;
  const next = [...layout.paneOrder] as [WorkspacePaneId, WorkspacePaneId, WorkspacePaneId];
  const tmp = next[idx];
  next[idx] = next[target];
  next[target] = tmp;
  return { ...layout, paneOrder: next };
}

export function setPaneFloating(
  layout: WorkspaceLayoutState,
  pane: 'inspector' | 'spatial',
  floating: boolean,
): WorkspaceLayoutState {
  if (pane === 'inspector') {
    return {
      ...layout,
      inspectorOpen: true,
      inspectorFloat: { ...layout.inspectorFloat, floating },
    };
  }
  return {
    ...layout,
    spatialOpen: true,
    spatialFloat: { ...layout.spatialFloat, floating },
  };
}

export function updateFloatingBounds(
  layout: WorkspaceLayoutState,
  pane: 'inspector' | 'spatial',
  patch: Partial<FloatingBounds>,
): WorkspaceLayoutState {
  if (pane === 'inspector') {
    return {
      ...layout,
      inspectorFloat: clampFloatBounds(
        { ...layout.inspectorFloat, ...patch },
        layout.inspectorFloat,
      ),
    };
  }
  return {
    ...layout,
    spatialFloat: clampFloatBounds({ ...layout.spatialFloat, ...patch }, layout.spatialFloat),
  };
}

export function loadPersistedWorkspaceLayout(): WorkspaceLayoutState {
  try {
    if (typeof window === 'undefined' || !window.localStorage) {
      return createDefaultWorkspaceLayout();
    }
    const raw = window.localStorage.getItem(LAYOUT_STORAGE_KEY);
    if (!raw) return createDefaultWorkspaceLayout();
    return sanitizeWorkspaceLayout(JSON.parse(raw) as Partial<WorkspaceLayoutState>);
  } catch {
    return createDefaultWorkspaceLayout();
  }
}

export function savePersistedWorkspaceLayout(layout: WorkspaceLayoutState): void {
  try {
    if (typeof window === 'undefined' || !window.localStorage) return;
    window.localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // Ignore storage quota or sandbox restrictions
  }
}
