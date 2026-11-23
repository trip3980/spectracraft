import type { FrameEvidence, NoteSummary } from './workbench.ts';

export interface DryListeningChecklist {
  rhythm: boolean;
  phrasing: boolean;
  dynamics: boolean;
  sustain: boolean;
  artifactFree: boolean;
}

export type DryCriterionKey = keyof DryListeningChecklist;

export interface DryCriterionDescriptor {
  key: DryCriterionKey;
  label: string;
  detail: string;
}

export const DRY_LISTENING_CRITERIA: readonly DryCriterionDescriptor[] = [
  { key: 'rhythm', label: 'Rhythm', detail: 'Note onsets, offsets, and timing' },
  { key: 'phrasing', label: 'Phrasing', detail: 'Pitch bends, slides, and gesture continuity' },
  { key: 'dynamics', label: 'Dynamics', detail: 'Accents, contrast, and envelope contour' },
  { key: 'sustain', label: 'Sustain', detail: 'Ringing tails and overlapping note state' },
  { key: 'artifactFree', label: 'No artifacts', detail: 'Free of crackle, shimmer, clicks, or smear' },
] as const;

export function emptyDryListeningChecklist(): DryListeningChecklist {
  return {
    rhythm: false,
    phrasing: false,
    dynamics: false,
    sustain: false,
    artifactFree: false,
  };
}

export function countPassedDryCriteria(checklist: DryListeningChecklist): number {
  let count = 0;
  for (const item of DRY_LISTENING_CRITERIA) {
    if (checklist[item.key]) count += 1;
  }
  return count;
}

/**
 * Core Design Contract invariant:
 * Post-processing (Capture / Room / Mic / Variants) is strictly unlocked ONLY when
 * all 5 human listening checks pass on the current dry reconstruction.
 */
export function isDryListeningPassed(checklist: DryListeningChecklist): boolean {
  return countPassedDryCriteria(checklist) === DRY_LISTENING_CRITERIA.length;
}

export type PipelineStage = 'empty' | 'analysis' | 'cues' | 'dry_gate' | 'post_unlocked';

export function computePipelineStage(params: {
  hasSession: boolean;
  noteCount: number;
  hasDryAudio: boolean;
  checklist: DryListeningChecklist;
}): PipelineStage {
  if (!params.hasSession) return 'empty';
  if (params.noteCount <= 0) return 'analysis';
  if (!params.hasDryAudio) return 'cues';
  if (!isDryListeningPassed(params.checklist)) return 'dry_gate';
  return 'post_unlocked';
}

export type CueEvidenceClass = 'strong_coherent' | 'weak_consistent' | 'uncertain';

/**
 * Classifies a note cue according to the Core Design Contract's evidence tiers:
 * strong & coherent | weaker but consistent | unresolved / uncertain.
 * Never treats out-of-target energy as "noise" or invents confidence.
 */
export function classifyNoteEvidence(note: Pick<NoteSummary, 'frames' | 'peak_level' | 'uncertain_overlap_count'>): CueEvidenceClass {
  const overlapRatio = note.frames > 0 ? note.uncertain_overlap_count / note.frames : 1;
  if (overlapRatio >= 0.25 || note.frames < 4) {
    return 'uncertain';
  }
  if (note.peak_level < 0.22 || note.uncertain_overlap_count > 0) {
    return 'weak_consistent';
  }
  return 'strong_coherent';
}

export interface EvidenceBreakdown {
  strongNotes: number;
  weakNotes: number;
  uncertainNotes: number;
  supportedFrames: number;
  uncertainFrames: number;
  outOfTargetFrames: number;
}

export function summarizeEvidence(
  notes: readonly Pick<NoteSummary, 'frames' | 'peak_level' | 'uncertain_overlap_count'>[],
  frames: readonly Pick<FrameEvidence, 'status'>[],
): EvidenceBreakdown {
  let strongNotes = 0;
  let weakNotes = 0;
  let uncertainNotes = 0;
  for (const n of notes) {
    const cls = classifyNoteEvidence(n);
    if (cls === 'strong_coherent') strongNotes += 1;
    else if (cls === 'weak_consistent') weakNotes += 1;
    else uncertainNotes += 1;
  }
  let supportedFrames = 0;
  let uncertainFrames = 0;
  let outOfTargetFrames = 0;
  for (const f of frames) {
    if (f.status === 'supported') supportedFrames += 1;
    else if (f.status === 'uncertain') uncertainFrames += 1;
    else if (f.status === 'out_of_target') outOfTargetFrames += 1;
  }
  return {
    strongNotes,
    weakNotes,
    uncertainNotes,
    supportedFrames,
    uncertainFrames,
    outOfTargetFrames,
  };
}

/**
 * Converts a dB level-match offset (e.g. Rust `ab_dry_match_gain_db`) into a safe linear GainNode multiplier.
 * Clamped to [-24 dB, +12 dB] so malformed metadata cannot clip output.
 */
export function dbToSafeLinearGain(db: number): number {
  if (!Number.isFinite(db)) return 1.0;
  const clamped = Math.max(-24, Math.min(12, db));
  return Math.pow(10, clamped / 20);
}
