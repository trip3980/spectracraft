#!/usr/bin/env node
/**
 * Offline recovery script for the SpectraCraft `capture-reference` package.
 *
 * Recovers published manufacturer microphone specifications, digitized frequency
 * response curves, and published polar plot frequency tables from Mic Archive
 * (https://micarchive.com/compare & https://micarchive.com/mics/<id>/) into a
 * self-contained offline JSON artifact so the app has ZERO runtime network
 * dependency on micarchive.com.
 *
 * Honesty rules:
 * - Never invent missing specs; missing manufacturer values are stored as `null`
 *   and recorded in `uncertainties`.
 * - Published curves and specs are strictly reference data — never labeled as
 *   measured microphone emulations.
 */

import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';

const OUT_DIR = path.resolve('spectracraft-mono/capture-reference');
const OUT_JSON = path.join(OUT_DIR, 'mic-archive-972.json');

function decodeHtmlEntities(s) {
  return String(s || '')
    .replace(/&#x27;/g, "'")
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/\u2011/g, '-')
    .replace(/\u00a0/g, ' ')
    .trim();
}

function parseRangeHz(rangeStr) {
  if (!rangeStr || typeof rangeStr !== 'string') return null;
  const cleaned = rangeStr.replace(/,/g, '').replace(/\u2013|\u2014|–|—/g, '-').trim();
  const m = cleaned.match(/(\d+(?:\.\d+)?)\s*(k)?\s*-\s*(\d+(?:\.\d+)?)\s*(k)?/i);
  if (!m) return null;
  const lo = parseFloat(m[1]) * (m[2] ? 1000 : 1);
  const hi = parseFloat(m[3]) * (m[4] ? 1000 : 1);
  if (!Number.isFinite(lo) || !Number.isFinite(hi) || lo <= 0 || hi <= lo) return null;
  return [Math.round(lo * 10) / 10, Math.round(hi * 10) / 10];
}

function inferPrimaryPatternKey(curvePattern, publishedPatternStr, summary) {
  const candidates = [curvePattern, publishedPatternStr, summary]
    .filter(Boolean)
    .map((s) => String(s).toLowerCase());
  for (const text of candidates) {
    // Check specific multi-word patterns before generic substrings
    if (/\bwide[\s-]*cardioid\b|\bsub[\s-]*cardioid\b|\bhypocardioid\b/.test(text)) return 'wide_cardioid';
    if (/\bhyper[\s-]*cardioid\b/.test(text)) return 'hypercardioid';
    if (/\bsuper[\s-]*cardioid\b|\blobar\b|\bshotgun\b|\binterference\b/.test(text)) return 'supercardioid';
    if (/\bfigure[\s-]*8\b|\bbidirectional\b|\bfig[\s-]*8\b/.test(text)) return 'figure8';
    if (/\bcardioid\b|\bunidirectional\b/.test(text)) return 'cardioid';
    if (/\bomni(?:directional)?\b|\bhemispherical\b|\bboundary\b/.test(text)) return 'omni';
  }
  return null;
}

function inferTransducerAndProfile(publishedType, modelName, summary, patternKey) {
  const t = `${publishedType || ''} ${modelName || ''} ${summary || ''}`.toLowerCase();
  if (/\bribbon\b/.test(t)) {
    return { transducer: 'ribbon', renderer_profile: 'ribbon_smooth' };
  }
  if (/\bdynamic\b|\bmoving[\s-]*coil\b/.test(t)) {
    if (/\bsm7\b|\bre20\b|\bmd[\s-]*421\b|\bpr[\s-]*40\b|\bbroadcast\b/.test(t)) {
      return { transducer: 'dynamic', renderer_profile: 'dynamic_sm7_class' };
    }
    return { transducer: 'dynamic', renderer_profile: 'dynamic_presence_57_class' };
  }
  if (/\bboundary\b|\bpzm\b|\bmeasurement\b|\bearthworks\b|\bdpa 4006\b|\bpiezo\b|\bcontact\b|\bpickup\b/.test(t)) {
    return { transducer: 'boundary_omni', renderer_profile: 'boundary_omni_flat' };
  }
  if (/\bsmall[\s-]*diaphragm\b|\bpencil\b|\bshotgun\b|\blavalier\b|\bminiature\b|\bheadset\b|\bclip-on\b|\bsdc\b|\b19\s*mm\b|\b16\s*mm\b/.test(t)) {
    return { transducer: 'small_condenser', renderer_profile: 'small_condenser_neutral' };
  }
  if (/\bcondenser\b|\belectret\b|\btube\b|\bvalve\b|\bfet\b/.test(t)) {
    if (patternKey === 'omni' && /\bmeasurement\b|\breference\b/.test(t)) {
      return { transducer: 'boundary_omni', renderer_profile: 'boundary_omni_flat' };
    }
    return { transducer: 'large_condenser', renderer_profile: 'large_condenser_warm' };
  }
  return { transducer: 'small_condenser', renderer_profile: 'small_condenser_neutral' };
}

