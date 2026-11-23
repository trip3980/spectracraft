import assert from 'node:assert/strict';
import test from 'node:test';
import {
  LAYOUT_LIMITS,
  createDefaultWorkspaceLayout,
  movePaneInOrder,
  resizeBottomDockHeight,
  resizeInspectorWidth,
  resizeSpatialWidth,
  sanitizeWorkspaceLayout,
  setPaneFloating,
  updateFloatingBounds,
} from './workspaceLayout.ts';
import { computeAcousticFingerprint, makeRoomPreset } from './studioRig.ts';

test('default workspace layout starts with Preview 19 order: inspector -> timeline -> spatial', () => {
  const layout = createDefaultWorkspaceLayout();
  assert.deepEqual(layout.paneOrder, ['inspector', 'timeline', 'spatial']);
  assert.equal(layout.inspectorOpen, true);
  assert.equal(layout.spatialOpen, true);
  assert.equal(layout.inspectorFloat.floating, false);
  assert.equal(layout.spatialFloat.floating, false);
});

test('panel resize clamps safely to usable min/max bounds and rejects NaN', () => {
  let layout = createDefaultWorkspaceLayout();
  layout = resizeInspectorWidth(layout, 40);
  assert.equal(layout.inspectorWidth, LAYOUT_LIMITS.inspectorMinW);
  layout = resizeInspectorWidth(layout, 1900);
  assert.equal(layout.inspectorWidth, LAYOUT_LIMITS.inspectorMaxW);
  layout = resizeInspectorWidth(layout, Number.NaN);
  assert.equal(layout.inspectorWidth, LAYOUT_LIMITS.inspectorMaxW);

  layout = resizeSpatialWidth(layout, 10);
  assert.equal(layout.spatialWidth, LAYOUT_LIMITS.spatialMinW);
  layout = resizeSpatialWidth(layout, 999);
  assert.equal(layout.spatialWidth, LAYOUT_LIMITS.spatialMaxW);

  layout = resizeBottomDockHeight(layout, 12);
  assert.equal(layout.bottomDockHeight, LAYOUT_LIMITS.dockMinH);
  layout = resizeBottomDockHeight(layout, 900);
  assert.equal(layout.bottomDockHeight, LAYOUT_LIMITS.dockMaxH);
});

test('reordering, undocking, and dragging panels preserves all panes and never touches rig/audio state', () => {
  const rig = makeRoomPreset('medium');
  const rigBefore = JSON.stringify(rig);
  const fpBefore = computeAcousticFingerprint(rig);

  let layout = createDefaultWorkspaceLayout();
  layout = movePaneInOrder(layout, 'inspector', 1);
  assert.deepEqual(layout.paneOrder, ['timeline', 'inspector', 'spatial']);
  layout = movePaneInOrder(layout, 'inspector', 1);
  assert.deepEqual(layout.paneOrder, ['timeline', 'spatial', 'inspector']);

  layout = setPaneFloating(layout, 'spatial', true);
  assert.equal(layout.spatialFloat.floating, true);
  layout = updateFloatingBounds(layout, 'spatial', { x: 240, y: 120, w: 500, h: 520 });
  assert.equal(layout.spatialFloat.w, 500);
  assert.equal(layout.spatialFloat.h, 520);

  layout = setPaneFloating(layout, 'spatial', false);
  assert.equal(layout.spatialFloat.floating, false);

  // Strict Section 4 invariant: layout changes never mutate rig or acoustic fingerprint
  assert.equal(JSON.stringify(rig), rigBefore);
  assert.equal(computeAcousticFingerprint(rig), fpBefore);
});

test('sanitizeWorkspaceLayout rejects corrupted persisted payloads cleanly', () => {
  const sanitized = sanitizeWorkspaceLayout({
    // @ts-expect-error testing malformed persisted order
    paneOrder: ['inspector', 'inspector', 'bogus'],
    inspectorWidth: -500,
    spatialWidth: Number.POSITIVE_INFINITY,
    spatialSplitRatio: 5.0,
    bottomDockHeight: 9999,
  });
  assert.deepEqual(sanitized.paneOrder, ['inspector', 'timeline', 'spatial']);
  assert.equal(sanitized.inspectorWidth, LAYOUT_LIMITS.inspectorMinW);
  assert.equal(sanitized.spatialWidth, createDefaultWorkspaceLayout().spatialWidth);
  assert.equal(sanitized.spatialSplitRatio, 0.75);
  assert.equal(sanitized.bottomDockHeight, LAYOUT_LIMITS.dockMaxH);
});
