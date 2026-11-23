export const BANDS_HZ = [125, 250, 500, 1000, 2000, 4000] as const;

export type UnitMode = 'ft' | 'in' | 'yd' | 'mm' | 'cm' | 'm';
export const ALL_UNITS: readonly UnitMode[] = ['ft', 'in', 'yd', 'mm', 'cm', 'm'] as const;

export type PolarPattern =
  | 'omni'
  | 'wide_cardioid'
  | 'cardioid'
  | 'supercardioid'
  | 'hypercardioid'
  | 'figure8';

export const ALL_PATTERNS: readonly { key: PolarPattern; label: string; alpha: number }[] = [
  { key: 'omni', label: 'Omni', alpha: 1.0 },
  { key: 'wide_cardioid', label: 'Wide cardioid', alpha: 0.7 },
  { key: 'cardioid', label: 'Cardioid', alpha: 0.5 },
  { key: 'supercardioid', label: 'Supercardioid', alpha: 0.37 },
  { key: 'hypercardioid', label: 'Hypercardioid', alpha: 0.25 },
  { key: 'figure8', label: 'Figure-8', alpha: 0.0 },
] as const;

export type RoomPresetKey = 'small' | 'medium' | 'large' | 'open' | 'direct';

export type GridDirectionKey = '+X' | '-X' | '+Y' | '-Y' | '+Z' | '-Z';

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface RustSourceSpectralRegions {
  body_warmth_hz: [number, number] | null;
  presence_attack_hz: [number, number] | null;
  air_high_hz: [number, number] | null;
  formants_and_resonances_note: string;
}

export interface RustSourcePhysicsConstraint {
  attack_ms: number;
  release_ms: number;
  breath_texture_amount: number;
  dynamic_brightness_amount: number;
  harmonic_ceiling_hz: number | null;
  resonance_contour_summary: string;
  texture_mechanism: string;
  plausibility_note: string;
}

export interface RustSourceRegenModel {
  model_id: string;
  model_label: string;
  description: string;
  physics_constraint?: RustSourcePhysicsConstraint;
}

export interface RustSourceEntry {
  index: number;
  id: string;
  name: string;
  category: 'Unknown' | 'Instrument' | 'Human';
  family: string;
  symbol:
    | 'unknown'
    | 'guitar'
    | 'amp'
    | 'bowed'
    | 'wind'
    | 'brass'
    | 'percussion'
    | 'keyboard'
    | 'voice'
    | 'ensemble';
  published_range_hz: [number, number] | null;
  published_fundamental_notes: string | null;
  published_practical_range_hz: [number, number] | null;
  spectral_regions: RustSourceSpectralRegions;
  published_notes: string;
  source_references: string[];
  uncertainties: string[];
  radiation_label: string;
  radiation_alpha: number;
  synthesis_model_hint?: string;
  regen_model: RustSourceRegenModel;
}

export interface RustSourceRegenResult {
  runId: string;
  drySha256: string;
  sourceIndex: number;
  sourceId: string;
  sourceName: string;
  regenModelId: string;
  regenModelLabel: string;
  samples: number;
  sampleRate: number;
  noteCount: number;
  spectral: {
    peak_dbfs: number | null;
    rms_dbfs: number | null;
    spectral_centroid_hz: number;
    low_band_pct: number;
    mid_band_pct: number;
    high_band_pct: number;
  };
  dryWavUrl: string;
  cuesJsonUrl: string;
}

export interface RustMicOffaxisTable {
  freqs_hz: number[];
  atten_90deg_db: number[];
  atten_180deg_db: number[];
}

export interface RustMicEntry {
  index: number;
  id: string;
  brand: string;
  model: string;
  source_url: string;
  transducer: string;
  default_pattern: PolarPattern;
  default_pattern_label: string;
  published_type: string | null;
  published_patterns: string | null;
  published_curves: string[];
  published_primary_curve: string | null;
  published_fr_points: [number, number][];
  published_offaxis: RustMicOffaxisTable | null;
  published_range_text: string | null;
  published_range_hz: [number, number] | null;
  published_sensitivity_dbv: number | null;
  published_self_noise_dba: number | null;
  published_max_spl_db: number | null;
  published_impedance_ohm: number | null;
  published_released_year: number | null;
  published_measurement_dist_cm: number | null;
  published_power: string | null;
  source_document: string;
  uncertainties: string[];
  renderer_profile_id: string;
  renderer_profile_description: string;
  curve_provenance: string;
  published_notes: string;
  shared_model_explanation: string;
}

export interface RustSurfaceBiotParameters {
  porosity: number | null;
  airflow_resistivity_pa_s_m2: number | null;
  tortuosity: number | null;
  vcl_um: number | null;
  tcl_um: number | null;
  standard_ref: string;
  status: string;
}

export interface RustSurfaceEntry {
  index: number;
  id: string;
  name: string;
  category: string;
  material_type: string;
  thickness_mm: number | null;
  assembly: string;
  incidence: string;
  standard: string;
  response_kind: string;
  raw_absorption_6band: [number, number, number, number, number, number];
  effective_absorption_6band: [number, number, number, number, number, number];
  pressure_reflectance_6band: [number, number, number, number, number, number];
  mean_reflectance: number;
  biot: RustSurfaceBiotParameters;
  provenance: string;
  notes: string;
  uncertainties: string[];
}

export interface RustRoomPreset {
  key: RoomPresetKey;
  label: string;
  dimensions_m: [number, number, number];
  default_surface_faces: [number, number, number, number, number, number];
}

export interface LooseProfileEntry {
  id: string;
  category: string;
  label: string;
  summary: string;
  bands_db: [number, number, number, number, number, number];
  noise_balance_hint: number;
  harmonic_decay_hint: number;
}

export interface LooseProfileLibrary {
  version: string;
  guidance_note: string;
  instrument_families: LooseProfileEntry[];
  mic_styles: LooseProfileEntry[];
  signal_characters: LooseProfileEntry[];
  room_categories: LooseProfileEntry[];
}

export interface RustStudioCatalogs {
  bands_hz: number[];
  face_labels: string[];
  sources: RustSourceEntry[];
  mics: RustMicEntry[];
  surfaces: RustSurfaceEntry[];
  room_presets: RustRoomPreset[];
  patterns: { key: PolarPattern; label: string; alpha: number }[];
  loose_profiles?: LooseProfileLibrary;
  catalog_provenance: {
    engine: string;
    source_count: number;
    mic_count: number;
    surface_count: number;
    mic_archive_note: string;
  };
}

