export type VariantKey = 'original' | 'natural' | 'digital' | 'analog' | 'expressive';

export const ALL_VARIANTS: readonly VariantKey[] = [
  'original',
  'natural',
  'digital',
  'analog',
  'expressive',
] as const;

export interface VariantDescriptor {
  key: VariantKey;
  title: string;
  subtitle: string;
  pathSummary: string;
}

export const VARIANT_DESCRIPTORS: readonly VariantDescriptor[] = [
  {
    key: 'original',
    title: 'Original',
    subtitle: 'Byte-for-byte unchanged copy',
    pathSummary: 'SHA-256 verified against the source WAV; original audio is never overwritten.',
  },
  {
    key: 'natural',
    title: 'Natural',
    subtitle: 'Clean amp-to-mic capture',
    pathSummary:
      'Labeled approximation of an SM7-class cardioid dynamic mic at ~2 ft in a padded 10 x 10 x 9 ft room.',
  },
  {
    key: 'digital',
    title: 'Digital',
    subtitle: 'Clean direct / modeled path',
    pathSummary: 'Direct linear modeled buffer with crisp articulation and no room reflections.',
  },
  {
    key: 'analog',
    title: 'Analog',
    subtitle: 'Nonlinear triode & booth capture',
    pathSummary:
      '2x oversampled asymmetric triode saturation, dynamic supply sag, passive tone stack, and close booth. Zero synthetic hum.',
  },
  {
    key: 'expressive',
    title: 'Expressive',
    subtitle: 'Pre-capture gesture edits + Natural',
    pathSummary:
      'Mutates phrasing contrast, pick accents, re-pick strength, and steady-note sustain vibrato in cues before synthesis.',
  },
] as const;

export interface Strike {
  offset_s: number;
  strength: number;
}

export interface PitchHypothesis {
  hz: number;
  score: number;
  harmonic_hits: number;
  nsdf: number;
}

export type EvidenceStatus = 'supported' | 'uncertain' | 'out_of_target' | 'silence';

export interface FrameEvidence {
  time_s: number;
  status: EvidenceStatus;
  selected_hz?: number | null;
  hypotheses: PitchHypothesis[];
  reason: string;
}

export interface NoteSummary {
  index: number;
  start: number;
  end: number;
  duration_s: number;
  median_hz: number;
  min_hz: number;
  max_hz: number;
  peak_level: number;
  re_picks: Strike[];
  frames: number;
  uncertain_overlap_count: number;
}

export interface GapRepairResult {
  runId: string;
  status: 'filled' | 'declined';
  startS: number;
  gapMs: number;
  startFrame: number;
  endFrame: number;
  repairedFrames: number;
  sampleRate: number;
  sourceSha256: string;
  repairedSha256: string | null;
  repairedWavUrl: string | null;
  unaffectedPreserved: boolean;
  message: string;
}

export interface CueSummary {
  rate: number;
  samples: number;
  duration_s: number;
  hop_s: number;
  note_count: number;
  supported_frames: number;
  uncertain_frames: number;
  out_of_target_frames: number;
  silence_frames: number;
  total_repicks: number;
  overlapping_ring_notes: number;
}

export interface IndependentPitchReport {
  hop_s: number;
  start_s: number;
  end_s: number;
  total_frames: number;
  both_resolved_frames: number;
  agreed_within_100_cents: number;
  octave_errors: number;
  disagreed_non_octave: number;
  unresolved_frames: number;
  unresolved_reference: number;
  unresolved_dry: number;
  unresolved_both: number;
  median_abs_cents_when_resolved: number | null;
  envelope_correlation: number | null;
}

export interface TransitionWindowSummary {
  window_s: [number, number];
  frames: number;
  supported: number;
  uncertain: number;
  out_of_target: number;
  silence: number;
  frames_with_competing_hypotheses: number;
}

