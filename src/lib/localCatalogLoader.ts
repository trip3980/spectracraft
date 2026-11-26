import fs from 'node:fs';
import path from 'node:path';
import type {
  PolarPattern,
  RoomPresetKey,
  RustMicEntry,
  RustMicOffaxisTable,
  RustRoomPreset,
  RustSourceEntry,
  RustSourcePhysicsConstraint,
  RustStudioCatalogs,
  RustSurfaceBiotParameters,
  RustSurfaceEntry,
} from './studioRig.ts';

const FEET_TO_METRES = 0.3048;

export const STUDIO_BANDS_HZ = [125, 250, 500, 1000, 2000, 4000] as const;

export const STUDIO_FACE_LABELS = [
  '-X Left wall',
  '+X Right wall',
  '-Y Front wall',
  '+Y Back wall',
  'Floor / Ground',
  'Ceiling',
] as const;

const MIC_ARCHIVE_PROVENANCE =
  'Published specs/curves from Mic Archive (reference only — not a measured hardware IR); renderer applies modeled analytical polar + parametric class';

export class CatalogLoadError extends Error {
  readonly statusCode: number;
  constructor(message: string, statusCode = 503) {
    super(message);
    this.name = 'CatalogLoadError';
    this.statusCode = statusCode;
  }
}

type SourceRegenKind =
  | 'neutral'
  | 'plucked_electric'
  | 'plucked_acoustic'
  | 'plucked_bass'
  | 'bowed_low'
  | 'bowed_high'
  | 'woodwind_flute'
  | 'woodwind_reed'
  | 'brass_bright'
  | 'brass_warm'
  | 'voice_male'
  | 'voice_female'
  | 'voice_ensemble'
  | 'keyboard_struck'
  | 'keyboard_organ'
  | 'percussion'
  | 'amp_cabinet';

function parseRegenKind(s: string): SourceRegenKind {
  switch (s) {
    case 'plucked_electric':
      return 'plucked_electric';
    case 'plucked_acoustic':
    case 'string':
      return 'plucked_acoustic';
    case 'plucked_bass':
    case 'bass':
      return 'plucked_bass';
    case 'bowed_low':
    case 'bowed':
      return 'bowed_low';
    case 'bowed_high':
      return 'bowed_high';
    case 'woodwind_flute':
    case 'flute':
      return 'woodwind_flute';
    case 'woodwind_reed':
    case 'wind':
    case 'reed':
      return 'woodwind_reed';
    case 'brass_bright':
    case 'brass':
      return 'brass_bright';
    case 'brass_warm':
      return 'brass_warm';
    case 'voice_male':
      return 'voice_male';
    case 'voice_female':
    case 'voice':
      return 'voice_female';
    case 'voice_ensemble':
      return 'voice_ensemble';
    case 'keyboard_struck':
    case 'keyboard':
      return 'keyboard_struck';
    case 'keyboard_organ':
      return 'keyboard_organ';
    case 'percussion':
    case 'membrane':
      return 'percussion';
    case 'amp_cabinet':
    case 'amp':
      return 'amp_cabinet';
    default:
      return 'neutral';
  }
}

function modelTypeHint(kind: SourceRegenKind): string {
  switch (kind) {
    case 'neutral':
      return 'neutral';
    case 'plucked_electric':
    case 'plucked_acoustic':
    case 'plucked_bass':
      return 'string';
    case 'bowed_low':
    case 'bowed_high':
      return 'bowed';
    case 'woodwind_flute':
    case 'woodwind_reed':
      return 'wind';
    case 'brass_bright':
    case 'brass_warm':
      return 'brass';
    case 'voice_male':
    case 'voice_female':
    case 'voice_ensemble':
      return 'voice';
    case 'keyboard_struck':
    case 'keyboard_organ':
      return 'keyboard';
    case 'percussion':
      return 'membrane';
    case 'amp_cabinet':
      return 'amp';
  }
}

