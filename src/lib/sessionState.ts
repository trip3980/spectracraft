/**
 * Authoritative frontend session-epoch, immutable revision, audio undo/redo,
 * revision-bound dry approval, and versioned Project v2 persistence helpers
 * for SpectraCraft (Audit H4, H5, H6, H7).
 *
 * Pure TypeScript logic with zero DOM/React dependencies so every invariant
 * can be verified directly under `node --import tsx --test`.
 */

import type { DryRegion } from './dryRegions.ts';
import type {
  PipelineStagesState,
  StudioRigState,
  VoicingControlsState,
} from './studioRig.ts';
import type { NoteEditPatch } from './workbench.ts';

export const PROJECT_SCHEMA_VERSION = 2 as const;

export interface DryRevision {
  /** Immutable client-side revision ID (unique per dry buffer version). */
  id: string;
  /** Parent revision ID, or null for the initial Stage 1 dry synthesis. */
  parentRevisionId: string | null;
  /** ID of the Stage 1 base dry revision (before Stage 2 surgical edits). */
  stage1BaseId: string;
  /** Authoritative SHA-256 of the dry WAV bytes produced by Rust. */
  drySha256: string;
  /** Media URL pointing to this exact immutable dry WAV artifact. */
  dryWavUrl: string;
  /** Media URL pointing to the cues JSON associated with this dry revision. */
  cuesJsonUrl: string;
  /** Server-side run ID (null/baseline for initial session dry take). */
  cuesRunId: string | null;
  /** Whether this revision was created by a Stage 2 surgical dry edit. */
  isStage2Edit: boolean;
  /** Short human-readable provenance label. */
  label: string;
  /** Optional summary of the edit or regeneration that produced this revision. */
  editSummary?: string;
  createdAt: string;
}

export interface RevisionApproval {
  approvedDryRevisionId: string;
  approvedDrySha256: string;
  approvedAt: string;
}

export interface AudioRevisionHistoryState {
  past: DryRevision[];
  present: DryRevision | null;
  future: DryRevision[];
  /** Authoritative Stage 1 base revision (used for Stage 2 bypass). */
  stage1Base: DryRevision | null;
  /** Whether Stage 2 surgical dry edits are currently bypassed. */
  stage2Bypassed: boolean;
}

export interface DependentArtifactProvenance {
  parentDryRevisionId: string;
  parentDrySha256: string;
  configKey?: string;
}

export interface ProjectEditTransactionRecord {
  id: string;
  kind: 'cue_render' | 'source_regen' | 'dry_region_edit';
  beforeDryRevisionId: string;
  beforeDrySha256: string;
  afterDryRevisionId: string;
  afterDrySha256: string;
  cuesRunId: string | null;
  summary: string;
  createdAt: string;
}

export interface ProjectFileV2 {
  schemaVersion: typeof PROJECT_SCHEMA_VERSION;
  kind: 'spectracraft_project';
  savedAt: string;
  source: {
    sessionId: string;
    label: string;
    sourceSha256: string;
    sampleRate: number;
    durationS: number;
  };
  cues: {
    cuesSha256: string | null;
    activeCuesRunId: string | null;
    patches: Record<number, NoteEditPatch>;
  };
  stage1: {
    sourceIndex: number;
    voicing: VoicingControlsState;
    baseDryRevision: DryRevision;
  };
  stage2: {
    activeDryRevision: DryRevision;
    stage2Bypassed: boolean;
    regions: DryRegion[];
    editTransactions: ProjectEditTransactionRecord[];
  };
  postDry: {
    rig: StudioRigState;
    pipelineStages: PipelineStagesState;
    lastPipelineRunId: string | null;
    lastPipelineSha256: string | null;
  };
  approval: RevisionApproval | null;
}

export type ProjectLoadResult =
  | {
      ok: true;
      kind: 'project_v2';
      project: ProjectFileV2;
      assetMatch: 'exact' | 'source_mismatch';
      warnings: string[];
    }
  | {
      ok: true;
      kind: 'settings_preset_v1';
      rig: Partial<StudioRigState> | null;
      warnings: string[];
    }
  | {
      ok: false;
      error: string;
      code: 'INVALID_JSON' | 'UNSUPPORTED_SCHEMA' | 'CORRUPT_PROJECT';
    };

let revisionCounter = 0;

export function nextDryRevisionId(prefix = 'dryrev'): string {
  revisionCounter += 1;
  const rand = Math.random().toString(36).slice(2, 7);
  return `${prefix}-${revisionCounter}-${rand}`;
}

/**
 * Monotonic session-epoch tracker preventing late async completions from overwriting
 * a newly opened or closed session (Audit H4).
 */
