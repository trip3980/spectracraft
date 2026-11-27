/**
 * Pure PCM visualization & Web Audio utility functions for SpectraCraft.
 * Operates strictly on real decoded AudioBuffer PCM from the Rust backend — never synthesizes fake plots.
 */

export interface WaveformColumn {
  min: number;
  max: number;
  rms: number;
}

export function computeWaveformColumns(
  pcm: Float32Array,
  sampleRate: number,
  viewStartS: number,
  viewEndS: number,
  numCols: number,
): WaveformColumn[] {
  const cols = Math.max(1, Math.floor(numCols));
  const out: WaveformColumn[] = new Array(cols);
  const total = pcm.length;
  if (total === 0 || viewEndS <= viewStartS) {
    for (let i = 0; i < cols; i++) out[i] = { min: 0, max: 0, rms: 0 };
    return out;
  }

  const startIdx = Math.max(0, Math.floor(viewStartS * sampleRate));
  const endIdx = Math.min(total, Math.ceil(viewEndS * sampleRate));
  const span = Math.max(1, endIdx - startIdx);
  const samplesPerCol = span / cols;

  for (let c = 0; c < cols; c++) {
    const i0 = Math.min(total - 1, Math.floor(startIdx + c * samplesPerCol));
    const i1 = Math.min(total, Math.max(i0 + 1, Math.floor(startIdx + (c + 1) * samplesPerCol)));
    const stride = Math.max(1, Math.floor((i1 - i0) / 128));
    let mn = 1;
    let mx = -1;
    let sumSq = 0;
    let count = 0;
    for (let i = i0; i < i1; i += stride) {
      const s = pcm[i];
      if (s < mn) mn = s;
      if (s > mx) mx = s;
      sumSq += s * s;
      count += 1;
    }
    if (count === 0) {
      out[c] = { min: 0, max: 0, rms: 0 };
    } else {
      out[c] = { min: mn, max: mx, rms: Math.sqrt(sumSq / count) };
    }
  }
  return out;
}

function fftRadix2(re: Float32Array, im: Float32Array): void {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    while (j & bit) {
      j ^= bit;
      bit >>= 1;
    }
    j ^= bit;
    if (i < j) {
      const tr = re[i];
      re[i] = re[j];
      re[j] = tr;
      const ti = im[i];
      im[i] = im[j];
      im[j] = ti;
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len;
    const wlenRe = Math.cos(ang);
    const wlenIm = Math.sin(ang);
    for (let i = 0; i < n; i += len) {
      let wRe = 1;
      let wIm = 0;
      const half = len >> 1;
      for (let j = 0; j < half; j++) {
        const uRe = re[i + j];
        const uIm = im[i + j];
        const vRe = re[i + j + half] * wRe - im[i + j + half] * wIm;
        const vIm = re[i + j + half] * wIm + im[i + j + half] * wRe;
        re[i + j] = uRe + vRe;
        im[i + j] = uIm + vIm;
        re[i + j + half] = uRe - vRe;
        im[i + j + half] = uIm - vIm;
        const nextRe = wRe * wlenRe - wIm * wlenIm;
        wIm = wRe * wlenIm + wIm * wlenRe;
        wRe = nextRe;
      }
    }
  }
}

const HANN_CACHE = new Map<number, Float32Array>();
function getHannWindow(n: number): Float32Array {
  let w = HANN_CACHE.get(n);
  if (!w) {
    w = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      w[i] = 0.5 * (1 - Math.cos((2 * Math.PI * i) / (n - 1)));
    }
    HANN_CACHE.set(n, w);
  }
  return w;
}

/**
 * Color LUT mapping normalized [0..255] dBFS intensity to an analytical charcoal-indigo-blue-amber ramp.
 */
const SPEC_LUT = (() => {
  const lut = new Uint8ClampedArray(256 * 3);
  const stops: [number, number, number, number][] = [
    [0.0, 18, 20, 24],
    [0.2, 24, 34, 58],
    [0.45, 36, 76, 132],
    [0.68, 55, 142, 212],
    [0.86, 224, 158, 64],
    [1.0, 255, 236, 180],
  ];
  for (let i = 0; i < 256; i++) {
    const t = i / 255;
    let s0 = stops[0];
    let s1 = stops[stops.length - 1];
    for (let k = 0; k < stops.length - 1; k++) {
      if (t >= stops[k][0] && t <= stops[k + 1][0]) {
        s0 = stops[k];
        s1 = stops[k + 1];
        break;
      }
    }
    const f = s1[0] === s0[0] ? 0 : (t - s0[0]) / (s1[0] - s0[0]);
    lut[i * 3] = Math.round(s0[1] + (s1[1] - s0[1]) * f);
    lut[i * 3 + 1] = Math.round(s0[2] + (s1[2] - s0[2]) * f);
    lut[i * 3 + 2] = Math.round(s0[3] + (s1[3] - s0[3]) * f);
  }
  return lut;
})();

