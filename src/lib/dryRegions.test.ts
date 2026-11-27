import assert from 'node:assert/strict';
import test from 'node:test';
import {
  addDryRegion,
  beginRegionGesture,
  cancelRegionGesture,
  commitRegionGesture,
  createInitialDryRegionHistory,
  deleteDryRegion,
  previewRegionGesture,
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

test('M5: delete-and-readd produces strictly unique region IDs without collisions', () => {
  let state = createInitialDryRegionHistory();
  state = addDryRegion(state, { kind: 'range', startS: 0.5, endS: 1.5 }, 10.0);
  state = addDryRegion(state, { kind: 'range', startS: 2.0, endS: 3.0 }, 10.0);
  const [firstId, secondId] = state.regions.map((r) => r.id);
  assert.notEqual(firstId, secondId);

  // Delete the first region and add a third — ID must NOT collide with firstId or secondId
  state = deleteDryRegion(state, firstId);
  state = addDryRegion(state, { kind: 'range', startS: 4.0, endS: 5.0 }, 10.0);
  const thirdId = state.regions[state.regions.length - 1].id;
  assert.notEqual(thirdId, firstId);
  assert.notEqual(thirdId, secondId);
  assert.equal(new Set(state.regions.map((r) => r.id)).size, 2);
});

test('M5: multi-frame drag gesture commits as 1 undo step restoring exact start geometry, and cancel restores baseline', () => {
  let state = createInitialDryRegionHistory();
  state = addDryRegion(
    state,
    { kind: 'marquee', startS: 1.0, endS: 2.0, minHz: 200, maxHz: 1000 },
    10.0,
  );
  const id = state.regions[0].id;
  const pastCountBeforeDrag = state.past.length;

  // Begin gesture and simulate 5 intermediate drag frames
  state = beginRegionGesture(state);
  for (let step = 1; step <= 5; step++) {
    state = previewRegionGesture(
      state,
      id,
      { startS: 1.0 + step * 0.2, endS: 2.0 + step * 0.2 },
      10.0,
    );
  }
  assert.equal(state.regions[0].startS, 2.0);
  assert.equal(state.past.length, pastCountBeforeDrag);

  // Commit produces exactly ONE undo entry whose baseline is the exact start (1.0..2.0)
  state = commitRegionGesture(state, id, { startS: 2.0, endS: 3.0 }, 10.0);
  assert.equal(state.past.length, pastCountBeforeDrag + 1);
  assert.equal(state.regions[0].startS, 2.0);

  // Single undo restores exact pre-drag geometry (1.0..2.0)
  state = undoDryRegionHistory(state);
  assert.equal(state.regions[0].startS, 1.0);
  assert.equal(state.regions[0].endS, 2.0);

  // Begin another gesture and cancel mid-drag -> restores exact baseline without pushing history
  const pastBeforeCancel = state.past.length;
  state = beginRegionGesture(state);
  state = previewRegionGesture(state, id, { startS: 4.5, endS: 5.5 }, 10.0);
  assert.equal(state.regions[0].startS, 4.5);
  state = cancelRegionGesture(state);
  assert.equal(state.regions[0].startS, 1.0);
  assert.equal(state.regions[0].endS, 2.0);
  assert.equal(state.past.length, pastBeforeCancel);
});
