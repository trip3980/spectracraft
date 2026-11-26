export type EvidenceTag = 'measured' | 'inferred';

export interface ReportFinding {
  category: string;
  label: string;
  value: string;
  tag: EvidenceTag;
  detail: string;
}

export interface SpectralBalance {
  centroid_hz: number;
  tilt_db_per_oct: number;
  low_band_dbfs: number;
  mid_band_dbfs: number;
  presence_band_dbfs: number;
  air_band_dbfs: number;
}

export interface HarmonicSignature {
  fundamental_range_hz: [number, number];
  median_f0_hz: number;
  harmonic_to_noise_ratio_db: number;
  odd_even_harmonic_ratio_db: number;
  mean_attack_sharpness: number;
  mean_decay_rate_db_per_s: number;
  spectral_rolloff_95_hz: number;
  harmonic_richness_partials: number;
  voiced_frame_fraction: number;
  harmonic_profile_summary: string;
}

export interface InstrumentSignature {
  inferred_family: string;
  display_name: string;
  confidence: number;
  detected_foreign_signatures: string[];
}

export interface SpatialSignature {
  direct_to_reverberant_ratio_db: number;
  early_reflection_delay_ms: number | null;
  estimated_mic_distance_ft: number | null;
  estimated_source_height_ft: number | null;
  spatial_assessment: string;
}

export interface RoomSignature {
  estimated_rt60_s: number | null;
  hf_damping_ratio_db: number;
  early_reflection_energy_db: number;
  inferred_room_type: string;
  inferred_material_class: string;
}

export interface MicSignature {
  sub_55hz_rolloff_db: number;
  low_mid_warmth_200hz_db: number;
  presence_3_to_6khz_db: number;
  hf_rolloff_above_10khz_db: number;
  effective_bandwidth_hz: number;
  sm7_signature_score: number;
  matches_sm7_class: boolean;
  inferred_mic_model: string;
}

export interface BlindSignatureProfile {
  instrument: InstrumentSignature;
  harmonic: HarmonicSignature;
  spatial: SpatialSignature;
  room: RoomSignature;
  mic: MicSignature;
}

export interface BeforeReport {
  blind_scope: string;
  sample_rate: number;
  samples: number;
  duration_s: number;
  rms_dbfs: number;
  sample_peak_dbfs: number;
  true_peak_dbtp: number;
  integrated_lufs: number;
  noise_floor_dbfs: number;
  estimated_snr_db: number;
  dynamic_range_db: number;
  crest_factor_db: number;
  dc_offset: number;
  dc_offset_dbfs: number;
  dc_present: boolean;
  clipped_samples: number;
  clipping_present: boolean;
  hum_50hz_db: number;
  hum_60hz_db: number;
  detected_mains_hum_hz: number | null;
  stationary_noise_ratio: number;
  spectral_balance: SpectralBalance;
  third_octave_dbfs: number[];
  body_100hz_prominence_db: number;
  body_200hz_prominence_db: number;
  upper_band_shimmer_index: number;
  upper_band_phase_coherence: number;
  dropout_count: number;
  click_count: number;
  estimated_rt60_s: number | null;
  mic_room_signature: string;
  signatures?: BlindSignatureProfile;
  parseval_verified: boolean;
  parseval_max_rel_err: number;
  findings: ReportFinding[];
}

export interface AfterReport {
  blind_scope: string;
  sample_rate: number;
  samples: number;
  duration_s: number;
  rms_dbfs: number;
  sample_peak_dbfs: number;
  true_peak_dbtp: number;
  integrated_lufs: number;
  residual_noise_dbfs: number;
  estimated_snr_db: number;
  dynamic_range_db: number;
  crest_factor_db: number;
  dc_offset: number;
  dc_offset_dbfs: number;
  dc_present: boolean;
  clipped_samples: number;
  clipping_present: boolean;
  hum_50hz_db: number;
  hum_60hz_db: number;
  detected_mains_hum_hz: number | null;
  stationary_noise_ratio: number;
  spectral_balance: SpectralBalance;
  third_octave_dbfs: number[];
  body_100hz_prominence_db: number;
  body_200hz_prominence_db: number;
  body_resonance_visible: boolean;
  upper_band_shimmer_index: number;
  upper_band_phase_coherence: number;
  dropout_count: number;
  click_count: number;
  room_tail_rt60_s: number | null;
  tail_decay_dbfs: number;
  defects_summary: string;
  signatures?: BlindSignatureProfile;
  parseval_verified: boolean;
  parseval_max_rel_err: number;
  findings: ReportFinding[];
}

export interface DeltaItem {
  metric: string;
  summary: string;
  before_value: string;
  after_value: string;
  delta_numeric: number | null;
  unit: string;
}

export interface ComparisonSummary {
  generated_after_both_blind_reports: boolean;
  deltas: DeltaItem[];
  body_resonance_peaks_noted: boolean;
  no_unexpected_dropouts_or_clicks: boolean;
}

export interface ThirdOctavePlot {
  centers_hz: number[];
  before_db: number[];
  after_db: number[];
  delta_db: number[];
  db_range: [number, number];
}

export interface UpperBandDiagnosticPlot {
  band_hz: [number, number];
  times_s: number[];
  before_phase_coherence: number[];
  after_phase_coherence: number[];
  before_modulation_depth: number[];
  after_modulation_depth: number[];
}

export interface ReportViewerPlots {
  sample_rate: number;
  duration_s: number;
  max_hz: number;
  waveform_scale_linear: number;
  waveform_bins: number;
  waveform_before: [number, number][];
  waveform_after: [number, number][];
  residual_waveform: [number, number][];
  spectrogram_cols: number;
  spectrogram_rows: number;
  spectrogram_freq_hz: number[];
  spectrogram_db_range: [number, number];
  diff_db_range: [number, number];
  spectrogram_before_db: number[];
  spectrogram_after_db: number[];
  spectrogram_diff_db: number[];
  third_octave: ThirdOctavePlot;
  upper_band: UpperBandDiagnosticPlot;
}