export interface SpectrogramRenderParams {
  pcm: Float32Array;
  /** Decoded AudioBuffer.sampleRate (NEVER source file metadata when resampled by Web Audio). */
  sampleRate: number;
  viewStartS: number;
  viewEndS: number;
  displayMaxHz: number;
  width: number;
  height: number;
  fftSize?: number;
  dbFloor?: number;
  dbCeil?: number;
  /** Optional original file sample rate for Nyquist ceiling display when source < decoded rate. */
  sourceNyquistSampleRate?: number;
}

export interface SpectrogramPixelsResult {
  width: number;
  height: number;
  data: Uint8ClampedArray;
  fftCount: number;
}

/**
 * Always return the decoded AudioBuffer's own sampleRate for PCM indexing, FFT bin frequencies,
 * and duration calculations (Audit H8). Never index decoded PCM using original file metadata
 * when Web Audio resampled on decode.
 */
export function getEffectiveBufferSampleRate(
  buffer: { sampleRate: number } | null | undefined,
  fallbackSampleRate = 48000,
): number {
  const sr = buffer?.sampleRate;
  return typeof sr === 'number' && Number.isFinite(sr) && sr > 0 ? sr : fallbackSampleRate;
}

/**
 * Pure real-FFT spectrogram RGBA pixel generator (testable in Node without Canvas2D).
 */
export function buildSpectrogramPixels(params: SpectrogramRenderParams): SpectrogramPixelsResult {
  const {
    pcm,
    sampleRate,
    viewStartS,
    viewEndS,
    displayMaxHz,
    width,
    height,
    fftSize = 1024,
    dbFloor = -96,
    dbCeil = -12,
    sourceNyquistSampleRate,
  } = params;

  const data = new Uint8ClampedArray(Math.max(0, width * height * 4));
  if (width <= 0 || height <= 0 || pcm.length === 0 || sampleRate <= 0) {
    return { width, height, data, fftCount: 0 };
  }

  const decodedNyquist = sampleRate / 2;
  const effectiveNyquist =
    sourceNyquistSampleRate && sourceNyquistSampleRate > 0
      ? Math.min(decodedNyquist, sourceNyquistSampleRate / 2)
      : decodedNyquist;
  const maxBins = fftSize >> 1;
  const hann = getHannWindow(fftSize);
  const re = new Float32Array(fftSize);
  const im = new Float32Array(fftSize);

  const cols = Math.min(width, 480);
  const colStep = width / cols;
  const dbSpan = Math.max(1, dbCeil - dbFloor);
  const total = pcm.length;
  let fftCount = 0;

  for (let c = 0; c < cols; c++) {
    const tS = viewStartS + ((c + 0.5) / cols) * (viewEndS - viewStartS);
    const center = Math.floor(tS * sampleRate);
    const start = center - (fftSize >> 1);

    for (let i = 0; i < fftSize; i++) {
      const idx = start + i;
      re[i] = idx >= 0 && idx < total ? pcm[idx] * hann[i] : 0;
      im[i] = 0;
    }
    fftRadix2(re, im);
    fftCount += 1;

    const x0 = Math.floor(c * colStep);
    const x1 = Math.min(width, Math.ceil((c + 1) * colStep));

    for (let y = 0; y < height; y++) {
      const freq = displayMaxHz * (1 - y / Math.max(1, height - 1));
      if (freq > effectiveNyquist) {
        for (let x = x0; x < x1; x++) {
          const p = (y * width + x) * 4;
          const stripe = (x + y) % 8 < 2 ? 38 : 28;
          data[p] = stripe;
          data[p + 1] = stripe;
          data[p + 2] = stripe + 4;
          data[p + 3] = 255;
        }
        continue;
      }

      const binF = (freq / decodedNyquist) * maxBins;
      const b0 = Math.max(0, Math.min(maxBins - 1, Math.floor(binF)));
      const b1 = Math.min(maxBins - 1, b0 + 1);
      const frac = binF - b0;
      const m0 = Math.hypot(re[b0], im[b0]);
      const m1 = Math.hypot(re[b1], im[b1]);
      const mag = (m0 + (m1 - m0) * frac) / (fftSize * 0.25);
      const db = 20 * Math.log10(mag + 1e-9);
      const norm = Math.max(0, Math.min(1, (db - dbFloor) / dbSpan));
      const lutIdx = Math.round(norm * 255) * 3;
      const r = SPEC_LUT[lutIdx];
      const g = SPEC_LUT[lutIdx + 1];
      const b = SPEC_LUT[lutIdx + 2];

      for (let x = x0; x < x1; x++) {
        const p = (y * width + x) * 4;
        data[p] = r;
        data[p + 1] = g;
        data[p + 2] = b;
        data[p + 3] = 255;
      }
    }
  }

  return { width, height, data, fftCount };
}

