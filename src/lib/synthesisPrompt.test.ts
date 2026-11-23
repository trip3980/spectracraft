import test from 'node:test';
import assert from 'node:assert/strict';
import {
  DEFAULT_PIPELINE_STAGES,
  DEFAULT_VOICING_CONTROLS,
  makeRoomPreset,
  type RustMicEntry,
  type RustSourceEntry,
  type RustSurfaceEntry,
} from './studioRig.ts';
import {
  buildStructuredSynthesisPrompt,
  type ReferenceShape,
} from './synthesisPrompt.ts';

const MOCK_SHAPE: ReferenceShape = {
  shape_sha256: 'abc123def4567890abc123def4567890abc123def4567890abc123def4567890',
  sample_rate: 48000,
  duration_s: 4.0,
  pitch_trajectory: {
    note_count: 6,
    min_hz: 110.0,
    max_hz: 329.6,
    median_hz: 196.0,
    max_bend_semitones: 1.4,
    mean_pitch_span_semitones: 0.45,
    notes_with_bends: 2,
    out_of_published_range_notes: 1,
    published_range_note: '1 note(s) lie outside published range — kept exactly as performed.',
  },
  dynamics: {
    peak_dbfs: -4.2,
    rms_dbfs: -16.8,
    dynamic_range_db: 12.6,
    note_level_contrast_db: 5.4,
    strongest_note_index: 4,
    quietest_note_index: 0,
  },
  spectral_envelope: {
    spectral_centroid_hz: 640.0,
    band_centers_hz: [125, 250, 500, 1000, 2000, 4000],
    band_relative_db: [-6.0, 0.0, -2.5, -7.0, -14.0, -22.0],
    prominence_regions: [
      { center_hz: 250, relative_db: 0.0, band_label: 'Warmth / chest (250 Hz)' },
      { center_hz: 500, relative_db: -2.5, band_label: 'Lower midrange (500 Hz)' },
    ],
    envelope_note: 'Broad 6-band reference envelope across 140 active frames.',
  },
  harmonic_vs_noise: {
    mean_tonal_fraction: 0.84,
    min_tonal_fraction: 0.55,
    max_tonal_fraction: 0.96,
    mean_noise_fraction: 0.16,
    balance_label: 'Predominantly harmonic / tonal',
    bleed_risk: 'Low (coherent single-source evidence)',
    bleed_note: 'Active frames show coherent single-source harmonic tracking.',
  },
  attack_and_decay: {
    median_attack_ms: 14.0,
    median_low_decay_per_s: 1.1,
    median_high_decay_per_s: 3.2,
    sustain_ratio: 0.62,
    total_repicks: 0,
    attack_character_label: 'Crisp onset with sustained body',
  },
  transitions: {
    overlapping_ring_notes: 2,
    connected_transitions: 3,
    separated_transitions: 2,
    continuity_ratio: 0.6,
    transition_note: '2 overlapping ring tail(s), 3 connected/legato handoff(s), 2 separated gap(s).',
  },
  uncertainty: {
    total_frames: 400,
    supported_frames: 310,
    uncertain_frames: 14,
    out_of_target_frames: 2,
    silence_frames: 74,
    uncertain_pct: 3.5,
    out_of_target_pct: 0.5,
    flags: ['14 uncertain frame(s) preserved as ambiguous.'],
    limits_note: 'Extracted shape is an interpretation of the signal, not a perfect record of player intent.',
  },
};

