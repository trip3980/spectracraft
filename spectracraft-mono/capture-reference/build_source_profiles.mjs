#!/usr/bin/env node
/**
 * Reproducible offline generator for `source-profiles-138.json` inside `capture-reference/`.
 *
 * Compiles all 138 SpectraCraft acoustic source entries with published fundamental &
 * practical frequency ranges, broad spectral character regions (body/warmth, presence/attack,
 * high/air), qualitative formant/body-resonance notes, source references (Yona Marie Music
 * Instrument Frequency Chart, orchestral acoustic reference tables [Adler, Meyer, Rossing],
 * RWC Musical Instrument Sound Database, audiolabs/SNDB), explicit uncertainties, and the
 * honest parametric regeneration model used by the Rust dry synthesizer.
 *
 * Strict honesty rules:
 * - Zero invented narrow instrument-specific resonance peaks.
 * - Missing or unspecified data is explicitly `null`.
 * - Published ranges are reference only — the engine NEVER clamps, filters, or transposes
 *   extracted performance cues into published ranges.
 * - Zero runtime network dependency.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const OUT_PATH = path.join(__dirname, 'source-profiles-138.json');

const REFS_ORCHESTRAL = [
  'Yona Marie Music Instrument Frequency Chart (yonamariemusic.com/instrument-frequency-chart)',
  'Orchestral acoustics & range tables (Adler, The Study of Orchestration; Meyer, Acoustics and the Performance of Music; Rossing, The Science of Sound)',
  'RWC Musical Instrument Sound Database (Goto et al.)',
  'Single Note Database (audiolabs/SNDB)',
];

const REFS_VOICE = [
  'Yona Marie Music Instrument Frequency Chart (yonamariemusic.com/instrument-frequency-chart)',
  'Vocal acoustics & formant tables (Sundberg, The Science of the Singing Voice; Rossing, The Science of Sound)',
  'RWC Music Database — Vocal & Choral reference ranges',
];

const REFS_PLUCKED_AMP = [
  'Yona Marie Music Instrument Frequency Chart (yonamariemusic.com/instrument-frequency-chart)',
  'Acoustics of stringed instruments & guitar cabinets (Fletcher & Rossing, The Physics of Musical Instruments)',
  'RWC Musical Instrument Sound Database (Goto et al.)',
  'Single Note Database (audiolabs/SNDB)',
];

const REFS_PERCUSSION = [
  'Yona Marie Music Instrument Frequency Chart (yonamariemusic.com/instrument-frequency-chart)',
  'Percussion acoustics tables (Rossing, Science of Percussion Instruments)',
  'RWC Musical Instrument Sound Database (Goto et al.)',
];

const BASE_UNCERTAINTIES = [
  'Published frequency ranges and broad spectral regions are general reference guidelines; actual playing range, extended techniques, alternate tunings, and harmonic extension vary per performer and recording.',
  'Exact narrow wood/cavity/bore/vocal-tract resonance peak frequencies vary significantly by individual physical instrument and are intentionally not invented here.',
  'Published ranges are informational only and never clamp, discard, or transpose extracted performance cues.',
];

function makeId(family, name) {
  return (
    family.toLowerCase().replace(/[ /—-]/g, '_') +
    '_' +
    name.toLowerCase().replace(/[ /—\-().]/g, '_')
  );
}

function hzToNearestNote(hz) {
  if (!Number.isFinite(hz) || hz <= 0) return null;
  const midi = Math.round(69 + 12 * Math.log2(hz / 440));
  const names = ['C', 'C#', 'D', 'Eb', 'E', 'F', 'F#', 'G', 'Ab', 'A', 'Bb', 'B'];
  const name = names[((midi % 12) + 12) % 12];
  const octave = Math.floor(midi / 12) - 1;
  return `${name}${octave}`;
}

function formatCompass(range, pitched = true) {
  if (!range || !pitched) return null;
  const lo = hzToNearestNote(range[0]);
  const hi = hzToNearestNote(range[1]);
  return lo && hi ? `${lo}–${hi}` : null;
}

const REGEN_MODELS = {
  neutral_cue_modal: {
    model_id: 'neutral_cue_modal',
    model_label: 'Neutral Cue-Driven Modal Synthesizer (Baseline)',
    family_kind: 'neutral',
    description:
      'Unbiased modal partial synthesis driven directly by extracted cue pitch, amplitude, and strike events with flat spectral tilt and zero instrument body assumptions.',
  },
  plucked_electric_string: {
    model_id: 'plucked_electric_string',
    model_label: 'Modeled Plucked Electric String (Baseline)',
    family_kind: 'plucked_electric',
    description:
      'Plucked-string modal synthesis with fret-scaled pick/pickup comb weighting, natural upper-partial decay, and clean midrange definition. Driven 100% by extracted cues.',
  },
  plucked_acoustic_wood: {
    model_id: 'plucked_acoustic_wood',
    model_label: 'Modeled Plucked Acoustic String + Wood Body',
    family_kind: 'plucked_acoustic',
    description:
      'Plucked-string modal synthesis with broad acoustic wood-body warmth (~140–240 Hz shelf), gentle low-mid scoop (~420 Hz), and crisp string/pick upper-harmonic sparkle (~3–5 kHz).',
  },
  plucked_bass_string: {
    model_id: 'plucked_bass_string',
    model_label: 'Modeled Plucked Bass String (Low Fundamental Weight)',
    family_kind: 'plucked_bass',
    description:
      'Strong fundamental and 2nd/3rd harmonic weighting with steeper upper-partial decay, warm low-body contour (~95–180 Hz), and controlled finger/pick definition (~1.2–2.2 kHz).',
  },
  bowed_cello_low: {
    model_id: 'bowed_cello_low',
    model_label: 'Modeled Bowed Low-String (Cello / Viola / Bass)',
    family_kind: 'bowed_low',
    description:
      'Sustained bowed-string harmonic envelope with slower bow-bite attack ramp (~26 ms), rich lower-order harmonics, broad warm body resonance (~170 Hz), mellow box dip (~650 Hz), bow-bridge presence (~2.1 kHz), and subtle cue-modulated rosin-bow air.',
  },
  bowed_violin_high: {
    model_id: 'bowed_violin_high',
    model_label: 'Modeled Bowed Upper-String (Violin / Fiddle)',
    family_kind: 'bowed_high',
    description:
      'Sustained bowed-string synthesis with smooth bow onset (~22 ms), extended upper-harmonic energy, warm body contour (~320 Hz), bridge-hill brilliance (~2.8 kHz), and subtle cue-modulated bow friction air.',
  },
  woodwind_flute: {
    model_id: 'woodwind_flute',
    model_label: 'Modeled Open-Tube Woodwind (Flute / Recorder)',
    family_kind: 'woodwind_flute',
    description:
      'Fundamental-dominant harmonic spectrum (steep harmonic rolloff above partial 4), smooth breath onset (~24 ms), airy upper presence (~3.5–6 kHz), and subtle cue-modulated embouchure breath noise.',
  },
  woodwind_reed: {
    model_id: 'woodwind_reed',
    model_label: 'Modeled Reed Woodwind (Clarinet / Oboe / Sax)',
    family_kind: 'woodwind_reed',
    description:
      'Reed-excited harmonic template emphasizing odd/nasal mid harmonics, smooth reed attack (~18 ms), broad nasal/reed presence contour (~1.4–2.6 kHz), and gentle high rolloff.',
  },
  brass_bright_bell: {
    model_id: 'brass_bright_bell',
    model_label: 'Modeled Bright Brass Bell (Trumpet / Trombone)',
    family_kind: 'brass_bright',
    description:
      'Lip-reed brass harmonic structure with stronger mid-upper partials (h2–h9) that brighten dynamically with cue level, crisp tongued attack (~12 ms), and focused bell presence (~1.8–3.2 kHz).',
  },
  brass_warm_horn: {
    model_id: 'brass_warm_horn',
    model_label: 'Modeled Warm Conical Brass (Horn / Euphonium / Tuba)',
    family_kind: 'brass_warm',
    description:
      'Warm conical-bore brass harmonic rolloff, rounded low-mid fullness (~220–420 Hz), smoother bell brilliance (~1.1–2.0 kHz), and gentle upper rolloff above 4.5 kHz.',
  },
  voice_male_formant: {
    model_id: 'voice_male_formant',
    model_label: 'Modeled Male Vocal Glottal + Formant Contour',
    family_kind: 'voice_male',
    description:
      'Glottal-like harmonic slope shaped through broad male vocal-tract formant regions (chest ~160 Hz, F1 ~500 Hz, F2 ~1350 Hz, singer/presence F3 ~2700 Hz), smooth syllabic attack (~20 ms), and subtle cue-modulated breathiness.',
  },
  voice_female_formant: {
    model_id: 'voice_female_formant',
    model_label: 'Modeled Female Vocal Glottal + Formant Contour',
    family_kind: 'voice_female',
    description:
      'Glottal-like harmonic slope shaped through broad female vocal-tract formant regions (F1 ~680 Hz, F2 ~1650 Hz, presence/brilliance F3 ~3100 Hz), smooth lyrical attack (~22 ms), and subtle cue-modulated vocal air.',
  },
  voice_treble_ensemble: {
    model_id: 'voice_treble_ensemble',
    model_label: 'Modeled Vocal Ensemble / Treble Formant Contour',
    family_kind: 'voice_ensemble',
    description:
      'Smooth blended glottal harmonic profile with wide neutral vocal formant contour (F1 ~580 Hz, F2 ~1500 Hz, air ~3000 Hz) and gentle sustain smoothing.',
  },
  keyboard_struck_hammer: {
    model_id: 'keyboard_struck_hammer',
    model_label: 'Modeled Struck String / Tine Keyboard (Piano / Rhodes)',
    family_kind: 'keyboard_struck',
    description:
      'Crisp hammer/tine attack (~6 ms), balanced harmonic series with progressive high-partial decay, broad soundboard warmth (~180–320 Hz), and articulate hammer clarity (~2.4 kHz).',
  },
  keyboard_reed_organ: {
    model_id: 'keyboard_reed_organ',
    model_label: 'Modeled Sustained Organ / Free-Reed Keyboard',
    family_kind: 'keyboard_organ',
    description:
      'Sustained steady-state harmonic drawbar/reed weighting with minimal mid-note decay, crisp key-on ramp (~10 ms), and broad reed midrange body (~600–2200 Hz).',
  },
  percussion_resonant: {
    model_id: 'percussion_resonant',
    model_label: 'Modeled Resonant Percussion / Mallet',
    family_kind: 'percussion',
    description:
      'Fast stick/mallet impact transient (~3 ms) with rapid upper-harmonic decay, warm shell/bar body resonance (~150–350 Hz), and crisp attack snap (~3.0–5.0 kHz).',
  },
  amp_cabinet_voiced: {
    model_id: 'amp_cabinet_voiced',
    model_label: 'Modeled Guitar / Bass Amplifier Cabinet Contour',
    family_kind: 'amp_cabinet',
    description:
      'Plucked-string excitation passed through a broad speaker-cabinet contour (cabinet low thump ~120 Hz, cone midrange push ~1.3–2.6 kHz, smooth speaker rolloff above 5 kHz).',
  },
};

function makeEntry({
  category,
  family,
  name,
  symbol,
  range,
  radiation,
  pitched = true,
  practical,
  body,
  presence,
  air,
  formantNote,
  notes,
  refs,
  regenKey,
  extraUncertainties = [],
}) {
  return {
    id: makeId(family, name),
    name,
    category,
    family,
    symbol,
    radiation,
    published_range_hz: range,
    published_fundamental_range_hz: range,
    published_fundamental_notes: formatCompass(range, pitched),
    published_practical_range_hz: practical,
    spectral_regions: {
      body_warmth_hz: body,
      presence_attack_hz: presence,
      air_high_hz: air,
      formants_and_resonances_note: formantNote,
    },
    published_notes: notes,
    source_references: refs,
    uncertainties: [...BASE_UNCERTAINTIES, ...extraUncertainties],
    regen_model: REGEN_MODELS[regenKey],
  };
}

const entries = [];

// 1. Unknown (1)
entries.push(
  makeEntry({
    category: 'Unknown',
    family: 'Unknown',
    name: 'Unknown / Unspecified Source',
    symbol: 'unknown',
    range: null,
    radiation: 'omni',
    pitched: false,
    practical: null,
    body: null,
    presence: null,
    air: null,
    formantNote:
      'No instrument or vocal source specified; no formant, body resonance, or harmonic bias assumed.',
    notes: 'Neutral omnidirectional radiation estimate; no instrument assumptions applied.',
    refs: ['SpectraCraft neutral reference baseline (no external instrument assumption)'],
    regenKey: 'neutral_cue_modal',
    extraUncertainties: [
      'Source type is unspecified; fundamental range and spectral character regions are intentionally null.',
    ],
  }),
);

// 2. Strings — Plucked & Fretted (22)
const plucked = [
  ['Electric Guitar (Direct / Unamped Body)', [82, 1319], 'forward_wide', 'plucked_electric_string', [100, 300], [1500, 4000], [4500, 8000], [82, 6500], 'Magnetic pickup & string harmonic content centers warmth in 100–300 Hz and pick/fret attack in 1.5–4 kHz; exact pickup resonance depends on coil inductance and cable capacitance.'],
  ['Electric Guitar (Clean 1x12 Open-Back Combo)', [82, 5500], 'dipole_open_back', 'amp_cabinet_voiced', [100, 260], [1200, 3500], [4000, 5500], [82, 5500], '12-inch guitar speaker cone emphasizes 1–3.5 kHz midrange and rolls off naturally above ~5.5 kHz; open back radiates rear dipole energy.'],
  ['Electric Guitar (Closed-Back 4x12 Cab)', [82, 5200], 'forward_cardioid', 'amp_cabinet_voiced', [95, 240], [1000, 3200], [3800, 5200], [82, 5200], 'Tight closed-back cabinet resonance reinforces 95–240 Hz low-end punch with directional cone presence around 1–3.2 kHz and steep rolloff above ~5.2 kHz.'],
  ['Electric Guitar (7-String)', [62, 1319], 'forward_wide', 'plucked_electric_string', [80, 250], [1400, 3800], [4500, 8000], [62, 6500], 'Extended low B1 fundamental (~62 Hz) adds sub-body weight while pick attack remains in 1.4–3.8 kHz.'],
  ['Electric Guitar (Baritone)', [62, 988], 'forward_wide', 'plucked_electric_string', [80, 250], [1200, 3400], [4000, 7500], [62, 6000], 'Longer scale length yields strong low-string fundamentals (~62–250 Hz) and piano-like upper partials.'],
  ['Acoustic Guitar (Dreadnought Steel)', [82, 1175], 'forward_cardioid', 'plucked_acoustic_wood', [100, 250], [2000, 5000], [6000, 12000], [82, 12000], 'Large dreadnought soundbox produces strong air/top body warmth around 100–250 Hz, with steel-string pick brilliance in 2–5 kHz and string air up to 12 kHz.'],
  ['Acoustic Guitar (OM / Auditorium)', [82, 1175], 'forward_cardioid', 'plucked_acoustic_wood', [110, 260], [2000, 5000], [6000, 12000], [82, 12000], 'Balanced medium-body acoustic guitar with focused 110–260 Hz warmth, clear midrange projection, and steel-string shimmer in 2–5 kHz.'],
  ['Acoustic Guitar (Parlor)', [82, 1175], 'forward_wide', 'plucked_acoustic_wood', [130, 300], [1800, 4800], [5500, 11000], [82, 11000], 'Smaller parlor soundbox shifts body warmth slightly higher (~130–300 Hz) with intimate midrange focus.'],
  ['Acoustic Guitar (12-String)', [82, 1319], 'forward_cardioid', 'plucked_acoustic_wood', [110, 260], [2200, 5500], [6500, 14000], [82, 14000], 'Octave-paired courses enrich 2.2–5.5 kHz shimmer and 6.5–14 kHz high-frequency chime above the wooden body warmth.'],
  ['Classical Nylon Guitar', [82, 988], 'forward_cardioid', 'plucked_acoustic_wood', [100, 250], [1200, 3200], [4500, 9000], [82, 9000], 'Nylon strings produce rounder upper harmonics than steel, with warm air/top body coupling (~100–250 Hz) and softer finger-nail attack (~1.2–3.2 kHz).'],
  ['Flamenco Guitar', [82, 988], 'forward_cardioid', 'plucked_acoustic_wood', [110, 260], [1600, 4200], [5000, 10000], [82, 10000], 'Lighter cypress-style build emphasizes fast percussive attack and rasgueado definition (~1.6–4.2 kHz) with tighter low-end decay.'],
  ['Archtop Jazz Guitar (Acoustic)', [82, 1175], 'forward_cardioid', 'plucked_acoustic_wood', [120, 280], [1400, 3600], [4500, 9500], [82, 9500], 'Carved archtop plate projects focused upper-bass and lower-midrange punch (~120–280 Hz and 1.4–3.6 kHz) with dry decay.'],
  ['Resonator Guitar', [82, 1175], 'directional_bell', 'plucked_acoustic_wood', [140, 320], [1500, 4500], [5000, 11000], [82, 11000], 'Spun metal resonator cone projects pronounced midrange nasal/metallic presence (~1.5–4.5 kHz) with directional forward projection.'],
  ['Bass Guitar (4-String)', [41, 392], 'omni', 'plucked_bass_string', [60, 200], [700, 2200], [2500, 5000], [41, 5000], 'Fundamental weight lives in 41–200 Hz with low-mid body warmth; finger/pick articulation and fret click sit around 700 Hz–2.2 kHz.'],
  ['Bass Guitar (5-String)', [31, 392], 'omni', 'plucked_bass_string', [50, 180], [700, 2200], [2500, 5000], [31, 5000], 'Low B0 (~31 Hz) extends sub-fundamental weight down to 31 Hz while 2nd/3rd harmonics carry pitch clarity in 60–200 Hz.'],
  ['Acoustic Bass Guitar', [41, 392], 'forward_wide', 'plucked_bass_string', [70, 220], [800, 2500], [3000, 6000], [41, 6000], 'Acoustic bass box emphasizes upper-fundamental warmth (70–220 Hz) and bronze string attack (800 Hz–2.5 kHz).'],
  ['Mandolin (F-Style)', [196, 1568], 'forward_wide', 'plucked_acoustic_wood', [200, 450], [2000, 5500], [6000, 12000], [196, 12000], 'Paired steel courses and small carved chamber focus warmth in 200–450 Hz and crisp chop/pick transients in 2–5.5 kHz.'],
  ['Mandola', [131, 1047], 'forward_wide', 'plucked_acoustic_wood', [150, 360], [1600, 4500], [5000, 10000], [131, 10000], 'Alto-register double-course instrument pitched a fifth below mandolin, combining woody 150–360 Hz warmth with clear pick attack.'],
  ['Banjo (5-String Open-Back)', [147, 1175], 'dipole_open_back', 'plucked_acoustic_wood', [180, 400], [1800, 5000], [5500, 11000], [147, 11000], 'Stretched drumhead membrane produces fast transient snap (~1.8–5 kHz) and open-back plunky warmth (~180–400 Hz).'],
  ['Banjo (Resonator)', [147, 1175], 'forward_cardioid', 'plucked_acoustic_wood', [180, 400], [2000, 5500], [6000, 12000], [147, 12000], 'Rear wooden resonator reflects membrane energy forward, boosting cutting midrange projection (~2–5.5 kHz).'],
  ['Ukulele (Soprano / Concert)', [262, 1047], 'forward_wide', 'plucked_acoustic_wood', [260, 550], [1500, 4000], [4500, 9000], [262, 9000], 'Small nylon-strung body concentrates fundamental warmth in 260–550 Hz with light, fast strumming transients in 1.5–4 kHz.'],
  ['Concert Pedal Harp', [33, 3136], 'dipole_open_back', 'plucked_acoustic_wood', [80, 280], [1500, 4500], [5000, 12000], [33, 12000], 'Wide 6.5-octave compass with resonant tapered soundboard warmth (80–280 Hz) and delicate finger-pluck shimmer (1.5–4.5 kHz).'],
];
for (const [name, range, rad, regenKey, body, presence, air, practical, formantNote] of plucked) {
  const sym = name.includes('Combo') || name.includes('Cab') ? 'amp' : 'guitar';
  entries.push(
    makeEntry({
      category: 'Instrument',
      family: 'Strings — Plucked & Fretted',
      name,
      symbol: sym,
      range,
      radiation: rad,
      pitched: !name.includes('Combo') && !name.includes('Cab'),
      practical,
      body,
      presence,
      air,
      formantNote,
      notes: 'Published fundamental range (informational only; upper harmonics & noise are never truncated).',
      refs: REFS_PLUCKED_AMP,
      regenKey,
    }),
  );
}

// 3. Strings — Bowed (11)
const bowed = [
  ['Violin (Solo)', [196, 3136], 'forward_wide', 'bowed_violin_high', [200, 500], [2000, 4500], [6000, 12000], [196, 12000], 'Published violin acoustics show warm air/main-wood body region (~200–500 Hz) and bright bridge-hill/bow-presence region (~2–4.5 kHz); exact Stradivari/Guarneri plate modes vary per violin and are not invented.'],
  ['Viola', [131, 1319], 'forward_wide', 'bowed_cello_low', [150, 420], [1500, 3500], [5000, 10000], [131, 10000], 'Darker, throatier bowed body than violin with main body warmth around 150–420 Hz and velvety upper-mid bow definition (~1.5–3.5 kHz).'],
  ['Cello', [65, 880], 'forward_cardioid', 'bowed_cello_low', [100, 280], [1400, 3500], [4500, 9000], [65, 9000], 'Rich low-string Helmholtz/wood body warmth (~100–280 Hz), vocal-like upper-mid lyricism (~1.4–3.5 kHz), and rosin bow texture extending above 4.5 kHz; exact individual body modes vary per instrument.'],
  ['Double Bass (Pizzicato)', [41, 262], 'omni', 'plucked_bass_string', [60, 200], [600, 2000], [2500, 5000], [41, 5000], 'Deep acoustic upright fundamentals (41–200 Hz) with woody fingerboard growl and pluck definition around 600 Hz–2 kHz.'],
  ['Double Bass (Arco)', [41, 392], 'forward_wide', 'bowed_cello_low', [60, 220], [700, 2200], [2500, 6000], [41, 6000], 'Sustained bowed fundamentals with massive low-end body resonance (60–220 Hz) and textured horsehair rosin bite (700 Hz–2.2 kHz).'],
  ['Baroque Violin', [196, 2637], 'forward_wide', 'bowed_violin_high', [200, 480], [1800, 4000], [5500, 10500], [196, 10500], 'Gut strings and lower bridge tension yield softer high-frequency edge and transparent woody midrange compared to modern steel-wound strings.'],
  ['Hardanger Fiddle', [196, 2637], 'forward_wide', 'bowed_violin_high', [220, 500], [2000, 4800], [6000, 12000], [196, 12000], 'Under-strings add sympathetic shimmering overtone sustain in the 2–6 kHz region above the bowed melody strings.'],
  ['Viola da Gamba', [73, 587], 'forward_wide', 'bowed_cello_low', [110, 300], [1200, 3200], [4000, 8500], [73, 8500], 'Fretted gut-strung viol family produces clear, reedy upper harmonics with lighter low-body boom than a modern cello.'],
  ['Erhu', [294, 1760], 'forward_cardioid', 'bowed_violin_high', [300, 650], [1500, 4000], [5000, 10000], [294, 10000], 'Python-skin membrane soundbox yields focused, vocal-like nasal formants in 1.5–4 kHz with expressive continuous pitch slides.'],
  ['Nyckelharpa', [131, 1568], 'forward_wide', 'bowed_violin_high', [160, 420], [1600, 4200], [5000, 10500], [131, 10500], 'Keyed bowed fiddle with sympathetic strings producing woody midrange warmth and lingering resonant upper-harmonic halo.'],
  ['Hurdy-Gurdy', [131, 1047], 'omni', 'bowed_cello_low', [150, 400], [1200, 3600], [4500, 9000], [131, 9000], 'Continuous rosined-wheel excitation creates rich sawtooth-like drone harmonics and buzzing trompette bridge transients.'],
];
for (const [name, range, rad, regenKey, body, presence, air, practical, formantNote] of bowed) {
  entries.push(
    makeEntry({
      category: 'Instrument',
      family: 'Strings — Bowed',
      name,
      symbol: 'bowed',
      range,
      radiation: rad,
      practical,
      body,
      presence,
      air,
      formantNote,
      notes: 'Published bowed-string fundamental range; bow noise & body resonances span higher frequencies.',
      refs: REFS_ORCHESTRAL,
      regenKey,
    }),
  );
}

// 4. Winds — Woodwinds (20)
const woodwinds = [
  ['Concert Flute (C)', [262, 2093], 'forward_wide', 'woodwind_flute', [260, 600], [2000, 4500], [6000, 12000], [262, 12000], 'Fundamental-dominant open cylindrical tube; lower register has gentle 2nd/3rd harmonics while upper register approaches pure sinusoid plus embouchure breath air (3–10 kHz).'],
  ['Piccolo', [587, 4186], 'forward_wide', 'woodwind_flute', [600, 1200], [2500, 6000], [7000, 14000], [587, 14000], 'Highest orchestral woodwind with piercing pure-tone fundamentals above 1 kHz and crisp breath/articulation air.'],
  ['Alto Flute', [196, 1568], 'forward_wide', 'woodwind_flute', [200, 500], [1500, 3800], [5000, 10000], [196, 10000], 'Wider bore yields warm, breathy low-register fundamentals (200–500 Hz) with softer upper-harmonic energy.'],
  ['Bass Flute', [131, 1047], 'forward_wide', 'woodwind_flute', [140, 400], [1200, 3200], [4500, 9500], [131, 9500], 'Deep velvety low fundamentals (140–400 Hz) accompanied by prominent embouchure breath texture.'],
  ['Irish Tin Whistle', [587, 2349], 'forward_wide', 'woodwind_flute', [580, 1100], [2200, 5000], [6000, 12000], [587, 12000], 'Fipple edge tone with clear bright fundamentals and airy chiff transient at note onset.'],
  ['Soprano Recorder', [523, 2093], 'forward_wide', 'woodwind_flute', [520, 1000], [2000, 4500], [5500, 11000], [523, 11000], 'Pure fipple tone with restrained upper harmonics and gentle onset chiff.'],
  ['Alto Recorder', [349, 1568], 'forward_wide', 'woodwind_flute', [350, 750], [1600, 4000], [5000, 10000], [349, 10000], 'Warm wooden fipple fundamentals (350–750 Hz) with rounded low-order harmonics.'],
  ['Bansuri Bamboo Flute', [262, 1568], 'forward_wide', 'woodwind_flute', [260, 650], [1500, 3800], [4500, 10000], [262, 10000], 'Warm organic bamboo bore with expressive breath harmonics and smooth microtonal glide character.'],
  ['Shakuhachi', [294, 1760], 'forward_wide', 'woodwind_flute', [290, 700], [1600, 4500], [5000, 12000], [294, 12000], 'End-blown bamboo bore combining pure tonal fundamentals with wide, expressive breath-jet broadband noise.'],
  ['Pan Flute', [294, 2093], 'forward_wide', 'woodwind_flute', [290, 700], [1800, 4500], [5500, 12000], [294, 12000], 'Closed-tube pipes favor odd harmonics at soft levels alongside characteristic airy attack transients.'],
  ['Oboe', [233, 1568], 'forward_cardioid', 'woodwind_reed', [250, 600], [1200, 3500], [4500, 10000], [233, 10000], 'Conical double-reed bore produces strong vocal-like nasal formant region around ~1.2–3.5 kHz that cuts cleanly through ensembles.'],
  ['English Horn (Cor Anglais)', [165, 1047], 'forward_cardioid', 'woodwind_reed', [200, 500], [1000, 2800], [4000, 8500], [165, 8500], 'Pear-shaped bell and double reed emphasize melancholic lower-formant warmth around 1–2.8 kHz.'],
  ['Bb Clarinet', [147, 1568], 'forward_cardioid', 'woodwind_reed', [160, 450], [1200, 3200], [4500, 9500], [147, 9500], 'Closed cylindrical bore strongly suppresses even harmonics in the chalumeau register, yielding hollow woody warmth and smooth clarion upper presence.'],
  ['A Clarinet', [139, 1480], 'forward_cardioid', 'woodwind_reed', [150, 430], [1150, 3100], [4500, 9500], [139, 9500], 'Slightly longer cylindrical bore than Bb clarinet with dark odd-harmonic chalumeau warmth.'],
  ['Bass Clarinet', [73, 784], 'forward_cardioid', 'woodwind_reed', [85, 280], [800, 2400], [3200, 7000], [73, 7000], 'Deep resonant odd-harmonic low register (73–280 Hz) with reedy midrange presence and mechanical key/pad texture.'],
  ['Bassoon', [58, 622], 'forward_wide', 'woodwind_reed', [80, 300], [900, 2500], [3500, 7500], [58, 7500], 'Folded conical double-reed bore shows strong warm mid-bass formant region (~300–600 Hz) and reedy tenor definition (~1–2.5 kHz).'],
  ['Contrabassoon', [29, 262], 'omni', 'woodwind_reed', [40, 160], [400, 1400], [2000, 4500], [29, 4500], 'Sub-octave double reed extending down to Bb0 (~29 Hz) with dense low-frequency reed rattle and buzzing overtones.'],
  ['Soprano Saxophone', [208, 1245], 'directional_bell', 'woodwind_reed', [220, 550], [1400, 3600], [4500, 10000], [208, 10000], 'Straight conical single-reed bore produces bright, oboe-adjacent upper-mid projection (~1.4–3.6 kHz).'],
  ['Alto Saxophone', [139, 831], 'directional_bell', 'woodwind_reed', [180, 480], [1200, 3200], [4500, 9500], [139, 9500], 'Conical brass body with single reed generates balanced even and odd harmonics with expressive presence in 1.2–3.2 kHz.'],
  ['Tenor Saxophone', [104, 622], 'directional_bell', 'woodwind_reed', [130, 380], [1000, 2800], [4000, 8500], [104, 8500], 'Warm smoky low-mid fullness (~130–380 Hz) paired with breathy reed edge (~1–2.8 kHz).'],
];
for (const [name, range, rad, regenKey, body, presence, air, practical, formantNote] of woodwinds) {
  entries.push(
    makeEntry({
      category: 'Instrument',
      family: 'Winds — Woodwinds',
      name,
      symbol: 'wind',
      range,
      radiation: rad,
      practical,
      body,
      presence,
      air,
      formantNote,
      notes: 'Published woodwind compass; embouchure breath and key clicks extend well above fundamental.',
      refs: REFS_ORCHESTRAL,
      regenKey,
    }),
  );
}

// 5. Winds — Brass (14)
const brass = [
  ['Bb Trumpet', [165, 988], 'directional_bell', 'brass_bright_bell', [200, 500], [1500, 4000], [5000, 11000], [165, 11000], 'Lip-reed excitation produces rich upper harmonics that expand nonlinearly at loud dynamics, with brilliant directional bell presence (~1.5–4 kHz).'],
  ['C Trumpet', [185, 1047], 'directional_bell', 'brass_bright_bell', [220, 520], [1600, 4200], [5200, 11500], [185, 11500], 'Slightly shorter orchestral trumpet bore delivering focused, bright upper-harmonic projection.'],
  ['Piccolo Trumpet', [294, 1568], 'directional_bell', 'brass_bright_bell', [320, 700], [2000, 5000], [6000, 13000], [294, 13000], 'High baroque-register brass instrument with crystalline upper-harmonic clarity and tight directional bell beam.'],
  ['Cornet', [165, 988], 'directional_bell', 'brass_warm_horn', [200, 500], [1200, 3200], [4500, 9500], [165, 9500], 'More conical tubing than trumpet softens high-frequency brass edge for warmer lyrical midrange.'],
  ['Flugelhorn', [165, 932], 'directional_bell', 'brass_warm_horn', [180, 480], [900, 2500], [3500, 7500], [165, 7500], 'Wide conical bore and deep mouthpiece emphasize mellow 180–480 Hz warmth with rolled-off upper brass bite.'],
  ['French Horn', [62, 698], 'forward_cardioid', 'brass_warm_horn', [150, 450], [800, 2200], [3200, 7000], [62, 7000], 'Long conical tubing and hand-in-bell acoustics yield warm golden midrange resonance (~150–450 Hz) and rich brass bloom when pushed.'],
  ['Tenor Trombone', [82, 587], 'directional_bell', 'brass_bright_bell', [120, 380], [1000, 3000], [4000, 9000], [82, 9000], 'Cylindrical slide bore produces strong fundamental warmth (120–380 Hz) and brassy upper-harmonic edge (~1–3 kHz) at forte.'],
  ['Bass Trombone', [58, 466], 'directional_bell', 'brass_bright_bell', [90, 300], [800, 2400], [3200, 7500], [58, 7500], 'Larger bore and bell deepen pedal/low-register brass weight while preserving metallic brass presence.'],
  ['Alto Trombone', [110, 698], 'directional_bell', 'brass_bright_bell', [150, 420], [1200, 3400], [4500, 9500], [110, 9500], 'Brighter, lighter orchestral trombone register sitting between horn and tenor trombone.'],
  ['Euphonium', [58, 466], 'directional_bell', 'brass_warm_horn', [100, 320], [700, 1900], [2800, 6000], [58, 6000], 'Wide conical bore gives dark, round low-brass lyricism with gentle upper harmonics.'],
  ['Baritone Horn', [65, 466], 'directional_bell', 'brass_warm_horn', [110, 340], [800, 2100], [3000, 6500], [65, 6500], 'Medium conical bore offers slightly brighter midrange articulation than euphonium.'],
  ['BBb Tuba', [29, 311], 'forward_wide', 'brass_warm_horn', [50, 180], [400, 1200], [1800, 4000], [29, 4000], 'Foundation orchestral bass instrument with broad low-frequency warmth (50–180 Hz) and smooth low-mid projection.'],
  ['Sousaphone', [29, 311], 'directional_bell', 'brass_warm_horn', [55, 190], [450, 1400], [2000, 4500], [29, 4500], 'Forward-facing marching tuba bell focuses low-mid punch (~150–600 Hz) directly toward the front axis.'],
  ['Baritone Saxophone (Brass-section role)', [69, 415], 'directional_bell', 'woodwind_reed', [90, 280], [800, 2400], [3200, 7000], [69, 7000], 'Large single-reed conical brass body combining deep 69–280 Hz foundation with gritty reed bark (~800 Hz–2.4 kHz).'],
];
for (const [name, range, rad, regenKey, body, presence, air, practical, formantNote] of brass) {
  entries.push(
    makeEntry({
      category: 'Instrument',
      family: 'Winds — Brass',
      name,
      symbol: 'brass',
      range,
      radiation: rad,
      practical,
      body,
      presence,
      air,
      formantNote,
      notes: 'Published brass fundamental range; bell directivity narrows at upper harmonics.',
      refs: REFS_ORCHESTRAL,
      regenKey,
    }),
  );
}

// 6. Percussion (24)
const percussion = [
  ['Snare Drum (Wood Shell)', [140, 4500], 'omni', false, [150, 300], [1500, 4500], [5000, 12000], [120, 12000], 'Wood shell warmth centers near 150–300 Hz while stick crack and snare wires span 1.5–12 kHz.'],
  ['Snare Drum (Metal Shell)', [160, 5500], 'omni', false, [160, 320], [1800, 5500], [6000, 14000], [140, 14000], 'Metal shell adds sharper ringing overtones and brighter snare wire sizzle up to 14 kHz.'],
  ['Kick Drum (22-inch Ported)', [40, 3500], 'forward_cardioid', false, [50, 110], [2000, 4500], [5000, 8000], [35, 8000], 'Low thump sits in 50–110 Hz with hollowed low-mid boxiness (~300–500 Hz) and beater click in 2–4.5 kHz.'],
  ['Kick Drum (20-inch Jazz)', [50, 2800], 'omni', false, [65, 140], [1500, 3500], [4000, 7000], [45, 7000], 'Higher-tuned resonant front head produces open tonal boom (65–140 Hz) and warm felt beater attack.'],
  ['Rack Tom (12-inch)', [95, 2200], 'omni', false, [100, 240], [1800, 4500], [5000, 9000], [90, 9000], 'Clear drumhead fundamental around 100–240 Hz with stick attack in 1.8–4.5 kHz.'],
  ['Floor Tom (16-inch)', [65, 1800], 'omni', false, [70, 160], [1500, 4000], [4500, 8000], [60, 8000], 'Deep shell resonance (70–160 Hz) and wide sustain tail with stick definition above 1.5 kHz.'],
  ['Hi-Hat (Closed / Chick)', [300, 16000], 'omni', false, [300, 800], [3000, 7000], [8000, 16000], [300, 16000], 'Dense inharmonic metallic shimmer concentrated from 3–16 kHz with crisp stick transient.'],
  ['Ride Cymbal', [220, 16000], 'omni', false, [250, 700], [2500, 6500], [7500, 16000], [220, 16000], 'Metallic wash across 250 Hz–16 kHz with focused bell ping and stick-tip articulation around 2.5–6.5 kHz.'],
  ['Crash Cymbal', [250, 16000], 'omni', false, [300, 800], [2500, 7000], [8000, 16000], [250, 16000], 'Broadband explosive inharmonic splash peaking in upper midrange and high shimmer (2.5–16 kHz).'],
  ['Overhead Drum Kit (Full)', [40, 16000], 'omni', false, [60, 250], [1500, 5000], [6000, 16000], [40, 16000], 'Full-spectrum acoustic drum ensemble spanning sub-kick thump to cymbal air.'],
  ['Timpani', [65, 392], 'omni', true, [70, 220], [800, 2500], [3000, 6000], [65, 6000], 'Tuned kettle membrane aligns overtones into perceived pitch (65–392 Hz) with warm felt mallet boom.'],
  ['Concert Bass Drum', [35, 250], 'dipole_open_back', false, [35, 110], [400, 1500], [2000, 4500], [30, 4500], 'Deep orchestral low-frequency energy (35–110 Hz) with broad reverberant bloom.'],
  ['Marimba (5-Octave)', [65, 2093], 'omni', true, [100, 400], [1000, 3500], [4000, 8000], [65, 8000], 'Rosewood/padauk bars tuned with 4th-harmonic overtone and tubular resonators yielding warm woody fundamentals.'],
  ['Vibraphone', [175, 1397], 'omni', true, [180, 450], [1200, 3500], [4000, 9000], [175, 9000], 'Aluminum bars produce long pure fundamental sustain with metallic mallet contact sheen.'],
  ['Xylophone', [349, 4186], 'omni', true, [350, 900], [2000, 5500], [6000, 12000], [349, 12000], 'Hard wooden bars tuned with bright 3rd-harmonic overtone and short, cutting percussive attack.'],
  ['Glockenspiel', [784, 4186], 'omni', true, [800, 1600], [2500, 6500], [7000, 15000], [784, 15000], 'Steel bars produce bell-like high fundamentals and shimmering inharmonic upper partials.'],
  ['Tubular Bells (Chimes)', [262, 1047], 'omni', true, [260, 700], [1500, 4500], [5000, 12000], [262, 12000], 'Strike tone arises from complex inharmonic metallic overtones with long ringing sustain.'],
  ['Cajon', [60, 4000], 'forward_cardioid', false, [70, 180], [1500, 4500], [5000, 10000], [60, 10000], 'Wooden box port delivers warm bass thump (70–180 Hz) while front tapa and snares give slap snap (1.5–4.5 kHz).'],
  ['Djembe', [80, 3500], 'forward_wide', false, [90, 220], [1200, 4000], [4500, 9000], [80, 9000], 'Goblet shell delivers deep bass tone, clear mid tone, and high-frequency hand-slap crack.'],
  ['Congas', [110, 2200], 'forward_wide', false, [120, 300], [1000, 3200], [3800, 8000], [110, 8000], 'Thick wood staves and rawhide head emphasize warm resonant mid-bass tones (120–300 Hz) and palm slap.'],
  ['Bongos', [180, 3000], 'forward_wide', false, [200, 450], [1400, 4200], [4500, 9500], [180, 9500], 'High-pitched paired hand drums with punchy woody attacks in 200 Hz–4.2 kHz.'],
  ['Tabla', [120, 3500], 'omni', false, [120, 350], [1200, 4000], [4500, 10000], [120, 10000], 'Weighted syahi membrane creates harmonic pitched resonance on dayan paired with bendable bayan bass.'],
  ['Frame Drum / Bodhran', [70, 2500], 'dipole_open_back', false, [80, 220], [900, 3000], [3500, 8000], [70, 8000], 'Open-backed goatskin drumhead yields warm pitch-modulated low thump and crisp tipper articulation.'],
  ['Tambourine / Shaker', [400, 16000], 'omni', false, [400, 1000], [3000, 8000], [8500, 16000], [400, 16000], 'High-frequency rhythmic shimmer concentrated in 3–16 kHz jingle/bead energy.'],
];
for (const [name, range, rad, pitched, body, presence, air, practical, formantNote] of percussion) {
  entries.push(
    makeEntry({
      category: 'Instrument',
      family: 'Percussion',
      name,
      symbol: 'percussion',
      range,
      radiation: rad,
      pitched,
      practical,
      body,
      presence,
      air,
      formantNote,
      notes: 'Published spectral reference; transient impact energy spans full audio bandwidth.',
      refs: REFS_PERCUSSION,
      regenKey: 'percussion_resonant',
    }),
  );
}

// 7. Keyboards & Free-Reed (13)
const keys = [
  ['Concert Grand Piano (Lid Open)', [28, 4186], 'forward_wide', 'keyboard_struck_hammer', [80, 280], [2000, 5000], [6000, 12000], [28, 12000], 'Full 88-key compass (A0–C8) with rich soundboard warmth (80–280 Hz), felt hammer articulation (2–5 kHz), and slight string inharmonicity.'],
  ['Baby Grand Piano', [28, 4186], 'forward_wide', 'keyboard_struck_hammer', [90, 300], [1800, 4800], [5500, 11000], [28, 11000], 'Shorter bass strings yield slightly tighter low-end and prominent midrange projection compared to a 9-ft concert grand.'],
  ['Upright Piano', [28, 4186], 'dipole_open_back', 'keyboard_struck_hammer', [100, 320], [1600, 4500], [5000, 10000], [28, 10000], 'Vertical soundboard radiates strongly front and rear with intimate wooden midrange character.'],
  ['Harpsichord', [44, 2794], 'forward_wide', 'plucked_acoustic_wood', [100, 300], [2000, 6000], [6500, 14000], [44, 14000], 'Quill-plucked strings generate dense upper-harmonic sparkle (2–14 kHz) with minimal dynamic touch variation.'],
  ['Celesta', [262, 4186], 'forward_wide', 'keyboard_struck_hammer', [260, 650], [1800, 5000], [5500, 12000], [262, 12000], 'Felt-struck steel plates over wooden resonators produce bell-like pure fundamentals and delicate high sheen.'],
  ['Electric Piano (Rhodes / Tine Amp)', [41, 2093], 'forward_cardioid', 'keyboard_struck_hammer', [100, 320], [1200, 3500], [4000, 7500], [41, 7500], 'Warm sine-heavy fundamental body (100–320 Hz) with signature tine bell bark (~1.2–3.5 kHz) when struck hard.'],
  ['Electric Piano (Wurlitzer Reed)', [44, 2093], 'forward_wide', 'keyboard_struck_hammer', [120, 360], [1000, 3200], [3800, 7000], [44, 7000], 'Struck steel reeds create punchy midrange bark (~1–3.2 kHz) and tighter bass than tine pianos.'],
  ['Clavinet (Through Amp)', [44, 2637], 'forward_cardioid', 'plucked_electric_string', [120, 340], [1500, 4500], [5000, 9000], [44, 9000], 'Tangent-struck strings with magnetic pickups yield sharp funky midrange transients (~1.5–4.5 kHz).'],
  ['Tonewheel Organ (Rotary Speaker)', [33, 4186], 'omni', 'keyboard_reed_organ', [80, 300], [1000, 3500], [4000, 8000], [33, 8000], 'Additive harmonic drawbars produce sustained steady-state organ harmonics modulated by horn/rotor dispersion.'],
  ['Pipe Organ (Positive / Small)', [33, 4186], 'forward_wide', 'keyboard_reed_organ', [60, 260], [1200, 4000], [5000, 12000], [33, 12000], 'Flue and reed pipe ranks span wide sustained harmonic spectra with characteristic wind-chest chiff.'],
  ['Piano Accordion', [65, 2093], 'dipole_open_back', 'keyboard_reed_organ', [100, 350], [1200, 3800], [4500, 9500], [65, 9500], 'Bellows-driven free reeds generate rich sustained harmonics with bright musette/reed presence in 1.2–3.8 kHz.'],
  ['Bandoneon / Concertina', [73, 1760], 'dipole_open_back', 'keyboard_reed_organ', [120, 380], [1200, 3600], [4200, 9000], [73, 9000], 'Compact wooden free-reed body creates expressive nasal midrange presence and intimate bellows air.'],
  ['Diatonic / Chromatic Harmonica', [262, 2093], 'forward_cardioid', 'keyboard_reed_organ', [280, 700], [1500, 4500], [5000, 10000], [262, 10000], 'Hand-cupped free-reed acoustics allow strong formant shaping across 1–4.5 kHz alongside breath articulation.'],
];
for (const [name, range, rad, regenKey, body, presence, air, practical, formantNote] of keys) {
  entries.push(
    makeEntry({
      category: 'Instrument',
      family: 'Keyboards & Free-Reed',
      name,
      symbol: 'keyboard',
      range,
      radiation: rad,
      practical,
      body,
      presence,
      air,
      formantNote,
      notes: 'Published keyboard compass; hammer, keybed, and reed transients extend beyond fundamentals.',
      refs: REFS_ORCHESTRAL,
      regenKey,
    }),
  );
}

// 8. Amplifiers & Cabinets (10)
const amps = [
  ['1x10 Open-Back Guitar Combo', [82, 5400], 'dipole_open_back', [110, 280], [1300, 3600], [4000, 5400], [82, 5400]],
  ['1x12 Open-Back Guitar Combo', [75, 5400], 'dipole_open_back', [95, 260], [1200, 3500], [4000, 5400], [75, 5400]],
  ['2x12 Open-Back Guitar Combo', [75, 5400], 'dipole_open_back', [90, 260], [1200, 3500], [4000, 5400], [75, 5400]],
  ['1x12 Closed-Back Guitar Cab', [75, 5400], 'forward_cardioid', [95, 250], [1100, 3400], [3800, 5400], [75, 5400]],
  ['2x12 Closed-Back Guitar Cab', [75, 5200], 'forward_cardioid', [90, 240], [1100, 3300], [3800, 5200], [75, 5200]],
  ['4x12 Slanted Guitar Cab', [70, 5000], 'directional_bell', [85, 230], [1000, 3200], [3600, 5000], [70, 5000]],
  ['1x15 Bass Combo', [38, 3500], 'forward_wide', [55, 180], [650, 2000], [2200, 3500], [38, 3500]],
  ['4x10 Bass Cabinet', [41, 4200], 'forward_cardioid', [55, 190], [750, 2400], [2600, 4200], [41, 4200]],
  ['8x10 Bass Cabinet', [38, 4000], 'forward_cardioid', [50, 180], [700, 2200], [2500, 4000], [38, 4000]],
  ['Acoustic Instrument Combo Amp', [55, 14000], 'forward_cardioid', [80, 250], [1800, 4800], [6000, 14000], [55, 14000]],
];
for (const [name, range, rad, body, presence, air, practical] of amps) {
  const isBass = name.includes('Bass');
  entries.push(
    makeEntry({
      category: 'Instrument',
      family: 'Amplifiers & Cabinets',
      name,
      symbol: 'amp',
      range,
      radiation: rad,
      pitched: false,
      practical,
      body,
      presence,
      air,
      formantNote: isBass
        ? 'Bass speaker enclosure reinforces low-end cabinet warmth (50–190 Hz) with punchy upper-bass articulation and natural speaker rolloff above 3.5–4.2 kHz.'
        : 'Instrument speaker cone shapes strong midrange presence (~1–3.6 kHz) and smooth high-frequency mechanical rolloff above ~5 kHz (except full-range tweeter acoustic combo).',
      notes: 'Published speaker passband estimate; open-back vs closed-back affects rear wall radiation.',
      refs: REFS_PLUCKED_AMP,
      regenKey: isBass ? 'plucked_bass_string' : 'amp_cabinet_voiced',
    }),
  );
}

// 9. Human — Male (8)
const male = [
  ['Male Bass Voice', [82, 330], [90, 240], [1800, 3400], [6000, 12000], [82, 12000], 'Deep chest resonance (90–240 Hz), lower vocal-tract formants F1/F2, singer presence cluster (~2.4–3.2 kHz), and consonant/breath air above 6 kHz.'],
  ['Male Baritone Voice', [98, 392], [110, 280], [2000, 3800], [6000, 12000], [98, 12000], 'Warm baritone chest body (110–280 Hz), expressive vowel formants (F1 ~400–700 Hz, F2 ~900–2100 Hz), and presence/singer cluster (~2.4–3.5 kHz).'],
  ['Male Tenor Voice', [131, 523], [140, 340], [2200, 4200], [6500, 12000], [131, 12000], 'Focused upper-chest and head balance with strong singer formant ring (~2.6–3.4 kHz) and clear intelligibility in 2.2–4.2 kHz.'],
  ['Male Countertenor Voice', [165, 698], [180, 420], [2400, 4500], [6500, 12500], [165, 12500], 'Alto/falsetto-register vocal fundamentals with pure upper harmonics and focused formant ring in 2.4–4.5 kHz.'],
  ['Male Spoken Voice (Broadcast)', [85, 255], [100, 250], [2000, 4500], [6000, 12000], [85, 12000], 'Conversational chest warmth (100–250 Hz), speech intelligibility consonants (2–4.5 kHz), and sibilance/breath (6–10 kHz).'],
  ['Male Narrator / Audiobook', [85, 280], [100, 260], [1800, 4200], [6000, 12000], [85, 12000], 'Close-miked spoken warmth with balanced articulation formants and natural mouth/breath detail.'],
  ['Male Percussive / Rap Vocal', [85, 350], [100, 260], [2200, 5000], [6500, 13000], [85, 13000], 'Strong rhythmic consonant plosives and sharp intelligibility presence (2.2–5 kHz) above chest weight.'],
  ['Male Whisper / Intimate Vocal', [100, 6000], [140, 350], [2000, 5500], [6000, 14000], [100, 14000], 'Unvoiced/breathy turbulent vocal-tract airflow emphasizes broadband formant bands (1–6 kHz) and high air rather than strong glottal fundamentals.'],
];
for (const [name, range, body, presence, air, practical, formantNote] of male) {
  entries.push(
    makeEntry({
      category: 'Human',
      family: 'Male',
      name,
      symbol: 'voice',
      range,
      radiation: 'forward_cardioid',
      pitched: !name.includes('Whisper'),
      practical,
      body,
      presence,
      air,
      formantNote,
      notes: 'Published vocal fundamental range; formants, breath, and sibilance extend to 12+ kHz.',
      refs: REFS_VOICE,
      regenKey: 'voice_male_formant',
    }),
  );
}

// 10. Human — Female (8)
const female = [
  ['Female Contralto Voice', [175, 698], [180, 400], [2200, 4500], [6500, 13000], [175, 13000], 'Rich dark female chest resonance (180–400 Hz) with warm lower vowel formants and articulate presence (2.2–4.5 kHz).'],
  ['Female Mezzo-Soprano Voice', [220, 880], [200, 450], [2400, 4800], [7000, 13500], [220, 13500], 'Balanced warm mid-register body (200–450 Hz) with bright lyrical vowel formants and crisp presence (2.4–4.8 kHz).'],
  ['Female Soprano Voice', [262, 1047], [240, 520], [2600, 5200], [7500, 14000], [262, 14000], 'High lyrical vocal fundamentals (C4–C6) with elevated vocal-tract formants (F1/F2/F3), luminous 2.6–5.2 kHz brilliance, and airy upper shimmer; exact formant peaks shift with vowel and pitch.'],
  ['Female Coloratura Soprano', [262, 1397], [250, 550], [2800, 5500], [8000, 14500], [262, 14500], 'Extended upper soprano compass (up to F6) where high fundamentals approach F1 tuning and produce pure flute-like brilliance.'],
  ['Female Spoken Voice (Broadcast)', [165, 350], [170, 360], [2400, 5000], [6500, 13000], [165, 13000], 'Clear spoken fundamentals (165–350 Hz), crisp consonant intelligibility (2.4–5 kHz), and delicate sibilance/air (6.5–13 kHz).'],
  ['Female Narrator / Audiobook', [165, 380], [170, 380], [2200, 4800], [6500, 13000], [165, 13000], 'Intimate spoken vocal warmth with transparent articulation and natural breath cadence.'],
  ['Female Belt / Contemporary Lead', [196, 988], [200, 460], [2500, 5200], [7000, 14000], [196, 14000], 'Strong chest-dominant harmonic energy in 1–4.5 kHz with powerful upper-mid presence and crisp air.'],
  ['Female Whisper / Intimate Vocal', [180, 7000], [200, 450], [2400, 6000], [7000, 15000], [180, 15000], 'Airy turbulent breath formants spanning 1.5–7 kHz with delicate high-frequency sibilance.'],
];
for (const [name, range, body, presence, air, practical, formantNote] of female) {
  entries.push(
    makeEntry({
      category: 'Human',
      family: 'Female',
      name,
      symbol: 'voice',
      range,
      radiation: 'forward_cardioid',
      pitched: !name.includes('Whisper'),
      practical,
      body,
      presence,
      air,
      formantNote,
      notes: 'Published vocal fundamental range; formants, breath, and sibilance extend to 12+ kHz.',
      refs: REFS_VOICE,
      regenKey: 'voice_female_formant',
    }),
  );
}

// 11. Human — Other / Ensemble (7)
const ensemble = [
  ['Child / Treble Voice', [262, 1047], 'forward_cardioid', 'voice_treble_ensemble', [260, 560], [2600, 5400], [7500, 14000], [262, 14000], 'Shorter vocal tract shifts vowel formants higher than adult voices, giving clear pure upper-mid brightness.'],
  ['Non-Binary / Androgynous Voice', [125, 660], 'forward_cardioid', 'voice_treble_ensemble', [140, 360], [2100, 4600], [6500, 13000], [125, 13000], 'Broad mid-register vocal compass spanning classic tenor/alto overlap with balanced formant warmth and intelligibility.'],
  ['Vocal Duo / Harmony Pair', [110, 880], 'forward_wide', 'voice_treble_ensemble', [140, 380], [2000, 4800], [6500, 13500], [110, 13500], 'Two blended voices spanning combined chest warmth and overlapping formant presence; approximated as one virtual point source.'],
  ['A Cappella Quartet', [82, 1047], 'forward_wide', 'voice_treble_ensemble', [100, 350], [1800, 4500], [6000, 13000], [82, 13000], 'SATB vocal blend spanning bass through soprano fundamentals with smooth choral formant fusion.'],
  ['Chamber Choir (SATB)', [82, 1047], 'forward_wide', 'voice_treble_ensemble', [100, 360], [1600, 4200], [6000, 13000], [82, 13000], 'Multi-voice choral blend smooths individual vowel peaks into a broad warm harmonic sheen.'],
  ['String Quartet (Ensemble)', [65, 3136], 'omni', 'bowed_cello_low', [100, 350], [1500, 4200], [5500, 12000], [65, 12000], 'Combined cello, viola, and two violins span warm low body (100–350 Hz) through violin bridge brilliance (2–4.5 kHz).'],
  ['Small Acoustic Combo (Ensemble)', [41, 4186], 'omni', 'plucked_acoustic_wood', [70, 300], [1500, 4500], [5500, 13000], [41, 13000], 'Full-range acoustic combo spanning bass fundamentals through acoustic guitar/piano/percussion transients.'],
];
for (const [name, range, rad, regenKey, body, presence, air, practical, formantNote] of ensemble) {
  entries.push(
    makeEntry({
      category: 'Human',
      family: 'Other / Ensemble',
      name,
      symbol: 'ensemble',
      range,
      radiation: rad,
      practical,
      body,
      presence,
      air,
      formantNote,
      notes: 'Published group reference; spatial radiation is approximated as a single virtual point source.',
      refs: name.includes('String') || name.includes('Combo') ? REFS_ORCHESTRAL : REFS_VOICE,
      regenKey,
    }),
  );
}

if (entries.length !== 138) {
  throw new Error(`Expected exactly 138 source entries, got ${entries.length}`);
}

const payload = {
  schema_version: 1,
  source_count: entries.length,
  generated_by: 'capture-reference/build_source_profiles.mjs',
  policy:
    'Published instrument & vocal frequency ranges and spectral regions are reference guidelines compiled offline from published acoustics charts (Yona Marie Music Instrument Frequency Chart, Adler/Meyer/Rossing orchestral & vocal tables, RWC Musical Instrument Sound Database, audiolabs/SNDB). Zero runtime network calls. Zero invented narrow resonance peaks. Published ranges NEVER filter, clamp, or transpose extracted performance cues.',
  sources: entries.map((e, index) => ({ index, ...e })),
};

fs.writeFileSync(OUT_PATH, JSON.stringify(payload, null, 2) + '\n', 'utf8');
console.log(`Wrote ${entries.length} source profiles to ${OUT_PATH} (${(fs.statSync(OUT_PATH).size / 1024).toFixed(1)} KB)`);
