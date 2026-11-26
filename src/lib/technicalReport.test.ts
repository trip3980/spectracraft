import test from 'node:test';
import assert from 'node:assert/strict';
import {
  divergingDiffRgb,
  formatBytes,
  formatFreqLabel,
  magmaRgb,
  sampleSpectrogramProbe,
  validatePlotDimensions,
  type ReportViewerPlots,
} from './technicalReport';

test('magmaRgb maps normalized [0, 1] range monotonically in brightness without overflow', () => {
  const lo = magmaRgb(0);
  const mid = magmaRgb(0.5);
  const hi = magmaRgb(1);
  for (const [r, g, b] of [lo, mid, hi, magmaRgb(-5), magmaRgb(99), magmaRgb(Number.NaN)]) {
    assert.ok(r >= 0 && r <= 255);
    assert.ok(g >= 0 && g <= 255);
    assert.ok(b >= 0 && b <= 255);
  }
  const lum = ([r, g, b]: [number, number, number]) => 0.2126 * r + 0.7152 * g + 0.0722 * b;
  assert.ok(lum(lo) < lum(mid) && lum(mid) < lum(hi));
});

test('divergingDiffRgb distinguishes positive boost (amber) from negative cut (cyan)', () => {
  const neutral = divergingDiffRgb(0, 24);
  const boost = divergingDiffRgb(18, 24);
  const cut = divergingDiffRgb(-18, 24);
  assert.ok(boost[0] > boost[2], 'positive delta should skew warm/amber');
  assert.ok(cut[2] > cut[0], 'negative delta should skew cool/cyan');
  assert.deepEqual(neutral, [24, 28, 36]);
});

test('formatFreqLabel and formatBytes produce concise technical labels', () => {
  assert.equal(formatFreqLabel(100), '100');
  assert.equal(formatFreqLabel(1000), '1k');
  assert.equal(formatFreqLabel(12500), '12.5k');
  assert.equal(formatBytes(512), '512 B');
  assert.equal(formatBytes(102400), '100.0 KB');
});

test('validatePlotDimensions checks waveform and spectrogram grid integrity', () => {
  const valid: ReportViewerPlots = {
    sample_rate: 48000,
    duration_s: 10,
    max_hz: 20000,
    waveform_scale_linear: 0.5,
    waveform_bins: 2,
    waveform_before: [
      [-0.2, 0.2],
      [-0.3, 0.3],
    ],
    waveform_after: [
      [-0.1, 0.1],
      [-0.25, 0.25],
    ],
    residual_waveform: [
      [-0.1, 0.1],
      [-0.05, 0.05],
    ],
    spectrogram_cols: 2,
    spectrogram_rows: 2,
    spectrogram_freq_hz: [100, 1000],
    spectrogram_db_range: [-96, 0],
    diff_db_range: [-24, 24],
    spectrogram_before_db: [-40, -30, -20, -10],
    spectrogram_after_db: [-42, -28, -21, -12],
    spectrogram_diff_db: [-2, 2, -1, -2],
    third_octave: {
      centers_hz: [100, 200],
      before_db: [-20, -20],
      after_db: [-18, -19],
      delta_db: [2, 1],
      db_range: [-96, 0],
    },
    upper_band: {
      band_hz: [4000, 16000],
      times_s: [0, 5],
      before_phase_coherence: [0.8, 0.8],
      after_phase_coherence: [0.75, 0.78],
      before_modulation_depth: [0.1, 0.1],
      after_modulation_depth: [0.12, 0.11],
    },
  };
  assert.equal(validatePlotDimensions(valid), true);
  assert.equal(validatePlotDimensions({ ...valid, spectrogram_diff_db: [] }), false);
});

test('sampleSpectrogramProbe reads exact grid cells and rejects out-of-bounds coordinates', () => {
  const grid = [-40, -30, -20, -10];
  const lowLeft = sampleSpectrogramProbe({
    uX: 0.1,
    uY: 0.1,
    grid,
    cols: 2,
    rows: 2,
    durationS: 10,
    maxHz: 20000,
    freqHz: [100, 1000],
  });
  assert.ok(lowLeft);
  assert.equal(lowLeft.col, 0);
  assert.equal(lowLeft.row, 0);
  assert.equal(lowLeft.freqHz, 100);
  assert.equal(lowLeft.valueDb, -40);
  assert.equal(lowLeft.timeS, 1);

  const topRight = sampleSpectrogramProbe({
    uX: 0.99,
    uY: 0.99,
    grid,
    cols: 2,
    rows: 2,
    durationS: 10,
    maxHz: 20000,
    freqHz: [100, 1000],
  });
  assert.ok(topRight);
  assert.equal(topRight.col, 1);
  assert.equal(topRight.row, 1);
  assert.equal(topRight.freqHz, 1000);
  assert.equal(topRight.valueDb, -10);

  assert.equal(
    sampleSpectrogramProbe({
      uX: -0.1,
      uY: 0.5,
      grid,
      cols: 2,
      rows: 2,
      durationS: 10,
      maxHz: 20000,
    }),
    null,
  );
  assert.equal(
    sampleSpectrogramProbe({
      uX: 0.5,
      uY: 1.2,
      grid,
      cols: 2,
      rows: 2,
      durationS: 10,
      maxHz: 20000,
    }),
    null,
  );
});
