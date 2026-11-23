import assert from 'node:assert/strict';
import test from 'node:test';
import {
  classifyNoteEvidence,
  computePipelineStage,
  countPassedDryCriteria,
  dbToSafeLinearGain,
  emptyDryListeningChecklist,
  isDryListeningPassed,
  summarizeEvidence,
} from './contract.ts';

test('Dry listening gate requires all 5 contract criteria before unlocking post-processing', () => {
  const empty = emptyDryListeningChecklist();
  assert.equal(countPassedDryCriteria(empty), 0);
  assert.equal(isDryListeningPassed(empty), false);

  const partial = {
    rhythm: true,
    phrasing: true,
    dynamics: true,
    sustain: true,
    artifactFree: false,
  };
  assert.equal(countPassedDryCriteria(partial), 4);
  assert.equal(isDryListeningPassed(partial), false);

  const full = { ...partial, artifactFree: true };
  assert.equal(countPassedDryCriteria(full), 5);
  assert.equal(isDryListeningPassed(full), true);
});

test('Pipeline stage enforces strict ordering: empty -> analysis -> cues -> dry_gate -> post_unlocked', () => {
  const empty = emptyDryListeningChecklist();
  const full = {
    rhythm: true,
    phrasing: true,
    dynamics: true,
    sustain: true,
    artifactFree: true,
  };

  assert.equal(
    computePipelineStage({ hasSession: false, noteCount: 0, hasDryAudio: false, checklist: empty }),
    'empty',
  );
  assert.equal(
    computePipelineStage({ hasSession: true, noteCount: 0, hasDryAudio: false, checklist: empty }),
    'analysis',
  );
  assert.equal(
    computePipelineStage({ hasSession: true, noteCount: 12, hasDryAudio: false, checklist: empty }),
    'cues',
  );
  assert.equal(
    computePipelineStage({ hasSession: true, noteCount: 12, hasDryAudio: true, checklist: empty }),
    'dry_gate',
  );
  assert.equal(
    computePipelineStage({ hasSession: true, noteCount: 12, hasDryAudio: true, checklist: full }),
    'post_unlocked',
  );
});

test('Evidence classification keeps strong, weak/consistent, and uncertain separate without collapsing ambiguity', () => {
  assert.equal(
    classifyNoteEvidence({ frames: 30, peak_level: 0.72, uncertain_overlap_count: 0 }),
    'strong_coherent',
  );
  assert.equal(
    classifyNoteEvidence({ frames: 30, peak_level: 0.15, uncertain_overlap_count: 0 }),
    'weak_consistent',
  );
  assert.equal(
    classifyNoteEvidence({ frames: 30, peak_level: 0.65, uncertain_overlap_count: 2 }),
    'weak_consistent',
  );
  assert.equal(
    classifyNoteEvidence({ frames: 20, peak_level: 0.65, uncertain_overlap_count: 8 }),
    'uncertain',
  );

  const summary = summarizeEvidence(
    [
      { frames: 40, peak_level: 0.8, uncertain_overlap_count: 0 },
      { frames: 40, peak_level: 0.15, uncertain_overlap_count: 1 },
      { frames: 12, peak_level: 0.5, uncertain_overlap_count: 5 },
    ],
    [
      { status: 'supported' },
      { status: 'supported' },
      { status: 'uncertain' },
      { status: 'out_of_target' },
    ],
  );
  assert.deepEqual(summary, {
    strongNotes: 1,
    weakNotes: 1,
    uncertainNotes: 1,
    supportedFrames: 2,
    uncertainFrames: 1,
    outOfTargetFrames: 1,
  });
});

test('dbToSafeLinearGain computes exact level-match gain and clamps extreme inputs', () => {
  assert.ok(Math.abs(dbToSafeLinearGain(0) - 1.0) < 1e-12);
  assert.ok(Math.abs(dbToSafeLinearGain(6.0205999) - 2.0) < 1e-4);
  assert.ok(Math.abs(dbToSafeLinearGain(-6.0205999) - 0.5) < 1e-4);
  assert.equal(dbToSafeLinearGain(Number.NaN), 1.0);
  assert.ok(dbToSafeLinearGain(99) <= Math.pow(10, 12 / 20) + 1e-9);
});
