//! Framework-neutral audio visualization math and bounded spectrogram tile cache.
//!
//! Extracted from the `egui` desktop frontend so both `spectracraft-desktop` and
//! `apps/spectracraft-gpui` share identical waveform min/max peak downsampling,
//! Hann-windowed dBFS spectrogram calibration, above-Nyquist marking, and bounded
//! LRU tile caching without pulling any GUI crate into `spectracraft-mono`.

use rustfft::{num_complex::Complex32, FftPlanner};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

pub const SPECTRUM_H: usize = 144;
pub const SPECTRUM_BOTTOM: f32 = -90.0;
pub const SPECTRUM_TOP: f32 = 0.0;

/// Downsampled `(min, max)` amplitude pairs for waveform rendering.
pub type Peaks = Vec<(f32, f32)>;

/// Compute min/max waveform envelope bins using the canonical 2048-bin cap.
pub fn measure_peaks(wave: &[f32]) -> Peaks {
    measure_peaks_bins(wave, 2048)
}

/// Compute min/max waveform envelope bins for up to `max_bins` horizontal columns.
pub fn measure_peaks_bins(wave: &[f32], max_bins: usize) -> Peaks {
    if wave.is_empty() || max_bins == 0 {
        return Vec::new();
    }
    let n = wave.len().min(max_bins);
    (0..n)
        .map(|i| {
            let a = i * wave.len() / n;
            let b = ((i + 1) * wave.len() / n).max(a + 1).min(wave.len());
            wave[a..b]
                .iter()
                .fold((0_f32, 0_f32), |(lo, hi), &v| (lo.min(v), hi.max(v)))
        })
        .collect()
}

/// Convert linear amplitude into dBFS with a `-200 dBFS` numeric floor.
pub fn to_dbfs(amplitude: f32) -> f32 {
    20.0 * amplitude.max(1e-10).log10()
}

/// Map a dBFS spectrogram value onto the SpectraCraft perceptual dark-mode RGB ramp.
/// Non-finite (`NaN`) cells mark frequency bands above Nyquist and render in neutral dark slate.
pub fn spectrum_color_rgb(dbfs: f32) -> [u8; 3] {
    if !dbfs.is_finite() {
        return [36, 36, 44];
    }
    const COLORS: [[f32; 3]; 5] = [
        [15., 20., 32.],
        [28., 66., 115.],
        [21., 136., 154.],
        [202., 172., 76.],
        [255., 248., 213.],
    ];
    let a = ((dbfs.clamp(SPECTRUM_BOTTOM, SPECTRUM_TOP) - SPECTRUM_BOTTOM)
        / (SPECTRUM_TOP - SPECTRUM_BOTTOM))
        * 4.0;
    let lo = (a.floor() as usize).min(3);
    let f = a - lo as f32;
    let c = |i| ((1.0 - f) * COLORS[lo][i] + f * COLORS[lo + 1][i]) as u8;
    [c(0), c(1), c(2)]
}

/// Precomputed overview spectrogram grid (row 0 = top / `max_hz`, row `SPECTRUM_H - 1` = DC).
#[derive(Clone, Debug)]
pub struct Spectrum {
    pub width: usize,
    pub max_hz: f32,
    pub nyquist_hz: f32,
    pub dbfs: Vec<f32>,
}

impl Spectrum {
    /// Measure a spectrogram using the default `min(12 kHz, Nyquist)` display ceiling.
    pub fn measure(wave: &[f32], rate: u32) -> Self {
        Self::measure_range(wave, rate, 12_000_f32.min(rate as f32 * 0.5))
    }