function physicsConstraintFor(kind: SourceRegenKind): RustSourcePhysicsConstraint {
  switch (kind) {
    case 'neutral':
    case 'plucked_electric':
      return {
        attack_ms: 8.0,
        release_ms: 18.0,
        breath_texture_amount: 0.0,
        dynamic_brightness_amount: 0.0,
        harmonic_ceiling_hz: null,
        resonance_contour_summary: 'Neutral cue-calibrated modal string partials (no added body EQ)',
        texture_mechanism: 'Pure cue-driven modal string excitation (no added breath/bow noise)',
        plausibility_note: 'Follows cue mode weights and decay directly; never clamps pitch or bends.',
      };
    case 'plucked_acoustic':
      return {
        attack_ms: 5.0,
        release_ms: 18.0,
        breath_texture_amount: 0.004,
        dynamic_brightness_amount: 0.15,
        harmonic_ceiling_hz: null,
        resonance_contour_summary:
          'Soundboard body +3.2 dB @ 185 Hz, box dip -2.4 dB @ 460 Hz, pick air +3.5 dB @ 3.6 kHz',
        texture_mechanism: 'Subtle pick/string transient friction coupled to reference tonal fraction',
        plausibility_note:
          'Favors acoustic body & pick attack while preserving cue bends and atypical sustain.',
      };
    case 'plucked_bass':
      return {
        attack_ms: 7.0,
        release_ms: 22.0,
        breath_texture_amount: 0.0,
        dynamic_brightness_amount: 0.08,
        harmonic_ceiling_hz: 3800.0,
        resonance_contour_summary:
          'Low fundamental body +4.5 dB @ 115 Hz, scoop -3.5 dB @ 1.4 kHz, lowpass 3.8 kHz',
        texture_mechanism: 'Heavy-string modal decay (no added breath noise)',
        plausibility_note:
          'Suppresses implausible >3.8 kHz fizz while keeping performed pitch/slides exact.',
      };
    case 'bowed_low':
      return {
        attack_ms: 26.0,
        release_ms: 32.0,
        breath_texture_amount: 0.014,
        dynamic_brightness_amount: 0.2,
        harmonic_ceiling_hz: 6200.0,
        resonance_contour_summary:
          'Wood cavity +5.2 dB @ 175 Hz, hollow dip -2.8 dB @ 650 Hz, bridge +2.4 dB @ 2.1 kHz, LP 6.2 kHz',
        texture_mechanism: 'Bow-rosin friction band-limited texture coupled to reference tonal fraction',
        plausibility_note:
          'Bowed onset and cello/viola body contour; follows cue slides, vibrato, and decay.',
      };
    case 'bowed_high':
      return {
        attack_ms: 22.0,
        release_ms: 28.0,
        breath_texture_amount: 0.016,
        dynamic_brightness_amount: 0.22,
        harmonic_ceiling_hz: null,
        resonance_contour_summary:
          'Body +2.6 dB @ 320 Hz, bridge hill +4.8 dB @ 2.8 kHz, bow sheen +2.2 dB @ 6 kHz',
        texture_mechanism: 'Bow-hair friction coupled to reference tonal fraction',
        plausibility_note:
          'Upper bowed-string projection without overriding unusual cue dynamics or pitch.',
      };
    case 'woodwind_flute':
      return {
        attack_ms: 24.0,
        release_ms: 26.0,
        breath_texture_amount: 0.024,
        dynamic_brightness_amount: 0.1,
        harmonic_ceiling_hz: 7500.0,
        resonance_contour_summary:
          'Air-column warmth +2.0 dB @ 480 Hz, embouchure breath +2.8 dB @ 4.8 kHz, LP 7.5 kHz',
        texture_mechanism: 'Air-jet breath aspiration coupled to reference tonal fraction',
        plausibility_note:
          'Smooth air-column partials + breath jet; never forces notes into flute compass.',
      };
    case 'woodwind_reed':
      return {
        attack_ms: 18.0,
        release_ms: 24.0,
        breath_texture_amount: 0.012,
        dynamic_brightness_amount: 0.18,
        harmonic_ceiling_hz: 6500.0,
        resonance_contour_summary:
          'Bore +2.2 dB @ 260 Hz, reed formant +4.6 dB @ 1.65 kHz, lowpass 6.5 kHz',
        texture_mechanism: 'Reed air-column turbulence coupled to reference tonal fraction',
        plausibility_note:
          'Reed formant emphasis with upper rolloff; cue phrasing and pitch remain authoritative.',
      };
    case 'brass_bright':
      return {
        attack_ms: 12.0,
        release_ms: 22.0,
        breath_texture_amount: 0.006,
        dynamic_brightness_amount: 0.35,
        harmonic_ceiling_hz: null,
        resonance_contour_summary:
          'Tight low -1.8 dB @ 240 Hz, bell brilliance +5.2 dB @ 2.2 kHz & +3.0 dB @ 4.2 kHz',
        texture_mechanism: 'Dynamic brass harmonic bloom + subtle lip/air onset',
        plausibility_note:
          'Louder cues open brighter brass partials while preserving exact cue envelope.',
      };
    case 'brass_warm':
      return {
        attack_ms: 18.0,
        release_ms: 26.0,
        breath_texture_amount: 0.005,
        dynamic_brightness_amount: 0.2,
        harmonic_ceiling_hz: 4200.0,
        resonance_contour_summary:
          'Horn warmth +4.0 dB @ 280 Hz, mid +2.0 dB @ 1.3 kHz, smooth bell lowpass 4.2 kHz',
        texture_mechanism: 'Warm brass dynamic harmonic bloom + mild breath onset',
        plausibility_note:
          'Rolls off implausible harsh >4.2 kHz harmonics while keeping cues intact.',
      };
    case 'voice_male':
      return {
        attack_ms: 20.0,
        release_ms: 30.0,
        breath_texture_amount: 0.016,
        dynamic_brightness_amount: 0.12,
        harmonic_ceiling_hz: 8000.0,
        resonance_contour_summary:
          'Chest +3.2 dB @ 160 Hz; vocal formants 500 / 1350 / 2700 Hz; lowpass 8.0 kHz',
        texture_mechanism: 'Glottal breath aspiration coupled to reference tonal fraction',
        plausibility_note:
          'Male vocal formant contour + breath; never forces pitch or erases vocal quirks.',
      };
    case 'voice_female':
      return {
        attack_ms: 22.0,
        release_ms: 30.0,
        breath_texture_amount: 0.018,
        dynamic_brightness_amount: 0.12,
        harmonic_ceiling_hz: null,
        resonance_contour_summary:
          'Low trim -5.5 dB @ 160 Hz; vocal formants 680 / 1650 / 3150 Hz + 6 kHz air',
        texture_mechanism: 'Glottal breath aspiration coupled to reference tonal fraction',
        plausibility_note:
          'Upper vocal formant contour; preserves low/out-of-range cues as performed.',
      };
    case 'voice_ensemble':
      return {
        attack_ms: 24.0,
        release_ms: 34.0,
        breath_texture_amount: 0.015,
        dynamic_brightness_amount: 0.1,
        harmonic_ceiling_hz: 8500.0,
        resonance_contour_summary:
          'Blended choir formants 580 / 1500 / 3000 Hz, smooth air lowpass 8.5 kHz',
        texture_mechanism: 'Soft choral breath texture coupled to reference tonal fraction',
        plausibility_note:
          'Broad ensemble formant contour while strictly following mono performance cues.',
      };
    case 'keyboard_struck':
      return {
        attack_ms: 6.0,
        release_ms: 22.0,
        breath_texture_amount: 0.0,
        dynamic_brightness_amount: 0.18,
        harmonic_ceiling_hz: null,
        resonance_contour_summary:
          'Soundboard +2.6 dB @ 220 Hz, hammer presence +2.8 dB @ 2.5 kHz',
        texture_mechanism: 'Hammer-struck string modal partials (no added breath)',
        plausibility_note:
          'Struck-string voicing that STILL follows cue bends/slides/sustain if performed.',
      };
    case 'keyboard_organ':
      return {
        attack_ms: 10.0,
        release_ms: 18.0,
        breath_texture_amount: 0.004,
        dynamic_brightness_amount: 0.05,
        harmonic_ceiling_hz: 6000.0,
        resonance_contour_summary: 'Pipe/drawbar mid +3.4 dB @ 900 Hz, smooth lowpass 6.0 kHz',
        texture_mechanism: 'Subtle pipe-chiff air texture coupled to reference tonal fraction',
        plausibility_note:
          'Steady organ harmonic contour without overriding cue dynamics or bends.',
      };
    case 'percussion':
      return {
        attack_ms: 3.0,
        release_ms: 16.0,
        breath_texture_amount: 0.012,
        dynamic_brightness_amount: 0.25,
        harmonic_ceiling_hz: null,
        resonance_contour_summary:
          'Shell fundamental +3.2 dB @ 190 Hz, stick transient +4.2 dB @ 3.6 kHz',
        texture_mechanism: 'Fast stick/mallet transient texture coupled to reference tonal fraction',
        plausibility_note:
          'Crisp strike voicing while still following cue pitch and atypical sustain envelopes.',
      };
    case 'amp_cabinet':
      return {
        attack_ms: 7.0,
        release_ms: 18.0,
        breath_texture_amount: 0.0,
        dynamic_brightness_amount: 0.16,
        harmonic_ceiling_hz: 4700.0,
        resonance_contour_summary:
          'Cabinet thump +3.2 dB @ 130 Hz, speaker midrange +4.4 dB @ 1.75 kHz, cone rolloff 4.7 kHz',
        texture_mechanism: 'Electric string + guitar-speaker cone filtering (no added breath)',
        plausibility_note:
          'Suppresses implausible >4.7 kHz fizz typical of raw DI while keeping cue expression.',
      };
  }
}