export interface CraftLibsIntegration {
  crates_used: string[];
  unavailable_fallbacks: string[];
}

export interface TechnicalReport {
  schema_version: number;
  title: string;
  pipeline_label: string;
  before: BeforeReport;
  after: AfterReport;
  comparison: ComparisonSummary;
  plots: ReportViewerPlots;
  whitepaper_markdown: string;
  craft_libs: CraftLibsIntegration;
}

export interface TechnicalReportApiResponse {
  reportId: string;
  report: TechnicalReport;
}

export interface ReportExportReceipt {
  requested_format: string;
  actual_format: string;
  extension: string;
  content_type: string;
  engine: string;
  fallback_used: boolean;
  fallback_note: string | null;
  bytes: number;
  sha256: string;
}

export interface ReportExportResponse {
  url: string;
  downloadUrl: string;
  filename: string;
  receipt: ReportExportReceipt;
}

/** High-contrast dark-mode magma colormap matching the Rust PDF renderer. */
export function magmaRgb(uRaw: number): [number, number, number] {
  const u = Math.max(0, Math.min(1, Number.isFinite(uRaw) ? uRaw : 0));
  let r: number;
  let g: number;
  let b: number;
  if (u < 0.25) {
    const t = u / 0.25;
    r = 14 + 38 * t;
    g = 17 + 20 * t;
    b = 28 + 96 * t;
  } else if (u < 0.55) {
    const t = (u - 0.25) / 0.3;
    r = 52 + 138 * t;
    g = 37 + 32 * t;
    b = 124 + 44 * t;
  } else if (u < 0.82) {
    const t = (u - 0.55) / 0.27;
    r = 190 + 60 * t;
    g = 69 + 125 * t;
    b = 168 - 110 * t;
  } else {
    const t = (u - 0.82) / 0.18;
    r = 250 + 5 * t;
    g = 194 + 58 * t;
    b = 58 + 180 * t;
  }
  return [Math.round(r), Math.round(g), Math.round(b)];
}

/** Diverging signed colormap for difference spectrograms (amber = boost, cyan = cut). */
export function divergingDiffRgb(diffDb: number, maxAbsDb: number): [number, number, number] {
  const span = Math.max(1, Number.isFinite(maxAbsDb) ? maxAbsDb : 24);
  const u = Math.max(-1, Math.min(1, (Number.isFinite(diffDb) ? diffDb : 0) / span));
  if (u >= 0) {
    return [
      Math.round(24 + 230 * u),
      Math.round(28 + 152 * u),
      Math.round(36 + 14 * u),
    ];
  }
  const t = -u;
  return [
    Math.round(24 + 18 * t),
    Math.round(28 + 178 * t),
    Math.round(36 + 214 * t),
  ];
}

export function formatFreqLabel(hz: number): string {
  if (!Number.isFinite(hz) || hz <= 0) return '0 Hz';
  if (hz >= 1000) {
    const khz = hz / 1000;
    return Number.isInteger(khz) ? `${khz}k` : `${khz.toFixed(1)}k`;
  }
  return `${Math.round(hz)}`;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B';
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export function validatePlotDimensions(plots: ReportViewerPlots): boolean {
  const expectedSpec = plots.spectrogram_cols * plots.spectrogram_rows;
  return (
    plots.waveform_bins > 0 &&
    plots.waveform_before.length === plots.waveform_bins &&
    plots.waveform_after.length === plots.waveform_bins &&
    plots.spectrogram_cols > 0 &&
    plots.spectrogram_rows > 0 &&
    plots.spectrogram_before_db.length === expectedSpec &&
    plots.spectrogram_after_db.length === expectedSpec &&
    plots.spectrogram_diff_db.length === expectedSpec &&
    plots.waveform_scale_linear > 0
  );
}

export interface SpectrogramProbe {
  timeS: number;
  freqHz: number;
  valueDb: number;
  col: number;
  row: number;
}

/**
 * Sample a real spectrogram buffer at normalized plot coordinates `(uX, uY)`
 * where `uX in [0, 1]` runs left-to-right in time and `uY in [0, 1]` runs
 * bottom-to-top in frequency. Returns `null` when out of bounds or empty.
 */
export function sampleSpectrogramProbe(params: {
  uX: number;
  uY: number;
  grid: number[];
  cols: number;
  rows: number;
  durationS: number;
  maxHz: number;
  freqHz?: number[];
}): SpectrogramProbe | null {
  const { uX, uY, grid, cols, rows, durationS, maxHz, freqHz } = params;
  if (
    !Number.isFinite(uX) ||
    !Number.isFinite(uY) ||
    uX < 0 ||
    uX > 1 ||
    uY < 0 ||
    uY > 1 ||
    cols <= 0 ||
    rows <= 0 ||
    grid.length < cols * rows
  ) {
    return null;
  }
  const col = Math.min(cols - 1, Math.max(0, Math.floor(uX * cols)));
  const row = Math.min(rows - 1, Math.max(0, Math.floor(uY * rows)));
  const valueDb = grid[row * cols + col];
  if (!Number.isFinite(valueDb)) return null;
  const timeS = uX * Math.max(0, durationS);
  const logLo = Math.log(20);
  const logHi = Math.log(Math.max(40, maxHz));
  const hz =
    freqHz && row < freqHz.length && Number.isFinite(freqHz[row])
      ? freqHz[row]
      : Math.exp(logLo + uY * (logHi - logLo));
  return { timeS, freqHz: hz, valueDb, col, row };
}

