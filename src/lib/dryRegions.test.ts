import assert from 'node:assert/strict';
import test from 'node:test';
import {
  addDryRegion,
  createInitialDryRegionHistory,
  deleteDryRegion,
  redoDryRegionHistory,
  serializeDryRegionForRust,
  transformRegionByHandle,
  undoDryRegionHistory,
  updateDryRegion,
} from './dryRegions.ts';

test('multi-region create, update, delete, undo, and redo stay consistent', () => {
  let state = createInitialDryRegionHistory();
  state = addDryRegion(
    state,
    {
      kind: 'marquee',
      startS: 1.2,
      endS: 2.4,
      minHz: 220,
      maxHz: 1800,
      note: 'straighten pitch wobble',
      action: 'straighten',
      strength: 0.65,
    },
    10.0,
  );
  assert.equal(state.regions.length, 1);
  const r1Id = state.regions[0].id;
  assert.equal(state.selectedId, r1Id);

  state = addDryRegion(
    state,
    {
      kind: 'range',
      startS: 4.0,
      endS: 5.5,
      note: 'smooth phrase tail',
      action: 'smooth',
    },
    10.0,
  );
  assert.equal(state.regions.length, 2);
  assert.equal(state.regions[1].minHz, null);
  assert.equal(state.regions[1].maxHz, null);

  state = updateDryRegion(state, r1Id, { note: 'reduce noise + steady' }, 10.0);
  assert.equal(state.regions[0].note, 'reduce noise + steady');

  state = deleteDryRegion(state, r1Id);
  assert.equal(state.regions.length, 1);

  // Undo deletion restores r1
  state = undoDryRegionHistory(state);
  assert.equal(state.regions.length, 2);
  assert.equal(state.regions[0].note, 'reduce noise + steady');

  // Redo removes r1 again
  state = redoDryRegionHistory(state);
  assert.equal(state.regions.length, 1);
});

test('move and resize handles clamp safely inside duration and frequency limits', () => {
  let state = createInitialDryRegionHistory();
  state = addDryRegion(
    state,
    { kind: 'marquee', startS: 2.0, endS: 3.5, minHz: 300, maxHz: 1500 },
    8.0,
  );
  const orig = state.regions[0];

  // Move preserves span even when pushed past duration
  const moved = transformRegionByHandle(orig, 'move', 20.0, -500, 8.0);
  assert.equal(moved.endS, 8.0);
  assert.ok(Math.abs((moved.endS - moved.startS) - 1.5) < 1e-3);
  assert.equal(moved.minHz, 20);
  assert.equal(moved.maxHz, 1220);

  // Corner resize adjusts both startS and maxHz without inverting bounds
  const resized = transformRegionByHandle(orig, 'top-left', 0.4, 500, 8.0);
  assert.equal(resized.startS, 2.4);
  assert.equal(resized.endS, 3.5);
  assert.equal(resized.maxHz, 2000);
  assert.equal(resized.minHz, 300);

  const rustPayload = serializeDryRegionForRust(resized);
  assert.equal(rustPayload.kind, 'marquee');
  assert.equal(rustPayload.start_s, 2.4);
  assert.equal(rustPayload.max_hz, 2000);
});