function parseSourceCategory(s: string | undefined): RustSourceEntry['category'] {
  if (s === 'Instrument') return 'Instrument';
  if (s === 'Human') return 'Human';
  return 'Unknown';
}

function parseSourceSymbol(s: string | undefined): RustSourceEntry['symbol'] {
  switch (s) {
    case 'guitar':
    case 'amp':
    case 'bowed':
    case 'wind':
    case 'brass':
    case 'percussion':
    case 'keyboard':
    case 'voice':
    case 'ensemble':
      return s;
    default:
      return 'unknown';
  }
}

function parseSourceRadiation(s: string | undefined): { label: string; alpha: number } {
  switch (s) {
    case 'forward_wide':
      return { label: 'Forward-biased wide', alpha: 0.22 };
    case 'forward_cardioid':
      return { label: 'Forward cardioid-like', alpha: 0.42 };
    case 'dipole_open_back':
      return { label: 'Dipole / open-back', alpha: 0.7 };
    case 'directional_bell':
      return { label: 'Directional forward bell', alpha: 0.58 };
    default:
      return { label: 'Omnidirectional-like', alpha: 0.0 };
  }
}

function parseTransducerLabel(s: string | undefined): string {
  switch (s) {
    case 'dynamic':
      return 'Dynamic';
    case 'ribbon':
      return 'Ribbon';
    case 'large_condenser':
      return 'Large-Diaphragm Condenser';
    case 'small_condenser':
      return 'Small-Diaphragm Condenser';
    case 'boundary_omni':
      return 'Measurement / Boundary Omni';
    default:
      return 'Small-Diaphragm Condenser';
  }
}

