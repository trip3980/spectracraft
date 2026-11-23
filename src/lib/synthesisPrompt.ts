import {
  FACE_LABELS,
  PATTERN_LABELS,
  computeAcousticFingerprint,
  metersToDisplay,
  speedOfSoundMps,
  unitDecimals,
  type PipelineStagesState,
  type RustAcousticAnalysis,
  type RustMicEntry,
  type RustSourceEntry,
  type RustSurfaceEntry,
  type StudioRigState,
  type VoicingControlsState,
} from './studioRig.ts';

export interface PitchTrajectoryShape {
  note_count: number;
  min_hz: number | null;
  max_hz: number | null;
  median_hz: number | null;
  max_bend_semitones: number;
  mean_pitch_span_semitones: number;
  notes_with_bends: number;
  out_of_published_range_notes: number;
  published_range_note: string;
}

export interface DynamicsShape {
  peak_dbfs: number;
  rms_dbfs: number;
  dynamic_range_db: number;
  note_level_contrast_db: number;
  strongest_note_index: number | null;
  quietest_note_index: number | null;
}

export interface SpectralProminence {
  center_hz: number;
  relative_db: number;
  band_label: string;
}

export interface SpectralEnvelopeShape {
  spectral_centroid_hz: number;
  band_centers_hz: number[];
  band_relative_db: number[];
  prominence_regions: SpectralProminence[];
  envelope_note: string;
}

export interface HarmonicNoiseShape {
  mean_tonal_fraction: number;
  min_tonal_fraction: number;
  max_tonal_fraction: number;
  mean_noise_fraction: number;
  balance_label: string;
  bleed_risk: string;
  bleed_note: string;
}

export interface AttackDecayShape {
  median_attack_ms: number;
  median_low_decay_per_s: number;
  median_high_decay_per_s: number;
  sustain_ratio: number;
  total_repicks: number;
  attack_character_label: string;
}

export interface TransitionsShape {
  overlapping_ring_notes: number;
  connected_transitions: number;
  separated_transitions: number;
  continuity_ratio: number;
  transition_note: string;
}

export interface UncertaintySummary {
  total_frames: number;
  supported_frames: number;
  uncertain_frames: number;
  out_of_target_frames: number;
  silence_frames: number;
  uncertain_pct: number;
  out_of_target_pct: number;
  flags: string[];
  limits_note: string;
}

export type SignalCharacterGuess = 'Natural' | 'Analog' | 'Digital' | 'Unclear' | 'natural' | 'analog' | 'digital' | 'unclear';

export interface LooseProfileSuggestion {
  id: string;
  label: string;
  confidence: string;
  score: number;
  note: string;
}

export interface SignalCharacterEstimate {
  guess: SignalCharacterGuess;
  confidence: string;
  noise_floor_dbfs: number;
  hf_rolloff_ratio: number;
  crest_factor_db: number;
  room_energy_estimate: number;
  linearity_score: number;
  wandering_frame_pct: number;
  straighten_recommended: boolean;
  evidence_summary: string;
  disclaimer: string;
  source_family_guess?: LooseProfileSuggestion;
  mic_style_guess?: LooseProfileSuggestion;
  room_category_guess?: LooseProfileSuggestion;
}

export interface ReferenceShape {
  shape_sha256: string;
  sample_rate: number;
  duration_s: number;
  pitch_trajectory: PitchTrajectoryShape;
  dynamics: DynamicsShape;
  spectral_envelope: SpectralEnvelopeShape;
  harmonic_vs_noise: HarmonicNoiseShape;
  attack_and_decay: AttackDecayShape;
  transitions: TransitionsShape;
  uncertainty: UncertaintySummary;
  signal_character?: SignalCharacterEstimate;
}

export interface StructuredMicCondition {
  label: string;
  modelName: string;
  patternLabel: string;
  distanceM: number;
  aimSummary: string;
}

export interface StructuredSynthesisPrompt {
  shapeAnchor: {
    sessionId: string;
    sourceFileName: string;
    shapeSha256: string;
    noteCount: number;
    pitchRangeLabel: string;
    maxBendSemitones: number;
    tonalPct: number;
    uncertainFrames: number;
    outOfTargetFrames: number;
    bleedRisk: string;
  };
  sourceCondition: {
    sourceIndex: number;
    sourceName: string;
    family: string;
    regenTemplateId: string;
    outOfRangeKeptCount: number;
  };
  captureCondition: {
    activeMics: StructuredMicCondition[];
    roomPreset: StudioRigState['preset'];
    roomSummary: string;
    materialsSummary: string;
    activeFaceMaterials: { face: string; material: string }[];
    temperatureC: number;
    speedOfSoundMps: number;
    directDelayMs: number | null;
  };
  promptSentence: string;
  /**
   * Deterministic fingerprint strictly of { shapeSha256, sourceIndex, acousticFingerprint }.
   * Guaranteed INVARIANT across display-only actions (orbit, pan, zoom, Fit View,
   * unit switches, Patterns / 3:1 overlays, inspector tab switches, or dock layout changes).
   */
  promptFingerprint: string;
}

