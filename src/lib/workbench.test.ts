import test from 'node:test';
import assert from 'node:assert/strict';
import {
  ALL_VARIANTS,
  buildVariantSelection,
  computeEffectiveLayerGains,
  countUncertainOverlaps,
  formatDbfs,
  formatHz,
  sliceMonoWavBuffer,
  toggleVariantSelection,
  validateGapRepairRange,
  validateRepickInput,
} from './workbench';

function makeTestFloatWav(rate: number, samples: number[]): Uint8Array {
  const bytes = samples.length * 4;
  const buf = new Uint8Array(44 + bytes);
  const view = new DataView(buf.buffer);
  const ascii = (off: number, s: string) => {
    for (let i = 0; i < s.length; i++) buf[off + i] = s.charCodeAt(i);
  };
  ascii(0, 'RIFF');
  view.setUint32(4, 36 + bytes, true);
  ascii(8, 'WAVE');
  ascii(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 3, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, rate, true);
  view.setUint32(28, rate * 4, true);
  view.setUint16(32, 4, true);
  view.setUint16(34, 32, true);
  ascii(36, 'data');
  view.setUint32(40, bytes, true);
  for (let i = 0; i < samples.length; i++) {
    view.setFloat32(44 + i * 4, samples[i], true);
  }
  return buf;
}

test('variant selection supports single, subset, empty, and all', () => {
  assert.equal(buildVariantSelection([]), '');
  assert.equal(buildVariantSelection(['natural']), 'natural');
  assert.equal(buildVariantSelection(['analog', 'natural']), 'natural,analog');
  assert.equal(buildVariantSelection(ALL_VARIANTS), 'all');
  assert.deepEqual(toggleVariantSelection(['natural'], 'analog'), ['natural', 'analog']);
  assert.deepEqual(toggleVariantSelection(['natural', 'analog'], 'natural'), ['analog']);
});

test('repick validation enforces note boundaries and strength limits', () => {
  assert.equal(validateRepickInput(1.0, 0.3, 0.45), null);
  assert.ok(validateRepickInput(1.0, 0.0, 0.45) !== null);
  assert.ok(validateRepickInput(1.0, 1.0, 0.45) !== null);
  assert.ok(validateRepickInput(1.0, 0.3, 2.0) !== null);
  assert.ok(validateRepickInput(1.0, Number.NaN, 0.5) !== null);
});

test('layer mixer gain math respects solo, mute, and fixed headroom', () => {
  const base = [
    { id: 'pitch' as const, gain: 1.0, muted: false, solo: false },
    { id: 'ringing' as const, gain: 0.5, muted: true, solo: false },
    { id: 'texture' as const, gain: 1.2, muted: false, solo: false },
    { id: 'breath_accent' as const, gain: 1.0, muted: false, solo: false },
  ];
  const normal = computeEffectiveLayerGains(base, 0.707);
  assert.equal(normal.pitch, 0.707);
  assert.equal(normal.ringing, 0);
  assert.ok(Math.abs(normal.texture - 1.2 * 0.707) < 1e-9);

  const soloed = computeEffectiveLayerGains(
    base.map((c) => (c.id === 'pitch' ? { ...c, solo: true } : c)),
    0.707,
  );
  assert.equal(soloed.pitch, 0.707);
  assert.equal(soloed.texture, 0);
  assert.equal(soloed.breath_accent, 0);
});

test('sliceMonoWavBuffer extracts exact float32 window without mutating source', () => {
  const rate = 16000;
  const total = rate * 2;
  const samples = Array.from({ length: total }, (_, i) => i / total);
  const src = makeTestFloatWav(rate, samples);
  const snapshot = new Uint8Array(src);

  const sliced = sliceMonoWavBuffer(src, 0.5, 1.25);
  assert.equal(sliced.sampleRate, rate);
  assert.equal(sliced.samples, Math.round(0.75 * rate));
  assert.deepEqual(src, snapshot, 'source WAV buffer must remain byte-for-byte unchanged');

  const outView = new DataView(sliced.buffer.buffer, sliced.buffer.byteOffset, sliced.buffer.byteLength);
  const firstSample = outView.getFloat32(44, true);
  assert.ok(Math.abs(firstSample - 0.5 * rate / total) < 1e-6);
});

