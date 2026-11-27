import assert from 'node:assert/strict';
import test from 'node:test';
import {
  computeCrossfadeOffsetS,
  computePlaybackPositionS,
  type PlaybackClockAnchor,
} from './useAudioEngine.ts';

test('M12: playback position tracks AudioContext.currentTime cleanly and caps at active buffer duration', () => {
  const anchor: PlaybackClockAnchor = {
    startCtxTimeS: 10.0,
    startOffsetS: 0.25,
    activeDurationS: 2.5,
  };

  assert.equal(computePlaybackPositionS(anchor, 10.0), 0.25);
  assert.ok(Math.abs(computePlaybackPositionS(anchor, 10.8) - 1.05) < 1e-9);
  assert.equal(computePlaybackPositionS(anchor, 14.0), 2.5);
  // Clamps safely if clock hasn't advanced past startCtxTimeS
  assert.equal(computePlaybackPositionS(anchor, 9.5), 0.25);
});

test('M12: switching from a 1.0 s dry buffer to a 2.5 s tail-preserving capture advances past 1.0 s', () => {
  // Playing dry buffer (1.0 s) at offset 0.85 s
  const dryAnchor: PlaybackClockAnchor = {
    startCtxTimeS: 5.0,
    startOffsetS: 0.85,
    activeDurationS: 1.0,
  };
  // Crossfade occurs at ctxTime = 5.05 -> live pos = 0.90 s
  const crossfadeOffset = computeCrossfadeOffsetS(dryAnchor, 5.05, 2.5);
  assert.ok(Math.abs(crossfadeOffset - 0.9) < 1e-9);

  // New anchor uses the destination buffer's 2.5 s duration, NOT the old 1.0 s dry duration
  const captureAnchor: PlaybackClockAnchor = {
    startCtxTimeS: 5.05,
    startOffsetS: crossfadeOffset,
    activeDurationS: 2.5,
  };
  const posInTail = computePlaybackPositionS(captureAnchor, 6.05);
  assert.ok(Math.abs(posInTail - 1.9) < 1e-9, `Expected playhead to advance into reverb tail at 1.9s, got ${posInTail}`);
  assert.equal(computePlaybackPositionS(captureAnchor, 7.2), 2.5);
});

test('M12: switching from a longer capture tail back to a shorter dry buffer clamps crossfade safely', () => {
  const captureAnchor: PlaybackClockAnchor = {
    startCtxTimeS: 2.0,
    startOffsetS: 1.8,
    activeDurationS: 3.0,
  };
  const nextOffset = computeCrossfadeOffsetS(captureAnchor, 2.1, 1.0);
  assert.ok(Math.abs(nextOffset - 0.98) < 1e-9);
});