function fnv1aHex(input: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, '0');
}

function summarizeMaterials(
  rig: StudioRigState,
  surfaces: RustSurfaceEntry[],
): { activeFaceMaterials: { face: string; material: string }[]; summary: string } {
  if (rig.preset === 'direct') {
    return {
      activeFaceMaterials: [],
      summary: 'Direct capture (no room reflections)',
    };
  }
  const activeIndices = rig.preset === 'open' ? [4] : [0, 1, 2, 3, 4, 5];
  const activeFaceMaterials = activeIndices.map((faceIdx) => {
    const surf = surfaces[rig.surfaceFaces[faceIdx]];
    return {
      face: FACE_LABELS[faceIdx],
      material: surf?.name ?? `Material #${rig.surfaceFaces[faceIdx]}`,
    };
  });
  const uniqueNames = Array.from(new Set(activeFaceMaterials.map((f) => f.material)));
  const summary =
    rig.preset === 'open'
      ? `Ground: ${uniqueNames[0] ?? 'Default'}`
      : uniqueNames.length === 1
        ? `All 6 faces: ${uniqueNames[0]}`
        : `${uniqueNames.length} distinct materials (${uniqueNames.slice(0, 2).join(', ')}${uniqueNames.length > 2 ? ', …' : ''})`;
  return { activeFaceMaterials, summary };
}

/**
 * Pure structured prompt builder uniting:
 *   1. Fixed ReferenceShape (from Rust single-source analysis)
 *   2. Selected Source voicing character
 *   3. Virtual Capture Lab setup (mic(s), materials, room)
 *
 * Notice: `promptFingerprint` depends ONLY on acoustic & shape inputs
 * (never on display units, camera view, overlays, or UI tabs).
 */
