#!/usr/bin/env node
/**
 * Generates `spectracraft-mono/source-qualification/coverage-matrix.json`
 * directly from the authoritative `spectracraft-mono/capture-reference/source-profiles-138.json`.
 *
 * Never maintain a second handwritten source list: every row maps 1:1 to a catalog entry,
 * keeping catalog presence, renderer availability, synthetic controls, real-recording
 * acceptance, human review, and application-surface checks strictly separate.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '..');
const catalogPath = path.join(repoRoot, 'spectracraft-mono', 'capture-reference', 'source-profiles-138.json');
const outPath = path.join(repoRoot, 'spectracraft-mono', 'source-qualification', 'coverage-matrix.json');

const catalog = JSON.parse(fs.readFileSync(catalogPath, 'utf8'));
if (!Array.isArray(catalog.sources) || catalog.sources.length !== 138) {
  throw new Error(`Expected 138 sources in source-profiles-138.json, got ${catalog.sources?.length}`);
}

function classifyCapabilities(src) {
  const family = src.family || 'Unknown';
  const nameLower = (src.name || '').toLowerCase();
  const kind = src.regen_model?.family_kind || 'neutral';
  const minHz = Array.isArray(src.published_fundamental_range_hz)
    ? src.published_fundamental_range_hz[0]
    : null;
  const maxHz = Array.isArray(src.published_fundamental_range_hz)
    ? src.published_fundamental_range_hz[1]
    : null;

  const limitations = [];
  let analyzerCapability = 'pitched_guitar_analyzer_75_1400hz_hp65hz';
  let scopeStatus = 'single_source_supported_target';
  let requiredBehavior = [];

  if (src.symbol === 'ensemble' || nameLower.includes('ensemble') || nameLower.includes('choir') || nameLower.includes('section') || nameLower.includes('kit')) {
    scopeStatus = 'explicit_scope_limited_no_full_mix_separation';
    limitations.push('Single-source invariant: catalog label cannot authorize full-mix separation; multi-instrument mixes are out of scope.');
  }

  if (minHz !== null && minHz < 75) {
    limitations.push(`Published fundamental (${minHz} Hz) extends below current guitar analyzer 75 Hz floor / 65 Hz high-pass.`);
  }
  if (maxHz !== null && maxHz > 1400) {
    limitations.push(`Published fundamental (${maxHz} Hz) extends above current guitar analyzer 1400 Hz ceiling.`);
  }

  if (family.includes('Vocal') || src.symbol === 'voice') {
    if (nameLower.includes('spoken') || nameLower.includes('narrat') || nameLower.includes('rap') || nameLower.includes('whisper') || nameLower.includes('podcast')) {
      analyzerCapability = 'unsupported_speech_whisper_consonants';
      limitations.push('Current pitched-note cue representation & fixed vocal formants do not establish preservation of changing vowels, lyrics, sibilants, plosives, or whispers.');
      requiredBehavior = ['complete_words', 'sibilants', 'plosives', 'pauses', 'fast_syllables', 'voiced_unvoiced_transitions', 'whisper_passages'];
    } else {
      analyzerCapability = 'pitched_contour_only_fixed_formants';
      limitations.push('Fixed vocal formants do not establish preservation of changing vowels, lyrics, consonants, or breath transitions.');
      requiredBehavior = ['low_and_high_registers', 'register_transitions', 'sustained_and_changing_vowels', 'lyrics_consonants', 'breath', 'vibrato', 'slides', 'soft_entrances_dynamics'];
    }
  } else if (kind === 'percussion_unpitched' || (src.symbol === 'percussion' && !nameLower.includes('timpani') && !nameLower.includes('marimba') && !nameLower.includes('vibraphone') && !nameLower.includes('xylophone') && !nameLower.includes('glockenspiel') && !nameLower.includes('chime') && !nameLower.includes('steel'))) {
    analyzerCapability = 'unsupported_unpitched_transient_extractor';
    limitations.push('Unpitched percussion requires dedicated transient/spectral-evolution extraction; pitched 75-1400 Hz F0 tracking is N/A.');
    requiredBehavior = ['rhythm', 'ghost_notes', 'rolls', 'attack_shape', 'spectral_evolution', 'decay'];
  } else if (src.symbol === 'percussion') {
    requiredBehavior = ['repeated_strikes', 'overlapping_decays', 'rolls', 'harmonic_inharmonic_resonances'];
  } else if (family.includes('Plucked') || src.symbol === 'guitar') {
    requiredBehavior = ['repeated_plucks', 'slides', 'bends', 'quiet_attacks', 'chords', 'overlapping_decay'];
  } else if (family.includes('Bowed') || src.symbol === 'bowed') {
    requiredBehavior = ['sustained_bowing', 'bow_changes', 'legato', 'vibrato', 'glissandi', 'dynamics', 'double_stops'];
  } else if (nameLower.includes('flute') || nameLower.includes('piccolo') || nameLower.includes('recorder') || nameLower.includes('whistle')) {
    requiredBehavior = ['breath_rich_attacks', 'tonguing', 'slurs', 'quiet_sustain', 'low_and_high_registers'];
  } else if (src.symbol === 'wind') {
    requiredBehavior = ['register_changes', 'tonguing', 'sustained_tones', 'low_fundamentals'];
  } else if (src.symbol === 'brass') {
    requiredBehavior = ['soft_and_strong_attacks', 'lip_slurs', 'dynamic_brightness', 'glissandi', 'low_fundamentals'];
  } else if (nameLower.includes('organ') || nameLower.includes('accordion') || nameLower.includes('harmonica') || nameLower.includes('melodica')) {
    requiredBehavior = ['sustained_chords', 'legato', 'staccato', 'release_behavior', 'level_swells'];
  } else if (src.symbol === 'keyboard') {
    requiredBehavior = ['low_and_high_notes', 'repeated_notes', 'chords', 'arpeggios', 'pedal_sustain_overlap', 'independent_releases'];
  } else if (src.symbol === 'amp') {
    requiredBehavior = ['documented_source_and_chain_conditions', 'preserved_performed_gestures', 'disclosed_cabinet_approximation'];
    limitations.push('Cabinet/amplifier entries represent modeled recording-chain conditions, not independent acoustic instruments.');
  } else {
    scopeStatus = 'unspecified_or_auxiliary_scope';
    requiredBehavior = ['explicit_scope_disclosure', 'honest_uncertainty'];
  }

  return { analyzerCapability, scopeStatus, requiredBehavior, limitations };
}

const families = new Set();
const regenKinds = new Set();

const rows = catalog.sources.map((src) => {
  families.add(src.family);
  regenKinds.add(src.regen_model?.family_kind || 'neutral');
  const cap = classifyCapabilities(src);
  return {
    catalog_index: src.index,
    catalog_id: src.id,
    name: src.name,
    category: src.category,
    display_family: src.family,
    symbol: src.symbol,
    renderer_model_id: src.regen_model?.model_id || 'neutral_cue_modal',
    renderer_class: src.regen_model?.family_kind || 'neutral',
    analyzer_capability: cap.analyzerCapability,
    scope_status: cap.scopeStatus,
    claimed_behavior: cap.requiredBehavior,
    real_recording_cases: src.id === 'strings___plucked_&_fretted_electric_guitar__clean_1x12_open_back_combo_'
      ? ['real_monniaux_slow_solo_20110108']
      : [],
    synthetic_control_cases: src.id === 'strings___plucked_&_fretted_electric_guitar__clean_1x12_open_back_combo_'
      ? ['ctrl_guitar_plucked_arpeggio']
      : src.id === 'strings___plucked_&_fretted_electric_bass_guitar__fingerstyle_'
        ? ['ctrl_bass_low_register']
        : src.id === 'human_voice_female_soprano_singing'
          ? ['ctrl_vocal_sustained_vowel']
          : src.id === 'percussion___unpitched_snare_drum'
            ? ['ctrl_unpitched_percussion_clicks']
            : [],
    automated_technical_status: 'NOT RUN',
    automated_musical_status: 'NOT RUN',
    synthetic_control_status: 'NOT RUN',
    real_recording_acceptance: 'NOT RUN',
    human_review_status: 'NOT RUN',
    application_surfaces: {
      cli: 'NOT RUN',
      desktop_egui: 'NOT RUN',
      web_bridge: 'NOT RUN',
      tauri_packaged: 'NOT RUN',
    },
    limitations: cap.limitations,
  };
});

const matrix = {
  schema_version: '1.0.0',
  generated_from: 'spectracraft-mono/capture-reference/source-profiles-138.json',
  baseline_commit: '9fbd775800b039f77cb6c9a901a458a0dadb715a',
  source_count: rows.length,
  display_family_count: families.size,
  renderer_class_count: regenKinds.size,
  policy: 'Catalog presence, renderer availability, synthetic-control success, real-recording acceptance, application-surface checks, and human listening review are tracked as strictly separate fields. No row is marked recording-qualified without real-recording evidence, passing technical/musical gates, and explicit human review.',
  rows,
};

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, `${JSON.stringify(matrix, null, 2)}\n`, 'utf8');
console.log(
  `Generated ${path.relative(repoRoot, outPath)}: ${rows.length} sources, ${families.size} families, ${regenKinds.size} renderer classes.`,
);