export class SessionEpochTracker {
  private epoch = 0;
  private activeSessionId: string | null = null;

  public current(): number {
    return this.epoch;
  }

  public sessionId(): string | null {
    return this.activeSessionId;
  }

  public beginSession(sessionId: string): number {
    this.epoch += 1;
    this.activeSessionId = sessionId;
    return this.epoch;
  }

  public closeSession(): number {
    this.epoch += 1;
    this.activeSessionId = null;
    return this.epoch;
  }

  public isCurrent(expectedEpoch: number, expectedSessionId?: string | null): boolean {
    if (this.epoch !== expectedEpoch) return false;
    if (expectedSessionId !== undefined && this.activeSessionId !== expectedSessionId) {
      return false;
    }
    return true;
  }
}

export function createInitialAudioHistory(): AudioRevisionHistoryState {
  return {
    past: [],
    present: null,
    future: [],
    stage1Base: null,
    stage2Bypassed: false,
  };
}

export function initAudioHistoryWithBase(base: DryRevision): AudioRevisionHistoryState {
  const normalized: DryRevision = {
    ...base,
    parentRevisionId: null,
    stage1BaseId: base.id,
    isStage2Edit: false,
  };
  return {
    past: [],
    present: normalized,
    future: [],
    stage1Base: normalized,
    stage2Bypassed: false,
  };
}

/**
 * Push a new Stage 1 dry regeneration (cue edit or source voicing change).
 * Updates `stage1Base` while still recording the prior dry revision on the undo stack
 * so the user can undo back across regenerations cleanly.
 */
export function pushStage1DryRevision(
  state: AudioRevisionHistoryState,
  next: Omit<DryRevision, 'parentRevisionId' | 'stage1BaseId' | 'isStage2Edit'>,
): AudioRevisionHistoryState {
  const rev: DryRevision = {
    ...next,
    parentRevisionId: state.present?.id ?? null,
    stage1BaseId: next.id,
    isStage2Edit: false,
  };
  const past = state.present ? [...state.past.slice(-29), state.present] : state.past;
  return {
    past,
    present: rev,
    future: [],
    stage1Base: rev,
    stage2Bypassed: false,
  };
}

/**
 * Push a committed Stage 2 surgical dry edit. Preserves `stage1Base` so Stage 2 bypass
 * can deterministically audition/export the unedited Stage 1 dry audio (Audit H7).
 */
export function pushStage2DryEditRevision(
  state: AudioRevisionHistoryState,
  next: Omit<DryRevision, 'parentRevisionId' | 'stage1BaseId' | 'isStage2Edit'>,
): AudioRevisionHistoryState {
  const baseId = state.stage1Base?.id ?? state.present?.stage1BaseId ?? next.id;
  const rev: DryRevision = {
    ...next,
    parentRevisionId: state.present?.id ?? null,
    stage1BaseId: baseId,
    isStage2Edit: true,
  };
  const past = state.present ? [...state.past.slice(-29), state.present] : state.past;
  return {
    past,
    present: rev,
    future: [],
    stage1Base: state.stage1Base ?? state.present ?? rev,
    stage2Bypassed: false,
  };
}

export function undoAudioRevision(state: AudioRevisionHistoryState): AudioRevisionHistoryState {
  if (!state.present || state.past.length === 0) return state;
  const previous = state.past[state.past.length - 1];
  const nextPast = state.past.slice(0, -1);
  const nextFuture = [state.present, ...state.future.slice(0, 29)];
  const nextBase = !previous.isStage2Edit ? previous : state.stage1Base;
  return {
    past: nextPast,
    present: previous,
    future: nextFuture,
    stage1Base: nextBase,
    stage2Bypassed: false,
  };
}

export function redoAudioRevision(state: AudioRevisionHistoryState): AudioRevisionHistoryState {
  if (!state.present || state.future.length === 0) return state;
  const next = state.future[0];
  const nextFuture = state.future.slice(1);
  const nextPast = [...state.past.slice(-29), state.present];
  const nextBase = !next.isStage2Edit ? next : state.stage1Base;
  return {
    past: nextPast,
    present: next,
    future: nextFuture,
    stage1Base: nextBase,
    stage2Bypassed: false,
  };
}

export function setStage2Bypass(
  state: AudioRevisionHistoryState,
  bypassed: boolean,
): AudioRevisionHistoryState {
  if (state.stage2Bypassed === bypassed) return state;
  return { ...state, stage2Bypassed: bypassed };
}

/**
 * Returns the effective dry revision currently heard and exported:
 * when Stage 2 bypass is enabled, returns `stage1Base`; otherwise returns `present`.
 */