/**
 * LRU Spectrogram tile cache keyed by audio buffer identity + viewport + scale parameters
 * so playhead-only animation executes ZERO FFT computations (Audit M3).
 */
export class SpectrogramTileCache {
  private readonly cache = new Map<string, SpectrogramPixelsResult>();
  private readonly bufferIds = new WeakMap<Float32Array, number>();
  private nextBufferId = 1;
  private readonly maxEntries: number;
  public totalFftComputations = 0;
  public cacheHits = 0;
  public cacheMisses = 0;

  constructor(maxEntries = 12) {
    this.maxEntries = Math.max(2, maxEntries);
  }

  private getBufferId(pcm: Float32Array): number {
    let id = this.bufferIds.get(pcm);
    if (!id) {
      id = this.nextBufferId++;
      this.bufferIds.set(pcm, id);
    }
    return id;
  }

  public makeKey(params: SpectrogramRenderParams): string {
    const bufId = this.getBufferId(params.pcm);
    return [
      bufId,
      params.pcm.length,
      params.sampleRate,
      params.sourceNyquistSampleRate ?? 0,
      params.viewStartS.toFixed(5),
      params.viewEndS.toFixed(5),
      Math.round(params.displayMaxHz),
      params.width,
      params.height,
      params.fftSize ?? 1024,
      params.dbFloor ?? -96,
      params.dbCeil ?? -12,
    ].join('|');
  }

  public getOrCompute(params: SpectrogramRenderParams): SpectrogramPixelsResult {
    const key = this.makeKey(params);
    const existing = this.cache.get(key);
    if (existing) {
      this.cacheHits += 1;
      // Refresh LRU ordering
      this.cache.delete(key);
      this.cache.set(key, existing);
      return existing;
    }
    this.cacheMisses += 1;
    const computed = buildSpectrogramPixels(params);
    this.totalFftComputations += computed.fftCount;
    this.cache.set(key, computed);
    if (this.cache.size > this.maxEntries) {
      const oldestKey = this.cache.keys().next().value;
      if (oldestKey !== undefined) {
        this.cache.delete(oldestKey);
      }
    }
    return computed;
  }

  public clear(): void {
    this.cache.clear();
  }
}

export const sharedSpectrogramTileCache = new SpectrogramTileCache(16);

/**
 * Renders a calibrated real-FFT spectrogram of `pcm` into the target canvas 2D context region,
 * reusing cached RGBA pixels whenever buffer, viewport, dimensions, and display scale are unchanged.
 */
export function renderSpectrogramToCanvas(
  ctx: CanvasRenderingContext2D,
  pcm: Float32Array,
  sampleRate: number,
  viewStartS: number,
  viewEndS: number,
  displayMaxHz: number,
  width: number,
  height: number,
  fftSize = 1024,
  dbFloor = -96,
  dbCeil = -12,
  sourceNyquistSampleRate?: number,
  tileCache: SpectrogramTileCache = sharedSpectrogramTileCache,
): void {
  if (width <= 0 || height <= 0 || pcm.length === 0) return;

  const pixels = tileCache.getOrCompute({
    pcm,
    sampleRate,
    viewStartS,
    viewEndS,
    displayMaxHz,
    width,
    height,
    fftSize,
    dbFloor,
    dbCeil,
    sourceNyquistSampleRate,
  });

  const img = ctx.createImageData(width, height);
  img.data.set(pixels.data);
  ctx.putImageData(img, 0, 0);
}

/**
 * Builds an actual sample-aligned Difference AudioBuffer (regenerated - original).
 */
export function buildDifferenceBuffer(
  ctx: AudioContext,
  original: AudioBuffer,
  regenerated: AudioBuffer,
): AudioBuffer {
  const len = Math.min(original.length, regenerated.length);
  const out = ctx.createBuffer(1, Math.max(1, len), original.sampleRate);
  const a = original.getChannelData(0);
  const b = regenerated.getChannelData(0);
  const dst = out.getChannelData(0);
  for (let i = 0; i < len; i++) {
    dst[i] = b[i] - a[i];
  }
  return out;
}

/**
 * Optionally quantizes a mono Float32Array to simulate preview bit depth (32f / 24 / 16) honestly.
 */
export function buildBitDepthBuffer(
  ctx: AudioContext,
  source: AudioBuffer,
  bits: 32 | 24 | 16,
): AudioBuffer {
  if (bits === 32) return source;
  const len = source.length;
  const out = ctx.createBuffer(1, len, source.sampleRate);
  const src = source.getChannelData(0);
  const dst = out.getChannelData(0);
  const levels = bits === 16 ? 32768 : 8388608;
  for (let i = 0; i < len; i++) {
    dst[i] = Math.round(Math.max(-1, Math.min(1, src[i])) * levels) / levels;
  }
  return out;
}
