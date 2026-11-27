import assert from 'node:assert/strict';
import test from 'node:test';
import {
  createInitialAudioHistory,
  getEffectiveDryRevision,
  initAudioHistoryWithBase,
  isApprovalValidForRevision,
  isArtifactStaleForDry,
  parseAndValidateProjectFile,
  PROJECT_SCHEMA_VERSION,
  pushStage1DryRevision,
  pushStage2DryEditRevision,
  redoAudioRevision,
  SessionEpochTracker,
  setStage2Bypass,
  undoAudioRevision,
  type DryRevision,
  type ProjectFileV2,
  type RevisionApproval,
} from './sessionState.ts';
import { DEFAULT_PIPELINE_STAGES, DEFAULT_VOICING_CONTROLS, makeRoomPreset } from './studioRig.ts';

function makeRevision(overrides: Partial<DryRevision> = {}): DryRevision {
  const id = overrides.id ?? 'rev-base';
  return {
    id,
    parentRevisionId: null,
    stage1BaseId: id,
    drySha256: overrides.drySha256 ?? 'a'.repeat(64),
    dryWavUrl: overrides.dryWavUrl ?? `/api/media/${id}.wav`,
    cuesJsonUrl: overrides.cuesJsonUrl ?? `/api/media/${id}.json`,
    cuesRunId: overrides.cuesRunId ?? null,
    isStage2Edit: false,
    label: overrides.label ?? 'Stage 1 dry',
    createdAt: '2026-11-27T00:00:00.000Z',
    ...overrides,
  };
}

test('H4: SessionEpochTracker rejects stale async completions after session switch or close', () => {
  const tracker = new SessionEpochTracker();
  const epochA = tracker.beginSession('sess-A');
  assert.equal(tracker.isCurrent(epochA, 'sess-A'), true);

  const epochB = tracker.beginSession('sess-B');
  assert.notEqual(epochA, epochB);
  assert.equal(tracker.isCurrent(epochA, 'sess-A'), false);
  assert.equal(tracker.isCurrent(epochA), false);
  assert.equal(tracker.isCurrent(epochB, 'sess-A'), false);
  assert.equal(tracker.isCurrent(epochB, 'sess-B'), true);

  const epochClosed = tracker.closeSession();
  assert.equal(tracker.sessionId(), null);
  assert.equal(tracker.isCurrent(epochB, 'sess-B'), false);
  assert.equal(tracker.isCurrent(epochClosed, null), true);
});

test('H5: dry approval is bound strictly to revision ID + SHA-256 and dependent artifacts detect staleness', () => {
  const rev1 = makeRevision({ id: 'rev-1', drySha256: '1'.repeat(64) });
  const rev2 = makeRevision({ id: 'rev-2', drySha256: '2'.repeat(64), isStage2Edit: true });

  const approval: RevisionApproval = {
    approvedDryRevisionId: rev1.id,
    approvedDrySha256: rev1.drySha256,
    approvedAt: '2026-11-27T00:01:00.000Z',
  };

  assert.equal(isApprovalValidForRevision(approval, rev1), true);
  assert.equal(isApprovalValidForRevision(approval, rev2), false);
  assert.equal(isApprovalValidForRevision(approval, { ...rev1, drySha256: '9'.repeat(64) }), false);
  assert.equal(isApprovalValidForRevision(null, rev1), false);

  const pipeProv = {
    parentDryRevisionId: rev1.id,
    parentDrySha256: rev1.drySha256,
    configKey: 'rig-cfg-1',
  };
  assert.equal(isArtifactStaleForDry(pipeProv, rev1, 'rig-cfg-1'), false);
  assert.equal(isArtifactStaleForDry(pipeProv, rev1, 'rig-cfg-2'), true);
  assert.equal(isArtifactStaleForDry(pipeProv, rev2, 'rig-cfg-1'), true);
});