export type CharacterTarget = 'keep' | 'natural' | 'analog' | 'digital';
export type SignalPathStyle = 'default' | 'mic_d' | 'pickup_direct' | 'analog_preamp' | 'clean_digital';
export type SourceCharacterVariant = 'standard' | 'rich' | 'basic' | 'warm' | 'bright';
export type StraightenMode = 'off' | 'light' | 'moderate';

export interface VoicingControlsState {
  characterTarget: CharacterTarget;
  signalPath: SignalPathStyle;
  sourceVariant: SourceCharacterVariant;
  straightenMode: StraightenMode;
  straightenPreserveBends: boolean;
  profileGuidanceStrength: number;
}

export const DEFAULT_VOICING_CONTROLS: VoicingControlsState = {
  characterTarget: 'keep',
  signalPath: 'default',
  sourceVariant: 'standard',
  straightenMode: 'off',
  straightenPreserveBends: true,
  profileGuidanceStrength: 0.4,
};

export type PluginSlotId = 'none' | 'gentle_eq' | 'soft_dynamics' | 'noise_control' | 'phase_limiter';

export interface PluginSlotState {
  enabled: boolean;
  pluginId: PluginSlotId;
  eqLowDb: number;
  eqHighDb: number;
  amount: number;
  ceilingDbfs: number;
}

export interface PipelineStagesState {
  /** Stage 3: Optional Microphone Stage (OFF by default). */
  micEnabled: boolean;
  /** Stage 4: Optional Space / Room Stage (OFF by default; defaults to 10x10x10 ft padded room). */
  roomEnabled: boolean;
  /** Stage 5: Optional Plugin / Processing Slot before final output (OFF / empty by default). */
  pluginSlot: PluginSlotState;
}

export const DEFAULT_PIPELINE_STAGES: PipelineStagesState = {
  micEnabled: false,
  roomEnabled: false,
  pluginSlot: {
    enabled: false,
    pluginId: 'none',
    eqLowDb: 0.0,
    eqHighDb: 0.0,
    amount: 0.45,
    ceilingDbfs: -1.0,
  },
};

export interface PipelineStageSummary {
  dry_sha256: string;
  output_sha256: string;
  mic_stage_active: boolean;
  room_stage_active: boolean;
  plugin_slot_active: boolean;
  active_plugin: PluginSlotId;
  bypassed_stages: string[];
  bit_identical_to_dry: boolean;
  original_audio_opened: boolean;
}

export interface PipelineRenderResponse {
  runId: string;
  wavUrl: string;
  wav_sha256: string;
  samples: number;
  sample_rate: number;
  summary: PipelineStageSummary;
  spectral: {
    peak_dbfs: number | null;
    rms_dbfs: number | null;
    spectral_centroid_hz: number;
    low_band_pct: number;
    mid_band_pct: number;
    high_band_pct: number;
  };
  acoustic_analysis: RustAcousticAnalysis;
}

export interface DryRegionEditResponse {
  runId: string;
  committed: boolean;
  editedWavUrl: string;
  beforeSliceUrl: string;
  afterSliceUrl: string;
  edited_sha256: string;
  before_slice_sha256: string;
  after_slice_sha256: string;
  start_sample: number;
  end_sample: number;
  crossfade_samples: number;
  affected_notes: number;
  effective_action: string;
  outside_bit_identical: boolean;
  original_audio_opened: boolean;
  sample_rate: number;
  samples: number;
  spectral: {
    peak_dbfs: number | null;
    rms_dbfs: number | null;
    spectral_centroid_hz: number;
    low_band_pct: number;
    mid_band_pct: number;
    high_band_pct: number;
  };
}

export function serializeVoicingForRust(v: VoicingControlsState) {
  return {
    character_target: v.characterTarget,
    signal_path: v.signalPath,
    source_variant: v.sourceVariant,
    straighten_mode: v.straightenMode,
    straighten_preserve_bends: v.straightenPreserveBends,
    profile_guidance_strength: v.profileGuidanceStrength,
  };
}

export function serializePluginSlotForRust(slot: PluginSlotState) {
  return {
    enabled: slot.enabled,
    plugin_id: slot.pluginId,
    eq_low_db: slot.eqLowDb,
    eq_high_db: slot.eqHighDb,
    amount: slot.amount,
    ceiling_dbfs: slot.ceilingDbfs,
  };
}

export interface MicAimState {
  tiltXDeg: number;
  rollYDeg: number;
  headingZDeg: number;
  followSource: boolean;
}

export interface MicSlotState {
  id: string;
  label: string;
  catalogIndex: number;
  patternOverride: PolarPattern | null;
  /** Offset from source origin in metres: [X left/right, Y depth, Z height]. */
  offsetM: Vec3;
  aim: MicAimState;
  enabled: boolean;
}

export interface StudioRigState {
  unit: UnitMode;
  preset: RoomPresetKey;
  /** Internal physical room size in metres: [width X, length/depth Y, height Z]. */
  roomMeters: Vec3;
  /** Individual chain links for [Width X, Length Y, Height Z]. Default: [true, true, false]. */
  dimLinked: [boolean, boolean, boolean];
  /** 6 independent face surface indices: [-X, +X, -Y, +Y, Floor/Ground, Ceiling]. */
  surfaceFaces: [number, number, number, number, number, number];
  temperatureC: number;
  sourceIndex: number;
  /** Offset from default centered-5-ft source origin in metres. Default [0, 0, 0]. */
  sourceOffsetM: Vec3;
  sourceLockCenterXY: boolean;
  sourceLockHeight5ft: boolean;
  /** Visual-only facing direction in degrees (0 = +Y); kept separate from acoustic radiation. */
  sourceFacingDeg: number;
  mics: MicSlotState[];
  selectedMic: number;
  /** Visual-only overlays & plan controls (NEVER alter audio). */
  showPatterns: boolean;
  showGuidanceOverlay: boolean;
  gridSnap: boolean;
}

export interface AxialRoomMode {
  axis: string;
  axis_key: string;
  dimension_m: number;
  harmonic: number;
  freq_hz: number;
  wavelength_m: number;
}

export interface MicAngularAnalysis {
  distance_m: number;
  direct_delay_ms: number;
  inverse_square_pressure_vs_1m: number;
  inverse_square_db_vs_1m: number;
  aim_unit: [number, number, number];
  heading_z_deg: number;
  tilt_x_deg: number;
  roll_y_deg: number;
  follow_source: boolean;
  off_axis_deg: number;
  cos_theta: number;
  pattern_label: string;
  pattern_formula: string;
  polar_gain_linear: number;
  polar_gain_db: number | null;
  source_radiation_label: string;
  source_radiation_gain: number;
  combined_direct_gain: number;
}