function parsePatternInfo(s: string | undefined): { key: PolarPattern; label: string } {
  switch (s) {
    case 'omni':
      return { key: 'omni', label: 'Omni' };
    case 'wide_cardioid':
      return { key: 'wide_cardioid', label: 'Wide cardioid' };
    case 'cardioid':
      return { key: 'cardioid', label: 'Cardioid' };
    case 'supercardioid':
      return { key: 'supercardioid', label: 'Supercardioid' };
    case 'hypercardioid':
      return { key: 'hypercardioid', label: 'Hypercardioid' };
    case 'figure8':
      return { key: 'figure8', label: 'Figure-8' };
    default:
      return { key: 'cardioid', label: 'Cardioid' };
  }
}

function parseRendererProfile(s: string | undefined): { id: string; description: string } {
  switch (s) {
    case 'flat_ideal':
      return {
        id: 'ideal_flat_analytical',
        description: 'Flat on-axis response with ideal analytical polar equation.',
      };
    case 'dynamic_sm7_class':
      return {
        id: 'dynamic_sm7_class_approx',
        description:
          'Parametric SM7-class approximation (55 Hz HPF, +1.2 dB @ 200 Hz, +1.9 dB @ 3.4 kHz, 10.5 kHz LPF).',
      };
    case 'dynamic_presence_57_class':
      return {
        id: 'dynamic_presence_approx',
        description:
          'Parametric instrument dynamic approximation (70 Hz HPF, +2.8 dB @ 4.8 kHz presence, 11 kHz LPF).',
      };
    case 'ribbon_smooth':
      return {
        id: 'ribbon_smooth_approx',
        description:
          'Parametric ribbon approximation (40 Hz HPF, +1.6 dB @ 180 Hz proximity warmth, 7.5 kHz gentle LPF).',
      };
    case 'large_condenser_warm':
      return {
        id: 'ld_condenser_warm_approx',
        description:
          'Parametric warm LDC approximation (35 Hz HPF, +1.1 dB @ 160 Hz, +1.5 dB @ 5.5 kHz).',
      };
    case 'boundary_omni_flat':
      return {
        id: 'omni_measurement_flat',
        description: 'Flat analytical omni pressure model (20 Hz HPF, flat to Nyquist).',
      };
    case 'small_condenser_neutral':
    default:
      return {
        id: 'sd_condenser_neutral_approx',
        description: 'Parametric neutral SDC approximation (30 Hz HPF, +0.7 dB @ 8 kHz air shelf).',
      };
  }
}