    /// Measure a calibrated Hann-windowed spectrogram up to `display_max_hz`.
    /// Rows whose lower frequency bound meets or exceeds Nyquist are explicitly marked `f32::NAN`.
    pub fn measure_range(wave: &[f32], rate: u32, display_max_hz: f32) -> Self {
        let nyquist_hz = rate as f32 * 0.5;
        let max_hz = display_max_hz.max(500.0);
        if wave.is_empty() || rate == 0 {
            return Self {
                width: 1,
                max_hz,
                nyquist_hz,
                dbfs: vec![SPECTRUM_BOTTOM; SPECTRUM_H],
            };
        }
        let width = wave
            .len()
            .div_ceil((rate as usize / 100).max(1))
            .clamp(1, 700);
        let n = (rate as usize / 24).next_power_of_two().max(64);
        let window: Vec<f32> = (0..n)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
            .collect();
        let norm = 2.0 / window.iter().sum::<f32>();
        let hz_to_bin = n as f32 / rate as f32;
        let bands: Vec<Option<std::ops::Range<usize>>> = (0..SPECTRUM_H)
            .map(|y| {
                let f_lo = (SPECTRUM_H - 1 - y) as f32 / SPECTRUM_H as f32 * max_hz;
                let f_hi = (SPECTRUM_H - y) as f32 / SPECTRUM_H as f32 * max_hz;
                if f_lo >= nyquist_hz {
                    return None; // Above Nyquist -> no data
                }
                let lo = (f_lo * hz_to_bin).ceil() as usize;
                let hi = (f_hi.min(nyquist_hz) * hz_to_bin).ceil() as usize;
                let start = lo.min(n / 2);
                Some(start..hi.max(start + 1).min(n / 2 + 1))
            })
            .collect();
        let mut dbfs = vec![SPECTRUM_BOTTOM; width * SPECTRUM_H];
        let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
        let mut block = vec![Complex32::default(); n];
        let mut scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
        for col in 0..width {
            let center = ((2 * col + 1) * wave.len()) / (2 * width);
            for (k, v) in block.iter_mut().enumerate() {
                let x = (center + k)
                    .checked_sub(n / 2)
                    .and_then(|i| wave.get(i))
                    .copied()
                    .unwrap_or(0.0);
                *v = Complex32::new(window[k] * x, 0.0);
            }
            fft.process_with_scratch(&mut block, &mut scratch);
            for (y, maybe_band) in bands.iter().enumerate() {
                if let Some(band) = maybe_band {
                    let peak = band
                        .clone()
                        .map(|i| {
                            let endpoint_scale = if i == 0 || i == n / 2 { 0.5 } else { 1.0 };
                            block[i].norm() * endpoint_scale
                        })
                        .fold(0_f32, f32::max);
                    dbfs[y * width + col] = to_dbfs(peak * norm);
                } else {
                    dbfs[y * width + col] = f32::NAN;
                }
            }
        }
        Self {
            width,
            max_hz,
            nyquist_hz,
            dbfs,
        }
    }

    /// Encode this spectrogram grid into packed RGBA8 bytes (`width * SPECTRUM_H * 4`).
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.dbfs.len() * 4);
        for &db in &self.dbfs {
            let [r, g, b] = spectrum_color_rgb(db);
            out.extend_from_slice(&[r, g, b, 255]);
        }
        out
    }
}

/// Combined peak envelope + calibrated spectrogram overview for an audio buffer.
#[derive(Clone, Debug)]
pub struct Visual {
    pub peaks: Peaks,
    pub spectrum: Spectrum,
}

impl Visual {
    pub fn measure(wave: &[f32], rate: u32) -> Self {
        Self::measure_range(wave, rate, 12_000_f32.min(rate as f32 * 0.5))
    }

    pub fn measure_range(wave: &[f32], rate: u32, max_hz: f32) -> Self {
        Self {
            peaks: measure_peaks(wave),
            spectrum: Spectrum::measure_range(wave, rate, max_hz),
        }
    }

    pub fn peak_dbfs(&self) -> String {
        let peak = self
            .peaks
            .iter()
            .fold(0_f32, |v, (lo, hi)| v.max(lo.abs()).max(hi.abs()));
        if peak == 0.0 {
            "silence".to_owned()
        } else {
            format!("{:+.1} dBFS peak", to_dbfs(peak))
        }
    }
}

/// Frequency-axis mapping for cached spectrogram tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrequencyScale {
    Linear,
    Log,
}

/// Complete cache identity for a rendered spectrogram tile.
/// Display-only playhead movement never changes this key, so panning/playback
/// redraws require zero FFT recomputation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpectrogramTileKey {
    pub artifact_id: u64,
    pub sample_rate: u32,
    pub channel: u16,
    pub frame_start: usize,
    pub frame_end: usize,
    pub fft_size: usize,
    pub width: u32,
    pub height: u32,
    pub max_hz_milli: u32,
    pub scale: FrequencyScale,
}