export interface VerificationReport {
  source_path: string;
  source_sha256: string;
  source_unchanged: boolean;
  sample_rate: number;
  samples: number;
  duration_s: number;
  source_peak_dbfs: number;
  source_rms_dbfs: number;
  dry_candidate_sha256: string;
  cues_sha256?: string | null;
  dry_peak_dbfs: number;
  dry_rms_dbfs: number;
  full_duration_match: boolean;
  headroom_ok: boolean;
  cue_only_rerun_identical: boolean;
  ab_dry_match_gain_db: number;
  ab_common_headroom_gain_db: number;
  cue_summary: CueSummary;
  independent_pitch_full: IndependentPitchReport;
  independent_pitch_20_27s: IndependentPitchReport | null;
  transition_25_26s: TransitionWindowSummary | null;
  audio_status: string;
  listening_verdict: string;
}

export interface VariantExportEntry {
  variant: VariantKey;
  label: string;
  file: string;
  sha256: string;
  sample_rate: number;
  samples: number;
  peak_linear: number;
  peak_dbfs: number;
  rms_dbfs: number;
  headroom_below_0dbfs_db: number;
  unmastered: boolean;
  unchanged_copy_of_original: boolean;
  url?: string;
}

export interface VariantsManifest {
  source_sha256: string;
  source_unchanged: boolean;
  selection: string;
  detail_window_s: [number, number];
  shared_waveform_scale_linear: number;
  shared_spectrogram_scale_dbfs: [number, number];
  exports: VariantExportEntry[];
  audio_status: string;
  figures?: {
    waveformsFull: string;
    waveformsDetail: string;
    spectrogramsFull: string;
    spectrogramsDetail: string;
  };
  technicalReport?: string;
}

export interface SessionPayload {
  id: string;
  label: string;
  isReference: boolean;
  attribution: string | null;
  verification: VerificationReport;
  reportMarkdown: string;
  files: {
    sourceWav: string;
    dryWav: string;
    abWav: string;
    cuesJson: string;
    waveformsFull: string;
    waveformsDetail: string | null;
    spectrogramsFull: string;
    spectrogramsDetail: string | null;
    spectrogramsTransition: string | null;
  };
  notes: NoteSummary[];
  transitionEvidence: FrameEvidence[];
  variants: VariantsManifest | null;
}

export interface NoteEditPatch {
  index: number;
  level_scale: number;
  re_picks: Strike[];
}

export interface LayerChannel {
  id: 'pitch' | 'ringing' | 'texture' | 'breath_accent';
  label: string;
  description: string;
  url: string;
  gain: number;
  muted: boolean;
  solo: boolean;
}

export interface LayerSessionResult {
  runId: string;
  startS: number;
  endS: number;
  faintRidges: boolean;
  mixUrl: string;
  analysisJsonUrl: string;
  stems: Array<{
    id: 'pitch' | 'ringing' | 'texture' | 'breath_accent';
    label: string;
    description: string;
    url: string;
  }>;
}

export function buildVariantSelection(selected: readonly VariantKey[]): string {
  const unique = ALL_VARIANTS.filter((k) => selected.includes(k));
  if (unique.length === 0) {
    return '';
  }
  if (unique.length === ALL_VARIANTS.length) {
    return 'all';
  }
  return unique.join(',');
}

export function toggleVariantSelection(
  current: readonly VariantKey[],
  key: VariantKey,
): VariantKey[] {
  if (current.includes(key)) {
    return current.filter((item) => item !== key);
  }
  return ALL_VARIANTS.filter((item) => current.includes(item) || item === key);
}

export function validateRepickInput(
  noteDurationS: number,
  offsetS: number,
  strength: number,
): string | null {
  if (!Number.isFinite(offsetS) || !Number.isFinite(strength)) {
    return 'Offset and strength must be finite numbers.';
  }
  if (offsetS < 0.005 || offsetS >= Math.max(0.01, noteDurationS - 0.005)) {
    return `Offset must sit inside the note (0.005 s to ${Math.max(0.01, noteDurationS - 0.005).toFixed(3)} s).`;
  }
  if (strength < 0.05 || strength > 1.5) {
    return 'Strike strength must be between 0.05 and 1.50.';
  }
  return null;
}

export type GapRangeValidation =
  | { ok: true; startFrame: number; endFrame: number; repairedFrames: number; contextFrames: number }
  | { ok: false; error: string };

/**
 * Mirrors Rust `src/gap.rs` bounds:
 * - explicit loss width must be 0.25..=4.0 ms
 * - at least ~48 ms valid context on both sides of [startFrame, endFrame)
 */