interface RawSourceEntry {
  id: string;
  name: string;
  category?: string;
  family?: string;
  symbol?: string;
  radiation?: string;
  synthesis_model_hint?: string | null;
  published_range_hz?: [number, number] | null;
  published_fundamental_notes?: string | null;
  published_practical_range_hz?: [number, number] | null;
  spectral_regions?: {
    body_warmth_hz?: [number, number] | null;
    presence_attack_hz?: [number, number] | null;
    air_high_hz?: [number, number] | null;
    formants_and_resonances_note?: string;
  };
  published_notes?: string;
  source_references?: string[];
  uncertainties?: string[];
  regen_model?: {
    model_id: string;
    model_label: string;
    family_kind: string;
    description: string;
  } | null;
}

interface RawMicEntry {
  id: string;
  brand: string;
  model: string;
  source_url: string;
  summary: string;
  transducer: string;
  pattern: string;
  renderer_profile: string;
  published_type?: string | null;
  published_patterns?: string | null;
  published_curves?: string[];
  published_primary_curve?: string | null;
  published_fr_points?: [number, number][];
  published_offaxis?: RustMicOffaxisTable | null;
  published_range_text?: string | null;
  published_range_hz?: [number, number] | null;
  published_sensitivity_dbv?: number | null;
  published_self_noise_dba?: number | null;
  published_max_spl_db?: number | null;
  published_impedance_ohm?: number | null;
  published_released_year?: number | null;
  published_measurement_dist_cm?: number | null;
  published_power?: string | null;
  source_document: string;
  uncertainties?: string[];
}

interface RawMaterialEntry {
  id: string;
  name: string;
  category: string;
  material_type: string;
  thickness_mm?: number | null;
  assembly: string;
  incidence: string;
  standard: string;
  response_kind: string;
  raw_absorption_6band: [number, number, number, number, number, number];
  biot: RustSurfaceBiotParameters;
  provenance: string;
  notes: string;
  uncertainties?: string[];
}