const MOCK_SOURCES: RustSourceEntry[] = [
  {
    index: 0,
    id: 'electric_guitar',
    name: 'Electric Guitar',
    category: 'Instrument',
    family: 'Strings — Plucked',
    symbol: 'guitar',
    published_range_hz: [82, 1318],
    published_fundamental_notes: 'E2–E6',
    published_practical_range_hz: [82, 5500],
    spectral_regions: {
      body_warmth_hz: [100, 260],
      presence_attack_hz: [1200, 3500],
      air_high_hz: [4000, 5500],
      formants_and_resonances_note: '',
    },
    published_notes: '',
    source_references: [],
    uncertainties: [],
    radiation_label: 'Dipole / Cabinet',
    radiation_alpha: 0.5,
    regen_model: {
      model_id: 'plucked_electric',
      model_label: 'Plucked Electric',
      description: 'Plucked electric guitar model',
    },
  },
  {
    index: 1,
    id: 'cello',
    name: 'Cello',
    category: 'Instrument',
    family: 'Strings — Bowed',
    symbol: 'bowed',
    published_range_hz: [65, 988],
    published_fundamental_notes: 'C2–B5',
    published_practical_range_hz: [65, 6000],
    spectral_regions: {
      body_warmth_hz: [120, 300],
      presence_attack_hz: [1000, 3000],
      air_high_hz: [4000, 6000],
      formants_and_resonances_note: '',
    },
    published_notes: '',
    source_references: [],
    uncertainties: [],
    radiation_label: 'Acoustic Body',
    radiation_alpha: 0.35,
    regen_model: {
      model_id: 'bowed_cello_low',
      model_label: 'Bowed Low Strings',
      description: 'Bowed low strings model',
    },
  },
];

const MOCK_MICS: RustMicEntry[] = [
  {
    index: 0,
    id: 'shure-sm7b',
    brand: 'Shure',
    model: 'SM7B',
    source_url: 'https://micarchive.com/mics/shure-sm7b/',
    transducer: 'Dynamic',
    default_pattern: 'cardioid',
    default_pattern_label: 'Cardioid',
    published_type: 'Dynamic',
    published_patterns: 'Cardioid',
    published_curves: [],
    published_primary_curve: null,
    published_fr_points: [],
    published_offaxis: null,
    published_range_text: '50 Hz – 20 kHz',
    published_range_hz: [50, 20000],
    published_sensitivity_dbv: -59,
    published_self_noise_dba: null,
    published_max_spl_db: null,
    published_impedance_ohm: 150,
    published_released_year: 2001,
    published_measurement_dist_cm: null,
    published_power: 'None',
    source_document: '',
    uncertainties: [],
    renderer_profile_id: 'dynamic_sm7_class',
    renderer_profile_description: 'Dynamic SM7 Class',
    curve_provenance: '',
    published_notes: '',
    shared_model_explanation: '',
  },
  {
    index: 1,
    id: 'sennheiser-mkh-50',
    brand: 'Sennheiser',
    model: 'MKH 50 P48',
    source_url: 'https://micarchive.com/mics/sennheiser-mkh-50/',
    transducer: 'Condenser',
    default_pattern: 'supercardioid',
    default_pattern_label: 'Supercardioid',
    published_type: 'Condenser',
    published_patterns: 'Supercardioid',
    published_curves: [],
    published_primary_curve: null,
    published_fr_points: [],
    published_offaxis: null,
    published_range_text: '40 Hz – 20 kHz',
    published_range_hz: [40, 20000],
    published_sensitivity_dbv: -32,
    published_self_noise_dba: 12,
    published_max_spl_db: 134,
    published_impedance_ohm: 150,
    published_released_year: 1990,
    published_measurement_dist_cm: null,
    published_power: '48V Phantom',
    source_document: '',
    uncertainties: [],
    renderer_profile_id: 'small_condenser_neutral',
    renderer_profile_description: 'Small Condenser Neutral',
    curve_provenance: '',
    published_notes: '',
    shared_model_explanation: '',
  },
];