/**
 * Downsample a dense digitized [[hz, db], ...] curve onto <= 24 log-spaced points
 * spanning the curve's actual [minHz, maxHz] without extrapolating outside published bounds.
 */
function samplePublishedCurve(points, maxPoints = 24) {
  if (!Array.isArray(points) || points.length < 2) return [];
  const clean = points
    .filter((p) => Array.isArray(p) && p.length >= 2 && Number.isFinite(p[0]) && Number.isFinite(p[1]) && p[0] > 0)
    .sort((a, b) => a[0] - b[0]);
  if (clean.length < 2) return [];
  if (clean.length <= maxPoints) {
    return clean.map(([hz, db]) => [Math.round(hz * 10) / 10, Math.round(db * 100) / 100]);
  }
  const minHz = clean[0][0];
  const maxHz = clean[clean.length - 1][0];
  const logMin = Math.log(minHz);
  const logMax = Math.log(maxHz);
  const out = [];
  let cursor = 0;
  for (let i = 0; i < maxPoints; i++) {
    const frac = i / (maxPoints - 1);
    const targetHz = Math.exp(logMin + frac * (logMax - logMin));
    while (cursor < clean.length - 2 && clean[cursor + 1][0] < targetHz) {
      cursor++;
    }
    const [x0, y0] = clean[cursor];
    const [x1, y1] = clean[cursor + 1];
    const t = x1 > x0 ? Math.max(0, Math.min(1, (targetHz - x0) / (x1 - x0))) : 0;
    const db = y0 + (y1 - y0) * t;
    out.push([Math.round(targetHz * 10) / 10, Math.round(db * 100) / 100]);
  }
  return out;
}

function extractOffaxisSummary(offaxis) {
  if (!offaxis || !Array.isArray(offaxis.f) || !Array.isArray(offaxis.db)) return null;
  const freqs = [];
  const atten90Db = [];
  const atten180Db = [];
  for (let i = 0; i < offaxis.f.length; i++) {
    const f = Number(offaxis.f[i]);
    const row = offaxis.db[i];
    if (!Number.isFinite(f) || !Array.isArray(row) || row.length < 3) continue;
    const midIdx = Math.floor((row.length - 1) / 2);
    const endIdx = row.length - 1;
    const d90 = Number(row[midIdx]);
    const d180 = Number(row[endIdx]);
    if (!Number.isFinite(d90) || !Number.isFinite(d180)) continue;
    freqs.push(Math.round(f));
    atten90Db.push(Math.round(d90 * 10) / 10);
    atten180Db.push(Math.round(d180 * 10) / 10);
  }
  if (freqs.length === 0) return null;
  return { freqs_hz: freqs, atten_90deg_db: atten90Db, atten_180deg_db: atten180Db };
}

