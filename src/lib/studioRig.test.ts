import assert from 'node:assert/strict';
import test from 'node:test';
import {
  ALL_UNITS,
  FEET_TO_METERS,
  computeAcousticFingerprint,
  computeNyquistDisplay,
  createHistory,
  displayToMeters,
  getMicWorldPosM,
  getSourcePosM,
  makeRoomPreset,
  metersToDisplay,
  passiveReflectance,
  polarGain,
  pushHistory,
  redoHistory,
  resetSourceToDefault,
  setLinkedDimensionM,
  setSourcePosM,
  snapAnglesToCardinalWithinThreshold,
  speedOfSoundMps,
  undoHistory,
} from './studioRig.ts';

test('display unit conversions round-trip accurately across all 6 units without altering acoustic fingerprint', () => {
  const base = makeRoomPreset('medium');
  const baseFp = computeAcousticFingerprint(base);
  for (const u of ALL_UNITS) {
    const rt = displayToMeters(metersToDisplay(base.roomMeters.x, u), u);
    assert.ok(Math.abs(rt - base.roomMeters.x) < 1e-6, `unit ${u} roundtrip`);
    assert.equal(computeAcousticFingerprint({ ...base, unit: u, showPatterns: true, gridSnap: true }), baseFp);
  }
});

test('room chain links: entering 24 ft with Width+Length linked sets both to 24 ft and keeps Height at 10 ft', () => {
  const rig = makeRoomPreset('small');
  assert.deepEqual(rig.dimLinked, [true, true, false]);
  const resized = setLinkedDimensionM(rig, 'x', 24 * FEET_TO_METERS);
  assert.ok(Math.abs(metersToDisplay(resized.roomMeters.x, 'ft') - 24) < 1e-4);
  assert.ok(Math.abs(metersToDisplay(resized.roomMeters.y, 'ft') - 24) < 1e-4);
  assert.ok(Math.abs(metersToDisplay(resized.roomMeters.z, 'ft') - 10) < 1e-4);
  // Source stays centered at (12 ft, 12 ft, 5 ft)
  const src = getSourcePosM(resized);
  assert.ok(Math.abs(metersToDisplay(src.x, 'ft') - 12) < 1e-4);
  assert.ok(Math.abs(metersToDisplay(src.y, 'ft') - 12) < 1e-4);
  assert.ok(Math.abs(metersToDisplay(src.z, 'ft') - 5) < 1e-4);
});

test('Open preset starts at 50x50x50 ft guide', () => {
  const open = makeRoomPreset('open');
  assert.ok(Math.abs(metersToDisplay(open.roomMeters.x, 'ft') - 50) < 1e-4);
  assert.ok(Math.abs(metersToDisplay(open.roomMeters.y, 'ft') - 50) < 1e-4);
  assert.ok(Math.abs(metersToDisplay(open.roomMeters.z, 'ft') - 50) < 1e-4);
});

test('moving source keeps mics stationary in world coords, re-aims Follow Source, and reset restores center+5ft', () => {
  let rig = makeRoomPreset('small');
  rig = {
    ...rig,
    mics: rig.mics.map((m) => ({ ...m, aim: { ...m.aim, followSource: true } })),
  };
  const micBefore = getMicWorldPosM(rig, 0);
  const moved = setSourcePosM(rig, { x: 1.0, y: 1.2, z: 1.1 });
  const micAfter = getMicWorldPosM(moved, 0);
  assert.ok(Math.abs(micAfter.x - micBefore.x) < 1e-5);
  assert.ok(Math.abs(micAfter.y - micBefore.y) < 1e-5);
  assert.ok(Math.abs(micAfter.z - micBefore.z) < 1e-5);
  assert.notEqual(moved.mics[0].aim.headingZDeg, rig.mics[0].aim.headingZDeg);

  const reset = resetSourceToDefault(moved);
  const resetSrc = getSourcePosM(reset);
  assert.ok(Math.abs(metersToDisplay(resetSrc.x, 'ft') - 5) < 1e-4);
  assert.ok(Math.abs(metersToDisplay(resetSrc.y, 'ft') - 5) < 1e-4);
  assert.ok(Math.abs(metersToDisplay(resetSrc.z, 'ft') - 5) < 1e-4);
  assert.equal(reset.mics[0].catalogIndex, moved.mics[0].catalogIndex);
  assert.deepEqual(reset.surfaceFaces, moved.surfaceFaces);
});