test('sliceMonoWavBuffer supports WAVE_FORMAT_EXTENSIBLE IEEE float WAVs', () => {
  const rate = 44100;
  const total = rate;
  const dataBytes = total * 4;
  const buf = new Uint8Array(68 + dataBytes);
  const view = new DataView(buf.buffer);
  const ascii = (off: number, s: string) => {
    for (let i = 0; i < s.length; i++) buf[off + i] = s.charCodeAt(i);
  };
  ascii(0, 'RIFF');
  view.setUint32(4, 60 + dataBytes, true);
  ascii(8, 'WAVE');
  ascii(12, 'fmt ');
  view.setUint32(16, 40, true);
  view.setUint16(20, 0xfffe, true); // WAVE_FORMAT_EXTENSIBLE
  view.setUint16(22, 1, true);
  view.setUint32(24, rate, true);
  view.setUint32(28, rate * 4, true);
  view.setUint16(32, 4, true);
  view.setUint16(34, 32, true);
  view.setUint16(36, 22, true);
  view.setUint16(38, 32, true);
  view.setUint32(40, 4, true);
  view.setUint16(44, 3, true); // SubFormat = IEEE Float
  ascii(60, 'data');
  view.setUint32(64, dataBytes, true);
  for (let i = 0; i < total; i++) {
    view.setFloat32(68 + i * 4, 0.25, true);
  }
  const sliced = sliceMonoWavBuffer(buf, 0.1, 0.6);
  assert.equal(sliced.sampleRate, rate);
  assert.equal(sliced.samples, Math.round(0.5 * rate));
  const outView = new DataView(sliced.buffer.buffer);
  assert.ok(Math.abs(outView.getFloat32(44, true) - 0.25) < 1e-6);
});

test('formatting helpers handle finite and abstained values cleanly', () => {
  assert.equal(formatDbfs(-3.0116), '-3.01 dBFS');
  assert.equal(formatHz(null), 'Abstained');
  assert.equal(formatHz(440), '440.0 Hz');
});

test('validateGapRepairRange enforces Rust 0.25-4.0 ms width and 48 ms edge context', () => {
  const rate = 44100;
  const total = rate * 10; // 10 seconds

  const ok = validateGapRepairRange(rate, total, 2.5, 2.0);
  assert.equal(ok.ok, true);
  if (ok.ok) {
    assert.equal(ok.startFrame, Math.round(2.5 * rate));
    assert.equal(ok.repairedFrames, Math.round(0.002 * rate));
    assert.equal(ok.endFrame, ok.startFrame + ok.repairedFrames);
  }

  const tooShort = validateGapRepairRange(rate, total, 2.5, 0.1);
  assert.equal(tooShort.ok, false);

  const tooLong = validateGapRepairRange(rate, total, 2.5, 5.5);
  assert.equal(tooLong.ok, false);

  const nearStart = validateGapRepairRange(rate, total, 0.01, 1.5);
  assert.equal(nearStart.ok, false);

  const nearEnd = validateGapRepairRange(rate, total, 9.98, 1.5);
  assert.equal(nearEnd.ok, false);
});

test('countUncertainOverlaps counts only uncertain frames inside note span', () => {
  const frames = [
    { time_s: 1.0, status: 'uncertain' as const },
    { time_s: 1.2, status: 'supported' as const },
    { time_s: 1.35, status: 'uncertain' as const },
    { time_s: 1.5, status: 'out_of_target' as const },
    { time_s: 1.6, status: 'uncertain' as const },
    { time_s: 2.1, status: 'uncertain' as const },
  ];
  assert.equal(countUncertainOverlaps(1.1, 1.8, frames), 2);
  assert.equal(countUncertainOverlaps(3.0, 4.0, frames), 0);
});