impl SpectrogramTileKey {
    pub fn max_hz(self) -> f32 {
        (self.max_hz_milli as f32 / 1000.0).max(500.0)
    }

    pub fn recommended_fft_size(sample_rate: u32) -> usize {
        (sample_rate as usize / 24)
            .next_power_of_two()
            .clamp(256, 8192)
    }
}

/// Immutable cached spectrogram tile ready for GPU upload or direct canvas painting.
#[derive(Clone, Debug)]
pub struct SpectrogramTile {
    pub key: SpectrogramTileKey,
    pub nyquist_hz: f32,
    pub dbfs: Arc<[f32]>,
    pub rgba: Arc<[u8]>,
}

impl SpectrogramTile {
    pub fn approx_bytes(&self) -> usize {
        self.dbfs.len() * std::mem::size_of::<f32>() + self.rgba.len()
    }
}

/// Compute a single spectrogram tile over `[key.frame_start, key.frame_end)` of `samples`.
/// Uses the same Hann-window amplitude normalization and above-Nyquist `NaN` contract as `Spectrum`.
pub fn compute_spectrogram_tile(samples: &[f32], key: SpectrogramTileKey) -> SpectrogramTile {
    let width = key.width.max(1) as usize;
    let height = key.height.max(1) as usize;
    let nyquist_hz = key.sample_rate as f32 * 0.5;
    let max_hz = key.max_hz();
    let start = key.frame_start.min(samples.len());
    let end = key.frame_end.clamp(start, samples.len());
    let slice_len = end.saturating_sub(start);

    if slice_len == 0 || key.sample_rate == 0 {
        let dbfs = vec![SPECTRUM_BOTTOM; width * height];
        let mut rgba = Vec::with_capacity(width * height * 4);
        let [r, g, b] = spectrum_color_rgb(SPECTRUM_BOTTOM);
        for _ in 0..(width * height) {
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
        return SpectrogramTile {
            key,
            nyquist_hz,
            dbfs: Arc::from(dbfs),
            rgba: Arc::from(rgba),
        };
    }

    let n = key.fft_size.next_power_of_two().clamp(64, 16_384);
    let window: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
        .collect();
    let norm = 2.0 / window.iter().sum::<f32>();
    let hz_to_bin = n as f32 / key.sample_rate as f32;
    let min_log_hz = 20.0_f32.min(max_hz * 0.5).max(1.0);

    let row_freq = |frac_from_bottom: f32| -> f32 {
        match key.scale {
            FrequencyScale::Linear => frac_from_bottom * max_hz,
            FrequencyScale::Log => {
                let ratio = (max_hz / min_log_hz).max(1.01);
                min_log_hz * ratio.powf(frac_from_bottom.clamp(0.0, 1.0))
            }
        }
    };

    let bands: Vec<Option<std::ops::Range<usize>>> = (0..height)
        .map(|y| {
            let f_lo = row_freq((height - 1 - y) as f32 / height as f32);
            let f_hi = row_freq((height - y) as f32 / height as f32);
            if f_lo >= nyquist_hz {
                return None;
            }
            let lo = (f_lo * hz_to_bin).ceil() as usize;
            let hi = (f_hi.min(nyquist_hz) * hz_to_bin).ceil() as usize;
            let b_start = lo.min(n / 2);
            Some(b_start..hi.max(b_start + 1).min(n / 2 + 1))
        })
        .collect();

    let mut dbfs = vec![SPECTRUM_BOTTOM; width * height];
    let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
    let mut block = vec![Complex32::default(); n];
    let mut scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];

    for col in 0..width {
        let center = start + ((2 * col + 1) * slice_len) / (2 * width);
        for (k, v) in block.iter_mut().enumerate() {
            let x = (center + k)
                .checked_sub(n / 2)
                .and_then(|i| samples.get(i))
                .copied()
                .unwrap_or(0.0);
            *v = Complex32::new(window[k] * x, 0.0);
        }
        fft.process_with_scratch(&mut block, &mut scratch);
        for (y, maybe_band) in bands.iter().enumerate() {
            if let Some(band) = maybe_band {
                let peak = band
                    .clone()
                    .map(|i| {
                        let endpoint_scale = if i == 0 || i == n / 2 { 0.5 } else { 1.0 };
                        block[i].norm() * endpoint_scale
                    })
                    .fold(0_f32, f32::max);
                dbfs[y * width + col] = to_dbfs(peak * norm);
            } else {
                dbfs[y * width + col] = f32::NAN;
            }
        }
    }

    let mut rgba = Vec::with_capacity(width * height * 4);
    for &db in &dbfs {
        let [r, g, b] = spectrum_color_rgb(db);
        rgba.extend_from_slice(&[r, g, b, 255]);
    }

    SpectrogramTile {
        key,
        nyquist_hz,
        dbfs: Arc::from(dbfs),
        rgba: Arc::from(rgba),
    }
}

