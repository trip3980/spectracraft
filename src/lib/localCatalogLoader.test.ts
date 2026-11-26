import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { CatalogLoadError, loadLocalStudioCatalogs } from './localCatalogLoader';

const CAPTURE_REF_DIR = path.resolve(process.cwd(), 'spectracraft-mono/capture-reference');

test('loads full offline capture-reference catalogs with zero network access', () => {
  const catalogs = loadLocalStudioCatalogs(CAPTURE_REF_DIR);

  assert.equal(catalogs.sources.length, 138);
  assert.equal(catalogs.mics.length, 972);
  assert.equal(catalogs.surfaces.length, 32);

  assert.deepEqual(catalogs.bands_hz, [125, 250, 500, 1000, 2000, 4000]);
  assert.equal(catalogs.face_labels.length, 6);
  assert.equal(catalogs.room_presets.length, 5);
  assert.equal(catalogs.patterns.length, 6);

  assert.equal(catalogs.catalog_provenance.source_count, 138);
  assert.equal(catalogs.catalog_provenance.mic_count, 972);
  assert.equal(catalogs.catalog_provenance.surface_count, 32);
});

test('preserves baseline entries and late-catalog entries across all three tabs', () => {
  const catalogs = loadLocalStudioCatalogs(CAPTURE_REF_DIR);

  // Source baseline + end-of-catalog entry
  assert.equal(catalogs.sources[0].index, 0);
  assert.equal(catalogs.sources[0].category, 'Unknown');
  assert.equal(catalogs.sources[137].index, 137);
  assert.ok(catalogs.sources[137].name.length > 0);
  assert.ok(catalogs.sources[137].regen_model.physics_constraint);

  // Mic baseline (Shure SM7B at index 0) + end-of-catalog entry (#971)
  assert.equal(catalogs.mics[0].index, 0);
  assert.equal(catalogs.mics[0].brand, 'Shure');
  assert.match(catalogs.mics[0].model, /SM7/i);
  assert.equal(catalogs.mics[0].renderer_profile_id, 'dynamic_sm7_class_approx');
  assert.equal(catalogs.mics[971].index, 971);
  assert.ok(catalogs.mics[971].brand.length > 0);
  assert.ok(catalogs.mics[971].model.length > 0);

  // Material raw vs clamped effective absorption and pressure reflectance
  const wedgeFoam = catalogs.surfaces[1];
  assert.equal(wedgeFoam.index, 1);
  assert.ok(wedgeFoam.raw_absorption_6band.some((a) => a > 1.0));
  for (let i = 0; i < 6; i++) {
    const eff = wedgeFoam.effective_absorption_6band[i];
    assert.ok(eff >= 0 && eff <= 1);
    assert.ok(Math.abs(wedgeFoam.pressure_reflectance_6band[i] - Math.sqrt(1 - eff)) < 1e-12);
  }
  assert.equal(catalogs.surfaces[31].index, 31);
  assert.ok(catalogs.surfaces[31].name.length > 0);
});

test('throws clear CatalogLoadError when capture-reference directory is missing', () => {
  assert.throws(
    () => loadLocalStudioCatalogs('/nonexistent-capture-reference-dir'),
    (err: unknown) =>
      err instanceof CatalogLoadError &&
      err.statusCode === 503 &&
      /capture-reference/i.test(err.message),
  );
});