test('rotation ball 8-degree snap locks to nearest cardinal axis and ignores angles > 8 deg', () => {
  const nearPosX = snapAnglesToCardinalWithinThreshold(
    { tiltXDeg: 3, rollYDeg: 0, headingZDeg: 86, followSource: false },
    8,
  );
  assert.equal(nearPosX.snapped, true);
  assert.equal(nearPosX.axis, '+X');
  assert.equal(nearPosX.aim.headingZDeg, 90);
  assert.equal(nearPosX.aim.tiltXDeg, 0);

  const oblique = snapAnglesToCardinalWithinThreshold(
    { tiltXDeg: 25, rollYDeg: 0, headingZDeg: 45, followSource: false },
    8,
  );
  assert.equal(oblique.snapped, false);
  assert.equal(oblique.axis, null);
});

test('passiveReflectance clamps Sabin alpha > 1.0 and NaN safely', () => {
  assert.equal(passiveReflectance(1.05), 0);
  assert.equal(passiveReflectance(-0.2), 1);
  assert.equal(passiveReflectance(Number.NaN), 0);
  assert.ok(Math.abs(passiveReflectance(0.75) - 0.5) < 1e-9);
});

test('polarGain matches analytical first-order curves', () => {
  assert.equal(polarGain('omni', -1), 1);
  assert.equal(polarGain('cardioid', -1), 0);
  assert.ok(Math.abs(polarGain('hypercardioid', 0) - 0.25) < 1e-9);
  assert.equal(polarGain('figure8', -1), -1);
});

test('speedOfSoundMps, Nyquist helper, and history stack work cleanly', () => {
  assert.ok(Math.abs(speedOfSoundMps(20) - 343.0) < 1e-6);
  const nyq = computeNyquistDisplay(48_000, 32_000);
  assert.equal(nyq.hasAboveNyquist, true);
  let h = createHistory(1);
  h = pushHistory(h, 2);
  h = undoHistory(h);
  assert.equal(h.present, 1);
  h = redoHistory(h);
  assert.equal(h.present, 2);
});

test('2 ft source-to-mic delay equals ~1.777 ms at 343 m/s and 3:1 guidance updates cleanly with position', async () => {
  const { applyStereoPreset, computeAcousticAnalysisLocal, makeDefaultMicSlot } = await import(
    './studioRig.ts'
  );
  const rig = makeRoomPreset('small');
  const analysis = computeAcousticAnalysisLocal(rig, [], [], []);
  assert.ok(Math.abs(analysis.speed_of_sound_mps - 343.0) < 1e-6);
  // 2 ft = 0.6096 m; 0.6096 / 343.0 * 1000 = 1.77726 ms
  assert.ok(Math.abs(analysis.direct_path.direct_delay_ms - 1.77726) < 0.005);
  assert.equal(analysis.placement_guidance.multi_mic_active, false);

  // Toggling showGuidanceOverlay must NEVER alter acoustic fingerprint
  const fpBefore = computeAcousticFingerprint(rig);
  assert.equal(computeAcousticFingerprint({ ...rig, showGuidanceOverlay: true }), fpBefore);

  // Two mics in Medium room: Mic 1 at 2 ft, Mic 2 at 3 ft -> shortfall; Mic 2 at 6 ft -> clears 3:1
  let med = makeRoomPreset('medium');
  const mic2 = makeDefaultMicSlot(1);
  mic2.offsetM = { x: 0, y: 3 * FEET_TO_METERS, z: 0 };
  med = { ...med, mics: [med.mics[0], mic2] };
  const shortRes = computeAcousticAnalysisLocal(med, [], [], []);
  assert.equal(shortRes.placement_guidance.multi_mic_active, true);
  assert.ok(Math.abs(shortRes.placement_guidance.three_to_one_min_distance_m - 6 * FEET_TO_METERS) < 1e-4);
  assert.equal(shortRes.placement_guidance.slots[1].meets_three_to_one, false);

  med = {
    ...med,
    mics: [med.mics[0], { ...mic2, offsetM: { x: 0, y: 6 * FEET_TO_METERS, z: 0 } }],
  };
  const clearRes = computeAcousticAnalysisLocal(med, [], [], []);
  assert.equal(clearRes.placement_guidance.slots[1].meets_three_to_one, true);
  assert.ok(Math.abs(clearRes.placement_guidance.slots[1].level_diff_vs_nearest_db - 9.5424) < 0.02);

  // ORTF preset sets 17 cm spacing & ±55° (110° included) without altering room dims or surfaces
  const ortf = applyStereoPreset(rig, 'ortf');
  assert.equal(ortf.mics.length, 2);
  assert.deepEqual(ortf.roomMeters, rig.roomMeters);
  assert.deepEqual(ortf.surfaceFaces, rig.surfaceFaces);
  const dx = Math.abs(ortf.mics[1].offsetM.x - ortf.mics[0].offsetM.x);
  assert.ok(Math.abs(dx - 0.17) < 1e-5, `expected 0.17 m ORTF spacing, got ${dx}`);
  assert.equal(ortf.mics[0].aim.headingZDeg, -55);
  assert.equal(ortf.mics[1].aim.headingZDeg, 55);
  assert.notEqual(computeAcousticFingerprint(ortf), fpBefore);
});