export interface MicSlotPlacementRow {
  slot_index: number;
  label: string;
  enabled: boolean;
  source_distance_m: number;
  direct_delay_ms: number;
  distance_from_nearest_mic_m: number;
  ratio_to_nearest_source_dist: number;
  level_diff_vs_nearest_db: number;
  delay_offset_vs_nearest_ms: number;
  meets_three_to_one: boolean | null;
  shortfall_m: number;
  status_label: string;
}

export interface StereoPairGuidance {
  ortf_spacing_m: number;
  ortf_included_angle_deg: number;
  xy_default_angle_deg: number;
  xy_min_angle_deg: number;
  xy_max_angle_deg: number;
  spaced_pair_width_deg: number;
  spaced_pair_suggested_spacing_m: number;
  guidance_note: string;
}

export interface MultiMicPlacementGuidance {
  active_mic_count: number;
  multi_mic_active: boolean;
  nearest_mic_index: number;
  nearest_mic_label: string;
  nearest_source_distance_m: number;
  three_to_one_min_distance_m: number;
  three_to_one_reference_level_diff_db: number;
  slots: MicSlotPlacementRow[];
  stereo_helpers: StereoPairGuidance;
}

export interface FaceAbsorptionContribution {
  face_index: number;
  face_label: string;
  active: boolean;
  surface_index: number;
  surface_name: string;
  area_m2: number;
  raw_absorption_6band: number[];
  effective_absorption_6band: number[];
  pressure_reflectance_6band: number[];
  mean_effective_alpha: number;
  sabins_6band_m2: number[];
  mean_sabins_m2: number;
  share_of_total_absorption_pct: number;
}

export interface RustAcousticAnalysis {
  speed_of_sound_mps: number;
  temperature_c: number;
  room_preset: RoomPresetKey;
  is_enclosed_room: boolean;
  dimensions_m: [number, number, number];
  volume_m3: number;
  total_surface_area_m2: number;
  bands_hz: number[];
  total_absorption_sabins_6band_m2: number[];
  mean_total_absorption_sabins_m2: number;
  sabine_rt60_6band_s: (number | null)[];
  sabine_rt60_mid_s: number | null;
  sabine_rt60_overall_s: number | null;
  axial_modes: AxialRoomMode[];
  face_contributions: FaceAbsorptionContribution[];
  direct_path: MicAngularAnalysis;
  placement_guidance: MultiMicPlacementGuidance;
  estimate_disclaimer: string;
}

export interface RustCaptureResult {
  runId: string;
  wavUrl: string;
  sha256: string;
  samples: number;
  sample_rate: number;
  peak_dbfs: number | null;
  rms_dbfs: number | null;
  speed_of_sound_mps: number;
  distance_m: number;
  direct_delay_ms: number;
  direct_pickup_gain: number;
  first_reflection_ms: number | null;
  last_path_ms: number;
  path_count: number;
  six_band_drr_db: (number | null)[];
  source_pos_m: [number, number, number];
  active_mic_pattern: PolarPattern;
  active_mic_pattern_label: string;
  room_preset: RoomPresetKey;
  effective_faces: {
    face: string;
    active: boolean;
    surface_index: number;
    surface_name: string;
    raw_absorption_6band: number[];
    effective_absorption_6band: number[];
    pressure_reflectance_6band: number[];
  }[];
  acoustic_analysis?: RustAcousticAnalysis;
}

export interface HistoryStack<T> {
  past: T[];
  present: T;
  future: T[];
}

export const FEET_TO_METERS = 0.3048;

export const METERS_PER_UNIT: Record<UnitMode, number> = {
  mm: 0.001,
  cm: 0.01,
  m: 1.0,
  in: 0.0254,
  ft: FEET_TO_METERS,
  yd: 0.9144,
};

export function metersToDisplay(meters: number, unit: UnitMode): number {
  return meters / METERS_PER_UNIT[unit];
}

export function displayToMeters(value: number, unit: UnitMode): number {
  return value * METERS_PER_UNIT[unit];
}

export function unitDecimals(unit: UnitMode): number {
  switch (unit) {
    case 'mm':
      return 0;
    case 'cm':
    case 'in':
      return 1;
    case 'm':
    case 'ft':
    case 'yd':
      return 2;
  }
}

export function niceGridStepM(unit: UnitMode, visibleSpanM: number): { stepM: number; stepUnit: number } {
  const spanU = metersToDisplay(Math.max(0.1, visibleSpanM), unit);
  const raw = spanU / 10;
  const mag = Math.pow(10, Math.floor(Math.log10(Math.max(1e-4, raw))));
  const norm = raw / mag;
  const stepUnit = norm <= 1.25 ? 1 * mag : norm <= 2.5 ? 2 * mag : norm <= 6 ? 5 * mag : 10 * mag;
  return { stepM: displayToMeters(stepUnit, unit), stepUnit };
}

export function speedOfSoundMps(tempC: number): number {
  const t = Math.max(-40, Math.min(60, Number.isFinite(tempC) ? tempC : 20));
  return 343.0 * Math.sqrt((t + 273.15) / 293.15);
}

export function effectiveAbsorption(alpha: number): number {
  return Number.isFinite(alpha) ? Math.max(0, Math.min(1, alpha)) : 1;
}

export function passiveReflectance(alpha: number): number {
  return Math.sqrt(1 - effectiveAbsorption(alpha));
}

export function normalizeVec3(v: Vec3): Vec3 {
  const len = Math.hypot(v.x, v.y, v.z);
  if (!Number.isFinite(len) || len < 1e-8) {
    return { x: 0, y: -1, z: 0 };
  }
  return { x: v.x / len, y: v.y / len, z: v.z / len };
}

export function polarGain(pattern: PolarPattern, cosTheta: number): number {
  const c = Math.max(-1, Math.min(1, cosTheta));
  switch (pattern) {
    case 'omni':
      return 1.0;
    case 'wide_cardioid':
      return 0.7 + 0.3 * c;
    case 'cardioid':
      return 0.5 + 0.5 * c;
    case 'supercardioid':
      return 0.37 + 0.63 * c;
    case 'hypercardioid':
      return 0.25 + 0.75 * c;
    case 'figure8':
      return c;
  }
}

/** Exact match to Rust `studio_catalog::MicAim::direction()` (heading 0° = -Y toward source). */
export function aimDirectionFromAngles(aim: MicAimState): Vec3 {
  const h = (aim.headingZDeg * Math.PI) / 180;
  const t = (aim.tiltXDeg * Math.PI) / 180;
  const ct = Math.cos(t);
  return normalizeVec3({
    x: Math.sin(h) * ct,
    y: -Math.cos(h) * ct,
    z: Math.sin(t),
  });
}