/// Bounded LRU spectrogram tile cache with explicit tile-count and byte budgets.
pub struct SpectrogramTileCache {
    max_tiles: usize,
    max_bytes: usize,
    current_bytes: usize,
    entries: HashMap<SpectrogramTileKey, Arc<SpectrogramTile>>,
    order: VecDeque<SpectrogramTileKey>,
    fft_compute_count: u64,
    cache_hits: u64,
}

impl SpectrogramTileCache {
    /// Create a cache with explicit entry and byte limits (default GPUI budget: 64 tiles / 32 MiB).
    pub fn new(max_tiles: usize, max_bytes: usize) -> Self {
        Self {
            max_tiles: max_tiles.max(1),
            max_bytes: max_bytes.max(1024),
            current_bytes: 0,
            entries: HashMap::new(),
            order: VecDeque::new(),
            fft_compute_count: 0,
            cache_hits: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    pub fn fft_compute_count(&self) -> u64 {
        self.fft_compute_count
    }

    pub fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.current_bytes = 0;
    }

    /// Look up an already-computed tile without triggering any FFT work.
    pub fn get(&mut self, key: &SpectrogramTileKey) -> Option<Arc<SpectrogramTile>> {
        let tile = self.entries.get(key).cloned()?;
        self.touch(*key);
        self.cache_hits += 1;
        Some(tile)
    }

    /// Retrieve a cached tile or compute & cache it on miss.
    pub fn get_or_compute(
        &mut self,
        samples: &[f32],
        key: SpectrogramTileKey,
    ) -> Arc<SpectrogramTile> {
        if let Some(hit) = self.get(&key) {
            return hit;
        }
        let tile = Arc::new(compute_spectrogram_tile(samples, key));
        self.fft_compute_count += 1;
        self.insert(tile.clone());
        tile
    }

    /// Insert a precomputed tile (for background worker completions) while enforcing LRU budgets.
    pub fn insert(&mut self, tile: Arc<SpectrogramTile>) {
        let key = tile.key;
        if let Some(prev) = self.entries.remove(&key) {
            self.current_bytes = self.current_bytes.saturating_sub(prev.approx_bytes());
            self.order.retain(|k| k != &key);
        }
        let tile_bytes = tile.approx_bytes();
        self.entries.insert(key, tile);
        self.order.push_back(key);
        self.current_bytes = self.current_bytes.saturating_add(tile_bytes);
        self.evict_to_budget();
    }

    fn touch(&mut self, key: SpectrogramTileKey) {
        if self.order.back() == Some(&key) {
            return;
        }
        self.order.retain(|k| k != &key);
        self.order.push_back(key);
    }

    fn evict_to_budget(&mut self) {
        while (self.entries.len() > self.max_tiles || self.current_bytes > self.max_bytes)
            && self.entries.len() > 1
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                self.current_bytes = self.current_bytes.saturating_sub(removed.approx_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn sine_wave(rate: u32, freq_hz: f32, amp: f32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs).round() as usize;
        (0..n)
            .map(|i| amp * (TAU * freq_hz * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn peak_dbfs_and_half_amplitude_scale_accurately() {
        let full = sine_wave(48_000, 1000.0, 1.0, 0.25);
        let half = sine_wave(48_000, 1000.0, 0.5, 0.25);
        let v_full = Visual::measure(&full, 48_000);
        let v_half = Visual::measure(&half, 48_000);

        let max_db = |s: &Spectrum| {
            s.dbfs
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .fold(f32::NEG_INFINITY, f32::max)
        };
        let db_full = max_db(&v_full.spectrum);
        let db_half = max_db(&v_half.spectrum);
        assert!(
            db_full.abs() < 0.75,
            "full-scale 1 kHz sine should read near 0 dBFS, got {db_full}"
        );
        let diff = db_full - db_half;
        assert!(
            (diff - 6.0206).abs() < 0.15,
            "half amplitude must drop ~6.02 dB, got {diff}"
        );
    }

    #[test]
    fn impulses_and_tones_hold_calibration_across_sample_rates() {
        for &rate in &[8_000_u32, 44_100, 48_000, 96_000] {
            let tone_hz = if rate == 8_000 { 1_000.0 } else { 2_000.0 };
            let wave = sine_wave(rate, tone_hz, 0.5, 0.2);
            let spec = Spectrum::measure_range(&wave, rate, 12_000.0);
            let peaks = measure_peaks(&wave);
            assert!(!peaks.is_empty());

            // For 8 kHz (Nyquist 4 kHz), rows above 4 kHz up to 12 kHz MUST be marked NaN (unavailable).
            if rate == 8_000 {
                let top_row = &spec.dbfs[0..spec.width];
                assert!(
                    top_row.iter().all(|v| v.is_nan()),
                    "above-Nyquist rows at 8 kHz must be marked NaN"
                );
                assert_eq!(spectrum_color_rgb(f32::NAN), [36, 36, 44]);
            }

            // Locate expected row for tone_hz in [0, 12_000]
            let expected_y = ((1.0 - tone_hz / 12_000.0) * SPECTRUM_H as f32) as usize;
            let mid_col = spec.width / 2;
            let mut best_y = 0;
            let mut best_db = f32::NEG_INFINITY;
            for y in 0..SPECTRUM_H {
                let v = spec.dbfs[y * spec.width + mid_col];
                if v.is_finite() && v > best_db {
                    best_db = v;
                    best_y = y;
                }
            }
            assert!(
                best_y.abs_diff(expected_y) <= 2,
                "rate {rate}: expected peak near row {expected_y}, got {best_y} ({best_db:.2} dBFS)"
            );
            assert!(
                (best_db - (-6.02)).abs() < 1.0,
                "rate {rate}: expected ~-6.02 dBFS for 0.5 amp sine, got {best_db:.2}"
            );
        }

        // Single impulse preserves peak visibility across downsampling.
        let mut imp = vec![0.0_f32; 48_000];
        imp[12_345] = 0.9;
        let peaks = measure_peaks(&imp);
        let max_peak = peaks.iter().map(|&(_, hi)| hi).fold(0.0_f32, f32::max);
        assert!((max_peak - 0.9).abs() < 1e-6);
    }

    #[test]
    fn tile_cache_does_zero_ffts_on_playhead_redraw_and_enforces_lru_eviction() {
        let wave = sine_wave(48_000, 880.0, 0.5, 0.5);
        let key_a = SpectrogramTileKey {
            artifact_id: 1,
            sample_rate: 48_000,
            channel: 0,
            frame_start: 0,
            frame_end: wave.len(),
            fft_size: 2048,
            width: 128,
            height: 64,
            max_hz_milli: 12_000_000,
            scale: FrequencyScale::Linear,
        };
        let mut cache = SpectrogramTileCache::new(2, 512 * 1024);
        let tile1 = cache.get_or_compute(&wave, key_a);
        assert_eq!(cache.fft_compute_count(), 1);
        assert_eq!(tile1.rgba.len(), 128 * 64 * 4);

        // Simulate 100 playhead-only redraws: same tile key -> 0 additional FFTs.
        for _ in 0..100 {
            let hit = cache.get_or_compute(&wave, key_a);
            assert!(Arc::ptr_eq(&tile1, &hit));
        }
        assert_eq!(cache.fft_compute_count(), 1);
        assert_eq!(cache.cache_hits(), 100);

        // Insert 3 distinct tiles into max_tiles=2 cache -> LRU evicts oldest, keeping <= 2 tiles.
        let mut key_b = key_a;
        key_b.artifact_id = 2;
        let mut key_c = key_a;
        key_c.artifact_id = 3;
        cache.get_or_compute(&wave, key_b);
        cache.get_or_compute(&wave, key_c);
        assert_eq!(cache.len(), 2);
        assert!(cache.current_bytes() <= 512 * 1024);
        assert!(cache.get(&key_a).is_none(), "oldest tile should be evicted");
        assert!(cache.get(&key_b).is_some());
        assert!(cache.get(&key_c).is_some());
    }
}