export function getEffectiveDryRevision(state: AudioRevisionHistoryState): DryRevision | null {
  if (state.stage2Bypassed && state.stage1Base) {
    return state.stage1Base;
  }
  return state.present;
}

/**
 * Dry approval is bound strictly to the exact dry revision ID and SHA-256 (Audit H5).
 * Any accepted dry change, undo/redo, or Stage 2 bypass toggle automatically invalidates
 * approval until the human listens to and approves the active dry revision.
 */
export function isApprovalValidForRevision(
  approval: RevisionApproval | null,
  activeDry: DryRevision | null,
): boolean {
  if (!approval || !activeDry) return false;
  return (
    approval.approvedDryRevisionId === activeDry.id &&
    approval.approvedDrySha256 === activeDry.drySha256 &&
    activeDry.drySha256.length > 0
  );
}

/**
 * Returns true if a dependent capture, report, variant, or DAW export is stale relative
 * to the currently active dry revision (and optional pipeline config key).
 */
export function isArtifactStaleForDry(
  provenance: DependentArtifactProvenance | null,
  activeDry: DryRevision | null,
  activeConfigKey?: string,
): boolean {
  if (!provenance || !activeDry) return false;
  if (
    provenance.parentDryRevisionId !== activeDry.id ||
    provenance.parentDrySha256 !== activeDry.drySha256
  ) {
    return true;
  }
  if (
    activeConfigKey !== undefined &&
    provenance.configKey !== undefined &&
    provenance.configKey !== activeConfigKey
  ) {
    return true;
  }
  return false;
}

/**
 * Validate and classify a loaded project JSON file (Audit H6).
 * Explicitly distinguishes full v2 projects from legacy v1 settings-only presets,
 * and flags source SHA-256 mismatches so settings are never silently applied onto wrong audio.
 */
export function parseAndValidateProjectFile(
  rawText: string,
  currentSourceSha256?: string | null,
): ProjectLoadResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(rawText);
  } catch {
    return {
      ok: false,
      code: 'INVALID_JSON',
      error: 'Project file is not valid JSON.',
    };
  }

  if (!parsed || typeof parsed !== 'object') {
    return {
      ok: false,
      code: 'CORRUPT_PROJECT',
      error: 'Project file root must be a JSON object.',
    };
  }

  const obj = parsed as Record<string, unknown>;

  // Legacy v1 settings-only file detection (e.g. native desktop StudioSessionFile v1)
  if (obj.schemaVersion === 1 || (obj.schemaVersion === undefined && obj.rig && !obj.stage1)) {
    return {
      ok: true,
      kind: 'settings_preset_v1',
      rig: (obj.rig as Partial<StudioRigState>) ?? null,
      warnings: [
        'This is a v1 settings-only preset file and does not contain audio provenance, cues, or Stage 2 edits. Only rig settings can be imported.',
      ],
    };
  }

  if (obj.schemaVersion !== PROJECT_SCHEMA_VERSION || obj.kind !== 'spectracraft_project') {
    return {
      ok: false,
      code: 'UNSUPPORTED_SCHEMA',
      error: `Unsupported project schema version (${String(obj.schemaVersion ?? 'unknown')}). Expected schemaVersion ${PROJECT_SCHEMA_VERSION}.`,
    };
  }

  const source = obj.source as Record<string, unknown> | undefined;
  const stage1 = obj.stage1 as Record<string, unknown> | undefined;
  const stage2 = obj.stage2 as Record<string, unknown> | undefined;
  const postDry = obj.postDry as Record<string, unknown> | undefined;
  if (
    !source ||
    typeof source.sourceSha256 !== 'string' ||
    !source.sourceSha256 ||
    typeof source.label !== 'string' ||
    !stage1 ||
    !stage1.baseDryRevision ||
    !stage2 ||
    !stage2.activeDryRevision ||
    !postDry ||
    !postDry.rig
  ) {
    return {
      ok: false,
      code: 'CORRUPT_PROJECT',
      error: 'Project v2 file is missing required source, stage1, stage2, or postDry records.',
    };
  }

  const project = parsed as ProjectFileV2;
  const warnings: string[] = [];
  let assetMatch: 'exact' | 'source_mismatch' = 'exact';
  if (currentSourceSha256 && project.source.sourceSha256 !== currentSourceSha256) {
    assetMatch = 'source_mismatch';
    warnings.push(
      `Source audio SHA-256 mismatch: project expects ${project.source.sourceSha256.slice(0, 12)} (${project.source.label}), while the open session is ${currentSourceSha256.slice(0, 12)}. Relink or open the original source WAV before restoring edits.`,
    );
  }

  return {
    ok: true,
    kind: 'project_v2',
    project,
    assetMatch,
    warnings,
  };
}