/** Exact match to Rust `studio_catalog::MicAim::point_at_source(offset)`. */
export function anglesPointingAtSource(offsetM: Vec3): { headingZDeg: number; tiltXDeg: number } {
  const vx = -offsetM.x;
  const vy = -offsetM.y;
  const vz = -offsetM.z;
  const horiz = Math.hypot(vx, vy);
  const headingZDeg = horiz > 1e-6 ? (Math.atan2(vx, -vy) * 180) / Math.PI : 0;
  const tiltXDeg = (Math.atan2(vz, Math.max(1e-6, horiz)) * 180) / Math.PI;
  return { headingZDeg, tiltXDeg };
}

export function gridDirectionAngles(dir: GridDirectionKey): {
  tiltXDeg: number;
  rollYDeg: number;
  headingZDeg: number;
} {
  switch (dir) {
    case '+X':
      return { tiltXDeg: 0, rollYDeg: 0, headingZDeg: 90 };
    case '-X':
      return { tiltXDeg: 0, rollYDeg: 0, headingZDeg: -90 };
    case '+Y':
      return { tiltXDeg: 0, rollYDeg: 0, headingZDeg: 180 };
    case '-Y':
      return { tiltXDeg: 0, rollYDeg: 0, headingZDeg: 0 };
    case '+Z':
      return { tiltXDeg: 90, rollYDeg: 0, headingZDeg: 0 };
    case '-Z':
      return { tiltXDeg: -90, rollYDeg: 0, headingZDeg: 0 };
  }
}

/**
 * On release of rotation ball, if the aim vector is within `thresholdDeg` (default 8°)
 * of any of the 6 cardinal axes, snap to that axis and report which axis snapped.
 */
export function snapAnglesToCardinalWithinThreshold(
  aim: MicAimState,
  thresholdDeg = 8,
): { snapped: boolean; axis: GridDirectionKey | null; aim: MicAimState } {
  const dir = aimDirectionFromAngles(aim);
  const cosMin = Math.cos((thresholdDeg * Math.PI) / 180);
  const candidates: { axis: GridDirectionKey; v: Vec3 }[] = [
    { axis: '+X', v: { x: 1, y: 0, z: 0 } },
    { axis: '-X', v: { x: -1, y: 0, z: 0 } },
    { axis: '+Y', v: { x: 0, y: 1, z: 0 } },
    { axis: '-Y', v: { x: 0, y: -1, z: 0 } },
    { axis: '+Z', v: { x: 0, y: 0, z: 1 } },
    { axis: '-Z', v: { x: 0, y: 0, z: -1 } },
  ];
  let bestAxis: GridDirectionKey | null = null;
  let bestDot = cosMin;
  for (const c of candidates) {
    const dot = dir.x * c.v.x + dir.y * c.v.y + dir.z * c.v.z;
    if (dot >= bestDot) {
      bestDot = dot;
      bestAxis = c.axis;
    }
  }
  if (!bestAxis) {
    return { snapped: false, axis: null, aim };
  }
  const snappedAngles = gridDirectionAngles(bestAxis);
  return {
    snapped: true,
    axis: bestAxis,
    aim: {
      ...aim,
      tiltXDeg: snappedAngles.tiltXDeg,
      headingZDeg: snappedAngles.headingZDeg,
      followSource: false,
    },
  };
}

export function getSourcePosM(rig: StudioRigState): Vec3 {
  const { x: w, y: d, z: h } = rig.roomMeters;
  const baseZ = Math.min(5 * FEET_TO_METERS, Math.max(0.25, h - 0.25));
  const ox = rig.sourceLockCenterXY ? 0 : rig.sourceOffsetM.x;
  const oy = rig.sourceLockCenterXY ? 0 : rig.sourceOffsetM.y;
  const oz = rig.sourceLockHeight5ft ? 0 : rig.sourceOffsetM.z;
  return {
    x: Math.max(0.15, Math.min(Math.max(0.15, w - 0.15), w * 0.5 + ox)),
    y: Math.max(0.15, Math.min(Math.max(0.15, d - 0.15), d * 0.5 + oy)),
    z: Math.max(0.15, Math.min(Math.max(0.15, h - 0.15), baseZ + oz)),
  };
}

export function getMicWorldPosM(rig: StudioRigState, micIdx = rig.selectedMic): Vec3 {
  const src = getSourcePosM(rig);
  const slot = rig.mics[Math.min(micIdx, Math.max(0, rig.mics.length - 1))];
  if (!slot) return { x: src.x, y: src.y + 2 * FEET_TO_METERS, z: src.z };
  return {
    x: src.x + slot.offsetM.x,
    y: src.y + slot.offsetM.y,
    z: src.z + slot.offsetM.z,
  };
}

export function clampRig(rig: StudioRigState): StudioRigState {
  const roomMeters: Vec3 = {
    x: Math.max(1.5, Math.min(40, Number.isFinite(rig.roomMeters.x) ? rig.roomMeters.x : 3.048)),
    y: Math.max(1.5, Math.min(40, Number.isFinite(rig.roomMeters.y) ? rig.roomMeters.y : 3.048)),
    z: Math.max(1.5, Math.min(40, Number.isFinite(rig.roomMeters.z) ? rig.roomMeters.z : 3.048)),
  };
  const nextBase: StudioRigState = { ...rig, roomMeters };
  const src = getSourcePosM(nextBase);
  const mics = nextBase.mics.map((m) => {
    let ox = Math.max(0.1 - src.x, Math.min(roomMeters.x - src.x - 0.1, m.offsetM.x));
    let oy = Math.max(0.1 - src.y, Math.min(roomMeters.y - src.y - 0.1, m.offsetM.y));
    let oz = Math.max(0.1 - src.z, Math.min(roomMeters.z - src.z - 0.1, m.offsetM.z));
    if (Math.hypot(ox, oy, oz) < 0.2) {
      ox = 0;
      oy = 0.2;
      oz = 0;
    }
    const offsetM = { x: ox, y: oy, z: oz };
    let aim = m.aim;
    if (aim.followSource) {
      const p = anglesPointingAtSource(offsetM);
      aim = { ...aim, headingZDeg: p.headingZDeg, tiltXDeg: p.tiltXDeg };
    }
    return { ...m, offsetM, aim };
  });
  return {
    ...nextBase,
    mics,
    selectedMic: Math.min(nextBase.selectedMic, Math.max(0, mics.length - 1)),
  };
}