const MOCK_SURFACES: RustSurfaceEntry[] = [
  {
    index: 0,
    id: 'open',
    name: 'Open Boundary',
    category: 'Open',
    material_type: 'Open',
    thickness_mm: null,
    assembly: '',
    incidence: 'Free field',
    standard: 'Ideal',
    response_kind: 'Free field',
    raw_absorption_6band: [1, 1, 1, 1, 1, 1],
    effective_absorption_6band: [1, 1, 1, 1, 1, 1],
    pressure_reflectance_6band: [0, 0, 0, 0, 0, 0],
    mean_reflectance: 0,
    biot: {
      porosity: null,
      airflow_resistivity_pa_s_m2: null,
      tortuosity: null,
      vcl_um: null,
      tcl_um: null,
      standard_ref: 'ISO 10534-2',
      status: 'Not published',
    },
    provenance: '',
    notes: '',
    uncertainties: [],
  },
  {
    index: 1,
    id: 'wedge-foam-2',
    name: 'Wedge foam · 2 inches',
    category: 'Treatment',
    material_type: 'Foam',
    thickness_mm: 50.8,
    assembly: '',
    incidence: 'Random',
    standard: 'ASTM C423',
    response_kind: 'Random',
    raw_absorption_6band: [0.14, 0.45, 0.85, 1.05, 0.99, 0.98],
    effective_absorption_6band: [0.14, 0.45, 0.85, 1.0, 0.99, 0.98],
    pressure_reflectance_6band: [0.93, 0.74, 0.39, 0, 0.1, 0.14],
    mean_reflectance: 0.38,
    biot: {
      porosity: null,
      airflow_resistivity_pa_s_m2: null,
      tortuosity: null,
      vcl_um: null,
      tcl_um: null,
      standard_ref: 'ISO 10534-2',
      status: 'Not published',
    },
    provenance: '',
    notes: '',
    uncertainties: [],
  },
  {
    index: 2,
    id: 'sheet-steel',
    name: 'Sheet steel',
    category: 'Metal',
    material_type: 'Metal',
    thickness_mm: 2.0,
    assembly: '',
    incidence: 'Random',
    standard: 'ISO 354',
    response_kind: 'Random',
    raw_absorption_6band: [0.02, 0.02, 0.02, 0.03, 0.03, 0.04],
    effective_absorption_6band: [0.02, 0.02, 0.02, 0.03, 0.03, 0.04],
    pressure_reflectance_6band: [0.99, 0.99, 0.99, 0.98, 0.98, 0.98],
    mean_reflectance: 0.99,
    biot: {
      porosity: null,
      airflow_resistivity_pa_s_m2: null,
      tortuosity: null,
      vcl_um: null,
      tcl_um: null,
      standard_ref: 'ISO 10534-2',
      status: 'Not published',
    },
    provenance: '',
    notes: '',
    uncertainties: [],
  },
];

test('structured synthesis prompt updates fingerprint on source/mic/material/room changes and stays invariant on display-only changes', () => {
  const baseRig = makeRoomPreset('small');
  baseRig.sourceIndex = 0;
  baseRig.surfaceFaces = [1, 1, 1, 1, 1, 1];

  const basePrompt = buildStructuredSynthesisPrompt({
    sessionId: 'slow-solo-ref',
    sourceFileName: 'six-note-fixture-48k.wav',
    shape: MOCK_SHAPE,
    rig: baseRig,
    sources: MOCK_SOURCES,
    mics: MOCK_MICS,
    surfaces: MOCK_SURFACES,
    analysis: null,
  });

  // Display-only changes MUST NOT alter promptFingerprint or shapeAnchor
  const displayChangedRig = structuredClone(baseRig);
  displayChangedRig.unit = 'cm';
  displayChangedRig.showPatterns = true;
  displayChangedRig.showGuidanceOverlay = true;
  displayChangedRig.sourceFacingDeg = 135;

  const displayPrompt = buildStructuredSynthesisPrompt({
    sessionId: 'slow-solo-ref',
    sourceFileName: 'six-note-fixture-48k.wav',
    shape: MOCK_SHAPE,
    rig: displayChangedRig,
    sources: MOCK_SOURCES,
    mics: MOCK_MICS,
    surfaces: MOCK_SURFACES,
    analysis: null,
  });

  assert.equal(displayPrompt.promptFingerprint, basePrompt.promptFingerprint);
  assert.equal(displayPrompt.shapeAnchor.shapeSha256, MOCK_SHAPE.shape_sha256);

  // Changing Source updates promptFingerprint while holding shapeSha256 fixed
  const srcRig = structuredClone(baseRig);
  srcRig.sourceIndex = 1;
  const srcPrompt = buildStructuredSynthesisPrompt({
    sessionId: 'slow-solo-ref',
    sourceFileName: 'six-note-fixture-48k.wav',
    shape: MOCK_SHAPE,
    rig: srcRig,
    sources: MOCK_SOURCES,
    mics: MOCK_MICS,
    surfaces: MOCK_SURFACES,
    analysis: null,
  });
  assert.notEqual(srcPrompt.promptFingerprint, basePrompt.promptFingerprint);
  assert.equal(srcPrompt.shapeAnchor.shapeSha256, basePrompt.shapeAnchor.shapeSha256);
  assert.match(srcPrompt.promptSentence, /Cello/);

  // Changing Mic updates promptFingerprint
  const micRig = structuredClone(baseRig);
  micRig.mics[0].catalogIndex = 1;
  const micPrompt = buildStructuredSynthesisPrompt({
    sessionId: 'slow-solo-ref',
    sourceFileName: 'six-note-fixture-48k.wav',
    shape: MOCK_SHAPE,
    rig: micRig,
    sources: MOCK_SOURCES,
    mics: MOCK_MICS,
    surfaces: MOCK_SURFACES,
    analysis: null,
  });
  assert.notEqual(micPrompt.promptFingerprint, basePrompt.promptFingerprint);
  assert.match(micPrompt.promptSentence, /MKH 50/);

  // Changing Material on one face updates promptFingerprint
  const matRig = structuredClone(baseRig);
  matRig.surfaceFaces[0] = 2;
  const matPrompt = buildStructuredSynthesisPrompt({
    sessionId: 'slow-solo-ref',
    sourceFileName: 'six-note-fixture-48k.wav',
    shape: MOCK_SHAPE,
    rig: matRig,
    sources: MOCK_SOURCES,
    mics: MOCK_MICS,
    surfaces: MOCK_SURFACES,
    analysis: null,
  });
  assert.notEqual(matPrompt.promptFingerprint, basePrompt.promptFingerprint);
  assert.match(matPrompt.captureCondition.materialsSummary, /Sheet steel/);

  // Out-of-range note preservation is explicitly surfaced
  assert.equal(basePrompt.sourceCondition.outOfRangeKeptCount, 1);
  assert.equal(basePrompt.shapeAnchor.uncertainFrames, 14);
});