test('default SM7B mic at +Y aims directly toward source (-Y) matching Rust MicAim convention', async () => {
  const { aimDirectionFromAngles, anglesPointingAtSource } = await import('./studioRig.ts');
  const rig = makeRoomPreset('small');
  const mic = rig.mics[0];
  assert.ok(mic.offsetM.y > 0);
  const dir = aimDirectionFromAngles(mic.aim);
  assert.ok(dir.y < -0.999, `default mic must aim -Y toward source, got y=${dir.y}`);
  const pointed = anglesPointingAtSource(mic.offsetM);
  assert.ok(Math.abs(pointed.headingZDeg) < 1e-4);
  assert.ok(Math.abs(pointed.tiltXDeg) < 1e-4);
});

test('swapping mic catalog model (including late-catalog entry) preserves position and aim while changing acoustic fingerprint', async () => {
  const { setMicCatalogIndex, setMicWorldPosM } = await import('./studioRig.ts');
  let rig = makeRoomPreset('medium');
  rig = setMicWorldPosM(rig, 0, { x: 2.2, y: 4.1, z: 1.8 });
  rig = {
    ...rig,
    mics: rig.mics.map((m) => ({
      ...m,
      aim: { tiltXDeg: -14, rollYDeg: 3, headingZDeg: 22, followSource: false },
    })),
  };
  const posBefore = { ...rig.mics[0].offsetM };
  const aimBefore = { ...rig.mics[0].aim };
  const fpBefore = computeAcousticFingerprint(rig);

  // Swap to late-catalog Mic Archive entries (743 = Sennheiser MKH 50, 971 = last of 972)
  const swapped = setMicCatalogIndex(rig, 0, 743);
  assert.equal(swapped.mics[0].catalogIndex, 743);
  assert.deepEqual(swapped.mics[0].offsetM, posBefore);
  assert.deepEqual(swapped.mics[0].aim, aimBefore);
  assert.notEqual(computeAcousticFingerprint(swapped), fpBefore);

  const swappedLast = setMicCatalogIndex(swapped, 0, 971);
  assert.equal(swappedLast.mics[0].catalogIndex, 971);
  assert.deepEqual(swappedLast.mics[0].offsetM, posBefore);
  assert.deepEqual(swappedLast.mics[0].aim, aimBefore);
});

test('Plans and 3D views share one single source of truth for source and mic world coordinates', async () => {
  const { setMicWorldPosM } = await import('./studioRig.ts');
  let rig = makeRoomPreset('large');
  rig = setSourcePosM(rig, { x: 6.4, y: 8.2, z: 1.65 });
  rig = setMicWorldPosM(rig, 0, { x: 7.1, y: 9.8, z: 1.75 });
  const src = getSourcePosM(rig);
  const mic = getMicWorldPosM(rig, 0);
  assert.ok(Math.abs(src.x - 6.4) < 1e-5);
  assert.ok(Math.abs(src.y - 8.2) < 1e-5);
  assert.ok(Math.abs(src.z - 1.65) < 1e-5);
  assert.ok(Math.abs(mic.x - 7.1) < 1e-5);
  assert.ok(Math.abs(mic.y - 9.8) < 1e-5);
  assert.ok(Math.abs(mic.z - 1.75) < 1e-5);
});