export function buildStructuredSynthesisPrompt(params: {
  sessionId: string;
  sourceFileName: string;
  shape: ReferenceShape;
  rig: StudioRigState;
  sources: RustSourceEntry[];
  mics: RustMicEntry[];
  surfaces: RustSurfaceEntry[];
  analysis: RustAcousticAnalysis | null;
  voicing?: VoicingControlsState;
  pipelineStages?: PipelineStagesState;
}): StructuredSynthesisPrompt {
  const {
    sessionId,
    sourceFileName,
    shape,
    rig,
    sources,
    mics,
    surfaces,
    analysis,
    voicing,
    pipelineStages,
  } = params;
  const src = sources[rig.sourceIndex];
  const sourceName = src?.name ?? `Source #${rig.sourceIndex}`;
  const sourceFamily = src?.family ?? 'Unknown';
  const regenTemplateId = src?.regen_model?.model_id ?? 'neutral';

  const unit = rig.unit;
  const dec = unitDecimals(unit);
  const fmtDist = (m: number) => `${metersToDisplay(m, unit).toFixed(dec)} ${unit}`;

  const activeMics: StructuredMicCondition[] = rig.mics
    .filter((m) => m.enabled)
    .map((slot) => {
      const cat = mics[slot.catalogIndex];
      const modelName = cat ? `${cat.brand} ${cat.model}`.trim() : `Mic #${slot.catalogIndex}`;
      const pattern = slot.patternOverride ?? cat?.default_pattern ?? 'cardioid';
      const patternLabel = PATTERN_LABELS[pattern] ?? pattern;
      const distM = Math.hypot(slot.offsetM.x, slot.offsetM.y, slot.offsetM.z);
      const aimSummary = slot.aim.followSource
        ? 'Aimed at source (Follow)'
        : `Heading ${Math.round(slot.aim.headingZDeg)}° · Tilt ${Math.round(slot.aim.tiltXDeg)}°`;
      return {
        label: slot.label,
        modelName,
        patternLabel,
        distanceM: distM,
        aimSummary,
      };
    });

  const roomSummary =
    rig.preset === 'direct'
      ? 'Direct (no room)'
      : rig.preset === 'open'
        ? `Open field (${fmtDist(rig.roomMeters.x)} × ${fmtDist(rig.roomMeters.y)} guide)`
        : `${rig.preset[0].toUpperCase()}${rig.preset.slice(1)} room (${fmtDist(rig.roomMeters.x)} × ${fmtDist(rig.roomMeters.y)} × ${fmtDist(rig.roomMeters.z)})`;

  const { activeFaceMaterials, summary: materialsSummary } = summarizeMaterials(rig, surfaces);

  const pitchRangeLabel =
    shape.pitch_trajectory.min_hz !== null && shape.pitch_trajectory.max_hz !== null
      ? `${Math.round(shape.pitch_trajectory.min_hz)}–${Math.round(shape.pitch_trajectory.max_hz)} Hz`
      : 'No tonal pitch extracted';

  const micDesc =
    activeMics.length === 0
      ? 'no enabled microphone'
      : activeMics
          .map((m) => `${m.modelName} (${m.patternLabel}, ${fmtDist(m.distanceM)}, ${m.aimSummary})`)
          .join(' + ');

  const voicingDesc = voicing
    ? ` [Stage 1 Dry: character=${voicing.characterTarget}, path=${voicing.signalPath}, variant=${voicing.sourceVariant}, straighten=${voicing.straightenMode}]`
    : '';
  const stagesDesc = pipelineStages
    ? ` [Post-Dry: Mic=${pipelineStages.micEnabled ? 'ON' : 'Bypassed'}, Room=${pipelineStages.roomEnabled ? 'ON' : 'Bypassed'}, Plugin=${pipelineStages.pluginSlot.enabled && pipelineStages.pluginSlot.pluginId !== 'none' ? pipelineStages.pluginSlot.pluginId : 'Bypassed'}]`
    : '';

  const promptSentence =
    `Take the fixed reference shape extracted from "${sourceFileName}" ` +
    `(${shape.pitch_trajectory.note_count} notes, ${pitchRangeLabel}, ${Math.round(shape.harmonic_vs_noise.mean_tonal_fraction * 100)}% tonal). ` +
    `Re-express it as a new signal voiced like ${sourceName}${voicingDesc}, ` +
    `captured by ${micDesc} in ${roomSummary} with ${materialsSummary}.${stagesDesc}`;

  // Strictly SI & acoustic fingerprint — immune to unit/camera/overlay/tab changes
  const acousticFp = computeAcousticFingerprint(rig);
  const voicingKey = voicing
    ? `|v:${voicing.characterTarget}:${voicing.signalPath}:${voicing.sourceVariant}:${voicing.straightenMode}:${voicing.straightenPreserveBends ? 1 : 0}:${voicing.profileGuidanceStrength.toFixed(2)}`
    : '';
  const stagesKey = pipelineStages
    ? `|p:${pipelineStages.micEnabled ? 1 : 0}:${pipelineStages.roomEnabled ? 1 : 0}:${pipelineStages.pluginSlot.enabled ? pipelineStages.pluginSlot.pluginId : 'off'}:${pipelineStages.pluginSlot.amount.toFixed(2)}:${pipelineStages.pluginSlot.eqLowDb.toFixed(1)}:${pipelineStages.pluginSlot.eqHighDb.toFixed(1)}:${pipelineStages.pluginSlot.ceilingDbfs.toFixed(1)}`
    : '';
  const promptFingerprint = fnv1aHex(
    `${shape.shape_sha256}|src:${rig.sourceIndex}|${acousticFp}${voicingKey}${stagesKey}`,
  );

  return {
    shapeAnchor: {
      sessionId,
      sourceFileName,
      shapeSha256: shape.shape_sha256,
      noteCount: shape.pitch_trajectory.note_count,
      pitchRangeLabel,
      maxBendSemitones: shape.pitch_trajectory.max_bend_semitones,
      tonalPct: Math.round(shape.harmonic_vs_noise.mean_tonal_fraction * 100),
      uncertainFrames: shape.uncertainty.uncertain_frames,
      outOfTargetFrames: shape.uncertainty.out_of_target_frames,
      bleedRisk: shape.harmonic_vs_noise.bleed_risk,
    },
    sourceCondition: {
      sourceIndex: rig.sourceIndex,
      sourceName,
      family: sourceFamily,
      regenTemplateId,
      outOfRangeKeptCount: shape.pitch_trajectory.out_of_published_range_notes,
    },
    captureCondition: {
      activeMics,
      roomPreset: rig.preset,
      roomSummary,
      materialsSummary,
      activeFaceMaterials,
      temperatureC: rig.temperatureC,
      speedOfSoundMps: analysis?.speed_of_sound_mps ?? speedOfSoundMps(rig.temperatureC),
      directDelayMs: analysis?.direct_path.direct_delay_ms ?? null,
    },
    promptSentence,
    promptFingerprint,
  };
}