/**
 * Move the source to absolute world coordinates WITHOUT moving any microphones in world coordinates.
 * Any mic with `followSource` enabled automatically re-aims toward the new source position.
 */
export function setSourcePosM(rig: StudioRigState, target: Vec3): StudioRigState {
  const oldSrc = getSourcePosM(rig);
  const { x: w, y: d, z: h } = rig.roomMeters;
  const baseZ = Math.min(5 * FEET_TO_METERS, Math.max(0.25, h - 0.25));
  const nx = rig.sourceLockCenterXY
    ? w * 0.5
    : Math.max(0.15, Math.min(Math.max(0.15, w - 0.15), target.x));
  const ny = rig.sourceLockCenterXY
    ? d * 0.5
    : Math.max(0.15, Math.min(Math.max(0.15, d - 0.15), target.y));
  const nz = rig.sourceLockHeight5ft
    ? baseZ
    : Math.max(0.15, Math.min(Math.max(0.15, h - 0.15), target.z));
  const nextOffset: Vec3 = { x: nx - w * 0.5, y: ny - d * 0.5, z: nz - baseZ };
  const dx = nx - oldSrc.x;
  const dy = ny - oldSrc.y;
  const dz = nz - oldSrc.z;
  const mics = rig.mics.map((m) => ({
    ...m,
    offsetM: { x: m.offsetM.x - dx, y: m.offsetM.y - dy, z: m.offsetM.z - dz },
  }));
  return clampRig({ ...rig, sourceOffsetM: nextOffset, mics });
}

export function resetSourceToDefault(rig: StudioRigState): StudioRigState {
  const { x: w, y: d, z: h } = rig.roomMeters;
  const baseZ = Math.min(5 * FEET_TO_METERS, Math.max(0.25, h - 0.25));
  const unlocked = { ...rig, sourceLockCenterXY: false, sourceLockHeight5ft: false };
  const moved = setSourcePosM(unlocked, { x: w * 0.5, y: d * 0.5, z: baseZ });
  return {
    ...moved,
    sourceLockCenterXY: rig.sourceLockCenterXY,
    sourceLockHeight5ft: rig.sourceLockHeight5ft,
  };
}

export function setMicWorldPosM(
  rig: StudioRigState,
  micIdx: number,
  worldPos: Vec3,
): StudioRigState {
  const src = getSourcePosM(rig);
  const mics = rig.mics.map((m, i) =>
    i === micIdx
      ? {
          ...m,
          offsetM: {
            x: worldPos.x - src.x,
            y: worldPos.y - src.y,
            z: worldPos.z - src.z,
          },
        }
      : m,
  );
  return clampRig({ ...rig, mics });
}

/**
 * Swap a microphone slot's catalog model WITHOUT moving its position or resetting its aim.
 */
export function setMicCatalogIndex(
  rig: StudioRigState,
  micIdx: number,
  catalogIndex: number,
): StudioRigState {
  if (micIdx < 0 || micIdx >= rig.mics.length || !Number.isFinite(catalogIndex)) return rig;
  const safeIdx = Math.max(0, Math.floor(catalogIndex));
  return {
    ...rig,
    mics: rig.mics.map((m, i) => (i === micIdx ? { ...m, catalogIndex: safeIdx } : m)),
  };
}

export function setLinkedDimensionM(
  rig: StudioRigState,
  axis: 'x' | 'y' | 'z',
  newMeters: number,
): StudioRigState {
  if (!Number.isFinite(newMeters)) return rig;
  const axes: ('x' | 'y' | 'z')[] = ['x', 'y', 'z'];
  const idx = axes.indexOf(axis);
  const clamped = Math.max(1.5, Math.min(40, newMeters));
  const old = Math.max(0.1, rig.roomMeters[axis]);
  const ratio = clamped / old;
  const nextDims = { ...rig.roomMeters, [axis]: clamped };
  if (rig.dimLinked[idx]) {
    axes.forEach((other, oi) => {
      if (oi !== idx && rig.dimLinked[oi]) {
        nextDims[other] = Math.max(1.5, Math.min(40, rig.roomMeters[other] * ratio));
      }
    });
  }
  return clampRig({ ...rig, roomMeters: nextDims });
}

export function makeDefaultMicSlot(index = 0): MicSlotState {
  return {
    id: `mic-${index + 1}`,
    label: index === 0 ? 'Mic 1 (SM7B)' : `Mic ${index + 1}`,
    catalogIndex: index === 0 ? 0 : 10,
    patternOverride: null,
    offsetM:
      index === 0
        ? { x: 0, y: 2 * FEET_TO_METERS, z: 0 }
        : { x: index % 2 === 1 ? 0.45 : -0.45, y: 3 * FEET_TO_METERS, z: 0.1 },
    aim: { tiltXDeg: 0, rollYDeg: 0, headingZDeg: 0, followSource: false },
    enabled: true,
  };
}

export function makeRoomPreset(
  preset: RoomPresetKey,
  prev?: StudioRigState,
): StudioRigState {
  const baseDims: Record<RoomPresetKey, Vec3> = {
    small: { x: 10 * FEET_TO_METERS, y: 10 * FEET_TO_METERS, z: 10 * FEET_TO_METERS },
    medium: { x: 20 * FEET_TO_METERS, y: 20 * FEET_TO_METERS, z: 10 * FEET_TO_METERS },
    large: { x: 50 * FEET_TO_METERS, y: 50 * FEET_TO_METERS, z: 10 * FEET_TO_METERS },
    open: { x: 50 * FEET_TO_METERS, y: 50 * FEET_TO_METERS, z: 50 * FEET_TO_METERS },
    direct: { x: 10 * FEET_TO_METERS, y: 10 * FEET_TO_METERS, z: 10 * FEET_TO_METERS },
  };
  const baseFaces: Record<RoomPresetKey, [number, number, number, number, number, number]> = {
    small: [1, 1, 1, 1, 1, 1],
    medium: [14, 14, 14, 14, 15, 6],
    large: [18, 18, 18, 18, 15, 18],
    open: [0, 0, 0, 0, 29, 0],
    direct: [0, 0, 0, 0, 0, 0],
  };

  const base: StudioRigState = prev
    ? {
        ...prev,
        preset,
        roomMeters: baseDims[preset],
        surfaceFaces: [...baseFaces[preset]],
      }
    : {
        unit: 'ft',
        preset,
        roomMeters: baseDims[preset],
        dimLinked: [true, true, false],
        surfaceFaces: [...baseFaces[preset]],
        temperatureC: 20,
        sourceIndex: 2, // Electric Guitar (Clean 1x12 Open-Back Combo) in Rust 138-source catalog
        sourceOffsetM: { x: 0, y: 0, z: 0 },
        sourceLockCenterXY: false,
        sourceLockHeight5ft: false,
        sourceFacingDeg: 0,
        mics: [makeDefaultMicSlot(0)],
        selectedMic: 0,
        showPatterns: false,
        showGuidanceOverlay: false,
        gridSnap: false,
      };
  return clampRig(base);
}