export function validateGapRepairRange(
  sampleRate: number,
  totalSamples: number,
  startS: number,
  gapMs: number,
): GapRangeValidation {
  if (!Number.isFinite(sampleRate) || sampleRate < 8000 || totalSamples <= 0) {
    return { ok: false, error: 'Invalid audio sample rate or duration.' };
  }
  if (!Number.isFinite(startS) || !Number.isFinite(gapMs)) {
    return { ok: false, error: 'Start time and gap width must be finite numbers.' };
  }
  const minGapFrames = Math.max(1, Math.round(0.00025 * sampleRate));
  const maxGapFrames = Math.round(0.004 * sampleRate);
  const contextFrames = Math.round(0.048 * sampleRate);

  const startFrame = Math.round(startS * sampleRate);
  const repairedFrames = Math.round((gapMs / 1000) * sampleRate);
  const endFrame = startFrame + repairedFrames;

  if (repairedFrames < minGapFrames || repairedFrames > maxGapFrames) {
    return {
      ok: false,
      error: `Explicit loss interval must be between 0.25 ms (${minGapFrames} frames) and 4.00 ms (${maxGapFrames} frames) at ${sampleRate} Hz.`,
    };
  }
  if (startFrame < contextFrames) {
    const minStartS = contextFrames / sampleRate;
    return {
      ok: false,
      error: `At least 48 ms of preceding context is required (start >= ${minStartS.toFixed(3)} s).`,
    };
  }
  if (endFrame + contextFrames > totalSamples) {
    const maxStartS = Math.max(0, (totalSamples - contextFrames - repairedFrames) / sampleRate);
    return {
      ok: false,
      error: `At least 48 ms of trailing context is required (start <= ${maxStartS.toFixed(3)} s).`,
    };
  }
  return { ok: true, startFrame, endFrame, repairedFrames, contextFrames };
}

/**
 * Picks a guaranteed-valid default Local Gap Repair start time (in seconds) with >= 48 ms
 * leading and trailing context for the loaded session duration and optional time selection.
 */
export function defaultGapRepairStartS(
  durationS: number,
  selection?: [number, number] | null,
  gapMs = 2.0,
): number {
  if (!Number.isFinite(durationS) || durationS <= 0) {
    return 0.1;
  }
  const safeGapS = Math.max(
    0.00025,
    Math.min(0.004, Number.isFinite(gapMs) ? gapMs / 1000 : 0.002),
  );
  const marginS = 0.06; // slightly wider than the 48 ms Rust guard for safe rounding
  if (durationS <= marginS * 2 + safeGapS) {
    return Number(Math.max(0, (durationS - safeGapS) / 2).toFixed(3));
  }
  const maxStart = durationS - safeGapS - marginS;
  const preferred =
    selection && Number.isFinite(selection[0])
      ? selection[0]
      : Math.min(12, durationS * 0.4);
  const clamped = Math.max(marginS, Math.min(maxStart, preferred));
  return Number(clamped.toFixed(3));
}

export function countUncertainOverlaps(
  noteStart: number,
  noteEnd: number,
  frames: readonly Pick<FrameEvidence, 'time_s' | 'status'>[],
): number {
  let count = 0;
  for (const frame of frames) {
    if (frame.status === 'uncertain' && frame.time_s >= noteStart && frame.time_s <= noteEnd) {
      count += 1;
    }
  }
  return count;
}

export function computeEffectiveLayerGains(
  channels: readonly Pick<LayerChannel, 'id' | 'gain' | 'muted' | 'solo'>[],
  headroomGain = 0.707,
  includeTexture = true,
): Record<string, number> {
  const anySolo = channels.some((c) => c.solo);
  const out: Record<string, number> = {};
  for (const ch of channels) {
    if (!includeTexture && ch.id === 'texture') {
      out[ch.id] = 0;
      continue;
    }
    const active = anySolo ? ch.solo && !ch.muted : !ch.muted;
    const clamped = Math.max(0, Math.min(2.0, Number.isFinite(ch.gain) ? ch.gain : 1));
    out[ch.id] = active ? clamped * headroomGain : 0;
  }
  return out;
}