async function fetchJsonOrNull(url) {
  try {
    const res = await fetch(url, { headers: { 'User-Agent': 'SpectraCraft-CaptureReference/0.1' } });
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

async function runPool(items, concurrency, fn) {
  const results = new Array(items.length);
  let nextIdx = 0;
  async function worker() {
    while (true) {
      const idx = nextIdx++;
      if (idx >= items.length) return;
      results[idx] = await fn(items[idx], idx);
    }
  }
  await Promise.all(Array.from({ length: Math.min(concurrency, items.length) }, () => worker()));
  return results;
}

async function main() {
  const searchData = JSON.parse(fs.readFileSync('/tmp/search.json', 'utf8'));
  const libSrc = fs.readFileSync('/tmp/mics-library.js', 'utf8');
  const indexHtml = fs.readFileSync('/tmp/mics_index.html', 'utf8');

  const sandbox = { window: {} };
  vm.runInNewContext(libSrc, sandbox);
  const micLibrary = sandbox.window.MIC_LIBRARY;

  // Group mics-library.js curve rows by mic id
  const libById = new Map();
  for (const row of micLibrary) {
    if (!libById.has(row.id)) libById.set(row.id, []);
    libById.get(row.id).push(row);
  }

  // Parse index HTML list attributes (covers type/pattern metadata for non-curve mics too)
  const indexMetaById = new Map();
  const liRegex = /<li\s+([^>]*)>([\s\S]*?)<\/li>/g;
  let m;
  while ((m = liRegex.exec(indexHtml)) !== null) {
    const attrs = m[1];
    const body = m[2];
    const typeM = attrs.match(/data-type="([^"]*)"/);
    const patM = attrs.match(/data-pat="([^"]*)"/);
    const brandM = attrs.match(/data-brand="([^"]*)"/);
    const hrefs = [...body.matchAll(/href="\.\.\/mics\/([a-z0-9-]+)\/"/g)].map((x) => x[1]);
    for (const id of hrefs) {
      if (!indexMetaById.has(id)) {
        indexMetaById.set(id, {
          type: typeM ? decodeHtmlEntities(typeM[1]) : null,
          pat: patM ? decodeHtmlEntities(patM[1]) : null,
          brand: brandM ? decodeHtmlEntities(brandM[1]) : null,
        });
      }
    }
  }

  // Fetch digitized curves.json for all 939 curve-bearing mics in parallel
  const curveIds = [...libById.keys()];
  console.log(`Fetching digitized curves.json for ${curveIds.length} Mic Archive entries...`);
  const t0 = Date.now();
  const fetchedCurves = await runPool(curveIds, 24, async (id) => {
    const data = await fetchJsonOrNull(`https://micarchive.com/mics/${id}/curves.json`);
    return [id, data];
  });
  const curvesById = new Map(fetchedCurves);
  let curveOkCount = 0;
  let offaxisCount = 0;
  for (const [, val] of curvesById) {
    if (val && typeof val === 'object' && Object.keys(val).length > 0) {
      curveOkCount++;
      const first = val[Object.keys(val)[0]];
      if (first?.offaxis?.f?.length) offaxisCount++;
    }
  }
  console.log(
    `Fetched ${curveOkCount}/${curveIds.length} curves.json (${offaxisCount} with published off-axis polar tables) in ${((Date.now() - t0) / 1000).toFixed(1)}s`,
  );

  // Assemble all 972 unique Mic Archive entries from search.json
  const entries = [];
  for (const row of searchData.mics) {
    const [id, fullModel, brandRaw, summaryRaw] = row;
    const brand = decodeHtmlEntities(brandRaw);
    const fullName = decodeHtmlEntities(fullModel);
    const model =
      fullName.toLowerCase().startsWith(brand.toLowerCase() + ' ')
        ? fullName.slice(brand.length + 1).trim()
        : fullName;
    const summary = decodeHtmlEntities(summaryRaw);

    const libRows = libById.get(id) || [];
    const primaryLib = libRows[0] || null;
    const specs = primaryLib?.specs || {};
    const idxMeta = indexMetaById.get(id) || {};

    const publishedType = specs.type || idxMeta.type || null;
    const publishedPatterns = specs.pattern || idxMeta.pat || null;
    const publishedCurves = libRows.map((r) => decodeHtmlEntities(r.name));
    const publishedRangeText = specs.range ? decodeHtmlEntities(specs.range) : null;
    const publishedRangeHz = parseRangeHz(publishedRangeText);
    const publishedSensitivityDbv =
      typeof specs.sens === 'number' && Number.isFinite(specs.sens) ? specs.sens : null;
    const publishedSelfNoiseDba =
      typeof specs.noise === 'number' && Number.isFinite(specs.noise) ? specs.noise : null;
    const publishedMaxSplDb =
      typeof specs.maxspl === 'number' && Number.isFinite(specs.maxspl) ? specs.maxspl : null;
    const publishedImpedanceOhm =
      typeof specs.imp === 'number' && Number.isFinite(specs.imp) ? specs.imp : null;
    const publishedReleasedYear =
      typeof specs.released === 'number' && Number.isFinite(specs.released) ? specs.released : null;
    const publishedDistCm =
      typeof primaryLib?.dist === 'number' && Number.isFinite(primaryLib.dist)
        ? primaryLib.dist
        : null;

    // Extract digitized published curve & off-axis polar table if available
    const curvesObj = curvesById.get(id) || null;
    let publishedFrPoints = [];
    let publishedPrimaryCurveName = null;
    let publishedOffaxis = null;
    if (curvesObj && typeof curvesObj === 'object') {
      const curveKeys = Object.keys(curvesObj);
      if (curveKeys.length > 0) {
        // Prefer a "flat" or "cardioid" curve as primary reference display if present
        const preferredKey =
          curveKeys.find((k) => /\bflat\b/i.test(k)) ||
          curveKeys.find((k) => /\bcardioid\b/i.test(k)) ||
          curveKeys[0];
        const primaryCurve = curvesObj[preferredKey];
        publishedPrimaryCurveName = preferredKey;
        publishedFrPoints = samplePublishedCurve(primaryCurve?.points, 24);
        publishedOffaxis = extractOffaxisSummary(primaryCurve?.offaxis);
      }
    }

    const inferredPattern = inferPrimaryPatternKey(
      primaryLib?.pattern,
      publishedPatterns,
      summary,
    );
    const patternKey = inferredPattern || 'omni';
    const { transducer, renderer_profile } = inferTransducerAndProfile(
      publishedType,
      model,
      summary,
      patternKey,
    );

    const uncertainties = [];
    if (publishedFrPoints.length === 0) {
      uncertainties.push('No digitized frequency-response curve in Mic Archive.');
    }
    if (!publishedOffaxis) {
      uncertainties.push('No digitized off-axis polar plot table in Mic Archive.');
    }
    if (publishedSensitivityDbv === null) uncertainties.push('Sensitivity (dBV/Pa) not published.');
    if (publishedSelfNoiseDba === null) uncertainties.push('Self-noise (dB(A)) not published.');
    if (publishedMaxSplDb === null) uncertainties.push('Maximum SPL not published.');
    if (!publishedRangeText) uncertainties.push('Frequency range not published.');
    if (!inferredPattern) {
      uncertainties.push(
        'No acoustic polar pattern stated (e.g. contact pickup/transducer); renderer defaults to Omni.',
      );
    } else if (publishedPatterns && /\//.test(publishedPatterns)) {
      uncertainties.push(
        `Multi-pattern published spec (${publishedPatterns}); renderer defaults to primary '${patternKey}' unless overridden.`,
      );
    }
    uncertainties.push(
      'Power requirement not included in Mic Archive bulk index (verify manufacturer spec sheet).',
    );
    uncertainties.push(
      'Published manufacturer data is reference only — not a measured microphone impulse response.',
    );

    entries.push({
      id,
      brand,
      model,
      source_url: `https://micarchive.com/mics/${id}/`,
      summary,
      transducer,
      pattern: patternKey,
      renderer_profile,
      published_type: publishedType,
      published_patterns: publishedPatterns,
      published_curves: publishedCurves,
      published_primary_curve: publishedPrimaryCurveName,
      published_fr_points: publishedFrPoints,
      published_offaxis: publishedOffaxis,
      published_range_text: publishedRangeText,
      published_range_hz: publishedRangeHz,
      published_sensitivity_dbv: publishedSensitivityDbv,
      published_self_noise_dba: publishedSelfNoiseDba,
      published_max_spl_db: publishedMaxSplDb,
      published_impedance_ohm: publishedImpedanceOhm,
      published_released_year: publishedReleasedYear,
      published_measurement_dist_cm: publishedDistCm,
      published_power: null,
      source_document: `Mic Archive (https://micarchive.com/mics/${id}/) — compiled from manufacturer spec sheets`,
      uncertainties,
    });
  }

  // Sort deterministically: Shure SM7B at index 0 (repeatable baseline invariant), then Brand -> Model
  entries.sort((a, b) => {
    if (a.id === 'shure-sm7b') return -1;
    if (b.id === 'shure-sm7b') return 1;
    const bc = a.brand.localeCompare(b.brand, 'en', { sensitivity: 'base' });
    if (bc !== 0) return bc;
    return a.model.localeCompare(b.model, 'en', { sensitivity: 'base' });
  });

  fs.mkdirSync(OUT_DIR, { recursive: true });
  const payload = {
    schema_version: 1,
    package: 'capture-reference',
    source_name: 'Mic Archive (micarchive.com)',
    source_urls: ['https://micarchive.com/compare', 'https://micarchive.com/mics/'],
    recovered_utc: new Date().toISOString().slice(0, 10),
    mic_count: entries.length,
    curve_bearing_count: entries.filter((e) => e.published_fr_points.length > 0).length,
    offaxis_bearing_count: entries.filter((e) => e.published_offaxis !== null).length,
    brand_count: new Set(entries.map((e) => e.brand)).size,
    honest_disclaimer:
      'Published manufacturer specs, frequency-response curves, and polar plots recovered from Mic Archive for offline reference. These are NOT measured microphone emulations; the audio renderer applies its explicitly labeled analytical polar pattern and shared parametric tonal class.',
    mics: entries,
  };

  fs.writeFileSync(OUT_JSON, JSON.stringify(payload));
  const stat = fs.statSync(OUT_JSON);
  console.log('RECOVERED_SUMMARY:', {
    outPath: OUT_JSON,
    sizeKb: Math.round(stat.size / 1024),
    micCount: payload.mic_count,
    brandCount: payload.brand_count,
    curveBearingCount: payload.curve_bearing_count,
    offaxisBearingCount: payload.offaxis_bearing_count,
    index0: `${entries[0].brand} ${entries[0].model}`,
    indexLast: `${entries[entries.length - 1].brand} ${entries[entries.length - 1].model}`,
    sampleDpa: entries.findIndex((e) => e.id === 'dpa-4011a'),
    sampleSennheiser: entries.findIndex((e) => e.id === 'sennheiser-mkh-50'),
  });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