/**
 * Pure acoustic key that excludes all visual-only state (`unit`, `showPatterns`, `gridSnap`, `sourceFacingDeg`).
 * If two rigs have the same fingerprint, Rust `studio-capture` produces bit-identical audio!
 */
export function computeAcousticFingerprint(rig: StudioRigState): string {
  const q = (v: number) => Math.round(v * 10_000);
  return JSON.stringify({
    p: rig.preset,
    d: [q(rig.roomMeters.x), q(rig.roomMeters.y), q(rig.roomMeters.z)],
    f: rig.surfaceFaces,
    t: q(rig.temperatureC),
    si: rig.sourceIndex,
    so: [q(rig.sourceOffsetM.x), q(rig.sourceOffsetM.y), q(rig.sourceOffsetM.z)],
    sl: [rig.sourceLockCenterXY, rig.sourceLockHeight5ft],
    sm: rig.selectedMic,
    m: rig.mics.map((m) => ({
      c: m.catalogIndex,
      po: m.patternOverride,
      o: [q(m.offsetM.x), q(m.offsetM.y), q(m.offsetM.z)],
      a: [q(m.aim.tiltXDeg), q(m.aim.rollYDeg), q(m.aim.headingZDeg), m.aim.followSource],
      e: m.enabled,
    })),
  });
}

export function toRustCaptureConfig(rig: StudioRigState) {
  return {
    room_preset: rig.preset,
    dimensions_m: [rig.roomMeters.x, rig.roomMeters.y, rig.roomMeters.z],
    source_index: rig.sourceIndex,
    source_offset_m: [rig.sourceOffsetM.x, rig.sourceOffsetM.y, rig.sourceOffsetM.z],
    source_lock_center_xy: rig.sourceLockCenterXY,
    source_lock_height_5ft: rig.sourceLockHeight5ft,
    surface_faces: rig.surfaceFaces,
    temperature_c: rig.temperatureC,
    selected_mic: rig.selectedMic,
    max_order: 2,
    mics: rig.mics.map((m) => ({
      label: m.label,
      catalog_index: m.catalogIndex,
      pattern_override: m.patternOverride,
      offset_m: [m.offsetM.x, m.offsetM.y, m.offsetM.z],
      aim: {
        tilt_x_deg: m.aim.tiltXDeg,
        roll_y_deg: m.aim.rollYDeg,
        heading_z_deg: m.aim.headingZDeg,
        follow_source: m.aim.followSource,
      },
      enabled: m.enabled,
    })),
  };
}

export function computeNyquistDisplay(
  sampleRate: number,
  displayMaxHz: number,
): { nyquistHz: number; hasAboveNyquist: boolean; nyquistFractionFromTop: number } {
  const nyquistHz = Math.max(1000, sampleRate / 2);
  const clampedMax = Math.max(1000, displayMaxHz);
  const hasAboveNyquist = clampedMax > nyquistHz + 1;
  const nyquistFractionFromTop = hasAboveNyquist ? (clampedMax - nyquistHz) / clampedMax : 0;
  return { nyquistHz, hasAboveNyquist, nyquistFractionFromTop };
}

export function createHistory<T>(initial: T): HistoryStack<T> {
  return { past: [], present: initial, future: [] };
}

export function pushHistory<T>(stack: HistoryStack<T>, next: T, maxDepth = 64): HistoryStack<T> {
  const past = [...stack.past, stack.present].slice(-maxDepth);
  return { past, present: next, future: [] };
}

export function undoHistory<T>(stack: HistoryStack<T>): HistoryStack<T> {
  if (stack.past.length === 0) return stack;
  const prev = stack.past[stack.past.length - 1];
  return {
    past: stack.past.slice(0, -1),
    present: prev,
    future: [stack.present, ...stack.future],
  };
}

export function redoHistory<T>(stack: HistoryStack<T>): HistoryStack<T> {
  if (stack.future.length === 0) return stack;
  const next = stack.future[0];
  return {
    past: [...stack.past, stack.present],
    present: next,
    future: stack.future.slice(1),
  };
}

export type StereoPresetKind = 'ortf' | 'xy' | 'spaced';

/**
 * Quick-apply a standard stereo pair preset (ORTF, X/Y, or Spaced Pair) to Mic 1 and Mic 2
 * WITHOUT modifying room dimensions, materials, source position, or catalog mic models.
 * Moves mics and sets aim -> legitimately updates the geometric capture.
 */
export function applyStereoPreset(
  rig: StudioRigState,
  kind: StereoPresetKind,
  desiredWidthDeg = 60,
): StudioRigState {
  const mics = rig.mics.map((m) => ({ ...m, offsetM: { ...m.offsetM }, aim: { ...m.aim } }));
  while (mics.length < 2) {
    mics.push(makeDefaultMicSlot(mics.length));
  }
  const refY = Math.max(2 * FEET_TO_METERS, Math.abs(mics[0].offsetM.y || 2 * FEET_TO_METERS));
  const refZ = mics[0].offsetM.z;

  if (kind === 'ortf') {
    // ORTF: 17 cm spacing, 110° included angle (±55°), cardioid capsules
    const half = 0.17 / 2;
    mics[0] = {
      ...mics[0],
      enabled: true,
      patternOverride: 'cardioid',
      offsetM: { x: -half, y: refY, z: refZ },
      aim: { tiltXDeg: 0, rollYDeg: 0, headingZDeg: -55, followSource: false },
    };
    mics[1] = {
      ...mics[1],
      enabled: true,
      patternOverride: 'cardioid',
      offsetM: { x: half, y: refY, z: refZ },
      aim: { tiltXDeg: 0, rollYDeg: 0, headingZDeg: 55, followSource: false },
    };
  } else if (kind === 'xy') {
    // X/Y: coincident capsules, 90°–135° included angle (default 90° => ±45°)
    const angle = Math.max(90, Math.min(135, desiredWidthDeg >= 90 ? desiredWidthDeg : 90));
    const halfAngle = angle / 2;
    mics[0] = {
      ...mics[0],
      enabled: true,
      patternOverride: 'cardioid',
      offsetM: { x: 0, y: refY, z: refZ },
      aim: { tiltXDeg: 0, rollYDeg: 0, headingZDeg: -halfAngle, followSource: false },
    };
    mics[1] = {
      ...mics[1],
      enabled: true,
      patternOverride: 'cardioid',
      offsetM: { x: 0, y: refY, z: refZ + 0.02 },
      aim: { tiltXDeg: 0, rollYDeg: 0, headingZDeg: halfAngle, followSource: false },
    };
  } else {
    // Spaced pair: spacing ≈ source_distance * tan(width / 2), capped [0.15, 3.0] m
    const wDeg = Math.max(20, Math.min(120, desiredWidthDeg));
    const spacing = Math.max(0.15, Math.min(3.0, refY * Math.tan(((wDeg * 0.5) * Math.PI) / 180)));
    const half = spacing / 2;
    mics[0] = {
      ...mics[0],
      enabled: true,
      patternOverride: 'omni',
      offsetM: { x: -half, y: refY, z: refZ },
      aim: { tiltXDeg: 0, rollYDeg: 0, headingZDeg: 0, followSource: false },
    };
    mics[1] = {
      ...mics[1],
      enabled: true,
      patternOverride: 'omni',
      offsetM: { x: half, y: refY, z: refZ },
      aim: { tiltXDeg: 0, rollYDeg: 0, headingZDeg: 0, followSource: false },
    };
  }

  return clampRig({
    ...rig,
    mics,
    selectedMic: 0,
  });
}