export function formatDbfs(value: number): string {
  if (!Number.isFinite(value)) return '—';
  return `${value.toFixed(2)} dBFS`;
}

export function formatHz(hz: number | null | undefined): string {
  if (hz == null || !Number.isFinite(hz) || hz <= 0) return 'Abstained';
  return `${hz.toFixed(1)} Hz`;
}

export function shortHash(hash: string): string {
  if (!hash || hash.length < 16) return hash;
  return `${hash.slice(0, 12)}…${hash.slice(-8)}`;
}

/**
 * Pure RIFF/WAVE mono slice extractor (PCM 16-bit or IEEE Float 32-bit -> Float32 mono WAV).
 * Used strictly for windowing a <= 30 s slice before passing to `spectracraft-mono analyze-layers`.
 * Never mutates the source buffer.
 */
export function sliceMonoWavBuffer(
  source: Uint8Array,
  startS: number,
  endS: number,
): {
  buffer: Uint8Array;
  sampleRate: number;
  samples: number;
  startS: number;
  endS: number;
  sourceEncoding: 'ieee_float_32' | 'pcm_8' | 'pcm_16' | 'pcm_24' | 'pcm_32';
  sourceBitsPerSample: number;
  convertedToFloat32: boolean;
} {
  const view = new DataView(source.buffer, source.byteOffset, source.byteLength);
  if (source.byteLength < 44) {
    throw new Error('WAV file is too short.');
  }
  const tag = (off: number) =>
    String.fromCharCode(source[off], source[off + 1], source[off + 2], source[off + 3]);
  if (tag(0) !== 'RIFF' || tag(8) !== 'WAVE') {
    throw new Error('Input is not a valid RIFF/WAVE file.');
  }

  let audioFormat = 0;
  let channels = 0;
  let sampleRate = 0;
  let bitsPerSample = 0;
  let dataOffset = -1;
  let dataSize = 0;

  let offset = 12;
  while (offset + 8 <= source.byteLength) {
    const id = tag(offset);
    const size = view.getUint32(offset + 4, true);
    const body = offset + 8;
    if (body + size > source.byteLength) {
      break;
    }
    if (id === 'fmt ') {
      const rawFmt = view.getUint16(body, true);
      channels = view.getUint16(body + 2, true);
      sampleRate = view.getUint32(body + 4, true);
      bitsPerSample = view.getUint16(body + 14, true);
      if (rawFmt === 0xfffe && size >= 40) {
        audioFormat = view.getUint16(body + 24, true);
      } else {
        audioFormat = rawFmt;
      }
    } else if (id === 'data') {
      dataOffset = body;
      dataSize = size;
      break;
    }
    offset = body + size + (size % 2);
  }

  if (dataOffset < 0 || channels !== 1 || sampleRate < 8000 || sampleRate > 96000) {
    throw new Error('Editable layers require a mono 8–96 kHz WAV file.');
  }
  const supported =
    (audioFormat === 3 && bitsPerSample === 32) ||
    (audioFormat === 1 &&
      (bitsPerSample === 8 || bitsPerSample === 16 || bitsPerSample === 24 || bitsPerSample === 32));
  if (!supported) {
    throw new Error(
      'Supported WAV formats for slicing are 8/16/24/32-bit PCM or 32-bit IEEE float.',
    );
  }

  const bytesPerSample = bitsPerSample / 8;
  const totalFrames = Math.floor(dataSize / bytesPerSample);
  const totalDur = totalFrames / sampleRate;
  const clampedStart = Math.max(0, Math.min(totalDur, startS));
  const clampedEnd = Math.max(clampedStart, Math.min(totalDur, Math.min(clampedStart + 29.5, endS)));
  const startFrame = Math.floor(clampedStart * sampleRate);
  const endFrame = Math.min(totalFrames, Math.ceil(clampedEnd * sampleRate));
  const outSamples = endFrame - startFrame;
  if (outSamples < sampleRate * 0.2) {
    throw new Error('Selected layer window must be at least 0.2 seconds.');
  }

  const outDataBytes = outSamples * 4;
  const out = new Uint8Array(44 + outDataBytes);
  const outView = new DataView(out.buffer);
  const writeAscii = (pos: number, text: string) => {
    for (let i = 0; i < text.length; i++) {
      out[pos + i] = text.charCodeAt(i);
    }
  };
  writeAscii(0, 'RIFF');
  outView.setUint32(4, 36 + outDataBytes, true);
  writeAscii(8, 'WAVE');
  writeAscii(12, 'fmt ');
  outView.setUint32(16, 16, true);
  outView.setUint16(20, 3, true); // IEEE float
  outView.setUint16(22, 1, true); // mono
  outView.setUint32(24, sampleRate, true);
  outView.setUint32(28, sampleRate * 4, true);
  outView.setUint16(32, 4, true);
  outView.setUint16(34, 32, true);
  writeAscii(36, 'data');
  outView.setUint32(40, outDataBytes, true);

  for (let i = 0; i < outSamples; i++) {
    const srcIdx = dataOffset + (startFrame + i) * bytesPerSample;
    let sample = 0;
    if (audioFormat === 3) {
      sample = view.getFloat32(srcIdx, true);
    } else if (bitsPerSample === 8) {
      // Standard RIFF/WAVE 8-bit PCM is unsigned [0..255] centered at 128
      sample = (source[srcIdx] - 128) / 128;
    } else if (bitsPerSample === 16) {
      sample = view.getInt16(srcIdx, true) / 32768;
    } else if (bitsPerSample === 24) {
      const b0 = source[srcIdx];
      const b1 = source[srcIdx + 1];
      const b2 = source[srcIdx + 2];
      let v = b0 | (b1 << 8) | (b2 << 16);
      if (v & 0x800000) v |= ~0xffffff;
      sample = v / 8388608;
    } else {
      // 32-bit signed integer PCM
      sample = view.getInt32(srcIdx, true) / 2147483648;
    }
    outView.setFloat32(44 + i * 4, sample, true);
  }

  const sourceEncoding: 'ieee_float_32' | 'pcm_8' | 'pcm_16' | 'pcm_24' | 'pcm_32' =
    audioFormat === 3
      ? 'ieee_float_32'
      : bitsPerSample === 8
        ? 'pcm_8'
        : bitsPerSample === 16
          ? 'pcm_16'
          : bitsPerSample === 24
            ? 'pcm_24'
            : 'pcm_32';
  return {
    buffer: out,
    sampleRate,
    samples: outSamples,
    startS: startFrame / sampleRate,
    endS: endFrame / sampleRate,
    sourceEncoding,
    sourceBitsPerSample: bitsPerSample,
    convertedToFloat32: audioFormat !== 3,
  };
}