test('H7: committed dry audio undo/redo and Stage 2 bypass preserve Stage 1 base deterministically', () => {
  assert.equal(getEffectiveDryRevision(createInitialAudioHistory()), null);

  const base = makeRevision({ id: 'stage1-v1', drySha256: '1'.repeat(64) });
  let hist = initAudioHistoryWithBase(base);
  assert.equal(getEffectiveDryRevision(hist)?.id, 'stage1-v1');

  hist = pushStage2DryEditRevision(hist, {
    id: 'stage2-e1',
    drySha256: '2'.repeat(64),
    dryWavUrl: '/api/media/e1.wav',
    cuesJsonUrl: '/api/media/e1.json',
    cuesRunId: 'run-e1',
    label: 'Smooth region 1',
    createdAt: '2026-11-27T00:02:00.000Z',
  });
  hist = pushStage2DryEditRevision(hist, {
    id: 'stage2-e2',
    drySha256: '3'.repeat(64),
    dryWavUrl: '/api/media/e2.wav',
    cuesJsonUrl: '/api/media/e2.json',
    cuesRunId: 'run-e2',
    label: 'Straighten region 2',
    createdAt: '2026-11-27T00:03:00.000Z',
  });

  assert.equal(getEffectiveDryRevision(hist)?.id, 'stage2-e2');
  assert.equal(hist.stage1Base?.id, 'stage1-v1');

  // Stage 2 bypass returns unedited Stage 1 base without mutating undo history
  hist = setStage2Bypass(hist, true);
  assert.equal(getEffectiveDryRevision(hist)?.id, 'stage1-v1');
  hist = setStage2Bypass(hist, false);
  assert.equal(getEffectiveDryRevision(hist)?.id, 'stage2-e2');

  // Undo twice returns to Stage 1 base; redo steps forward cleanly
  hist = undoAudioRevision(hist);
  assert.equal(getEffectiveDryRevision(hist)?.id, 'stage2-e1');
  hist = undoAudioRevision(hist);
  assert.equal(getEffectiveDryRevision(hist)?.id, 'stage1-v1');

  hist = redoAudioRevision(hist);
  assert.equal(getEffectiveDryRevision(hist)?.id, 'stage2-e1');

  // A new Stage 1 regeneration clears redo stack and updates stage1Base
  hist = pushStage1DryRevision(hist, {
    id: 'stage1-v2',
    drySha256: '4'.repeat(64),
    dryWavUrl: '/api/media/s2.wav',
    cuesJsonUrl: '/api/media/s2.json',
    cuesRunId: 'run-s2',
    label: 'Source voicing regen',
    createdAt: '2026-11-27T00:04:00.000Z',
  });
  assert.equal(hist.future.length, 0);
  assert.equal(hist.stage1Base?.id, 'stage1-v2');
  assert.equal(getEffectiveDryRevision(hist)?.id, 'stage1-v2');
});

test('H6: ProjectFileV2 validation distinguishes v2 projects, v1 presets, corrupt JSON, and SHA mismatches', () => {
  const base = makeRevision({ id: 'stage1-base', drySha256: 'a'.repeat(64) });
  const project: ProjectFileV2 = {
    schemaVersion: PROJECT_SCHEMA_VERSION,
    kind: 'spectracraft_project',
    savedAt: '2026-11-27T00:00:00.000Z',
    source: {
      sessionId: 'sess-1',
      label: 'take_48k.wav',
      sourceSha256: 'f'.repeat(64),
      sampleRate: 48000,
      durationS: 4.5,
    },
    cues: {
      cuesSha256: 'c'.repeat(64),
      activeCuesRunId: 'run-1',
      patches: {},
    },
    stage1: {
      sourceIndex: 0,
      voicing: DEFAULT_VOICING_CONTROLS,
      baseDryRevision: base,
    },
    stage2: {
      activeDryRevision: base,
      stage2Bypassed: false,
      regions: [],
      editTransactions: [],
    },
    postDry: {
      rig: makeRoomPreset('small'),
      pipelineStages: DEFAULT_PIPELINE_STAGES,
      lastPipelineRunId: null,
      lastPipelineSha256: null,
    },
    approval: null,
  };

  const exactRes = parseAndValidateProjectFile(JSON.stringify(project), 'f'.repeat(64));
  assert.equal(exactRes.ok, true);
  if (exactRes.ok) {
    assert.equal(exactRes.kind, 'project_v2');
    if (exactRes.kind === 'project_v2') {
      assert.equal(exactRes.assetMatch, 'exact');
      assert.equal(exactRes.warnings.length, 0);
    }
  }

  const mismatchRes = parseAndValidateProjectFile(JSON.stringify(project), '0'.repeat(64));
  assert.equal(mismatchRes.ok, true);
  if (mismatchRes.ok && mismatchRes.kind === 'project_v2') {
    assert.equal(mismatchRes.assetMatch, 'source_mismatch');
    assert.ok(mismatchRes.warnings[0].includes('SHA-256 mismatch'));
  }

  // Legacy v1 settings-only file is explicitly classified as a settings preset
  const v1Res = parseAndValidateProjectFile(JSON.stringify({ schemaVersion: 1, rig: { sourceIndex: 2 } }));
  assert.equal(v1Res.ok, true);
  if (v1Res.ok) {
    assert.equal(v1Res.kind, 'settings_preset_v1');
    assert.ok(v1Res.warnings[0].includes('settings-only'));
  }

  const badJson = parseAndValidateProjectFile('{not json');
  assert.equal(badJson.ok, false);
  if (!badJson.ok) assert.equal(badJson.code, 'INVALID_JSON');

  const corrupt = parseAndValidateProjectFile(
    JSON.stringify({ schemaVersion: 2, kind: 'spectracraft_project', source: {} }),
  );
  assert.equal(corrupt.ok, false);
  if (!corrupt.ok) assert.equal(corrupt.code, 'CORRUPT_PROJECT');
});