const PATTERN_FORMULAS: Record<PolarPattern, string> = {
  omni: '1.0 (omnidirectional)',
  wide_cardioid: '0.70 + 0.30 · cos(θ)',
  cardioid: '0.50 + 0.50 · cos(θ)',
  supercardioid: '0.37 + 0.63 · cos(θ)',
  hypercardioid: '0.25 + 0.75 · cos(θ)',
  figure8: 'cos(θ) (bidirectional)',
};

export const PATTERN_LABELS: Record<PolarPattern, string> = {
  omni: 'Omni',
  wide_cardioid: 'Wide Cardioid',
  cardioid: 'Cardioid',
  supercardioid: 'Supercardioid',
  hypercardioid: 'Hypercardioid',
  figure8: 'Figure-8',
};

export const FACE_LABELS = [
  '-X Left wall',
  '+X Right wall',
  '-Y Front wall',
  '+Y Back wall',
  'Floor / Ground',
  'Ceiling',
] as const;

/**
 * Synchronous SI acoustic math mirror of Rust `acoustic_math::analyze_studio_rig`.
 * Provides instant zero-flicker readouts before the live Rust `/api/studio/analyze` response arrives.
 */
export function computeAcousticAnalysisLocal(
  rig: StudioRigState,
  sources: RustSourceEntry[],
  mics: RustMicEntry[],
  surfaces: RustSurfaceEntry[],
): RustAcousticAnalysis {
  const r = clampRig(rig);
  const c = speedOfSoundMps(r.temperatureC);
  const { x: w, y: d, z: h } = r.roomMeters;
  const volume_m3 = w * d * h;
  const faceAreas = [d * h, d * h, w * h, w * h, w * d, w * d];
  const total_surface_area_m2 = faceAreas.reduce((a, b) => a + b, 0);
  const is_enclosed_room = r.preset === 'small' || r.preset === 'medium' || r.preset === 'large';

  const totalSabins6 = [0, 0, 0, 0, 0, 0];
  const face_contributions: FaceAbsorptionContribution[] = [];

  for (let f = 0; f < 6; f++) {
    const active = r.preset === 'direct' ? false : r.preset === 'open' ? f === 4 : true;
    const surfIdx = Math.min(r.surfaceFaces[f], Math.max(0, surfaces.length - 1));
    const surf = surfaces[surfIdx];
    const raw6 = surf ? surf.raw_absorption_6band : [1, 1, 1, 1, 1, 1];
    const eff6 = raw6.map(effectiveAbsorption);
    const refl6 = eff6.map((a) => Math.sqrt(1 - a));
    const area = faceAreas[f];
    const sabins6 = active ? eff6.map((a) => a * area) : [0, 0, 0, 0, 0, 0];
    for (let b = 0; b < 6; b++) totalSabins6[b] += sabins6[b];
    const meanEff = eff6.reduce((a, b) => a + b, 0) / 6;
    const meanSab = sabins6.reduce((a, b) => a + b, 0) / 6;
    face_contributions.push({
      face_index: f,
      face_label: FACE_LABELS[f],
      active,
      surface_index: surfIdx,
      surface_name: surf?.name ?? 'Unknown',
      area_m2: area,
      raw_absorption_6band: raw6,
      effective_absorption_6band: eff6,
      pressure_reflectance_6band: refl6,
      mean_effective_alpha: meanEff,
      sabins_6band_m2: sabins6,
      mean_sabins_m2: meanSab,
      share_of_total_absorption_pct: 0,
    });
  }

  const meanTotalSabins = totalSabins6.reduce((a, b) => a + b, 0) / 6;
  if (meanTotalSabins > 1e-9) {
    for (const fc of face_contributions) {
      fc.share_of_total_absorption_pct = (fc.mean_sabins_m2 / meanTotalSabins) * 100;
    }
  }

  const rt60Fn = (a: number) => (is_enclosed_room && a > 1e-6 ? (0.161 * volume_m3) / a : null);
  const sabine_rt60_6band_s = totalSabins6.map(rt60Fn);
  const sabine_rt60_mid_s =
    sabine_rt60_6band_s[2] !== null && sabine_rt60_6band_s[3] !== null
      ? 0.5 * (sabine_rt60_6band_s[2] + sabine_rt60_6band_s[3])
      : sabine_rt60_6band_s[2] ?? sabine_rt60_6band_s[3];
  const sabine_rt60_overall_s = rt60Fn(meanTotalSabins);

  const axial_modes: AxialRoomMode[] = [];
  if (is_enclosed_room) {
    const axes: [string, string, number][] = [
      ['Width (X)', 'X', w],
      ['Length (Y)', 'Y', d],
      ['Height (Z)', 'Z', h],
    ];
    for (const [axis, axis_key, len] of axes) {
      for (let n = 1; n <= 4; n++) {
        axial_modes.push({
          axis,
          axis_key,
          dimension_m: len,
          harmonic: n,
          freq_hz: (n * c) / (2 * len),
          wavelength_m: (2 * len) / n,
        });
      }
    }
    axial_modes.sort((a, b) => a.freq_hz - b.freq_hz);
    axial_modes.splice(12);
  }

  const slot = r.mics[Math.min(r.selectedMic, r.mics.length - 1)] ?? r.mics[0];
  const dist = Math.max(0.2, Math.hypot(slot.offsetM.x, slot.offsetM.y, slot.offsetM.z));
  const toSrc = { x: -slot.offsetM.x / dist, y: -slot.offsetM.y / dist, z: -slot.offsetM.z / dist };
  const aim = aimDirectionFromAngles(slot.aim);
  const cosTheta = Math.max(-1, Math.min(1, toSrc.x * aim.x + toSrc.y * aim.y + toSrc.z * aim.z));
  const offAxisDeg = (Math.acos(cosTheta) * 180) / Math.PI;
  const catMic = mics[slot.catalogIndex];
  const pat: PolarPattern = slot.patternOverride ?? catMic?.default_pattern ?? 'cardioid';
  const pGain = polarGain(pat, cosTheta);
  const pGainDb = Math.abs(pGain) > 1e-12 ? 20 * Math.log10(Math.abs(pGain)) : null;
  const srcEntry = sources[r.sourceIndex];
  const srcAlpha = srcEntry?.radiation_alpha ?? 1.0;
  const srcRadGain = srcAlpha + (1 - srcAlpha) * (slot.offsetM.y / dist);

  const slotDists = r.mics.map((m) => Math.max(0.2, Math.hypot(m.offsetM.x, m.offsetM.y, m.offsetM.z)));
  const activeIndices = r.mics.map((m, i) => (m.enabled ? i : -1)).filter((i) => i >= 0);
  const activeCount = activeIndices.length;
  const multiMicActive = activeCount >= 2;
  let nearestIdx = activeIndices[0] ?? 0;
  for (const idx of activeIndices) {
    if (slotDists[idx] < slotDists[nearestIdx]) nearestIdx = idx;
  }
  const nearestDist = slotDists[nearestIdx] ?? dist;
  const nearestOff = r.mics[nearestIdx]?.offsetM ?? { x: 0, y: nearestDist, z: 0 };
  const min3to1 = nearestDist * 3.0;
  const nearestDelayMs = (nearestDist / c) * 1000;

  const placementSlots: MicSlotPlacementRow[] = r.mics.map((m, idx) => {
    const dSrc = slotDists[idx];
    const delayMs = (dSrc / c) * 1000;
    const dBetween = Math.hypot(
      m.offsetM.x - nearestOff.x,
      m.offsetM.y - nearestOff.y,
      m.offsetM.z - nearestOff.z,
    );
    const ratio = dSrc / Math.max(1e-6, nearestDist);
    const lvlDb = 20 * Math.log10(dSrc / Math.max(1e-6, nearestDist));
    let meets: boolean | null = null;
    let shortfall = 0;
    let statusLabel = 'Nearest Reference';
    if (!m.enabled) {
      statusLabel = 'Muted';
    } else if (multiMicActive && idx !== nearestIdx) {
      if (dSrc + 1e-4 >= min3to1) {
        meets = true;
        statusLabel = 'Meets 3:1';
      } else {
        meets = false;
        shortfall = Math.max(0, min3to1 - dSrc);
        statusLabel = 'Shortfall (< 3:1)';
      }
    }
    return {
      slot_index: idx,
      label: m.label,
      enabled: m.enabled,
      source_distance_m: dSrc,
      direct_delay_ms: delayMs,
      distance_from_nearest_mic_m: dBetween,
      ratio_to_nearest_source_dist: ratio,
      level_diff_vs_nearest_db: lvlDb,
      delay_offset_vs_nearest_ms: delayMs - nearestDelayMs,
      meets_three_to_one: meets,
      shortfall_m: shortfall,
      status_label: statusLabel,
    };
  });

  return {
    speed_of_sound_mps: c,
    temperature_c: r.temperatureC,
    room_preset: r.preset,
    is_enclosed_room,
    dimensions_m: [w, d, h],
    volume_m3,
    total_surface_area_m2,
    bands_hz: [125, 250, 500, 1000, 2000, 4000],
    total_absorption_sabins_6band_m2: totalSabins6,
    mean_total_absorption_sabins_m2: meanTotalSabins,
    sabine_rt60_6band_s,
    sabine_rt60_mid_s,
    sabine_rt60_overall_s,
    axial_modes,
    face_contributions,
    direct_path: {
      distance_m: dist,
      direct_delay_ms: (dist / c) * 1000,
      inverse_square_pressure_vs_1m: 1 / dist,
      inverse_square_db_vs_1m: 20 * Math.log10(1 / dist),
      aim_unit: [aim.x, aim.y, aim.z],
      heading_z_deg: slot.aim.headingZDeg,
      tilt_x_deg: slot.aim.tiltXDeg,
      roll_y_deg: slot.aim.rollYDeg,
      follow_source: slot.aim.followSource,
      off_axis_deg: offAxisDeg,
      cos_theta: cosTheta,
      pattern_label: PATTERN_LABELS[pat],
      pattern_formula: PATTERN_FORMULAS[pat],
      polar_gain_linear: pGain,
      polar_gain_db: pGainDb,
      source_radiation_label: srcEntry?.radiation_label ?? 'Omni',
      source_radiation_gain: srcRadGain,
      combined_direct_gain: Math.max(-1, Math.min(1, srcRadGain * pGain)),
    },
    placement_guidance: {
      active_mic_count: activeCount,
      multi_mic_active: multiMicActive,
      nearest_mic_index: nearestIdx,
      nearest_mic_label: r.mics[nearestIdx]?.label ?? 'Mic 1',
      nearest_source_distance_m: nearestDist,
      three_to_one_min_distance_m: min3to1,
      three_to_one_reference_level_diff_db: 20 * Math.log10(3),
      slots: placementSlots,
      stereo_helpers: {
        ortf_spacing_m: 0.17,
        ortf_included_angle_deg: 110,
        xy_default_angle_deg: 90,
        xy_min_angle_deg: 90,
        xy_max_angle_deg: 135,
        spaced_pair_width_deg: 60,
        spaced_pair_suggested_spacing_m: Math.max(
          0.15,
          Math.min(3.0, nearestDist * Math.tan((30 * Math.PI) / 180)),
        ),
        guidance_note:
          'Starting-point placement guidance only — never a hard constraint. The geometric capture engine always renders from actual mic positions, aims, patterns, and room surfaces.',
      },
    },
    estimate_disclaimer:
      'Informational estimates (Sabine RT60, axial room modes, inverse-square, wavelength, 3:1 guidance) derived from room geometry, clamped effective absorption, and mic aim. Rendered capture audio uses geometric 6-band image-source paths — never replaced by a synthetic RT60 tail.',
  };
}