/**
 * Explicit capability alignment across web upload, layer slicing, and native desktop workflows (Audit M13).
 */
export const WORKFLOW_CAPABILITIES = {
  upload: {
    channels: 'mono_only' as const,
    minSampleRateHz: 8000,
    maxSampleRateHz: 96000,
    maxDurationS: 90,
    supportedEncodings: ['pcm_8', 'pcm_16', 'pcm_24', 'pcm_32', 'ieee_float_32'] as const,
  },
  layerSlice: {
    channels: 'mono_only' as const,
    minSampleRateHz: 8000,
    maxSampleRateHz: 96000,
    minWindowS: 0.2,
    maxWindowS: 29.5,
    supportedEncodings: ['pcm_8', 'pcm_16', 'pcm_24', 'pcm_32', 'ieee_float_32'] as const,
  },
  nativeDesktop: {
    channels: 'mono_or_stereo_folded_with_provenance' as const,
    minSampleRateHz: 8000,
    maxSampleRateHz: 96000,
    maxExcerptRegenS: 30,
  },
} as const;

export type WavHeaderCheck =
  | {
      ok: true;
      sampleRate: number;
      bitsPerSample: number;
      audioFormat: number;
      samples: number;
      durationS: number;
    }
  | { ok: false; error: string };

/**
 * Validate mono RIFF/WAVE header bytes before writing an upload to disk or invoking Rust CLI.
 */