function readJsonFile<T>(filePath: string, label: string): T {
  if (!fs.existsSync(filePath)) {
    throw new CatalogLoadError(
      `Local capture-reference ${label} catalog missing at ${filePath}`,
      503,
    );
  }
  let rawText: string;
  try {
    rawText = fs.readFileSync(filePath, 'utf8');
  } catch (err) {
    throw new CatalogLoadError(
      `Failed reading local ${label} catalog at ${filePath}: ${err instanceof Error ? err.message : String(err)}`,
      503,
    );
  }
  try {
    return JSON.parse(rawText) as T;
  } catch (err) {
    throw new CatalogLoadError(
      `Malformed JSON in local ${label} catalog at ${filePath}: ${err instanceof Error ? err.message : String(err)}`,
      500,
    );
  }
}

export function buildRoomPresets(): RustRoomPreset[] {
  const presets: { key: RoomPresetKey; label: string; ft: [number, number, number]; faces: [number, number, number, number, number, number] }[] = [
    { key: 'small', label: 'Small (10 × 10 × 10 ft)', ft: [10, 10, 10], faces: [1, 1, 1, 1, 1, 1] },
    { key: 'medium', label: 'Medium (20 × 20 × 10 ft)', ft: [20, 20, 10], faces: [14, 14, 14, 14, 15, 6] },
    { key: 'large', label: 'Large (50 × 50 × 10 ft)', ft: [50, 50, 10], faces: [18, 18, 18, 18, 15, 18] },
    { key: 'open', label: 'Open (Field · Ground Only)', ft: [50, 50, 50], faces: [0, 0, 0, 0, 29, 0] },
    { key: 'direct', label: 'Direct (No Added Room)', ft: [10, 10, 10], faces: [0, 0, 0, 0, 0, 0] },
  ];
  return presets.map((p) => ({
    key: p.key,
    label: p.label,
    dimensions_m: [p.ft[0] * FEET_TO_METRES, p.ft[1] * FEET_TO_METRES, p.ft[2] * FEET_TO_METRES],
    default_surface_faces: p.faces,
  }));
}

export function buildPatterns(): { key: PolarPattern; label: string; alpha: number }[] {
  return [
    { key: 'omni', label: 'Omni', alpha: 1.0 },
    { key: 'wide_cardioid', label: 'Wide cardioid', alpha: 0.7 },
    { key: 'cardioid', label: 'Cardioid', alpha: 0.5 },
    { key: 'supercardioid', label: 'Supercardioid', alpha: 0.37 },
    { key: 'hypercardioid', label: 'Hypercardioid', alpha: 0.25 },
    { key: 'figure8', label: 'Figure-8', alpha: 0.0 },
  ];
}