test('voicing controls and post-dry pipeline stage toggles update promptFingerprint without changing shapeAnchor', () => {
  const baseRig = makeRoomPreset('small');
  baseRig.sourceIndex = 0;
  baseRig.surfaceFaces = [1, 1, 1, 1, 1, 1];

  const basePrompt = buildStructuredSynthesisPrompt({
    sessionId: 'slow-solo-ref',
    sourceFileName: 'six-note-fixture-48k.wav',
    shape: MOCK_SHAPE,
    rig: baseRig,
    sources: MOCK_SOURCES,
    mics: MOCK_MICS,
    surfaces: MOCK_SURFACES,
    analysis: null,
    voicing: DEFAULT_VOICING_CONTROLS,
    pipelineStages: DEFAULT_PIPELINE_STAGES,
  });

  const analogPrompt = buildStructuredSynthesisPrompt({
    sessionId: 'slow-solo-ref',
    sourceFileName: 'six-note-fixture-48k.wav',
    shape: MOCK_SHAPE,
    rig: baseRig,
    sources: MOCK_SOURCES,
    mics: MOCK_MICS,
    surfaces: MOCK_SURFACES,
    analysis: null,
    voicing: { ...DEFAULT_VOICING_CONTROLS, characterTarget: 'analog', straightenMode: 'light' },
    pipelineStages: DEFAULT_PIPELINE_STAGES,
  });
  assert.notEqual(analogPrompt.promptFingerprint, basePrompt.promptFingerprint);
  assert.equal(analogPrompt.shapeAnchor.shapeSha256, basePrompt.shapeAnchor.shapeSha256);

  const micEnabledPrompt = buildStructuredSynthesisPrompt({
    sessionId: 'slow-solo-ref',
    sourceFileName: 'six-note-fixture-48k.wav',
    shape: MOCK_SHAPE,
    rig: baseRig,
    sources: MOCK_SOURCES,
    mics: MOCK_MICS,
    surfaces: MOCK_SURFACES,
    analysis: null,
    voicing: DEFAULT_VOICING_CONTROLS,
    pipelineStages: { ...DEFAULT_PIPELINE_STAGES, micEnabled: true },
  });
  assert.notEqual(micEnabledPrompt.promptFingerprint, basePrompt.promptFingerprint);
  assert.equal(micEnabledPrompt.shapeAnchor.shapeSha256, basePrompt.shapeAnchor.shapeSha256);
});