export function validateMonoWavHeader(
  source: Uint8Array,
  maxDurationS = 90.5,
): WavHeaderCheck {
  if (source.byteLength < 44) {
    return {
      ok: false,
      error: 'Upload a valid mono RIFF/WAVE file (8–96 kHz, up to 90 seconds).',
    };
  }
  const view = new DataView(source.buffer, source.byteOffset, source.byteLength);
  const tag = (off: number) =>
    String.fromCharCode(source[off], source[off + 1], source[off + 2], source[off + 3]);

  if (tag(0) !== 'RIFF' || tag(8) !== 'WAVE') {
    return {
      ok: false,
      error: 'Upload a valid mono RIFF/WAVE file (8–96 kHz, up to 90 seconds).',
    };
  }

  let foundFmt = false;
  let audioFormat = 0;
  let channels = 0;
  let sampleRate = 0;
  let bitsPerSample = 0;
  let dataSize = -1;

  let offset = 12;
  let chunks = 0;
  while (offset + 8 <= source.byteLength && chunks < 128) {
    chunks += 1;
    const id = tag(offset);
    const size = view.getUint32(offset + 4, true);
    const body = offset + 8;
    if (body + size > source.byteLength) {
      return { ok: false, error: 'WAV file appears truncated or has an invalid chunk length.' };
    }
    if (id === 'fmt ') {
      if (size < 16) {
        return { ok: false, error: 'Invalid WAV format chunk.' };
      }
      foundFmt = true;
      const rawFmt = view.getUint16(body, true);
      channels = view.getUint16(body + 2, true);
      sampleRate = view.getUint32(body + 4, true);
      bitsPerSample = view.getUint16(body + 14, true);
      if (rawFmt === 0xfffe && size >= 40) {
        audioFormat = view.getUint16(body + 24, true);
      } else {
        audioFormat = rawFmt;
      }
    } else if (id === 'data') {
      dataSize = size;
      break;
    }
    offset = body + size + (size % 2);
  }

  if (!foundFmt || dataSize <= 0) {
    return { ok: false, error: 'WAV file is missing a valid format or audio data chunk.' };
  }
  if (channels !== 1) {
    return { ok: false, error: 'SpectraCraft requires a single-source mono (1-channel) WAV file.' };
  }
  if (sampleRate < 8000 || sampleRate > 96000) {
    return { ok: false, error: 'WAV sample rate must be between 8 kHz and 96 kHz.' };
  }
  const supportedFormat =
    (audioFormat === 3 && bitsPerSample === 32) ||
    (audioFormat === 1 && (bitsPerSample === 8 || bitsPerSample === 16 || bitsPerSample === 24 || bitsPerSample === 32));
  if (!supportedFormat) {
    return {
      ok: false,
      error: 'Unsupported WAV encoding (expected PCM 8/16/24/32-bit or 32-bit IEEE float).',
    };
  }
  const bytesPerSample = bitsPerSample / 8;
  const samples = Math.floor(dataSize / bytesPerSample);
  if (samples <= 0) {
    return { ok: false, error: 'WAV file contains zero audio frames.' };
  }
  const durationS = samples / sampleRate;
  if (durationS > maxDurationS) {
    return { ok: false, error: 'WAV clip exceeds the 90-second single-source upload limit.' };
  }
  return { ok: true, sampleRate, bitsPerSample, audioFormat, samples, durationS };
}

/**
 * Redact internal filesystem paths and stack trace lines before returning an error message to clients.
 */
export function sanitizeClientErrorMessage(
  raw: string,
  sensitiveRoots: readonly string[] = [],
  fallback = 'Request failed.',
): string {
  const lines = raw
    .replace(/\r/g, '')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !/^at\s+/.test(line));
  let msg = lines[0] ?? '';
  if (!msg) return fallback;

  for (const root of sensitiveRoots) {
    if (root && root.length > 1) {
      msg = msg.split(root).join('<path>');
    }
  }
  msg = msg
    .replace(/\/(?:tmp|var|home|Users|root|app|opt|etc|usr|private)\/[^\s'"`),;:]+/g, '<path>')
    .replace(/[A-Za-z]:\\[^\s'"`),;:]+/g, '<path>')
    .replace(/Command failed:\s*[^\n]+/i, 'Audio engine command failed.')
    .replace(/\s+/g, ' ')
    .trim();

  if (!msg) return fallback;
  return msg.length > 240 ? `${msg.slice(0, 237)}...` : msg;
}