export function transformSourceEntry(r: RawSourceEntry, index: number): RustSourceEntry {
  const rad = parseSourceRadiation(r.radiation);
  let modelId: string;
  let modelLabel: string;
  let familyKind: SourceRegenKind;
  let description: string;

  if (r.regen_model) {
    familyKind = parseRegenKind(r.regen_model.family_kind);
    modelId = r.regen_model.model_id;
    modelLabel = r.regen_model.model_label;
    description = r.regen_model.description;
  } else {
    const hintStr = (r.synthesis_model_hint ?? 'neutral').toLowerCase();
    familyKind = parseRegenKind(hintStr);
    modelId = `hint_${hintStr}`;
    modelLabel = `Guided ${modelTypeHint(familyKind)} Model`;
    description = physicsConstraintFor(familyKind).plausibility_note;
  }

  const synthesisModelHint =
    r.synthesis_model_hint && r.synthesis_model_hint.trim().length > 0
      ? r.synthesis_model_hint
      : modelTypeHint(familyKind);

  const specRegions = r.spectral_regions ?? {};
  const formantsNote =
    specRegions.formants_and_resonances_note && specRegions.formants_and_resonances_note.length > 0
      ? specRegions.formants_and_resonances_note
      : 'No narrow resonance peaks assumed; missing data left explicit.';

  return {
    index,
    id: r.id,
    name: r.name,
    category: parseSourceCategory(r.category),
    family: r.family && r.family.length > 0 ? r.family : 'Uncategorized',
    symbol: parseSourceSymbol(r.symbol),
    published_range_hz: r.published_range_hz ?? null,
    published_fundamental_notes: r.published_fundamental_notes ?? null,
    published_practical_range_hz: r.published_practical_range_hz ?? null,
    spectral_regions: {
      body_warmth_hz: specRegions.body_warmth_hz ?? null,
      presence_attack_hz: specRegions.presence_attack_hz ?? null,
      air_high_hz: specRegions.air_high_hz ?? null,
      formants_and_resonances_note: formantsNote,
    },
    published_notes: r.published_notes ?? '',
    source_references: Array.isArray(r.source_references) ? r.source_references : [],
    uncertainties: Array.isArray(r.uncertainties) ? r.uncertainties : [],
    radiation_label: rad.label,
    radiation_alpha: rad.alpha,
    synthesis_model_hint: synthesisModelHint,
    regen_model: {
      model_id: modelId,
      model_label: modelLabel,
      description,
      physics_constraint: physicsConstraintFor(familyKind),
    },
  };
}

export function transformMicEntry(r: RawMicEntry, index: number): RustMicEntry {
  const pattern = parsePatternInfo(r.pattern);
  const renderer = parseRendererProfile(r.renderer_profile);
  const sharedExplanation =
    `Modeled renderer profile '${renderer.id}': ${renderer.description} ` +
    `Published curves/specs are manufacturer reference only (not a measured hardware IR). ` +
    `Any entries sharing this renderer profile and '${pattern.label}' polar pattern produce identical DSP unless room/position/aim differ.`;

  return {
    index,
    id: r.id,
    brand: r.brand,
    model: r.model,
    source_url: r.source_url,
    transducer: parseTransducerLabel(r.transducer),
    default_pattern: pattern.key,
    default_pattern_label: pattern.label,
    published_type: r.published_type ?? null,
    published_patterns: r.published_patterns ?? null,
    published_curves: Array.isArray(r.published_curves) ? r.published_curves : [],
    published_primary_curve: r.published_primary_curve ?? null,
    published_fr_points: Array.isArray(r.published_fr_points) ? r.published_fr_points : [],
    published_offaxis: r.published_offaxis ?? null,
    published_range_text: r.published_range_text ?? null,
    published_range_hz: r.published_range_hz ?? null,
    published_sensitivity_dbv: r.published_sensitivity_dbv ?? null,
    published_self_noise_dba: r.published_self_noise_dba ?? null,
    published_max_spl_db: r.published_max_spl_db ?? null,
    published_impedance_ohm: r.published_impedance_ohm ?? null,
    published_released_year: r.published_released_year ?? null,
    published_measurement_dist_cm: r.published_measurement_dist_cm ?? null,
    published_power: r.published_power ?? null,
    source_document: r.source_document,
    uncertainties: Array.isArray(r.uncertainties) ? r.uncertainties : [],
    renderer_profile_id: renderer.id,
    renderer_profile_description: renderer.description,
    curve_provenance: MIC_ARCHIVE_PROVENANCE,
    published_notes: r.summary ?? '',
    shared_model_explanation: sharedExplanation,
  };
}

