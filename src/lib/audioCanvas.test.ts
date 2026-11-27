import assert from 'node:assert/strict';
import test from 'node:test';
import {
  buildSpectrogramPixels,
  computeWaveformColumns,
  getEffectiveBufferSampleRate,
  SpectrogramTileCache,
} from './audioCanvas.ts';

function generateSine(freqHz: number, sampleRate: number, durationS: number): Float32Array {
  const len = Math.floor(sampleRate * durationS);
  const out = new Float32Array(len);
  for (let i = 0; i < len; i++) {
    out[i] = 0.8 * Math.sin((2 * Math.PI * freqHz * i) / sampleRate);
  }
  return out;
}

function findBrightestRow(data: Uint8ClampedArray, width: number, height: number): number {
  const midX = Math.floor(width / 2);
  let bestY = -1;
  let bestLuma = -1;
  for (let y = 0; y < height; y++) {
    const p = (y * width + midX) * 4;
    const luma = data[p] + data[p + 1] + data[p + 2];
    if (luma > bestLuma) {
      bestLuma = luma;
      bestY = y;
    }
  }
  return bestY;
}

test('H8: decoded buffer sampleRate is always authoritative across 8, 44.1, 48, and 96 kHz', () => {
  for (const sr of [8000, 44100, 48000, 96000]) {
    assert.equal(getEffectiveBufferSampleRate({ sampleRate: sr }, 48000), sr);

    // Place a 1000 Hz sine tone in a 0.5 s buffer at this exact decoded rate
    const pcm = generateSine(1000, sr, 0.5);
    const displayMaxHz = 4000;
    const height = 200;
    const width = 80;
    const spec = buildSpectrogramPixels({
      pcm,
      sampleRate: sr,
      viewStartS: 0.05,
      viewEndS: 0.45,
      displayMaxHz,
      width,
      height,
      fftSize: 1024,
    });

    const brightestY = findBrightestRow(spec.data, width, height);
    // Expected row: y = (1 - 1000 / 4000) * height = 0.75 * 200 = 150
    const expectedY = (1 - 1000 / displayMaxHz) * height;
    assert.ok(
      Math.abs(brightestY - expectedY) <= 6,
      `At ${sr} Hz, brightest row ${brightestY} deviated too far from expected ${expectedY}`,
    );

    // Waveform column indexing with decoded sampleRate covers the exact [0.1s, 0.2s] impulse
    const impulse = new Float32Array(Math.floor(sr * 0.5));
    impulse[Math.floor(sr * 0.15)] = 0.95;
    const cols = computeWaveformColumns(impulse, sr, 0.1, 0.2, 10);
    const peakCol = cols.findIndex((c) => c.max > 0.8);
    assert.ok(peakCol >= 4 && peakCol <= 5, `Impulse at 0.15s should land at center column, got ${peakCol}`);
  }
});

test('H8: getEffectiveBufferSampleRate falls back only when buffer is missing or invalid', () => {
  assert.equal(getEffectiveBufferSampleRate(null, 44100), 44100);
  assert.equal(getEffectiveBufferSampleRate({ sampleRate: 0 }, 44100), 44100);
  assert.equal(getEffectiveBufferSampleRate({ sampleRate: Number.NaN }, 44100), 44100);
});

test('M3: SpectrogramTileCache executes ZERO FFT computations during playhead-only redraws', () => {
  const cache = new SpectrogramTileCache(8);
  const pcm = generateSine(440, 48000, 1.0);
  const params = {
    pcm,
    sampleRate: 48000,
    viewStartS: 0,
    viewEndS: 1.0,
    displayMaxHz: 6000,
    width: 160,
    height: 100,
  };

  const first = cache.getOrCompute(params);
  assert.ok(first.fftCount > 0);
  assert.equal(cache.cacheMisses, 1);
  assert.equal(cache.cacheHits, 0);
  const fftsAfterInitialRender = cache.totalFftComputations;

  // Simulate 60 frames of playhead movement with unchanged viewport, buffer, and scale
  for (let frame = 0; frame < 60; frame++) {
    const cached = cache.getOrCompute(params);
    assert.strictEqual(cached, first);
  }

  assert.equal(cache.cacheHits, 60);
  assert.equal(cache.cacheMisses, 1);
  assert.equal(
    cache.totalFftComputations,
    fftsAfterInitialRender,
    'Playhead-only animation must run 0 additional FFTs',
  );

  // Changing zoom/viewport triggers a single fresh computation
  const zoomed = cache.getOrCompute({ ...params, viewStartS: 0.2, viewEndS: 0.6 });
  assert.notStrictEqual(zoomed, first);
  assert.equal(cache.cacheMisses, 2);
  assert.ok(cache.totalFftComputations > fftsAfterInitialRender);
});