export function transformSurfaceEntry(m: RawMaterialEntry, index: number): RustSurfaceEntry {
  const raw = m.raw_absorption_6band;
  const eff = raw.map((a) => (Number.isFinite(a) ? Math.min(1.0, Math.max(0.0, a)) : 1.0)) as [
    number,
    number,
    number,
    number,
    number,
    number,
  ];
  const refl = eff.map((a) => Math.sqrt(1.0 - a)) as [
    number,
    number,
    number,
    number,
    number,
    number,
  ];
  const meanReflectance = refl.reduce((acc, v) => acc + v, 0) / 6.0;

  return {
    index,
    id: m.id,
    name: m.name,
    category: m.category,
    material_type: m.material_type,
    thickness_mm: m.thickness_mm ?? null,
    assembly: m.assembly,
    incidence: m.incidence,
    standard: m.standard,
    response_kind: m.response_kind,
    raw_absorption_6band: raw,
    effective_absorption_6band: eff,
    pressure_reflectance_6band: refl,
    mean_reflectance: meanReflectance,
    biot: m.biot,
    provenance: m.provenance,
    notes: m.notes,
    uncertainties: Array.isArray(m.uncertainties) ? m.uncertainties : [],
  };
}

/**
 * Load the complete offline capture-reference catalogs directly from local JSON files
 * (source-profiles-138.json, mic-archive-972.json, acoustic-materials-32.json).
 * Zero external network calls and zero dependency on a prebuilt spectracraft-qa binary.
 */
export function loadLocalStudioCatalogs(
  captureRefDir: string,
  envSourceCatalogPath?: string,
): RustStudioCatalogs {
  const sourcePath =
    envSourceCatalogPath && fs.existsSync(envSourceCatalogPath)
      ? envSourceCatalogPath
      : path.join(captureRefDir, 'source-profiles-138.json');
  const micPath = path.join(captureRefDir, 'mic-archive-972.json');
  const matPath = path.join(captureRefDir, 'acoustic-materials-32.json');

  const rawSourcesFile = readJsonFile<{ sources?: RawSourceEntry[] }>(sourcePath, 'source');
  const rawMicsFile = readJsonFile<{ mics?: RawMicEntry[] }>(micPath, 'microphone');
  const rawMaterialsFile = readJsonFile<{ materials?: RawMaterialEntry[] }>(matPath, 'material');

  if (!Array.isArray(rawSourcesFile.sources) || rawSourcesFile.sources.length === 0) {
    throw new CatalogLoadError(`Local source catalog at ${sourcePath} contains no source entries.`, 500);
  }
  if (!Array.isArray(rawMicsFile.mics) || rawMicsFile.mics.length === 0) {
    throw new CatalogLoadError(`Local microphone catalog at ${micPath} contains no microphone entries.`, 500);
  }
  if (!Array.isArray(rawMaterialsFile.materials) || rawMaterialsFile.materials.length === 0) {
    throw new CatalogLoadError(`Local material catalog at ${matPath} contains no material entries.`, 500);
  }

  const sources = rawSourcesFile.sources.map((entry, idx) => transformSourceEntry(entry, idx));
  const mics = rawMicsFile.mics.map((entry, idx) => transformMicEntry(entry, idx));
  const surfaces = rawMaterialsFile.materials.map((entry, idx) => transformSurfaceEntry(entry, idx));

  const brands = new Set(mics.map((m) => m.brand)).size;
  const digitizedCurves = mics.filter((m) => m.published_fr_points.length > 0).length;
  const offaxisTables = mics.filter((m) => m.published_offaxis !== null).length;

  return {
    bands_hz: [...STUDIO_BANDS_HZ],
    face_labels: [...STUDIO_FACE_LABELS],
    sources,
    mics,
    surfaces,
    room_presets: buildRoomPresets(),
    patterns: buildPatterns(),
    catalog_provenance: {
      engine: 'spectracraft-mono/capture-reference (local offline JSON catalog)',
      source_count: sources.length,
      mic_count: mics.length,
      surface_count: surfaces.length,
      mic_archive_note: `${mics.length} microphones across ${brands} brands loaded locally from capture-reference (${digitizedCurves} with digitized curves, ${offaxisTables} with published off-axis polar tables) + ${surfaces.length} architectural 6-band acoustic material profiles + ${sources.length} source profiles. Zero runtime network dependency; published specs/curves are reference only, never measured emulations.`,
    },
  };
}
